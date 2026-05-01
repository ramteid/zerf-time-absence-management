//! Shared test infrastructure for KitaZeit integration tests.
//!
//! Provides [`TestApp`] which spins up an ephemeral Postgres database,
//! runs migrations, seeds initial data, and starts the Axum server on a
//! random port.  Each test session gets a fully isolated database.

use kitazeit::{build_app, categories, config::Config, db, holidays, seed_admin, AppState};
use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::sync::Arc;

/// A running test application with its own database and HTTP client.
pub struct TestApp {
    pub base_url: String,
    pub admin_password: String,
    /// The unique test database name (for cleanup).
    db_name: String,
    /// Connection to the *admin* database (for DROP on cleanup).
    admin_pool: sqlx::PgPool,
}

/// A cookie-jar-equipped HTTP client that targets a specific [`TestApp`].
/// Each `TestClient` maintains its own session (like a separate browser).
pub struct TestClient {
    client: Client,
    base_url: String,
}

impl TestApp {
    /// Boot a fully isolated test application.
    ///
    /// Requires `DATABASE_URL` env var pointing to a Postgres instance
    /// (e.g. `postgres://user:pass@localhost/postgres`).  A unique
    /// database is created for every call.
    pub async fn spawn() -> Self {
        let base_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for integration tests");

        // Connect to the default database to create a unique test DB.
        let admin_pool = sqlx::PgPool::connect(&base_url)
            .await
            .expect("cannot connect to admin database");

        let db_name = format!("kitazeit_test_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE DATABASE \"{}\"", db_name))
            .execute(&admin_pool)
            .await
            .expect("failed to create test database");

        // Build a connection URL for the test database.
        let test_db_url = if base_url.contains('?') {
            base_url.replace(&extract_dbname(&base_url), &db_name)
        } else {
            let trimmed = base_url.trim_end_matches('/');
            let prefix = &trimmed[..trimmed.rfind('/').unwrap() + 1];
            format!("{}{}", prefix, db_name)
        };

        let cfg = Config {
            database_url: test_db_url.clone(),
            session_secret: "integration-test-secret-do-not-use-in-prod-32-characters".into(),
            admin_email: "admin@example.com".into(),
            organization_name: "Integration Test".into(),
            region: "BW".into(),
            bind: "127.0.0.1:0".into(),
            static_dir: "static".into(),
            public_url: None,
            allowed_origins: vec![],
            secure_cookies: false,
            enforce_origin: false,
            enforce_csrf: false,
            trust_proxy: false,
        };

        let pool = db::init(&cfg).await.expect("failed to init test database");
        categories::ensure_initial(&pool)
            .await
            .expect("failed to seed categories");
        let year = chrono::Local::now().year_ce().1 as i32;
        holidays::ensure_holidays(&pool, year)
            .await
            .expect("failed to seed holidays");
        holidays::ensure_holidays(&pool, year + 1)
            .await
            .expect("failed to seed holidays+1");

        let admin_password = seed_admin(&pool, &cfg.admin_email)
            .await
            .expect("failed to seed admin")
            .expect("admin should have been created");

        let state = AppState {
            pool: pool.clone(),
            cfg: Arc::new(cfg),
        };

        let app = build_app(state);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind test listener");
        let addr = listener.local_addr().unwrap();
        let server_url = format!("http://{}", addr);

        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });

        // Wait for the server to be ready.
        let client = reqwest::Client::new();
        for _ in 0..50 {
            if client
                .get(format!("{}/healthz", server_url))
                .send()
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        Self {
            base_url: server_url,
            admin_password,
            db_name,
            admin_pool,
        }
    }

    /// Create a new [`TestClient`] with its own cookie jar (= fresh session).
    pub fn client(&self) -> TestClient {
        TestClient::new(&self.base_url)
    }

    /// Cleanup: drop the test database.
    pub async fn cleanup(self) {
        // Terminate connections then drop the database.
        let _ = sqlx::query(&format!(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{}'",
            self.db_name
        ))
        .execute(&self.admin_pool)
        .await;
        let _ = sqlx::query(&format!("DROP DATABASE IF EXISTS \"{}\"", self.db_name))
            .execute(&self.admin_pool)
            .await;
    }
}

use chrono::Datelike;

impl TestClient {
    fn new(base_url: &str) -> Self {
        let client = Client::builder()
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("failed to build reqwest client");
        Self {
            client,
            base_url: base_url.to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// GET request, returns (status, body as Value).
    pub async fn get(&self, path: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .get(self.url(path))
            .send()
            .await
            .expect("GET failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, body)
    }

    /// GET request, returns (status, raw body string).
    pub async fn get_raw(&self, path: &str) -> (StatusCode, String) {
        let resp = self
            .client
            .get(self.url(path))
            .send()
            .await
            .expect("GET failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        (status, text)
    }

    /// POST with JSON body.
    pub async fn post(&self, path: &str, json: &Value) -> (StatusCode, Value) {
        let resp = self
            .client
            .post(self.url(path))
            .json(json)
            .send()
            .await
            .expect("POST failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, body)
    }

    /// POST with raw string body (for malformed JSON tests).
    pub async fn post_raw(&self, path: &str, body: &str) -> (StatusCode, String) {
        let resp = self
            .client
            .post(self.url(path))
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .expect("POST raw failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        (status, text)
    }

    /// PUT with JSON body.
    pub async fn put(&self, path: &str, json: &Value) -> (StatusCode, Value) {
        let resp = self
            .client
            .put(self.url(path))
            .json(json)
            .send()
            .await
            .expect("PUT failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, body)
    }

    /// DELETE request.
    pub async fn delete(&self, path: &str) -> (StatusCode, Value) {
        let resp = self
            .client
            .delete(self.url(path))
            .send()
            .await
            .expect("DELETE failed");
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let body = serde_json::from_str(&text).unwrap_or(Value::String(text));
        (status, body)
    }

    // ----- Convenience helpers -----

    /// Login and return the response body.
    pub async fn login(&self, email: &str, password: &str) -> (StatusCode, Value) {
        self.post(
            "/api/v1/auth/login",
            &serde_json::json!({"email": email, "password": password}),
        )
        .await
    }

    /// Change password.
    pub async fn change_password(&self, current: &str, new: &str) -> (StatusCode, Value) {
        self.put(
            "/api/v1/auth/password",
            &serde_json::json!({"current_password": current, "new_password": new}),
        )
        .await
    }
}

/// Extract the database name from a Postgres URL.
fn extract_dbname(url: &str) -> String {
    // postgres://user:pass@host:port/dbname?params
    let without_params = url.split('?').next().unwrap();
    let dbname = without_params.rsplit('/').next().unwrap();
    dbname.to_string()
}

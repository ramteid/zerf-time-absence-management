//! Shared test infrastructure for Zerf integration tests.
//!
//! Provides [`TestApp`] which spins up an ephemeral Postgres container via
//! testcontainers, runs migrations, seeds initial data, and starts the Axum
//! server on a random port. Each test session gets a fully isolated database.

use reqwest::{Client, StatusCode};
use serde_json::Value;
use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use zerf::{build_app, categories, config::Config, db, holidays, seed_admin, AppState};

/// A running test application with its own database and HTTP client.
pub struct TestApp {
    pub base_url: String,
    pub admin_password: String,
    /// Keep the container alive for the duration of the test.
    _container: ContainerAsync<Postgres>,
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
    /// Starts a Postgres container via testcontainers, creates the schema,
    /// seeds initial data, and starts the Axum server on a random port.
    pub async fn spawn() -> Self {
        let container = Postgres::default()
            .start()
            .await
            .expect("failed to start Postgres container");

        let host_port = container
            .get_host_port_ipv4(5432)
            .await
            .expect("failed to get container port");

        let database_url = format!(
            "postgres://postgres:postgres@127.0.0.1:{}/postgres",
            host_port
        );

        let cfg = Config {
            database_url: database_url.clone(),
            session_secret: "integration-test-secret-do-not-use-in-prod-32-characters".into(),
            admin_email: "admin@example.com".into(),
            bind: "127.0.0.1:0".into(),
            static_dir: "static".into(),
            public_url: None,
            allowed_origins: vec![],
            secure_cookies: false,
            enforce_origin: false,
            enforce_csrf: false,
            trust_proxy: false,
            smtp: None,
        };

        let pool = db::init(&cfg).await.expect("failed to init test database");
        categories::ensure_initial(&pool)
            .await
            .expect("failed to seed categories");
        // Seed country/region so that ensure_holidays can fetch from the API.
        // A fresh database has no app_settings rows for country or region.
        sqlx::query(
            "INSERT INTO app_settings(key, value) \
             VALUES ('country', 'DE'), ('region', 'DE-BW') \
             ON CONFLICT (key) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("failed to seed country settings");
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
            notifications: zerf::notifications::broadcaster(),
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
            _container: container,
        }
    }

    /// Create a new [`TestClient`] with its own cookie jar (= fresh session).
    pub fn client(&self) -> TestClient {
        TestClient::new(&self.base_url)
    }

    /// Cleanup: container is dropped automatically when TestApp is dropped.
    pub async fn cleanup(self) {
        // Container is dropped when `self` goes out of scope, which stops
        // and removes the Postgres container automatically.
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

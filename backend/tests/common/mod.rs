//! Shared test infrastructure for Zerf integration tests.
//!
//! Provides [`TestApp`] which spins up an ephemeral Postgres container via
//! testcontainers, runs migrations, seeds initial data, and starts the Axum
//! server on a random port. Each test session gets a fully isolated database.

use reqwest::{Client, StatusCode};
use serde_json::Value;
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use zerf::{auth, build_app, categories, config::Config, db, holidays, users, AppState};

static TEST_DB_COUNTER: AtomicU64 = AtomicU64::new(0);
static TEST_MIGRATOR: Migrator = sqlx::migrate!("./migrations");

async fn create_isolated_database(admin_database_url: &str) -> anyhow::Result<String> {
    let db_name = format!(
        "zerf_test_{}_{}",
        std::process::id(),
        TEST_DB_COUNTER.fetch_add(1, Ordering::Relaxed),
    );
    let admin_pool = sqlx::PgPool::connect(admin_database_url).await?;
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&admin_pool)
        .await?;
    Ok(db_name)
}

async fn init_test_database(database_url: &str) -> anyhow::Result<db::DatabasePool> {
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .test_before_acquire(true)
        .connect(database_url)
        .await?;

    // sqlx migrations are expected to be serialized, but in CI and highly parallel
    // local runs we occasionally observe a duplicate insert into _sqlx_migrations.
    // Retry a couple of times so transient migration-table races don't fail tests.
    let mut last_err: Option<anyhow::Error> = None;
    for _ in 0..3 {
        match TEST_MIGRATOR.run(&pool).await {
            Ok(_) => return Ok(pool),
            Err(err) => {
                let msg = err.to_string();
                if msg.contains("_sqlx_migrations_pkey")
                    || msg.contains("duplicate key value violates unique constraint")
                {
                    last_err = Some(err.into());
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
                return Err(err.into());
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("failed to run migrations")))
}

/// Seed a test admin user with a CSPRNG-generated password.
/// Only used in test code — never compiled into the production binary.
async fn seed_admin(pool: &db::DatabasePool, admin_email: &str) -> anyhow::Result<Option<String>> {
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role='admin'")
        .fetch_one(pool)
        .await?;
    if admin_count == 0 {
        let temp = users::generate_password();
        let hash = auth::hash_password(&temp)?;
        let ref_date = reference_date();
        sqlx::query("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,start_date,must_change_password,overtime_start_balance_min) VALUES ($1,$2,$3,$4,'admin',39.0,$5,TRUE,0)")
            .bind(admin_email.to_lowercase()).bind(hash).bind("Test").bind("Admin").bind(ref_date)
            .execute(pool).await?;

        let admin_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE email=$1")
            .bind(admin_email.to_lowercase())
            .fetch_one(pool)
            .await?;
        let current_year = ref_date.year();
        users::set_leave_days(pool, admin_id, current_year, 30).await?;
        users::set_leave_days(pool, admin_id, current_year + 1, 30).await?;

        Ok(Some(temp))
    } else {
        Ok(None)
    }
}

/// A running test application with its own database and HTTP client.
pub struct TestApp {
    pub base_url: String,
    pub admin_password: String,
    pub state: AppState,
    /// Keep the container alive for the duration of the test (None when TEST_DATABASE_URL is set).
    _container: Option<ContainerAsync<Postgres>>,
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
        Self::spawn_inner(None).await
    }

    /// Like [`spawn`] but with a `public_url` set in the config.
    /// Used to test that the app URL is appended to email bodies but not to
    /// in-app notification bodies.
    pub async fn spawn_with_public_url(public_url: &str) -> Self {
        Self::spawn_inner(Some(public_url.to_string())).await
    }

    async fn spawn_inner(public_url: Option<String>) -> Self {
        let (admin_database_url, database_url_base, _container) =
            if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
                // No container runtime — use a pre-existing local Postgres instance.
                let base = url.rsplitn(2, '/').nth(1).unwrap_or(&url).to_string();
                (url, base, None)
            } else {
                let container = Postgres::default()
                    .start()
                    .await
                    .expect("failed to start Postgres container");
                let host_port = container
                    .get_host_port_ipv4(5432)
                    .await
                    .expect("failed to get container port");
                let admin_url = format!(
                    "postgres://postgres:postgres@127.0.0.1:{}/postgres",
                    host_port
                );
                let base = format!("postgres://postgres:postgres@127.0.0.1:{}", host_port);
                (admin_url, base, Some(container))
            };

        let database_name = create_isolated_database(&admin_database_url)
            .await
            .expect("failed to create isolated test database");
        let database_url = format!("{}/{}", database_url_base, database_name);

        let cfg = Config {
            database_url: database_url.clone(),
            session_secret: "integration-test-secret-do-not-use-in-prod-32-characters".into(),
            git_commit: "test".into(),
            bind: "127.0.0.1:0".into(),
            static_dir: "static".into(),
            public_url,
            allowed_origins: vec![],
            secure_cookies: false,
            enforce_origin: false,
            enforce_csrf: false,
            trust_proxy: false,
        };

        let pool = init_test_database(&cfg.database_url)
            .await
            .expect("failed to init test database");
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
        let year = reference_date().year();
        holidays::ensure_holidays(&pool, year)
            .await
            .expect("failed to seed holidays");
        holidays::ensure_holidays(&pool, year + 1)
            .await
            .expect("failed to seed holidays+1");

        let admin_password = seed_admin(&pool, "admin@example.com")
            .await
            .expect("failed to seed admin")
            .expect("admin should have been created");

        let broadcaster = zerf::notifications::broadcaster();
        let db = zerf::repository::Db::new(pool.clone(), broadcaster.clone());
        let state = AppState {
            pool: pool.clone(),
            db,
            cfg: Arc::new(cfg),
            notifications: broadcaster,
        };

        let app = build_app(state.clone());

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
            state,
            _container,
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

/// Returns the reference date used by all test date helpers.
/// Reads TEST_REFERENCE_DATE (YYYY-MM-DD) when set, otherwise today.
fn reference_date() -> chrono::NaiveDate {
    if let Ok(s) = std::env::var("TEST_REFERENCE_DATE") {
        chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
            .expect("TEST_REFERENCE_DATE must be YYYY-MM-DD")
    } else {
        chrono::Local::now().date_naive()
    }
}

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

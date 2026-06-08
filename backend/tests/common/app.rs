use std::sync::Arc;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use chrono::Datelike;
use zerf::{build_app, config::Config, AppState};
use zerf::services::{categories, holidays};

use super::client::TestClient;
use super::helpers::{create_isolated_database, init_test_database, reference_date, seed_admin};

/// A running test application with its own database and HTTP client.
pub struct TestApp {
    pub base_url: String,
    pub admin_password: String,
    pub state: AppState,
    /// Keep the container alive for the duration of the test (None when TEST_DATABASE_URL is set).
    _container: Option<ContainerAsync<Postgres>>,
    /// Admin URL used to drop the database on cleanup (Some only when TEST_DATABASE_URL is set).
    admin_database_url: Option<String>,
    /// Name of the isolated test database to drop on cleanup.
    database_name: String,
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

    /// Like [`spawn`] but skips the admin seed step, leaving the database
    /// completely empty of users.  Use this when testing the initial-setup
    /// endpoint (`POST /api/v1/auth/setup`) so the call can actually succeed.
    pub async fn spawn_unseeded() -> (Self, String) {
        Self::spawn_unseeded_inner().await
    }

    async fn spawn_unseeded_inner() -> (Self, String) {
        let (admin_database_url, database_url_base, cleanup_admin_url, _container) =
            if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
                let base = url
                    .rsplit_once('/')
                    .map(|(before, _)| before)
                    .unwrap_or(&url)
                    .to_string();
                let cleanup_url = Some(url.clone());
                (url, base, cleanup_url, None)
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
                (admin_url, base, None, Some(container))
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
            public_url: None,
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

        // Intentionally skip seed_admin so the DB has no users.

        let broadcaster = zerf::services::notifications::broadcaster();
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

        // Return a dummy password for the unseeded app (the caller will set the real
        // password via the setup endpoint).
        let dummy_password = String::new();
        (
            Self {
                base_url: server_url,
                admin_password: dummy_password.clone(),
                state,
                _container,
                admin_database_url: cleanup_admin_url,
                database_name,
            },
            dummy_password,
        )
    }

    async fn spawn_inner(public_url: Option<String>) -> Self {
        let (admin_database_url, database_url_base, cleanup_admin_url, _container) =
            if let Ok(url) = std::env::var("TEST_DATABASE_URL") {
                // No container runtime — use a pre-existing local Postgres instance.
                let base = url
                    .rsplit_once('/')
                    .map(|(before, _)| before)
                    .unwrap_or(&url)
                    .to_string();
                let cleanup_url = Some(url.clone());
                (url, base, cleanup_url, None)
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
                (admin_url, base, None, Some(container))
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

        let broadcaster = zerf::services::notifications::broadcaster();
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
            admin_database_url: cleanup_admin_url,
            database_name,
        }
    }

    /// Create a new [`TestClient`] with its own cookie jar (= fresh session).
    pub fn client(&self) -> TestClient {
        TestClient::new(&self.base_url)
    }

    /// Cleanup: container is dropped automatically when TestApp is dropped.
    pub async fn cleanup(self) {
        // When using TEST_DATABASE_URL (no container), explicitly drop the
        // isolated test database so it doesn't accumulate between runs.
        if let Some(admin_url) = self.admin_database_url {
            if let Ok(pool) = sqlx::PgPool::connect(&admin_url).await {
                let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
                    "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
                    self.database_name
                )))
                .execute(&pool)
                .await;
            }
        }
        // Container is dropped when `self` goes out of scope, which stops
        // and removes the Postgres container automatically.
    }
}

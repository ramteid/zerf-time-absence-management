mod config;
mod db;
mod error;
mod auth;
mod audit;
mod holidays;
mod categories;
mod users;
mod time_entries;
mod absences;
mod change_requests;
mod reports;

use anyhow::Result;
use axum::{
    http::{header, HeaderName, HeaderValue},
    routing::{get, post, put, delete},
    Router, middleware,
};
use chrono::Datelike;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub cfg: Arc<config::Config>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,sqlx=warn".into())
    ).init();

    let cfg = config::Config::from_env();
    let pool = db::init(&cfg).await?;
    categories::ensure_initial(&pool).await?;
    let year = chrono::Local::now().year();
    holidays::ensure_holidays(&pool, year).await?;
    holidays::ensure_holidays(&pool, year + 1).await?;

    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role='admin'").fetch_one(&pool).await?;
    if admin_count == 0 {
        let temp = users::generate_password();
        let hash = auth::hash_password(&temp)?;
        let today = chrono::Local::now().date_naive();
        sqlx::query("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,annual_leave_days,start_date,must_change_password) VALUES (?,?,?,?,'admin',39.0,30,?,1)")
            .bind(cfg.admin_email.to_lowercase()).bind(hash).bind("Admin").bind("User").bind(today)
            .execute(&pool).await?;
        tracing::info!("==========================================================");
        tracing::info!("Admin email:    {}", cfg.admin_email);
        tracing::info!("Admin password: {}", temp);
        tracing::info!("Please change immediately after first login.");
        tracing::info!("==========================================================");
    }

    let state = AppState { pool: pool.clone(), cfg: Arc::new(cfg.clone()) };

    // Background hygiene: clean expired sessions and old login attempts.
    tokio::spawn(auth::cleanup_loop(pool.clone()));

    let api = Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .merge(
            Router::new()
                .route("/auth/me", get(auth::me))
                .route("/auth/password", put(auth::change_password))
                .route("/time-entries", get(time_entries::list).post(time_entries::create))
                .route("/time-entries/all", get(time_entries::list_all))
                .route("/time-entries/submit", post(time_entries::submit))
                .route("/time-entries/batch-approve", post(time_entries::batch_approve))
                .route("/time-entries/:id", put(time_entries::update).delete(time_entries::delete))
                .route("/time-entries/:id/approve", post(time_entries::approve))
                .route("/time-entries/:id/reject", post(time_entries::reject))
                .route("/absences", get(absences::list).post(absences::create))
                .route("/absences/all", get(absences::list_all))
                .route("/absences/calendar", get(absences::calendar))
                .route("/absences/:id", put(absences::update).delete(absences::cancel))
                .route("/absences/:id/approve", post(absences::approve))
                .route("/absences/:id/reject", post(absences::reject))
                .route("/leave-balance/:uid", get(absences::balance))
                .route("/change-requests", get(change_requests::list).post(change_requests::create))
                .route("/change-requests/all", get(change_requests::list_all))
                .route("/change-requests/:id/approve", post(change_requests::approve))
                .route("/change-requests/:id/reject", post(change_requests::reject))
                .route("/users", get(users::list).post(users::create))
                .route("/users/:id", get(users::get_one).put(users::update))
                .route("/users/:id/deactivate", post(users::deactivate))
                .route("/users/:id/reset-password", post(users::reset_password))
                .route("/categories", get(categories::list).post(categories::create))
                .route("/categories/:id", put(categories::update))
                .route("/holidays", get(holidays::list).post(holidays::create))
                .route("/holidays/:id", delete(holidays::delete))
                .route("/reports/month", get(reports::month))
                .route("/reports/month/csv", get(reports::month_csv))
                .route("/reports/team", get(reports::team))
                .route("/reports/categories", get(reports::categories))
                .route("/reports/overtime", get(reports::overtime))
                .route("/audit-log", get(audit::list))
                .layer(middleware::from_fn_with_state(state.clone(), auth::auth_middleware))
        );

    let static_dir = state.cfg.static_dir.clone();
    let index = format!("{}/index.html", static_dir);

    // Application-wide hardening:
    //  - 1 MiB body limit (no upload feature in v1)
    //  - 30 s request timeout
    //  - Strict HTTP security headers (HSTS handled by Caddy)
    let security_headers = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(HeaderName::from_static("x-content-type-options"), HeaderValue::from_static("nosniff")))
        .layer(SetResponseHeaderLayer::overriding(HeaderName::from_static("x-frame-options"), HeaderValue::from_static("DENY")))
        .layer(SetResponseHeaderLayer::overriding(HeaderName::from_static("referrer-policy"), HeaderValue::from_static("strict-origin-when-cross-origin")))
        .layer(SetResponseHeaderLayer::overriding(HeaderName::from_static("permissions-policy"), HeaderValue::from_static("accelerometer=(), camera=(), geolocation=(), gyroscope=(), microphone=(), payment=(), usb=()")))
        .layer(SetResponseHeaderLayer::overriding(HeaderName::from_static("cross-origin-opener-policy"), HeaderValue::from_static("same-origin")))
        .layer(SetResponseHeaderLayer::overriding(HeaderName::from_static("cross-origin-resource-policy"), HeaderValue::from_static("same-origin")))
        .layer(SetResponseHeaderLayer::overriding(HeaderName::from_static("content-security-policy"), HeaderValue::from_static(
            "default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'"
        )));

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api/v1", api)
        .fallback_service(
            ServeDir::new(&static_dir).not_found_service(ServeFile::new(index))
        )
        .with_state(state.clone())
        .layer(security_headers)
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("no-store")))
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .layer(TraceLayer::new_for_http());

    let addr: SocketAddr = cfg.bind.parse().expect("invalid KITAZEIT_BIND");
    tracing::info!(
        "KitaZeit listening on http://{} (secure_cookies={}, csrf={}, origin={})",
        addr, cfg.secure_cookies, cfg.enforce_csrf, cfg.enforce_origin
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

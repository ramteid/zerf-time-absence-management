pub mod absences;
pub mod audit;
pub mod auth;
pub mod categories;
pub mod change_requests;
pub mod config;
pub mod db;
pub mod email;
pub mod error;
pub mod holidays;
pub mod i18n;
pub mod notifications;
pub mod reopen_requests;
pub mod reports;
pub mod settings;
pub mod time_entries;
pub mod users;

use axum::http::{Method, StatusCode, Uri};
use axum::{
    extract::State,
    http::{header, HeaderName, HeaderValue},
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: db::DatabasePool,
    pub cfg: Arc<config::Config>,
    pub notifications: notifications::NotificationBroadcaster,
}

/// Seed the admin user if no admin exists yet.  Returns the temporary
/// password when a new admin was created (for log output / tests).
pub async fn seed_admin(
    pool: &db::DatabasePool,
    admin_email: &str,
) -> anyhow::Result<Option<String>> {
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role='admin'")
        .fetch_one(pool)
        .await?;
    if admin_count == 0 {
        let temp = "admin".to_string();
        let hash = auth::hash_password(&temp)?;
        let today = chrono::Local::now().date_naive();
        sqlx::query("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,annual_leave_days,start_date,must_change_password,overtime_start_balance_min) VALUES ($1,$2,$3,$4,'admin',39.0,30,$5,TRUE,0)")
            .bind(admin_email.to_lowercase()).bind(hash).bind("Admin").bind("User").bind(today)
            .execute(pool).await?;
        Ok(Some(temp))
    } else {
        Ok(None)
    }
}

/// Build the API router (without static-file serving).
pub fn build_api_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/settings/public", get(settings::public_settings))
        .merge(
            Router::new()
                .route("/auth/me", get(auth::me))
                .route("/auth/password", put(auth::change_password))
                .route("/auth/preferences", put(auth::update_preferences))
                .route(
                    "/settings",
                    get(settings::admin_settings).put(settings::update_admin_settings),
                )
                .route(
                    "/time-entries",
                    get(time_entries::list).post(time_entries::create),
                )
                .route("/time-entries/all", get(time_entries::list_all))
                .route("/time-entries/submit", post(time_entries::submit))
                .route(
                    "/time-entries/batch-approve",
                    post(time_entries::batch_approve),
                )
                .route(
                    "/time-entries/batch-reject",
                    post(time_entries::batch_reject),
                )
                .route(
                    "/time-entries/{id}",
                    put(time_entries::update).delete(time_entries::delete),
                )
                .route("/time-entries/{id}/approve", post(time_entries::approve))
                .route("/time-entries/{id}/reject", post(time_entries::reject))
                .route("/absences", get(absences::list).post(absences::create))
                .route("/absences/all", get(absences::list_all))
                .route("/absences/calendar", get(absences::calendar))
                .route(
                    "/absences/{id}",
                    put(absences::update).delete(absences::cancel),
                )
                .route("/absences/{id}/approve", post(absences::approve))
                .route("/absences/{id}/reject", post(absences::reject))
                .route("/absences/{id}/revoke", post(absences::revoke))
                .route("/leave-balance/{uid}", get(absences::balance))
                .route(
                    "/change-requests",
                    get(change_requests::list).post(change_requests::create),
                )
                .route("/change-requests/all", get(change_requests::list_all))
                .route(
                    "/change-requests/{id}/approve",
                    post(change_requests::approve),
                )
                .route(
                    "/change-requests/{id}/reject",
                    post(change_requests::reject),
                )
                .route("/users", get(users::list).post(users::create))
                .route("/users/{id}", get(users::get_one).put(users::update))
                .route("/users/{id}/deactivate", post(users::deactivate))
                .route("/users/{id}/reset-password", post(users::reset_password))
                .route(
                    "/users/{id}/leave-overrides",
                    get(users::get_leave_overrides).put(users::set_leave_override),
                )
                .route(
                    "/categories",
                    get(categories::list).post(categories::create),
                )
                .route("/categories/{id}", put(categories::update))
                .route("/holidays", get(holidays::list).post(holidays::create))
                .route("/holidays/countries", get(holidays::available_countries))
                .route(
                    "/holidays/regions/{country}",
                    get(holidays::available_regions),
                )
                .route("/holidays/{id}", delete(holidays::delete))
                .route("/reports/month", get(reports::month))
                .route("/reports/range", get(reports::range))
                .route("/reports/csv", get(reports::range_csv))
                .route("/reports/month/csv", get(reports::month_csv))
                .route("/reports/team", get(reports::team))
                .route("/reports/categories", get(reports::categories))
                .route("/reports/overtime", get(reports::overtime))
                .route("/reports/flextime", get(reports::flextime))
                .route("/audit-log", get(audit::list))
                .route(
                    "/reopen-requests",
                    get(reopen_requests::list_mine).post(reopen_requests::create),
                )
                .route(
                    "/reopen-requests/pending",
                    get(reopen_requests::list_pending),
                )
                .route(
                    "/reopen-requests/{id}/approve",
                    post(reopen_requests::approve),
                )
                .route(
                    "/reopen-requests/{id}/reject",
                    post(reopen_requests::reject),
                )
                .route(
                    "/notifications",
                    get(notifications::list).delete(notifications::delete_all),
                )
                .route(
                    "/notifications/unread-count",
                    get(notifications::unread_count),
                )
                .route("/notifications/stream", get(notifications::stream))
                .route("/notifications/{id}/read", post(notifications::mark_read))
                .route(
                    "/notifications/read-all",
                    post(notifications::mark_all_read),
                )
                .route("/team-settings", get(users::team_settings_list))
                .route("/team-settings/{id}", put(users::team_settings_update))
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth::auth_middleware,
                )),
        )
}

async fn serve_spa_index(
    static_dir: &str,
) -> Result<([(HeaderName, HeaderValue); 1], Vec<u8>), StatusCode> {
    let body = tokio::fs::read(format!("{static_dir}/index.html"))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok((
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )],
        body,
    ))
}

async fn spa_index(
    State(state): State<AppState>,
) -> Result<([(HeaderName, HeaderValue); 1], Vec<u8>), StatusCode> {
    serve_spa_index(&state.cfg.static_dir).await
}

async fn spa_fallback(
    State(state): State<AppState>,
    method: Method,
    uri: Uri,
) -> Result<([(HeaderName, HeaderValue); 1], Vec<u8>), StatusCode> {
    if method != Method::GET && method != Method::HEAD {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }

    let last_segment = uri.path().rsplit('/').next().unwrap_or_default();
    if last_segment.contains('.') {
        return Err(StatusCode::NOT_FOUND);
    }

    serve_spa_index(&state.cfg.static_dir).await
}

/// Build the complete application (API + static files + middleware).
pub fn build_app(state: AppState) -> Router {
    let api = build_api_router(state.clone());
    let static_dir = state.cfg.static_dir.clone();
    let assets_dir = format!("{}/assets", static_dir);

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

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api/v1", api)
        .nest_service("/assets", ServeDir::new(assets_dir))
        .route("/", get(spa_index))
        .route("/index.html", get(spa_index))
        .fallback(spa_fallback)
        .with_state(state)
        .layer(security_headers)
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(TraceLayer::new_for_http())
}

use crate::AppState;
use axum::http::{Method, StatusCode, Uri};
use axum::{
    extract::State,
    http::{header, HeaderName, HeaderValue},
    middleware,
    routing::{delete, get, post, put},
    Router,
};
use std::time::Duration;
use tower::ServiceBuilder;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::middleware::auth::auth_middleware;

/// Build the API router (without static-file serving).
pub fn build_api_router(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(handlers::auth::login))
        .route("/auth/logout", post(handlers::auth::logout))
        .route("/auth/setup-status", get(handlers::auth::setup_status))
        .route("/auth/setup", post(handlers::auth::setup))
        .route("/auth/forgot-password", post(handlers::auth::forgot_password))
        .route(
            "/auth/reset-password",
            post(handlers::auth::reset_password_with_token),
        )
        .route("/settings/public", get(handlers::settings::public_settings))
        .merge(
            Router::new()
                .route("/auth/me", get(handlers::auth::me))
                .route("/auth/password", put(handlers::auth::change_password))
                .route("/auth/preferences", put(handlers::auth::update_preferences))
                .route(
                    "/settings",
                    get(handlers::settings::admin_settings).put(handlers::settings::update_admin_settings),
                )
                .route("/settings/smtp", put(handlers::settings::update_smtp_settings))
                .route("/settings/smtp/test", post(handlers::settings::test_smtp_connection))
                .route(
                    "/time-entries",
                    get(handlers::time_entries::list).post(handlers::time_entries::create),
                )
                .route("/time-entries/all", get(handlers::time_entries::list_all))
                .route("/time-entries/submit", post(handlers::time_entries::submit))
                .route(
                    "/time-entries/batch-approve",
                    post(handlers::time_entries::batch_approve),
                )
                .route(
                    "/time-entries/batch-reject",
                    post(handlers::time_entries::batch_reject),
                )
                .route(
                    "/time-entries/{id}",
                    put(handlers::time_entries::update).delete(handlers::time_entries::delete),
                )
                .route("/absences", get(handlers::absences::list).post(handlers::absences::create))
                .route("/absences/all", get(handlers::absences::list_all))
                .route("/absences/calendar", get(handlers::absences::calendar))
                .route(
                    "/absences/{id}",
                    get(handlers::absences::get_one)
                        .put(handlers::absences::update)
                        .delete(handlers::absences::cancel),
                )
                .route("/absences/{id}/approve", post(handlers::absences::approve))
                .route("/absences/{id}/reject", post(handlers::absences::reject))
                .route("/absences/{id}/revoke", post(handlers::absences::revoke))
                .route(
                    "/absences/{id}/approve-cancellation",
                    post(handlers::absences::approve_cancellation),
                )
                .route(
                    "/absences/{id}/reject-cancellation",
                    post(handlers::absences::reject_cancellation),
                )
                .route("/leave-balance/{uid}", get(handlers::absences::balance))
                .route("/users", get(handlers::users::list).post(handlers::users::create))
                .route(
                    "/users/{id}",
                    get(handlers::users::get_one)
                        .put(handlers::users::update)
                        .delete(handlers::users::delete_user),
                )
                .route("/users/{id}/deactivate", post(handlers::users::deactivate))
                .route("/users/{id}/reset-password", post(handlers::users::reset_password))
                .route(
                    "/users/{id}/leave-days",
                    get(handlers::users::get_leave_days_handler).put(handlers::users::set_leave_days_handler),
                )
                .route(
                    "/categories",
                    get(handlers::categories::list).post(handlers::categories::create),
                )
                .route("/categories/all", get(handlers::categories::list_all))
                .route("/categories/{id}", put(handlers::categories::update))
                .route("/holidays", get(handlers::holidays::list).post(handlers::holidays::create))
                .route("/holidays/countries", get(handlers::holidays::available_countries))
                .route(
                    "/holidays/regions/{country}",
                    get(handlers::holidays::available_regions),
                )
                .route("/holidays/{id}", delete(handlers::holidays::delete))
                .route("/reports/month", get(handlers::reports::month))
                .route("/reports/range", get(handlers::reports::range))
                .route("/reports/csv", get(handlers::reports::range_csv))
                .route("/reports/month/csv", get(handlers::reports::month_csv))
                .route("/reports/team", get(handlers::reports::team))
                .route("/reports/categories", get(handlers::reports::categories))
                .route("/reports/team-categories", get(handlers::reports::team_categories))
                .route("/reports/overtime", get(handlers::reports::overtime))
                .route("/reports/flextime", get(handlers::reports::flextime))
                .route("/audit-log", get(crate::audit::list))
                .route(
                    "/reopen-requests",
                    get(handlers::reopen_requests::list_mine).post(handlers::reopen_requests::create),
                )
                .route(
                    "/reopen-requests/pending",
                    get(handlers::reopen_requests::list_pending),
                )
                .route(
                    "/reopen-requests/{id}/approve",
                    post(handlers::reopen_requests::approve),
                )
                .route(
                    "/reopen-requests/{id}/reject",
                    post(handlers::reopen_requests::reject),
                )
                .route(
                    "/notifications",
                    get(handlers::notifications::list).delete(handlers::notifications::delete_all),
                )
                .route(
                    "/notifications/unread-count",
                    get(handlers::notifications::unread_count),
                )
                .route("/notifications/stream", get(handlers::notifications::stream))
                .route("/notifications/{id}/read", post(handlers::notifications::mark_read))
                .route(
                    "/notifications/read-all",
                    post(handlers::notifications::mark_all_read),
                )
                .route("/team-settings", get(handlers::users::team_settings_list))
                .route("/team-settings/{id}", put(handlers::users::team_settings_update))
                .layer(middleware::from_fn_with_state(
                    state.clone(),
                    auth_middleware,
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

async fn serve_favicon(
    State(state): State<AppState>,
) -> Result<([(HeaderName, HeaderValue); 1], Vec<u8>), StatusCode> {
    let body = tokio::fs::read(format!("{}/favicon.svg", state.cfg.static_dir))
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok((
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("image/svg+xml"),
        )],
        body,
    ))
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

    // no-store for API, SPA index, and fallback — but NOT for hashed /assets/*
    let no_store_layer = SetResponseHeaderLayer::overriding(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );

    // Long-lived immutable caching for hashed static assets (/assets/*)
    let assets_service = ServeDir::new(assets_dir);
    let assets_router = Router::new().nest_service("/assets", assets_service).layer(
        SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        ),
    );

    // API + SPA routes get no-store
    let app_routes = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/api/v1", api)
        .route("/", get(spa_index))
        .route("/index.html", get(spa_index))
        .route("/favicon.svg", get(serve_favicon))
        .fallback(spa_fallback)
        .with_state(state.clone())
        .layer(no_store_layer);

    Router::new()
        .merge(assets_router)
        .merge(app_routes)
        .with_state(state)
        .layer(security_headers)
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(TraceLayer::new_for_http())
}

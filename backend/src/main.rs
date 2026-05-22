use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use zerf::{build_app, config, db, AppState};
use zerf::services::{categories, holidays, notifications, settings, auth as auth_service};
use zerf::background;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let config = config::Config::from_env();
    let pool = db::init(&config).await?;
    categories::ensure_initial(&pool).await?;
    let year = settings::app_current_year(&pool).await;
    holidays::ensure_holidays(&pool, year).await?;
    holidays::ensure_holidays(&pool, year + 1).await?;

    // Check if initial setup is needed (no users exist).
    let user_count = zerf::repository::UserDb::new(pool.clone()).count().await?;
    if user_count == 0 {
        tracing::info!("==========================================================");
        tracing::info!("No admin account found.");
        tracing::info!("Please open the application in your browser to complete");
        tracing::info!("the initial setup.");
        tracing::info!("==========================================================");
    }

    let broadcaster = notifications::broadcaster();
    let db = zerf::repository::Db::new(pool.clone(), broadcaster.clone());

    let state = AppState {
        pool: pool.clone(),
        db,
        cfg: Arc::new(config.clone()),
        notifications: broadcaster,
    };

    // Background hygiene: clean expired sessions, old login attempts, and
    // old notifications (>90 days).
    tokio::spawn(auth_service::cleanup_loop(pool.clone()));
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                interval.tick().await;
                notifications::cleanup_old(&db).await;
            }
        });
    }

    // Weekly holiday scheduler: every Monday at 12:00, check if next year holidays exist.
    tokio::spawn(background::holidays::run_loop(pool.clone()));

    // Submission reminder scheduler: wakes at 07:00 on the configured deadline day.
    tokio::spawn(background::submission_reminders::run_loop(
        pool.clone(),
        state.clone(),
    ));

    // Approval reminder scheduler: wakes every Monday at 07:00.
    tokio::spawn(background::approval_reminders::run_loop(state.clone()));

    let app = build_app(state);

    let addr: SocketAddr = config.bind.parse().expect("invalid ZERF_BIND");
    tracing::info!(
        "Zerf listening on http://{} (secure_cookies={}, csrf={}, origin={})",
        addr,
        config.secure_cookies,
        config.enforce_csrf,
        config.enforce_origin
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

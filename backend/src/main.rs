use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;
use zerf::background;
use zerf::services::{auth as auth_service, categories, holidays, notifications, settings};
use zerf::{build_app, config, db, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    // Console logging (env-filtered) plus a capture layer that forwards every
    // warn/error event to the database once the writer task starts below.
    let (log_layer, log_receiver) = zerf::log_capture::channel();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer().with_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "info,sqlx=warn".into()),
            ),
        )
        .with(log_layer.with_filter(tracing_subscriber::filter::LevelFilter::WARN))
        .init();

    let config = config::Config::from_env();
    let pool = db::init(&config).await?;
    // Drain captured warn/error events into the app_logs table.
    tokio::spawn(zerf::log_capture::run_writer(pool.clone(), log_receiver));
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
        let pool = pool.clone();
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                interval.tick().await;
                notifications::cleanup_old(&db).await;

                // Prune audit log entries older than 10 years.
                db.audit.cleanup_old().await;

                // Enforce app log bounds (1000 rows / 365 days). Same
                // daily-only pattern as the other prunes in this loop; a
                // fresh install's first tick fires immediately (see
                // tokio::time::interval), so bounds are also enforced right
                // after every restart.
                let _ = db.app_logs.prune().await;

                // Prune resolved reopen requests older than retention setting (default 365 days).
                let reopen_days =
                    settings::load_setting(&pool, "reopen_request_retention_days", "365")
                        .await
                        .ok()
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(365);
                db.reopen_requests.cleanup_old(reopen_days).await;

                // Remove stale system-alert email throttle keys (not touched in 30 days).
                db.settings.cleanup_stale_alert_keys().await;
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

    // Monthly timesheet PDF upload to Nextcloud: checks daily at midnight.
    tokio::spawn(background::report_upload::run_loop(state.clone()));

    // Monthly payroll report email to the tax office: checks daily at midnight.
    tokio::spawn(background::payroll_report::run_loop(state.clone()));

    // Error-notification worker: drains the error queue (backend + backup
    // failures) and alerts opted-in admins in-app and by email.
    tokio::spawn(background::error_notifications::run_loop(state.clone()));

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

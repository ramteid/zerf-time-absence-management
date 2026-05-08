use anyhow::Result;
use chrono::Datelike;
use std::net::SocketAddr;
use std::sync::Arc;
use zerf::{build_app, categories, config, db, holidays, AppState};

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
    let year = chrono::Local::now().year();
    holidays::ensure_holidays(&pool, year).await?;
    holidays::ensure_holidays(&pool, year + 1).await?;

    // Check if initial setup is needed (no users exist).
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await?;
    if user_count == 0 {
        tracing::info!("==========================================================");
        tracing::info!("No admin account found.");
        tracing::info!("Please open the application in your browser to complete");
        tracing::info!("the initial setup.");
        tracing::info!("==========================================================");
    }

    let broadcaster = zerf::notifications::broadcaster();
    let db = zerf::repository::Db::new(pool.clone(), broadcaster.clone());

    let state = AppState {
        pool: pool.clone(),
        db,
        cfg: Arc::new(config.clone()),
        notifications: broadcaster,
    };

    // Background hygiene: clean expired sessions, old login attempts, and
    // old notifications (>90 days).
    tokio::spawn(zerf::auth::cleanup_loop(pool.clone()));
    {
        let db = state.db.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                interval.tick().await;
                zerf::notifications::cleanup_old(&db).await;
            }
        });
    }

    // Weekly holiday scheduler: every Monday at 12:00, check if next year holidays exist.
    {
        let pool = pool.clone();
        tokio::spawn(async move {
            loop {
                let now = chrono::Local::now();
                let wait = holidays::duration_until_next_monday_noon(now)
                    .unwrap_or(std::time::Duration::from_secs(3600));
                tokio::time::sleep(wait).await;

                let next_year = chrono::Local::now().year() + 1;
                if let Err(error) = holidays::ensure_holidays(&pool, next_year).await {
                    tracing::warn!(
                        "Holiday scheduler: failed to ensure holidays for {next_year}: {error:?}"
                    );
                } else {
                    tracing::info!("Holiday scheduler: ensured holidays for {next_year}");
                }
            }
        });
    }

    // Submission reminder scheduler: wakes at 07:00 on the configured deadline day.
    tokio::spawn(zerf::submission_reminders::run_loop(
        pool.clone(),
        state.clone(),
    ));

    // Approval reminder scheduler: wakes every Monday at 07:00.
    tokio::spawn(zerf::approval_reminders::run_loop(
        pool.clone(),
        state.clone(),
    ));

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

use anyhow::Result;
use chrono::Datelike;
use kitazeit::{build_app, categories, config, db, holidays, seed_admin, AppState};
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let cfg = config::Config::from_env();
    let pool = db::init(&cfg).await?;
    categories::ensure_initial(&pool).await?;
    let year = chrono::Local::now().year();
    holidays::ensure_holidays(&pool, year).await?;
    holidays::ensure_holidays(&pool, year + 1).await?;

    if let Some(temp) = seed_admin(&pool, &cfg.admin_email).await? {
        tracing::info!("==========================================================");
        tracing::info!("Admin email:    {}", cfg.admin_email);
        tracing::info!("Admin password: {}", temp);
        tracing::info!("Please change immediately after first login.");
        tracing::info!("==========================================================");
    }

    let state = AppState {
        pool: pool.clone(),
        cfg: Arc::new(cfg.clone()),
        notifications: kitazeit::notifications::broadcaster(),
    };

    // Background hygiene: clean expired sessions, old login attempts, and
    // old notifications (>90 days).
    tokio::spawn(kitazeit::auth::cleanup_loop(pool.clone()));
    {
        let p = pool.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(86_400));
            loop {
                interval.tick().await;
                kitazeit::notifications::cleanup_old(&p).await;
            }
        });
    }

    // Weekly holiday scheduler: every Monday at 12:00, check if next year holidays exist.
    {
        let p = pool.clone();
        tokio::spawn(async move {
            loop {
                let now = chrono::Local::now();
                let wait = holidays::duration_until_next_monday_noon(now)
                    .unwrap_or(std::time::Duration::from_secs(3600));
                tokio::time::sleep(wait).await;

                let next_year = chrono::Local::now().year() + 1;
                if let Err(e) = holidays::ensure_holidays(&p, next_year).await {
                    tracing::warn!(
                        "Holiday scheduler: failed to ensure holidays for {next_year}: {e:?}"
                    );
                } else {
                    tracing::info!("Holiday scheduler: ensured holidays for {next_year}");
                }
            }
        });
    }

    let app = build_app(state);

    let addr: SocketAddr = cfg.bind.parse().expect("invalid KITAZEIT_BIND");
    tracing::info!(
        "KitaZeit listening on http://{} (secure_cookies={}, csrf={}, origin={})",
        addr,
        cfg.secure_cookies,
        cfg.enforce_csrf,
        cfg.enforce_origin
    );
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

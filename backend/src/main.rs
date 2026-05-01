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
    };

    // Background hygiene: clean expired sessions and old login attempts.
    tokio::spawn(kitazeit::auth::cleanup_loop(pool.clone()));

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

use crate::db::DatabasePool;

/// Weekly holiday scheduler: every Monday at 12:00, check if next year holidays exist.
pub async fn run_loop(pool: DatabasePool) {
    loop {
        let tz = crate::services::settings::load_app_timezone(&pool).await;
        let now = chrono::Utc::now().with_timezone(&tz);
        let wait = crate::services::holidays::duration_until_next_monday_noon(now)
            .unwrap_or(std::time::Duration::from_secs(3600));
        tokio::time::sleep(wait).await;

        let next_year = crate::services::settings::app_current_year(&pool).await + 1;
        if let Err(error) = crate::services::holidays::ensure_holidays(&pool, next_year).await {
            tracing::warn!(
                "Holiday scheduler: failed to ensure holidays for {next_year}: {error:?}"
            );
        } else {
            tracing::info!("Holiday scheduler: ensured holidays for {next_year}");
        }
    }
}

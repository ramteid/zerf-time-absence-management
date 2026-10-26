use crate::db::DatabasePool;

/// Weekly holiday scheduler: every Monday at 12:00, check if next year holidays exist.
pub async fn run_loop(pool: DatabasePool) {
    let mut last_run = None;
    loop {
        last_run = Some(
            super::schedule::wait_for_calendar_run(
                &pool,
                super::schedule::CalendarSchedule::MondayNoon,
                last_run,
            )
            .await,
        );

        let next_year = crate::services::settings::app_current_year(&pool).await + 1;
        if let Err(error) = crate::services::holidays::ensure_holidays(&pool, next_year).await {
            tracing::error!(
                "Holiday scheduler: failed to ensure holidays for {next_year}: {error:?}"
            );
        } else {
            tracing::info!("Holiday scheduler: ensured holidays for {next_year}");
        }
    }
}

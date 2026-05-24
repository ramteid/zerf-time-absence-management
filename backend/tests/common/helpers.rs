use chrono::Datelike;
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use zerf::{db, repository::UserDb};
use zerf::services::{auth, users};

static TEST_DB_COUNTER: AtomicU64 = AtomicU64::new(0);
pub static TEST_MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub async fn create_isolated_database(admin_database_url: &str) -> anyhow::Result<String> {
    let db_name = format!(
        "zerf_test_{}_{}",
        std::process::id(),
        TEST_DB_COUNTER.fetch_add(1, Ordering::Relaxed),
    );
    let admin_pool = sqlx::PgPool::connect(admin_database_url).await?;
    sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
        .execute(&admin_pool)
        .await?;
    Ok(db_name)
}

pub async fn init_test_database(database_url: &str) -> anyhow::Result<db::DatabasePool> {
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .test_before_acquire(true)
        .connect(database_url)
        .await?;

    // sqlx migrations are expected to be serialized, but in CI and highly parallel
    // local runs we occasionally observe a duplicate insert into _sqlx_migrations.
    // Retry a couple of times so transient migration-table races don't fail tests.
    let mut last_err: Option<anyhow::Error> = None;
    for _ in 0..3 {
        match TEST_MIGRATOR.run(&pool).await {
            Ok(_) => return Ok(pool),
            Err(err) => {
                let msg = err.to_string();
                if msg.contains("_sqlx_migrations_pkey")
                    || msg.contains("duplicate key value violates unique constraint")
                {
                    last_err = Some(err.into());
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
                return Err(err.into());
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("failed to run migrations")))
}

/// Seed a test admin user with a CSPRNG-generated password.
/// Only used in test code — never compiled into the production binary.
pub async fn seed_admin(pool: &db::DatabasePool, admin_email: &str) -> anyhow::Result<Option<String>> {
    let admin_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE role='admin'")
        .fetch_one(pool)
        .await?;
    if admin_count == 0 {
        let temp = users::generate_password();
        let hash = auth::hash_password(&temp)?;
        let ref_date = reference_date();
        sqlx::query("INSERT INTO users(email,password_hash,first_name,last_name,role,weekly_hours,start_date,must_change_password,overtime_start_balance_min) VALUES ($1,$2,$3,$4,'admin',39.0,$5,TRUE,0)")
            .bind(admin_email.to_lowercase()).bind(hash).bind("Test").bind("Admin").bind(ref_date)
            .execute(pool).await?;

        let admin_id: i64 = sqlx::query_scalar("SELECT id FROM users WHERE email=$1")
            .bind(admin_email.to_lowercase())
            .fetch_one(pool)
            .await?;
        let current_year = ref_date.year();
        let user_db = UserDb::new(pool.clone());
        user_db.set_leave_days(admin_id, current_year, 30).await?;
        user_db
            .set_leave_days(admin_id, current_year + 1, 30)
            .await?;

        Ok(Some(temp))
    } else {
        Ok(None)
    }
}

/// Returns the reference date used by all test date helpers.
/// Reads TEST_REFERENCE_DATE (YYYY-MM-DD) when set, otherwise today.
pub fn reference_date() -> chrono::NaiveDate {
    if let Ok(s) = std::env::var("TEST_REFERENCE_DATE") {
        chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")
            .expect("TEST_REFERENCE_DATE must be YYYY-MM-DD")
    } else {
        chrono::Local::now().date_naive()
    }
}

use crate::config::Config;
use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

const MIGRATIONS: &[(&str, &str)] = &[
    ("001_initial",  include_str!("../migrations/001_initial.sql")),
    ("002_security", include_str!("../migrations/002_security.sql")),
];

pub async fn init(cfg: &Config) -> Result<SqlitePool> {
    if let Some(parent) = std::path::Path::new(&cfg.database_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", cfg.database_path))?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
        .busy_timeout(std::time::Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect_with(opts).await?;

    // Tighten DB file permissions: only the owning unix user may read/write.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&cfg.database_path) {
            let mut p = meta.permissions(); p.set_mode(0o600);
            let _ = std::fs::set_permissions(&cfg.database_path, p);
        }
    }

    // Lightweight migration ledger.
    sqlx::query("CREATE TABLE IF NOT EXISTS schema_migrations(name TEXT PRIMARY KEY, applied_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP)")
        .execute(&pool).await?;

    for (name, sql) in MIGRATIONS {
        let already: Option<String> = sqlx::query_scalar("SELECT name FROM schema_migrations WHERE name = ?")
            .bind(name).fetch_optional(&pool).await?;
        if already.is_some() { continue; }
        // Strip `--` comments first so embedded ';' inside comments don't confuse
        // the naive splitter, then run each statement individually.
        let cleaned: String = sql.lines()
            .map(|l| if let Some(i) = l.find("--") { &l[..i] } else { l })
            .collect::<Vec<_>>().join("\n");
        for stmt in cleaned.split(';') {
            let s = stmt.trim();
            if s.is_empty() { continue; }
            if let Err(e) = sqlx::query(s).execute(&pool).await {
                let msg = e.to_string();
                if msg.contains("duplicate column name") { continue; }
                return Err(e.into());
            }
        }
        sqlx::query("INSERT INTO schema_migrations(name) VALUES (?)").bind(name).execute(&pool).await?;
    }
    Ok(pool)
}

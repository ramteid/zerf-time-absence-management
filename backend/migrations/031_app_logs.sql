-- Centralized application log for warn/error messages.
--
-- Every tracing::warn!/tracing::error! emitted anywhere in the backend is
-- captured by a tracing layer (src/applog.rs) and persisted here so admins
-- can inspect problems in the UI without shell access to the host.
--
-- Storage is bounded in two ways to avoid unbounded growth (enforced by
-- AppLogDb::prune, called after every insert and once per day):
--   * at most 1000 rows are kept (oldest deleted first)
--   * rows older than 365 days expire

CREATE TABLE IF NOT EXISTS app_logs (
    id BIGSERIAL PRIMARY KEY,
    -- Severity of the captured event; only warn and error are persisted.
    level TEXT NOT NULL CHECK (level IN ('warn', 'error')),
    -- The formatted log message.
    message TEXT NOT NULL,
    -- Rust module path / tracing target the event originated from.
    target TEXT NOT NULL DEFAULT '',
    -- Additional structured tracing fields (key/value), if any.
    fields JSONB,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Listing and pruning both order by recency.
CREATE INDEX IF NOT EXISTS idx_app_logs_occurred_at ON app_logs (occurred_at DESC);

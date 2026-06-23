-- Persistent backup timestamp: survives container restarts so the interval is
-- calculated from the last actual backup, not from when the container started.
-- Seeded with NOW() so a fresh install waits one full interval before the first
-- backup rather than running immediately on every container restart.
INSERT INTO app_settings (key, value)
VALUES ('backup_last_success_at', TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'))
ON CONFLICT (key) DO NOTHING;

-- Days-based local retention is replaced by a hard count (keep last 10 files).
-- The backup container no longer reads this key.
DELETE FROM app_settings WHERE key = 'backup_retention_days';

-- Pinned flag for system-error notifications.  Pinned+unread rows sort to the
-- top of the notification panel and receive a distinct visual treatment in the UI.
ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS pinned BOOLEAN NOT NULL DEFAULT FALSE;

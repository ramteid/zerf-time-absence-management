-- Add per-session CSRF token (double-submit cookie pattern).
-- SQLite ALTER TABLE supports adding columns. NULL is acceptable for legacy
-- rows; the auth middleware tolerates an empty csrf_token only when the global
-- KITAZEIT_ENFORCE_CSRF flag is off.
ALTER TABLE sessions ADD COLUMN csrf_token TEXT NOT NULL DEFAULT '';

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_active ON sessions(last_active_at);

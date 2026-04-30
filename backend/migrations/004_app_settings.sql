-- Compatibility migration for databases that were initialized before
-- `app_settings` was folded into `001_initial.sql`.
CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO app_settings(key, value)
VALUES ('ui_language', 'en')
ON CONFLICT (key) DO NOTHING;
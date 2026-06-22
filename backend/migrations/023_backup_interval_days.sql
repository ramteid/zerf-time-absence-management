-- Convert backup_interval_seconds to backup_interval_days.
-- The stored value (seconds) is divided by 86400; values < 1 day default to 1.
-- This is a one-time migration; the backup container now reads backup_interval_days
-- and multiplies by 86400 internally.
INSERT INTO app_settings (key, value)
SELECT
    'backup_interval_days',
    GREATEST(1, (value::BIGINT / 86400))::TEXT
FROM app_settings
WHERE key = 'backup_interval_seconds'
  AND value ~ '^[0-9]+$'
  AND value::BIGINT > 0
ON CONFLICT (key) DO NOTHING;

-- Ensure the row exists even if backup_interval_seconds was missing/invalid.
INSERT INTO app_settings (key, value) VALUES ('backup_interval_days', '1')
ON CONFLICT (key) DO NOTHING;

DELETE FROM app_settings WHERE key = 'backup_interval_seconds';

-- Seed Nextcloud upload and backup frequency/retention settings.
-- Backup interval and retention migrate from env vars (BACKUP_INTERVAL_SECONDS /
-- BACKUP_RETENTION_DAYS) to app_settings so they are editable in the Admin UI.
-- The report upload day controls when the monthly PDF is uploaded to Nextcloud.
INSERT INTO app_settings (key, value)
VALUES
    ('backup_interval_seconds', '86400'),
    ('backup_retention_days', '30'),
    ('report_upload_day_of_month', '5')
ON CONFLICT (key) DO NOTHING;

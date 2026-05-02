-- Add is_auto flag to distinguish API-fetched holidays from manually added ones
ALTER TABLE holidays ADD COLUMN IF NOT EXISTS is_auto BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE holidays ADD COLUMN IF NOT EXISTS local_name TEXT;

-- Mark all existing holidays as auto-generated (they came from the old hardcoded holidays_bw function)
UPDATE holidays SET is_auto = TRUE WHERE is_auto = FALSE;

-- Add country and region settings
INSERT INTO app_settings(key, value) VALUES ('country', 'DE') ON CONFLICT (key) DO NOTHING;
INSERT INTO app_settings(key, value) VALUES ('region', 'DE-BW') ON CONFLICT (key) DO NOTHING;

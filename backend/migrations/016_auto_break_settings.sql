-- Seed the automatic break deduction settings with the feature disabled.
-- Administrators can enable and configure this via Admin → General Settings.
INSERT INTO app_settings (key, value)
VALUES ('auto_break_enabled', 'false')
ON CONFLICT (key) DO NOTHING;

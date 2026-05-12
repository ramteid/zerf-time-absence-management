ALTER TABLE notifications
    ADD COLUMN IF NOT EXISTS dedupe_key TEXT;

DROP INDEX IF EXISTS uq_notifications_reminder_daily;
DROP INDEX IF EXISTS uq_notifications_approval_reminder_daily;
DROP FUNCTION IF EXISTS notifications_created_date(TIMESTAMPTZ);

CREATE UNIQUE INDEX IF NOT EXISTS uq_notifications_user_kind_dedupe_key
    ON notifications (user_id, kind, dedupe_key)
    WHERE dedupe_key IS NOT NULL;

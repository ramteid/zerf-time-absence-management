-- Prevent duplicate submission_reminder notifications for the same user on the same day.
-- The check-then-insert pattern in submission_reminders.rs is susceptible to a TOCTOU race
-- if the background job restarts or overlaps with itself. This unique index makes the INSERT
-- idempotent via ON CONFLICT DO NOTHING.
--
-- PostgreSQL requires index expressions to be IMMUTABLE. Casting timestamptz to
-- date is only STABLE (it depends on the session timezone), so we create a thin
-- IMMUTABLE wrapper that pins the conversion to UTC.
CREATE OR REPLACE FUNCTION notifications_created_date(ts TIMESTAMPTZ)
RETURNS DATE
LANGUAGE sql
IMMUTABLE
AS $$ SELECT (ts AT TIME ZONE 'UTC')::date $$;

CREATE UNIQUE INDEX IF NOT EXISTS uq_notifications_reminder_daily
    ON notifications (user_id, kind, notifications_created_date(created_at))
    WHERE kind = 'submission_reminder';

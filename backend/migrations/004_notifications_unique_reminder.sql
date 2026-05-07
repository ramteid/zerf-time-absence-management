-- Prevent duplicate submission_reminder notifications for the same user on the same day.
-- The check-then-insert pattern in submission_reminders.rs is susceptible to a TOCTOU race
-- if the background job restarts or overlaps with itself. This unique index makes the INSERT
-- idempotent via ON CONFLICT DO NOTHING.
CREATE UNIQUE INDEX IF NOT EXISTS uq_notifications_reminder_daily
    ON notifications (user_id, kind, (created_at::date))
    WHERE kind = 'submission_reminder';

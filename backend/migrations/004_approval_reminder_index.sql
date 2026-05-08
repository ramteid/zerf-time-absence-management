-- Idempotency guard for weekly approval reminders (mirrors uq_notifications_reminder_daily
-- which covers submission_reminder). Prevents duplicate approval_reminder notifications
-- within the same calendar day for the same approver.
CREATE UNIQUE INDEX IF NOT EXISTS uq_notifications_approval_reminder_daily
    ON notifications (user_id, kind, notifications_created_date(created_at))
    WHERE kind = 'approval_reminder';

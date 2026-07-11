-- Opt-in admin notifications for technical system errors.
--
-- 1) Per-user opt-in flag (admins only).
--
-- When TRUE (and the user is an active admin), the user receives an in-app
-- notification and an email whenever the backend logs a technical error or the
-- backup container reports a failure. Default FALSE so neither existing users
-- nor newly created ones are opted in until an admin explicitly enables it.
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS receives_error_notifications BOOLEAN NOT NULL DEFAULT FALSE;

-- 2) Queue of technical-error events awaiting fan-out to opted-in admins.
--
-- Producers (the backend's log-capture writer, curated backend call sites, and
-- the backup container via psql) INSERT one row per error event. A backend
-- worker drains the queue promptly: for each row it delivers an in-app + email
-- notification to every active admin who opted in
-- (users.receives_error_notifications), then deletes the row. Processing a row
-- exactly once (delete after the attempt) means a missing SMTP configuration
-- cannot cause endless retries.
CREATE TABLE IF NOT EXISTS error_notification_queue (
    id BIGSERIAL PRIMARY KEY,
    -- Deduplicates repeat alerts of the same failure class across recipients
    -- (pinned re-alert semantics in the notifications table).
    dedupe_key TEXT,
    -- Short summary shown bold in the UI and used as the email subject.
    title TEXT NOT NULL,
    -- Failure-specific detail shown as secondary text / email body.
    body TEXT,
    -- Origin of the event, for diagnostics only ('app' | 'backup').
    source TEXT NOT NULL DEFAULT 'app',
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

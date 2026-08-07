-- Durable outbound email queue.
--
-- Previously, notification emails were sent fire-and-forget in a detached
-- task (the old crate::email::send_async): a transient SMTP failure silently
-- lost the message, logged only as a WARN. Every email that should be
-- delivered is now inserted here with its fully rendered subject/body first
-- (rendering — including i18n and the timestamp/app-URL footer — always
-- happens once at enqueue time, never re-derived at send time). A background
-- worker (backend/src/background/email_queue.rs) polls every 2 minutes and
-- processes rows oldest-first; a row is deleted only once the SMTP server
-- actually accepted the message. A row that keeps failing simply stays
-- queued and is retried on the next poll — nothing is ever silently dropped.
CREATE TABLE IF NOT EXISTS email_queue (
    id BIGSERIAL PRIMARY KEY,
    to_address TEXT NOT NULL,
    -- Recipient display name, e.g. "Jane Doe". Empty when none is known.
    to_name TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL,
    body_text TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    -- Delivery attempts made so far. Only incremented on an actual SMTP
    -- attempt — a cycle skipped because SMTP is unconfigured or the circuit
    -- breaker is open does not count.
    attempts INTEGER NOT NULL DEFAULT 0,
    last_attempt_at TIMESTAMPTZ,
    last_error TEXT
);

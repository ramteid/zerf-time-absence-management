-- Remember which kind of notification produced each queued email.
--
-- The queue worker reports a failed delivery to the opted-in admins. Those
-- reports are themselves emails (kind 'system_error'), so without knowing a
-- row's kind the worker could not tell "an ordinary mail failed" from "the
-- alert about a failed mail failed" — and would answer the second with yet
-- another alert mail, endlessly, for as long as the mail server stays down.
-- Rows queued before this migration keep the empty default and are treated
-- as ordinary mail.
ALTER TABLE email_queue
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT '';

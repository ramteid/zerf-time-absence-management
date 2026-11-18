-- Mark queued emails that the worker has stopped trying to deliver.
--
-- Until now a mail that kept failing stayed in `email_queue` and was retried on
-- every poll for as long as it took, and every failed attempt was reported to
-- the admins. Now a failed mail is retried on a widening schedule (see
-- backend/src/background/email_queue.rs). When the last scheduled attempt
-- fails too, the worker sets `given_up_at`, reports that one failure to the
-- admins and never picks the row up again. The row itself stays for a while so
-- a lost mail can still be looked up, and the daily cleanup removes it later.
--
-- NULL means the mail is still waiting for delivery. Rows queued before this
-- migration keep NULL: they get their remaining attempts under the new
-- schedule and are reported if those fail as well, so nothing is dropped
-- silently by the change itself.
ALTER TABLE email_queue
    ADD COLUMN IF NOT EXISTS given_up_at TIMESTAMPTZ;

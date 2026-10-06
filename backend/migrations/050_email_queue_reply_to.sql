-- Remember who a queued email should be answered by.
--
-- Mail that follows directly from a person's action (an approver rejecting an
-- absence or a timesheet, for example) is sent from the shared system account.
-- A reply to it would land in that mailbox, where nobody reads it. The queue
-- therefore stores an optional Reply-To (the acting person) next to the
-- already-rendered message, and the worker sets it as the header on delivery.
-- Empty means "no Reply-To": rows queued before this migration and all system
-- mail keep the empty default and are sent exactly as before.
ALTER TABLE email_queue
    ADD COLUMN IF NOT EXISTS reply_to_address TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS reply_to_name TEXT NOT NULL DEFAULT '';

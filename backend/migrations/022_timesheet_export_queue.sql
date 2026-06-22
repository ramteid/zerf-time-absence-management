-- Per-employee monthly timesheet PDF export queue.
-- Populated on the configured day-of-month for the previous month; entries are
-- deleted after a successful upload.  ON DELETE CASCADE keeps the queue clean
-- when a user is removed.
CREATE TABLE IF NOT EXISTS timesheet_export_queue (
    user_id    BIGINT  NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period     CHAR(7) NOT NULL,   -- "YYYY-MM"
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, period)
);

-- Track whether a rejected time entry has been closed by a replacement entry.
-- Rejected rows stay in the audit/history trail, but closed rows no longer keep
-- a week incomplete or get reset by a later reopen request.
ALTER TABLE time_entries
    ADD COLUMN IF NOT EXISTS rejection_resolved_at TIMESTAMPTZ;

ALTER TABLE time_entries
    ADD COLUMN IF NOT EXISTS rejection_resolved_by BIGINT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'time_entries_rejection_resolved_by_fkey'
        AND conrelid = 'time_entries'::regclass
    ) THEN
        ALTER TABLE time_entries
            ADD CONSTRAINT time_entries_rejection_resolved_by_fkey
            FOREIGN KEY (rejection_resolved_by)
            REFERENCES users(id)
            ON DELETE SET NULL;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_te_unresolved_rejected_user_date
    ON time_entries(user_id, entry_date)
    WHERE status = 'rejected' AND rejection_resolved_at IS NULL;

UPDATE time_entries rejected
SET rejection_resolved_at = COALESCE(rejected.reviewed_at, rejected.updated_at, NOW()),
    rejection_resolved_by = rejected.user_id
WHERE rejected.status = 'rejected'
AND rejected.rejection_resolved_at IS NULL
AND EXISTS (
    SELECT 1
    FROM time_entries replacement
    WHERE replacement.user_id = rejected.user_id
    AND replacement.entry_date = rejected.entry_date
    AND replacement.status IN ('submitted', 'approved')
    AND replacement.id <> rejected.id
    AND replacement.start_time::time < rejected.end_time::time
    AND replacement.end_time::time > rejected.start_time::time
);

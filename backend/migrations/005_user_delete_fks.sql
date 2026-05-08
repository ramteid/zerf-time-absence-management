-- Fix ON DELETE behaviors to enable hard user deletion.
-- User's own data → CASCADE; reviewer/audit references → SET NULL (preserves records).
-- Also fix change_requests.time_entry_id so cascading time_entry deletes work.

ALTER TABLE sessions          DROP CONSTRAINT IF EXISTS sessions_user_id_fkey;
ALTER TABLE time_entries      DROP CONSTRAINT IF EXISTS time_entries_user_id_fkey;
ALTER TABLE time_entries      DROP CONSTRAINT IF EXISTS time_entries_reviewed_by_fkey;
ALTER TABLE absences          DROP CONSTRAINT IF EXISTS absences_user_id_fkey;
ALTER TABLE absences          DROP CONSTRAINT IF EXISTS absences_reviewed_by_fkey;
ALTER TABLE change_requests   DROP CONSTRAINT IF EXISTS change_requests_user_id_fkey;
ALTER TABLE change_requests   DROP CONSTRAINT IF EXISTS change_requests_time_entry_id_fkey;
ALTER TABLE change_requests   DROP CONSTRAINT IF EXISTS change_requests_reviewed_by_fkey;
ALTER TABLE reopen_requests   DROP CONSTRAINT IF EXISTS reopen_requests_user_id_fkey;
ALTER TABLE reopen_requests   DROP CONSTRAINT IF EXISTS reopen_requests_reviewed_by_fkey;
ALTER TABLE notifications     DROP CONSTRAINT IF EXISTS notifications_user_id_fkey;
ALTER TABLE audit_log         DROP CONSTRAINT IF EXISTS audit_log_user_id_fkey;
ALTER TABLE user_annual_leave DROP CONSTRAINT IF EXISTS user_annual_leave_user_id_fkey;

-- audit_log.user_id must be nullable so ON DELETE SET NULL can work.
ALTER TABLE audit_log ALTER COLUMN user_id DROP NOT NULL;

-- User's own data: delete everything when the user is deleted.
ALTER TABLE sessions          ADD CONSTRAINT sessions_user_id_fkey          FOREIGN KEY (user_id)        REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE time_entries      ADD CONSTRAINT time_entries_user_id_fkey      FOREIGN KEY (user_id)        REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE absences          ADD CONSTRAINT absences_user_id_fkey          FOREIGN KEY (user_id)        REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE change_requests   ADD CONSTRAINT change_requests_user_id_fkey   FOREIGN KEY (user_id)        REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE change_requests   ADD CONSTRAINT change_requests_time_entry_id_fkey FOREIGN KEY (time_entry_id) REFERENCES time_entries(id) ON DELETE CASCADE;
ALTER TABLE reopen_requests   ADD CONSTRAINT reopen_requests_user_id_fkey   FOREIGN KEY (user_id)        REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE notifications     ADD CONSTRAINT notifications_user_id_fkey     FOREIGN KEY (user_id)        REFERENCES users(id) ON DELETE CASCADE;
ALTER TABLE user_annual_leave ADD CONSTRAINT user_annual_leave_user_id_fkey FOREIGN KEY (user_id)        REFERENCES users(id) ON DELETE CASCADE;

-- Reviewer/approver references: keep the record, just lose the attribution.
ALTER TABLE time_entries      ADD CONSTRAINT time_entries_reviewed_by_fkey     FOREIGN KEY (reviewed_by) REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE absences          ADD CONSTRAINT absences_reviewed_by_fkey         FOREIGN KEY (reviewed_by) REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE change_requests   ADD CONSTRAINT change_requests_reviewed_by_fkey  FOREIGN KEY (reviewed_by) REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE reopen_requests   ADD CONSTRAINT reopen_requests_reviewed_by_fkey  FOREIGN KEY (reviewed_by) REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE audit_log         ADD CONSTRAINT audit_log_user_id_fkey            FOREIGN KEY (user_id)     REFERENCES users(id) ON DELETE SET NULL;

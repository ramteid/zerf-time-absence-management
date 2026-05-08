-- Phase 1: Create new user_approvers junction table
CREATE TABLE IF NOT EXISTS user_approvers (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    approver_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, approver_id),
    CONSTRAINT user_approvers_not_self CHECK (user_id <> approver_id)
);
CREATE INDEX IF NOT EXISTS idx_user_approvers_approver ON user_approvers(approver_id);

-- Phase 2: Migrate existing approver_id values
INSERT INTO user_approvers (user_id, approver_id)
SELECT id, approver_id FROM users WHERE approver_id IS NOT NULL;

-- Phase 3: Remove the approver_id column from users table
-- This drops the column and its associated constraints and index
ALTER TABLE users DROP CONSTRAINT users_non_admin_has_approver;
ALTER TABLE users DROP CONSTRAINT users_approver_not_self;
DROP INDEX IF EXISTS idx_users_approver;
ALTER TABLE users DROP COLUMN approver_id;

-- Phase 4: Update reopen_requests table
-- Rename approver_id to reviewed_by and make it nullable (stores who approved/rejected)
ALTER TABLE reopen_requests RENAME COLUMN approver_id TO reviewed_by;
ALTER TABLE reopen_requests ALTER COLUMN reviewed_by DROP NOT NULL;

-- Recreate the index with the new column name
DROP INDEX IF EXISTS idx_reopen_requests_approver_status;
CREATE INDEX IF NOT EXISTS idx_reopen_requests_reviewed_by_status
    ON reopen_requests(reviewed_by, status, created_at DESC);

-- Add a check constraint to ensure reviewed_by is only set for non-pending requests
ALTER TABLE reopen_requests ADD CONSTRAINT reopen_requests_reviewed_by_pending
    CHECK (reviewed_by IS NULL OR status IN ('approved', 'auto_approved', 'rejected'));

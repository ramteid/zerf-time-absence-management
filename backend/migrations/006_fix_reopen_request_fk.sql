-- reopen_requests.reviewed_by was originally named approver_id (migration 001).
-- PostgreSQL kept the FK constraint name reopen_requests_approver_id_fkey even after
-- the column was renamed in migration 002. Migration 005 tried to drop the wrong name
-- (reopen_requests_reviewed_by_fkey), so the old RESTRICT constraint was never removed.
-- After 005 the column had two FKs: the old RESTRICT one and a new SET NULL one.
-- Deleting a user who reviewed any reopen request would fail on the RESTRICT guard.

ALTER TABLE reopen_requests DROP CONSTRAINT IF EXISTS reopen_requests_approver_id_fkey;
ALTER TABLE reopen_requests DROP CONSTRAINT IF EXISTS reopen_requests_reviewed_by_fkey;
ALTER TABLE reopen_requests ADD CONSTRAINT reopen_requests_reviewed_by_fkey
    FOREIGN KEY (reviewed_by) REFERENCES users(id) ON DELETE SET NULL;

-- Add archiving support: archived users are soft-deleted with a timestamp.
-- Archived implies deactivated: archive sets active=FALSE, archived_at=NOW().
-- Restore sets active=TRUE, archived_at=NULL.
ALTER TABLE users ADD COLUMN archived_at TIMESTAMPTZ;

-- Partial index for quick lookups of archived users only.
CREATE INDEX idx_users_archived ON users(archived_at) WHERE archived_at IS NOT NULL;

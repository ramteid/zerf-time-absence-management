-- Per-user opt-out of the time-entry submission approval workflow. When TRUE,
-- a user's submitted weeks are auto-approved (draft -> approved directly, no
-- 'submitted' stop). Mirrors allow_reopen_without_approval. Both auto-approval
-- paths are intentionally silent: no notifications or emails are sent to the
-- requester or to approvers.
ALTER TABLE users
  ADD COLUMN allow_submission_without_approval BOOLEAN NOT NULL DEFAULT FALSE;

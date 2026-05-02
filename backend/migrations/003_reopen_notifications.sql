-- Reopen-week feature: approver assignment, per-approver auto-approve policy,
-- weekly reopen requests, and persistent in-app notifications.
--
-- Design notes:
--   * Employees MUST have an approver. Team-leads/admins MAY but are not
--     required to (they can self-service their own weeks).
--   * The policy "may reopen without approval" is approver-scoped and lives
--     directly on `users` for the lead/admin row (column nullable for plain
--     employees who never approve anything).
--   * `reopen_requests.week_start` is always the Monday (ISO) of the week.
--     A unique partial index prevents duplicate concurrent pending requests.

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS approver_id BIGINT REFERENCES users(id),
    ADD COLUMN IF NOT EXISTS allow_reopen_without_approval BOOLEAN NOT NULL DEFAULT FALSE;

-- Enforce that every active employee row has an approver.  Leads/admins are
-- exempt (they can either be self-approving or have an explicit approver).
-- We do NOT enforce on inactive users so existing rows from before this
-- migration aren't broken — admin must set approver_id before re-activation.
ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_employee_has_approver;
ALTER TABLE users
    ADD CONSTRAINT users_employee_has_approver
    CHECK (role <> 'employee' OR active = FALSE OR approver_id IS NOT NULL);

-- Approver must reference a lead/admin row.  Enforced at app level (Postgres
-- doesn't support cross-row FK constraints inline); kept as a comment.
-- The application validates: target.role IN ('team_lead','admin') AND target.active.

-- Approver cannot point at self for employees (would be meaningless).
ALTER TABLE users
    DROP CONSTRAINT IF EXISTS users_approver_not_self;
ALTER TABLE users
    ADD CONSTRAINT users_approver_not_self
    CHECK (approver_id IS NULL OR approver_id <> id);

CREATE INDEX IF NOT EXISTS idx_users_approver ON users(approver_id);


CREATE TABLE IF NOT EXISTS reopen_requests (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    week_start DATE NOT NULL,
    approver_id BIGINT NOT NULL REFERENCES users(id),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','approved','auto_approved','rejected')),
    reviewed_at TIMESTAMPTZ,
    rejection_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (EXTRACT(ISODOW FROM week_start) = 1)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_reopen_requests_pending_unique
    ON reopen_requests(user_id, week_start)
    WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_reopen_requests_approver_status
    ON reopen_requests(approver_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reopen_requests_user
    ON reopen_requests(user_id, created_at DESC);


CREATE TABLE IF NOT EXISTS notifications (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT,
    reference_type TEXT,
    reference_id BIGINT,
    is_read BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_notifications_user_unread
    ON notifications(user_id, is_read, created_at DESC);

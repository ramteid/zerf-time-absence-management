# Reopen Week Feature – Plan Review & Gaps

## Identified Gaps & Refinements

### 1. Mixed statuses during reopen
The bulk reset to `draft` must handle ALL non-draft entries for the week (`submitted`, `approved`, `rejected`), not just `submitted`/`approved`.

### 2. DB-level duplicate prevention
Add a `UNIQUE` partial index `ON reopen_requests(user_id, week_start) WHERE status = 'pending'` to prevent concurrent duplicate requests.

### 3. Orphaned change_requests
When a week is reopened, any open `change_requests` for entries in that week become moot. Auto-cancel them with a system reason.

### 4. Approver self-service
Team leads and admins should be able to reopen their own weeks without a request (auto-approve).

### 5. Approver deactivation
If an approver is deactivated while a reopen request is pending, admin should be able to resolve it, or pending requests get auto-reassigned.

### 6. Migration numbering
New migration: `003_reopen_notifications.sql`.

### 7. SMTP crate
Add `lettre` to `Cargo.toml`. Use `tokio::spawn` for fire-and-forget email. Errors logged, never block the business flow.

### 8. Polling optimization
Default 60s interval. Skip polling when `document.hidden` is true.

### 9. Notification retention
Add "mark all as read" and "delete all" actions. Consider auto-cleanup of notifications older than 90 days (cron or on-read cleanup).

### 10. Empty week guard
Validate that the week has non-draft entries before allowing a reopen request.

## Suggested Migration 003 Schema

```sql
ALTER TABLE users ADD COLUMN approver_id BIGINT REFERENCES users(id);

CREATE TABLE approver_policies (
  approver_id BIGINT PRIMARY KEY REFERENCES users(id),
  allow_reopen_without_approval BOOLEAN NOT NULL DEFAULT FALSE,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE reopen_requests (
  id BIGSERIAL PRIMARY KEY,
  user_id BIGINT NOT NULL REFERENCES users(id),
  week_start DATE NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending','approved','rejected','auto_approved')),
  approver_id BIGINT NOT NULL REFERENCES users(id),
  reviewed_at TIMESTAMPTZ,
  rejection_reason TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX idx_reopen_pending
  ON reopen_requests(user_id, week_start) WHERE status = 'pending';

CREATE TABLE notifications (
  id BIGSERIAL PRIMARY KEY,
  user_id BIGINT NOT NULL REFERENCES users(id),
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  body TEXT,
  reference_type TEXT,
  reference_id BIGINT,
  read BOOLEAN NOT NULL DEFAULT FALSE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_notif_user ON notifications(user_id, read, created_at DESC);
```

## Verdict

The phased plan is architecturally sound. The gaps above are edge-case refinements, not structural changes. They fit naturally into the existing phases.

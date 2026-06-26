# User Archiving Implementation Plan

## Current State

- Users have an `active` boolean column. Deactivation sets `active=FALSE`, blocks login, kills sessions.
- Hard deletion (`DELETE FROM users WHERE id=$1`) exists via `DELETE /api/v1/users/{id}` and cascades all owned data (time entries, absences, reopen requests, notifications, sessions, annual leave).
- `reviewed_by` and `audit_log.user_id` use `ON DELETE SET NULL` to preserve audit trail.
- Both deactivation and deletion require reassigning approver dependents first.
- Admin user list ([`find_all_ordered`](backend/src/repository/users.rs:128)) returns ALL users (including inactive). Most other queries filter `active=TRUE`.
- Frontend [`AdminUsers.svelte`](frontend/src/routes/AdminUsers.svelte) shows all users with deactivate/delete buttons.

## Design: `archived_at` Column

Add a nullable `TIMESTAMPTZ` column `archived_at` to the `users` table. This is orthogonal to `active`:

| State | `active` | `archived_at` |
|-------|----------|----------------|
| Normal active user | TRUE | NULL |
| Deactivated (temp) | FALSE | NULL |
| Archived | FALSE | NOT NULL |
| Restored | TRUE | NULL |

Archived implies deactivated. The archive operation sets both `active=FALSE` and `archived_at=NOW()`. Restore clears both.

---

## 1. Database Migration (027_user_archive.sql)

```sql
ALTER TABLE users ADD COLUMN archived_at TIMESTAMPTZ;
CREATE INDEX idx_users_archived ON users(archived_at) WHERE archived_at IS NOT NULL;
```

No FK changes needed - archived users keep all their data in place.

---

## 2. Backend Repository Layer

### 2.1 Guard: Default Archived Exclusion

Every existing query that currently filters `active=TRUE` already excludes archived users (since archive sets `active=FALSE`). The key additions:

- [`find_all_ordered`](backend/src/repository/users.rs:128) (admin user list) currently returns ALL users including inactive. Change to `WHERE archived_at IS NULL` by default. Add a new `find_all_including_archived` for the archived-users admin view.
- [`find_by_email`](backend/src/repository/users.rs:98) - used by login. Already returns archived users (no active filter). Login handler already checks `active` and rejects. No change needed.
- [`find_by_id`](backend/src/repository/users.rs:108) - used broadly. Keep as-is (needed for audit/reports referencing archived users by ID). Add `find_by_id_non_archived` if needed.
- [`find_for_approver_including_inactive`](backend/src/repository/users.rs:156) - add `AND archived_at IS NULL` so archived assistants do not show in team-lead views.

### 2.2 New Repository Methods

| Method | SQL |
|--------|-----|
| `archive_tx(tx, id)` | `UPDATE users SET active=FALSE, archived_at=NOW() WHERE id=$1` |
| `restore_tx(tx, id, new_start_date)` | `UPDATE users SET active=TRUE, archived_at=NULL, start_date=COALESCE($2, start_date) WHERE id=$1` |
| `find_archived_ordered()` | `{USER_SELECT} WHERE archived_at IS NOT NULL ORDER BY archived_at DESC` |
| `has_time_data(tx, id)` | `SELECT EXISTS(SELECT 1 FROM time_entries WHERE user_id=$1) OR EXISTS(SELECT 1 FROM absences WHERE user_id=$1)` |

### 2.3 Queries Needing `archived_at IS NULL` Addition

These queries currently show ALL users (no `active=TRUE` filter) and must exclude archived:

- [`find_all_ordered`](backend/src/repository/users.rs:128) - admin user list
- [`find_for_approver_including_inactive`](backend/src/repository/users.rs:156) - team lead assistant view

---

## 3. Backend Service Layer

### 3.1 Archive Service (`services::users::archive`)

```
archive(app_state, requester_id, target_id, replacement_approver_map) -> AppResult<()>
```

Steps within a single transaction (user-graph lock held):
1. Fetch target user, verify not self, not last admin
2. Check if target is approver for active users - if yes, `replacement_approver_map` must cover all of them. Reassign approvers within the tx.
3. Auto-reject all pending absences owned by the archived user (status: requested/cancellation_pending)
4. Auto-reject all pending reopen requests owned by the archived user
5. Future planned absences (approved, start_date > today) remain unchanged per requirements
6. Set `active=FALSE, archived_at=NOW()`
7. Delete all sessions for the user
8. Commit, then audit log

### 3.2 Restore Service (`services::users::restore`)

```
restore(app_state, requester_id, target_id, new_start_date, approver_ids) -> AppResult<User>
```

Steps:
1. Fetch target, verify `archived_at IS NOT NULL`
2. Validate approver_ids (non-admin must have at least one)
3. Optionally reset `start_date` to `new_start_date` (avoids flextime gap)
4. Set `active=TRUE, archived_at=NULL`
5. Set approvers
6. Set `must_change_password=TRUE` (security best practice)
7. Commit, audit log

### 3.3 Delete Guard

Modify [`delete_user`](backend/src/handlers/users.rs:663) handler: before hard-deleting, check `has_time_data`. If true, return `400 "User has historical data. Use archive instead."`. Hard delete only permitted for users with zero time entries and zero absences.

---

## 4. Backend Handlers and Router

### 4.1 New Endpoints

| Method | Path | Handler | Auth |
|--------|------|---------|------|
| POST | `/users/{id}/archive` | `archive_user` | Admin |
| POST | `/users/{id}/restore` | `restore_user` | Admin |
| GET | `/users/archived` | `list_archived` | Admin |

### 4.2 API Contracts

**POST /users/{id}/archive**
```json
Request: {
  "approver_replacements": { "3": 5, "7": 5 }
}
// Keys = user IDs currently approved by target; values = new approver IDs.
// Required only if target is approver for active users. Empty object if not.

Response: { "ok": true }
// 400 if target is approver and replacements missing/invalid
```

**POST /users/{id}/restore**
```json
Request: {
  "start_date": "2026-06-23",   // optional; null = keep original
  "approver_ids": [2, 5]        // required for non-admin
}

Response: { "id": 4, "email": "...", ... }  // full user object
```

**GET /users/archived**
```json
Response: [
  { "id": 4, "email": "...", "first_name": "...", "last_name": "...",
    "role": "employee", "archived_at": "2026-06-20T10:00:00Z" }
]
```

### 4.3 Router Changes

Add routes in [`router.rs`](backend/src/router.rs) under the `/users` nest.

---

## 5. Backend Background Tasks

All background tasks already filter `active=TRUE`, which excludes archived users. Verify each:

- [`submission_reminders.rs`](backend/src/background/submission_reminders.rs) - queries [`active_users_for_reminders`](backend/src/repository/users.rs:998) which has `active=TRUE`. OK.
- [`approval_reminders.rs`](backend/src/background/approval_reminders.rs) - queries pending approvals joined with active approvers. OK.
- [`report_upload.rs`](backend/src/background/report_upload.rs) - uses `timesheet_members_for_period` which includes deactivated users who had entries. Archived users with entries in the period should still appear here for completeness. No change needed.
- [`system_alerts.rs`](backend/src/background/system_alerts.rs) - verify no archived user notifications are created.

---

## 6. Reports

- [`timesheet_members_for_period`](backend/src/repository/reports.rs:282) already includes deactivated users who had entries/absences in the period. Archived users with historical data in a report period should appear. No change needed.
- Standard report user dropdowns (active users) already filter `active=TRUE`. OK.
- Team report, category report, overtime report - all filter active users. OK.

---

## 7. Frontend Changes

### 7.1 AdminUsers.svelte

- Remove "Delete" button for users who have time data. Replace with "Archive" button.
- Keep "Delete" only for users with no time data (fresh accounts).
- Add "Archived Users" tab/section with restore capability.
- Archive button: if user is approver, show dialog to reassign dependents before confirming.
- Restore button: show dialog with optional start-date reset and approver assignment.

### 7.2 New/Modified Dialogs

- `ArchiveDialog.svelte` - shows dependents needing reassignment, confirmation
- `RestoreDialog.svelte` - start date option, approver selection

### 7.3 API Layer

Add to [`api.js`](frontend/src/api.js) or use existing `api()` directly:
- `POST /users/{id}/archive`
- `POST /users/{id}/restore`
- `GET /users/archived`

### 7.4 i18n

Add translations in [`i18n.js`](frontend/src/i18n.js) and [`i18n.rs`](backend/src/i18n.rs):
- Archive/restore labels, confirmations, error messages
- "User has historical data" guard message
- Archived user list headers

---

## 8. Edge Cases and Invariants

1. **Self-archive**: blocked (same as self-deactivate/delete)
2. **Last admin**: blocked (same guard as deactivation)
3. **Approver chain**: archive requires all active dependents to be reassigned first. The `approver_replacements` map handles this atomically.
4. **Pending absences**: auto-rejected on archive. Notification sent to the user's approvers.
5. **Future approved absences**: preserved per requirements. If the archived user is later restored, these absences remain valid.
6. **Pending reopen requests**: auto-rejected on archive.
7. **Notifications for archived user**: existing notifications remain but no new ones are created (active=FALSE guards this).
8. **Email uniqueness**: archived user's email remains in the `users` table. Creating a new user with the same email is blocked. This is intentional - restore the archived user instead.
9. **Name uniqueness**: same as email - the unique constraint on (first_name, last_name) still applies.
10. **Flextime gap on restore**: optional `start_date` reset avoids accumulated negative flextime from the gap period.
11. **Login**: already blocked by `active=FALSE` check in auth handler.
12. **CSRF/sessions**: cleared on archive, fresh session on restore (via normal login).

---

## 9. Critical Files Requiring Changes

| File | Rationale |
|------|-----------|
| `backend/migrations/027_user_archive.sql` | New column + index |
| [`backend/src/repository/users.rs`](backend/src/repository/users.rs) | New methods, archived exclusion in `find_all_ordered` and `find_for_approver_including_inactive` |
| [`backend/src/services/users.rs`](backend/src/services/users.rs) | Archive/restore business logic |
| [`backend/src/handlers/users.rs`](backend/src/handlers/users.rs) | New handlers, delete guard |
| [`backend/src/router.rs`](backend/src/router.rs) | New routes |
| [`backend/src/repository/absences.rs`](backend/src/repository/absences.rs) | Auto-reject pending absences method |
| [`backend/src/repository/reopen_requests.rs`](backend/src/repository/reopen_requests.rs) | Auto-reject pending reopen requests method |
| [`backend/src/i18n.rs`](backend/src/i18n.rs) | Backend translations |
| [`frontend/src/routes/AdminUsers.svelte`](frontend/src/routes/AdminUsers.svelte) | Archive/restore UI, archived list tab |
| `frontend/src/dialogs/ArchiveDialog.svelte` | New dialog for approver reassignment |
| `frontend/src/dialogs/RestoreDialog.svelte` | New dialog for start date + approver |
| [`frontend/src/i18n.js`](frontend/src/i18n.js) | Frontend translations |
| [`docs/user-guide.md`](docs/user-guide.md) | Document archive/restore feature |
| Integration tests | Archive/restore scenarios, delete guard |

---

## 10. Risks

1. **Report correctness**: archived users must still appear in historical reports. The existing `timesheet_members_for_period` query handles this since it checks for actual data presence, not active status. Verify all report paths.
2. **Approver reassignment atomicity**: the archive tx must reassign ALL dependents or fail entirely. Partial reassignment would leave orphaned users.
3. **Future absence leave balance**: approved future absences for archived users still count against leave balance. On restore, balance calculations must include these. Already works since balance queries use user_id, not active status.
4. **Legal retention**: archiving satisfies data retention requirements better than deletion. The `archived_at` timestamp provides an audit trail of when the user was archived.
5. **Email/name collision on restore**: no issue since the user row persists. But if an admin manually created a workaround user with a similar name, the unique constraint on names could block restore if the name was reused (unlikely given the constraint prevents it during archive).
6. **Background report upload**: [`report_upload.rs`](backend/src/background/report_upload.rs) includes deactivated users with period data. Archived users are a subset of deactivated. Should still appear in period exports. Verify.
7. **Dashboard widgets**: team dashboards filter by active users. Archived users excluded correctly.

---

## 11. Implementation Order (Todo List)

1. Database migration (027_user_archive.sql)
2. Repository: add `archived_at` to User struct, new methods, update `find_all_ordered` and `find_for_approver_including_inactive`
3. Repository: add auto-reject methods for absences and reopen requests
4. Service: archive and restore logic
5. Handlers: archive, restore, list-archived endpoints; delete guard
6. Router: wire new routes
7. i18n: backend translations
8. Frontend: AdminUsers archived tab, archive/restore dialogs
9. Frontend: i18n translations
10. Integration tests
11. User guide update

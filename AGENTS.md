# Zerf - Agent Reference

Zerf (Zeiterfassung) is a self-hosted time tracking and absence management platform for teams. It covers working hours, leave requests, approvals, and monthly reports. Data stays on your infrastructure.

## Development Workflow

- All development happens on `main`.
- Do not create feature branches or pull requests unless explicitly requested.

## Repository Layout

```
backend/      Rust/Axum HTTP API + PostgreSQL integration
frontend/     Svelte 5 single-page app
docker/       Docker Compose configurations and Dockerfiles
migrations/   SQL migrations (backend/migrations/)
scripts/      Backup utility
```

## Backend

**Language/Runtime**: Rust (Edition 2021), async Tokio multi-thread runtime
**Framework**: Axum 0.8
**Database**: PostgreSQL via sqlx 0.8 (compile-time checked queries, built-in migrations)
**Crate name**: `zerf`

### Key dependencies

| Crate | Purpose |
|-------|---------|
| axum + tower | HTTP routing and middleware |
| sqlx | PostgreSQL queries and migrations |
| argon2 + subtle | Password hashing and constant-time comparison |
| rand | CSPRNG (session tokens) |
| lettre | SMTP email delivery |
| reqwest | External holiday API calls |
| chrono | Date/time |
| csv | Report CSV export |
| tracing | Structured logging |
| testcontainers | Postgres containers for integration tests |

### Architecture: 3-layer structure

The backend is organised into three strict layers. See `ARCHITECTURE.md` for the full spec.

```
handlers/ → services/ → repository/
```

| Layer | Location | Rule |
|-------|----------|------|
| **Handlers** | `src/handlers/*.rs` | HTTP only. Extract request, call service, return JSON. No `sqlx`, no `repository` imports. |
| **Services** | `src/services/*.rs` | Business logic. Own transactions, dispatch notifications. No `axum::extract/response`. |
| **Repository** | `src/repository/*.rs` | SQL only. No business rules. Only `AppError::NotFound` via `From<sqlx::Error>`. |

Additional modules:

| Module | Purpose |
|--------|---------|
| `middleware/auth.rs` | `auth_middleware`, `User` struct, cookie/token/CSRF helpers — single source of `User` |
| `background/` | Scheduled loops: submission reminders, approval reminders, holiday seeding, monthly timesheet upload, monthly payroll report |
| `state.rs` | `AppState` definition |
| `router.rs` | Route declarations (`build_api_router`, `build_app`) |
| `config.rs` | Environment variable loading |
| `db.rs` | Connection pool setup |
| `error.rs` | `AppError`, `AppResult` |
| `audit.rs` | Audit log dispatch |
| `email.rs` | SMTP delivery via lettre |
| `i18n.rs` | Backend translations |
| `time_calc.rs` | Time duration helpers |

**Key types:**

| Type | Location | Role |
|------|----------|------|
| `AppState` | `state.rs` | Holds `pool`, `db` (repo façade), `cfg`, `notifications` |
| `User` | `middleware/auth.rs` | Authenticated requester extracted by `auth_middleware` |
| `repository::Db` | `repository/mod.rs` | Façade owning all sub-repositories |
| `*Db` (e.g. `UserDb`) | `repository/*.rs` | Domain-specific query collections |

**Sub-repositories** (fields on `repository::Db`):

`sessions`, `users`, `time_entries`, `flextime_adjustments`, `absences`, `reopen_requests`, `categories`, `holidays`, `notifications`, `audit`, `settings`, `reports`, `export_queue`, `payroll_queue`, `error_queue`, `email_queue`

**Access patterns in services:**

```rust
// Simple reads via the façade
let entries = app_state.db.time_entries.list_for_user(user_id, from, to).await?;

// Transaction-bound writes (services own the transaction lifecycle)
let mut tx = app_state.db.users.begin().await?;
SubDb::method_tx(&mut *tx, ...).await?;
tx.commit().await?;
// Dispatch notifications AFTER commit:
services::notifications::create(...).await?;

// Standalone context (background tasks)
let user = UserDb::new(pool.clone()).find_by_id(id).await?;
```

**Type conversion:** Repository structs are converted to service/response types via `repo_*_to_service()` helpers located in the relevant service module (e.g. `services::users::repo_user_to_auth_user()`).

**Rules:**
- SQL is allowed only in `backend/src/repository/*.rs` (plus `db.rs` bootstrap).
- Handlers must not import `sqlx` or `crate::repository`.
- Services must not import `axum::extract`, `axum::response`, or `axum::routing`.
- All new database operations must go through repository methods.

### Background tasks (spawned in main.rs)

- Auth cleanup: purge expired sessions and login attempts (hourly)
- Notification cleanup: delete notifications older than 90 days (daily)
- Holiday scheduler: ensure current and next year holidays exist (weekly, Monday noon)
- Submission reminder scheduler
- Monthly timesheet PDF upload to Nextcloud (daily, after midnight)
- Monthly payroll report email to the tax office (daily, after midnight)
- Error-notification worker: drains `error_notification_queue` and alerts opted-in admins in-app + by email (poll every 10s)
- Email queue worker: drains `email_queue` and delivers via SMTP, guarded by a shared circuit breaker (poll every 2 minutes)

Both monthly jobs share `background/schedule.rs` (daily loop, `YYYY-MM` period
math, queue backfill through the previous month, day-of-month deferral) and the
`services::reports::month_export_readiness` gate, so they judge "this month is
final" by the same rules.

**Email delivery** (`email.rs`, `background/email_queue.rs`): almost every
outbound email (password resets, absence decisions, reminders, error alerts)
goes through `services::notifications::deliver` → `email::queue_email`, which
persists the already-rendered subject/body to `email_queue` rather than
sending it inline. The background worker above drains that table every 2 minutes (new messages
first, then previously-failed ones least-recently-retried first — so one
undeliverable message can't monopolize the circuit breaker's retry slot and
starve everything queued behind it) and deletes a row only once SMTP
confirmed delivery — a message that keeps failing simply stays queued and is
retried indefinitely, so a transient SMTP outage can no longer silently lose
an email. That delete is itself retried a few times (idempotent — deleting an
already-gone row is a no-op) before giving up, so a momentary DB hiccup right
after a confirmed send can't leave the row looking untouched and cause the
same email to go out again next cycle; the payroll report's own period
delete gets the identical treatment for the same reason. Enqueueing
itself is gated on `SettingsDb::load_smtp_config()` returning `Some`
(SMTP enabled and fully configured); if SMTP is disabled after messages are
already queued, they are left in place untouched rather than dropped or
warned about. A shared `email::CircuitBreaker` (5 consecutive failures opens
it; a 5-minute cooldown then grants one half-open trial) guards every real
SMTP attempt so a longer outage stops being retried on every poll; the
breaker is shared with the payroll report's own `send_with_attachment` call so
both paths back off together. A row is only logged as a system warning (not
raised as an admin notification, to avoid emailing about email being broken)
once it has failed 100 delivery attempts — logged once at that threshold, not
repeated on every attempt after. The one exception that bypasses the queue
entirely is the monthly payroll report PDF, which already has its own
period-keyed retry queue (`payroll_report_queue`) with "stays queued until
confirmed sent" semantics; it still routes its actual SMTP transaction
through the same breaker-guarded sender. The admin's SMTP "test connection"
probe also bypasses both the queue and the breaker deliberately — it never
sends a real message and must not be blocked by unrelated breaker state.

### Configuration (environment variables)

| Variable | Required | Default | Purpose |
|----------|----------|---------|---------|
| `ZERF_DATABASE_URL` | yes | - | PostgreSQL connection string |
| `ZERF_SESSION_SECRET` | yes | - | >= 32 chars random secret (`openssl rand -hex 32`) |
| `ZERF_BIND` | no | `0.0.0.0:3333` | HTTP listen address |
| `ZERF_STATIC_DIR` | no | `static` | Frontend asset directory |
| `ZERF_PUBLIC_URL` | no | - | Public HTTPS URL (password reset links, CORS) |
| `ZERF_ALLOWED_ORIGINS` | no | derived | Comma-separated CORS origins |
| `ZERF_DEV` | no | false | Dev mode: disables secure cookies and CSRF |
| `ZERF_SECURE_COOKIES` | no | !DEV | Require HTTPS for cookies |
| `ZERF_ENFORCE_CSRF` | no | !DEV | Enforce CSRF double-submit tokens |
| `ZERF_ENFORCE_ORIGIN` | no | true if origins set | Enforce Origin/Referer checking |
| `ZERF_TRUST_PROXY` | no | true | Trust X-Forwarded-* headers |

`ZERF_SESSION_SECRET` is rejected at startup if it contains placeholder values like `please-change` or `change-me`.

### Database schema (key tables)

| Table | Purpose |
|-------|---------|
| `users` | Users, approver hierarchy, weekly hours, start date |
| `sessions` | Hashed session tokens, CSRF tokens, activity timestamps |
| `login_attempts` | Failed login tracking for rate-limit lockout |
| `categories` | Work categories |
| `time_entries` | Daily entries (date, start/end, category, status) |
| `flextime_adjustments` | Dated, signed changes to a flextime balance that no worked time explains (carry-in balance, admin corrections) |
| `absences` | Absence requests with status workflow |
| `holidays` | Public holidays (auto-fetched or manual) |
| `reopen_requests` | Requests to reopen a submitted week |
| `payroll_report_queue` | Months whose payroll report PDF still has to be emailed |
| `error_notification_queue` | Technical-error events awaiting fan-out to opted-in admins |
| `email_queue` | Outbound emails awaiting SMTP delivery (attempts, last error) |
| `notifications` | Per-user in-app notifications |
| `app_settings` | Key-value app settings |
| `audit_log` | Before/after JSON snapshots of all mutations |
| `password_reset_tokens` | One-time hashed tokens (1h expiry) |
| `user_leave_accounts` | Per-user base entitlement for each leave-account absence category |
| `user_leave_account_year_overrides` | Per-user leave-account entitlement overrides by year |

Notable constraints: non-admin users must have an approver; users cannot approve themselves; vacation range <= 1 year; time entry end_time >= start_time; at most one `opening_balance` row and at most one reversal per row in `flextime_adjustments`.

**Flextime balances.** A balance is `sum(worked - target) through the flextime
cutoff + sum(flextime_adjustments effective on or before the date asked for)`.
The carry-in balance used to live in `users.overtime_start_balance_min`, where
editing it silently rewrote the employee's whole reported history; migration 043
moved every value into `flextime_adjustments` and then dropped the column, so
there is no second, writable copy of the same fact left anywhere. That makes the
migration one-way: an older binary still selects the column and cannot start
against a migrated database. Adjustments are **not** capped at the
flextime cutoff — an admin booking is authoritative immediately — and one dated
before a user's start date is pulled forward to that date, so moving a start
date relocates it instead of dropping it. The table is **append-only**: rows are
never updated or deleted, and a wrong entry is cancelled by a row carrying the
opposite minutes on the same date (`reverses_id`). Deleting would reintroduce
the original defect, so there is no delete endpoint. Effective dates may lie in
the future; every balance query asks "effective on or before date X", so a
future booking simply has not applied yet.

### Build

```
# Development
cargo build

# Production (strip + thin LTO)
cargo build --release
```

## Frontend

**Framework**: Svelte 5.55.5
**Build tool**: Vite 8.0.10
**Test runner**: Vitest 4.1.5 + jsdom
**Linter**: ESLint 10 + eslint-plugin-svelte (covers JS and `.svelte` files)
**Dev server port**: 5173 (proxies `/api` and `/healthz` to `http://127.0.0.1:3333`)
**Build output**: `frontend/dist/`

### NPM scripts

| Script | Command | Purpose |
|--------|---------|---------|
| `dev` | `vite` | Start dev server |
| `build` | `vite build` | Production build |
| `lint` | `eslint .` | Lint all JS and Svelte files |
| `format` | `prettier --check` | Check formatting |
| `format:write` | `prettier --write` | Auto-format |
| `test` | `vitest run` | Run tests |

### Linting

ESLint is configured via `frontend/eslint.config.js` and covers **both** `.js` and `.svelte` files using `eslint-plugin-svelte`.

**Run before committing:**
```bash
cd frontend
npm run lint
```

**Key rules in effect:**
- `no-unused-vars` / `no-unused-imports` — remove dead imports/variables
- `svelte/require-each-key` — every `{#each}` block must have a key expression `(item.id)`
- `no-dupe-keys` — no duplicate keys in object literals (catches i18n mistakes)
- `svelte/no-immutable-reactive-statements` — don't write `$:` blocks whose inputs never change

**Intentional suppressions (do not remove):**
- `svelte/prefer-svelte-reactivity` — disabled globally; using native `Map`/`Set`/`Date` is acceptable
- `svelte/no-reactive-functions` — disabled; Svelte 4-era rule that crashes on ESLint 10
- `<!-- eslint-disable-next-line svelte/no-at-html-tags -->` in `Icons.svelte` — SVG icon content is trusted static markup
- `// eslint-disable-next-line no-useless-assignment` on reactive tracker variables — ESLint cannot see cross-reactive-statement usage (e.g. `$: lastX = x;` paired with `$: if (x !== lastX) { ... }`)
- `// eslint-disable-next-line svelte/infinite-reactive-loop` — false positives when assignments occur inside `.then()` callbacks within `$:` blocks

### Key source files

| File | Purpose |
|------|---------|
| `src/api.js` | Fetch wrapper: CSRF header injection, 401/session-expiry handling, error mapping |
| `src/stores.js` | Svelte stores: current user, categories, routing path, notifications |
| `src/i18n.js` | Translation tables (en, de), localStorage preference |
| `src/App.svelte` | Root component, boot logic, session expiry gate |
| `src/Layout.svelte` | Main layout |
| `src/apiMappers.js` | Response-to-domain object mapping |
| `src/dialogs/` | Modal dialogs (AbsenceDialog, EntryDialog, CategoryDialog, etc.) |
| `src/routes/` | Page components (Time, Absences, Calendar, Reports, Admin*, Account) |
| `src/styles/` | Global stylesheet modules, imported in order by `index.css` |

### Styling

- Global styles are split into ordered modules under `src/styles/` (tokens,
  base, buttons, badges, forms, layout, components, pages, feedback,
  notifications, responsive). `index.css` imports them in cascade order —
  `responsive.css` must stay last so its media queries win.
- **No inline `style=` attributes.** Shared/repeated patterns belong in the
  matching `src/styles/` module; page- or component-specific one-offs belong in
  that component's scoped `<style>` block. Truly dynamic values (colors from
  data, computed positions) use Svelte `style:property={value}` directives.
- Font sizes are declared in `rem`. The root size in `base.css` is the single
  knob for the type scale: 100% (16px) as default, raised to 106.25% (17px)
  on desktop viewports (>1024px). Never hardcode `px` font sizes.
- `base.css` provides small utilities for the most common one-liners
  (`.flex-1`, `.text-right`, `.text-tertiary`, `.fs-14`, `.mt-8`/`.mb-12` etc.)
  plus `.zf-row`/`.zf-col` stacks - reuse them before writing a new class.
- Page content is width-capped and centered via `--page-max-width` (default
  1200px), applied as horizontal padding on `.top-bar` and `.content-area`.
  Form-heavy pages narrow it with the `.page-narrow` (640px) or `.page-medium`
  (760px) class set on **both** elements so title and content stay aligned.

### i18n

Supported languages: `en` (en-US) and `de` (de-DE). Stored in localStorage key `zerf.ui-language`. Default: English. Locale used for `Intl` date/time formatting.

### API integration

- Base URL: `/api/v1` (relative to origin)
- CSRF token received from `GET /auth/me` or login response; sent as `X-CSRF-Token` header
- 401 triggers session-expiry handler (except on auth endpoints); a gate prevents duplicate handlers from concurrent requests
- `ZERF_FRONTEND_DEBUG_BUILD=true` disables minification and adds sourcemaps

## API routes (summary)

```
/auth/*             Login, logout, setup, forgot/reset password, preferences
/time-entries/*     CRUD, submit, batch-approve, batch-reject
/flextime-adjustments/{id}/reverse  Cancel a flextime entry out (admin; no delete exists)
/absences/*         CRUD, approve, reject, revoke, calendar, leave balances
/reopen-requests/*  Create, list pending, approve/reject
/users/*            CRUD, deactivate, reset password, leave-account entitlements, flextime account
/categories/*       CRUD
/holidays/*         CRUD, country/region lists
/reports/*          Month, range, team, categories, overtime, flextime, CSV
/audit-log          Read audit history
/settings/*         Public and admin settings, uploads, payroll report
/notifications/*    List, mark read, dismiss
```

## Security model

- **Passwords**: Argon2id; 5 failed attempts per 15 min lockout
- **Sessions**: 256-bit random tokens (HttpOnly/Secure/SameSite=Strict), 4d idle / 14d absolute timeout
- **CSRF**: SameSite=Strict + Origin/Referer check + X-CSRF-Token double-submit
- **Database auth**: SCRAM-SHA-256, checksums, internal-only Docker network
- **Data at rest**: [pg_tde](https://docs.percona.com/pg-tde/) (Percona Transparent Data Encryption) encrypts all tables and WAL segments at the PostgreSQL storage layer. The pg_tde principal key is auto-generated on first start, then encrypted with `ZERF_DB_ENCRYPTION_KEY` (AES-256-CBC, PBKDF2) and stored as `pg_tde_keyring.enc` in the data volume. On each container start the custom entrypoint decrypts the blob into a Docker-managed in-memory tmpfs (`/var/lib/pg_tde_keyring`); no elevated container capabilities are needed.
- **Backups**: Each backup is a zip archive (`zerf-<ts>.zip`) whose `dump.enc` entry is AES-256-CBC encrypted (PBKDF2, 100 000 iterations) using the same `ZERF_DB_ENCRYPTION_KEY`. One key governs both layers.
- **Audit log**: All mutations logged with JSON snapshots; passwords and secrets never logged
- **Password reset**: One-time 1h tokens, forced change on first login

## Deployment

Two Docker Compose configurations in `docker/` (`docker-compose-public.yml` is an
overlay applied on top of the local file, not a standalone stack):

| File | Purpose |
|------|---------|
| `docker-compose-local.yml` | Local stack (supports `DEBUG=true` via `.env`) |
| `docker-compose-public.yml` | Public deployment overlay: adds Caddy, drops the host port |

Caddy handles HTTPS termination and serves the frontend static assets. Backend listens on port 3333.

> ⚠ **Local mode is LAN-only.** `start_local.sh` publishes the app on
> `0.0.0.0:3333` with `ZERF_SECURE_COOKIES=false` and `ZERF_ENFORCE_ORIGIN=false`
> (plaintext HTTP, no Origin enforcement; CSRF tokens are still enforced). This is
> intended for a trusted LAN only. **Never expose a local-mode host to the
> internet** — session cookies would travel in cleartext. For any internet-facing
> deployment use `start_public.sh`, which terminates TLS at Caddy and re-enables
> secure cookies and Origin enforcement.

The PostgreSQL container is built from `docker/postgres.Dockerfile` (based on `percona/percona-distribution-postgresql:18`, which bundles pg_tde). A custom entrypoint (`docker/entrypoint-postgres.sh`) decrypts the pg_tde keyring from the data volume into an in-memory tmpfs before handing off to the official postgres entrypoint. The container runs with `cap_drop: [ALL]` and re-adds only the minimal capability set its root→gosu startup needs (`CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`); `app` and `backup` run with `cap_drop: [ALL]` and no added capabilities. All three set `no-new-privileges`.

### Docker images

| Image | Dockerfile | Purpose |
|-------|-----------|---------|
| `zerf-time-absence-management` | `docker/app.Dockerfile` | Rust/Axum backend + frontend assets |
| `zerf-time-absence-management-postgres` | `docker/postgres.Dockerfile` | Percona PostgreSQL 18 with pg_tde |
| `zerf-time-absence-management-caddy` | `docker/Caddyfile.Dockerfile` | Caddy reverse proxy (built with the caddy-ratelimit module) |
| `zerf-time-absence-management-backup` | `docker/backup.Dockerfile` | PostgreSQL 18 client + curl, with `scripts/backup.sh` baked in (self-contained — no host bind-mount). Built from the repo root so the script is in the build context. |

The `backup` service in `docker-compose-local.yml` is connected to two networks:
- `backup_net` — internal network shared with `db`, required for `pg_dump`.
- `backup_egress` — non-internal network for outbound HTTPS to Nextcloud. The app container is **not** in this network.

### Start scripts

| Script | Purpose |
|--------|---------|
| `start_local.sh` | Start local stack (set `DEBUG=true` in `.env` for debug build) |
| `start_public.sh` | Start public stack |
| `scripts/backup.sh` | Dump, AES-encrypt, and bundle into a single zip archive (`zerf-<ts>.zip`, or `zerf-<ts>-manual.zip` for an on-demand backup — see `backup_requested_at` below), then optionally upload that archive to a Nextcloud share. The zip contains `dump.enc` (encrypted pg_dump), `metadata` (plaintext provenance), and — when the keyring volume is mounted at `/keyring-src` — `keyring.enc` (already-AES-encrypted pg_tde keyring for physical PGDATA recovery). Backup interval is read from `app_settings` at runtime via `psql`; local retention is a fixed count (the 10 most recent archives), tracked independently for scheduled and manual runs so repeated manual backups can never evict scheduled history. Refactored into sourceable functions (guarded by `BACKUP_LIB_ONLY=1`) for bats unit tests. |
| `scripts/restore.sh` | Interactive: extract and decrypt a backup archive, then restore it into the live instance. Supports both the new zip format (`zerf-<ts>.zip`) and legacy encrypted dumps (`zerf-<ts>.dump.enc`). `--keyring [DIR]` extracts the `keyring.enc` entry from the selected archive for physical recovery without touching the database. Container names, the backup volume, and the `.env` path default to the production values but are overridable (`ZERF_RESTORE_POSTGRES_CONTAINER`, `ZERF_RESTORE_APP_CONTAINER`, `ZERF_RESTORE_BACKUP_VOLUME`, `ZERF_RESTORE_ENV_FILE`) — used by `e2e/backup-restore-check.sh` to run it non-interactively against the isolated e2e stack. Requires `unzip` on the host. |
| `scripts/backup.bats` | bats unit tests for `backup.sh` helper functions (parse_share_url, interval resolution, upload credential handling, 0-byte rejection, zip archive creation with and without keyring, keyring-copy failure handling, retention pruning). Requires `zip` and `unzip` in the test environment. |
| `e2e/backup-restore-check.sh` | Final step of `e2e/run.sh`: triggers a real backup cycle, verifies the zip archive and its entries (dump.enc, metadata, keyring.enc), mutates the live e2e database, restores via `scripts/restore.sh`, and verifies the mutation is undone, every table's row count matches the pre-backup snapshot, and (via `e2e/post-restore-ui-check.mjs`, a real browser) the restored data renders in the app's UI. |

### Disaster recovery prerequisites

The database is encrypted at rest with pg_tde, and the keyring is wrapped with
`ZERF_DB_ENCRYPTION_KEY`. **Two distinct artifacts are required to read the data —
losing either renders the database unrecoverable:**

1. `ZERF_DB_ENCRYPTION_KEY` (from `.env`). `deploy.sh` never overwrites an existing
   key for exactly this reason.
2. The pg_tde keyring. It lives in the **`zerf_postgres_data`** volume
   (`/data/pg_tde_keyring.enc`), which is **separate** from the data directory in
   **`zerf_postgres_db_data`** (`/data/db`). A filesystem/volume snapshot that
   captures only the data volume **cannot be decrypted**.

For recovery you therefore need **either** both volumes (`zerf_postgres_db_data`
*and* `zerf_postgres_data`) **or** a logical backup archive `zerf-<ts>.zip` plus the
key. Each backup archive bundles a `keyring.enc` entry so an orphaned, encrypted
data volume can still be recovered with `scripts/restore.sh --keyring`.

### Key environment variables (encryption)

| Variable | Purpose |
|----------|---------|
| `ZERF_DB_ENCRYPTION_KEY` | Single passphrase that wraps the pg_tde keyring (DB at rest) and encrypts backups via openssl. Generate: `openssl rand -hex 32`. **Losing this key makes both the database and all backups unreadable.** |

### Backup and upload settings (app_settings)

Backup frequency, Nextcloud upload, and payroll report settings are stored in `app_settings` (not in `.env`) and are editable in the Admin UI under **Nextcloud Backups** and **Payroll Report**. The backup container reads them via `psql` at the start of each cycle. Local retention is not configurable — the 10 most recent backups are always kept, tracked separately for scheduled and manual runs (see `backup_requested_at` below).

| Key | Default | Description |
|-----|---------|-------------|
| `backup_interval_days` | 1 | Days between backup cycles |
| `backup_last_success_at` | — | UTC timestamp of the last successful **scheduled** backup; `is_backup_due` measures the interval from this. Written only by `scripts/backup.sh`, never by a manual run |
| `backup_requested_at` | — | Set by the admin's **Back up now** button (`request_backup_now`); the backup container's loop polls for a value it hasn't already handled (~every 20s, via `sleep_until_deadline_or_request` — which every sleep in the loop routes through, including the post-failure backoff) and runs an immediate backup. Never cleared by the app — the script tracks what it has handled itself (`backup_last_request_handled_at`, script-internal, no Rust constant), so a failed clear can't cause a repeat-backup loop. Not directly user-editable |
| `backup_last_manual_at` | — | UTC timestamp of the last successful **manual** backup, kept separate from `backup_last_success_at` so an on-demand backup never postpones or starves the schedule. The Admin UI shows the more recent of the two |
| `backup_upload_enabled` | false | Enable upload to Nextcloud |
| `backup_upload_url` | — | Nextcloud public share URL (`https://…/s/<token>`) |
| `backup_upload_password` | — | Optional share password (write-only) |
| `report_upload_enabled` | false | Enable monthly timesheet PDF upload |
| `report_upload_url` | — | Nextcloud public share URL for timesheets |
| `report_upload_password` | — | Optional share password (write-only) |
| `report_upload_day_of_month` | 5 | Day of month to upload previous month's PDF |
| `payroll_report_enabled` | false | Email the monthly payroll report |
| `payroll_report_recipient` | — | Comma-separated recipient addresses (tax office / payroll accountant). Singular key name, list value — see `parse_recipient_list` |
| `payroll_report_day_of_month` | 5 | Day of month the previous month's report is prepared |
| `payroll_report_include_assistant_hours` | true | List assistants' working days and hours |
| `payroll_report_include_employee_hours` | false | List all other employees' working days and hours |
| `payroll_report_excluded_users` | — | Comma-separated user IDs left out of the report entirely (they also stop blocking delivery). Admins are excluded unconditionally and never listed here |

The earlier `backup_interval_seconds`/`backup_retention_days` keys are gone: migration 023 replaced the interval with `backup_interval_days`, and migration 024 dropped the retention setting in favour of a fixed count (the 10 most recent backups). Neither is set via environment variables.

`payroll_report_absence_categories` is also gone: migration 036 removed it because the report now derives its categories automatically from `AbsenceCategory::is_payroll_relevant`. The key still appears read-only in `AdminSettingsData` so the UI can show which categories are currently included.

**Payroll report delivery rules** (`background/payroll_report.rs`): the scheduled run sends only once every covered person's month is final — a blocked month is logged, never raised as an error notification. The admin "Send now" button sends a *provisional* report covering whoever is already final, marked as partial in both the PDF and the email, and never deletes the queue entry. `GET /reports/payroll-status` (leads only) backs the dashboard tile and reuses the same member filter and readiness gate, so tile and document can never disagree; names of people outside a team lead's own team are stripped server-side. "Already delivered" is derived from the queue (period reached `payroll_report_queue_period` **and** no longer in `payroll_report_queue`) rather than a stored marker, so it is correct on installations that predate the card. The tile's amber/red split cannot be read off `MonthExportReadiness` alone: the gate returns `PendingAbsenceRequests` before it checks week submission, so `status_for_member` re-checks `all_weeks_submitted_for_month` to keep "still owes weeks" red.

### Integration tests

Integration tests use `testcontainers_modules::postgres::Postgres` (plain `postgres:17` image, no pg_tde). This is intentional: pg_tde is a deployment concern and has no effect on application logic or SQL correctness. `postgres:17` (Debian) is used rather than the module default (`11-alpine`) because lz4 TOAST compression requires PostgreSQL 14+ compiled with `--with-lz4`, which is included in the official Debian-based `postgres:17` image.

## Testing

### Frontend

```bash
cd frontend
npm run lint   # see Linting section above — must pass before committing
npm test -- --run && npm run build
```

Tests use Vitest + jsdom. Test files are co-located with source under `src/` and `src/routes/`.

> **Note:** Lint is not part of CI — run it locally before committing.

### End-to-end (browser)

```bash
./e2e/run.sh
```

A single realistic [Playwright](https://playwright.dev/) scenario in `e2e/` runs
against a **freshly provisioned, production-like Docker stack** (postgres with
pg_tde, the app, and the backup sidecar — the same services `start_local.sh`
brings up). The bash script `e2e/run.sh` is the entry point: it writes an
isolated env file with generated secrets, runs `docker compose ... up --build
--wait` under a dedicated project name (`zerf_e2e`), waits for the API, runs the
flow in `e2e/tests/full-flow.spec.js`, and always tears the stack down (`down
-v`) on exit.

The flow exercises the real UI: bootstrap the first admin → admin completes
first-run settings → admin creates an employee (reads the generated temporary
password) → employee changes the password, books time entries, submits the week,
and requests two absences → admin sees every pending request on the dashboard and
approves them. Admin and employee use separate browser contexts so both sessions
stay live at once.

Requires Docker + Node 22. First run builds images (several minutes); set
`ZERF_E2E_KEEP_UP=1` to keep the stack up for iterating with
`cd e2e && npx playwright test`. CI runs this as the `e2e` job (after the
`rust` and `frontend` jobs). See `e2e/README.md` for details.

### Backend

```bash
cd backend

# Unit tests only (no database required, ~3 s)
cargo test --lib

# Integration tests with Docker (each test gets its own container)
cargo test --test integration

# Integration tests without Docker — requires a local PostgreSQL instance
TEST_DATABASE_URL=postgres://<role>:<password>@127.0.0.1/<admin-db> cargo test --test integration
```

**Integration test isolation:** every `TestApp::spawn()` call creates a unique database
(`zerf_test_{pid}_{counter}`), migrates it, seeds it, and drops it via `cleanup()`.
Tests never share rows, ports, or sessions — parallel execution is safe.

**Parallelism:** `.cargo/config.toml` sets `test-threads = 8` by default, matching the
8-CPU dev container. Each test pool uses 3 connections max; peak usage is ~24 connections.
PostgreSQL `max_connections` must be ≥ 50 (set to 200 in the dev container).
The full suite runs in ~2 minutes.

**Running without Docker (local PostgreSQL):**

- Start PostgreSQL: `pg_ctlcluster 14 main start` (or `service postgresql start`).
- Verify: `pg_isready -h 127.0.0.1`.
- The local superuser role is `vscode` in this dev container. Enable TCP auth if needed:
  ```bash
  psql -h /var/run/postgresql -U vscode -d postgres -c "ALTER USER vscode PASSWORD 'secret';"
  ```
- Run tests:
  ```bash
  TEST_REFERENCE_DATE=2030-01-07 TEST_DATABASE_URL=postgres://vscode:secret@127.0.0.1/postgres cargo test --test integration
  ```

  > **Important:** Always set `TEST_REFERENCE_DATE=2030-01-07` (a Monday with no nearby public holidays)
  > when running locally. Without it the helpers fall back to wall-clock time and date-relative tests
  > will fail whenever today's date lands on or near a public holiday.

**Cleaning up between runs:**

Each test creates an isolated database (`zerf_test_{pid}_{counter}`) and drops it in `cleanup()`.
If a test run is killed mid-flight (e.g. Ctrl-C, OOM, crash), those databases are left behind and
accumulate over time. They do not affect correctness but they consume disk space and connections.
Drop them before the next run to start with a clean slate:

```bash
# List leftover test databases
psql -U vscode -h 127.0.0.1 postgres -c \
  "SELECT datname FROM pg_database WHERE datname LIKE 'zerf_test_%';"

# Drop all leftover test databases in one shot
psql -U vscode -h 127.0.0.1 postgres -t -c \
  "SELECT 'DROP DATABASE IF EXISTS \"' || datname || '\";' FROM pg_database WHERE datname LIKE 'zerf_test_%';" \
  | psql -U vscode -h 127.0.0.1 postgres
```

> **Note:** PostgreSQL must be restarted if it crashed mid-run (WAL recovery after an unclean
> shutdown can take up to 60 s before accepting connections):
> ```bash
> pg_ctlcluster 14 main stop -m immediate && pg_ctlcluster 14 main start
> # then wait:
> until pg_isready; do sleep 2; done
> ```

**Verification after changes:**

```bash
cargo build                              # zero compilation errors
cargo clippy -- -D warnings             # zero warnings
cargo test --lib                        # unit tests (no DB)
TEST_REFERENCE_DATE=2030-01-07 TEST_DATABASE_URL=... cargo test  # full suite including integration
grep -rn "sqlx::" backend/src/handlers/ # must be empty (no SQL in handlers)
grep -rn "axum::extract\|axum::response\|axum::routing\|axum::Json" backend/src/services/ # must be empty
```

`backend/tests/nager_contract.rs` validates the external Nager.Date holiday API contract.

## Coding Conventions

- Use explicit, descriptive variable and function names that reveal intent without requiring a comment.
- Prioritize readability for humans over brevity; code is read far more often than it is written.
- Keep functions and modules small and focused on a single responsibility.
- Reduce complexity: avoid unnecessary abstractions, indirection, and nesting.
- Prefer simple, direct solutions over clever ones. Keep it concise.
- Apply appropriate architectural patterns (e.g., handler/service/repository separation) consistently across the codebase.
- Keep database logic in repository modules only; handlers/services orchestrate business flow and call repository APIs.
- Do not introduce new `sqlx::query*` calls outside `backend/src/repository/*.rs`.
- Prefer adding repository methods over duplicating SQL in callers.
- Add comprehensive inline comments e. g. explaining decisions, intent and high-level logic.
- Translate all texts that are displayed to the user (UI, errors, E-Mail, etc.)
- Translations must be handled centrally in i18n.rs for the backend and i18n.js for the frontend.
- Frontend styling lives in CSS only — never in inline `style=` attributes. See the Styling section above for where rules belong.
- Update docs/user-guide.md to reflect the correct app behavior. It is a document meant for human users and should not contain technical background, but the mere user-view behavior. Use natural, simple and concise language.

### Migrations

- **Every migration must be idempotent.** Use `CREATE TABLE/INDEX … IF NOT EXISTS`,
  `ALTER TABLE … ADD COLUMN IF NOT EXISTS`, `INSERT … ON CONFLICT DO NOTHING`, and
  guarded `DO $$ … $$` blocks. A migration must be safe to re-run against a database
  that already has the change.
- **Never edit a migration that has already been committed/applied.** The app runs
  `sqlx::migrate!()` (`backend/src/db.rs`), which **checksums every applied migration
  on startup**; changing a committed migration's bytes triggers a `VersionMismatch`
  error and the **live application refuses to boot**. To change schema, always add a
  new, higher-numbered migration.

## Release Process

Commits follow [Conventional Commits](https://www.conventionalcommits.org/) format — git-cliff reads them to generate the changelog automatically.

Tag and push — the CI release workflow takes it from there:

```bash
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin vX.Y.Z
```

The CI release workflow (`release.yml`) then:
1. Injects the tag version into `Cargo.toml` and `package.json` (no commit)
2. Builds and pushes all four Docker images (app, postgres, caddy, backup) tagged with the version and `latest`
3. Generates the changelog via git-cliff and creates a GitHub Release with it as release notes

# Zerf - Agent Reference

Zerf (Zeiterfassung) is a self-hosted time tracking and absence management platform for teams. It covers working hours, leave requests, approvals, and monthly reports. Data stays on your infrastructure.

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
| `background/` | Scheduled loops: submission reminders, approval reminders, holiday seeding |
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

`sessions`, `users`, `time_entries`, `absences`, `reopen_requests`, `categories`, `holidays`, `notifications`, `audit`, `settings`, `reports`

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
| `absences` | Absence requests with status workflow |
| `holidays` | Public holidays (auto-fetched or manual) |
| `reopen_requests` | Requests to reopen a submitted week |
| `notifications` | Per-user in-app notifications |
| `app_settings` | Key-value app settings |
| `audit_log` | Before/after JSON snapshots of all mutations |
| `password_reset_tokens` | One-time hashed tokens (1h expiry) |
| `user_annual_leave` | Annual leave entitlement per user per year |

Notable constraints: non-admin users must have an approver; users cannot approve themselves; vacation range <= 1 year; time entry end_time >= start_time.

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
/absences/*         CRUD, approve, reject, revoke, calendar, leave balance
/reopen-requests/*  Create, list pending, approve/reject
/users/*            CRUD, deactivate, reset password, annual leave days
/categories/*       CRUD
/holidays/*         CRUD, country/region lists
/reports/*          Month, range, team, categories, overtime, flextime, CSV
/audit-log          Read audit history
/settings/*         Public and admin settings
/notifications/*    List, mark read, dismiss
```

## Security model

- **Passwords**: Argon2id; 5 failed attempts per 15 min lockout
- **Sessions**: 256-bit random tokens (HttpOnly/Secure/SameSite=Strict), 8h idle / 7d absolute timeout
- **CSRF**: SameSite=Strict + Origin/Referer check + X-CSRF-Token double-submit
- **Database auth**: SCRAM-SHA-256, checksums, internal-only Docker network
- **Data at rest**: [pg_tde](https://docs.percona.com/pg-tde/) (Percona Transparent Data Encryption) encrypts all tables and WAL segments at the PostgreSQL storage layer. The pg_tde principal key is auto-generated on first start, then encrypted with `ZERF_DB_ENCRYPTION_KEY` (AES-256-CBC, PBKDF2) and stored as `pg_tde_keyring.enc` in the data volume. On each container start the custom entrypoint decrypts the blob into a Docker-managed in-memory tmpfs (`/var/lib/pg_tde_keyring`); no elevated container capabilities are needed.
- **Backups**: Each `.dump.enc` file is AES-256-CBC encrypted (PBKDF2, 100 000 iterations) using the same `ZERF_DB_ENCRYPTION_KEY`. One key governs both layers.
- **Audit log**: All mutations logged with JSON snapshots; passwords and secrets never logged
- **Password reset**: One-time 1h tokens, forced change on first login

## Deployment

Three Docker Compose configurations in `docker/`:

| File | Purpose |
|------|---------|
| `docker-compose-local.yml` | Local stack (supports `DEBUG=true` via `.env`) |
| `docker-compose-public.yml` | Public deployment with Caddy reverse proxy |

Caddy handles HTTPS termination and serves the frontend static assets. Backend listens on port 3333.

The PostgreSQL container is built from `docker/postgres.Dockerfile` (based on `percona/percona-distribution-postgresql:18`, which bundles pg_tde). A custom entrypoint (`docker/entrypoint-postgres.sh`) decrypts the pg_tde keyring from the data volume into an in-memory tmpfs before handing off to the official postgres entrypoint. No elevated container capabilities are required.

### Docker images

| Image | Dockerfile | Purpose |
|-------|-----------|---------|
| `zerf-time-absence-management` | `docker/app.Dockerfile` | Rust/Axum backend + frontend assets |
| `zerf-time-absence-management-postgres` | `docker/postgres.Dockerfile` | Percona PostgreSQL 18 with pg_tde |
| `zerf-time-absence-management-caddy` | `docker/caddy.Dockerfile` | Caddy reverse proxy |
| `zerf-time-absence-management-backup` | `docker/backup.Dockerfile` | PostgreSQL 18 client + curl for backup + Nextcloud upload |

The `backup` service in `docker-compose-local.yml` is connected to two networks:
- `backup_net` — internal network shared with `db`, required for `pg_dump`.
- `backup_egress` — non-internal network for outbound HTTPS to Nextcloud. The app container is **not** in this network.

### Start scripts

| Script | Purpose |
|--------|---------|
| `start_local.sh` | Start local stack (set `DEBUG=true` in `.env` for debug build) |
| `start_public.sh` | Start public stack |
| `scripts/backup.sh` | Dump, AES-encrypt, and optionally upload the database to a Nextcloud share. Each cycle also copies the pg_tde keyring (`zerf-<ts>.keyring.enc`, from the read-only `/keyring-src` mount) next to the dump so an orphaned encrypted PGDATA volume can be recovered. The backup interval is read from `app_settings` at runtime via `psql`; local retention is a fixed count (the 10 most recent). Refactored into sourceable functions (guarded by `BACKUP_LIB_ONLY=1`) for bats unit tests. |
| `scripts/restore.sh` | Interactive: decrypt a backup and restore it into the live instance. `--keyring [DIR]` extracts a backup's captured pg_tde keyring for physical recovery without touching the database. |
| `scripts/backup.bats` | bats unit tests for `backup.sh` helper functions (parse_share_url, interval resolution, upload credential handling, 0-byte rejection, keyring sidecar capture, retention pruning) |

### Key environment variables (encryption)

| Variable | Purpose |
|----------|---------|
| `ZERF_DB_ENCRYPTION_KEY` | Single passphrase that wraps the pg_tde keyring (DB at rest) and encrypts backups via openssl. Generate: `openssl rand -hex 32`. **Losing this key makes both the database and all backups unreadable.** |

### Backup and upload settings (app_settings)

Backup frequency and Nextcloud upload settings are stored in `app_settings` (not in `.env`) and are editable in the Admin UI under **Nextcloud Upload**. The backup container reads them via `psql` at the start of each cycle. Local retention is not configurable — the 10 most recent backups are always kept.

| Key | Default | Description |
|-----|---------|-------------|
| `backup_interval_days` | 1 | Days between backup cycles |
| `backup_upload_enabled` | false | Enable upload to Nextcloud |
| `backup_upload_url` | — | Nextcloud public share URL (`https://…/s/<token>`) |
| `backup_upload_password` | — | Optional share password (write-only) |
| `report_upload_enabled` | false | Enable monthly timesheet PDF upload |
| `report_upload_url` | — | Nextcloud public share URL for timesheets |
| `report_upload_password` | — | Optional share password (write-only) |
| `report_upload_day_of_month` | 5 | Day of month to upload previous month's PDF |

The earlier `backup_interval_seconds`/`backup_retention_days` keys are gone: migration 023 replaced the interval with `backup_interval_days`, and migration 024 dropped the retention setting in favour of a fixed count (the 10 most recent backups). Neither is set via environment variables.

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
- Update docs/user-guide.md to reflect the correct app behavior.

## Release Process

Commits follow [Conventional Commits](https://www.conventionalcommits.org/) format — git-cliff reads them to generate the changelog automatically.

Tag and push — the CI release workflow takes it from there:

```bash
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin vX.Y.Z
```

The CI release workflow (`release.yml`) then:
1. Injects the tag version into `Cargo.toml` and `package.json` (no commit)
2. Builds and pushes all three Docker images tagged with the version and `latest`
3. Generates the changelog via git-cliff and creates a GitHub Release with it as release notes

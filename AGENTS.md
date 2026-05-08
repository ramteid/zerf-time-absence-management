# Zerf - Agent Reference

Zerf (Zeiterfassung) is a self-hosted time tracking and absence management platform for teams. It covers working hours, leave requests, approvals, and monthly reports. Data stays on your infrastructure.

## Repository Layout

```
backend/      Rust/Axum HTTP API + PostgreSQL integration
frontend/     Svelte 5 single-page app
docker/       Docker Compose configurations and Caddy Dockerfile
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

### Modules

| File | Responsibility |
|------|---------------|
| `main.rs` | Startup: config, DB init, migrations, background tasks, Axum server |
| `config.rs` | Environment variable loading and validation |
| `db.rs` | Connection pool setup |
| `auth.rs` | Login, sessions, password hashing, CSRF |
| `users.rs` | User management, approver hierarchy |
| `time_entries.rs` | Daily time entries (draft/submitted/approved/rejected) |
| `absences.rs` | Absence requests (vacation, sick, training, etc.) |
| `change_requests.rs` | Change requests for submitted entries |
| `reopen_requests.rs` | Week reopen requests |
| `categories.rs` | Work categories (color-coded) |
| `holidays.rs` | Public holidays (auto-fetched and manual) |
| `reports.rs` | Monthly and team reports, CSV/PDF export |
| `notifications.rs` | In-app and email notifications |
| `submission_reminders.rs` | Scheduled weekly submission reminders |
| `audit.rs` | Audit log (JSON before/after snapshots) |
| `settings.rs` | App-wide key-value settings |
| `email.rs` | SMTP delivery via lettre |
| `i18n.rs` | Backend translations |
| `error.rs` | Error types |

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
| `change_requests` | Proposals to amend submitted entries |
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
**Linter**: ESLint 10.3.0
**Dev server port**: 5173 (proxies `/api` and `/healthz` to `http://127.0.0.1:3333`)
**Build output**: `frontend/dist/`

### NPM scripts

| Script | Command | Purpose |
|--------|---------|---------|
| `dev` | `vite` | Start dev server |
| `build` | `vite build` | Production build |
| `lint` | `eslint .` | Lint source |
| `format` | `prettier --check` | Check formatting |
| `format:write` | `prettier --write` | Auto-format |
| `test` | `vitest run` | Run tests |

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
/time-entries/*     CRUD, submit, approve, reject, batch operations
/absences/*         CRUD, approve, reject, revoke, calendar, leave balance
/change-requests/*  CRUD, approve, reject
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
- **Database**: SCRAM auth, checksums, internal-only network
- **Audit log**: All mutations logged with JSON snapshots; passwords and secrets never logged
- **Password reset**: One-time 1h tokens, forced change on first login

## Deployment

Three Docker Compose configurations in `docker/`:

| File | Purpose |
|------|---------|
| `docker-compose-local.yml` | Local production-like stack |
| `docker-compose-local-debug.yml` | Local debug stack |
| `docker-compose-public.yml` | Public deployment with Caddy reverse proxy |

Caddy handles HTTPS termination and serves the frontend static assets. Backend listens on port 3333.

### Start scripts

| Script | Purpose |
|--------|---------|
| `start_local.sh` | Start local stack |
| `start_local_debug.sh` | Start local debug stack |
| `start_public.sh` | Start public stack |
| `scripts/backup.sh` | Backup PostgreSQL data to local Docker volume |

## Testing

### Frontend

```bash
cd frontend
npm run lint && npm test -- --run && npm run build
```

Tests use Vitest + jsdom. Test files are co-located with source under `src/` and `src/routes/`.

### Backend

Integration tests are in `backend/tests/integration/`. Each test gets an isolated PostgreSQL database created and dropped automatically via `tests/common/mod.rs` (testcontainers).

```bash
cd backend
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/postgres cargo test
```

For a local Postgres container:

```bash
docker run -d -p 55432:5432 -e POSTGRES_PASSWORD=postgres postgres
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:55432/postgres cargo test
```

`backend/tests/nager_contract.rs` validates the external Nager.Date holiday API contract.

## Coding Conventions

- Use explicit, descriptive variable and function names that reveal intent without requiring a comment.
- Prioritize readability for humans over brevity; code is read far more often than it is written.
- Keep functions and modules small and focused on a single responsibility.
- Reduce complexity: avoid unnecessary abstractions, indirection, and nesting.
- Prefer simple, direct solutions over clever ones. Keep it concise.
- Apply appropriate architectural patterns (e.g., handler/service/repository separation) consistently across the codebase.
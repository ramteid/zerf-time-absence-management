# Zerf — Security Model

Zerf handles personal data of staff (names, working hours, sick leave,
holiday balance) and is therefore deployed with a defence-in-depth posture.
This document summarises the controls in place and how to operate the system
safely on a public, internet-facing server.

The backend is written in Rust, which eliminates whole classes of memory-safety
bugs (buffer overflows, use-after-free, data races) at compile time.

## Reporting a vulnerability

Please report security issues privately to the maintainer's email or via a
private GitHub Security Advisory. Do **not** open a public issue.

## Threat model (in scope)

| Asset                        | Risk                                          | Control                                                                |
|------------------------------|-----------------------------------------------|-------------------------------------------------------------------------|
| Login credentials            | Credential stuffing, brute force              | Argon2id hashing, 5/15 min lockout, generic error messages              |
| Session cookies              | XSS theft, MITM, fixation, replay             | HttpOnly, Secure, SameSite=Strict, rotated on login, 8 h idle / 7 d absolute, idle enforced in middleware |
| Personal data in DB          | Data theft, lateral movement, tampering       | Internal-only PostgreSQL network, SCRAM auth, checksums, app least privilege, backup isolated on separate network |
| State-changing endpoints     | CSRF                                          | SameSite=Strict cookie + Origin/Referer check + X-CSRF-Token header     |
| HTTP traffic                 | MITM, downgrade, sniffing                     | Caddy + Let's Encrypt + HSTS preload + CSP + COOP/CORP                  |
| Account takeover via reset   | Token replay, reuse of leaked temp pw         | One-time 1 h reset tokens, forced password change on first login, sessions cleared on reset/change |
| Logs                         | Sensitive data leakage                        | Passwords/secrets never logged; tracing on info; JSON-file 10MB rotation|

Out of scope (v1): payroll integrations, SSO, multi-tenant isolation.

## Authentication

* **Argon2id**, OWASP-recommended parameters (m = 19 456 KiB, t = 2, p = 1).
* **Constant-time** verification path: failed lookups still run a verify against
  a dummy hash to keep timing uniform.
* **Lockout**: 5 failed attempts per email in 15 min ⇒ generic "Invalid email or
  password." (no account-existence oracle). Requests that arrive while an
  account is already locked are **not** counted toward extending the
  window — that would let any unauthenticated attacker who knows a target
  email keep the account locked indefinitely. Lockouts therefore expire
  after 15 min of attacker silence and the legitimate user can retry.
* **Edge rate limiting** in Caddy supplements the per-email lockout with
  per-IP caps so a distributed attacker that varies usernames cannot
  scan at high QPS — see "Transport & HTTP hardening" below.
* **Password policy** (enforced server-side):
  * ≥ 12 characters, ≤ 256 characters
  * at least 3 of {lowercase, uppercase, digit, symbol}
  * may not equal the previous password.
* **Generated temporary passwords** (16 chars, mixed-class) come from `OsRng`
  (the OS CSPRNG), never from the thread RNG.
* **Self-service password reset** requires SMTP plus `ZERF_PUBLIC_URL`, stores
  only SHA-256 token hashes, allows one live token per user, and consumes the
  token atomically during password change.

## Sessions

* 256-bit random token (hex-encoded), stored hashed in `sessions.token`.
* Cookie flags: `HttpOnly; Secure; SameSite=Strict; Path=/`.
* **`__Host-` prefix** is applied to the cookie name in production
  (`__Host-zerf_session`). The prefix forces `Path=/`, forbids a `Domain=`
  attribute, and requires HTTPS — preventing a sibling subdomain or a network
  attacker from overwriting or fixating the session cookie. In dev mode (plain
  HTTP) the prefix is dropped (`zerf_session`) because browsers reject
  `__Host-` cookies on non-HTTPS origins.
* **Session fixation**: a fresh token is issued on every successful login;
  any pre-existing token in the request is ignored.
* **Idle timeout 8 h**, **absolute timeout 7 days** — whichever fires
  first. Both are enforced directly in the auth middleware (the previous
  version relied on the background cleanup task, which created a brief
  window where idle sessions remained valid). The hourly cleanup loop
  uses the same constants as the middleware so the two layers cannot
  drift apart.
* **Session invalidation**: on password reset, deactivation, and logout, all
  sessions of the affected user are deleted server-side. On a voluntary password
  change the caller's current session is preserved (so they remain logged in)
  while all other sessions for that user are revoked.
* **First-boot setup race**: `POST /auth/setup` now takes a Postgres
  transaction-scoped advisory lock around the count/insert, so two
  concurrent setup requests cannot both create an admin on a fresh
  database. The same lock (`lock_user_graph`) guards every write that
  changes the approver/admin graph.
* Background task purges expired sessions and old login attempts hourly.

## CSRF

`SameSite=Strict` already prevents cross-site cookie attachment for the modern
threat model. We add two layers of defence-in-depth:

1. **Origin / Referer** allow-list (`ZERF_ALLOWED_ORIGINS`, derived from
   `ZERF_PUBLIC_URL` by default). All non-GET requests must originate from
   an allowed origin. The login endpoint is checked the same way.
2. **Double-submit token**: each session carries a random `csrf_token`.
   The SPA reads it from `/api/v1/auth/me` and on the login response, then
   echoes it as `X-CSRF-Token` on every state-changing request. The server
   compares it in constant time (`subtle::ConstantTimeEq`).

## Authorisation

Every API handler checks the role on the authenticated `User` extension
inserted by the auth middleware:

* **employee** — only own data (time entries, absences, balance, calendar);
* **assistant** — like employee but without access to the dashboard; intended for
  part-time or supporting staff who do not track flextime;
* **team\_lead** — read team data, approve/reject; cannot self-approve;
* **admin** — full management; can self-approve as documented exception.

All write actions are recorded in `audit_log` with a JSON snapshot of the
before/after row. Admin-only endpoints additionally check `is_admin()`.

Per-user records (reports, leave balance, time entries, absences) pass through a
single least-privilege check (`assert_can_access_user`): an employee reaches only
their own rows, a team lead only their direct reports, an admin everyone. Team
scoping is enforced **inside the SQL** (direct-report subqueries with row locks),
so a crafted `user_id` cannot widen access. Ownership is re-verified before every
mutation, and batch operations confirm that **all** targeted rows are permitted
before any write. Privilege escalation is blocked: roles come from a fixed
allow-list, the last active admin cannot be removed or demoted, an admin cannot
demote themselves, and a non-admin can only create `assistant` users with a
server-forced approver.

## Input handling

* All SQL is parameterised (sqlx `bind`). No string interpolation.
* JSON body limit: **1 MiB**, enforced before deserialization.
* Per-request **30 s timeout** at the tower layer.
* Date / time / numeric fields are parsed by `chrono` / `serde`; invalid input
  produces a `400 Bad Request`. Time-entry validation enforces overlap-free,
  ≤ 14 h/day, end > start, no future dates.
* Email is lowercased and length-bounded (≤ 254) on **every** auth
  endpoint, including `forgot-password` (the latter previously had no
  length cap, so an attacker could persist near-1 MiB strings into the
  rate-limit table).
* **XSS**: the SPA never injects untrusted HTML — the only raw-HTML sink is the
  trusted static icon set; the CSP (`script-src 'self'`, `object-src 'none'`)
  blocks injected and inline scripts. The session cookie is HttpOnly and the
  CSRF token is held in memory, not `localStorage`.
* **Outbound requests (SSRF)**: holiday lookups go only to a hardcoded upstream
  host with a validated 2-letter country code; Nextcloud backup/report uploads
  must be `https` share URLs, and the target host is resolved and **rejected when
  it maps to a loopback, private, link-local, or cloud-metadata address**
  (`169.254.169.254`), with the validated IPs pinned for the request as a
  DNS-rebinding defence.

## Transport & HTTP hardening

Backend (tower-http `SetResponseHeaderLayer`) and Caddy both emit:

* `Strict-Transport-Security: max-age=63072000; includeSubDomains; preload` (Caddy)
* `Content-Security-Policy: default-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'unsafe-inline'; font-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'`
* `X-Content-Type-Options: nosniff`
* `X-Frame-Options: DENY`
* `Referrer-Policy: strict-origin-when-cross-origin`
* `Permissions-Policy: accelerometer=(), camera=(), geolocation=(), gyroscope=(), microphone=(), payment=(), usb=()`
* `Cross-Origin-Opener-Policy: same-origin`
* `Cross-Origin-Resource-Policy: same-origin`
* Server / X-Powered-By suppressed.
* `Cache-Control: no-store` for all dynamic (API/SPA) responses; hashed static
  assets under `/assets/*` receive `public, max-age=31536000, immutable` for
  long-lived browser caching.

Caddy also bumps to TLS 1.2+/H2/H3 and renews certificates via Let's Encrypt
(`tls-alpn-01`), with HTTP→HTTPS redirect enabled by default.

### Caddy edge controls (defence-in-depth)

The reverse proxy is built with the
[`caddy-ratelimit`](https://github.com/mholt/caddy-ratelimit) module
(see `docker/Caddyfile.Dockerfile`) and applies:

* **Per-IP rate limits** — `auth/login` 10/min, `auth/forgot-password`
  5/15 min, `auth/reset-password` 10/15 min, `auth/setup` 5/h, generic
  `/api/*` 600/min. Stops distributed credential stuffing, mass
  password-reset bombing and bulk enumeration even when individual
  emails would not trip the per-account lockout. `auth/setup-status`
  is intentionally **not** capped tightly — the SPA polls it on every
  Login-page load, so it rides the generic API ceiling.
* **Server-level connection timeouts** — `read_header 5 s`,
  `read_body 10 s`, `write 30 s`, `idle 2 min` — defends against
  slow-loris and connection-flooding without disrupting the SSE
  notification stream (which sends a keep-alive every 30 s).
* **Method allow-list** — only `GET HEAD POST PUT DELETE OPTIONS`
  reach the application; everything else (`TRACE`, `CONNECT`,
  `PATCH`, exotic verbs) is rejected with 405 at the edge.
* **Scanner suppression** — common probe paths (`.git/*`, `.env`,
  `/wp-*`, `/xmlrpc.php`, `/phpmyadmin*`, `/server-status`,
  `/actuator/*`) are answered with 404 without ever touching the app
  or its logs.
* **Reverse-proxy upstream timeouts** — bound dial / response-header /
  read / write so a stuck application instance cannot tie up edge
  workers.
* **Body cap** — 1 MB at the edge, 1 MiB inside the app (edge is the
  stricter of the two on purpose).
* **TLS floor** — `tls1.2`/`tls1.3` only; older protocol versions are
  refused at the handshake.
* **Admin API disabled** — `admin off` removes Caddy's local
  configuration API (default `127.0.0.1:2019`). Deployments restart the
  container rather than `caddy reload`, so the endpoint is unused; turning
  it off means a foothold inside the Caddy container cannot read TLS
  private keys, dump the running config, or live-rewrite the proxy.

## Secrets & configuration

* `ZERF_SESSION_SECRET` is **required**, ≥ 32 characters, must not be a
  known placeholder; the app refuses to start otherwise. Generate with
  `openssl rand -hex 32` and store in `.env` with `chmod 600`.
* `ZERF_POSTGRES_PASSWORD` is **required** for the bundled database and
  should likewise be generated with `openssl rand -hex 32`.
* `.env` is git-ignored. `.env.example` documents every variable.
* `docker-compose.yml` references variables with `:?` so the stack refuses to
  start when a critical secret is missing.
* Stored secrets (SMTP password, Nextcloud share passwords) are **write-only over
  the API**: the admin UI receives only a "password is set" flag, never the
  value. Password hashes and session tokens are never serialized into API
  responses or `audit_log` snapshots.
* Internal/database errors return a generic message to the client; SQL text and
  stack details are logged server-side only.

## Container & runtime

* Multi-stage Debian-slim image (~80 MiB final), `tini` as PID 1.
* **Non-root** UID 10001, group 10001.
* **`read_only: true`** root filesystem; `tmpfs:/tmp`.
* `cap_drop: [ALL]`; `security_opt: no-new-privileges:true`.
* Caddy runs with only `NET_BIND_SERVICE` capability.
* **Three isolated Docker networks** enforce least-privilege connectivity:
  * `public` (bridge) — Caddy + app; the only ingress network.
  * `private` (internal) — app + postgres; no external connectivity.
  * `backup_net` (internal) — postgres + backup only. The backup service holds
    `ZERF_DB_ENCRYPTION_KEY` and is deliberately absent from `private`, so a
    compromised app container cannot reach it.
* No database or backup service port is published on the host.
* In the **public** deployment the application's HTTP port is **not** published
  on the host: Caddy (ports 80/443) is the only ingress, and it reaches the app
  over the `public` Docker network. The base/local compose intentionally
  publishes 3333 for direct LAN access where no reverse proxy is present; the
  public overlay clears that host port mapping (`ports: !reset []`).
* PostgreSQL initializes with `scram-sha-256` auth for local and host
  connections, `password_encryption=scram-sha-256`, data checksums, and 30 s
  statement / idle-in-transaction timeouts.
* The application pool uses bounded connections, acquire timeouts, idle
  timeouts, and connection health checks before reuse.
* `HEALTHCHECK` against `/healthz` for orchestrators.
* JSON-file logs are size-capped (10 MiB × 5).

## Data encryption at rest

* **Database**: all PostgreSQL tables and WAL segments are transparently
  encrypted at the storage layer with
  [pg_tde](https://docs.percona.com/pg-tde/) (Percona Transparent Data
  Encryption). The pg_tde principal key is wrapped with `ZERF_DB_ENCRYPTION_KEY`
  and stored encrypted on the data volume; on container start the entrypoint
  decrypts it into an in-memory tmpfs, so the plaintext key never touches disk.
* **Backups**: every `.dump.enc` is AES-256-CBC encrypted (PBKDF2, 100 000
  iterations) under the same `ZERF_DB_ENCRYPTION_KEY`. One key governs both
  layers — losing it makes the live database **and** all backups permanently
  unreadable.

## Backups

The dedicated `backup` compose service runs `scripts/backup.sh`, which reads its
interval from `app_settings` (`backup_interval_days`, admin-configurable in the
UI) and stores AES-256-encrypted `pg_dump --format=custom` snapshots
(`.dump.enc`) in the `zerf_backup_data` named volume with `umask 077`. Each cycle
also captures the wrapped pg_tde keyring (`*.keyring.enc`) next to the dump, so an
orphaned encrypted data volume can still be recovered. Local retention keeps the
10 most recent backups; optional upload to a Nextcloud public share is
configurable (and SSRF-guarded — see "Outbound requests" under Input handling).

## Supply-chain & CI

`.github/dependabot.yml` schedules **weekly** updates for:

* `cargo` (Rust crates)
* `docker` (base images)
* `github-actions`

`.github/workflows/ci.yml` runs on every push/PR and weekly:

* `cargo build --release --locked`, Rust unit + integration tests (testcontainers)
* **`cargo audit`** (RustSec advisories) and **`npm audit --omit=dev
  --audit-level=high`** — both **blocking** (a new advisory fails CI)
* `bats` Bash unit tests; frontend Vitest + build
* Docker smoke test, Trivy filesystem **and** image scan (HIGH/CRITICAL ⇒ failure)
* CodeQL JavaScript analysis on the SPA

`.github/workflows/audit.yml` additionally runs both dependency audits daily (and
on dependency-file changes), filing an issue on new findings.

`.github/workflows/auto-merge-deps.yml` auto-merges Dependabot patch and minor
updates after CI is green; major updates require a human review.

## Operational checklist

1. `cp .env.example .env && chmod 600 .env`
2. Replace `ZERF_SESSION_SECRET` and `ZERF_POSTGRES_PASSWORD` with `openssl rand -hex 32` outputs.
3. Set `ZERF_DOMAIN` in `.env`.
4. `./start_public.sh` — open the application in your browser to create the initial admin account.
5. Sign in with the credentials you just created, then add real users.
6. Set the backup interval in the admin UI (Settings → Nextcloud Upload) and copy snapshots off-host.
7. Subscribe to release notes; let Dependabot keep dependencies fresh.

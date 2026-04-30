# KitaZeit — Security Model

KitaZeit handles personal data of staff (names, working hours, sick leave,
holiday balance) and is therefore deployed with a defence-in-depth posture.
This document summarises the controls in place and how to operate the system
safely on a public, internet-facing server.

## Reporting a vulnerability

Please report security issues privately to the maintainer's email or via a
private GitHub Security Advisory. Do **not** open a public issue.

## Threat model (in scope)

| Asset                        | Risk                                          | Control                                                                |
|------------------------------|-----------------------------------------------|-------------------------------------------------------------------------|
| Login credentials            | Credential stuffing, brute force              | Argon2id hashing, 5/15 min lockout, generic error messages              |
| Session cookies              | XSS theft, MITM, fixation, replay             | HttpOnly, Secure, SameSite=Strict, rotated on login, 8 h idle / 24 h max|
| Personal data in DB          | Data theft, lateral movement, tampering       | Internal-only PostgreSQL network, SCRAM auth, checksums, app least privilege |
| State-changing endpoints     | CSRF                                          | SameSite=Strict cookie + Origin/Referer check + X-CSRF-Token header     |
| HTTP traffic                 | MITM, downgrade, sniffing                     | Caddy + Let's Encrypt + HSTS preload + CSP + COOP/CORP                  |
| Account takeover via reset   | Reuse of leaked temp pw                       | Forced password change on first login, sessions cleared on reset/change |
| Logs                         | Sensitive data leakage                        | Passwords/secrets never logged; tracing on info; JSON-file 10MB rotation|

Out of scope (v1): payroll integrations, e-mail, SSO, multi-tenant isolation.

## Authentication

* **Argon2id**, OWASP-recommended parameters (m = 19 456 KiB, t = 2, p = 1).
* **Constant-time** verification path: failed lookups still run a verify against
  a dummy hash to keep timing uniform.
* **Lockout**: 5 failed attempts per email in 15 min ⇒ generic "Invalid email or
  password." (no account-existence oracle).
* **Password policy** (enforced server-side):
  * ≥ 12 characters, ≤ 256 characters
  * at least 3 of {lowercase, uppercase, digit, symbol}
  * may not equal the previous password.
* **Generated temporary passwords** (16 chars, mixed-class) come from `OsRng`
  (the OS CSPRNG), never from the thread RNG.

## Sessions

* 256-bit random token (hex-encoded), stored hashed in `sessions.token`.
* Cookie flags: `HttpOnly; Secure; SameSite=Strict; Path=/`.
* **Session fixation**: a fresh token is issued on every successful login;
  any pre-existing token in the request is ignored.
* **Idle timeout 8 h**, **absolute timeout 24 h** — whichever fires first.
* **Session invalidation**: on password change, password reset, deactivation
  and on logout, all sessions of the affected user are deleted server-side.
* Background task purges expired sessions and old login attempts hourly.

## CSRF

`SameSite=Strict` already prevents cross-site cookie attachment for the modern
threat model. We add two layers of defence-in-depth:

1. **Origin / Referer** allow-list (`KITAZEIT_ALLOWED_ORIGINS`, derived from
   `KITAZEIT_PUBLIC_URL` by default). All non-GET requests must originate from
   an allowed origin. The login endpoint is checked the same way.
2. **Double-submit token**: each session carries a random `csrf_token`.
   The SPA reads it from `/api/v1/auth/me` and on the login response, then
   echoes it as `X-CSRF-Token` on every state-changing request. The server
   compares it in constant time (`subtle::ConstantTimeEq`).

## Authorisation

Every API handler checks the role on the authenticated `User` extension
inserted by the auth middleware:

* **employee** — only own data (time entries, absences, balance, calendar);
* **team\_lead** — read team data, approve/reject; cannot self-approve;
* **admin** — full management; can self-approve as documented exception.

All write actions are recorded in `audit_log` with a JSON snapshot of the
before/after row. Admin-only endpoints additionally check `is_admin()`.

## Input handling

* All SQL is parameterised (sqlx `bind`). No string interpolation.
* JSON body limit: **1 MiB**, enforced before deserialization.
* Per-request **30 s timeout** at the tower layer.
* Date / time / numeric fields are parsed by `chrono` / `serde`; invalid input
  produces a `400 Bad Request`. Time-entry validation enforces overlap-free,
  ≤ 14 h/day, end > start, no future dates.
* Email is lowercased and length-bounded (≤ 254).

## Transport & HTTP hardening

Backend (tower-http `SetResponseHeaderLayer`) and Caddy both emit:

* `Strict-Transport-Security: max-age=63072000; includeSubDomains; preload` (Caddy)
* `Content-Security-Policy: default-src 'self'; ... frame-ancestors 'none'; object-src 'none'`
* `X-Content-Type-Options: nosniff`
* `X-Frame-Options: DENY`
* `Referrer-Policy: strict-origin-when-cross-origin`
* `Permissions-Policy: accelerometer=(), camera=(), geolocation=(), microphone=(), ...`
* `Cross-Origin-Opener-Policy: same-origin`
* `Cross-Origin-Resource-Policy: same-origin`
* Server / X-Powered-By suppressed.
* `Cache-Control: no-store` for dynamic responses.

Caddy also bumps to TLS 1.2+/H2/H3 and renews certificates via Let's Encrypt
(`tls-alpn-01`), with HTTP→HTTPS redirect enabled by default.

## Secrets & configuration

* `KITAZEIT_SESSION_SECRET` is **required**, ≥ 32 characters, must not be a
  known placeholder; the app refuses to start otherwise. Generate with
  `openssl rand -hex 32` and store in `.env` with `chmod 600`.
* `KITAZEIT_POSTGRES_PASSWORD` is **required** for the bundled database and
  should likewise be generated with `openssl rand -hex 32`.
* `.env` is git-ignored. `.env.example` documents every variable.
* `docker-compose.yml` references variables with `:?` so the stack refuses to
  start when a critical secret is missing.

## Container & runtime

* Multi-stage Debian-slim image (~80 MiB final), `tini` as PID 1.
* **Non-root** UID 10001, group 10001.
* **`read_only: true`** root filesystem; `tmpfs:/tmp`.
* `cap_drop: [ALL]`; `security_opt: no-new-privileges:true`.
* Caddy runs with only `NET_BIND_SERVICE` capability.
* PostgreSQL is reachable only on an **internal Docker network**; no database
  port is published on the host.
* PostgreSQL initializes with `scram-sha-256` auth for local and host
  connections, `password_encryption=scram-sha-256`, data checksums, and 30 s
  statement / idle-in-transaction timeouts.
* The application pool uses bounded connections, acquire timeouts, idle
  timeouts, and connection health checks before reuse.
* `HEALTHCHECK` against `/healthz` for orchestrators.
* JSON-file logs are size-capped (10 MiB × 5).

## Backups

`scripts/backup.sh` streams `pg_dump --format=custom` from the PostgreSQL
container into `./backups/` with `umask 077`. Setting
`BACKUP_GPG_RECIPIENT=<keyid>` encrypts each dump via `gpg --encrypt`; the
plaintext copy is then `shred`-ed. Retention defaults to 30 days.

## Supply-chain & CI

`.github/dependabot.yml` schedules **weekly** updates for:

* `cargo` (Rust crates)
* `docker` (base images)
* `github-actions`

`.github/workflows/ci.yml` runs on every push/PR and weekly:

* `cargo fmt --check`, `cargo clippy -D warnings`, `cargo build --release --locked`
* `rustsec/audit-check` (RustSec advisories)
* full `tests/run.sh` integration suite inside Docker
* Trivy filesystem **and** image scan (HIGH/CRITICAL ⇒ failure)
* CodeQL JavaScript analysis on the SPA

`.github/workflows/auto-merge-deps.yml` auto-merges Dependabot patch and minor
updates after CI is green; major updates require a human review.

## Operational checklist

1. `cp .env.example .env && chmod 600 .env`
2. Replace `KITAZEIT_SESSION_SECRET` and `KITAZEIT_POSTGRES_PASSWORD` with `openssl rand -hex 32` outputs.
3. Set `KITAZEIT_DOMAIN` and `KITAZEIT_ADMIN_EMAIL`.
4. `docker compose up -d` — note the one-time admin password from the logs.
5. Sign in, change the admin password (forced), create real users.
6. Schedule `scripts/backup.sh` via cron and copy snapshots off-host.
7. Subscribe to release notes; let Dependabot keep dependencies fresh.

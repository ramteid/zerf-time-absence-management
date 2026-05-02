# KitaZeit

Self-hosted time tracking for kindergartens.


KitaZeit lets a team record working hours, request leave,
get approvals, and produce monthly reports — without the fuss of a payroll
suite. The whole thing runs from one `docker compose up`.

---

## Why use it

- **Quick to learn.** German-style work-time forms, calm interface, sensible defaults.
- **Mobile-first.** Educators record their hours from a phone in the cloakroom.
- **Self-hosted.** Your data stays on your server. No SaaS, no telemetry.
- **Operable.** Caddy, the app, and PostgreSQL ship as one compose stack with hardened defaults.

## What you get

| Role | What they can do |
|------|------------------|
| Employee | Log hours per category, request vacation, report sick days, ask to reopen a submitted week, see own balance |
| Team lead | Approve/decline timesheets, leave, change & reopen requests; team calendar; reports; manage own approval policy |
| Admin | Manage users, categories, holidays; audit log; reset passwords; manage all approval policies |

Built-in workflows include weekly time-entry submission, approvals, change
requests for already-submitted entries, week reopen requests with
optional auto-approval per approver, persistent in-app notifications
(plus opt-in email), vacation balance tracking, public holidays for
Baden-Württemberg, an overtime ledger, and CSV exports.

A short tour of the UI:

- **Time** — week view, one row per category, today highlighted, single-tap "submit week"; once submitted, a **Request edit** button lets the employee ask to reopen the week for corrections.
- **Absences** — one form per type (vacation, sick, training, special leave, unpaid).
- **Calendar** — month view of who is away, colour-coded by type.
- **Dashboard** — team leads see all open approvals in one place, including a dedicated *Week reopen requests* queue.
- **Reports** — monthly per-employee, team summary, category breakdown, CSV.
- **Team Policy** *(team leads & admins)* — toggle "auto-approve reopens" per approver to skip manual review.
- **Admin** — users, categories, holidays, audit log.
- **Notification center** — bell in the sidebar with unread count; lists reopen-request events, approvals, rejections.

### How week reopen works

1. After an employee submits a week, the **Submit Week** button is replaced
   by a **Request edit** action.
2. If the employee's assigned approver has *Auto-approve reopens* enabled
   (set under **Team Policy**), the week is reopened immediately — every
   non-draft entry returns to `draft` and any open per-entry change
   requests for that week are auto-cancelled.
3. Otherwise the request is queued. The approver receives an in-app
   notification (and an email when SMTP is configured), and can
   approve or reject from the Dashboard. The employee gets the
   corresponding follow-up notification.

Each employee **must** have an approver assigned (Team lead or Admin); the
selector in the user dialog is mandatory and the schema enforces this.

## Install

You need a Linux host with Docker, a domain name pointing at it, and ports
80/443 open.

```bash
git clone <repo-url> kitazeit && cd kitazeit
cp .env.example .env
chmod 600 .env
$EDITOR .env                       # set domain, admin email, generate secret
docker compose up -d
docker compose logs app
```

The initial admin credentials are the email set in `KITAZEIT_ADMIN_EMAIL` (defaults to `admin@example.com`) and the password `admin`. Sign in at
`https://<your-domain>` — you will be required to change it on first login.

### Configuration

Everything lives in `.env`. The example file documents every variable; the
ones you must set are:

```env
KITAZEIT_DOMAIN=example.de
KITAZEIT_SESSION_SECRET=$(openssl rand -hex 32)
KITAZEIT_POSTGRES_PASSWORD=$(openssl rand -hex 32)
KITAZEIT_ADMIN_EMAIL=admin@example.de
```

Optional SMTP variables (`KITAZEIT_SMTP_HOST`, `KITAZEIT_SMTP_PORT`,
`KITAZEIT_SMTP_USERNAME`, `KITAZEIT_SMTP_PASSWORD`, `KITAZEIT_SMTP_FROM`,
`KITAZEIT_SMTP_ENCRYPTION`) enable outbound email for notifications. When
unset, the in-app notification center keeps working unchanged; only the
email side-channel is disabled.

Caddy obtains a Let's Encrypt certificate automatically on first start. The
bundled PostgreSQL service stays on an internal Docker network and is not
published to the internet.

### Backups

```bash
# Daily snapshot at 03:00:
0 3 * * * cd /opt/kitazeit && /opt/kitazeit/scripts/backup.sh /opt/kitazeit/backups
```

The helper streams a `pg_dump` custom-format snapshot from the internal
PostgreSQL container. Set `BACKUP_GPG_RECIPIENT=<your-key>` to encrypt every
snapshot at rest.

### Updates

```bash
git pull && docker compose up -d --build
```

The schema migrates itself; no manual steps required.

## Security

KitaZeit is meant to live on the open internet. Hardening is documented in
[`SECURITY.md`](SECURITY.md). Highlights:

- Argon2id passwords, lockout after 5 failed attempts in 15 min.
- Session cookies HttpOnly + Secure + SameSite=Strict; 8 h idle / 24 h hard cap.
- CSRF: SameSite + Origin allow-list + double-submit `X-CSRF-Token`.
- PostgreSQL stays on an internal Docker network with SCRAM auth and data checksums.
- HSTS preload, full CSP, X-Frame-Options DENY, COOP/CORP same-origin.
- Container runs non-root with read-only rootfs and all capabilities dropped.
- 1 MiB body limit, 30 s request timeout, no sensitive data in logs.

If you find a vulnerability, please report it privately — see [`SECURITY.md`](SECURITY.md).

## Development

The canonical regression suite is the Rust integration test in
[`backend/tests/integration.rs`](backend/tests/integration.rs:1) — every
business rule (validation, permissions, workflow transitions, response
shapes) is asserted there. The frontend has its own quality gates via
[Vitest](https://vitest.dev) under [`frontend/src/`](frontend/src/).

```bash
# Backend integration suite (requires DATABASE_URL)
cd backend && DATABASE_URL=postgres://localhost/postgres cargo test --test integration

# Frontend lint + format + tests + production build
cd frontend && npm install && npm run lint && npm run format && npm test && npm run build

```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml:1)) runs both on
every push: backend (fmt, clippy, build, integration tests, cargo-audit),
frontend (lint, format, Vitest, build, npm audit), plus Docker smoke, Trivy,
and CodeQL (JavaScript) jobs.

| Path | What's there |
|------|--------------|
| [`backend/`](backend/) | Rust + Axum + PostgreSQL — owns all business rules |
| [`frontend/`](frontend/) | Svelte + Vite SPA — thin client that renders backend-shaped state |
| [`scripts/`](scripts/) | PostgreSQL backup helper |

## Roadmap (out of scope for v1)

Payroll integration, SSO/LDAP, native mobile app,
multi-tenant — all deliberately *not* built. The whole point is to stay small
and easy to operate.

## License

MIT — see [`LICENSE`](LICENSE).

# KitaZeit

Self-hosted time tracking for kindergartens.


KitaZeit lets a small team (5–50 people) record working hours, request leave,
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
| Employee | Log hours per category, request vacation, report sick days, see own balance |
| Team lead | Approve/decline timesheets, leave, change requests; team calendar; reports |
| Admin | Manage users, categories, holidays; audit log; reset passwords |

Built-in workflows include weekly time-entry submission, approvals, change
requests for already-submitted entries, vacation balance tracking, public
holidays for Baden-Württemberg, an overtime ledger, and CSV exports.

A short tour of the UI:

- **Time** — week view, one row per category, today highlighted, single-tap "submit week".
- **Absences** — one form per type (vacation, sick, training, special leave, unpaid).
- **Calendar** — month view of who is away, colour-coded by type.
- **Dashboard** — team leads see all open approvals in one place.
- **Reports** — monthly per-employee, team summary, category breakdown, CSV.
- **Admin** — users, categories, holidays, audit log.

## Install

You need a Linux host with Docker, a domain name pointing at it, and ports
80/443 open.

```bash
git clone <repo-url> kitazeit && cd kitazeit
cp .env.example .env
chmod 600 .env
$EDITOR .env                       # set domain, admin email, generate secret
docker compose up -d
docker compose logs app | grep "Admin password"
```

The first line of the logs prints a one-time admin password. Sign in at
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

`bash tests/run.sh` spins up an isolated, hardened container and runs the full
API + browser regression (~70 assertions) against it. CI runs the same suite
on every push.

| Path | What's there |
|------|--------------|
| [`backend/`](backend/) | Rust + Axum + PostgreSQL |
| [`frontend/`](frontend/) | Single-page app (vanilla JS) |
| [`tests/`](tests/) | End-to-end test runner |
| [`scripts/`](scripts/) | PostgreSQL backup helper |

## Roadmap (out of scope for v1)

Payroll integration, e-mail notifications, SSO/LDAP, native mobile app,
multi-tenant — all deliberately *not* built. The whole point is to stay small
and easy to operate.

## License

MIT — see [`LICENSE`](LICENSE).

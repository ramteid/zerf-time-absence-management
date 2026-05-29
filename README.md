# Zerf Work Time Tracking

[![CI](https://github.com/ramteid/zerf-work-time-tracking/actions/workflows/ci.yml/badge.svg)](https://github.com/ramteid/zerf-work-time-tracking/actions/workflows/ci.yml)
[![Security Audit](https://github.com/ramteid/zerf-work-time-tracking/actions/workflows/audit.yml/badge.svg)](https://github.com/ramteid/zerf-work-time-tracking/actions/workflows/audit.yml)
[![Build & Push Image](https://github.com/ramteid/zerf-work-time-tracking/actions/workflows/build-push-image.yml/badge.svg)](https://github.com/ramteid/zerf-work-time-tracking/actions/workflows/build-push-image.yml)
[![Nager.Date Contract](https://github.com/ramteid/zerf-work-time-tracking/actions/workflows/nager-contract.yml/badge.svg)](https://github.com/ramteid/zerf-work-time-tracking/actions/workflows/nager-contract.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Backend: Rust](https://img.shields.io/badge/backend-Rust-orange.svg)](https://www.rust-lang.org)
[![Frontend: Svelte](https://img.shields.io/badge/frontend-Svelte-FF3E00.svg)](https://svelte.dev)
[![Database: PostgreSQL](https://img.shields.io/badge/database-PostgreSQL-336791.svg)](https://www.postgresql.org)
[![Deploy: Docker](https://img.shields.io/badge/deploy-Docker-2496ED.svg)](https://www.docker.com)
[![Self-hosted](https://img.shields.io/badge/self--hosted-yes-success.svg)](#quick-setup)

**Self-hosted time tracking and absence management for teams — fast, focused, and fully under your control.**

Track working hours, manage leave requests, run approvals, and generate reports — all in one lightweight tool your whole team will actually enjoy using. No SaaS subscription. No bloat. Your data stays where you put it.

`Zerf` is derived from the German word *Zeiterfassung* — time tracking.

<p align="center">
  <img src="docs/screenshots/tour.gif" alt="Animated tour of Zerf: dashboard, time entry, absences, team calendar, and reports" width="320">
</p>

## 🗂️ Overview

Zerf covers the full daily workflow between employees, team leads, and admins:

- **Employees** log hours, request leave, and submit weeks for approval.
- **Team leads** review time sheets, approve or reject absence requests, and keep an eye on the team calendar.
- **Admins** manage users, categories, holidays, and settings — and get a full audit trail of every change.

Everything is designed for speed: quick to set up, quick to use on desktop or mobile, and easy to keep running.

## ✨ Key features

- ⏱️ **Time tracking** — category-based daily entries, weekly submission flow, overtime visibility, and week-reopen requests for after-the-fact corrections
- 🏖️ **Absence management** — vacation, sick leave, training, special leave, unpaid leave, and more, with a full approval workflow
- ✅ **Approval dashboard** — one place for team leads to act on submitted weeks, absence requests, and reopen requests
- 📅 **Team calendar** — shared view of who is absent, with public holiday context
- 📊 **Reports & CSV export** — monthly per-employee breakdowns, team-level summaries, and raw CSV for downstream processing
- 🔔 **Notifications** — in-app alerts and optional email reminders; automated submission nudges when monthly deadlines approach
- 🔐 **Encryption at rest** — database and backups encrypted with a single key you control
- 🌍 **English & German** — full UI and email translations out of the box

## 🏆 Why Zerf instead of the alternatives?

Most open-source time trackers are either too simple (just a timer, no team features) or too complex (full HR suites with payroll, recruiting, and multi-tenant overhead nobody asked for). Zerf hits the sweet spot:

| | Zerf | Typical SaaS | Generic HR suite |
|---|---|---|---|
| Self-hosted, data on your infra | ✅ | ❌ | sometimes |
| Absence + approval workflows | ✅ | limited | ✅ |
| Lightweight & fast to deploy | ✅ | n/a | ❌ |
| No subscription fee | ✅ | ❌ | ❌ |
| Encrypted database & backups | ✅ | trust vendor | varies |
| Focused scope — no bloat | ✅ | varies | ❌ |

One `docker compose up` and you're running. No external dependencies, no accounts to create, no data leaving your network.

## User documentation

Detailed usage guidance and workflow logic are documented in [docs/user-guide.md](docs/user-guide.md).

If you are new to Zerf, start there for:

- first-login and first-week onboarding,
- role-based workflows,
- status and approval logic,
- flextime and vacation balance behavior,
- practical answers for common edge cases.

## Quick setup

The application is deliberately small in scope and operationally simple: a Rust backend, a Svelte frontend, PostgreSQL, and Docker-based deployment.

### Prerequisites

- Docker and Docker Compose on a Linux host.
- `openssl` for secret generation.
- For public deployment: a domain pointing to the host and ports 80 and 443 reachable from the internet.

### 1. Clone and prepare the environment

Copy the example file and fill in the required values:

```bash
cp .env.example .env && chmod 600 .env
```

| Variable | Description |
| --- | --- |
| `ZERF_POSTGRES_USER` | Database user name |
| `ZERF_POSTGRES_DB` | Database name |
| `ZERF_SESSION_SECRET` | Random secret (≥ 32 chars) |
| `ZERF_POSTGRES_PASSWORD` | Database password |
| `ZERF_DB_ENCRYPTION_KEY` | Encryption key for database and backups — keep it safe |
| `ZERF_DOMAIN` | Public hostname (e.g. `zerf.example.com`) — public deployment only |

The three secret keys can be generated and written in one step:

```bash
sed -i "s|ZERF_SESSION_SECRET=.*|ZERF_SESSION_SECRET=$(openssl rand -hex 32)|" .env
sed -i "s|ZERF_POSTGRES_PASSWORD=.*|ZERF_POSTGRES_PASSWORD=$(openssl rand -hex 32)|" .env
sed -i "s|ZERF_DB_ENCRYPTION_KEY=.*|ZERF_DB_ENCRYPTION_KEY=$(openssl rand -hex 32)|" .env
```

#### Data encryption

`ZERF_DB_ENCRYPTION_KEY` protects data at two layers: the database is encrypted at rest, and every backup file is encrypted before being written to disk.

> **Keep this key safe.** Losing it while the stack is stopped makes the database and all backups permanently unreadable. Store it in a password manager or secrets vault alongside your `.env` file.

#### Restoring a backup

Use the interactive restore script:

```bash
./scripts/restore.sh                        # pick from available backups
./scripts/restore.sh path/to/file.dump.enc  # restore a specific file
```

The script decrypts the backup, stops the app to prevent mid-restore writes, restores the data, and tells you when to restart. On restart the app automatically applies any pending schema migrations.

**Migration compatibility:**
- Backup older than current code → the app applies pending migrations on start (safe).
- Backup newer than current code → update the app binary before restarting.

### 2. Start the stack

| Mode | Command | Use case |
| --- | --- | --- |
| Local | `./start_local.sh` | Personal or LAN use. The app is reachable at `http://localhost:3333`. No reverse proxy — HTTP only. |
| Public | `./start_public.sh` | Internet-facing deployment. Caddy handles HTTPS termination and serves the frontend. Requires a domain and a valid `ZERF_PUBLIC_URL`. |

### 3. Initial setup

On first launch, open the application in your browser. You will be prompted to create the initial administrator account with your email, name, and password.

### Demo data (optional)

Seeds a fresh (never-bootstrapped) database with realistic users, time entries, absences, and reopen-requests across all statuses — useful for evaluations and screencasts.

```bash
sudo apt install -y python3-psycopg2 python3-dotenv python3-argon2
python3 scripts/seed_test_data.py --yes
```

## Updating to a new version

Releases are published on the [GitHub Releases page](https://github.com/ramteid/zerf-work-time-tracking/releases). Set `ZERF_VERSION` in your `.env` file to control which version runs:

| Value | Behaviour |
|-------|-----------|
| `1.2.0` (pinned) | Runs that exact release — recommended for production |
| `latest` | Always follows the latest release |
| `dev` | Tracks the latest development build from `main` |

Then pull and restart:

```bash
docker compose -f docker/docker-compose-local.yml pull
docker compose -f docker/docker-compose-local.yml up -d
```

On restart the app automatically applies any pending database migrations.

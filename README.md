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

Self-hosted time tracking and absence management for small teams. Covers working hours, leave requests, approvals, and monthly reports. Data stays on your own infrastructure.

`Zerf` is derived from the German word *Zeiterfassung* — time tracking.

<p align="center">
  <img src="docs/screenshots/tour.gif" alt="Animated tour of Zerf: dashboard, time entry, absences, team calendar, and reports" width="320">
</p>

## 🗂️ Overview

Three roles: employees log hours and request leave, team leads approve time sheets and absences, admins manage users, categories, holidays, and settings. Every change is recorded in an audit log.

Works on desktop and mobile.

## ✨ Key features

- ⏱️ **Time tracking** — category-based daily entries, weekly submission, overtime visibility, and week-reopen requests for corrections after submission
- 🏖️ **Absence management** — vacation, sick leave, training, special leave, unpaid leave, and more, each with a request/approve/reject workflow
- ✅ **Approval dashboard** — submitted weeks, absence requests, and reopen requests in one view
- 📅 **Team calendar** — absence overview with public holiday context
- 📊 **Reports & CSV export** — monthly per-employee breakdowns, team-level summaries, CSV for downstream processing
- 🔔 **Notifications** — in-app and optional email; automated reminders when employees have unsubmitted months past a configured deadline
- 🔐 **Encryption** — database and backups encrypted at rest; one key covers both
- 🌍 **English & German**

## 🔒 Security

- 🦀 **Rust backend** — memory-safe by design
- 🔑 **Argon2id** password hashing; accounts lock after 5 failed attempts per 15 minutes
- 🍪 **Sessions** — 256-bit random tokens, HttpOnly + Secure + SameSite=Strict cookies, 8 h idle / 7 day absolute timeout
- 🛡️ **CSRF** — SameSite cookies + Origin/Referer check + X-CSRF-Token double-submit
- 🗄️ **Database encryption** — all tables and WAL segments encrypted at the storage layer via pg_tde; key never written to disk in plaintext
- 💾 **Backup encryption** — AES-256-CBC, same key as the database
- 📋 **Audit log** — before/after JSON snapshots for every mutation; passwords and secrets excluded
- 🌐 **Network isolation** — database container not reachable from outside the Docker network

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

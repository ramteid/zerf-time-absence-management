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

Simple but powerful self-hosted time tracking and absence management for teams.

Zerf covers working hours, leave and absence requests, approvals, and monthly reports in one operational tool. It supports the daily workflow between employees, team leads, and admins without expanding into a full HR or payroll suite.

`Zerf` is derived from the German word "Zeiterfassung" which means "time tracking".

<p align="center">
  <img src="docs/screenshots/tour.gif" alt="Animated tour of Zerf: dashboard, time entry, absences, team calendar, and reports" width="320">
</p>

## Overview

Zerf is built for day-to-day team operations.
Employees capture hours and absences, team leads review requests and submitted work, and admins manage the people and rules behind the process. The focus is on clear workflows, fast daily use on desktop or phone, and predictable self-hosted operation.

## Key features

- Time tracking with category-based entries, weekly submission, overtime visibility, and atomic week-level edit requests for after-the-fact corrections.
- Absence workflows for vacation, sick leave, training, special leave, and unpaid leave.
- Approval dashboard for submitted weeks, absence requests, and week reopen requests.
- Team calendar with shared absence visibility and holiday context.
- Reports for monthly employee breakdowns and team-level reporting.
- CSV export for report data and downstream processing.
- Role-based administration for users, categories, holidays, settings, and audit history.
- In-app notifications with optional SMTP-based email delivery.
- Automated submission reminders: on a configured deadline day each month, users who have not yet submitted all past months' time entries receive an in-app notification and, if SMTP is enabled, an email reminder.
- Self-hosted Docker deployment with a scripted backup utility that writes to a local Docker volume.
- Currently supported lanvuages: English, German

## How it differs from comparable software

- It is designed for teams that want focused operational workflows rather than a generic corporate HR suite.
- It focuses on time, absences, approvals, and reporting instead of bundling payroll, recruiting, or multi-tenant enterprise features.
- It is self-hosted by default, so data stays on your own infrastructure instead of in a SaaS service.
- It is easy to operate: the provided Docker Compose entrypoints cover local, debug, and public deployments.
- Start scripts pass the current Git commit into built images as `org.opencontainers.image.revision` and `ZERF_GIT_COMMIT`; backups include the same value in a metadata sidecar.
- It keeps the workflow opinionated and small, which reduces setup overhead for teams that want a practical operational tool instead of a broad platform.

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

```bash
cp .env.example .env && chmod 600 .env
sed -i "s|ZERF_SESSION_SECRET=.*|ZERF_SESSION_SECRET=$(openssl rand -hex 32)|" .env
sed -i "s|ZERF_POSTGRES_PASSWORD=.*|ZERF_POSTGRES_PASSWORD=$(openssl rand -hex 32)|" .env
sed -i "s|ZERF_DB_ENCRYPTION_KEY=.*|ZERF_DB_ENCRYPTION_KEY=$(openssl rand -hex 32)|" .env
```

Edit `.env` and set the remaining required values:

- `ZERF_POSTGRES_DB` and `ZERF_POSTGRES_USER`: choose any names for the database and user.
- `ZERF_DOMAIN`: required only for public deployment (`start_public.sh`) — set this to your public hostname (e.g. `zerf.example.com`). Not needed for local deployment.
- `ZERF_PUBLIC_URL`: required for password reset emails. The provided start scripts set it automatically for local and public deployments.

#### Data encryption

`ZERF_DB_ENCRYPTION_KEY` is a single key that protects data at two layers:

1. **Database at rest** — all tables and WAL segments are transparently encrypted by [pg_tde](https://docs.percona.com/pg-tde/) (AES-256). The pg_tde principal key is wrapped with `ZERF_DB_ENCRYPTION_KEY` and stored encrypted on disk; the plaintext key exists only in an in-memory tmpfs during a running container. No elevated container capabilities are required.
2. **Backups** — every automated backup file (`.dump.enc`) is encrypted with AES-256-CBC before being written to the backup volume. Decryption requires the same key.

> **Keep this key safe.** Losing `ZERF_DB_ENCRYPTION_KEY` while the stack is stopped makes the database and all encrypted backups permanently unreadable. Store it in a password manager or secrets vault alongside your `.env` file.

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

#### Migrating an existing deployment (adding encryption)

If you are upgrading from a version without encryption, follow these steps once:

```bash
# 1. Take an unencrypted dump from the currently running stack.
docker exec zerf-postgres pg_dump \
    -U "$ZERF_POSTGRES_USER" "$ZERF_POSTGRES_DB" \
    --format=custom > zerf-pre-encryption.dump

# 2. Stop the stack and remove the data volume.
docker compose -f docker/docker-compose-local.yml down
docker volume rm zerf_postgres_data

# 3. Set ZERF_DB_ENCRYPTION_KEY in .env and start the new stack.
#    The Percona-based postgres image initialises a fresh encrypted database.
./start_local.sh

# 4. Restore the unencrypted dump into the now-encrypted database.
docker cp zerf-pre-encryption.dump zerf-postgres:/tmp/pre-enc.dump
docker exec -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" zerf-postgres \
    pg_restore --host 127.0.0.1 \
               --username "$ZERF_POSTGRES_USER" \
               --dbname "$ZERF_POSTGRES_DB" \
               --no-owner --clean --if-exists \
               /tmp/pre-enc.dump
docker exec zerf-postgres rm /tmp/pre-enc.dump
```

### 2. Start the stack

| Mode | Command | Use case |
| --- | --- | --- |
| Local | `./start_local.sh` | Run the app locally at `http://localhost:3333` without the public reverse proxy. |
| Public | `./start_public.sh` | Run the public deployment stack with Caddy and HTTPS. |

### 3. Initial setup

On first launch, open the application in your browser. You will be prompted to create the initial administrator account with your email, name, and password.

### Demo data (optional)

For evaluations, demos, and screencasts there is a Python seeder that fills a *freshly migrated, never-bootstrapped* database with a complete and internally consistent set of test data — one user per role (admin, team lead, employee, assistant), several months of approved/submitted/draft time entries per user, mixed absences in every status (`vacation`, `sick`, `training`, `special_leave`, `unpaid`, `general_absence`, `flextime_reduction`, including `cancelled` and `cancellation_pending`), and reopen-requests covering the `pending`, `approved`, and `rejected` paths. Generation is deterministic (fixed RNG seed) so re-runs produce byte-identical data.

> **Safety guard.** The seeder refuses to run as soon as the `users` table contains any row — that is, as soon as someone has gone through the `/auth/setup` flow. There is no `--force` flag. To re-seed an already-bootstrapped deployment, drop the postgres data volume and redeploy first.

```bash
# On the host that runs the docker stack (no port-forwarding required —
# the script resolves the zerf-postgres container's docker IP on its own):
sudo apt install -y python3-psycopg2 python3-dotenv python3-argon2
python3 scripts/seed_test_data.py --yes

# Dry-run that connects and exercises the full insert path, then rolls back:
python3 scripts/seed_test_data.py --yes --dry-run
```

If `python3-psycopg2` / `python3-dotenv` are not packaged on your distro, install via pip instead:

```bash
pip install --break-system-packages psycopg2-binary argon2-cffi python-dotenv
```

The script writes one transaction; either every row lands or nothing does. Login passwords for the generated personas are printed to stderr on success.

## Updating to a new version

Released versions are published as Docker images on the GitHub Container Registry. To update a running deployment:

```bash
# Pin to a specific release (recommended for production)
# Set ZERF_VERSION=1.2.0 in your .env file, then:
docker compose -f docker/docker-compose-local.yml pull
docker compose -f docker/docker-compose-local.yml up -d

# Or always follow the latest release
# ZERF_VERSION=latest (the default) in .env
docker compose -f docker/docker-compose-local.yml pull
docker compose -f docker/docker-compose-local.yml up -d
```

On restart the app automatically applies any pending database migrations.

Available images (all tagged with the version and `latest`):

| Image | Purpose |
|-------|---------|
| `ghcr.io/ramteid/zerf-work-time-tracking` | Application (backend + frontend) |
| `ghcr.io/ramteid/zerf-work-time-tracking-postgres` | PostgreSQL with pg_tde encryption |
| `ghcr.io/ramteid/zerf-work-time-tracking-caddy` | Caddy reverse proxy (public deployment) |

## Cutting a release

Commits follow [Conventional Commits](https://www.conventionalcommits.org/) format — the changelog is generated automatically by [git-cliff](https://git-cliff.org):

```
feat: add CSV export for time entries
fix: session timeout after long idle periods
chore: update dependencies
```

Tag and push — the CI release workflow handles everything else:

```bash
git tag -a vX.Y.Z -m "Release vX.Y.Z"
git push origin vX.Y.Z
```

The release workflow injects the version into `Cargo.toml` and `package.json` (without committing), builds and pushes all three Docker images tagged with the version and `latest`, generates the changelog from commit history via git-cliff, and creates a GitHub Release with it as release notes.

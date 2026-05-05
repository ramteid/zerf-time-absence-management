# ZERF Work Time Tracking

Simple but powerful self-hosted time tracking and absence management for teams.

Zerf covers working hours, leave and absence requests, approvals, and monthly reports in one operational tool. It supports the daily workflow between employees, team leads, and admins without expanding into a full HR or payroll suite.

`Zerf` is derived from the German word "Zeiterfassung" which means "time tracking".

## Overview

Zerf is built for day-to-day team operations.
Employees capture hours and absences, team leads review requests and submitted work, and admins manage the people and rules behind the process. The focus is on clear workflows, fast daily use on desktop or phone, and predictable self-hosted operation.

## Key features

- Time tracking with category-based entries, weekly submission, overtime visibility, and change requests.
- Absence workflows for vacation, sick leave, training, special leave, and unpaid leave.
- Approval dashboard for submitted time, absence requests, change requests, and week reopen requests.
- Team calendar with shared absence visibility and holiday context.
- Reports for monthly employee breakdowns and team-level reporting.
- CSV export for report data and downstream processing.
- Role-based administration for users, categories, holidays, settings, and audit history.
- In-app notifications with optional SMTP-based email delivery.
- Self-hosted Docker deployment with automatic backup to a local Docker volume.

## How it differs from comparable software

- It is designed for teams that want focused operational workflows rather than a generic corporate HR suite.
- It focuses on time, absences, approvals, and reporting instead of bundling payroll, recruiting, or multi-tenant enterprise features.
- It is self-hosted by default, so data stays on your own infrastructure instead of in a SaaS service.
- It is easy to operate: the provided Docker Compose entrypoints cover local, debug, and public deployments.
- It keeps the workflow opinionated and small, which reduces setup overhead for teams that want a practical operational tool instead of a broad platform.

## Roles and approval model

The default reporting structure is many employees to one assigned team lead, and team leads can themselves report to another team lead for approval. Admins are primarily technical and organizational administrators. They can approve requests as a fallback, but they are not intended to be the regular approval path.

## Time and absence management

Employees log daily working hours by category and submit completed weeks for lead review. Flextime and overtime balances are tracked automatically based on configured weekly hours.

Absences include vacation, sick leave, training, special leave, general absence, and unpaid leave. Requests follow a standard approval flow; sick leave starting today or earlier is auto-approved. Vacation budgets support annual entitlements and carryover.

After submission, employees can request changes to reviewed entries or ask to reopen a week. Leads approve or reject these requests through the same dashboard used for time and absence reviews.

### Role organigram

```mermaid
flowchart TD
	Admin[Admin]
	LeadB[Approver team lead]
	LeadA[Team lead]

	subgraph TeamGroup[Operational team]
		E1[Employee 1]
		E2[Employee 2]
		EN[Employee n]
	end

	LeadA -->|primary approver for| E1
	LeadA -->|primary approver for| E2
	LeadA -->|primary approver for| EN
	LeadB -->|primary approver for| LeadA

	Admin -->|manages platform and users| LeadB
	Admin -. fallback approval only .-> LeadA
```

### Example approval flow

Admins can still approve requests for any user when needed, even though the normal operational approval path runs through assigned team leads.

```mermaid
flowchart LR
	Employee[Employee submits request]
	Lead[Assigned team lead]
	LeadApprover[Approver team lead]
	Approved[Approved]
	Rejected[Rejected]
	LeadOwn[Team lead submits own request]

	Employee -->|default review path| Lead
	Lead -->|approve| Approved
	Lead -->|reject| Rejected

	LeadOwn -->|default review path| LeadApprover
	LeadApprover -->|approve| Approved
	LeadApprover -->|reject| Rejected
```

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
```

Edit `.env` and set these values:

- `ZERF_DOMAIN` is required for public deployment.
- On first boot, open the application in your browser to create the initial admin account.

### 2. Start the stack

| Mode | Command | Use case |
| --- | --- | --- |
| Local | `./start_local.sh` | Run the app locally at `http://localhost:3000` without the public reverse proxy. |
| Local debug | `./start_local_debug.sh` | Run a debug-oriented local stack for backend and frontend debugging. |
| Public | `./start_public.sh` | Run the public deployment stack with Caddy and HTTPS. |

### 3. Sign in

Use the admin email from `.env` with the initial password `admin`.
Change that password immediately after the first login.

# End-to-end tests

A [Playwright](https://playwright.dev/) browser suite run against a **freshly
provisioned, production-like Docker stack** (PostgreSQL with pg_tde, the
Rust/Axum app, and the backup sidecar — the same services `start_local.sh`
brings up).

The bash script `run.sh` is the entry point: it boots the stack, waits for the
API, runs the suite in `tests/`, verifies the real backup/restore mechanism
(`backup-restore-check.sh`), then always tears the stack down.

## What the suite covers

The spec files run in order (numeric prefix) against one shared, evolving
database — each builds on state left by the previous ones:

| File | Covers |
|---|---|
| `01-bootstrap.spec.js` | First-admin setup, mandatory first-run settings |
| `02-admin-create-users.spec.js` | Admin creates a team lead and an employee |
| `03-admin-config.spec.js` | Categories, absence categories, holidays, audit log, SMTP settings |
| `04-team-lead-onboarding.spec.js` | Team lead password change, team settings, creates + edits an assistant |
| `05-employee-workflows.spec.js` | Time entries (add/edit/delete), week submit, absences, cancel, reports, calendar |
| `06-team-lead-approves-employee.spec.js` | Team lead approves/rejects the employee's week and absences |
| `07-assistant-workflows.spec.js` | Assistant password change, time entry, absence, permission boundaries |
| `08-team-lead-approves-assistant.spec.js` | Team lead approves the assistant's submissions |
| `09-employee-reopen-and-cancellation.spec.js` | Employee requests a reopen and an absence cancellation (verifies 06's approvals show up as status chips in the employee's own UI) |
| `10-team-lead-final-reviews.spec.js` | Team lead rejects a reopen request, approves a cancellation |
| `11-final-ui-state.spec.js` | Employee and assistant confirm, through their own UI, that every approve/reject/cancel decision landed as the right persistent status chip |
| `12-admin-user-lifecycle.spec.js` | Admin archives, restores, and resets a user's password |

`tests/helpers.js` holds the shared building blocks (sign-in, date/time picker
drivers, the credential store written/read across files). Each role's
authenticated session is saved with Playwright's `storageState` after its
first login, so later spec files resume the session instead of re-implementing
login.

### Backup/restore check

After the Playwright suite finishes (so there is real, varied data in the
database), `backup-restore-check.sh` exercises the actual backup and restore
mechanism described in `AGENTS.md` — not a re-implementation of it:

1. Triggers one backup cycle through `scripts/backup.sh`'s own
   `run_backup_once` (the same code the scheduled `backup` container runs),
   and confirms the `.dump.enc`, `.metadata`, and `.keyring.enc` files all
   exist and are non-empty.
2. Mutates the live database (inserts a throwaway holiday dated 2099-12-31)
   *after* the backup was taken.
3. Runs `scripts/restore.sh` — the same script an operator would run —
   non-interactively against that backup.
4. Confirms the mutation is gone (proving restore actually overwrote the live
   data, not a no-op) and every other table's row count matches what it was
   before the mutation.
5. Finally verifies the restored instance **through the UI**
   (`post-restore-ui-check.mjs`): drives a real browser to sign the admin in
   and confirm the restored team members actually render in the app — not
   just that row counts match at the data layer.

`scripts/restore.sh` normally targets a real deployment's fixed container
names, backup volume, and `.env` file. Three env vars
(`ZERF_RESTORE_POSTGRES_CONTAINER`, `ZERF_RESTORE_APP_CONTAINER`,
`ZERF_RESTORE_BACKUP_VOLUME`, `ZERF_RESTORE_ENV_FILE`) let this check point it
at the isolated e2e stack instead — all default to the production names, so
normal interactive use of the script is unaffected.

## Run it locally

Requires Docker (with the Compose plugin) and Node 22+. One-time setup:

```bash
cd e2e && npm install && npx playwright install chromium
```

Then, from the repo root:

```bash
./e2e/run.sh
```

First run builds the images and can take several minutes. On failure, Docker
logs print directly in the terminal and a Playwright HTML report is written to
`e2e/playwright-report/` (open with `npx playwright show-report`).

Set `ZERF_E2E_KEEP_UP=1` to leave the stack running after the test, e.g. to
iterate on a spec with `cd e2e && BASE_URL=http://localhost:3333 npx
playwright test --headed`. Tear it down afterwards with:

```bash
docker compose -p zerf_e2e --env-file e2e/.env.e2e \
  -f docker/docker-compose-local.yml -f e2e/docker-compose.e2e.yml down -v
```

### Isolation from your local stack

`run.sh` applies `docker-compose.e2e.yml` on top of the base local compose. The
base file pins fixed volume and container names, so without this overlay the
e2e teardown (`down -v`) would wipe a real `start_local.sh` database. The
overlay gives the e2e stack its own volumes (`zerf_e2e_*`) and container names
(`zerf-e2e-*`). It still publishes port `3333`, so stop a local dev stack first.

## CI

The `e2e` job in `.github/workflows/ci.yml` runs this on every push and pull
request, after the backend and frontend jobs pass.

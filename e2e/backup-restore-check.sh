#!/usr/bin/env bash
# Verifies the real backup + restore mechanism end-to-end against the
# already-running e2e stack (called from run.sh as the suite's final step,
# after the Playwright flow has populated the database with real data).
#
# What "verified" means here, concretely:
#   1. A backup cycle is triggered through the exact same code path the
#      scheduled backup container uses (scripts/backup.sh's run_backup_once),
#      not a hand-rolled pg_dump -- producing zerf-<ts>.zip, whose contents
#      (dump.enc, metadata, and keyring.enc) are each checked for existence
#      and non-zero size via unzip -l inside the backup container.
#   2. The live database is then mutated (a throwaway holiday row, inserted
#      *after* the backup was taken) so that a successful restore has
#      something observable to undo -- proving restore overwrites the live
#      DB rather than trivially "succeeding" against already-matching data.
#   3. scripts/restore.sh -- the same script an operator would run -- restores
#      that backup non-interactively (its two confirmation prompts are
#      answered via piped stdin).
#   4. Afterwards: the mutation is gone, every other table's row count
#      matches what it was before the mutation (nothing else was lost or
#      duplicated), and the app is healthy and serving again.
#
# Usage: backup-restore-check.sh <postgres-container> <app-container> \
#          <backup-container> <backup-volume> <env-file> <base-url>
set -euo pipefail

POSTGRES_CONTAINER="$1"
APP_CONTAINER="$2"
BACKUP_CONTAINER="$3"
BACKUP_VOLUME="$4"
ENV_FILE="$5"
BASE_URL="$6"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck disable=SC1090
source "$ENV_FILE"

# Tables whose row counts should be fully restored to their pre-mutation
# values. Deliberately excludes tables with their own background
# cleanup/rotation (sessions, login_attempts, notifications,
# password_reset_tokens) -- those aren't meaningful "did the backup preserve
# my data" signals. app_settings is safe to include: write_last_success_at
# (the only thing that touches it during a backup cycle) is called from
# main()'s loop, not from run_backup_once itself, so triggering just the
# latter -- as this script does -- never mutates it.
TABLES="users time_entries absences categories absence_categories holidays audit_log reopen_requests user_annual_leave app_settings"

psql_count() {
  docker exec -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" "$POSTGRES_CONTAINER" \
    psql -U "$ZERF_POSTGRES_USER" -d "$ZERF_POSTGRES_DB" -tAc "SELECT count(*) FROM $1"
}

snapshot() {
  for table in $TABLES; do
    printf '%s=%s\n' "$table" "$(psql_count "$table")"
  done
}

echo "Backup/restore check: snapshotting table row counts before backup..."
BEFORE_SNAPSHOT="$(snapshot)"
echo "$BEFORE_SNAPSHOT"

echo "Backup/restore check: triggering one backup cycle (same code path as the scheduled backup container)..."
# dash (the container shell) ignores arguments to the dot (.) builtin, so
# positional args cannot be used to set OUT_DIR when sourcing backup.sh.
# Set OUT_DIR explicitly after sourcing, as documented in backup.sh's header.
docker exec "$BACKUP_CONTAINER" sh -c \
  'BACKUP_LIB_ONLY=1 . /usr/local/bin/backup.sh && OUT_DIR=/backups && run_backup_once' \
  | tee /tmp/zerf-e2e-backup-output.txt
grep -q "^Backup written:" /tmp/zerf-e2e-backup-output.txt \
  || { echo "FAIL: backup did not report 'Backup written:'" >&2; exit 1; }

# Locate the most recent zip archive produced by the backup cycle.
ARCHIVE_FILE="$(docker exec "$BACKUP_CONTAINER" sh -c 'ls -1t /backups/zerf-*.zip | head -1')"

# Verify the archive exists and is non-empty.
ARCHIVE_SIZE="$(docker exec "$BACKUP_CONTAINER" sh -c "stat -c%s '$ARCHIVE_FILE' 2>/dev/null || echo 0")"
[ "$ARCHIVE_SIZE" -gt 0 ] \
  || { echo "FAIL: backup archive missing or empty: $ARCHIVE_FILE" >&2; exit 1; }
echo "  ok: $ARCHIVE_FILE ($ARCHIVE_SIZE bytes)"

# List the archive once and reuse that single listing for every entry check
# below, instead of shelling out to `docker exec ... unzip -l` again per
# entry (six invocations for three entries). Piping a fresh `docker exec`
# straight into `grep -q` per entry made this check flaky under CI load: if
# that particular exec call transiently failed or its output was dropped,
# `grep -q` silently saw empty input and reported the generic (and
# misleading) "archive does not contain entry" -- observed in CI as a
# *different* entry "missing" on each run, even though the zip -j log
# confirmed all three entries were written every time. Failing loudly here
# (via `docker exec`'s own exit status, `set -e`) and dumping the actual
# listing on a genuine mismatch makes a real content bug distinguishable
# from a transient exec hiccup.
ARCHIVE_LISTING="$(docker exec "$BACKUP_CONTAINER" sh -c "unzip -l '$ARCHIVE_FILE'")"
if [ -z "$ARCHIVE_LISTING" ]; then
  echo "FAIL: unzip -l returned no output for $ARCHIVE_FILE" >&2
  exit 1
fi

# Verify that all three expected entries are present inside the archive.
for entry in dump.enc metadata keyring.enc; do
  entry_line="$(printf '%s\n' "$ARCHIVE_LISTING" | grep "$entry" || true)"
  if [ -z "$entry_line" ]; then
    echo "FAIL: archive does not contain entry: $entry" >&2
    echo "--- unzip -l $ARCHIVE_FILE ---" >&2
    printf '%s\n' "$ARCHIVE_LISTING" >&2
    exit 1
  fi
  entry_size="$(printf '%s\n' "$entry_line" | awk '{print $1}')"
  [ "${entry_size:-0}" -gt 0 ] \
    || { echo "FAIL: archive entry is empty: $entry" >&2; exit 1; }
  echo "  ok: $ARCHIVE_FILE contains $entry ($entry_size bytes)"
done
echo "Backup/restore check: zip archive present and contains dump.enc, metadata, and keyring.enc."

echo "Backup/restore check: mutating the live database after the backup was taken..."
MARKER="E2E_BACKUP_MUTATION_MARKER"
docker exec -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" "$POSTGRES_CONTAINER" \
  psql -U "$ZERF_POSTGRES_USER" -d "$ZERF_POSTGRES_DB" -c \
  "INSERT INTO holidays (holiday_date, name, year, is_auto) VALUES ('2099-12-31', '$MARKER', 2099, false)" \
  >/dev/null
MUTATED_COUNT="$(psql_count holidays)"
echo "  holidays count after mutation: $MUTATED_COUNT"

echo "Backup/restore check: running scripts/restore.sh (non-interactive) against the e2e stack..."
# restore.sh reads from the backup volume directly when no path argument is
# given -- list is newest-first, so '1' selects the backup we just created.
# Pass BACKUP_CONTAINER so restore stops/restarts the backup daemon.
# Input: '1' = select newest backup, 'y' = confirm restore, 'y' = restart app.
printf '1\ny\ny\n' | ZERF_RESTORE_POSTGRES_CONTAINER="$POSTGRES_CONTAINER" \
  ZERF_RESTORE_APP_CONTAINER="$APP_CONTAINER" \
  ZERF_RESTORE_BACKUP_CONTAINER="$BACKUP_CONTAINER" \
  ZERF_RESTORE_BACKUP_VOLUME="$BACKUP_VOLUME" \
  ZERF_RESTORE_ENV_FILE="$ENV_FILE" \
  "$ROOT_DIR/scripts/restore.sh"

echo "Backup/restore check: waiting for the app to come back up..."
for _ in $(seq 1 30); do
  if curl -fsS "$BASE_URL/api/v1/settings/public" 2>/dev/null | grep -q ui_language; then
    echo "  app is healthy again."
    break
  fi
  sleep 2
done
curl -fsS "$BASE_URL/api/v1/settings/public" 2>/dev/null | grep -q ui_language \
  || { echo "FAIL: app did not become healthy after restore." >&2; exit 1; }

echo "Backup/restore check: verifying the mutation was undone and all data was restored..."
AFTER_HOLIDAYS_COUNT="$(psql_count holidays)"
MARKER_STILL_PRESENT="$(docker exec -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" "$POSTGRES_CONTAINER" \
  psql -U "$ZERF_POSTGRES_USER" -d "$ZERF_POSTGRES_DB" -tAc \
  "SELECT count(*) FROM holidays WHERE name = '$MARKER'")"
[ "$MARKER_STILL_PRESENT" = "0" ] \
  || { echo "FAIL: mutation marker survived the restore -- the DB was not actually overwritten." >&2; exit 1; }
echo "  ok: post-backup mutation is gone (holidays: $MUTATED_COUNT -> $AFTER_HOLIDAYS_COUNT)."

AFTER_SNAPSHOT="$(snapshot)"
if [ "$AFTER_SNAPSHOT" != "$BEFORE_SNAPSHOT" ]; then
  echo "FAIL: table row counts after restore do not match the pre-backup snapshot." >&2
  echo "--- before ---" >&2
  echo "$BEFORE_SNAPSHOT" >&2
  echo "--- after ---" >&2
  echo "$AFTER_SNAPSHOT" >&2
  exit 1
fi
echo "  ok: every table's row count matches the pre-backup snapshot exactly."

echo "Backup/restore check: verifying the restored instance through the UI..."
node "$SCRIPT_DIR/post-restore-ui-check.mjs" "$BASE_URL"

echo "Backup/restore check passed: backup produced a valid zip archive (dump.enc + metadata + keyring.enc), restore overwrote live data with it, the data layer matches the pre-backup snapshot, and the app is fully functional in the browser again."

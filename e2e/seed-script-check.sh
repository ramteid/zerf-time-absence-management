#!/usr/bin/env bash
# Verifies scripts/seed_test_data.py against the freshly booted, still-empty
# e2e stack. Called from run.sh right after the API becomes reachable and
# BEFORE the Playwright flow, because the seed script's hard safety guard
# refuses to run against any database that already has a user row -- and
# 01-bootstrap.spec.js completing the admin setup flow would create exactly
# that row.
#
# What "verified" means here, concretely:
#   1. A --dry-run pass succeeds and rolls back (still zero users afterwards).
#   2. A real run commits and exits 0.
#   3. Row counts for users/time_entries/absences/reopen_requests match what
#      the script's own PERSONAS/ABSENCE_SCRIPT/REOPEN_SCRIPT tables define.
#   4. Every seeded persona can actually authenticate through the real HTTP
#      API (not just "a row exists with *a* password hash").
#   5. Running the script again against the now-seeded database refuses --
#      the safety guard actually guards.
#
# Usage: seed-script-check.sh <postgres-container> <env-file> <base-url>
set -euo pipefail

POSTGRES_CONTAINER="$1"
ENV_FILE="$2"
BASE_URL="$3"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# shellcheck disable=SC1090
source "$ENV_FILE"

psql_count() {
  docker exec -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" "$POSTGRES_CONTAINER" \
    psql -U "$ZERF_POSTGRES_USER" -d "$ZERF_POSTGRES_DB" -tAc "SELECT count(*) FROM $1"
}

echo "Seed script check: confirming the database is empty..."
[ "$(psql_count users)" = "0" ] \
  || { echo "FAIL: expected an empty database before seeding, found existing users." >&2; exit 1; }

VENV_DIR="$(mktemp -d)"
cleanup_venv() { rm -rf "$VENV_DIR"; }
trap cleanup_venv EXIT

echo "Seed script check: installing seeder dependencies into a throwaway venv..."
python3 -m venv "$VENV_DIR"
"$VENV_DIR/bin/pip" install --quiet psycopg2-binary argon2-cffi python-dotenv

# PGHOST overrides the script's "zerf-postgres" default to this stack's actual
# container name (zerf-e2e-postgres) -- resolved to a routable docker IP by
# the script's own resolve_docker_container_ip(), exactly as it would be on
# the real prod host against the internal-only docker_private network.
run_seed() {
  PGHOST="$POSTGRES_CONTAINER" "$VENV_DIR/bin/python3" \
    "$ROOT_DIR/scripts/seed_test_data.py" --env-file "$ENV_FILE" "$@"
}

echo "Seed script check: dry-run pass (must roll back cleanly)..."
run_seed --yes --dry-run
[ "$(psql_count users)" = "0" ] \
  || { echo "FAIL: --dry-run left rows behind -- it did not roll back." >&2; exit 1; }
echo "  ok: dry-run rolled back, database still empty."

echo "Seed script check: real run..."
run_seed --yes

USERS_COUNT="$(psql_count users)"
[ "$USERS_COUNT" = "4" ] \
  || { echo "FAIL: expected 4 seeded users, found $USERS_COUNT." >&2; exit 1; }
echo "  ok: $USERS_COUNT users."

TIME_ENTRIES_COUNT="$(psql_count time_entries)"
[ "$TIME_ENTRIES_COUNT" -gt "0" ] \
  || { echo "FAIL: expected time entries to be seeded, found none." >&2; exit 1; }
echo "  ok: $TIME_ENTRIES_COUNT time entries."

# Must match len(ABSENCE_SCRIPT) / len(REOPEN_SCRIPT) in seed_test_data.py.
ABSENCES_COUNT="$(psql_count absences)"
[ "$ABSENCES_COUNT" = "17" ] \
  || { echo "FAIL: expected 17 seeded absences, found $ABSENCES_COUNT." >&2; exit 1; }
echo "  ok: $ABSENCES_COUNT absences."

REOPEN_COUNT="$(psql_count reopen_requests)"
[ "$REOPEN_COUNT" = "5" ] \
  || { echo "FAIL: expected 5 seeded reopen requests, found $REOPEN_COUNT." >&2; exit 1; }
echo "  ok: $REOPEN_COUNT reopen requests."

echo "Seed script check: verifying every seeded persona can sign in through the real API..."
for cred in \
  "arnold.admin@waldkindergarten-gundelfingen.de:Admin!Pass-2026" \
  "tabea.teamlead@waldkindergarten-gundelfingen.de:TeamLead!2026" \
  "eva.employee@waldkindergarten-gundelfingen.de:Employee!2026" \
  "alina.assistant@waldkindergarten-gundelfingen.de:Assistant!2026"
do
  email="${cred%%:*}"
  password="${cred#*:}"
  status="$(curl -sS -o /dev/null -w '%{http_code}' -X POST "$BASE_URL/api/v1/auth/login" \
    -H 'Content-Type: application/json' \
    -d "{\"email\":\"$email\",\"password\":\"$password\"}")"
  [ "$status" = "200" ] \
    || { echo "FAIL: login failed for $email (HTTP $status)." >&2; exit 1; }
  echo "  ok: $email logs in."
done

echo "Seed script check: confirming the safety guard refuses a re-run against the now-seeded database..."
RERUN_LOG="$(mktemp)"
if run_seed --yes >"$RERUN_LOG" 2>&1; then
  echo "FAIL: seed script did not refuse to run against an already-seeded database." >&2
  cat "$RERUN_LOG" >&2
  rm -f "$RERUN_LOG"
  exit 1
fi
rm -f "$RERUN_LOG"
echo "  ok: safety guard refused the re-run."

echo "Seed script check passed: seeder produced the expected users/time entries/absences/reopen requests, every persona can log in, --dry-run rolled back cleanly, and the safety guard blocks re-seeding."

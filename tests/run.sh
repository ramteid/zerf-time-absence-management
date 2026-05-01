#!/usr/bin/env bash
# KitaZeit browser smoke test runner.
#
# Boots a clean PostgreSQL + app test stack on a private port (no Caddy,
# no public DNS), then runs a headless browser smoke test (Puppeteer in
# Docker) to verify the frontend loads correctly.
#
# The comprehensive API integration tests have been moved to native Rust
# tests in backend/tests/integration.rs.
#
# Usage:  bash tests/run.sh
# Exit code is non-zero if any assertion fails.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CONTAINER=kitazeit-it
DB_CONTAINER=kitazeit-it-postgres
NETWORK=kitazeit-it
DB_VOLUME=kitazeit-it-dbdata
DB_NAME=kitazeit_it
DB_USER=kitazeit_it
DB_PASSWORD=integration-test-db-password-32-chars
PORT=${KITAZEIT_TEST_PORT:-3137}
BASE="http://127.0.0.1:$PORT"
# Allow CI to pass a pre-built image via the IMG environment variable so the
# build step below is skipped (the image is already present in the daemon).
IMG=${IMG:-zerf2-app:latest}

PASS=0; FAIL=0
banner(){ printf "\n\033[1;36m== %s ==\033[0m\n" "$*"; }
ok()    { PASS=$((PASS+1)); printf "  \033[32m✓\033[0m %s\n" "$*"; }
bad()   { FAIL=$((FAIL+1)); printf "  \033[31m✗\033[0m %s\n" "$*"; }

cleanup(){
  docker rm -f "$CONTAINER" "$DB_CONTAINER" >/dev/null 2>&1 || true
  docker volume rm -f "$DB_VOLUME" >/dev/null 2>&1 || true
  docker network rm "$NETWORK" >/dev/null 2>&1 || true
}
trap cleanup EXIT

# Skip the build when a pre-built image is already present in the daemon
# (e.g. loaded by CI via docker/build-push-action before calling this script).
if docker image inspect "$IMG" >/dev/null 2>&1; then
  ok "using pre-built image ($IMG)"
else
  banner "Build app image (cached layers reused)"
  DOCKER_BUILDKIT=0 docker build -q -t "$IMG" "$ROOT" >/dev/null
  ok "image built"
fi

banner "Start ephemeral PostgreSQL + app stack on :$PORT"
cleanup
docker network create "$NETWORK" >/dev/null
docker volume create "$DB_VOLUME" >/dev/null
docker run -d --name "$DB_CONTAINER" \
  --network "$NETWORK" \
  -e POSTGRES_DB="$DB_NAME" \
  -e POSTGRES_USER="$DB_USER" \
  -e POSTGRES_PASSWORD="$DB_PASSWORD" \
  -e POSTGRES_INITDB_ARGS="--auth-host=scram-sha-256 --auth-local=scram-sha-256 --data-checksums" \
  -v "$DB_VOLUME:/var/lib/postgresql/data" \
  postgres:16-alpine \
  postgres -c password_encryption=scram-sha-256 -c ssl=off -c idle_in_transaction_session_timeout=30000 -c statement_timeout=30000 >/dev/null
ok "database container started ($DB_CONTAINER)"
for i in $(seq 1 60); do
  if docker exec "$DB_CONTAINER" pg_isready -U "$DB_USER" -d "$DB_NAME" -h 127.0.0.1 >/dev/null 2>&1; then ok "database ready after ${i}x250ms"; break; fi
  sleep 0.25
  if [ "$i" = 60 ]; then bad "database did not become ready"; docker logs "$DB_CONTAINER"; exit 1; fi
done

docker run -d --name "$CONTAINER" \
  --network "$NETWORK" \
  -p 127.0.0.1:$PORT:3000 \
  --user 10001:10001 \
  --read-only --tmpfs /tmp:size=16m \
  --cap-drop=ALL --security-opt=no-new-privileges:true \
  -e KITAZEIT_DATABASE_URL="postgres://${DB_USER}:${DB_PASSWORD}@${DB_CONTAINER}:5432/${DB_NAME}?sslmode=disable" \
  -e KITAZEIT_SESSION_SECRET=integration-test-secret-do-not-use-in-prod-32-characters \
  -e KITAZEIT_ADMIN_EMAIL=admin@example.com \
  -e KITAZEIT_ORGANIZATION_NAME="Integration Test" \
  -e KITAZEIT_REGION=BW \
  -e KITAZEIT_DEV=1 \
  -e KITAZEIT_SECURE_COOKIES=false \
  -e KITAZEIT_ENFORCE_CSRF=false \
  -e KITAZEIT_ENFORCE_ORIGIN=false \
  "$IMG" >/dev/null
ok "app container started ($CONTAINER)"

# Wait for readiness
for i in $(seq 1 120); do
  if curl -fsS "$BASE/healthz" -o /dev/null 2>/dev/null; then ok "ready after ${i}x250ms"; break; fi
  sleep 0.25
  if [ "$i" = 120 ]; then bad "container did not become ready"; docker logs "$CONTAINER"; exit 1; fi
done

# ---------------------------------------------------------------------------
# Browser smoke test (only if Docker can pull the puppeteer image)
# ---------------------------------------------------------------------------
banner "Browser smoke (Puppeteer in Docker)"
if docker image inspect ghcr.io/puppeteer/puppeteer:22 >/dev/null 2>&1 || docker pull -q ghcr.io/puppeteer/puppeteer:22 >/dev/null 2>&1; then
  OUT=$(docker run --rm --network host \
    -e NODE_PATH=/home/pptruser/node_modules \
    -e URL="$BASE/" \
    -v "$ROOT/scripts:/s" \
    ghcr.io/puppeteer/puppeteer:22 node /s/browser_test.js 2>&1)
  echo "$OUT" | sed 's/^/    /'
  echo "$OUT" | grep -q "^HTTP 200"             && ok "browser HTTP 200"          || bad "no HTTP 200"
  echo "$OUT" | grep -q "title: KitaZeit"       && ok "title=KitaZeit"            || bad "wrong title"
  echo "$OUT" | grep -q "<form>"                && ok "login form rendered"       || bad "form missing"
  echo "$OUT" | grep -q "Maximum call stack"    && bad "infinite recursion!"      || ok "no infinite recursion"
  ! echo "$OUT" | grep -q "^pageerror:"          && ok "no page errors"            || bad "page errors present"
else
  printf "  (skipped — could not pull puppeteer image)\n"
fi

# ---------------------------------------------------------------------------
echo
printf "\033[1mResult: %d passed, %d failed\033[0m\n" "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]

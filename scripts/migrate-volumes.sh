#!/bin/bash
set -euo pipefail

# One-time migration: re-create Docker volumes under project "zerf".
#
# When the compose project name changed from "docker" (derived from the
# docker/ directory) to "zerf" (explicit name: zerf in compose file),
# Docker began warning that the named volumes were created by a different
# project. This script fixes that by backing up all four volumes, removing
# them, letting Docker Compose recreate them with the correct project label,
# and then restoring the data.
#
# Run once from the repo root on any host that ran the stack before the
# project name was pinned to "zerf".
#
# Usage: bash scripts/migrate-volumes.sh

cd "$(dirname "$0")/.."

if [ ! -f .env ]; then
  echo "ERROR: .env not found. Run from the repo root." >&2
  exit 1
fi

if [ -z "${ZERF_GIT_COMMIT:-}" ] && git_commit="$(git rev-parse --verify HEAD 2>/dev/null)"; then
  export ZERF_GIT_COMMIT="$git_commit"
fi

COMPOSE_FILES="-f docker/docker-compose-local.yml -f docker/docker-compose-public.yml"
TMPDIR_VOLS=/tmp/zerf-volume-migration
mkdir -p "$TMPDIR_VOLS"

backup_volume() {
  local vol="$1"
  echo "  backing up $vol"
  docker run --rm \
    -v "${vol}:/src:ro" \
    -v "${TMPDIR_VOLS}:/out" \
    alpine tar -czf "/out/${vol}.tar.gz" -C /src .
}

restore_volume() {
  local vol="$1"
  echo "  restoring $vol"
  docker run --rm \
    -v "${vol}:/dst" \
    -v "${TMPDIR_VOLS}:/in:ro" \
    alpine sh -c "cd /dst && tar -xzf /in/${vol}.tar.gz"
}

echo "=== 1/5  Back up all volumes ==="
backup_volume zerf_postgres_data
backup_volume zerf_backup_data
backup_volume zerf_caddy_data
backup_volume zerf_caddy_config

echo "=== 2/5  Stop the stack ==="
# shellcheck disable=SC2086
docker compose $COMPOSE_FILES --env-file .env down

echo "=== 3/5  Remove old volumes ==="
docker volume rm zerf_postgres_data zerf_backup_data zerf_caddy_data zerf_caddy_config

echo "=== 4/5  Create fresh volumes under project 'zerf' (no start) ==="
# shellcheck disable=SC2086
docker compose $COMPOSE_FILES --env-file .env up --no-start --build

echo "=== 5/5  Restore data ==="
restore_volume zerf_postgres_data
restore_volume zerf_backup_data
restore_volume zerf_caddy_data
restore_volume zerf_caddy_config

echo ""
echo "Starting the stack..."
# shellcheck disable=SC2086
docker compose $COMPOSE_FILES --env-file .env start

echo ""
echo "Migration complete. Backups left in ${TMPDIR_VOLS} — remove when satisfied."

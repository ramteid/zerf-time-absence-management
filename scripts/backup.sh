#!/bin/sh
# KitaZeit PostgreSQL backup helper.
#
# Usage:  sh scripts/backup.sh [OUTPUT_DIR]
# Intended for the dedicated backup container service or other one-off runs
# with explicit PostgreSQL connection settings.
#
# Optional env:
#   BACKUP_INTERVAL_SECONDS - if set to a positive integer, keep running and
#                             create a new backup after each interval.
#   BACKUP_RETENTION_DAYS   - delete older snapshots (default 30)
#   PGHOST / PGPORT / PGDATABASE / PGUSER / PGPASSWORD
#   KITAZEIT_POSTGRES_HOST / KITAZEIT_POSTGRES_PORT / KITAZEIT_POSTGRES_DB
#   KITAZEIT_POSTGRES_USER / KITAZEIT_POSTGRES_PASSWORD
set -eu
umask 077

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${1:-$ROOT/backups}"
INTERVAL="${BACKUP_INTERVAL_SECONDS:-}"
RETENTION="${BACKUP_RETENTION_DAYS:-30}"
mkdir -p "$OUT_DIR"
chmod 700 "$OUT_DIR"

DIRECT_HOST=""
DIRECT_PORT=""
DIRECT_DB=""
DIRECT_USER=""
DIRECT_PASSWORD=""

validate_interval() {
  if [ -z "$INTERVAL" ]; then
    return 0
  fi

  case "$INTERVAL" in
    *[!0-9]*|'')
      echo "BACKUP_INTERVAL_SECONDS must be a positive integer." >&2
      return 1
      ;;
  esac

  if [ "$INTERVAL" -le 0 ]; then
    echo "BACKUP_INTERVAL_SECONDS must be greater than zero." >&2
    return 1
  fi
}

validate_retention() {
  case "$RETENTION" in
    *[!0-9]*|'')
      echo "BACKUP_RETENTION_DAYS must be a non-negative integer." >&2
      return 1
      ;;
  esac
}

resolve_direct_connection() {
  DIRECT_HOST="${PGHOST:-${KITAZEIT_POSTGRES_HOST:-${POSTGRES_HOST:-}}}"
  DIRECT_PORT="${PGPORT:-${KITAZEIT_POSTGRES_PORT:-${POSTGRES_PORT:-5432}}}"
  DIRECT_DB="${PGDATABASE:-${KITAZEIT_POSTGRES_DB:-${POSTGRES_DB:-}}}"
  DIRECT_USER="${PGUSER:-${KITAZEIT_POSTGRES_USER:-${POSTGRES_USER:-}}}"
  DIRECT_PASSWORD="${PGPASSWORD:-${KITAZEIT_POSTGRES_PASSWORD:-${POSTGRES_PASSWORD:-}}}"

  [ -n "$DIRECT_HOST" ] &&
    [ -n "$DIRECT_DB" ] &&
    [ -n "$DIRECT_USER" ] &&
    [ -n "$DIRECT_PASSWORD" ]
}

run_direct_pg_dump() {
  command -v pg_dump >/dev/null 2>&1 || return 1
  resolve_direct_connection || return 1

  PGPASSWORD="$DIRECT_PASSWORD" \
    pg_dump \
      --host "$DIRECT_HOST" \
      --port "$DIRECT_PORT" \
      --username "$DIRECT_USER" \
      --dbname "$DIRECT_DB" \
      --format=custom \
      --no-owner \
      --no-privileges
}

apply_retention() {
  find "$OUT_DIR" -type f -name 'kitazeit-*.dump' \
    -mtime "+$RETENTION" \
    -exec rm -f {} \;
}

run_backup_once() {
  validate_retention || return 1

  ts="$(date -u +%Y%m%dT%H%M%SZ)"
  output_file="$OUT_DIR/kitazeit-$ts.dump"
  temp_file="$output_file.tmp"

  rm -f "$temp_file"

  if ! run_direct_pg_dump > "$temp_file"; then
    rm -f "$temp_file"
    echo "PostgreSQL connection settings are incomplete or pg_dump is unavailable." >&2
    return 1
  fi

  chmod 600 "$temp_file"
  mv "$temp_file" "$output_file"

  apply_retention
  echo "Backup written: $output_file"
}

validate_interval || exit 1
run_backup_once

if [ -z "$INTERVAL" ]; then
  exit 0
fi

while :; do
  sleep "$INTERVAL"
  run_backup_once
done

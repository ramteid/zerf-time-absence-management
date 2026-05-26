#!/bin/sh
# Zerf PostgreSQL backup helper.
#
# Usage:  sh scripts/backup.sh [OUTPUT_DIR]
# Intended for the dedicated backup container service or other one-off runs
# with explicit PostgreSQL connection settings.
#
# Required env:
#   ZERF_DB_ENCRYPTION_KEY  - passphrase for AES-256-CBC backup encryption
#                             (same key used by the Postgres container for
#                              gocryptfs data-at-rest encryption).
#                             Generate with: openssl rand -hex 32
#
# Optional env:
#   BACKUP_INTERVAL_SECONDS - if set to a positive integer, keep running and
#                             create a new backup after each interval.
#   BACKUP_RETENTION_DAYS   - delete older snapshots (default 30)
#   PGHOST / PGPORT / PGDATABASE / PGUSER / PGPASSWORD
#   ZERF_POSTGRES_HOST / ZERF_POSTGRES_PORT / ZERF_POSTGRES_DB
#   ZERF_POSTGRES_USER / ZERF_POSTGRES_PASSWORD
#   ZERF_GIT_COMMIT        - written to the backup metadata sidecar
set -eu
umask 077

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${1:-$ROOT/backups}"
INTERVAL="${BACKUP_INTERVAL_SECONDS:-}"
RETENTION="${BACKUP_RETENTION_DAYS:-30}"
mkdir -p "$OUT_DIR"
chmod 700 "$OUT_DIR"

# ZERF_DB_ENCRYPTION_KEY must be a non-empty string used to derive the AES-256
# backup encryption key via PBKDF2. Generate with: openssl rand -hex 32
ENCRYPTION_KEY="${ZERF_DB_ENCRYPTION_KEY:-}"

DIRECT_HOST=""
DIRECT_PORT=""
DIRECT_DB=""
DIRECT_USER=""
DIRECT_PASSWORD=""

validate_encryption_key() {
  if [ -z "$ENCRYPTION_KEY" ]; then
    echo "ZERF_DB_ENCRYPTION_KEY must be set in .env (generate with: openssl rand -hex 32)." >&2
    return 1
  fi
}

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
      echo "BACKUP_RETENTION_DAYS must be a positive integer." >&2
      return 1
      ;;
  esac
  if [ "$RETENTION" -eq 0 ]; then
    echo "BACKUP_RETENTION_DAYS must be greater than zero." >&2
    return 1
  fi
}

resolve_direct_connection() {
  DIRECT_HOST="${PGHOST:-${ZERF_POSTGRES_HOST:-${POSTGRES_HOST:-}}}"
  DIRECT_PORT="${PGPORT:-${ZERF_POSTGRES_PORT:-${POSTGRES_PORT:-5432}}}"
  DIRECT_DB="${PGDATABASE:-${ZERF_POSTGRES_DB:-${POSTGRES_DB:-}}}"
  DIRECT_USER="${PGUSER:-${ZERF_POSTGRES_USER:-${POSTGRES_USER:-}}}"
  DIRECT_PASSWORD="${PGPASSWORD:-${ZERF_POSTGRES_PASSWORD:-${POSTGRES_PASSWORD:-}}}"

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

metadata_value() {
  if [ -z "${1:-}" ]; then
    printf 'unknown'
    return 0
  fi

  printf '%s' "$1" | tr '\n\r' '  '
}

write_backup_metadata() {
  target_file="$1"
  created_at="$2"

  resolve_direct_connection || return 1

  {
    printf 'backup_format=pg_dump_custom\n'
    printf 'created_at_utc=%s\n' "$(metadata_value "$created_at")"
    printf 'ZERF_GIT_COMMIT=%s\n' "$(metadata_value "${ZERF_GIT_COMMIT:-unknown}")"
    printf 'PGHOST=%s\n' "$(metadata_value "$DIRECT_HOST")"
    printf 'PGPORT=%s\n' "$(metadata_value "$DIRECT_PORT")"
    printf 'PGDATABASE=%s\n' "$(metadata_value "$DIRECT_DB")"
    printf 'PGUSER=%s\n' "$(metadata_value "$DIRECT_USER")"
  } > "$target_file"
}

apply_retention() {
  find "$OUT_DIR" -type f \( -name 'zerf-*.dump.enc' -o -name 'zerf-*.metadata' \) \
    -mtime "+$RETENTION" \
    -exec rm -f {} +
}

run_backup_once() {
  validate_retention || return 1

  # Sweep stale temp files from previous runs interrupted by SIGTERM/SIGKILL.
  # The retention pattern matches only finalized snapshots, so orphan .tmp
  # files would otherwise accumulate forever in the backup volume.  The
  # `dump.plain.tmp` pattern is kept here to clean up leftovers from older
  # versions of this script that staged plaintext in $OUT_DIR.
  find "$OUT_DIR" -maxdepth 1 -type f \
    \( -name 'zerf-*.dump.enc.tmp' -o -name 'zerf-*.dump.plain.tmp' -o -name 'zerf-*.metadata.tmp' \) \
    -exec rm -f {} +

  ts="$(date -u +%Y%m%dT%H%M%SZ)"
  # Backup files are AES-256-CBC encrypted via openssl; the .enc suffix makes
  # this explicit so operators know decryption is required before pg_restore.
  output_file="$OUT_DIR/zerf-$ts.dump.enc"
  metadata_file="$OUT_DIR/zerf-$ts.metadata"
  # Stage the plaintext dump in $TMPDIR (defaults to /tmp), NOT in $OUT_DIR.
  # The backup container mounts /tmp as a RAM-backed tmpfs (compose), so the
  # plaintext copy never touches the persistent backup volume.  Even if the
  # container is killed mid-dump, the plaintext disappears with the tmpfs.
  if ! plain_temp_file="$(mktemp "${TMPDIR:-/tmp}/zerf-$ts.dump.plain.XXXXXX")"; then
    echo "Failed to create plaintext temp file in ${TMPDIR:-/tmp}." >&2
    return 1
  fi
  chmod 600 "$plain_temp_file"
  temp_file="$output_file.tmp"
  temp_metadata_file="$metadata_file.tmp"

  # Step 1: dump to the in-RAM plaintext temp file.
  if ! run_direct_pg_dump > "$plain_temp_file"; then
    rm -f "$plain_temp_file"
    echo "PostgreSQL connection settings are incomplete or pg_dump is unavailable." >&2
    return 1
  fi

  # pg_dump should never exit 0 with empty output, but if a future bug or odd
  # signal handling produces that combination, encrypting 0 bytes would silently
  # advance the backup timestamp and (under retention) push out a real backup.
  # Reject it explicitly so monitoring catches the broken state.
  if [ ! -s "$plain_temp_file" ]; then
    rm -f "$plain_temp_file"
    echo "pg_dump produced empty output — refusing to encrypt a zero-byte backup." >&2
    return 1
  fi

  # Step 2: encrypt the plaintext dump.  AES-256-CBC with a PBKDF2-derived key
  # (100 000 iterations) prevents the backup from being read without the key.
  # The passphrase is read from the environment variable by openssl so it never
  # appears in process arguments.
  if ! openssl enc -aes-256-cbc -salt -pbkdf2 -iter 100000 \
      -pass env:ZERF_DB_ENCRYPTION_KEY \
      -in  "$plain_temp_file" \
      -out "$temp_file"; then
    rm -f "$plain_temp_file" "$temp_file"
    echo "Failed to encrypt backup." >&2
    return 1
  fi

  # Remove the plaintext file as soon as the encrypted copy exists.
  rm -f "$plain_temp_file"

  if ! write_backup_metadata "$temp_metadata_file" "$ts"; then
    rm -f "$temp_file" "$temp_metadata_file"
    echo "Failed to write backup metadata." >&2
    return 1
  fi

  chmod 600 "$temp_file" "$temp_metadata_file"
  if ! mv "$temp_file" "$output_file"; then
    rm -f "$temp_file" "$temp_metadata_file"
    echo "Failed to finalize backup file." >&2
    return 1
  fi
  if ! mv "$temp_metadata_file" "$metadata_file"; then
    rm -f "$output_file" "$temp_metadata_file"
    echo "Failed to finalize backup metadata." >&2
    return 1
  fi

  apply_retention
  echo "Backup written: $output_file"
  echo "Backup metadata written: $metadata_file"
}

validate_encryption_key || exit 1
validate_interval || exit 1

if [ -z "$INTERVAL" ]; then
  # One-shot mode: propagate failure so ad-hoc callers see a non-zero exit.
  run_backup_once
  exit 0
fi

# Daemon mode: tolerate failures on every attempt — including the first —
# so a transient pg_dump error on startup does not turn into a Docker
# restart loop against `restart: unless-stopped`. The container stays up
# and the next scheduled attempt still fires.
run_backup_once || echo "Initial backup attempt failed; will retry in ${INTERVAL}s." >&2

while :; do
  sleep "$INTERVAL"
  run_backup_once || echo "Backup attempt failed; will retry in ${INTERVAL}s." >&2
done

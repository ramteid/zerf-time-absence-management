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
#                              pg_tde transparent data encryption).
#                             Generate with: openssl rand -hex 32
#
# Optional env (DB connection):
#   PGHOST / PGPORT / PGDATABASE / PGUSER / PGPASSWORD
#   ZERF_POSTGRES_HOST / ZERF_POSTGRES_PORT / ZERF_POSTGRES_DB
#   ZERF_POSTGRES_USER / ZERF_POSTGRES_PASSWORD
#   ZERF_GIT_COMMIT        - written to the backup metadata sidecar
#
# Frequency and Nextcloud upload settings are read from the app_settings table
# at the start of each backup cycle (not from env).
# Hard-coded last-resort defaults (1 day) apply only when the database is not
# yet available (bootstrap race on first start).
#
# Local retention: always keeps the 10 most recent encrypted backup files.
# Uploaded files in Nextcloud are never deleted automatically.
#
# Each backup cycle writes up to three sibling files in OUTPUT_DIR:
#   zerf-<ts>.dump.enc     encrypted pg_dump custom dump (logical restore)
#   zerf-<ts>.metadata     plaintext metadata sidecar
#   zerf-<ts>.keyring.enc  copy of the (already AES-encrypted) pg_tde keyring,
#                          for physical recovery of an orphaned encrypted PGDATA
#                          volume. Only written when the keyring volume is
#                          mounted at /keyring-src (see KEYRING_SRC); its absence
#                          is a warning, never a failure.
#
# Sourcing:  set BACKUP_LIB_ONLY=1 before sourcing to load helper functions
# without starting the daemon loop -- used by automated tests (backup.bats).
set -eu
umask 077

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${1:-$ROOT/backups}"

ENCRYPTION_KEY="${ZERF_DB_ENCRYPTION_KEY:-}"

# Path to the pg_tde keyring as seen from inside the backup container.  The
# compose file mounts the postgres keyring volume (zerf_postgres_data) read-only
# at /keyring-src, which contains only the already-AES-encrypted keyring file
# (and an empty db/ mountpoint) -- not the data directory.  Each backup copies
# this keyring alongside the dump so that an orphaned, encrypted PGDATA volume
# can still be recovered (physical recovery) even if the keyring volume itself
# is later lost or overwritten.  Overridable for tests.
KEYRING_SRC="${ZERF_KEYRING_SRC:-/keyring-src/pg_tde_keyring.enc}"

# -- Database connection -------------------------------------------------------

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

# -- App-settings helpers ------------------------------------------------------

# Read a single key from app_settings via psql.  Returns empty string (not an
# error) when the table/row does not yet exist -- the DB migration may not have
# run yet on first start.  All error output is suppressed so the caller can
# distinguish between "not found" (empty stdout) and "found" (value).
read_app_setting() {
  _key="$1"
  if ! resolve_direct_connection; then
    printf ''
    return 0
  fi
  PGPASSWORD="$DIRECT_PASSWORD" \
    psql \
      --host "$DIRECT_HOST" \
      --port "$DIRECT_PORT" \
      --username "$DIRECT_USER" \
      --dbname "$DIRECT_DB" \
      --no-psqlrc \
      --tuples-only \
      --no-align \
      -c "SELECT value FROM app_settings WHERE key = '$_key'" \
      2>/dev/null || true
}

# Upsert a single key in app_settings via psql.  Errors are suppressed so a
# transient DB hiccup does not abort the daemon (the caller handles the
# consequence, e.g. treating the backup as still overdue).
write_app_setting() {
  _key="$1"
  _value="$2"
  if ! resolve_direct_connection; then
    return 0
  fi
  PGPASSWORD="$DIRECT_PASSWORD" \
    psql \
      --host "$DIRECT_HOST" \
      --port "$DIRECT_PORT" \
      --username "$DIRECT_USER" \
      --dbname "$DIRECT_DB" \
      --no-psqlrc \
      -c "INSERT INTO app_settings (key, value)
          VALUES ('$_key', '$_value')
          ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value" \
      2>/dev/null || true
}

# Write the current UTC timestamp to backup_last_success_at so the next cycle
# can calculate the next due time independently of when the container started.
write_last_success_at() {
  _ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  write_app_setting "backup_last_success_at" "$_ts"
}

# Resolve backup interval in days from app_settings; fall back to 1 day when the
# setting is unavailable (bootstrap race / DB not migrated yet).
resolve_interval_days() {
  _raw="$(read_app_setting "backup_interval_days")"
  _raw="$(printf '%s' "$_raw" | tr -d '[:space:]')"
  case "$_raw" in
    ''|*[!0-9]*)
      printf '1'
      ;;
    *)
      if [ "$_raw" -le 0 ] 2>/dev/null; then
        printf '1'
      else
        printf '%s' "$_raw"
      fi
      ;;
  esac
}

# Return true (exit 0) when enough time has elapsed since the last successful
# backup to warrant a new one.  An empty or unparseable last_ts is treated as
# "overdue" so the backup runs immediately on a fresh install or after data loss.
is_backup_due() {
  _last="$1"
  _interval_days="$2"
  if [ -z "$_last" ]; then
    return 0
  fi
  _last_epoch="$(date -d "$_last" +%s 2>/dev/null || echo 0)"
  _now_epoch="$(date -u +%s)"
  _interval_secs="$(( _interval_days * 86400 ))"
  [ "$(( _now_epoch - _last_epoch ))" -ge "$_interval_secs" ]
}

# Return the number of seconds from now until the next backup is due.
# Returns 0 when the backup is already overdue or when last_ts is empty/invalid.
seconds_until_next_backup() {
  _last="$1"
  _interval_days="$2"
  if [ -z "$_last" ]; then
    printf '0'
    return
  fi
  _last_epoch="$(date -d "$_last" +%s 2>/dev/null || echo 0)"
  _interval_secs="$(( _interval_days * 86400 ))"
  _next_epoch="$(( _last_epoch + _interval_secs ))"
  _now_epoch="$(date -u +%s)"
  _remaining="$(( _next_epoch - _now_epoch ))"
  if [ "$_remaining" -le 0 ]; then
    printf '0'
  else
    printf '%d' "$_remaining"
  fi
}

# -- Nextcloud upload helpers --------------------------------------------------

# Parse a Nextcloud share URL into UPLOAD_BASE and UPLOAD_TOKEN.
# Accepts only https:// URLs.  Returns 1 on invalid input.
parse_share_url() {
  _url="$1"
  case "$_url" in
    https://*) ;;
    *)
      printf 'backup upload: share URL must start with https://\n' >&2
      return 1
      ;;
  esac
  case "$_url" in
    */s/*)
      # Everything before /s/ is the base (schema + host + optional subpath).
      UPLOAD_BASE="${_url%%/s/*}"
      # First path segment after /s/ is the token.
      _after="${_url#*/s/}"
      UPLOAD_TOKEN="${_after%%/*}"
      ;;
    *)
      printf 'backup upload: share URL must contain /s/<token>\n' >&2
      return 1
      ;;
  esac
  if [ -z "$UPLOAD_TOKEN" ]; then
    printf 'backup upload: empty token in share URL\n' >&2
    return 1
  fi
}

# Build the WebDAV target URL.
build_upload_target() {
  _base="$1"
  _token="$2"
  _filename="$3"
  printf '%s/public.php/webdav/%s' "$_base" "$_filename"
}

# Upload a backup file to Nextcloud via WebDAV PUT.
# Credentials are fed to curl via --config stdin so they never appear in ps output.
upload_backup() {
  _file="$1"
  _base="$2"
  _token="$3"
  _password="$4"
  _filename="$(basename "$_file")"
  _target="$(build_upload_target "$_base" "$_token" "$_filename")"

  curl \
    --config - \
    --silent \
    --show-error \
    --fail \
    --retry 2 \
    --retry-delay 5 \
    --upload-file "$_file" \
    <<EOF
url = "$_target"
user = "$_token:$_password"
header = "Content-Type: application/octet-stream"
EOF
}

# -- Admin notifications -------------------------------------------------------

# Upsert a pinned system-error notification for all active admins directly in
# the database.  Designed to mirror the Rust services::notifications::
# notify_admins_system_error logic but callable from shell.
#
# _dedup_key  e.g. "backup_failed" or "backup_upload_failed" -- must match the
#             dedupe_key used in the Rust system-alerts email throttle so the
#             hourly background task picks up the right key.
# _message    Short human-readable description (no single quotes).
#
# Behaviour:
#   - Not exists -> INSERT (unread, pinned).
#   - Exists and is_read=FALSE -> DO NOTHING (already alerting, no duplicate).
#   - Exists and is_read=TRUE  -> UPDATE: mark unread again (re-alert).
notify_admins_backup_error() {
  _dedup_key="$1"
  _message="$2"
  if ! resolve_direct_connection; then
    return 0
  fi
  PGPASSWORD="$DIRECT_PASSWORD" \
    psql \
      --host "$DIRECT_HOST" \
      --port "$DIRECT_PORT" \
      --username "$DIRECT_USER" \
      --dbname "$DIRECT_DB" \
      --no-psqlrc \
      -c "INSERT INTO notifications
            (user_id, kind, title, body, dedupe_key, pinned, is_read, created_at)
          SELECT id, 'system_error', '$_message', NULL,
                 '$_dedup_key', TRUE, FALSE, NOW()
          FROM users WHERE role = 'admin' AND active = TRUE
          ON CONFLICT (user_id, kind, dedupe_key)
          WHERE dedupe_key IS NOT NULL
          DO UPDATE SET
            title      = EXCLUDED.title,
            pinned     = TRUE,
            is_read    = FALSE,
            created_at = NOW()
          WHERE notifications.is_read = TRUE" \
      2>/dev/null || true
}

# -- Validation ----------------------------------------------------------------

validate_encryption_key() {
  if [ -z "$ENCRYPTION_KEY" ]; then
    printf 'ZERF_DB_ENCRYPTION_KEY must be set in .env (generate with: openssl rand -hex 32).\n' >&2
    return 1
  fi
}

# -- pg_dump -------------------------------------------------------------------

run_direct_pg_dump() {
  command -v pg_dump >/dev/null 2>&1 || return 1
  resolve_direct_connection || return 1

  # statement_timeout=30000 and idle_in_transaction_session_timeout=30000 are set
  # server-wide in docker-compose to protect the application, but they also apply
  # to pg_dump.  statement_timeout would cancel long COPY statements;
  # idle_in_transaction_session_timeout would kill pg_dump's snapshot transaction
  # if it ever blocks for >30s while writing its output (e.g. a slow downstream
  # consumer in the streamed `pg_dump | openssl` pipeline).  Disable both for this
  # session only via PGOPTIONS.
  #
  # --lock-wait-timeout=30s: fail fast when another session holds an exclusive
  # lock, rather than hanging forever.
  PGPASSWORD="$DIRECT_PASSWORD" \
  PGOPTIONS='--statement_timeout=0 --idle_in_transaction_session_timeout=0' \
    pg_dump \
      --host "$DIRECT_HOST" \
      --port "$DIRECT_PORT" \
      --username "$DIRECT_USER" \
      --dbname "$DIRECT_DB" \
      --format=custom \
      --no-owner \
      --no-privileges \
      --lock-wait-timeout=30s
}

# -- Metadata ------------------------------------------------------------------

metadata_value() {
  if [ -z "${1:-}" ]; then
    printf 'unknown'
    return 0
  fi
  printf '%s' "$1" | tr '\n\r' '  '
}

write_backup_metadata() {
  _target_file="$1"
  _created_at="$2"
  _keyring_included="${3:-false}"

  resolve_direct_connection || return 1

  {
    printf 'backup_format=pg_dump_custom\n'
    printf 'created_at_utc=%s\n' "$(metadata_value "$_created_at")"
    printf 'ZERF_GIT_COMMIT=%s\n' "$(metadata_value "${ZERF_GIT_COMMIT:-unknown}")"
    printf 'PGHOST=%s\n' "$(metadata_value "$DIRECT_HOST")"
    printf 'PGPORT=%s\n' "$(metadata_value "$DIRECT_PORT")"
    printf 'PGDATABASE=%s\n' "$(metadata_value "$DIRECT_DB")"
    printf 'PGUSER=%s\n' "$(metadata_value "$DIRECT_USER")"
    # Records whether a sibling zerf-<ts>.keyring.enc was captured for this
    # backup. The logical dump restores without it; it only matters for
    # physical recovery of an orphaned, encrypted PGDATA volume.
    printf 'pg_tde_keyring_included=%s\n' "$(metadata_value "$_keyring_included")"
  } > "$_target_file"
}

# -- Retention -----------------------------------------------------------------

# Keep only the 10 most recent encrypted backup files; delete all older ones
# along with their associated .metadata and .keyring.enc sidecar files.
# Local retention is count-based (not time-based) so the volume stays bounded
# regardless of backup frequency changes.
apply_retention() {
  ls -1t "$OUT_DIR"/zerf-*.dump.enc 2>/dev/null | tail -n +11 | while IFS= read -r f; do
    rm -f "$f" "${f%.dump.enc}.metadata" "${f%.dump.enc}.keyring.enc"
  done
}

# -- Core backup ---------------------------------------------------------------

run_backup_once() {
  # Sweep stale temp files from previous runs interrupted by SIGTERM/SIGKILL.
  find "$OUT_DIR" -maxdepth 1 -type f \
    \( -name 'zerf-*.dump.enc.tmp' -o -name 'zerf-*.dump.plain.tmp' \
       -o -name 'zerf-*.metadata.tmp' -o -name 'zerf-*.keyring.enc.tmp' \) \
    -exec rm -f {} +

  ts="$(date -u +%Y%m%dT%H%M%SZ)"
  output_file="$OUT_DIR/zerf-$ts.dump.enc"
  metadata_file="$OUT_DIR/zerf-$ts.metadata"
  keyring_file="$OUT_DIR/zerf-$ts.keyring.enc"
  temp_file="$output_file.tmp"
  temp_metadata_file="$metadata_file.tmp"
  temp_keyring_file="$keyring_file.tmp"

  # Dump and encrypt in a single stream: pg_dump's custom-format output is piped
  # straight into openssl, which writes the ciphertext to $temp_file on the
  # (disk-backed) backup volume.  No plaintext dump is ever staged -- not on disk
  # and not in a RAM tmpfs -- so there is NO size ceiling tied to /tmp; the only
  # bound is free space on the backups volume.  This removes the previous failure
  # mode where a growing database eventually exceeded the 64 MiB /tmp tmpfs and
  # every backup silently failed.
  #
  # AES-256-CBC with a PBKDF2-derived key (100000 iterations); passphrase read
  # from env, never in process args.  These openssl parameters are byte-for-byte
  # identical to the previous implementation, so existing .dump.enc files (and the
  # pg_tde keyring sidecars) remain decryptable with the same ZERF_DB_ENCRYPTION_KEY.
  #
  # /bin/sh has no `pipefail`, so capture pg_dump's exit status across the pipe
  # with the POSIX fd trick: fd 4 is the script's real stdout; inside the command
  # substitution pg_dump's status is echoed to fd 3 (the substitution's stdout,
  # captured into $dump_status) while openssl's own stdout is routed to fd 4.
  # `$?` after the assignment is the pipeline's status, i.e. openssl's.
  exec 4>&1
  dump_status="$( { { run_direct_pg_dump; echo "$?" >&3; } \
      | openssl enc -aes-256-cbc -salt -pbkdf2 -iter 100000 \
          -pass env:ZERF_DB_ENCRYPTION_KEY \
          -out "$temp_file"; } 3>&1 >&4 )"
  enc_status=$?
  exec 4>&-

  # Do NOT rely on `set -e` here: it is suspended throughout run_backup_once
  # because main() invokes the function in an `if` condition.  Check both statuses
  # explicitly.  Note: if pg_dump fails or is interrupted, $dump_status is either
  # its non-zero code or empty (when an early exit skips the `echo`); both are
  # caught by the `!= 0` test, so a partial/failed dump never becomes a backup.
  if [ "$dump_status" != 0 ]; then
    rm -f "$temp_file"
    printf 'pg_dump failed (status %s) -- connection settings incomplete, pg_dump unavailable, or the dump was interrupted.\n' "${dump_status:-unknown}" >&2
    return 1
  fi
  if [ "$enc_status" != 0 ]; then
    rm -f "$temp_file"
    printf 'Failed to encrypt backup (openssl status %s).\n' "$enc_status" >&2
    return 1
  fi

  # Reject a zero-byte encrypted file.  pg_dump in custom format always emits a
  # header, so it should never exit 0 with empty output; guard against it anyway
  # so monitoring catches a broken state rather than silently advancing the backup
  # timestamp.  (The old code checked the plaintext size; with streaming we check
  # the ciphertext, the only artifact that exists.)
  if [ ! -s "$temp_file" ]; then
    rm -f "$temp_file"
    printf 'pg_dump produced empty output -- refusing to encrypt a zero-byte backup.\n' >&2
    return 1
  fi

  # Stage a copy of the pg_tde keyring next to the dump.  Best-effort: a missing
  # or unreadable keyring (volume not mounted, older deployment) is a warning,
  # never a backup failure, because the logical dump restores without it.  The
  # keyring is already AES-encrypted, so it is copied verbatim.
  keyring_included=false
  if [ -f "$KEYRING_SRC" ]; then
    if cp "$KEYRING_SRC" "$temp_keyring_file" 2>/dev/null; then
      chmod 600 "$temp_keyring_file"
      keyring_included=true
    else
      rm -f "$temp_keyring_file"
      printf 'WARNING: failed to copy pg_tde keyring from %s -- backup will not include it.\n' "$KEYRING_SRC" >&2
    fi
  else
    printf 'WARNING: pg_tde keyring not found at %s -- backup will not include it (logical restore is unaffected).\n' "$KEYRING_SRC" >&2
  fi

  if ! write_backup_metadata "$temp_metadata_file" "$ts" "$keyring_included"; then
    rm -f "$temp_file" "$temp_metadata_file" "$temp_keyring_file"
    printf 'Failed to write backup metadata.\n' >&2
    return 1
  fi

  chmod 600 "$temp_file" "$temp_metadata_file"
  if ! mv "$temp_file" "$output_file"; then
    rm -f "$temp_file" "$temp_metadata_file" "$temp_keyring_file"
    printf 'Failed to finalize backup file.\n' >&2
    return 1
  fi
  if ! mv "$temp_metadata_file" "$metadata_file"; then
    rm -f "$output_file" "$temp_metadata_file" "$temp_keyring_file"
    printf 'Failed to finalize backup metadata.\n' >&2
    return 1
  fi
  # Finalize the keyring sidecar last.  A failure here is non-fatal: the dump
  # and metadata are already valid, so downgrade to a warning rather than
  # discarding a good backup.
  if [ "$keyring_included" = "true" ]; then
    if ! mv "$temp_keyring_file" "$keyring_file"; then
      rm -f "$temp_keyring_file"
      printf 'WARNING: failed to finalize keyring sidecar -- backup kept without it.\n' >&2
      keyring_included=false
    fi
  fi

  apply_retention
  printf 'Backup written: %s\n' "$output_file"
  printf 'Backup metadata written: %s\n' "$metadata_file"
  if [ "$keyring_included" = "true" ]; then
    printf 'Backup keyring written: %s\n' "$keyring_file"
  fi

  # Step 3 (optional): upload to Nextcloud via WebDAV.
  _upload_enabled="$(read_app_setting "backup_upload_enabled")"
  _upload_enabled="$(printf '%s' "$_upload_enabled" | tr -d '[:space:]')"
  if [ "$_upload_enabled" = "true" ]; then
    _upload_url="$(read_app_setting "backup_upload_url")"
    _upload_url="$(printf '%s' "$_upload_url" | tr -d '[:space:]')"
    _upload_pw="$(read_app_setting "backup_upload_password")"

    if [ -n "$_upload_url" ]; then
      if parse_share_url "$_upload_url"; then
        if upload_backup "$output_file" "$UPLOAD_BASE" "$UPLOAD_TOKEN" "$_upload_pw"; then
          printf 'Backup uploaded: %s\n' "$(basename "$output_file")"
        else
          # Upload failure is non-fatal: local backup is valid.
          printf 'WARNING: Nextcloud upload failed for %s -- local backup retained.\n' \
            "$(basename "$output_file")" >&2
          notify_admins_backup_error "backup_upload_failed" \
            "Nextcloud backup upload failed. Check backup container logs."
        fi
        # Also upload the keyring sidecar so off-site recovery is possible.
        # Secondary to the dump: a failure here is a warning only and does not
        # raise the admin alert (the logical dump is what restores the data).
        if [ "$keyring_included" = "true" ]; then
          if upload_backup "$keyring_file" "$UPLOAD_BASE" "$UPLOAD_TOKEN" "$_upload_pw"; then
            printf 'Backup keyring uploaded: %s\n' "$(basename "$keyring_file")"
          else
            printf 'WARNING: Nextcloud upload failed for %s -- local copy retained.\n' \
              "$(basename "$keyring_file")" >&2
          fi
        fi
      else
        printf 'WARNING: Invalid backup_upload_url in app_settings -- skipping upload.\n' >&2
      fi
    fi
  fi
}

# -- Entry point ---------------------------------------------------------------

main() {
  mkdir -p "$OUT_DIR"
  chmod 700 "$OUT_DIR"

  validate_encryption_key || exit 1

  # Daemon mode: loop forever.  The backup interval is calculated from the
  # last successful backup timestamp stored in app_settings so the schedule
  # survives container restarts without running an unnecessary backup.
  while :; do
    INTERVAL_DAYS="$(resolve_interval_days)"
    LAST="$(read_app_setting "backup_last_success_at" | tr -d '[:space:]')"

    if is_backup_due "$LAST" "$INTERVAL_DAYS"; then
      if run_backup_once; then
        # Write the success timestamp so the next cycle starts the interval
        # from now, not from container start.
        write_last_success_at
      else
        notify_admins_backup_error "backup_failed" \
          "Database backup failed. Check backup container logs."
        # On failure retry in 1 hour rather than waiting the full interval.
        sleep 3600
        continue
      fi
    fi

    # Re-read in case interval changed during the backup run.
    INTERVAL_DAYS="$(resolve_interval_days)"
    LAST="$(read_app_setting "backup_last_success_at" | tr -d '[:space:]')"
    SLEEP_SECS="$(seconds_until_next_backup "$LAST" "$INTERVAL_DAYS")"
    # Guard against a tight loop if write_last_success_at failed silently or
    # the timestamp is unparseable (both produce SLEEP_SECS=0).
    if [ "$SLEEP_SECS" -le 0 ]; then
      SLEEP_SECS=3600
    fi
    sleep "$SLEEP_SECS"
  done
}

# Allow sourcing this file without running main (used by bats unit tests).
[ -n "${BACKUP_LIB_ONLY:-}" ] || main "$@"

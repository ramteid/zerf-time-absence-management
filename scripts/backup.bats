#!/usr/bin/env bats
# Unit tests for scripts/backup.sh helper functions.
#
# Run with:  bats scripts/backup.bats
# Requires:  bats-core  (https://github.com/bats-core/bats-core)
#
# The tests source backup.sh with BACKUP_LIB_ONLY=1 so the daemon loop
# never starts.  External commands (psql, curl, openssl, pg_dump) are
# replaced by lightweight PATH shims defined in setup().

setup() {
  # Create a temp directory for shims and test files.
  export BATS_TMPDIR="${BATS_TEST_TMPDIR:-${TMPDIR:-/tmp}}/bats_$$"
  mkdir -p "$BATS_TMPDIR/bin" "$BATS_TMPDIR/out"

  # Prepend the shim directory to PATH so our fakes override real commands.
  export PATH="$BATS_TMPDIR/bin:$PATH"

  # Minimal env that validate_encryption_key and resolve_direct_connection need.
  export ZERF_DB_ENCRYPTION_KEY="test-key-for-bats"
  export PGHOST="db"
  export PGPORT="5432"
  export PGDATABASE="zerf"
  export PGUSER="zerf"
  export PGPASSWORD="secret"

  # Source only the library functions; do not run main.
  export BACKUP_LIB_ONLY=1
  # shellcheck source=/dev/null
  . "$(dirname "$BATS_TEST_FILENAME")/../scripts/backup.sh"
}

teardown() {
  rm -rf "$BATS_TMPDIR"
}

# Helper: write a shim script.
make_shim() {
  _name="$1"
  _body="$2"
  printf '#!/bin/sh\n%s\n' "$_body" > "$BATS_TMPDIR/bin/$_name"
  chmod +x "$BATS_TMPDIR/bin/$_name"
}

# Shim openssl so `enc ... -in X -out Y` simply copies X to Y, letting
# run_backup_once succeed end-to-end without real cryptography.
make_openssl_copy_shim() {
  make_shim openssl '
in=""; out=""
while [ $# -gt 0 ]; do
  case "$1" in
    -in) in="$2"; shift 2 ;;
    -out) out="$2"; shift 2 ;;
    *) shift ;;
  esac
done
[ -n "$out" ] && cp "$in" "$out"
'
}

# ── parse_share_url ──────────────────────────────────────────────────────────

@test "parse_share_url: valid URL extracts base and token" {
  # Call without `run` so variable assignments are visible in the current shell.
  parse_share_url "https://cloud.example.com/s/AbCdEfGhIj"
  [ "$UPLOAD_BASE" = "https://cloud.example.com" ]
  [ "$UPLOAD_TOKEN" = "AbCdEfGhIj" ]
}

@test "parse_share_url: sub-path Nextcloud preserves base subpath" {
  parse_share_url "https://example.com/nextcloud/s/MyToken"
  [ "$UPLOAD_BASE" = "https://example.com/nextcloud" ]
  [ "$UPLOAD_TOKEN" = "MyToken" ]
}

@test "parse_share_url: rejects http:// URL" {
  run parse_share_url "http://cloud.example.com/s/Token"
  [ "$status" -ne 0 ]
}

@test "parse_share_url: rejects URL without /s/ segment" {
  run parse_share_url "https://cloud.example.com/share/Token"
  [ "$status" -ne 0 ]
}

@test "parse_share_url: rejects empty token after /s/" {
  run parse_share_url "https://cloud.example.com/s/"
  [ "$status" -ne 0 ]
}

# ── resolve_interval_days ────────────────────────────────────────────────────

@test "resolve_interval_days: returns value from app_settings when valid" {
  make_shim psql 'printf "7\n"'
  run resolve_interval_days
  [ "$status" -eq 0 ]
  [ "$output" = "7" ]
}

@test "resolve_interval_days: falls back to 1 when psql returns empty" {
  make_shim psql 'printf ""'
  run resolve_interval_days
  [ "$status" -eq 0 ]
  [ "$output" = "1" ]
}

@test "resolve_interval_days: falls back to 1 when psql fails" {
  make_shim psql 'exit 1'
  run resolve_interval_days
  [ "$status" -eq 0 ]
  [ "$output" = "1" ]
}

@test "resolve_interval_days: falls back to 1 when value is zero" {
  make_shim psql 'printf "0\n"'
  run resolve_interval_days
  [ "$status" -eq 0 ]
  [ "$output" = "1" ]
}

@test "resolve_interval_days: falls back to 1 when value is non-integer" {
  make_shim psql 'printf "abc\n"'
  run resolve_interval_days
  [ "$status" -eq 0 ]
  [ "$output" = "1" ]
}

# ── is_backup_due ────────────────────────────────────────────────────────────

@test "is_backup_due: returns true when last_ts is empty" {
  # Empty timestamp -> treat as overdue; exit 0 means true in shell.
  run is_backup_due "" 1
  [ "$status" -eq 0 ]
}

@test "is_backup_due: returns true when interval has fully elapsed" {
  # Use epoch 0 (1970-01-01) as last timestamp; far more than 1 day has passed.
  run is_backup_due "1970-01-01T00:00:00Z" 1
  [ "$status" -eq 0 ]
}

@test "is_backup_due: returns false when interval has not yet elapsed" {
  # Use the current time as last timestamp; with a 1-day interval it is not due.
  _now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  run is_backup_due "$_now" 1
  [ "$status" -ne 0 ]
}

@test "is_backup_due: returns true when unparseable timestamp is given" {
  # An invalid timestamp falls back to epoch 0, making it appear overdue.
  run is_backup_due "not-a-date" 1
  [ "$status" -eq 0 ]
}

# ── seconds_until_next_backup ────────────────────────────────────────────────

@test "seconds_until_next_backup: returns 0 when last_ts is empty" {
  run seconds_until_next_backup "" 1
  [ "$status" -eq 0 ]
  [ "$output" = "0" ]
}

@test "seconds_until_next_backup: returns 0 for an overdue timestamp" {
  run seconds_until_next_backup "1970-01-01T00:00:00Z" 1
  [ "$status" -eq 0 ]
  [ "$output" = "0" ]
}

@test "seconds_until_next_backup: returns positive value for a recent timestamp" {
  _now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  run seconds_until_next_backup "$_now" 1
  [ "$status" -eq 0 ]
  [ "$output" -gt 0 ]
  [ "$output" -le 86400 ]
}

# ── build_upload_target ──────────────────────────────────────────────────────

@test "build_upload_target: constructs WebDAV URL" {
  run build_upload_target "https://cloud.example.com" "AbCdEf" "zerf-20260101.dump.enc"
  [ "$status" -eq 0 ]
  [ "$output" = "https://cloud.example.com/public.php/webdav/zerf-20260101.dump.enc" ]
}

@test "build_upload_target: works with subpath base" {
  run build_upload_target "https://example.com/nextcloud" "TokXyz" "backup.dump.enc"
  [ "$status" -eq 0 ]
  [ "$output" = "https://example.com/nextcloud/public.php/webdav/backup.dump.enc" ]
}

# ── upload_backup ────────────────────────────────────────────────────────────

@test "upload_backup: passes credentials via curl config stdin not CLI args" {
  # Shim curl that records its config stdin and arguments.
  mkdir -p "$BATS_TMPDIR/curl_capture"
  make_shim curl '
config_file="$BATS_TMPDIR/curl_capture/config"
stdin_file="$BATS_TMPDIR/curl_capture/stdin"
printf "%s\n" "$*" > "$config_file"
cat > "$stdin_file"
exit 0
'
  # Create a small dummy file to upload.
  printf "dummy content" > "$BATS_TMPDIR/dummy.dump.enc"

  upload_backup "$BATS_TMPDIR/dummy.dump.enc" \
    "https://cloud.example.com" "MyToken" "mypassword"

  # Verify the token and password appear in stdin config, NOT in the CLI args.
  grep -q "user = \"MyToken:mypassword\"" "$BATS_TMPDIR/curl_capture/stdin"
  # Ensure password is NOT in the CLI argument string.
  ! grep -q "mypassword" "$BATS_TMPDIR/curl_capture/config"
}

# ── run_backup_once: 0-byte dump rejection ───────────────────────────────────

@test "run_backup_once: refuses to encrypt a zero-byte dump" {
  # pg_dump shim exits 0 but produces no output.
  make_shim pg_dump 'exit 0'
  make_shim psql 'printf ""'
  make_shim openssl 'exit 0'

  export OUT_DIR="$BATS_TMPDIR/out"
  run run_backup_once
  [ "$status" -ne 0 ]
  [[ "$output" =~ "zero-byte" ]]
}

# ── run_backup_once: pg_tde keyring sidecar ──────────────────────────────────

@test "run_backup_once: captures the pg_tde keyring sidecar when present" {
  make_shim pg_dump 'printf "PGDMP-fake-dump-bytes"'
  make_shim psql 'printf ""'
  make_openssl_copy_shim

  export OUT_DIR="$BATS_TMPDIR/out"
  # Provide a fake (already-encrypted) keyring source file.
  printf 'fake-encrypted-keyring' > "$BATS_TMPDIR/keyring.enc"
  export KEYRING_SRC="$BATS_TMPDIR/keyring.enc"

  run run_backup_once
  [ "$status" -eq 0 ]

  # Exactly one keyring sidecar exists and is a verbatim copy of the source.
  kr="$(ls "$OUT_DIR"/zerf-*.keyring.enc)"
  [ -f "$kr" ]
  [ "$(cat "$kr")" = "fake-encrypted-keyring" ]
  # Metadata records that the keyring was included.
  grep -q '^pg_tde_keyring_included=true$' "$OUT_DIR"/zerf-*.metadata
}

@test "run_backup_once: succeeds without a keyring when the source is absent" {
  make_shim pg_dump 'printf "PGDMP-fake-dump-bytes"'
  make_shim psql 'printf ""'
  make_openssl_copy_shim

  export OUT_DIR="$BATS_TMPDIR/out"
  export KEYRING_SRC="$BATS_TMPDIR/does-not-exist.enc"

  run run_backup_once
  # A missing keyring is a warning, never a failure.
  [ "$status" -eq 0 ]
  # No keyring sidecar is produced...
  ! ls "$OUT_DIR"/zerf-*.keyring.enc 2>/dev/null
  # ...and the metadata records its absence.
  grep -q '^pg_tde_keyring_included=false$' "$OUT_DIR"/zerf-*.metadata
}

# ── apply_retention (count-based: keep last 10) ──────────────────────────────

@test "apply_retention: deletes oldest files when more than 10 exist" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  # Create 12 backup files with staggered mtimes so ls -t sorts them reliably.
  for i in $(seq 1 12); do
    f="$OUT_DIR/zerf-$(printf '%012d' "$i").dump.enc"
    printf 'backup' > "$f"
    touch -d "$i seconds ago" "$f"
  done

  apply_retention

  count="$(ls "$OUT_DIR"/*.dump.enc 2>/dev/null | wc -l | tr -d '[:space:]')"
  [ "$count" -eq 10 ]
}

@test "apply_retention: keeps fewer than 10 files untouched" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  for i in $(seq 1 3); do
    printf 'backup' > "$OUT_DIR/zerf-$(printf '%012d' "$i").dump.enc"
  done

  apply_retention

  count="$(ls "$OUT_DIR"/*.dump.enc 2>/dev/null | wc -l | tr -d '[:space:]')"
  [ "$count" -eq 3 ]
}

@test "apply_retention: also removes associated metadata files for deleted backups" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  for i in $(seq 1 12); do
    f="$OUT_DIR/zerf-$(printf '%012d' "$i").dump.enc"
    m="${f%.dump.enc}.metadata"
    printf 'backup' > "$f"
    printf 'meta'   > "$m"
    touch -d "$i seconds ago" "$f"
  done

  apply_retention

  meta_count="$(ls "$OUT_DIR"/*.metadata 2>/dev/null | wc -l | tr -d '[:space:]')"
  [ "$meta_count" -eq 10 ]
}

@test "apply_retention: also removes keyring sidecars for deleted backups" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  for i in $(seq 1 12); do
    f="$OUT_DIR/zerf-$(printf '%012d' "$i").dump.enc"
    printf 'backup' > "$f"
    printf 'keyr'   > "${f%.dump.enc}.keyring.enc"
    touch -d "$i seconds ago" "$f"
  done

  apply_retention

  kr_count="$(ls "$OUT_DIR"/*.keyring.enc 2>/dev/null | wc -l | tr -d '[:space:]')"
  [ "$kr_count" -eq 10 ]
}

@test "apply_retention: no-ops when backup directory is empty" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  # Should not fail on empty directory.
  run apply_retention
  [ "$status" -eq 0 ]
}

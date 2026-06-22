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

# ── resolve_interval ─────────────────────────────────────────────────────────

@test "resolve_interval: returns value from app_settings when valid" {
  # Shim psql to return a known interval.
  make_shim psql 'printf "3600\n"'
  run resolve_interval
  [ "$status" -eq 0 ]
  [ "$output" = "3600" ]
}

@test "resolve_interval: falls back to 86400 when psql returns empty" {
  make_shim psql 'printf ""'
  run resolve_interval
  [ "$status" -eq 0 ]
  [ "$output" = "86400" ]
}

@test "resolve_interval: falls back to 86400 when psql fails" {
  make_shim psql 'exit 1'
  run resolve_interval
  [ "$status" -eq 0 ]
  [ "$output" = "86400" ]
}

@test "resolve_interval: falls back to 86400 when value is zero" {
  make_shim psql 'printf "0\n"'
  run resolve_interval
  [ "$status" -eq 0 ]
  [ "$output" = "86400" ]
}

@test "resolve_interval: falls back to 86400 when value is non-integer" {
  make_shim psql 'printf "abc\n"'
  run resolve_interval
  [ "$status" -eq 0 ]
  [ "$output" = "86400" ]
}

# ── resolve_retention ────────────────────────────────────────────────────────

@test "resolve_retention: returns value from app_settings when valid" {
  make_shim psql 'printf "14\n"'
  run resolve_retention
  [ "$status" -eq 0 ]
  [ "$output" = "14" ]
}

@test "resolve_retention: falls back to 30 when psql returns empty" {
  make_shim psql 'printf ""'
  run resolve_retention
  [ "$status" -eq 0 ]
  [ "$output" = "30" ]
}

@test "resolve_retention: falls back to 30 when psql fails" {
  make_shim psql 'exit 1'
  run resolve_retention
  [ "$status" -eq 0 ]
  [ "$output" = "30" ]
}

@test "resolve_retention: falls back to 30 when value is zero" {
  make_shim psql 'printf "0\n"'
  run resolve_retention
  [ "$status" -eq 0 ]
  [ "$output" = "30" ]
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

# ── apply_retention ──────────────────────────────────────────────────────────

@test "apply_retention: removes files older than the retention period" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  # Create an old .dump.enc file.
  old_file="$OUT_DIR/zerf-old.dump.enc"
  printf "old backup" > "$old_file"
  # Force the file timestamp to be 31 days old.
  touch -d "31 days ago" "$old_file" 2>/dev/null || \
    touch -t "$(date -d '31 days ago' +%Y%m%d%H%M 2>/dev/null || date -v-31d +%Y%m%d%H%M)" \
      "$old_file" 2>/dev/null || true

  apply_retention "30"
  [ ! -f "$old_file" ]
}

@test "apply_retention: keeps files newer than the retention period" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  new_file="$OUT_DIR/zerf-new.dump.enc"
  printf "new backup" > "$new_file"
  # File was just created -- definitely newer than 30 days.

  apply_retention "30"
  [ -f "$new_file" ]
}

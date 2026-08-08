#!/usr/bin/env bats
# Unit tests for scripts/backup.sh helper functions.
#
# Run with:  bats scripts/backup.bats
# Requires:  bats-core  (https://github.com/bats-core/bats-core)
#            zip / unzip  (apt-get install zip unzip)
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
  hash -r 2>/dev/null || true
}

# Shim openssl so `enc ... -out Y` copies its STDIN to Y, letting run_backup_once
# succeed end-to-end without real cryptography.  backup.sh streams the dump
# into openssl via a pipe (`pg_dump | openssl ... -out file`), so the shim reads
# stdin and writes it to the -out path, mirroring real openssl.
#
# IMPORTANT: the shim always prepends a fake 32-byte "Salted__" header before
# copying stdin, mirroring what real openssl enc ALWAYS emits (8-byte magic +
# 8-byte salt + 16-byte padded first block = 32 bytes minimum) even for empty
# plaintext input.  A shim that only copies stdin (no header) would exit 0 and
# produce a zero-byte file from an empty pg_dump, making the old `[ ! -s ]`
# zero-byte guard appear to work when it actually fires against an artefact that
# real openssl would never produce.  The new 512-byte floor correctly rejects
# the 32-byte fake ciphertext; tests that feed real content (PGDMP + padding to
# >512 bytes) will produce files large enough to pass the floor check.
make_openssl_copy_shim() {
  make_shim openssl '
out=""
while [ $# -gt 0 ]; do
  case "$1" in
    -out) out="$2"; shift 2 ;;
    *) shift ;;
  esac
done
# Always write a 32-byte fake header (Salted__ + 24 filler bytes) before the
# plaintext content, matching real openssl enc output structure.
if [ -n "$out" ]; then
  { printf "Salted__"; head -c 24 /dev/zero; cat; } > "$out"
else
  { printf "Salted__"; head -c 24 /dev/zero; cat; }
fi
'
}

# -- parse_share_url ----------------------------------------------------------

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

# -- resolve_interval_days ----------------------------------------------------

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

# -- is_backup_due ------------------------------------------------------------

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

# -- seconds_until_next_backup ------------------------------------------------

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

# -- build_upload_target ------------------------------------------------------

@test "build_upload_target: constructs WebDAV URL" {
  run build_upload_target "https://cloud.example.com" "AbCdEf" "zerf-20260101T000000Z.zip"
  [ "$status" -eq 0 ]
  [ "$output" = "https://cloud.example.com/public.php/webdav/zerf-20260101T000000Z.zip" ]
}

@test "build_upload_target: works with subpath base" {
  run build_upload_target "https://example.com/nextcloud" "TokXyz" "zerf-20260101T000000Z.zip"
  [ "$status" -eq 0 ]
  [ "$output" = "https://example.com/nextcloud/public.php/webdav/zerf-20260101T000000Z.zip" ]
}

# -- upload_backup ------------------------------------------------------------

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
  # Create a small dummy archive to upload.
  printf "dummy content" > "$BATS_TMPDIR/dummy.zip"

  upload_backup "$BATS_TMPDIR/dummy.zip" \
    "https://cloud.example.com" "MyToken" "mypassword"

  # Verify the token and password appear in stdin config, NOT in the CLI args.
  grep -q "user = \"MyToken:mypassword\"" "$BATS_TMPDIR/curl_capture/stdin"
  # Ensure password is NOT in the CLI argument string.
  ! grep -q "mypassword" "$BATS_TMPDIR/curl_capture/config"
}

# -- run_backup_once: small ciphertext rejection (512-byte floor) -------------

@test "run_backup_once: refuses to record a suspiciously small encrypted dump" {
  # pg_dump shim exits 0 but produces no output.
  # The faithful openssl shim always prepends a 32-byte header regardless of
  # plaintext size, so an empty pg_dump yields a 32-byte ciphertext file --
  # well below the 512-byte floor that catches empty/broken dumps.
  make_shim pg_dump 'exit 0'
  make_shim psql 'printf ""'
  make_openssl_copy_shim

  export OUT_DIR="$BATS_TMPDIR/out"
  run run_backup_once
  [ "$status" -ne 0 ]
  [[ "$output" =~ "suspiciously small" ]]
}

# -- run_backup_once: zip archive creation ------------------------------------

@test "run_backup_once: produces a single zip archive containing dump.enc and metadata" {
  # Use >512 bytes of fake dump content so the 512-byte floor passes.
  make_shim pg_dump 'printf "PGDMP"; head -c 4096 /dev/zero'
  make_shim psql 'printf ""'
  make_openssl_copy_shim

  export OUT_DIR="$BATS_TMPDIR/out"
  export KEYRING_SRC="$BATS_TMPDIR/does-not-exist.enc"

  run run_backup_once
  [ "$status" -eq 0 ]

  # Exactly one zip archive is produced.
  archive="$(ls "$OUT_DIR"/zerf-*.zip)"
  [ -f "$archive" ]

  # Archive must contain dump.enc and metadata entries.
  unzip -l "$archive" | grep -q 'dump\.enc'
  unzip -l "$archive" | grep -q 'metadata'

  # Without a keyring source there should be no keyring.enc entry.
  ! unzip -l "$archive" | grep -q 'keyring\.enc'

  # Metadata records keyring absent.
  unzip -p "$archive" metadata | grep -q '^pg_tde_keyring_included=false$'
}

@test "run_backup_once: captures the pg_tde keyring inside the zip archive" {
  # Use >512 bytes of fake dump content so the 512-byte ciphertext floor passes.
  make_shim pg_dump 'printf "PGDMP"; head -c 4096 /dev/zero'
  make_shim psql 'printf ""'
  make_openssl_copy_shim

  export OUT_DIR="$BATS_TMPDIR/out"
  # Provide a fake (already-encrypted) keyring source file.
  printf 'fake-encrypted-keyring' > "$BATS_TMPDIR/keyring.enc"
  export KEYRING_SRC="$BATS_TMPDIR/keyring.enc"

  run run_backup_once
  [ "$status" -eq 0 ]

  archive="$(ls "$OUT_DIR"/zerf-*.zip)"
  [ -f "$archive" ]

  # Archive contains all three entries.
  unzip -l "$archive" | grep -q 'dump\.enc'
  unzip -l "$archive" | grep -q 'metadata'
  unzip -l "$archive" | grep -q 'keyring\.enc'

  # Keyring content is preserved verbatim inside the zip.
  [ "$(unzip -p "$archive" keyring.enc)" = "fake-encrypted-keyring" ]

  # Metadata records keyring included.
  unzip -p "$archive" metadata | grep -q '^pg_tde_keyring_included=true$'
}

@test "run_backup_once: metadata records keyring absent when keyring copy fails" {
  make_shim pg_dump 'printf "PGDMP"; head -c 4096 /dev/zero'
  make_shim psql 'printf ""'
  make_openssl_copy_shim
  # Capture the real cp path before the shim overrides PATH.
  _real_cp="$(command -v cp)"
  # Fail cp when the destination is keyring.enc inside the work directory.
  make_shim cp "
case \"\$2\" in
  */keyring.enc) exit 1 ;;
esac
exec '$_real_cp' \"\$@\"
"

  export OUT_DIR="$BATS_TMPDIR/out"
  printf 'fake-encrypted-keyring' > "$BATS_TMPDIR/keyring.enc"
  export KEYRING_SRC="$BATS_TMPDIR/keyring.enc"

  run run_backup_once
  [ "$status" -eq 0 ]
  [[ "$output" =~ "failed to copy" ]]

  archive="$(ls "$OUT_DIR"/zerf-*.zip)"
  # Archive exists but has no keyring entry.
  ! unzip -l "$archive" 2>/dev/null | grep -q 'keyring\.enc'
  # Metadata records keyring absent.
  unzip -p "$archive" metadata | grep -q '^pg_tde_keyring_included=false$'
}

@test "run_backup_once: succeeds without a keyring when the source is absent" {
  make_shim pg_dump 'printf "PGDMP"; head -c 4096 /dev/zero'
  make_shim psql 'printf ""'
  make_openssl_copy_shim

  export OUT_DIR="$BATS_TMPDIR/out"
  export KEYRING_SRC="$BATS_TMPDIR/does-not-exist.enc"

  run run_backup_once
  # A missing keyring is a warning, never a failure.
  [ "$status" -eq 0 ]

  archive="$(ls "$OUT_DIR"/zerf-*.zip)"
  # No keyring entry in the archive.
  ! unzip -l "$archive" 2>/dev/null | grep -q 'keyring\.enc'
  # Metadata records keyring absent.
  unzip -p "$archive" metadata | grep -q '^pg_tde_keyring_included=false$'
}

# -- apply_retention (count-based: keep last 10) -------------------------------

@test "apply_retention: deletes oldest archives when more than 10 exist" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  # Create 12 zip archives with staggered mtimes so ls -t sorts them reliably.
  for i in $(seq 1 12); do
    f="$OUT_DIR/zerf-$(printf '%012d' "$i").zip"
    printf 'archive' > "$f"
    touch -d "$i seconds ago" "$f"
  done

  apply_retention

  count="$(ls "$OUT_DIR"/*.zip 2>/dev/null | wc -l | tr -d '[:space:]')"
  [ "$count" -eq 10 ]
}

@test "apply_retention: keeps fewer than 10 archives untouched" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  for i in $(seq 1 3); do
    printf 'archive' > "$OUT_DIR/zerf-$(printf '%012d' "$i").zip"
  done

  apply_retention

  count="$(ls "$OUT_DIR"/*.zip 2>/dev/null | wc -l | tr -d '[:space:]')"
  [ "$count" -eq 3 ]
}

@test "apply_retention: no-ops when backup directory is empty" {
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  # Should not fail on empty directory.
  run apply_retention
  [ "$status" -eq 0 ]
}

# -- curl_config_escape -------------------------------------------------------

@test "curl_config_escape: passes through plain ASCII unchanged" {
  run curl_config_escape "MyToken1234"
  [ "$status" -eq 0 ]
  [ "$output" = "MyToken1234" ]
}

@test "curl_config_escape: escapes backslash" {
  run curl_config_escape 'pa\ss'
  [ "$status" -eq 0 ]
  [ "$output" = 'pa\\ss' ]
}

@test "curl_config_escape: escapes double-quote" {
  run curl_config_escape 'pa"ss'
  [ "$status" -eq 0 ]
  [ "$output" = 'pa\"ss' ]
}

@test "curl_config_escape: rejects value containing newline" {
  # Use a bats-bash heredoc variable to embed a literal newline.
  local val
  val="$(printf 'tok\nen')"
  run curl_config_escape "$val"
  [ "$status" -ne 0 ]
}

# -- backup error notifications ------------------------------------------------

@test "notify_admins_backup_error: queues only the central event key" {
  mkdir -p "$BATS_TMPDIR/psql_capture"
  make_shim psql '
args_file="$BATS_TMPDIR/psql_capture/last_args"
printf "%s\n" "$*" > "$args_file"
exit 0
'
  notify_admins_backup_error "backup_failed"
  grep -qF "VALUES ('backup_failed', '', NULL, 'backup')" \
    "$BATS_TMPDIR/psql_capture/last_args"
  ! grep -qF "Database backup failed" "$BATS_TMPDIR/psql_capture/last_args"
}

@test "resolve_admins_backup_error: issues UPDATE for dedupe_key" {
  mkdir -p "$BATS_TMPDIR/psql_capture"
  make_shim psql '
args_file="$BATS_TMPDIR/psql_capture/last_args"
printf "%s\n" "$*" > "$args_file"
exit 0
'
  resolve_admins_backup_error "backup_failed"
  grep -q "UPDATE notifications" "$BATS_TMPDIR/psql_capture/last_args" \
    || grep -qF "backup_failed" "$BATS_TMPDIR/psql_capture/last_args"
}

# -- seconds_until_next_backup: sleep-cap behavior ----------------------------

@test "seconds_until_next_backup: value >3600 is capped by main() sleep logic" {
  # seconds_until_next_backup itself returns the raw remaining seconds (it does
  # not apply the cap -- the cap is in main()).  Verify a 7-day interval from
  # now returns a value >3600 so the main() cap has something to act on.
  _now="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  run seconds_until_next_backup "$_now" 7
  [ "$status" -eq 0 ]
  [ "$output" -gt 3600 ]
}

# -- manual_backup_pending -----------------------------------------------------
#
# This is the idempotency predicate main() uses to decide whether an on-demand
# "Back up now" click still needs acting on. It is tested directly (rather
# than only via main()'s infinite loop, which bats cannot drive) because it is
# the single piece of logic responsible for avoiding both failure modes a
# naive implementation could hit: swallowing a click that arrives mid-run, and
# looping forever if the "handled" marker fails to persist.

@test "manual_backup_pending: false when PENDING is empty (no request ever made)" {
  run manual_backup_pending "" "" ""
  [ "$status" -ne 0 ]
}

@test "manual_backup_pending: true on a brand-new request (fresh boot, nothing handled yet)" {
  run manual_backup_pending "T1" "" ""
  [ "$status" -eq 0 ]
}

@test "manual_backup_pending: false once the persisted HANDLED marker catches up" {
  run manual_backup_pending "T1" "T1" ""
  [ "$status" -ne 0 ]
}

@test "manual_backup_pending: false once the in-memory CONSUMED marker catches up" {
  # Regression: even if backup_last_request_handled_at never gets written
  # (write_app_setting silently swallows errors), CONSUMED alone must still
  # suppress a re-trigger for the rest of this process's lifetime.
  run manual_backup_pending "T1" "" "T1"
  [ "$status" -ne 0 ]
}

@test "manual_backup_pending: true for a new token even though an older one was already handled" {
  run manual_backup_pending "T2" "T1" "T1"
  [ "$status" -eq 0 ]
}

@test "manual_backup_pending: true when PENDING matches neither a stale HANDLED nor a stale CONSUMED" {
  run manual_backup_pending "T3" "T1" "T2"
  [ "$status" -eq 0 ]
}

# -- sleep_until_deadline_or_request ------------------------------------------
#
# Every long sleep in main() routes through this. The regression it guards:
# the post-failure retry backoff used to be a bare `sleep 3600`, which ignored
# a "Back up now" click for up to an hour -- exactly when an admin whose
# backups are failing would be pressing it to test whether they work again.

@test "sleep_until_deadline_or_request: returns early once a new request appears" {
  export ZERF_BACKUP_POLL_INTERVAL_SECS=1
  POLL_INTERVAL_SECS=1
  # psql always reports a pending request token, so the very first poll fires.
  make_shim psql 'printf "T1\n"'

  _start="$(date +%s)"
  run sleep_until_deadline_or_request 30 "" ""
  _elapsed=$(( "$(date +%s)" - _start ))

  [ "$status" -eq 0 ]
  # Returned on the first poll rather than sleeping the full 30 s.
  [ "$_elapsed" -lt 5 ]
}

@test "sleep_until_deadline_or_request: sleeps the full duration when no request appears" {
  export ZERF_BACKUP_POLL_INTERVAL_SECS=1
  POLL_INTERVAL_SECS=1
  make_shim psql 'printf ""'

  _start="$(date +%s)"
  run sleep_until_deadline_or_request 3 "" ""
  _elapsed=$(( "$(date +%s)" - _start ))

  [ "$status" -eq 0 ]
  [ "$_elapsed" -ge 3 ]
}

@test "sleep_until_deadline_or_request: ignores a request both markers already account for" {
  export ZERF_BACKUP_POLL_INTERVAL_SECS=1
  POLL_INTERVAL_SECS=1
  # The stored token matches the HANDLED marker, so it is NOT a new request
  # and must not cut the sleep short.
  make_shim psql 'printf "T1\n"'

  _start="$(date +%s)"
  run sleep_until_deadline_or_request 3 "T1" ""
  _elapsed=$(( "$(date +%s)" - _start ))

  [ "$status" -eq 0 ]
  [ "$_elapsed" -ge 3 ]
}

# -- read_backup_request / read_handled_request / mark_request_handled --------

@test "read_backup_request: returns the stored, whitespace-trimmed value" {
  make_shim psql 'printf " T1 \n"'
  run read_backup_request
  [ "$status" -eq 0 ]
  [ "$output" = "T1" ]
}

@test "read_backup_request: empty when no request has ever been made" {
  make_shim psql 'printf ""'
  run read_backup_request
  [ "$status" -eq 0 ]
  [ "$output" = "" ]
}

@test "read_handled_request: returns the stored, whitespace-trimmed value" {
  make_shim psql 'printf " T1 \n"'
  run read_handled_request
  [ "$status" -eq 0 ]
  [ "$output" = "T1" ]
}

@test "read_handled_request: empty before any request has been handled" {
  make_shim psql 'printf ""'
  run read_handled_request
  [ "$status" -eq 0 ]
  [ "$output" = "" ]
}

@test "mark_request_handled: writes backup_last_request_handled_at with the given token" {
  mkdir -p "$BATS_TMPDIR/psql_capture"
  make_shim psql '
args_file="$BATS_TMPDIR/psql_capture/last_args"
printf "%s\n" "$*" > "$args_file"
exit 0
'
  mark_request_handled "T1"
  grep -qF "backup_last_request_handled_at" "$BATS_TMPDIR/psql_capture/last_args"
  grep -qF "'T1'" "$BATS_TMPDIR/psql_capture/last_args"
}

# -- write_last_manual_at: must never touch the scheduled clock ---------------

@test "write_last_manual_at: writes backup_last_manual_at, never backup_last_success_at" {
  # Regression for the schedule-starvation flaw: a manual backup must not
  # move the clock that is_backup_due measures the scheduled interval from,
  # or repeated manual runs would postpone -- and could indefinitely starve --
  # the scheduled backup.
  mkdir -p "$BATS_TMPDIR/psql_capture"
  make_shim psql '
args_file="$BATS_TMPDIR/psql_capture/last_args"
printf "%s\n" "$*" > "$args_file"
exit 0
'
  write_last_manual_at
  grep -qF "backup_last_manual_at" "$BATS_TMPDIR/psql_capture/last_args"
  ! grep -qF "backup_last_success_at" "$BATS_TMPDIR/psql_capture/last_args"
}

# -- run_backup_once: manual vs scheduled archive naming -----------------------

@test "run_backup_once: manual run produces a -manual.zip archive" {
  make_shim pg_dump 'printf "PGDMP"; head -c 4096 /dev/zero'
  make_shim psql 'printf ""'
  make_openssl_copy_shim

  export OUT_DIR="$BATS_TMPDIR/out"
  export KEYRING_SRC="$BATS_TMPDIR/does-not-exist.enc"

  run run_backup_once manual
  [ "$status" -eq 0 ]

  archive="$(ls "$OUT_DIR"/zerf-*-manual.zip)"
  [ -f "$archive" ]
}

@test "run_backup_once: no argument (scheduled default) never produces a -manual.zip archive" {
  make_shim pg_dump 'printf "PGDMP"; head -c 4096 /dev/zero'
  make_shim psql 'printf ""'
  make_openssl_copy_shim

  export OUT_DIR="$BATS_TMPDIR/out"
  export KEYRING_SRC="$BATS_TMPDIR/does-not-exist.enc"

  run run_backup_once
  [ "$status" -eq 0 ]

  archive="$(ls "$OUT_DIR"/zerf-*.zip)"
  [ -f "$archive" ]
  case "$archive" in
    *-manual.zip) false ;;
  esac
}

# -- apply_retention: scheduled and manual classes are pruned independently ---

@test "apply_retention: manual archives can never evict scheduled ones, and vice versa" {
  # Regression test for the original design flaw in this feature: a naive
  # count-based retention over ALL zerf-*.zip files would let repeated manual
  # "Back up now" clicks silently evict scheduled backup history. Ages are
  # interleaved across both classes (rather than each class occupying its own
  # time range) specifically so that a bug treating them as one combined pool
  # of 10 is actually caught -- it would keep only 5 of each instead of 10 of
  # each.
  export OUT_DIR="$BATS_TMPDIR/out"
  mkdir -p "$OUT_DIR"

  # Scheduled archives at odd ages 1,3,5,...,23 seconds ago (12 files).
  for i in $(seq 1 12); do
    age=$(( i * 2 - 1 ))
    f="$OUT_DIR/zerf-$(printf '%03d' "$age").zip"
    printf 'archive' > "$f"
    touch -d "$age seconds ago" "$f"
  done
  # Manual archives at even ages 2,4,6,...,24 seconds ago (12 files),
  # interleaved with the scheduled ones above.
  for i in $(seq 1 12); do
    age=$(( i * 2 ))
    f="$OUT_DIR/zerf-$(printf '%03d' "$age")-manual.zip"
    printf 'archive' > "$f"
    touch -d "$age seconds ago" "$f"
  done

  apply_retention

  scheduled_count="$(ls "$OUT_DIR"/zerf-*.zip 2>/dev/null | grep -v -- '-manual\.zip$' | wc -l | tr -d '[:space:]')"
  manual_count="$(ls "$OUT_DIR"/zerf-*-manual.zip 2>/dev/null | wc -l | tr -d '[:space:]')"
  [ "$scheduled_count" -eq 10 ]
  [ "$manual_count" -eq 10 ]

  # Retention now sorts by filename (timestamp lex sort) reverse, not mtime – newest timestamp = largest filename.
  # With test data 001 newest by mtime but smallest filename, the "newest by name" are the largest numbers.
  # So survivors are 023..005 (scheduled) and 024..006 (manual); oldest by name (001,003,002,004) are pruned.
  [ -f "$OUT_DIR/zerf-023.zip" ]
  [ -f "$OUT_DIR/zerf-024-manual.zip" ]
  [ ! -f "$OUT_DIR/zerf-001.zip" ]
  [ ! -f "$OUT_DIR/zerf-003.zip" ]
  [ ! -f "$OUT_DIR/zerf-002-manual.zip" ]
  [ ! -f "$OUT_DIR/zerf-004-manual.zip" ]
}

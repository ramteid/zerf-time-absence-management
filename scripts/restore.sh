#!/bin/bash
# Zerf backup restore helper.
#
# Usage:
#   ./scripts/restore.sh                    — list available backups and choose
#   ./scripts/restore.sh <file.dump.enc>    — restore a specific file
#
# What this script does:
#   1. Loads ZERF_DB_ENCRYPTION_KEY and database credentials from .env.
#   2. Decrypts the chosen .dump.enc file to a host temp file.
#   3. Stops the app container (to prevent writes during restore).
#   4. Drops existing application objects and restores from the backup.
#   5. Optionally restarts the app.  On startup the app applies any pending
#      sqlx migrations automatically.
#
# Migration compatibility:
#   Backup older than current code  → app applies pending migrations on start.
#   Backup newer than current code  → schema may contain columns/tables the
#                                     current binary does not understand; update
#                                     the app before restarting after restore.
set -euo pipefail

# Ensure all temp files (PLAIN_TMP, TMP_COPY, META_TMP) are created 0600 from
# the instant they appear, not just after a follow-up chmod.  On Linux mktemp
# already creates 0600 files (glibc default), but the explicit umask makes the
# intent clear and is defensive against environments where that default differs.
umask 077

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
ENV_FILE="$ROOT/.env"

POSTGRES_CONTAINER="zerf-postgres"
APP_CONTAINER="zerf-app"
BACKUP_VOLUME="zerf_backup_data"
# postgres:18 has pg_dump / pg_restore and is already used by the backup
# container, so it should be cached locally — no extra pull.
HELPER_IMAGE="postgres:18"

# ── Helpers ──────────────────────────────────────────────────────────────────

die()  { echo "ERROR: $*" >&2; exit 1; }
info() { echo "  $*"; }

confirm() {
    local prompt="$1"
    local answer
    printf '%s [y/N] ' "$prompt"
    read -r answer
    case "$answer" in y|Y|yes|YES) return 0 ;; esac
    echo "Aborted."
    exit 0
}

# ── Load .env ─────────────────────────────────────────────────────────────────

[ -f "$ENV_FILE" ] || die ".env not found at $ENV_FILE — copy .env.example and fill in the values."

# `set -a` exports every variable that gets defined during the source.
# This is the standard way to load a .env style file into the current shell.
set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

# Required values — fail fast if any are missing.
: "${ZERF_DB_ENCRYPTION_KEY:?ZERF_DB_ENCRYPTION_KEY must be set in .env}"
: "${ZERF_POSTGRES_USER:?ZERF_POSTGRES_USER must be set in .env}"
: "${ZERF_POSTGRES_PASSWORD:?ZERF_POSTGRES_PASSWORD must be set in .env}"
: "${ZERF_POSTGRES_DB:?ZERF_POSTGRES_DB must be set in .env}"

# ── Cleanup on exit (success or failure) ─────────────────────────────────────

PLAIN_TMP=""
TMP_COPY=""
TMP_COPY_DIR=""   # isolated dir — only this dir is mounted into helper containers
META_TMP=""
META_TMP_DIR=""   # isolated dir — same reason
cleanup() {
    [ -n "$PLAIN_TMP" ]    && rm -f  "$PLAIN_TMP"
    [ -n "$TMP_COPY" ]     && rm -f  "$TMP_COPY"
    [ -n "$TMP_COPY_DIR" ] && rm -rf "$TMP_COPY_DIR"
    [ -n "$META_TMP" ]     && rm -f  "$META_TMP"
    [ -n "$META_TMP_DIR" ] && rm -rf "$META_TMP_DIR"
    # Best-effort: remove the temp dump from inside the postgres container.
    # Suppress errors so cleanup never masks the real exit status.
    docker exec "$POSTGRES_CONTAINER" rm -f /tmp/zerf-restore.dump 2>/dev/null || true
}
trap cleanup EXIT

# ── Choose backup file ────────────────────────────────────────────────────────

BACKUP_FILE="${1:-}"
BACKUP_CAME_FROM_VOLUME=0
SELECTED=""

if [ -z "$BACKUP_FILE" ]; then
    echo ""
    echo "Available backups (newest first):"
    echo ""

    docker volume inspect "$BACKUP_VOLUME" >/dev/null 2>&1 \
        || die "Docker volume $BACKUP_VOLUME not found. Is the stack running?"

    # List .dump.enc files inside the volume.  The helper container reads only.
    mapfile -t BACKUPS < <(
        docker run --rm \
            -v "$BACKUP_VOLUME:/backups:ro" \
            --entrypoint sh \
            "$HELPER_IMAGE" \
            -c 'ls -1t /backups/*.dump.enc 2>/dev/null' \
        | sed 's|/backups/||'
    )

    [ ${#BACKUPS[@]} -gt 0 ] || die "No .dump.enc files found in $BACKUP_VOLUME."

    for i in "${!BACKUPS[@]}"; do
        printf '  [%d] %s\n' "$((i+1))" "${BACKUPS[$i]}"
    done
    echo ""
    printf 'Choose a backup [1-%d]: ' "${#BACKUPS[@]}"
    read -r CHOICE

    [[ "$CHOICE" =~ ^[0-9]+$ ]] || die "Not a number."
    [ "$CHOICE" -ge 1 ] && [ "$CHOICE" -le "${#BACKUPS[@]}" ] \
        || die "Choice out of range."

    SELECTED="${BACKUPS[$((CHOICE-1))]}"

    # Copy the chosen file out of the volume to a host temp location.  The
    # cleanup trap removes it when the script exits.
    #
    # Use an isolated temp directory (not all of /tmp) as the bind-mount so
    # the helper container only sees one file, not every process's temp files.
    # We pass SELECTED via -e so the helper's `sh -c` sees it as an env var,
    # never as part of the command string — shell metacharacters in the
    # filename are therefore inert.
    TMP_COPY_DIR="$(mktemp -d)"
    TMP_COPY="$TMP_COPY_DIR/backup.dump.enc"
    docker run --rm \
        -v "$BACKUP_VOLUME:/backups:ro" \
        -v "$TMP_COPY_DIR:/out" \
        -e "SRC=$SELECTED" \
        --entrypoint sh \
        "$HELPER_IMAGE" \
        -c 'cp "/backups/$SRC" "/out/backup.dump.enc"' \
        || die "Could not copy $SELECTED out of the backup volume."

    BACKUP_FILE="$TMP_COPY"
    BACKUP_CAME_FROM_VOLUME=1
else
    [ -f "$BACKUP_FILE" ] || die "File not found: $BACKUP_FILE"
fi

# ── Look up matching metadata (best-effort, no failure if absent) ────────────

METADATA_FILE="${BACKUP_FILE%.dump.enc}.metadata"

if [ ! -f "$METADATA_FILE" ] && [ "$BACKUP_CAME_FROM_VOLUME" = "1" ]; then
    META_NAME="${SELECTED%.dump.enc}.metadata"
    META_TMP_DIR="$(mktemp -d)"
    META_TMP="$META_TMP_DIR/metadata"
    docker run --rm \
        -v "$BACKUP_VOLUME:/backups:ro" \
        -v "$META_TMP_DIR:/out" \
        -e "SRC=$META_NAME" \
        --entrypoint sh \
        "$HELPER_IMAGE" \
        -c 'cp "/backups/$SRC" "/out/metadata" 2>/dev/null' \
        2>/dev/null || true
    [ -s "$META_TMP" ] && METADATA_FILE="$META_TMP" || { rm -rf "$META_TMP_DIR"; META_TMP_DIR=""; META_TMP=""; }
fi

# ── Show metadata and confirm ────────────────────────────────────────────────

echo ""
echo "┌─ Restore target ───────────────────────────────────────────────────"
echo "│  File:     ${SELECTED:-$BACKUP_FILE}"
if [ -f "$METADATA_FILE" ]; then
    BACKUP_TS=$(grep '^created_at_utc=' "$METADATA_FILE" | cut -d= -f2- || true)
    BACKUP_COMMIT=$(grep '^ZERF_GIT_COMMIT=' "$METADATA_FILE" | cut -d= -f2- || true)
    [ -n "$BACKUP_TS" ]     && info "Created:  $BACKUP_TS"
    [ -n "$BACKUP_COMMIT" ] && info "Commit:   $BACKUP_COMMIT"

    # Use the full SHA on both sides so the equality check matches: backups
    # produced by start_public.sh record the full hash via `git rev-parse HEAD`,
    # so comparing against the short hash would always trigger the warning.
    CURRENT_COMMIT="${ZERF_GIT_COMMIT:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)}"
    if [ -n "$BACKUP_COMMIT" ] \
       && [ "$BACKUP_COMMIT" != "$CURRENT_COMMIT" ] \
       && [ "$BACKUP_COMMIT" != "unknown" ]; then
        echo "│"
        echo "│  ⚠  Backup commit ($BACKUP_COMMIT) differs from current ($CURRENT_COMMIT)."
        echo "│     • Backup older than code → app applies pending migrations on start."
        echo "│     • Backup newer than code → update the app BEFORE restarting it."
    fi
fi
echo "│  Database: $ZERF_POSTGRES_DB  (user: $ZERF_POSTGRES_USER)"
echo "└────────────────────────────────────────────────────────────────────"
echo ""
echo "⚠  This will REPLACE ALL DATA in the live database."
confirm "Continue?"

# ── Verify postgres is running before we go further ──────────────────────────

POSTGRES_STATUS="$(docker inspect -f '{{.State.Status}}' "$POSTGRES_CONTAINER" 2>/dev/null || echo missing)"
[ "$POSTGRES_STATUS" = "running" ] \
    || die "Container $POSTGRES_CONTAINER is not running (status: $POSTGRES_STATUS).  Start the stack first."

# ── Decrypt ───────────────────────────────────────────────────────────────────

PLAIN_TMP="$(mktemp /tmp/zerf-restore-XXXXXX.dump)"

echo ""
echo "Decrypting backup…"
openssl enc -d -aes-256-cbc -pbkdf2 -iter 100000 \
    -pass env:ZERF_DB_ENCRYPTION_KEY \
    -in  "$BACKUP_FILE" \
    -out "$PLAIN_TMP" \
    || die "Decryption failed — wrong ZERF_DB_ENCRYPTION_KEY or corrupted file."

# openssl can exit 0 on a truncated input that still parses as a (very small)
# valid stream.  Reject empty decrypted output before we touch the live DB.
if [ ! -s "$PLAIN_TMP" ]; then
    die "Decrypted dump is empty — refusing to restore.  Backup file is likely truncated or corrupted."
fi

# ── Stop the app to prevent mid-restore writes ───────────────────────────────

APP_WAS_RUNNING=0
APP_STATUS="$(docker inspect -f '{{.State.Status}}' "$APP_CONTAINER" 2>/dev/null || echo missing)"
if [ "$APP_STATUS" = "running" ]; then
    echo "Stopping app container…"
    docker stop "$APP_CONTAINER" >/dev/null
    APP_WAS_RUNNING=1
fi

# ── Restore ───────────────────────────────────────────────────────────────────

echo "Copying dump into postgres container…"
# Pre-clear any stale dump left by a previous restore that was hard-killed
# (SIGKILL bypasses the EXIT trap, so the cleanup inside the container may
# not have run).  Idempotent: a missing file is silently ignored.
docker exec "$POSTGRES_CONTAINER" rm -f /tmp/zerf-restore.dump 2>/dev/null || true
docker cp "$PLAIN_TMP" "$POSTGRES_CONTAINER:/tmp/zerf-restore.dump"

echo "Restoring…"
# --clean      drop objects before recreating them (replaces existing data)
# --if-exists  suppress errors for objects that don't exist in the target db
# --no-owner   do not set ownership (current db role owns everything)
# --no-privileges  skip GRANT/REVOKE (the app role uses its own fixed grants)
docker exec \
    -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" \
    "$POSTGRES_CONTAINER" \
    pg_restore \
        --host 127.0.0.1 \
        --username "$ZERF_POSTGRES_USER" \
        --dbname "$ZERF_POSTGRES_DB" \
        --clean \
        --if-exists \
        --no-owner \
        --no-privileges \
        /tmp/zerf-restore.dump \
    || die "pg_restore exited with errors — review the output above."

echo ""
echo "✓ Restore complete."
echo ""

# ── Restart the app ──────────────────────────────────────────────────────────

if [ "$APP_WAS_RUNNING" = "1" ]; then
    if confirm "Restart the app container now?"; then
        docker start "$APP_CONTAINER" >/dev/null
        echo "App restarted. Pending sqlx migrations (if any) will run on startup."
    else
        echo "App is stopped. Start it manually when ready:"
        echo "  docker start $APP_CONTAINER"
        echo ""
        echo "If the backup is from a NEWER app version than the current binary,"
        echo "update the app first to avoid schema mismatches."
    fi
fi

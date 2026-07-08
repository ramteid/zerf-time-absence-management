#!/bin/bash
# Zerf backup restore helper.
#
# Usage:
#   ./scripts/restore.sh                    - list available backups and choose
#   ./scripts/restore.sh <file.dump.enc>    - restore a specific file
#   ./scripts/restore.sh --keyring [DIR]    - extract a backup's pg_tde keyring
#                                             to DIR (default: cwd) for physical
#                                             recovery; makes no database changes
#
# What this script does:
#   1. Loads ZERF_DB_ENCRYPTION_KEY and database credentials from .env.
#   2. Verifies the chosen .dump.enc file can be decrypted and read by pg_restore.
#   3. Stops the app container AND the backup container (to prevent mid-restore
#      writes and backup-container pg_dump racing the restore).
#   4. Drops all non-extension objects in the public schema so that restoring an
#      older dump onto a newer schema works correctly (tables added by later
#      migrations would otherwise block pg_restore --clean).
#   5. Restores the backup with --exit-on-error.
#   6. Restarts the backup container (if it was running), then optionally the app.
#      On startup the app applies any pending sqlx migrations automatically.
#
# Migration compatibility:
#   Backup older than current code  -> app applies pending migrations on start.
#   Backup newer than current code  -> schema may contain columns/tables the
#                                     current binary does not understand; update
#                                     the app before restarting after restore.
set -euo pipefail

# Ensure all temp files (META_TMP) are created 0600 from the instant they
# appear, not just after a follow-up chmod.  On Linux mktemp already creates
# 0600 files (glibc default), but the explicit umask makes the intent clear and
# is defensive against environments where that default differs.
umask 077

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# Overridable so the e2e suite (e2e/backup-restore-check.sh) can point this at
# its own isolated stack (different container names, backup volume, and env
# file) without ever touching a real deployment's .env. Defaults are exactly
# the production names, so normal interactive use is unaffected.
ENV_FILE="${ZERF_RESTORE_ENV_FILE:-$ROOT/.env}"

POSTGRES_CONTAINER="${ZERF_RESTORE_POSTGRES_CONTAINER:-zerf-postgres}"
APP_CONTAINER="${ZERF_RESTORE_APP_CONTAINER:-zerf-app}"
BACKUP_CONTAINER="${ZERF_RESTORE_BACKUP_CONTAINER:-zerf-backup}"
BACKUP_VOLUME="${ZERF_RESTORE_BACKUP_VOLUME:-zerf_backup_data}"
# The helper image runs pg_restore and lists files from the backup volume.
# It may be pulled from Docker Hub on registry-deployed hosts where the base
# layer was not cached locally (the backup image is *built* from postgres:18 but
# the base tag is not present when images are pulled from ghcr).
# Override with ZERF_RESTORE_HELPER_IMAGE to use a pre-pulled local image.
HELPER_IMAGE="${ZERF_RESTORE_HELPER_IMAGE:-postgres:18}"

# -- Helpers ------------------------------------------------------------------

die()  { echo "ERROR: $*" >&2; exit 1; }
info() { echo "  $*"; }

# Prompt for confirmation.  Returns 0 on yes, 1 on no/anything else.
# Does NOT call exit -- the caller decides what to do on decline.
confirm() {
    local prompt="$1"
    local answer
    printf '%s [y/N] ' "$prompt"
    read -r answer
    case "$answer" in y|Y|yes|YES) return 0 ;; esac
    return 1
}

decrypt_backup_to_stdout() {
    local file="$1"
    openssl enc -d -aes-256-cbc -pbkdf2 -iter 100000 \
        -pass env:ZERF_DB_ENCRYPTION_KEY \
        -in "$file"
}

# -- Keyring extraction mode (physical recovery) ------------------------------
#
# Each backup may carry a sibling zerf-<ts>.keyring.enc - a copy of the pg_tde
# keyring. The logical restore below does NOT need it, but recovering an
# orphaned, encrypted PGDATA volume does. This mode copies a chosen keyring out
# of the backup volume so it can be paired with such a volume. It never touches
# the database, so it needs no .env / credentials.
extract_keyring() {
    local out_dir="${1:-$PWD}"
    [ -d "$out_dir" ] || die "Output directory does not exist: $out_dir"
    out_dir="$(cd "$out_dir" && pwd)" || die "Could not resolve output directory: $out_dir"

    docker volume inspect "$BACKUP_VOLUME" >/dev/null 2>&1 \
        || die "Docker volume $BACKUP_VOLUME not found. Is the stack running?"

    local keyrings=()
    mapfile -t keyrings < <(
        docker run --rm \
            -v "$BACKUP_VOLUME:/backups:ro" \
            --entrypoint sh \
            "$HELPER_IMAGE" \
            -c 'ls -1t /backups/*.keyring.enc 2>/dev/null' \
        | sed 's|/backups/||'
    )

    [ ${#keyrings[@]} -gt 0 ] \
        || die "No .keyring.enc files in $BACKUP_VOLUME. Backups made before keyring capture was added do not contain one; the postgres keyring volume (zerf_postgres_data) is then the only source."

    echo ""
    echo "Available keyrings (newest first):"
    echo ""
    local i
    for i in "${!keyrings[@]}"; do
        printf '  [%d] %s\n' "$((i+1))" "${keyrings[$i]}"
    done
    echo ""
    printf 'Choose a keyring to extract [1-%d]: ' "${#keyrings[@]}"
    local choice
    read -r choice
    [[ "$choice" =~ ^[0-9]+$ ]] || die "Not a number."
    [ "$choice" -ge 1 ] && [ "$choice" -le "${#keyrings[@]}" ] || die "Choice out of range."

    local selected="${keyrings[$((choice-1))]}"
    # Pass the filename via -e so shell metacharacters in it stay inert.
    docker run --rm \
        -v "$BACKUP_VOLUME:/backups:ro" \
        -v "$out_dir:/out" \
        -e "SRC=$selected" \
        --entrypoint sh \
        "$HELPER_IMAGE" \
        -c 'cp "/backups/$SRC" "/out/$SRC" && chmod 0644 "/out/$SRC"' \
        || die "Could not copy $selected out of the backup volume."

    # The helper (image uid) already made the copy world-readable; this host-side
    # chmod tightens it to 0600 when the host user owns it, and is a harmless
    # no-op (|| true) when the copy is owned by the container's uid instead.
    chmod 600 "$out_dir/$selected" 2>/dev/null || true
    echo ""
    echo "Keyring extracted to: $out_dir/$selected"
    echo ""
    echo "This is the pg_tde keyring, encrypted with ZERF_DB_ENCRYPTION_KEY. To"
    echo "recover an orphaned, encrypted PGDATA volume, place it as"
    echo "pg_tde_keyring.enc in the postgres keyring volume (zerf_postgres_data)"
    echo "and start postgres against the recovered data directory."
    echo "See the 'Backup and restore' section of docs/user-guide.md for the"
    echo "full physical recovery procedure."
    echo ""
    echo "WARNING: Do NOT overwrite a working keyring: if the live database already"
    echo "   starts and decrypts its data, its current keyring is the right one."
}

# Dispatch keyring-extraction mode before loading .env (no DB credentials
# needed). Any remaining argument is the output directory.
if [ "${1:-}" = "--keyring" ] || [ "${1:-}" = "--extract-keyring" ]; then
    shift
    extract_keyring "$@"
    exit 0
fi

# -- Load .env ----------------------------------------------------------------

[ -f "$ENV_FILE" ] || die ".env not found at $ENV_FILE - copy .env.example and fill in the values."

# `set -a` exports every variable that gets defined during the source.
# This is the standard way to load a .env style file into the current shell.
set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

# Required values - fail fast if any are missing.
: "${ZERF_DB_ENCRYPTION_KEY:?ZERF_DB_ENCRYPTION_KEY must be set in .env}"
: "${ZERF_POSTGRES_USER:?ZERF_POSTGRES_USER must be set in .env}"
: "${ZERF_POSTGRES_PASSWORD:?ZERF_POSTGRES_PASSWORD must be set in .env}"
: "${ZERF_POSTGRES_DB:?ZERF_POSTGRES_DB must be set in .env}"

# -- Cleanup on exit (success or failure) -------------------------------------

META_TMP=""
META_TMP_DIR=""   # isolated dir - same reason
cleanup() {
    [ -n "$META_TMP" ]     && rm -f  "$META_TMP"
    [ -n "$META_TMP_DIR" ] && rm -rf "$META_TMP_DIR"
    # Best-effort: remove restore temp files from inside the
    # postgres container.  Suppress errors so cleanup never masks the real exit.
    docker exec "$POSTGRES_CONTAINER" rm -f /tmp/zerf-restore.enc /tmp/zerf-restore.toc /tmp/zerf-restore.full.toc 2>/dev/null || true
}
trap cleanup EXIT

# -- Choose backup file --------------------------------------------------------

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
    # BACKUP_FILE is a placeholder that matches the naming convention used by
    # the code below to derive METADATA_FILE and to display the source.  It is
    # never created on disk: the encrypted dump is streamed directly into the
    # postgres container's tmpfs (see below).
    BACKUP_FILE="/volume/${SELECTED}"

    # Copy the chosen file out of the volume directly into the postgres
    # container's tmpfs, bypassing the host disk entirely (plaintext never
    # touches persistent host storage).  Use docker exec -i cat rather than
    # docker cp so the write lands on the tmpfs, not behind it.
    #
    # We stream: volume -> helper container stdout -> host pipe -> postgres
    # container /tmp/zerf-restore.enc.
    docker run --rm \
        -v "$BACKUP_VOLUME:/backups:ro" \
        -e "SRC=$SELECTED" \
        --entrypoint sh \
        "$HELPER_IMAGE" \
        -c 'cat "/backups/$SRC"' \
    | docker exec -i "$POSTGRES_CONTAINER" sh -c 'cat > /tmp/zerf-restore.enc' \
        || die "Could not copy $SELECTED into the restore container."
    BACKUP_CAME_FROM_VOLUME=1
else
    [ -f "$BACKUP_FILE" ] || die "File not found: $BACKUP_FILE"
fi

# -- Look up matching metadata (best-effort, no failure if absent) -------------

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
        -c 'cp "/backups/$SRC" "/out/metadata" && chmod 0644 "/out/metadata"' \
        2>/dev/null || true
    if [ -s "$META_TMP" ]; then
        METADATA_FILE="$META_TMP"
    else
        rm -rf "$META_TMP_DIR"; META_TMP_DIR=""; META_TMP=""
    fi
fi

# -- Show metadata and confirm -------------------------------------------------

echo ""
echo "Restore target"
echo "--------------"
echo "  File:     ${SELECTED:-$BACKUP_FILE}"
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
        echo ""
        echo "  WARNING: Backup commit ($BACKUP_COMMIT) differs from current ($CURRENT_COMMIT)."
        echo "     Backup older than code -> app applies pending migrations on start."
        echo "     Backup newer than code -> update the app BEFORE restarting it."
    fi
fi
echo "  Database: $ZERF_POSTGRES_DB  (user: $ZERF_POSTGRES_USER)"
echo ""
echo "WARNING: This will REPLACE ALL DATA in the live database."
if ! confirm "Continue?"; then
    echo "Aborted."
    exit 0
fi

# -- Verify postgres is running before we go further ---------------------------

POSTGRES_STATUS="$(docker inspect -f '{{.State.Status}}' "$POSTGRES_CONTAINER" 2>/dev/null || echo missing)"
[ "$POSTGRES_STATUS" = "running" ] \
    || die "Container $POSTGRES_CONTAINER is not running (status: $POSTGRES_STATUS).  Start the stack first."

# -- Validate backup and prepare restore list ---------------------------------
#
# Build a restore list that EXCLUDES the pg_tde extension (and its COMMENT).
#
# Why this is required: --clean drops every object it is about to restore, in
# reverse order.  With the extension in the list that includes
# `DROP EXTENSION pg_tde`, which wipes the pg_tde principal-key configuration.
# The extension is then recreated empty - its key-provider setup lives in the
# container init scripts, NOT in the dump - so every `CREATE TABLE ... USING
# tde_heap` fails with "principal key not configured" and the restore destroys
# the database instead of repopulating it.  Filtering the extension out leaves
# the live pg_tde (and its key) untouched while still dropping and recreating
# all application objects.  Verified end-to-end against the Percona pg_tde image.
#
# This step is read-only with respect to the database, so it runs BEFORE
# stopping any containers.  A corrupt dump or decryption failure is caught here
# while everything is still running.
echo ""
echo "Preparing restore list (preserving the pg_tde extension)..."
docker exec "$POSTGRES_CONTAINER" rm -f /tmp/zerf-restore.toc /tmp/zerf-restore.full.toc 2>/dev/null || true

if [ "$BACKUP_CAME_FROM_VOLUME" = "1" ]; then
    # The encrypted dump was already copied into the container as /tmp/zerf-restore.enc.
    # Decrypt it in-container and pipe into pg_restore --list.
    # Do NOT use -i: the command reads from /tmp/zerf-restore.enc (not from host stdin),
    # so -i would attach the host's stdin (which may be a piped prompt-response sequence)
    # and docker exec's stdin-forwarding goroutine could consume bytes meant for later
    # read() calls in this script (e.g. the restart confirmation prompt).
    docker exec \
        -e ZERF_DB_ENCRYPTION_KEY="$ZERF_DB_ENCRYPTION_KEY" \
        "$POSTGRES_CONTAINER" \
        sh -c "openssl enc -d -aes-256-cbc -pbkdf2 -iter 100000 \
                   -pass env:ZERF_DB_ENCRYPTION_KEY \
                   -in /tmp/zerf-restore.enc \
               | pg_restore --list > /tmp/zerf-restore.full.toc && \
               grep -vE 'EXTENSION - pg_tde|COMMENT - EXTENSION pg_tde' \
                   /tmp/zerf-restore.full.toc > /tmp/zerf-restore.toc" \
        || die "Failed to decrypt or inspect the backup archive. Check ZERF_DB_ENCRYPTION_KEY and the backup file."
else
    decrypt_backup_to_stdout "$BACKUP_FILE" | docker exec -i "$POSTGRES_CONTAINER" sh -c \
        "pg_restore --list > /tmp/zerf-restore.full.toc &&
         grep -vE 'EXTENSION - pg_tde|COMMENT - EXTENSION pg_tde' /tmp/zerf-restore.full.toc > /tmp/zerf-restore.toc" \
        || die "Failed to decrypt or inspect the backup archive. Check ZERF_DB_ENCRYPTION_KEY and the backup file."
fi

# -- Stop the app and backup containers before touching the DB -----------------

APP_WAS_RUNNING=0
APP_STATUS="$(docker inspect -f '{{.State.Status}}' "$APP_CONTAINER" 2>/dev/null || echo missing)"
if [ "$APP_STATUS" = "running" ]; then
    echo "Stopping app container..."
    docker stop "$APP_CONTAINER" >/dev/null
    APP_WAS_RUNNING=1
fi

BACKUP_WAS_RUNNING=0
BACKUP_STATUS="$(docker inspect -f '{{.State.Status}}' "$BACKUP_CONTAINER" 2>/dev/null || echo missing)"
if [ "$BACKUP_STATUS" = "running" ]; then
    echo "Stopping backup container (prevents a concurrent pg_dump from racing the restore)..."
    docker stop "$BACKUP_CONTAINER" >/dev/null
    BACKUP_WAS_RUNNING=1
fi

# -- Pre-drop all non-extension objects in schema public ----------------------
#
# --clean drops only objects present in the dump.  If the live schema is newer
# than the backup (additional tables added by later migrations), those newer
# tables have foreign-key references to core tables (e.g. users, categories).
# `DROP TABLE users CASCADE` would be required, but --clean emits individual
# DROP statements that are blocked by the FK constraints from the newer tables.
# pg_restore then continues past the errors (even with --exit-on-error, because
# --clean errors are treated differently) producing a mixed/partial state.
#
# Solution: drop everything in schema public that is NOT owned by an extension
# (pg_tde registers its objects with pg_depend deptype='e') before the restore
# runs.  This leaves the database in a clean, empty state that the backup can
# fill without conflicts.  The pg_tde extension itself -- and the database-level
# `default_table_access_method = tde_heap` setting stored in pg_db_role_setting,
# which is untouched by object drops -- are preserved.
echo "Pre-dropping all non-extension public-schema objects..."
docker exec -i \
    -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" \
    -e PGOPTIONS='--statement_timeout=0 --idle_in_transaction_session_timeout=0' \
    "$POSTGRES_CONTAINER" \
    psql \
        --host 127.0.0.1 \
        --username "$ZERF_POSTGRES_USER" \
        --dbname "$ZERF_POSTGRES_DB" \
        -v ON_ERROR_STOP=1 \
        <<'EOSQL'
DO $$
DECLARE
    _obj RECORD;
BEGIN
    -- Drop relations (tables, views, sequences, etc.) in dependency order
    -- by using CASCADE.  Skip objects owned by an extension (deptype='e').
    FOR _obj IN
        WITH ext_owned AS (
            SELECT objid
            FROM pg_depend
            WHERE deptype = 'e'
              AND classid = 'pg_class'::regclass
        )
        SELECT quote_ident(c.relname) AS qname, c.relkind
        FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public'
          AND c.relkind IN ('r','p','v','m','S','f')
          AND c.oid NOT IN (SELECT objid FROM ext_owned)
        ORDER BY c.relkind  -- sequences/views before tables (CASCADE handles FKs anyway)
    LOOP
        CASE _obj.relkind
            WHEN 'r' THEN EXECUTE format('DROP TABLE IF EXISTS %s CASCADE', _obj.qname);
            WHEN 'p' THEN EXECUTE format('DROP TABLE IF EXISTS %s CASCADE', _obj.qname);
            WHEN 'v' THEN EXECUTE format('DROP VIEW  IF EXISTS %s CASCADE', _obj.qname);
            WHEN 'm' THEN EXECUTE format('DROP MATERIALIZED VIEW IF EXISTS %s CASCADE', _obj.qname);
            WHEN 'S' THEN EXECUTE format('DROP SEQUENCE IF EXISTS %s CASCADE', _obj.qname);
            WHEN 'f' THEN EXECUTE format('DROP FOREIGN TABLE IF EXISTS %s CASCADE', _obj.qname);
            ELSE NULL;
        END CASE;
    END LOOP;

    -- Drop functions/procedures in the public schema (extension-owned excluded).
    FOR _obj IN
        WITH ext_owned AS (
            SELECT objid
            FROM pg_depend
            WHERE deptype = 'e'
              AND classid = 'pg_proc'::regclass
        )
        SELECT p.oid, pg_get_function_identity_arguments(p.oid) AS args,
               quote_ident(p.proname) AS qname, p.prokind
        FROM pg_proc p
        JOIN pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname = 'public'
          AND p.oid NOT IN (SELECT objid FROM ext_owned)
    LOOP
        IF _obj.prokind = 'p' THEN
            EXECUTE format('DROP PROCEDURE IF EXISTS %s(%s) CASCADE', _obj.qname, _obj.args);
        ELSE
            EXECUTE format('DROP FUNCTION IF EXISTS %s(%s) CASCADE', _obj.qname, _obj.args);
        END IF;
    END LOOP;

    -- Drop types (enums, composite types) in public, extension-owned excluded.
    FOR _obj IN
        WITH ext_owned AS (
            SELECT objid
            FROM pg_depend
            WHERE deptype = 'e'
              AND classid = 'pg_type'::regclass
        )
        SELECT quote_ident(t.typname) AS qname
        FROM pg_type t
        JOIN pg_namespace n ON n.oid = t.typnamespace
        WHERE n.nspname = 'public'
          AND t.typtype IN ('e','c')  -- enum, composite
          AND t.oid NOT IN (SELECT objid FROM ext_owned)
    LOOP
        EXECUTE format('DROP TYPE IF EXISTS %s CASCADE', _obj.qname);
    END LOOP;
END;
$$;
EOSQL
predrop_exit=$?
if [ "$predrop_exit" -ne 0 ]; then
    echo ""
    echo "ERROR: pre-drop failed. The database may be in a partial state." >&2
    echo "The app and backup containers are stopped. Review the output above." >&2
    echo "To restart manually (without the restore completing):" >&2
    [ "$BACKUP_WAS_RUNNING" = "1" ] && echo "  docker start $BACKUP_CONTAINER" >&2
    [ "$APP_WAS_RUNNING"    = "1" ] && echo "  docker start $APP_CONTAINER" >&2
    exit 1
fi

# -- Restore ------------------------------------------------------------------

echo "Restoring..."
# --clean      drop objects before recreating them (belt + suspenders after
#              the pre-drop above, in case the dump contains objects not in
#              the live schema)
# --if-exists  suppress errors for objects that don't exist in the target db
# --no-owner   do not set ownership (current db role owns everything)
# --no-privileges  skip GRANT/REVOKE (the app role uses its own fixed grants)
# --use-list   restore only the filtered TOC, so pg_tde is never dropped (above)
# --single-transaction  roll back all drops/recreates if any restore step fails
# --exit-on-error       stop at the first SQL error instead of continuing
#
# PGOPTIONS disables server-side statement and idle-in-transaction timeouts for
# this session.  Large schema recreations or COPY streams can legitimately exceed
# the 30 s application timeout; cancelling them would abort the restore.
restore_exit=0
if [ "$BACKUP_CAME_FROM_VOLUME" = "1" ]; then
    # Decrypt in-container from the already-copied /tmp/zerf-restore.enc.
    # Do NOT use -i: the command reads from /tmp/zerf-restore.enc (not from
    # host stdin), and -i would allow docker exec's stdin-copy goroutine to
    # consume bytes from the host's stdin pipe (prompt-response sequence).
    docker exec \
        -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" \
        -e ZERF_DB_ENCRYPTION_KEY="$ZERF_DB_ENCRYPTION_KEY" \
        -e PGOPTIONS='--statement_timeout=0 --idle_in_transaction_session_timeout=0' \
        "$POSTGRES_CONTAINER" \
        sh -c "openssl enc -d -aes-256-cbc -pbkdf2 -iter 100000 \
                   -pass env:ZERF_DB_ENCRYPTION_KEY \
                   -in /tmp/zerf-restore.enc \
               | pg_restore \
                   --host 127.0.0.1 \
                   --username \"$ZERF_POSTGRES_USER\" \
                   --dbname \"$ZERF_POSTGRES_DB\" \
                   --clean \
                   --if-exists \
                   --no-owner \
                   --no-privileges \
                   --single-transaction \
                   --exit-on-error \
                   --use-list=/tmp/zerf-restore.toc" \
        || restore_exit=$?
else
    decrypt_backup_to_stdout "$BACKUP_FILE" | docker exec -i \
        -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" \
        -e PGOPTIONS='--statement_timeout=0 --idle_in_transaction_session_timeout=0' \
        "$POSTGRES_CONTAINER" \
        pg_restore \
            --host 127.0.0.1 \
            --username "$ZERF_POSTGRES_USER" \
            --dbname "$ZERF_POSTGRES_DB" \
            --clean \
            --if-exists \
            --no-owner \
            --no-privileges \
            --single-transaction \
            --exit-on-error \
            --use-list=/tmp/zerf-restore.toc \
        || restore_exit=$?
fi

if [ "$restore_exit" -ne 0 ]; then
    echo ""
    echo "ERROR: pg_restore exited with errors (status $restore_exit)." >&2
    echo "The transaction was rolled back; the database is in the pre-restore state." >&2
    echo "The app and backup containers are stopped. Review the output above." >&2
    echo "To restart manually (without starting the app against a failed restore):" >&2
    [ "$BACKUP_WAS_RUNNING" = "1" ] && echo "  docker start $BACKUP_CONTAINER" >&2
    [ "$APP_WAS_RUNNING"    = "1" ] && echo "  docker start $APP_CONTAINER" >&2
    exit 1
fi

echo ""
echo "Restore complete."
echo ""

# -- Restart backup container -------------------------------------------------

if [ "$BACKUP_WAS_RUNNING" = "1" ]; then
    echo "Restarting backup container..."
    docker start "$BACKUP_CONTAINER" >/dev/null
fi

# -- Restart the app -----------------------------------------------------------

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

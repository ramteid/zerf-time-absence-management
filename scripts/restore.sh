#!/bin/bash
# Zerf backup restore helper.
#
# Usage:
#   ./scripts/restore.sh                    - list available backups and choose
#   ./scripts/restore.sh <file.zip>         - restore a specific archive
#   ./scripts/restore.sh --keyring [DIR]    - extract a backup's pg_tde keyring
#                                             to DIR (default: cwd) for physical
#                                             recovery; makes no database changes
#
# Backup format:
#   New format (zerf-<ts>.zip): a zip archive containing dump.enc (encrypted
#   pg_dump), metadata (plaintext provenance), and optionally keyring.enc
#   (encrypted pg_tde keyring). Created by scripts/backup.sh.
#
#   Legacy format (zerf-<ts>.dump.enc): the encrypted dump file as produced by
#   earlier versions of backup.sh. Restore is still fully supported; metadata
#   and keyring sidecars are looked up as zerf-<ts>.metadata / .keyring.enc.
#
# What this script does:
#   1. Loads ZERF_DB_ENCRYPTION_KEY and database credentials from .env.
#   2. Verifies the chosen backup can be decrypted and read by pg_restore.
#   3. Stops the app container AND the backup container (to prevent mid-restore
#      writes and backup-container pg_dump racing the restore).
#   4. Drops all non-extension objects in the public schema so that restoring an
#      older dump onto a newer schema works correctly.
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

# Ensure all temp files are created 0600 from the instant they appear.
umask 077

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
# Overridable so the e2e suite can point this at its own isolated stack.
ENV_FILE="${ZERF_RESTORE_ENV_FILE:-$ROOT/.env}"

POSTGRES_CONTAINER="${ZERF_RESTORE_POSTGRES_CONTAINER:-zerf-postgres}"
APP_CONTAINER="${ZERF_RESTORE_APP_CONTAINER:-zerf-app}"
BACKUP_CONTAINER="${ZERF_RESTORE_BACKUP_CONTAINER:-zerf-backup}"
BACKUP_VOLUME="${ZERF_RESTORE_BACKUP_VOLUME:-zerf_backup_data}"
# The helper image lists files and streams them from the backup volume.
# Override with ZERF_RESTORE_HELPER_IMAGE to use a pre-pulled local image.
HELPER_IMAGE="${ZERF_RESTORE_HELPER_IMAGE:-postgres:18}"

# -- Helpers ------------------------------------------------------------------

die()  { echo "ERROR: $*" >&2; exit 1; }
info() { echo "  $*"; }

# Prompt for confirmation.  Returns 0 on yes, 1 on no/anything else.
confirm() {
    local prompt="$1"
    local answer
    printf '%s [y/N] ' "$prompt"
    read -r answer
    case "$answer" in y|Y|yes|YES) return 0 ;; esac
    return 1
}

# -- Keyring extraction mode (physical recovery) ------------------------------
#
# Each zip backup may contain a keyring.enc entry -- a copy of the pg_tde
# keyring. The logical restore below does NOT need it, but recovering an
# orphaned, encrypted PGDATA volume does. This mode extracts a chosen keyring
# from the backup volume so it can be paired with such a volume. It never
# touches the database, so it needs no .env / credentials.
extract_keyring() {
    local out_dir="${1:-$PWD}"
    [ -d "$out_dir" ] || die "Output directory does not exist: $out_dir"
    out_dir="$(cd "$out_dir" && pwd)" || die "Could not resolve output directory: $out_dir"

    docker volume inspect "$BACKUP_VOLUME" >/dev/null 2>&1 \
        || die "Docker volume $BACKUP_VOLUME not found. Is the stack running?"

    # List all backup archives (new zip format) and legacy .keyring.enc sidecars.
    local keyrings=()
    mapfile -t keyrings < <(
        docker run --rm \
            -v "$BACKUP_VOLUME:/backups:ro" \
            --entrypoint sh \
            "$HELPER_IMAGE" \
            -c 'ls -1t /backups/zerf-*.zip /backups/zerf-*.keyring.enc 2>/dev/null' \
        | sed 's|/backups/||'
    )

    [ ${#keyrings[@]} -gt 0 ] \
        || die "No backup archives or keyring sidecars found in $BACKUP_VOLUME."

    echo ""
    echo "Available backups with keyring (newest first):"
    echo ""
    local i
    for i in "${!keyrings[@]}"; do
        printf '  [%d] %s\n' "$((i+1))" "${keyrings[$i]}"
    done
    echo ""
    printf 'Choose a backup to extract the keyring from [1-%d]: ' "${#keyrings[@]}"
    local choice
    read -r choice
    [[ "$choice" =~ ^[0-9]+$ ]] || die "Not a number."
    [ "$choice" -ge 1 ] && [ "$choice" -le "${#keyrings[@]}" ] || die "Choice out of range."

    local selected="${keyrings[$((choice-1))]}"

    case "$selected" in
        *.zip)
            # Download zip to host temp file, extract keyring.enc entry from it.
            local tmp_zip
            tmp_zip="$(mktemp -t zerf-keyring-XXXXXX.zip)"
            docker run --rm \
                -v "$BACKUP_VOLUME:/backups:ro" \
                -e "SRC=$selected" \
                --entrypoint sh \
                "$HELPER_IMAGE" \
                -c 'cat "/backups/$SRC"' > "$tmp_zip" \
                || { rm -f "$tmp_zip"; die "Could not read $selected from backup volume."; }

            local out_name="${selected%.zip}.keyring.enc"
            unzip -p "$tmp_zip" keyring.enc > "$out_dir/$out_name" 2>/dev/null \
                || { rm -f "$tmp_zip" "$out_dir/$out_name"; die "Archive $selected does not contain a keyring.enc entry."; }
            rm -f "$tmp_zip"
            chmod 600 "$out_dir/$out_name" 2>/dev/null || true
            echo ""
            echo "Keyring extracted to: $out_dir/$out_name"
            ;;
        *.keyring.enc)
            # Legacy sidecar: copy it out of the volume directly.
            docker run --rm \
                -v "$BACKUP_VOLUME:/backups:ro" \
                -v "$out_dir:/out" \
                -e "SRC=$selected" \
                --entrypoint sh \
                "$HELPER_IMAGE" \
                -c 'cp "/backups/$SRC" "/out/$SRC" && chmod 0644 "/out/$SRC"' \
                || die "Could not copy $selected out of the backup volume."
            chmod 600 "$out_dir/$selected" 2>/dev/null || true
            echo ""
            echo "Keyring extracted to: $out_dir/$selected"
            ;;
    esac

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

# Dispatch keyring-extraction mode before loading .env (no DB credentials needed).
if [ "${1:-}" = "--keyring" ] || [ "${1:-}" = "--extract-keyring" ]; then
    shift
    extract_keyring "$@"
    exit 0
fi

# -- Load .env ----------------------------------------------------------------

[ -f "$ENV_FILE" ] || die ".env not found at $ENV_FILE - copy .env.example and fill in the values."

# `set -a` exports every variable that gets defined during the source.
set -a
# shellcheck disable=SC1090
. "$ENV_FILE"
set +a

# Required values - fail fast if any are missing.
: "${ZERF_DB_ENCRYPTION_KEY:?ZERF_DB_ENCRYPTION_KEY must be set in .env}"
: "${ZERF_POSTGRES_USER:?ZERF_POSTGRES_USER must be set in .env}"
: "${ZERF_POSTGRES_PASSWORD:?ZERF_POSTGRES_PASSWORD must be set in .env}"
: "${ZERF_POSTGRES_DB:?ZERF_POSTGRES_DB must be set in .env}"

# unzip is required to extract dumps from zip archives.
command -v unzip >/dev/null 2>&1 \
    || die "unzip is required for restore operations (install with: apt-get install unzip)"

# -- Cleanup on exit (success or failure) -------------------------------------

TMP_ZIP=""
META_TMP=""
META_TMP_DIR=""
cleanup() {
    [ -n "$TMP_ZIP" ]      && rm -f  "$TMP_ZIP"
    [ -n "$META_TMP" ]     && rm -f  "$META_TMP"
    [ -n "$META_TMP_DIR" ] && rm -rf "$META_TMP_DIR"
    # Best-effort: remove restore temp files from inside the postgres container.
    docker exec "$POSTGRES_CONTAINER" rm -f /tmp/zerf-restore.enc /tmp/zerf-restore.toc /tmp/zerf-restore.full.toc 2>/dev/null || true
}
trap cleanup EXIT

# -- Choose backup file -------------------------------------------------------

BACKUP_FILE="${1:-}"
BACKUP_CAME_FROM_VOLUME=0
SELECTED=""

if [ -z "$BACKUP_FILE" ]; then
    echo ""
    echo "Available backups (newest first):"
    echo ""

    docker volume inspect "$BACKUP_VOLUME" >/dev/null 2>&1 \
        || die "Docker volume $BACKUP_VOLUME not found. Is the stack running?"

    # List both new zip archives and legacy .dump.enc files so backups created
    # before this script version remain accessible.
    mapfile -t BACKUPS < <(
        docker run --rm \
            -v "$BACKUP_VOLUME:/backups:ro" \
            --entrypoint sh \
            "$HELPER_IMAGE" \
            -c 'ls -1t /backups/zerf-*.zip /backups/zerf-*.dump.enc 2>/dev/null' \
        | sed 's|/backups/||'
    )

    [ ${#BACKUPS[@]} -gt 0 ] || die "No backup archives found in $BACKUP_VOLUME."

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
    # BACKUP_FILE is a placeholder for display; actual data comes from the volume.
    BACKUP_FILE="/volume/${SELECTED}"
    BACKUP_CAME_FROM_VOLUME=1
else
    [ -f "$BACKUP_FILE" ] || die "File not found: $BACKUP_FILE"
    SELECTED="$(basename "$BACKUP_FILE")"
fi

# -- Copy encrypted dump into postgres container tmpfs ------------------------
#
# Both the new zip format and the legacy .dump.enc format are normalised to a
# single encrypted dump at /tmp/zerf-restore.enc inside the postgres container.
# All subsequent steps (TOC inspection, pre-drop, restore) use this file so the
# two code paths stay unified.
#
# For zip archives the zip is first downloaded to a host temp file ($TMP_ZIP)
# so that `unzip -p` can extract the dump.enc entry (unzip requires seekable
# input; docker stdin is not seekable).
echo ""
echo "Reading backup..."
case "$SELECTED" in
    *.zip)
        if [ "$BACKUP_CAME_FROM_VOLUME" = "1" ]; then
            # Stream the zip out of the Docker volume into a host temp file.
            # unzip -p requires seekable input; a streaming docker pipe is not
            # seekable, so we must land the zip on the local filesystem first.
            TMP_ZIP="$(mktemp -t zerf-restore-XXXXXX.zip)"
            docker run --rm \
                -v "$BACKUP_VOLUME:/backups:ro" \
                -e "SRC=$SELECTED" \
                --entrypoint sh \
                "$HELPER_IMAGE" \
                -c 'cat "/backups/$SRC"' > "$TMP_ZIP" \
                || die "Could not read $SELECTED from backup volume."
            unzip -p "$TMP_ZIP" dump.enc \
                | docker exec -i "$POSTGRES_CONTAINER" sh -c 'cat > /tmp/zerf-restore.enc' \
                || die "Could not extract dump.enc from $SELECTED. Check that the archive is intact."
        else
            # BACKUP_FILE is already a seekable local file; use it directly.
            # Do NOT assign TMP_ZIP here -- cleanup() would delete the user's file.
            unzip -p "$BACKUP_FILE" dump.enc \
                | docker exec -i "$POSTGRES_CONTAINER" sh -c 'cat > /tmp/zerf-restore.enc' \
                || die "Could not extract dump.enc from $SELECTED. Check that the archive is intact."
        fi
        ;;
    *.dump.enc)
        if [ "$BACKUP_CAME_FROM_VOLUME" = "1" ]; then
            docker run --rm \
                -v "$BACKUP_VOLUME:/backups:ro" \
                -e "SRC=$SELECTED" \
                --entrypoint sh \
                "$HELPER_IMAGE" \
                -c 'cat "/backups/$SRC"' \
            | docker exec -i "$POSTGRES_CONTAINER" sh -c 'cat > /tmp/zerf-restore.enc' \
                || die "Could not copy $SELECTED into the restore container."
        else
            docker exec -i "$POSTGRES_CONTAINER" sh -c 'cat > /tmp/zerf-restore.enc' \
                < "$BACKUP_FILE" \
                || die "Could not copy $BACKUP_FILE into the restore container."
        fi
        ;;
    *)
        die "Unsupported backup format: $SELECTED. Expected *.zip or *.dump.enc."
        ;;
esac

# -- Look up matching metadata (best-effort, no failure if absent) ------------

case "$SELECTED" in
    *.zip)
        # Metadata lives inside the zip archive itself.
        _zip_for_meta="${TMP_ZIP:-$BACKUP_FILE}"
        META_TMP_DIR="$(mktemp -d)"
        META_TMP="$META_TMP_DIR/metadata"
        unzip -p "$_zip_for_meta" metadata > "$META_TMP" 2>/dev/null || true
        if [ ! -s "$META_TMP" ]; then
            rm -rf "$META_TMP_DIR"; META_TMP_DIR=""; META_TMP=""
        fi
        ;;
    *.dump.enc)
        # Metadata may be a sibling .metadata file next to the .dump.enc.
        if [ "$BACKUP_CAME_FROM_VOLUME" = "1" ]; then
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
            if [ ! -s "$META_TMP" ]; then
                rm -rf "$META_TMP_DIR"; META_TMP_DIR=""; META_TMP=""
            fi
        else
            METADATA_FILE="${BACKUP_FILE%.dump.enc}.metadata"
            if [ -f "$METADATA_FILE" ] && [ -s "$METADATA_FILE" ]; then
                # Copy to a temp file so cleanup() never deletes the user's file.
                META_TMP_DIR="$(mktemp -d)"
                META_TMP="$META_TMP_DIR/metadata"
                cp "$METADATA_FILE" "$META_TMP"
            fi
        fi
        ;;
esac

# -- Show metadata and confirm ------------------------------------------------

echo ""
echo "Restore target"
echo "--------------"
echo "  File:     $SELECTED"
if [ -n "${META_TMP:-}" ] && [ -f "$META_TMP" ]; then
    BACKUP_TS=$(grep '^created_at_utc=' "$META_TMP" | cut -d= -f2- || true)
    BACKUP_COMMIT=$(grep '^ZERF_GIT_COMMIT=' "$META_TMP" | cut -d= -f2- || true)
    [ -n "$BACKUP_TS" ]     && info "Created:  $BACKUP_TS"
    [ -n "$BACKUP_COMMIT" ] && info "Commit:   $BACKUP_COMMIT"

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

# -- Verify postgres is running before we go further --------------------------

POSTGRES_STATUS="$(docker inspect -f '{{.State.Status}}' "$POSTGRES_CONTAINER" 2>/dev/null || echo missing)"
[ "$POSTGRES_STATUS" = "running" ] \
    || die "Container $POSTGRES_CONTAINER is not running (status: $POSTGRES_STATUS).  Start the stack first."

# -- Validate backup and prepare restore list ---------------------------------
#
# Build a restore list that EXCLUDES the pg_tde extension (and its COMMENT).
#
# Why: --clean drops every object in reverse order, including
# `DROP EXTENSION pg_tde`, which wipes the pg_tde principal-key configuration.
# The extension is then recreated empty and every `CREATE TABLE ... USING
# tde_heap` fails with "principal key not configured".  Filtering the extension
# out leaves the live pg_tde (and its key) untouched.  Verified end-to-end
# against the Percona pg_tde image.
echo ""
echo "Preparing restore list (preserving the pg_tde extension)..."
docker exec "$POSTGRES_CONTAINER" rm -f /tmp/zerf-restore.toc /tmp/zerf-restore.full.toc 2>/dev/null || true

# The encrypted dump is already at /tmp/zerf-restore.enc inside the container.
# Decrypt it in-container and pipe into pg_restore --list.
# Do NOT use -i: the command reads from the container tmpfs, not host stdin,
# so -i would allow docker exec's stdin-copy goroutine to consume bytes meant
# for later read() calls in this script (confirmation prompts).
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

# -- Stop the app and backup containers before touching the DB ----------------

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
# pg_restore then continues past the errors producing a mixed/partial state.
#
# Solution: drop everything in schema public that is NOT owned by an extension
# (pg_tde registers its objects with pg_depend deptype='e') before the restore
# runs.  The pg_tde extension itself is preserved.
echo "Pre-dropping all non-extension public-schema objects..."
predrop_exit=0
docker exec -i \
    -e PGPASSWORD="$ZERF_POSTGRES_PASSWORD" \
    -e PGOPTIONS='--statement_timeout=0 --idle_in_transaction_session_timeout=0' \
    "$POSTGRES_CONTAINER" \
    psql \
        --host 127.0.0.1 \
        --username "$ZERF_POSTGRES_USER" \
        --dbname "$ZERF_POSTGRES_DB" \
        -v ON_ERROR_STOP=1 \
        <<'EOSQL' || predrop_exit=$?
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
        ORDER BY c.relkind
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
          AND t.typtype IN ('e','c')
          AND t.oid NOT IN (SELECT objid FROM ext_owned)
    LOOP
        EXECUTE format('DROP TYPE IF EXISTS %s CASCADE', _obj.qname);
    END LOOP;
END;
$$;
EOSQL
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

# -- Restart the app ----------------------------------------------------------

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

#!/bin/bash
# Zerf Postgres entrypoint wrapper (pg_tde + Percona Distribution image).
#
# Encryption model:
#   - pg_tde encrypts tables and WAL at the PostgreSQL storage layer.
#     No FUSE, no SYS_ADMIN, no privileged container.
#   - The pg_tde principal key file lives in /var/lib/pg_tde_keyring/keyring.per,
#     a Docker-managed tmpfs that exists only in RAM.
#   - The persistent volume only contains the keyring encrypted with
#     ZERF_DB_ENCRYPTION_KEY (AES-256-CBC / PBKDF2).
#
# First container start (empty PGDATA, no keyring.enc):
#   1. Our setup runs (chown tmpfs, create /data if missing).
#   2. /entrypoint.sh runs initdb.
#   3. 00-pg-tde-setup.sql sets up the extension and creates the keyring.
#   4. 99-encrypt-keyring.sh wraps the keyring with ZERF_DB_ENCRYPTION_KEY
#      and writes it to /data/pg_tde_keyring.enc.
#
# Every subsequent start:
#   1. Our setup runs (chown tmpfs).
#   2. We decrypt /data/pg_tde_keyring.enc → in-memory tmpfs.
#   3. /entrypoint.sh detects PGDATA is already initialised and starts postgres
#      directly; pg_tde finds the keyring at its catalog-recorded path.
set -euo pipefail

# Any new file we create (decrypted keyring, etc.) must be private to the
# postgres user from the moment it appears, not just after a follow-up chmod.
# 077 → 0600 for files, 0700 for dirs — closes the TOCTOU window where a
# default-umask file would briefly be world-readable.
umask 077

KEYRING_DIR="/var/lib/pg_tde_keyring"
KEYRING_PLAIN="$KEYRING_DIR/keyring.per"
# /data is the parent of Percona's PGDATA (/data/db).  Storing the encrypted
# keyring there means it is co-located with PGDATA inside the same Docker
# volume but is not part of the data directory itself.
KEYRING_ENC="/data/pg_tde_keyring.enc"

# --- Validate -----------------------------------------------------------

if [ -z "${ZERF_DB_ENCRYPTION_KEY:-}" ]; then
    echo "ERROR: ZERF_DB_ENCRYPTION_KEY must be set in .env" \
         "(generate with: openssl rand -hex 32)" >&2
    exit 1
fi

# Resolve the postgres user dynamically — UID 26 on UBI (Percona),
# UID 999 on Debian (vanilla postgres).  Don't hardcode either.
PG_UID="$(id -u postgres)"
PG_GID="$(id -g postgres)"

# --- Prepare directories ------------------------------------------------

# The tmpfs at KEYRING_DIR is created by Docker (compose tmpfs:); ownership
# defaults to root.  Hand it to the postgres user so pg_tde can write
# during initdb and our decrypt step below can write on subsequent starts.
mkdir -p "$KEYRING_DIR"
chown "$PG_UID:$PG_GID" "$KEYRING_DIR"
chmod 700 "$KEYRING_DIR"

# /data is the Docker named volume root.  Make sure postgres can create
# files there (PGDATA itself is /data/db and is created by initdb).
mkdir -p /data
chown "$PG_UID:$PG_GID" /data

# --- Decrypt keyring on subsequent starts -------------------------------

if [ -f "$KEYRING_ENC" ]; then
    if ! openssl enc -d -aes-256-cbc -pbkdf2 -iter 100000 \
        -pass env:ZERF_DB_ENCRYPTION_KEY \
        -in  "$KEYRING_ENC" \
        -out "$KEYRING_PLAIN"; then
        # Don't leave a half-written file that would confuse pg_tde.
        rm -f "$KEYRING_PLAIN"
        echo "ERROR: failed to decrypt pg_tde keyring." \
             "Wrong ZERF_DB_ENCRYPTION_KEY or corrupted blob?" >&2
        exit 1
    fi
    chown "$PG_UID:$PG_GID" "$KEYRING_PLAIN"
    chmod 600 "$KEYRING_PLAIN"
    echo "Zerf: pg_tde keyring decrypted to in-memory tmpfs."
fi
# On first start KEYRING_ENC does not exist.  initdb will create the keyring
# via 00-pg-tde-setup.sql and persist its encrypted form via 99-encrypt-keyring.sh.

# --- Hand off to the upstream entrypoint --------------------------------
# Percona's image uses /entrypoint.sh (NOT docker-entrypoint.sh).
# It re-execs itself as the postgres user via gosu.
exec /entrypoint.sh "$@"

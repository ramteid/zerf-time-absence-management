#!/bin/bash
# Zerf: persist an encrypted copy of the pg_tde keyring.
#
# Runs once, at the end of first-start initdb, after 00-pg-tde-setup.sql has
# created the principal key at KEYRING_PLAIN (an in-memory tmpfs path).
# Encrypts the keyring with ZERF_DB_ENCRYPTION_KEY and writes the ciphertext
# to KEYRING_ENC inside the persistent data volume.
#
# On every subsequent container start the custom entrypoint decrypts
# KEYRING_ENC back to the same in-memory tmpfs path so pg_tde can load the
# key.  The host volume never contains the plaintext keyring.
set -euo pipefail

# Created files (encrypted keyring + sibling .tmp) must be private to the
# postgres user the instant they appear, not just after a follow-up chmod.
umask 077

KEYRING_PLAIN="/var/lib/pg_tde_keyring/keyring.per"
# /data is the Docker volume root; /data/db is PGDATA.  Keep the encrypted
# keyring next to PGDATA (not inside it) so it's clearly a separate artifact.
KEYRING_ENC="/data/pg_tde_keyring.enc"

if [ ! -f "$KEYRING_PLAIN" ]; then
    echo "ERROR: pg_tde keyring not found at $KEYRING_PLAIN" \
         "— 00-pg-tde-setup.sql may have failed." >&2
    exit 1
fi

if [ -f "$KEYRING_ENC" ]; then
    echo "Zerf: encrypted keyring already present, skipping."
    exit 0
fi

if [ -z "${ZERF_DB_ENCRYPTION_KEY:-}" ]; then
    echo "ERROR: ZERF_DB_ENCRYPTION_KEY is not set in the container env." >&2
    exit 1
fi

# Atomic write: encrypt to a sibling .tmp then rename, so a partial file
# never appears at KEYRING_ENC (which would brick subsequent starts).
# Remove any stale .tmp left over from a previous crashed initdb run before
# writing a new one — if we don't, a race between two parallel initdb
# invocations (unlikely but possible) could leave a partial file behind.
TMP="$KEYRING_ENC.tmp"
rm -f "$TMP"
openssl enc -aes-256-cbc -salt -pbkdf2 -iter 100000 \
    -pass env:ZERF_DB_ENCRYPTION_KEY \
    -in  "$KEYRING_PLAIN" \
    -out "$TMP"
chmod 600 "$TMP"
mv "$TMP" "$KEYRING_ENC"

echo "Zerf: pg_tde keyring encrypted and saved to $KEYRING_ENC."

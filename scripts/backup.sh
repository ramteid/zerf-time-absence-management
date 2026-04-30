#!/usr/bin/env bash
# KitaZeit SQLite backup helper.
#
# Usage:  bash scripts/backup.sh [DATA_DIR]
# Example cron (daily at 03:00):
#   0 3 * * *  /opt/kitazeit/scripts/backup.sh /opt/kitazeit/data
#
# Optional env:
#   BACKUP_RETENTION_DAYS   - delete older snapshots (default 30)
#   BACKUP_GPG_RECIPIENT    - if set, encrypt every snapshot with this GPG key
#                              (e.g. ops@example.com); the .db file is removed
#                              after a successful encryption.
set -euo pipefail
umask 077

DATA_DIR="${1:-./data}"
DB="$DATA_DIR/kitazeit.db"
RETENTION="${BACKUP_RETENTION_DAYS:-30}"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="$DATA_DIR/backups"
mkdir -p "$OUT_DIR"
chmod 700 "$OUT_DIR"

if [ ! -f "$DB" ]; then
  echo "Database not found: $DB" >&2
  exit 1
fi

OUT="$OUT_DIR/kitazeit-$TS.db"
sqlite3 "$DB" ".backup '$OUT'"
chmod 600 "$OUT"

if [ -n "${BACKUP_GPG_RECIPIENT:-}" ]; then
  gpg --batch --yes --trust-model always --output "$OUT.gpg" \
      --encrypt --recipient "$BACKUP_GPG_RECIPIENT" "$OUT"
  shred -u "$OUT" 2>/dev/null || rm -f "$OUT"
  chmod 600 "$OUT.gpg"
  echo "Encrypted backup written: $OUT.gpg"
else
  echo "Backup written: $OUT"
fi

# Retention.
find "$OUT_DIR" -type f \( -name 'kitazeit-*.db' -o -name 'kitazeit-*.db.gpg' \) \
    -mtime +"$RETENTION" -delete

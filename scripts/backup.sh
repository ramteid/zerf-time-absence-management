#!/usr/bin/env bash
# KitaZeit PostgreSQL backup helper.
#
# Usage:  bash scripts/backup.sh [OUTPUT_DIR]
# Example cron (daily at 03:00):
#   0 3 * * *  cd /opt/kitazeit && /opt/kitazeit/scripts/backup.sh /opt/kitazeit/backups
#
# Optional env:
#   BACKUP_RETENTION_DAYS   - delete older snapshots (default 30)
#   BACKUP_GPG_RECIPIENT    - if set, encrypt every snapshot with this GPG key
#                              (e.g. ops@example.com); the dump file is removed
#                              after a successful encryption.
#   KITAZEIT_POSTGRES_SERVICE - docker compose service name for PostgreSQL
set -euo pipefail
umask 077

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="${1:-$ROOT/backups}"
RETENTION="${BACKUP_RETENTION_DAYS:-30}"
SERVICE="${KITAZEIT_POSTGRES_SERVICE:-postgres}"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
mkdir -p "$OUT_DIR"
chmod 700 "$OUT_DIR"

if ! docker compose ps -q "$SERVICE" >/dev/null 2>&1; then
  echo "PostgreSQL service not found in docker compose: $SERVICE" >&2
  exit 1
fi

OUT="$OUT_DIR/kitazeit-$TS.dump"
docker compose exec -T "$SERVICE" sh -lc 'PGPASSWORD="$POSTGRES_PASSWORD" pg_dump --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" --format=custom --no-owner --no-privileges' > "$OUT"
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
find "$OUT_DIR" -type f \( -name 'kitazeit-*.dump' -o -name 'kitazeit-*.dump.gpg' \) \
    -mtime +"$RETENTION" -delete

# This container only ever speaks the PostgreSQL wire protocol (pg_dump/psql)
# against the separate `postgres` service -- it never runs a server itself --
# so the ~450 MB `postgres` server image is unnecessary. Alpine's versioned
# client-only package provides the same PostgreSQL 18 tools in a fraction of
# the size (image shrinks from ~460 MB to ~20 MB).
#
# If the server's PostgreSQL major version ever changes (see
# postgres.Dockerfile), bump `postgresql18-client` below to match, e.g.
# postgresql19-client -- pg_dump should be the same or newer major version
# than the server it connects to.
FROM alpine:3

# curl is needed for Nextcloud WebDAV uploads.
# ca-certificates ensures HTTPS connections are trusted.
# zip/unzip are needed to bundle backup files into a single archive and
# to inspect archive contents (e2e tests, restore verification).
# openssl (CLI) encrypts/decrypts backups; postgresql18-client provides
# pg_dump/psql.
# coreutils replaces BusyBox's date/stat/mktemp/etc. with the GNU versions
# backup.sh relies on -- BusyBox `date` cannot parse the ISO-8601
# (`date -u +%Y-%m-%dT%H:%M:%SZ`) timestamps the script stores in
# app_settings, which would silently break the backup-interval scheduling
# (every timestamp would fail to parse and read back as epoch 0, making the
# daemon think a backup is always overdue).
RUN apk add --no-cache \
        postgresql18-client curl ca-certificates zip unzip openssl coreutils

# Bake the backup script into the image so it is self-contained: the published
# image no longer depends on a host bind-mount of scripts/backup.sh. This build
# uses a repo-root context (see docker-compose / release.yml), so the COPY source
# is relative to the repository root. A matching .dockerignore exception keeps
# scripts/backup.sh in the build context.
COPY scripts/backup.sh /usr/local/bin/backup.sh
RUN chmod 0755 /usr/local/bin/backup.sh

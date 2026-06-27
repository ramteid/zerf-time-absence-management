FROM postgres:18

# curl is needed for Nextcloud WebDAV uploads.
# ca-certificates ensures HTTPS connections are trusted.
# openssl and psql are already present in the postgres base image.
RUN apt-get update && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Bake the backup script into the image so it is self-contained: the published
# image no longer depends on a host bind-mount of scripts/backup.sh. This build
# uses a repo-root context (see docker-compose / release.yml), so the COPY source
# is relative to the repository root. A matching .dockerignore exception keeps
# scripts/backup.sh in the build context.
COPY scripts/backup.sh /usr/local/bin/backup.sh
RUN chmod 0755 /usr/local/bin/backup.sh

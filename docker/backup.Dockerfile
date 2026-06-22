FROM postgres:18

# curl is needed for Nextcloud WebDAV uploads.
# ca-certificates ensures HTTPS connections are trusted.
# openssl and psql are already present in the postgres base image.
RUN apt-get update && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

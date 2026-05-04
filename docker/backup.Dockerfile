# syntax=docker/dockerfile:1
FROM postgres:16

RUN apt-get update && \
    apt-get install -y --no-install-recommends tini && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY scripts/backup.sh /app/scripts/backup.sh
RUN chmod 0555 /app/scripts/backup.sh

USER root
ENTRYPOINT ["/usr/bin/tini", "--", "/bin/sh", "/app/scripts/backup.sh", "/backups"]
# The Percona Distribution for PostgreSQL image is based on Red Hat UBI 9
# (not Debian).  Package manager is microdnf, the postgres user is uid 26,
# PGDATA defaults to /data/db, and the upstream entrypoint is /entrypoint.sh.
FROM percona/percona-distribution-postgresql:18

# We need root to install packages and copy our scripts into system paths.
# The base image switches to USER 26 (postgres) before its ENTRYPOINT;
# overriding it back to root means our entrypoint starts privileged, does
# its setup, then exec's into the upstream entrypoint, which itself drops
# privileges via gosu — the standard postgres-image pattern.
USER root

# openssl is required by our entrypoint and the keyring-encrypt init script
# to wrap/unwrap the pg_tde principal key.  The UBI minimal base does not
# ship the openssl CLI by default, so we add it explicitly.
RUN microdnf install -y openssl \
    && microdnf clean all

# Bake the pg_tde init scripts into the image so they run automatically on
# first-run initdb.  The official postgres entrypoint sorts init scripts
# lexicographically; 00-... runs before 99-...
COPY initdb/ /docker-entrypoint-initdb.d/

# Custom entrypoint wraps the upstream /entrypoint.sh: it decrypts the
# pg_tde keyring into an in-memory tmpfs before handing off.
COPY entrypoint-postgres.sh /usr/local/bin/entrypoint-postgres.sh

RUN chmod +x /usr/local/bin/entrypoint-postgres.sh \
              /docker-entrypoint-initdb.d/99-encrypt-keyring.sh

ENTRYPOINT ["/usr/local/bin/entrypoint-postgres.sh"]
CMD ["postgres"]

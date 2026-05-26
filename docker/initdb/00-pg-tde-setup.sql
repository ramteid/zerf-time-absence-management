-- Zerf: enable Transparent Data Encryption for the application database.
--
-- Runs once during first-start initdb (docker-entrypoint-initdb.d) against
-- the database named by POSTGRES_DB (which the operator chooses via
-- ZERF_POSTGRES_DB).  We must not hardcode that name here.
--
-- Prerequisites:
--   shared_preload_libraries=pg_tde   (set in docker-compose postgres command)

CREATE EXTENSION IF NOT EXISTS pg_tde;

-- File-based key provider, pointing at the in-memory tmpfs.  pg_tde writes
-- the principal key here; the surrounding entrypoint script later wraps it
-- with ZERF_DB_ENCRYPTION_KEY and persists the ciphertext to /data/.
SELECT pg_tde_add_database_key_provider_file(
    'file-vault',
    '/var/lib/pg_tde_keyring/keyring.per'
);

SELECT pg_tde_create_key_using_database_key_provider('zerf-principal-key', 'file-vault');
SELECT pg_tde_set_key_using_database_key_provider('zerf-principal-key', 'file-vault');

-- Encrypt the Write-Ahead Log so WAL segments on disk are also ciphertext.
-- ALTER SYSTEM persists this to postgresql.auto.conf; it takes effect when
-- postgres restarts (which the official entrypoint does after initdb).
ALTER SYSTEM SET pg_tde.wal_encrypt = on;

-- Make tde_heap the default access method for the *current* database — the
-- one POSTGRES_DB pointed at, whatever it is named.  All tables created by
-- sqlx migrations after this point will be transparently encrypted.
DO $do$
BEGIN
    EXECUTE format(
        'ALTER DATABASE %I SET default_table_access_method = %L',
        current_database(),
        'tde_heap'
    );
END
$do$;

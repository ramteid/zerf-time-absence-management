-- Apply lz4 TOAST compression to audit_log snapshot columns.
-- lz4 requires PostgreSQL compiled with --with-lz4 (Percona and most modern distros).
-- Plain postgres builds without lz4 raise feature_not_supported (0A000); the DO
-- block makes this migration a no-op on those hosts so CI tests keep passing.
DO $$
BEGIN
    ALTER TABLE audit_log ALTER COLUMN before_data SET COMPRESSION lz4;
    ALTER TABLE audit_log ALTER COLUMN after_data  SET COMPRESSION lz4;
EXCEPTION WHEN feature_not_supported THEN
    NULL; -- lz4 not compiled in, skip silently
END;
$$;

-- Apply lz4 TOAST compression to the audit_log snapshot columns.
-- Requires PostgreSQL 14+ compiled with --with-lz4 (standard on modern Debian/Percona builds).
ALTER TABLE audit_log ALTER COLUMN before_data SET COMPRESSION lz4;
ALTER TABLE audit_log ALTER COLUMN after_data  SET COMPRESSION lz4;

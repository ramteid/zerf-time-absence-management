-- Convert audit_log before_data/after_data from text to jsonb and enable lz4
-- TOAST compression. jsonb binary encoding plus lz4 reduces storage ~30-50 %
-- for rows over the TOAST threshold. NULL values survive the cast unchanged.
ALTER TABLE audit_log
    ALTER COLUMN before_data TYPE jsonb USING before_data::jsonb,
    ALTER COLUMN after_data  TYPE jsonb USING after_data::jsonb;

ALTER TABLE audit_log
    ALTER COLUMN before_data SET COMPRESSION lz4,
    ALTER COLUMN after_data  SET COMPRESSION lz4;

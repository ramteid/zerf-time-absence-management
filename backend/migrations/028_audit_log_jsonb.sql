-- Convert audit_log before_data/after_data from text to jsonb.
-- jsonb binary encoding reduces storage and enables native JSON operators.
-- NULL values survive the cast unchanged.
ALTER TABLE audit_log
    ALTER COLUMN before_data TYPE jsonb USING before_data::jsonb,
    ALTER COLUMN after_data  TYPE jsonb USING after_data::jsonb;

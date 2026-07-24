-- Recurring manual holidays: an admin can mark a manually-added holiday as
-- "repeats every year", optionally bounded by a last applicable year.
--
-- A recurring holiday is still exactly ONE physical row (holiday_date/year
-- keep meaning "the date it was first added for"). Later occurrences are
-- projected on read in the repository layer (same month/day, later year),
-- not materialized as additional rows. recurring defaults to FALSE, so every
-- existing row (including all is_auto Nager.Date imports) is unaffected;
-- recurrence is a manual-holiday-only feature.
ALTER TABLE holidays ADD COLUMN IF NOT EXISTS recurring BOOLEAN NOT NULL DEFAULT FALSE;

-- Last year (inclusive) the recurrence still applies. NULL = no end.
ALTER TABLE holidays ADD COLUMN IF NOT EXISTS recurrence_end_year INTEGER;

-- An end year is only meaningful on a recurring holiday.
ALTER TABLE holidays DROP CONSTRAINT IF EXISTS holidays_recurrence_end_requires_recurring;
ALTER TABLE holidays ADD CONSTRAINT holidays_recurrence_end_requires_recurring
  CHECK (recurrence_end_year IS NULL OR recurring = TRUE);

-- The end year cannot be before the defining occurrence's own year.
ALTER TABLE holidays DROP CONSTRAINT IF EXISTS holidays_recurrence_end_not_before_year;
ALTER TABLE holidays ADD CONSTRAINT holidays_recurrence_end_not_before_year
  CHECK (recurrence_end_year IS NULL OR recurrence_end_year >= year);

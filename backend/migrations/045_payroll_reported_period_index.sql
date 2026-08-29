-- Index for reading back what a given payroll report carried.
--
-- WHY: migration 044's partial index only covers the "not yet reported" side
-- (`payroll_reported_period IS NULL`). The dashboard's payroll card asks the
-- opposite question for a month that has already gone out — "which days did
-- *that* report carry" — on every page load, and without an index that is a
-- full scan of the time entries table.
CREATE INDEX IF NOT EXISTS idx_te_payroll_reported_period
  ON time_entries(payroll_reported_period, entry_date)
  WHERE payroll_reported_period IS NOT NULL;

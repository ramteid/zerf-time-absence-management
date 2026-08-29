-- Records which payroll report already accounted for a time entry, so hours
-- booked after a month's report went out can still be paid.
--
-- WHY: an assistant is paid by the hour. When one of them books a day of the
-- reported month only after the tax office already received that month's
-- report, those hours are in no document at all — the month is closed, and the
-- next month's report covers a different window. The column marks every entry
-- the scheduled report accounted for; an approved entry from a closed month
-- that carries no mark is therefore provably a late booking, and the next
-- report prints it in its own section under its real date.
--
-- The value is the reported period ("YYYY-MM") rather than a boolean, so the
-- report a given entry went out with stays visible for support questions.
--
-- The backfill runs only when the column is created. Everything that exists at
-- that moment counts as accounted for: which of those rows a past report
-- actually contained can no longer be reconstructed, and assuming "not yet
-- reported" would dump years of history into the next report. Re-running the
-- UPDATE unconditionally would do exactly that to genuine late bookings, hence
-- the guard rather than a bare idempotent UPDATE.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'time_entries'
      AND column_name = 'payroll_reported_period'
  ) THEN
    ALTER TABLE time_entries ADD COLUMN payroll_reported_period TEXT;
    UPDATE time_entries SET payroll_reported_period = to_char(entry_date, 'YYYY-MM');
  END IF;
END $$;

-- Partial index: every lookup asks for the unmarked rows only, and after the
-- backfill those are a handful out of the whole table.
CREATE INDEX IF NOT EXISTS idx_te_payroll_unreported
  ON time_entries(entry_date)
  WHERE payroll_reported_period IS NULL;

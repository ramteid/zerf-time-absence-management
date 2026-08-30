-- Records which payroll report already printed an absence, so a sick note
-- filed for an already-reported month can still reach the tax office.
--
-- WHY: the entries side of this problem was solved by migration 044, but
-- absences are the other half of the same document and were left exposed.
-- `AbsenceCategory::is_payroll_relevant` is `auto_approve_past OR unpaid`, so
-- every sick-like payroll category approves a *past* absence on the spot. Such
-- an absence therefore never sits in `requested`, never trips the
-- `PendingAbsences::PayrollRelevant` gate, and never holds a report back — it
-- simply appears after the month it belongs to has already been filed. Without
-- a marker those days are in no document at all, and continued pay is never
-- claimed for them.
--
-- The value is the first period whose report printed any part of the absence.
-- "First", not "every", is deliberate and is what makes a single column enough
-- for an absence spanning a month boundary: the marker only ever gates the
-- catch-up path, never the normal per-month clamping, so July's report still
-- prints the July part and August's still prints the August part. NULL
-- therefore means precisely "no report has ever shown any of this".
--
-- The backfill runs only when the column is created, for the same reason as
-- migration 044: which absences a past report contained can no longer be
-- reconstructed, and assuming "never reported" would flood the next report
-- with the whole history. Re-running the UPDATE unconditionally would do
-- exactly that to genuine late filings, hence the guard.
DO $$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM information_schema.columns
    WHERE table_schema = current_schema()
      AND table_name = 'absences'
      AND column_name = 'payroll_reported_period'
  ) THEN
    ALTER TABLE absences ADD COLUMN payroll_reported_period TEXT;
    -- Keyed on the end date: an absence belongs to the last month it touches,
    -- which is the latest report that could have printed part of it.
    UPDATE absences SET payroll_reported_period = to_char(end_date, 'YYYY-MM');
  END IF;
END $$;

-- Partial index: every lookup asks for the unmarked rows only, and after the
-- backfill those are a handful out of the whole table.
CREATE INDEX IF NOT EXISTS idx_absences_payroll_unreported
  ON absences(end_date)
  WHERE payroll_reported_period IS NULL;

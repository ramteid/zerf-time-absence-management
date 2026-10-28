-- Preserve the history that was already approved before the first report.
-- Later creations and approvals stay unmarked and can be reported under their
-- actual dates, even when those dates predate the first reported month.
DO $$
DECLARE
    first_period TEXT;
    first_day DATE;
    cutoff TIMESTAMPTZ;
BEGIN
    SELECT value, updated_at INTO first_period, cutoff FROM app_settings
    WHERE key = 'payroll_report_first_period' AND value ~ '^[0-9]{4}-(0[1-9]|1[0-2])$';
    IF first_period IS NULL THEN
        RETURN;
    END IF;

    first_day := to_date(first_period || '-01', 'YYYY-MM-DD');
    UPDATE time_entries SET payroll_reported_period = '0000-00'
    WHERE payroll_reported_period IS NULL AND status = 'approved'
      AND entry_date < first_day AND created_at <= cutoff
      AND (reviewed_at IS NULL OR reviewed_at <= cutoff);

    UPDATE absences SET payroll_reported_period = '0000-00'
    WHERE payroll_reported_period IS NULL AND status IN ('approved', 'cancellation_pending')
      AND start_date < first_day AND created_at <= cutoff
      AND (reviewed_at IS NULL OR reviewed_at <= cutoff);
END $$;

-- A cancellation of a previously reported absence needs payroll review.
-- The earlier document may predate the detailed report snapshot, so the
-- notice carries the original range instead of guessing a reversal amount.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema() AND table_name = 'absences'
          AND column_name = 'payroll_cancellation_pending'
    ) THEN
        ALTER TABLE absences
            ADD COLUMN payroll_cancellation_pending BOOLEAN NOT NULL DEFAULT FALSE;

        -- Cancellations since the original absence marker was installed may
        -- already need review. The notice asks payroll to check the old report.
        UPDATE absences
        SET payroll_cancellation_pending = TRUE
        WHERE status = 'cancelled' AND payroll_reported_period IS NOT NULL
          AND reviewed_at >= (
              SELECT installed_on FROM _sqlx_migrations WHERE version = 46
          );
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_absence_payroll_cancellation_pending
    ON absences (user_id, start_date, id)
    WHERE payroll_cancellation_pending = TRUE;

ALTER TABLE payroll_reported_content
    DROP CONSTRAINT IF EXISTS payroll_reported_content_kind_check;
ALTER TABLE payroll_reported_content
    ADD CONSTRAINT payroll_reported_content_kind_check
    CHECK (kind IN ('absence', 'hours', 'late_absence', 'late_hours', 'absence_cancellation', 'absence_review'));

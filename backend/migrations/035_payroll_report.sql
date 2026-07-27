-- Monthly payroll report email to the tax office / payroll accountant.
--
-- The report is a single PDF per month covering the whole company (absence
-- days per selected category, plus working days and hours), emailed to one
-- configured recipient. It follows the same schedule as the Nextcloud
-- timesheet export: the previous month is queued once the configured day of
-- month is reached and stays queued until every included employee's month is
-- final, so late submissions are caught up automatically.
--
-- One row per period (not per user): the report aggregates all employees into
-- a single document. Rows are deleted after the email was accepted by the
-- SMTP server.
CREATE TABLE IF NOT EXISTS payroll_report_queue (
    period     CHAR(7) PRIMARY KEY,   -- "YYYY-MM"
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Defaults for the new settings. Sick and unpaid absences are pre-selected
-- because both directly change what the payroll accountant has to file:
-- sick days drive health-insurance reimbursement, unpaid days reduce the
-- salary payout. Admins can change the selection in the Admin UI.
INSERT INTO app_settings (key, value)
VALUES
    ('payroll_report_day_of_month', '5'),
    ('payroll_report_absence_categories', 'sick,unpaid')
ON CONFLICT (key) DO NOTHING;

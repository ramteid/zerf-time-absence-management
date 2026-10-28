-- A cancellation notice asks payroll to check an absence an earlier report
-- contained. Migration 052 raised it for every cancelled absence with a
-- marker, but migration 046 had marked every absence that existed when it ran:
-- planned ones, other categories and assistants' absences, none of which a
-- report ever printed. Keep a notice only for an absence a report could have
-- printed: a payroll-relevant category, an owner who is not an assistant, and
-- a month whose report has gone out. This only clears notices, so re-running
-- it changes nothing.
UPDATE absences SET payroll_cancellation_pending = FALSE
WHERE payroll_cancellation_pending = TRUE
  AND NOT (
      absences.payroll_reported_period IS NOT NULL
      AND absences.payroll_reported_period <> '0000-00'
      AND EXISTS (SELECT 1 FROM absence_categories c
                  WHERE c.id = absences.category_id
                  AND (c.auto_approve_past = TRUE OR c.unpaid = TRUE))
      AND EXISTS (SELECT 1 FROM users u
                  WHERE u.id = absences.user_id AND lower(btrim(u.role)) <> 'assistant')
      AND EXISTS (SELECT 1 FROM app_settings queued,
                       generate_series(date_trunc('month', absences.start_date),
                                       date_trunc('month', absences.end_date),
                                       interval '1 month') AS month(start)
                  WHERE queued.key = 'payroll_report_queue_period'
                  AND to_char(month.start, 'YYYY-MM') <= queued.value
                  AND NOT EXISTS (SELECT 1 FROM payroll_report_queue q
                                  WHERE q.period = to_char(month.start, 'YYYY-MM')))
  );

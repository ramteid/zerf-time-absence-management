-- The payroll report's absence-category selection is no longer an admin
-- setting: the report now automatically includes categories that are
-- sick-like (auto_approve_past) or cost neither vacation nor flextime
-- (cost_type = 'none') — see AbsenceCategory::is_payroll_relevant in the
-- application. The stored slug list is therefore dead configuration; remove
-- it so it doesn't linger as an orphaned, unread setting.
DELETE FROM app_settings WHERE key = 'payroll_report_absence_categories';

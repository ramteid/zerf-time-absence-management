-- Per-user base annual leave entitlement (days/year), used whenever no
-- explicit user_annual_leave override exists for a given year. Backfilled
-- from the org-wide default so existing users keep their current effective
-- entitlement.
ALTER TABLE users
    ADD COLUMN annual_leave_days BIGINT NOT NULL DEFAULT 30
        CHECK (annual_leave_days >= 0 AND annual_leave_days <= 366);

UPDATE users
SET annual_leave_days = COALESCE(
    (SELECT value::BIGINT FROM app_settings WHERE key = 'default_annual_leave_days'),
    30
);

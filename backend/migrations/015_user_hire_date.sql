-- Add an optional hire_date column to users.
--
-- start_date anchors several things at once: the earliest date a user may
-- record time entries/absences, the flextime starting-balance injection point,
-- and (until now) the annual-leave proration calculation. That conflation
-- breaks down when introducing Zerf to an existing team: an employee who has
-- worked the full year but only starts using Zerf mid-year would otherwise
-- have their leave entitlement wrongly prorated from their Zerf start date.
--
-- hire_date lets admins record the employee's actual employment start
-- separately. When NULL, leave proration falls back to start_date — the
-- existing behavior is preserved for the normal case where employment and
-- Zerf usage begin on the same day.
ALTER TABLE users ADD COLUMN IF NOT EXISTS hire_date DATE;

COMMENT ON COLUMN users.hire_date IS
  'Optional employment start date used to anchor annual-leave proration. '
  'Falls back to start_date when NULL.';

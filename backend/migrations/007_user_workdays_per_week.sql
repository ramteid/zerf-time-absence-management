-- Per-user configurable workdays per week (flexible work schedules).
-- 
-- Field: workdays_per_week (SMALLINT, 1-7)
-- Default: 5 (Mon-Fri standard week)
--
-- Semantics:
--   * Contract workdays are the first N days of the ISO week (0=Mon, 1=Tue, ...)
--   * Example: workdays_per_week=5 → Mon-Fri (days 0-4)
--   * Example: workdays_per_week=4 → Mon-Thu (days 0-3)
--   * Example: workdays_per_week=6 → Mon-Sat (days 0-5)
--
-- Used for:
--   * Vacation/absence counting: workdays, not calendar days
--   * Daily target hour calculations: weekly_hours / workdays_per_week
--   * Submission status: validates all contract workdays are covered by entries or absences
--   * Month/overtime/flextime reports: excludes non-contract days from targets
--
-- Business Rules:
--   * Constraint: workdays_per_week >= 1 AND workdays_per_week <= 7
--   * Cannot be NULL; always has a meaningful value
--   * Changing a user's workdays_per_week affects all future calculations retroactively

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS workdays_per_week SMALLINT NOT NULL DEFAULT 5
    CHECK (workdays_per_week >= 1 AND workdays_per_week <= 7);

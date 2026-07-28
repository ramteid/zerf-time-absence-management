-- Whether an approved absence in this category actually reduces the
-- employee's pay. Only meaningful for cost_type = 'none': vacation and
-- flextime categories are always "paid" through their own balance mechanics,
-- so combining either with "unpaid" would be contradictory.
--
-- WHY: the monthly payroll report needs to know which categories genuinely
-- reduce salary (see AbsenceCategory::is_payroll_relevant). cost_type='none'
-- alone is not a reliable proxy for that: it also covers paid special leave,
-- paid training counted as working time, and legally mandated paid
-- educational leave (Bildungsurlaub in Germany) — none of which payroll
-- should ever be told is unpaid. This column captures the pay distinction
-- explicitly instead of inferring it from the balance-cost field, which was
-- never meant to encode it (see the pre-existing help_cost_type_none copy:
-- "Whether the day is paid is decided in payroll, not in Zerf").
ALTER TABLE absence_categories ADD COLUMN IF NOT EXISTS unpaid BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE absence_categories DROP CONSTRAINT IF EXISTS abs_cat_unpaid_requires_none_cost;
ALTER TABLE absence_categories ADD CONSTRAINT abs_cat_unpaid_requires_none_cost
    CHECK (NOT unpaid OR cost_type = 'none');

-- Seed data (migration 017) ships a category literally slugged "unpaid" for
-- exactly this purpose; mark it unpaid so its pre-existing, hand-picked spot
-- in the payroll report's category list isn't silently dropped by this
-- migration. Every other category defaults to FALSE — admins review and
-- opt in the rest deliberately on the Categories page.
UPDATE absence_categories SET unpaid = TRUE WHERE slug = 'unpaid' AND NOT unpaid;

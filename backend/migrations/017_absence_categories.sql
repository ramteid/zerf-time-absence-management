-- Configurable absence categories.
--
-- Before this migration, absence kinds were a hardcoded slug set
-- ('vacation','sick','training','special_leave','unpaid','general_absence',
-- 'flextime_reduction') stored as `absences.kind` and validated by a CHECK
-- constraint. Three orthogonal behaviors were tangled into those slugs:
--   * `vacation` deducts from annual leave entitlement
--   * `sick` auto-approves when in the past, allows backdating up to 30 days,
--     and is permitted to coexist with logged time on the same day
--   * `flextime_reduction` keeps the daily work target (drives flextime down)
-- All other kinds were behavior-identical "free days off".
--
-- This migration extracts those properties into a configurable table so
-- admins can rename, recolor, reorder, deactivate, or add absence categories
-- without code changes. The behavior is now driven by explicit booleans on
-- each category, not by a magic slug.

CREATE TABLE IF NOT EXISTS absence_categories (
    id BIGSERIAL PRIMARY KEY,
    -- Stable identifier kept across renames; used for i18n key lookup and
    -- migration mapping. Defaults are seeded with the legacy kind names so
    -- existing absences can be backfilled deterministically.
    slug TEXT NOT NULL UNIQUE CHECK (slug ~ '^[a-z][a-z0-9_]*$'),
    name TEXT NOT NULL,
    color TEXT NOT NULL CHECK (color ~ '^#[0-9A-Fa-f]{6}$'),
    sort_order BIGINT NOT NULL DEFAULT 0,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    -- Deducts from annual vacation balance (carryover/expiry policy applies).
    counts_as_vacation BOOLEAN NOT NULL DEFAULT FALSE,
    -- When TRUE the day still carries its normal work target -- the user
    -- effectively "pays" by reducing their flextime balance instead of being
    -- granted a free day off. When FALSE (the common case) the day's target
    -- drops to zero and flextime is untouched.
    keeps_work_target BOOLEAN NOT NULL DEFAULT FALSE,
    -- "Sick-like" behavior:
    --   * absences whose start_date <= today are auto-approved on creation
    --   * backdating up to 30 days is permitted (otherwise: any date >= user.start_date)
    --   * an absence in this category may coexist with logged time on the same day
    auto_approve_past BOOLEAN NOT NULL DEFAULT FALSE,
    -- Two flags that "pay for the absence" must not both be set: a category
    -- can deduct from vacation OR reduce flextime, never both at once.
    CONSTRAINT abs_cat_only_one_cost
        CHECK (NOT (counts_as_vacation AND keeps_work_target))
);

CREATE INDEX IF NOT EXISTS idx_absence_categories_active_order
    ON absence_categories(active DESC, sort_order, name);

-- Seed the seven legacy kinds idempotently. New installations get all defaults;
-- existing installations (already containing the rows from a prior run) skip.
INSERT INTO absence_categories
    (slug, name, color, sort_order, active, counts_as_vacation, keeps_work_target, auto_approve_past)
VALUES
    ('vacation',           'Vacation',           '#3b82f6', 1, TRUE, TRUE,  FALSE, FALSE),
    ('sick',               'Sick',               '#ef4444', 2, TRUE, FALSE, FALSE, TRUE),
    ('training',           'Training',           '#a855f7', 3, TRUE, FALSE, FALSE, FALSE),
    ('special_leave',      'Special leave',      '#0ea5e9', 4, TRUE, FALSE, FALSE, FALSE),
    ('unpaid',             'Unpaid',             '#64748b', 5, TRUE, FALSE, FALSE, FALSE),
    ('general_absence',    'General absence',    '#f59e0b', 6, TRUE, FALSE, FALSE, FALSE),
    ('flextime_reduction', 'Flextime Reduction', '#6D4C41', 7, TRUE, FALSE, TRUE,  FALSE)
ON CONFLICT (slug) DO NOTHING;

-- Backfill: add the FK column, map every existing absence to a category id by
-- its legacy slug, then enforce NOT NULL.
ALTER TABLE absences ADD COLUMN IF NOT EXISTS category_id BIGINT
    REFERENCES absence_categories(id);

UPDATE absences
SET category_id = absence_categories.id
FROM absence_categories
WHERE absences.category_id IS NULL
  AND absence_categories.slug = absences.kind;

-- Any absence row still NULL would be data corruption (unknown kind); fail
-- the migration loudly rather than silently strand them.
DO $$
DECLARE
    orphan_count BIGINT;
BEGIN
    SELECT COUNT(*) INTO orphan_count FROM absences WHERE category_id IS NULL;
    IF orphan_count > 0 THEN
        RAISE EXCEPTION
            'Migration 017: % absence rows have an unknown kind and could not be mapped',
            orphan_count;
    END IF;
END $$;

ALTER TABLE absences ALTER COLUMN category_id SET NOT NULL;

-- Drop the legacy slug column + its CHECK constraint. Behavior now derives
-- entirely from absence_categories flags joined via category_id.
ALTER TABLE absences DROP CONSTRAINT IF EXISTS absences_kind_check;
ALTER TABLE absences DROP COLUMN IF EXISTS kind;

CREATE INDEX IF NOT EXISTS idx_abs_category ON absences(category_id);

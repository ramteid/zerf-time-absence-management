-- Migration 019: collapse the two mutex booleans
-- (counts_as_vacation, keeps_work_target) into a single 3-state column
-- cost_type ∈ {'none', 'vacation', 'flextime'}.
--
-- WHY:
-- The two booleans were never logically independent — the CHECK constraint
-- `abs_cat_only_one_cost` enforced `NOT (counts_as_vacation AND
-- keeps_work_target)`, so only three of the four boolean combinations were
-- ever valid. The XOR invariant had to be re-asserted at three levels
-- (DB CHECK, service-layer validation, and the Bug-7/Bug-9 mutation guards),
-- each of which had to keep the two flags in sync. A single 3-state field
-- makes the invariant impossible to violate by construction, simplifies the
-- admin dialog to one radio group instead of two coupled checkboxes, and
-- shrinks the Bug-7/Bug-9 guards to a plain comparison.
--
-- ROLLBACK SAFETY:
-- The backfill is deterministic from the existing boolean values, so this
-- migration is safe to apply in production. We assert that no row ends up
-- with NULL cost_type before dropping the booleans, mirroring the
-- atomic-backfill pattern from migration 017. Reverse migration would also
-- be deterministic ('vacation' → counts_as_vacation=true, 'flextime' →
-- keeps_work_target=true, 'none' → both false).

-- 1. Add the new column nullable so the backfill can populate it.
ALTER TABLE absence_categories ADD COLUMN cost_type TEXT;

-- 2. Backfill from the existing booleans. The CASE order matters only when
--    both flags happen to be TRUE (impossible under the existing CHECK
--    constraint, but we make the precedence explicit and self-documenting).
UPDATE absence_categories SET cost_type = CASE
    WHEN counts_as_vacation THEN 'vacation'
    WHEN keeps_work_target  THEN 'flextime'
    ELSE 'none'
END;

-- 3. Sanity check: refuse to drop the booleans if any row failed to map.
--    Mirrors migration 017's RAISE EXCEPTION pattern for absence backfill.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM absence_categories WHERE cost_type IS NULL) THEN
        RAISE EXCEPTION 'Migration 019 failed: at least one absence_categories row has NULL cost_type after backfill';
    END IF;
END $$;

-- 4. Tighten the new column.
ALTER TABLE absence_categories ALTER COLUMN cost_type SET NOT NULL;
ALTER TABLE absence_categories ADD CONSTRAINT abs_cat_cost_type
    CHECK (cost_type IN ('none', 'vacation', 'flextime'));

-- 5. Drop the legacy XOR constraint and the two boolean columns. With
--    cost_type as a single 3-state field the mutex is satisfied by
--    construction; the CHECK and the column type together carry the entire
--    invariant the boolean pair used to enforce.
ALTER TABLE absence_categories DROP CONSTRAINT IF EXISTS abs_cat_only_one_cost;
ALTER TABLE absence_categories DROP COLUMN counts_as_vacation;
ALTER TABLE absence_categories DROP COLUMN keeps_work_target;

-- 6. Drop `team_visible`. This column was added in migration 018 to let
--    admins opt benign categories into team-wide calendar visibility, but
--    the calendar's actual visibility model is governed entirely by the
--    requester's scope (`calendar_scope_user_ids`):
--      - admins see every absence,
--      - leads see their direct reports' absences,
--      - employees see only their own absences.
--    Under that strict scope rule the per-category flag is redundant —
--    it could only affect employees viewing other employees' entries, a
--    case the scope query never exposes in the first place. Keeping the
--    column added a per-category UX toggle that didn't actually toggle
--    anything. Remove it so the data model matches the actual visibility
--    model.
ALTER TABLE absence_categories DROP COLUMN team_visible;

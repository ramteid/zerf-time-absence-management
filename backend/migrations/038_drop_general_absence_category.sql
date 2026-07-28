-- Remove the "General absence" category.
--
-- WHY: it is behaviourally identical to "Special leave" — both are
-- (cost_type='none', auto_approve_past=false, unpaid=false), so Zerf treats
-- them the same in every calculation. Two interchangeable catch-all buckets
-- for "some other excused, paid absence" only make the request dropdown
-- harder to choose from; one is enough.
--
-- SAFETY: the delete is skipped whenever the category still has absences
-- attached. Installations that actually use it keep it (and its rows stay
-- valid); only installations where it was never used lose it. The
-- user_absence_category_access rows cascade (migration 025).
DELETE FROM absence_categories
WHERE slug = 'general_absence'
  AND NOT EXISTS (
      SELECT 1 FROM absences WHERE absences.category_id = absence_categories.id
  );

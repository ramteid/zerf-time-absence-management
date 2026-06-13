-- Migration 018: add team_visible flag to absence_categories
--
-- `team_visible` controls whether non-lead team members can see the real
-- absence kind in the shared calendar. This decouples privacy from the
-- vacation-cost flag (`counts_as_vacation`) so that benign non-vacation
-- categories (training, flextime_reduction, etc.) are visible to teammates
-- while health-related categories (sick leave, GDPR Art. 9) stay masked.
--
-- Default FALSE keeps all existing categories private until explicitly enabled,
-- preventing data leaks on upgrade.

ALTER TABLE absence_categories ADD COLUMN team_visible BOOLEAN NOT NULL DEFAULT FALSE;

-- Enable visibility for all seeded categories except sick leave.
-- Sick leave is GDPR Art. 9 health data and must remain private.
UPDATE absence_categories SET team_visible = TRUE
WHERE slug IN ('vacation', 'training', 'special_leave', 'unpaid', 'general_absence', 'flextime_reduction');

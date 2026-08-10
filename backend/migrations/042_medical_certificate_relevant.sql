-- Marks absence categories whose absences count toward the "AU" (medical
-- certificate / Arbeitsunfähigkeitsbescheinigung) requirement calculation.
--
-- WHY: German employers must receive a doctor's certificate once a
-- continuous illness reaches a configurable number of calendar days (see
-- `medical_certificate_threshold_days` in app_settings). Whether that
-- threshold is reached is computed dynamically from approved absences in
-- flagged categories (services::medical_certificate) — this column only
-- decides which categories count as "sick-like" for that calculation. Kept
-- independent from `auto_approve_past`: an org may want a category to behave
-- like sick leave without counting toward the AU threshold, or vice versa.
ALTER TABLE absence_categories ADD COLUMN IF NOT EXISTS medical_certificate_relevant BOOLEAN NOT NULL DEFAULT FALSE;

-- Seed data (migration 017) ships a category slugged "sick" for exactly this
-- purpose; mark it relevant so the threshold calculation covers it out of
-- the box. Every other category defaults to FALSE — admins opt in deliberately
-- on the Categories page.
UPDATE absence_categories SET medical_certificate_relevant = TRUE WHERE slug = 'sick' AND NOT medical_certificate_relevant;

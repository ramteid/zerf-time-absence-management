ALTER TABLE categories
    ADD COLUMN IF NOT EXISTS counts_as_work BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE absences
    DROP CONSTRAINT IF EXISTS absences_kind_check;

ALTER TABLE absences
    ADD CONSTRAINT absences_kind_check
    CHECK (kind IN (
        'vacation',
        'sick',
        'training',
        'special_leave',
        'unpaid',
        'general_absence',
        'flextime_reduction'
    ));

INSERT INTO categories (name, description, color, sort_order, counts_as_work)
SELECT
    'Flextime Reduction',
    'Blocks time without crediting worked hours.',
    '#6D4C41',
    7,
    FALSE
WHERE EXISTS (SELECT 1 FROM categories)
  AND NOT EXISTS (SELECT 1 FROM categories WHERE name = 'Flextime Reduction');

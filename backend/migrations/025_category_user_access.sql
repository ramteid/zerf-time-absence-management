-- Per-employee enable/disable for time categories and absence categories.
-- Existing categories/absences are unaffected by this change; only future
-- category selection in time entries / absence requests is restricted.

CREATE TABLE IF NOT EXISTS user_category_access (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category_id BIGINT NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, category_id)
);

CREATE TABLE IF NOT EXISTS user_absence_category_access (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    category_id BIGINT NOT NULL REFERENCES absence_categories(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, category_id)
);

-- Backfill: every existing user gets every existing category enabled, so
-- this migration does not change behavior for anyone until an admin
-- explicitly disables a category for a specific employee.
INSERT INTO user_category_access (user_id, category_id)
SELECT u.id, c.id FROM users u CROSS JOIN categories c
ON CONFLICT DO NOTHING;

INSERT INTO user_absence_category_access (user_id, category_id)
SELECT u.id, c.id FROM users u CROSS JOIN absence_categories c
ON CONFLICT DO NOTHING;

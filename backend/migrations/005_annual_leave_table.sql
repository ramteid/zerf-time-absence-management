-- Replaces the two-layer vacation system (users.annual_leave_days +
-- user_annual_leave_overrides) with a single unified table.
--
-- Strategy:
--   1. Create new table user_annual_leave(user_id, year, days).
--   2. Seed current + next year from users.annual_leave_days for every user.
--   3. Upsert existing override rows (they win over the base value).
--   4. Drop the superseded table and column.

CREATE TABLE IF NOT EXISTS user_annual_leave (
    user_id BIGINT  NOT NULL REFERENCES users(id),
    year    INTEGER NOT NULL CHECK (year >= 2000 AND year <= 2100),
    days    BIGINT  NOT NULL CHECK (days >= 0 AND days <= 366),
    PRIMARY KEY (user_id, year)
);

INSERT INTO user_annual_leave (user_id, year, days)
SELECT id, EXTRACT(YEAR FROM CURRENT_DATE)::INTEGER, annual_leave_days
FROM users
ON CONFLICT DO NOTHING;

INSERT INTO user_annual_leave (user_id, year, days)
SELECT id, EXTRACT(YEAR FROM CURRENT_DATE)::INTEGER + 1, annual_leave_days
FROM users
ON CONFLICT DO NOTHING;

INSERT INTO user_annual_leave (user_id, year, days)
SELECT user_id, year, days
FROM user_annual_leave_overrides
ON CONFLICT (user_id, year) DO UPDATE SET days = EXCLUDED.days;

DROP TABLE IF EXISTS user_annual_leave_overrides;

ALTER TABLE users DROP COLUMN IF EXISTS annual_leave_days;

-- Per-user per-year vacation day overrides.
-- When a row exists for (user_id, year), it takes precedence over
-- users.annual_leave_days for that year's entitlement calculation.
CREATE TABLE IF NOT EXISTS user_annual_leave_overrides (
  user_id BIGINT NOT NULL REFERENCES users(id),
  year INTEGER NOT NULL CHECK (year >= 2000 AND year <= 2100),
  days BIGINT NOT NULL CHECK (days >= 0 AND days <= 366),
  PRIMARY KEY (user_id, year)
);

-- Default carryover expiry date setting (MM-DD format, e.g. "03-31").
INSERT INTO app_settings(key, value)
VALUES ('carryover_expiry_date', '03-31')
ON CONFLICT (key) DO NOTHING;

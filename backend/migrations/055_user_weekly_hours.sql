-- Weekly contract hours as a dated history.
--
-- `users.weekly_hours` holds one value. Zerf recomputes every flextime figure
-- from scratch on each query, so changing that value after the start date
-- silently re-judged every week the person ever worked: a raise from 20 to 30
-- hours would have handed them a 30-hour target for their whole past.
--
-- Each row says "from this date on, the contract is this many hours a week".
-- A change adds a row (or replaces the row of the same date to fix a typo) and
-- never rewrites an earlier one. Every day asks for the value in force on it.
--
-- No backfill: a person with no rows keeps reading `users.weekly_hours` for
-- all time, exactly as before. The first change writes the old value as a
-- starting row dated from the contract start, followed by the new one.
-- `users.weekly_hours` stays as the value in force today (the latest row), so
-- everything that only needs the current contract keeps working unchanged.
CREATE TABLE IF NOT EXISTS user_weekly_hours (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- First day these hours apply to.
    valid_from DATE NOT NULL,
    weekly_hours DOUBLE PRECISION NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    CONSTRAINT user_weekly_hours_range CHECK (weekly_hours >= 0 AND weekly_hours <= 168),
    CONSTRAINT user_weekly_hours_one_per_day UNIQUE (user_id, valid_from)
);

CREATE INDEX IF NOT EXISTS idx_user_weekly_hours_lookup
    ON user_weekly_hours (user_id, valid_from DESC);

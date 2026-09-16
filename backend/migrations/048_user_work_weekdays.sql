-- Which weekdays a contract places work on, as a dated history.
--
-- `workdays_per_week` (migration 007) only ever said *how many* days somebody
-- works, never which. Every calculation therefore had to guess: the work target
-- was spread evenly over Monday to Friday, leave days were counted with a
-- weekly cap standing in for "which days are yours", and a public holiday was
-- pro-rated because nobody could say whether it fell on a working day.
--
-- Those guesses disagreed with each other. The same three leave days bought a
-- whole week off when booked Monday to Friday and 60% of a week when booked
-- Monday to Wednesday; a week split by the turn of the year was charged once
-- per side; and somebody working Tuesday to Friday had their Monday charged as
-- leave while their Friday went free.
--
-- Why a *history* and not one stored pattern: every flextime and leave figure
-- in Zerf is recomputed from scratch on each query. A single pattern would be
-- applied to the whole past as well, so the day somebody's working days change,
-- every week they ever worked is silently re-judged under the new pattern.
-- Moving from Monday-to-Thursday to Tuesday-to-Friday would strip the target
-- off every past Monday and put one on every past Friday, with nothing on the
-- record to say why.
--
-- Each row states "from this date on, these are the working days". A change
-- adds a row and never edits one. A week looks up the row with the greatest
-- `valid_from` that is not after its Monday, so the past keeps the pattern it
-- was actually worked under.
--
-- This is deliberately not the "freeze the computed week" approach. A frozen
-- number would need recomputing whenever a week is reopened, edited and
-- resubmitted, whenever an absence is approved for a past date, and whenever a
-- holiday is added — three refresh paths that all go stale silently if one is
-- missed. Dating the pattern needs no refresh: everything except the pattern
-- stays live, so a sick note entered for a past week still takes effect at once.
CREATE TABLE IF NOT EXISTS user_work_weekdays (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- First day this pattern applies to. The oldest row for a person carries
    -- their contract start, so every day they have been employed is covered.
    valid_from DATE NOT NULL,
    -- ISO weekday numbers: Monday is 1 through Friday is 5.
    --
    -- `cardinality`, not `array_length`: on an empty array `array_length`
    -- returns NULL, and a CHECK that evaluates to NULL passes, so an empty
    -- pattern would slip through. `cardinality` returns zero there and the row
    -- is refused, which is the point — "works no days" is a different statement
    -- from "days not recorded", and the latter is expressed by having no row.
    --
    -- Duplicates cannot be rejected here, because a CHECK may not contain a
    -- subquery. They are harmless, since the value is read as a set, and the
    -- repository sorts and de-duplicates on write.
    weekdays SMALLINT[] NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    CONSTRAINT user_work_weekdays_valid CHECK (
        cardinality(weekdays) BETWEEN 1 AND 5
        AND weekdays <@ ARRAY[1, 2, 3, 4, 5]::SMALLINT[]
    ),
    CONSTRAINT user_work_weekdays_one_per_day UNIQUE (user_id, valid_from)
);

-- The lookup every week performs: newest row for this person not after a date.
CREATE INDEX IF NOT EXISTS idx_user_work_weekdays_lookup
    ON user_work_weekdays (user_id, valid_from DESC);

-- Backfill a starting pattern for everyone who has a work target, dated from
-- their contract start so their whole history is covered.
--
-- For a five-day contract the pattern is Monday to Friday, which is exactly
-- what the old spread over the potential pool assumed, so those rows are
-- provably unchanged. For a shorter contract "the first N weekdays" is a guess:
-- the stored count says how many days, never which, so somebody working Tuesday
-- to Friday gets Monday to Thursday here. An admin corrects those in the user
-- settings; the guess is safe in the meantime because nothing reads this table
-- until the calculation is switched over in a later release.
--
-- Assistants get no row at all. They are paid for the hours they are present,
-- have no work target and no flextime account, so there is nothing for a
-- weekday pattern to decide. Pure-admin users (tracks_time = false) likewise.
INSERT INTO user_work_weekdays (user_id, valid_from, weekdays)
SELECT
    id,
    start_date,
    (
        SELECT array_agg(d::SMALLINT ORDER BY d)
        FROM generate_series(1, LEAST(GREATEST(workdays_per_week, 1), 5)) AS d
    )
FROM users
WHERE tracks_time AND role <> 'assistant'
ON CONFLICT (user_id, valid_from) DO NOTHING;

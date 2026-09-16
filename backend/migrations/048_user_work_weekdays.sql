-- Which weekdays a contract actually places work on.
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
-- `work_weekdays` removes the guessing. It stores the ISO weekday numbers the
-- person works, 1 for Monday through 7 for Sunday, as a sorted array.
--
-- NULL means "no fixed days", which is the assistants' case: they are paid for
-- the hours they are present, have no work target and no flextime account, so
-- there is nothing for a weekday pattern to decide. Every user who does have a
-- flextime account carries a pattern.
ALTER TABLE users ADD COLUMN IF NOT EXISTS work_weekdays SMALLINT[];

-- Only real weekday numbers, Monday through Friday, and never an empty list:
-- an empty pattern is not the same statement as "no pattern" and would leave a
-- contract with nowhere to put its hours.
--
-- Duplicates are not rejected here because a CHECK constraint may not contain a
-- subquery. They are harmless — the application reads the list as a set — and
-- the repository sorts and de-duplicates on write.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'users_work_weekdays_valid'
    ) THEN
        ALTER TABLE users ADD CONSTRAINT users_work_weekdays_valid CHECK (
            work_weekdays IS NULL
            OR (
                array_length(work_weekdays, 1) BETWEEN 1 AND 5
                AND work_weekdays <@ ARRAY[1,2,3,4,5]::SMALLINT[]
            )
        );
    END IF;
END $$;

-- Backfill with a default pattern: the first N weekdays, Monday onwards.
--
-- For a five-day contract that is Monday to Friday, which is exactly what the
-- old spread over the potential pool assumed, so those rows are provably
-- unchanged. For a shorter contract it is a guess. The stored count says how
-- many days somebody works, never which, and somebody working Tuesday to
-- Friday gets Monday to Thursday written here — wrong in both directions.
--
-- That guess is safe only because nothing reads this column yet. It must stay
-- that way until an admin has corrected the shorter contracts in the user
-- settings: switching the calculation over first would charge such a person's
-- Monday as leave, let their Friday go free, and drop a Friday sick note out
-- of the payroll report sent to the tax office. Column and admin screen ship
-- first, the corrections are made, and only then does the calculation move.
UPDATE users
SET work_weekdays = (
    SELECT array_agg(d::SMALLINT ORDER BY d)
    FROM generate_series(1, LEAST(GREATEST(workdays_per_week, 1), 5)) AS d
)
WHERE work_weekdays IS NULL
  AND tracks_time
  AND role <> 'assistant';

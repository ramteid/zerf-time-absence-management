-- Mark the time entries that exist when the work-in-a-row guard arrives.
--
-- The optional guard refuses an entry that would put more work in a row than
-- the break threshold allows, both when it is created and when its date,
-- times or category are changed. Entries that were already booked before the
-- guard existed must stay editable as they are, so they carry a flag that
-- exempts them from the check when they are changed. Every entry booked
-- afterwards starts out unflagged and is checked like a new one.
--
-- The flag is set only in the run that adds the column. A later run (the
-- migration must be safe to repeat) finds the column and leaves everything
-- alone; flagging all rows again would exempt entries booked since.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'time_entries'
          AND column_name = 'work_stretch_exempt'
    ) THEN
        ALTER TABLE time_entries
            ADD COLUMN work_stretch_exempt BOOLEAN NOT NULL DEFAULT FALSE;
        UPDATE time_entries SET work_stretch_exempt = TRUE;
    END IF;
END
$$;

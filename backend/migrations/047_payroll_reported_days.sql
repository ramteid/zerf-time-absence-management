-- Worked minutes a payroll report declared for one person on one day.
--
-- WHY: `time_entries.payroll_reported_period` records that an *entry existed*
-- when its month's report was produced. That is enough to recognise a day no
-- report has ever printed, but not enough to say how much of a day was already
-- paid — and the difference is money.
--
-- A week can be reopened (`reopen_requests` resets approved entries to draft),
-- and a draft can be deleted and booked again. The replacement is a new row
-- with no marker, so the catch-up path reads it as a day no report ever
-- printed and declares it a second time, in full, on top of the payment the
-- original already received. The same gap makes a second shift added to an
-- already-reported day deduct its automatic break against that shift alone
-- instead of against the whole day, so the day is paid with too little break
-- taken off.
--
-- The ledger is append-only by report. Ordinary rows declare the net day total
-- in that month's report. Later corrections are signed rows, so summing a
-- person's rows for a day is exactly what payroll has already received. The
-- catch-up section carries the difference between that sum and what the day is
-- worth now. A rebooked day of unchanged length carries nothing; a day that
-- genuinely grew carries exactly what it grew by, with the break computed over
-- the whole day. A cross-month move produces an equal negative row on the old
-- day and positive row on the new one instead of paying the hours twice.
--
-- No backfill, deliberately. What earlier reports declared per day cannot be
-- reconstructed, and inventing a zero would make every day ever reported look
-- as though it still owed its full hours. A day with no row whose own period
-- also predates `payroll_reported_periods` falls back to the entry marker
-- permanently, including when that fallback later prints a correction.
-- Recording only that correction here would mistake a partial amount for the
-- whole historic baseline on the following report. For a period recorded
-- below, an absent day row is a known zero and can safely start its ledger when
-- working time is added later.
CREATE TABLE IF NOT EXISTS payroll_reported_days (
    -- A declaration is historical payroll data. Deleting its user would also
    -- delete the baseline needed to issue a later negative correction, so the
    -- foreign key deliberately blocks hard deletion instead of cascading.
    user_id BIGINT NOT NULL REFERENCES users(id),
    day DATE NOT NULL,
    -- The report that declared these minutes, "YYYY-MM". Together with the
    -- person and day this identifies one exact document line.
    period TEXT NOT NULL,
    -- Net worked minutes this document declared for the day, after the
    -- automatic break deduction. Corrections can be negative.
    minutes BIGINT NOT NULL,
    reported_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (period, user_id, day)
);

-- The catch-up path sums a person's past declarations for candidate days.
CREATE INDEX IF NOT EXISTS idx_payroll_reported_days_user_day
    ON payroll_reported_days (user_id, day);

-- Periods settled after the day ledger became available. This distinguishes a
-- genuinely known zero baseline (a post-migration report had no value for the
-- day) from an absent row belonging to a pre-migration report whose per-day
-- value cannot be reconstructed.
CREATE TABLE IF NOT EXISTS payroll_reported_periods (
    period TEXT PRIMARY KEY,
    accounted_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Exact dashboard readback of a delivered document. The day ledger preserves
-- working-time arithmetic, while these rows preserve the rendered member set,
-- sections, labels, and zero-hour rows when settings, exclusions, names, or
-- absence data change later. `user_id` deliberately has no foreign key: a
-- historical report must remain readable after an otherwise data-free user is
-- deleted, and the id is retained only for the dashboard's visibility check.
CREATE TABLE IF NOT EXISTS payroll_reported_content (
    period TEXT NOT NULL REFERENCES payroll_reported_periods(period) ON DELETE CASCADE,
    position BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    employee TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('absence', 'hours', 'late_absence', 'late_hours')),
    category TEXT,
    from_date DATE,
    to_date DATE,
    days DOUBLE PRECISION NOT NULL,
    minutes BIGINT,
    medical_certificate_required BOOLEAN,
    PRIMARY KEY (period, position)
);

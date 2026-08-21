-- Turns the flextime carry-in balance from a mutable user setting into a
-- dated ledger booking.
--
-- WHY: `users.overtime_start_balance_min` was a single number attached to the
-- user profile. Editing it silently rewrote the employee's *entire* flextime
-- history, because every balance the app shows is "carry-in + sum of all daily
-- differences". After a few years of use, one careless edit in the user dialog
-- would move every balance ever reported for that person, with nothing in the
-- data to show what the balance used to be. There was also no way at all to
-- record a legitimate later correction (an overtime payout, a negotiated
-- reset) other than by falsifying that same carry-in number.
--
-- This table replaces it with an append-only ledger: every deliberate,
-- non-worked change to a flextime balance is its own dated row with an author
-- and a note. The carry-in balance is simply the first such row
-- (kind='opening_balance'), dated on the employee's start date. Reports fold
-- these rows into the balance on their effective date, exactly the way a day's
-- worked-minus-target difference is folded in — so history before an
-- adjustment's date is untouched by definition.
--
-- Rows are never updated or deleted. A mistaken entry is cancelled by booking
-- its opposite on the same date (`reverses_id`), which leaves both the mistake
-- and the correction visible. Allowing deletion would put the original problem
-- straight back: removing the opening row would silently move every balance
-- ever reported for that person.
CREATE TABLE IF NOT EXISTS flextime_adjustments (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The day the adjustment takes effect in the flextime ledger. Balances
    -- for any date before this one are unaffected.
    effective_date DATE NOT NULL,
    -- Signed minutes. Positive credits the account, negative debits it.
    -- The +/- one-year bound is added as a separate constraint after the
    -- backfill below, deliberately -- see the comment there.
    minutes BIGINT NOT NULL,
    -- 'opening_balance' is the carry-in booked once when the account is
    -- created; 'correction' is every later admin-made change.
    kind TEXT NOT NULL CHECK (kind IN ('opening_balance', 'correction')),
    -- Free-text note shown in the flextime account view and the audit log.
    reason TEXT,
    -- The admin who booked it. NULL for rows written by this migration and
    -- for rows whose author was later deleted.
    created_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    -- Set on a row that cancels an earlier one out: it carries the opposite
    -- minutes on the same effective date, so the balance returns to what it
    -- was while both rows stay on the record.
    reverses_id BIGINT REFERENCES flextime_adjustments(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_flextime_adjustments_user_date
    ON flextime_adjustments(user_id, effective_date);

-- A user has at most one carry-in row; every later change is a 'correction'.
CREATE UNIQUE INDEX IF NOT EXISTS idx_flextime_adjustments_one_opening
    ON flextime_adjustments(user_id)
    WHERE kind = 'opening_balance';

-- An entry can be cancelled at most once, so a double click cannot book the
-- reversal twice and swing the balance the other way.
CREATE UNIQUE INDEX IF NOT EXISTS idx_flextime_adjustments_one_reversal
    ON flextime_adjustments(reverses_id)
    WHERE reverses_id IS NOT NULL;

-- Migrate every existing carry-in balance into the ledger, dated on the
-- user's start date — which is exactly where the old code injected it. Users
-- whose balance is 0 carry no information and get no row. `created_by` stays
-- NULL so the UI can label these as system-migrated rather than attributing
-- them to whichever admin happens to run the upgrade.
--
-- Wrapped in a guarded DO block because the statement below drops the source
-- column: a re-run of this migration must not fail parsing an INSERT that
-- references a column that no longer exists. The NOT EXISTS clause guards the
-- other direction — a re-run against a database that still has the column
-- leaves already-migrated rows untouched (and the partial unique index above
-- would reject a duplicate anyway).
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'users'
          AND column_name = 'overtime_start_balance_min'
    ) THEN
        EXECUTE $backfill$
            INSERT INTO flextime_adjustments
                (user_id, effective_date, minutes, kind, reason, created_by)
            SELECT u.id, u.start_date, u.overtime_start_balance_min,
                   'opening_balance', NULL, NULL
            FROM users u
            WHERE u.overtime_start_balance_min <> 0
              AND NOT EXISTS (
                  SELECT 1 FROM flextime_adjustments fa
                  WHERE fa.user_id = u.id AND fa.kind = 'opening_balance'
              )
        $backfill$;
    END IF;
END $$;

-- Bound new bookings to +/- one year of minutes, matching what the old column
-- was validated against and what the API still enforces.
--
-- Added AFTER the backfill and marked NOT VALID on purpose: NOT VALID skips
-- checking rows that already exist while still enforcing the rule on every
-- later insert and update. A value outside the range can only get into the old
-- column by editing the database by hand, but if one ever did, an inline CHECK
-- would abort this migration -- and since migrations run at startup, the
-- application would refuse to boot until somebody edited production data under
-- pressure. Faithfully carrying such a value across is the safer outcome.
--
-- DROP IF EXISTS first so re-running the migration is a no-op rather than a
-- duplicate-name error.
ALTER TABLE flextime_adjustments
    DROP CONSTRAINT IF EXISTS flextime_adjustments_minutes_range;
ALTER TABLE flextime_adjustments
    ADD CONSTRAINT flextime_adjustments_minutes_range
    CHECK (minutes BETWEEN -525600 AND 525600) NOT VALID;

-- Drop the source column now that every value it held is a ledger row. Leaving
-- it behind would mean a second, writable copy of the same fact that nothing
-- reads — exactly the ambiguity this migration exists to remove.
--
-- Note that this makes the schema change one-way: an older application binary
-- selects this column and would fail to start against the migrated database.
-- Going back therefore needs a database restore, not just a redeploy.
ALTER TABLE users DROP COLUMN IF EXISTS overtime_start_balance_min;

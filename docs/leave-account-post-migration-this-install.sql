-- Installation-specific correction after sqlx migration 039.
--
-- Applies only to the production installation documented in PLAN.md:
--   id 9 / slug regenerationstage, id 8 / slug bildungsurlaub,
--   canonical Vacation account id 1, and the explicitly verified absence IDs.
-- Also revokes assistants' access to both accounts entirely (operator's
-- explicit instruction) and excludes user 10 (an employee who lacks access
-- to Regenerationstage) from the raised entitlement, so access and visible
-- balance agree from the moment this script commits — see PLAN.md's
-- 2026-08-06 addendum. Do not run this against another installation. It
-- bypasses the application audit log. Create and verify a backup with
-- scripts/backup.sh first; recovery uses scripts/restore.sh.
--
-- This script is idempotent for the documented target state. It intentionally
-- does not belong to backend/migrations because these IDs and historic rows
-- are installation-specific.
--
-- Run interactively in psql. Inspect every SELECT result below before allowing
-- the transaction to close. Replace COMMIT with ROLLBACK if anything looks
-- wrong.

BEGIN;

-- ── Identity checks ────────────────────────────────────────────────────────
-- Uses both id and slug; never relies on names or sort order.
DO $$
DECLARE
    expected_count integer;
    regen_start_year integer;
BEGIN
    SELECT count(*) INTO expected_count
    FROM absence_categories
    WHERE (id = 1 AND slug = 'vacation')
       OR (id = 8 AND slug = 'bildungsurlaub')
       OR (id = 9 AND slug = 'regenerationstage');
    IF expected_count <> 3 THEN
        RAISE EXCEPTION 'Expected Vacation (1), bildungsurlaub (8), and regenerationstage (9)';
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM absences
        WHERE id = 21 AND user_id = 8 AND category_id = 9
          AND start_date = DATE '2026-08-24' AND end_date = DATE '2026-08-25'
          AND status = 'approved'
    ) THEN
        RAISE EXCEPTION 'Expected regenerationstage absence 21 is missing or differs';
    END IF;

    IF (SELECT count(*) FROM absences
        WHERE id IN (16, 18, 19, 20) AND user_id = 8 AND category_id = 8) <> 4 THEN
        RAISE EXCEPTION 'Expected four bildungsurlaub absence rows 16, 18, 19, 20';
    END IF;

    -- Regenerationstage must already be a leave-account category set up by
    -- migration 039 with cost_type = 'vacation'. The start year must not be
    -- after 2026; NULL means the migration did not run or left an invalid state.
    SELECT leave_account_start_year
      INTO regen_start_year
    FROM absence_categories
    WHERE id = 9 AND slug = 'regenerationstage';

    IF regen_start_year IS NULL THEN
        RAISE EXCEPTION
            'Regenerationstage has no leave_account_start_year; migration 039 may not have run';
    END IF;
    IF regen_start_year > 2026 THEN
        RAISE EXCEPTION
            'Regenerationstage leave_account_start_year is %; the 2026 absence would precede the account',
            regen_start_year;
    END IF;

    -- Bildungsurlaub must be unpaid = false before converting to a vacation
    -- category (cost_type = 'vacation' and unpaid = true is a constraint violation).
    IF EXISTS (
        SELECT 1 FROM absence_categories
        WHERE id = 8 AND slug = 'bildungsurlaub'
          AND cost_type = 'none'
          AND unpaid = true
    ) THEN
        RAISE EXCEPTION
            'Bildungsurlaub has unpaid = true; convert unpaid to false before running this script';
    END IF;

    -- Bildungsurlaub must not be auto_approve_past (vacation + auto_approve = constraint violation).
    IF EXISTS (
        SELECT 1 FROM absence_categories
        WHERE id = 8 AND slug = 'bildungsurlaub'
          AND cost_type = 'none'
          AND auto_approve_past = true
    ) THEN
        RAISE EXCEPTION
            'Bildungsurlaub has auto_approve_past = true; this is incompatible with cost_type = vacation';
    END IF;

    -- User 10 must be a regular employee currently lacking access to
    -- Regenerationstage (id 9). This is the specific case the operator
    -- confirmed must not receive the raised 2-day entitlement below: they are
    -- not an assistant, so without this check the generic role-based CASE in
    -- step 2 would incorrectly raise their base_days from 0 to 2. This
    -- invariant is not changed by this script, so it holds on every rerun.
    IF NOT EXISTS (SELECT 1 FROM users WHERE id = 10 AND role = 'employee') THEN
        RAISE EXCEPTION 'Expected user 10 to exist and be a regular employee';
    END IF;
    IF EXISTS (
        SELECT 1 FROM user_absence_category_access
        WHERE user_id = 10 AND category_id = 9
    ) THEN
        RAISE EXCEPTION
            'User 10 unexpectedly already has access to Regenerationstage; re-verify before running this script';
    END IF;
END $$;

-- ── Before-state snapshot for user 8 ──────────────────────────────────────
-- Inspect this result and compare it to the after-state query near the end.
-- On a first run, absence 21 (category regenerationstage) will show
-- leave_account_category_id = 1 (Vacation) — the wrong account that will be
-- corrected by step 1 below. On a repeated run it shows the already-corrected
-- state.
SELECT 'BEFORE: absences for user 8 in categories vacation / bildungsurlaub / regenerationstage' AS label;
SELECT a.id,
       a.category_id,
       ac.slug  AS category_slug,
       a.leave_account_category_id,
       lac.slug AS account_slug,
       a.start_date,
       a.end_date,
       a.status
FROM   absences a
LEFT JOIN absence_categories ac  ON ac.id  = a.category_id
LEFT JOIN absence_categories lac ON lac.id = a.leave_account_category_id
WHERE  a.user_id = 8
  AND  a.category_id IN (1, 8, 9)
ORDER  BY a.category_id, a.start_date;

-- ── Step 1: Detach Regenerationstage from the Vacation account ─────────────
-- Move absence 21 from leave_account_category_id = 1 (Vacation) to
-- leave_account_category_id = 9 (Regenerationstage). IS DISTINCT FROM keeps
-- the UPDATE idempotent.
UPDATE absences
SET    leave_account_category_id = 9
WHERE  id = 21
  AND  category_id = 9
  AND  leave_account_category_id IS DISTINCT FROM 9;

-- ── Step 1b: Revoke assistants' access to both corrected accounts ─────────
-- Operator's explicit instruction: Regenerationstage and Bildungsurlaub are
-- not meant to be available to assistants at all (not just zero-balance).
-- Plain DELETE is idempotent — a rerun finds no matching rows.
DELETE FROM user_absence_category_access
WHERE  category_id IN (8, 9)
  AND  user_id IN (SELECT id FROM users WHERE role = 'assistant');

-- ── Step 2: Configure the Regenerationstage account ───────────────────────
-- Migration 039 created this account with base_days = 0 and the global
-- default carryover expiry (typically '03-31'). Update to the intended 2-day
-- entitlement and '01-01' expiry (no carryover). The start_year is preserved
-- from migration 039 (verified above to be <= 2026).
UPDATE absence_categories
SET    leave_account_default_days    = 2,
       leave_account_carryover_expiry = '01-01'
WHERE  id = 9 AND slug = 'regenerationstage';

-- Raise every user's base entitlement from 0 (set by migration 039) to 2
-- days — except assistants (role-based, per PLAN.md's existing rule) and
-- anyone currently lacking access to this category (verified above to be
-- exactly user 10). Access, not just role, gates the raised entitlement so
-- nobody sees a balance they cannot actually request. ON CONFLICT makes this
-- idempotent.
INSERT INTO user_leave_accounts (user_id, category_id, base_days)
SELECT u.id, 9,
       CASE
           WHEN u.role = 'assistant' THEN 0
           WHEN NOT EXISTS (
               SELECT 1 FROM user_absence_category_access acc
               WHERE  acc.user_id = u.id AND acc.category_id = 9
           ) THEN 0
           ELSE 2
       END
FROM   users u
ON CONFLICT (user_id, category_id) DO UPDATE
    SET base_days = EXCLUDED.base_days;

-- ── Step 3: Convert Bildungsurlaub to a leave-account category ────────────
-- Set all three account fields in a single UPDATE so the DB constraint
-- abs_cat_leave_account_fields_match_cost_type never sees an invalid
-- intermediate state. The constraint also ensures unpaid = false and
-- auto_approve_past = false for vacation categories (verified above).
UPDATE absence_categories
SET    cost_type                    = 'vacation',
       leave_account_default_days    = 5,
       leave_account_carryover_expiry = '01-01',
       leave_account_start_year      = 2026
WHERE  id = 8 AND slug = 'bildungsurlaub';

-- Create base-entitlement rows for all users (migration 039 created none for
-- this category because it was cost_type = 'none'). Same role-and-access
-- CASE as step 2 — after step 1b, every assistant already lacks access to
-- category 8 too, so the access branch alone would suffice, but the
-- role check is kept for defense in depth and consistency with step 2.
-- ON CONFLICT is defensive for idempotency.
INSERT INTO user_leave_accounts (user_id, category_id, base_days)
SELECT u.id, 8,
       CASE
           WHEN u.role = 'assistant' THEN 0
           WHEN NOT EXISTS (
               SELECT 1 FROM user_absence_category_access acc
               WHERE  acc.user_id = u.id AND acc.category_id = 8
           ) THEN 0
           ELSE 5
       END
FROM   users u
ON CONFLICT (user_id, category_id) DO UPDATE
    SET base_days = EXCLUDED.base_days;

-- ── Step 4: Book non-cancelled Bildungsurlaub absences against account 8 ──
-- Absence 18 (cancelled) is intentionally excluded from the id list and stays
-- NULL. IS DISTINCT FROM keeps the UPDATE idempotent.
UPDATE absences
SET    leave_account_category_id = 8
WHERE  id IN (16, 19, 20)
  AND  category_id = 8
  AND  leave_account_category_id IS DISTINCT FROM 8;

-- ── Verification before commit ─────────────────────────────────────────────
DO $$
DECLARE
    user_count             integer;
    active_regen_days_2026 bigint;
    active_edu_days_2026   bigint;
BEGIN
    SELECT count(*) INTO user_count FROM users;

    -- Absence 21 must charge account 9, not account 1 or NULL.
    -- IS DISTINCT FROM catches both the NULL case and any unexpected value.
    IF EXISTS (
        SELECT 1 FROM absences
        WHERE  id = 21 AND leave_account_category_id IS DISTINCT FROM 9
    ) THEN
        RAISE EXCEPTION 'Regenerationstage absence 21 does not charge account 9';
    END IF;

    -- Active Bildungsurlaub absences must charge account 8.
    IF EXISTS (
        SELECT 1 FROM absences
        WHERE  id IN (16, 19, 20) AND leave_account_category_id IS DISTINCT FROM 8
    ) THEN
        RAISE EXCEPTION 'An active Bildungsurlaub absence does not charge account 8';
    END IF;

    -- Cancelled absence 18 must remain unbooked.
    IF EXISTS (
        SELECT 1 FROM absences
        WHERE  id = 18 AND leave_account_category_id IS NOT NULL
    ) THEN
        RAISE EXCEPTION 'Cancelled Bildungsurlaub absence 18 must stay unbooked';
    END IF;

    -- Category configuration must match the intended target values exactly.
    -- For id = 9, leave_account_start_year is not checked here because it was
    -- verified in the identity check above and is not modified by this script.
    IF EXISTS (
        SELECT 1 FROM absence_categories
        WHERE  (id = 9 AND (cost_type IS DISTINCT FROM 'vacation'
                            OR leave_account_default_days    IS DISTINCT FROM 2
                            OR leave_account_carryover_expiry IS DISTINCT FROM '01-01'))
           OR  (id = 8 AND (cost_type IS DISTINCT FROM 'vacation'
                            OR leave_account_default_days    IS DISTINCT FROM 5
                            OR leave_account_carryover_expiry IS DISTINCT FROM '01-01'
                            OR leave_account_start_year       IS DISTINCT FROM 2026))
    ) THEN
        RAISE EXCEPTION 'Target account category configuration is invalid';
    END IF;

    -- Every user must have exactly one base row per corrected account.
    IF (SELECT count(*) FROM user_leave_accounts WHERE category_id = 9) <> user_count
       OR (SELECT count(*) FROM user_leave_accounts WHERE category_id = 8) <> user_count THEN
        RAISE EXCEPTION 'Every user must have one base row for each corrected account';
    END IF;

    -- User 10 must stay at zero for Regenerationstage: they are a regular
    -- employee (not an assistant) but were deliberately excluded from the
    -- raised entitlement because they lack access to this account.
    IF (SELECT base_days FROM user_leave_accounts WHERE user_id = 10 AND category_id = 9) <> 0 THEN
        RAISE EXCEPTION 'User 10 must keep base_days = 0 for Regenerationstage (no access)';
    END IF;

    -- No assistant may retain access to either corrected account.
    IF EXISTS (
        SELECT 1 FROM user_absence_category_access acc
        JOIN   users u ON u.id = acc.user_id
        WHERE  acc.category_id IN (8, 9) AND u.role = 'assistant'
    ) THEN
        RAISE EXCEPTION 'An assistant still has access to Regenerationstage or Bildungsurlaub';
    END IF;

    -- No assistant may have ever booked an absence in either category (this
    -- was true before this script ran and this script books nothing new for
    -- assistants, so it must still hold).
    IF EXISTS (
        SELECT 1 FROM absences a
        JOIN   users u ON u.id = a.user_id
        WHERE  a.category_id IN (8, 9) AND u.role = 'assistant'
    ) THEN
        RAISE EXCEPTION 'An assistant unexpectedly has an absence in Regenerationstage or Bildungsurlaub';
    END IF;

    -- User 8's base entitlement must cover the verified 2026 usage.
    IF (SELECT base_days FROM user_leave_accounts WHERE user_id = 8 AND category_id = 9) < 2
       OR (SELECT base_days FROM user_leave_accounts WHERE user_id = 8 AND category_id = 8) < 5 THEN
        RAISE EXCEPTION 'User 8 base_days are too low for the verified 2026 usage';
    END IF;

    -- Verify the total calendar-day span of active 2026 absences charged to
    -- each new account for user 8 does not exceed the base entitlement.
    -- Calendar days are used because workday counting requires holiday data
    -- that cannot be expressed in pure SQL. For this installation:
    --   - Absence 21 (Mon 24 + Tue 25 Aug 2026): 2 calendar days = 2 workdays.
    --   - Absences 16, 19, 20 (each a single weekday): 3 calendar days = 3 workdays.
    -- Both confirmed as weekday-only ranges; calendar days = workdays here.
    SELECT COALESCE(SUM(end_date - start_date + 1), 0)
      INTO active_regen_days_2026
    FROM   absences
    WHERE  user_id = 8
      AND  leave_account_category_id = 9
      AND  status NOT IN ('cancelled', 'rejected')
      AND  EXTRACT(YEAR FROM start_date) = 2026;

    IF active_regen_days_2026 > 2 THEN
        RAISE EXCEPTION
            'User 8: % calendar days of Regenerationstage charged in 2026 exceeds budget of 2',
            active_regen_days_2026;
    END IF;

    SELECT COALESCE(SUM(end_date - start_date + 1), 0)
      INTO active_edu_days_2026
    FROM   absences
    WHERE  user_id = 8
      AND  leave_account_category_id = 8
      AND  status NOT IN ('cancelled', 'rejected')
      AND  EXTRACT(YEAR FROM start_date) = 2026;

    IF active_edu_days_2026 > 5 THEN
        RAISE EXCEPTION
            'User 8: % calendar days of Bildungsurlaub charged in 2026 exceeds budget of 5',
            active_edu_days_2026;
    END IF;
END $$;

-- ── After-state snapshot for user 8 ───────────────────────────────────────
-- Absence 21: account_slug must be 'regenerationstage'.
-- Absences 16, 19, 20: account_slug must be 'bildungsurlaub'.
-- Absence 18: account_slug must be NULL (cancelled, unbooked).
SELECT 'AFTER: absences for user 8 in categories vacation / bildungsurlaub / regenerationstage' AS label;
SELECT a.id,
       a.category_id,
       ac.slug  AS category_slug,
       a.leave_account_category_id,
       lac.slug AS account_slug,
       a.start_date,
       a.end_date,
       a.status
FROM   absences a
LEFT JOIN absence_categories ac  ON ac.id  = a.category_id
LEFT JOIN absence_categories lac ON lac.id = a.leave_account_category_id
WHERE  a.user_id = 8
  AND  a.category_id IN (1, 8, 9)
ORDER  BY a.category_id, a.start_date;

-- Corrected category configuration and base entitlements for user 8.
-- Regenerationstage: default_days = 2, expiry = '01-01', start_year set by migration 039 (<= 2026).
-- Bildungsurlaub: default_days = 5, expiry = '01-01', start_year = 2026.
SELECT c.id,
       c.slug,
       c.cost_type,
       c.leave_account_default_days,
       c.leave_account_carryover_expiry,
       c.leave_account_start_year,
       ula.base_days
FROM   absence_categories c
JOIN   user_leave_accounts ula ON ula.category_id = c.id
WHERE  c.id IN (8, 9) AND ula.user_id = 8
ORDER  BY c.id;

-- Active-vs-cancelled summary for user 8 per corrected account in 2026.
-- Expected: regenerationstage = 1 active row / 2 calendar days / 0 cancelled;
--           bildungsurlaub    = 3 active rows / 3 calendar days / 0 cancelled.
-- Cancelled absence 18 deliberately stays NULL (step 4 excludes it), so the
-- inner join below never matches it and cancelled_rows reads 0 for both
-- accounts — it does not mean "no cancelled Bildungsurlaub absence exists",
-- only that none of them charge an account. See the AFTER snapshot above for
-- absence 18's actual (unbooked) row.
SELECT lac.slug                                                              AS account_slug,
       count(*) FILTER (WHERE a.status NOT IN ('cancelled', 'rejected'))    AS active_rows,
       COALESCE(SUM(a.end_date - a.start_date + 1)
                FILTER (WHERE a.status NOT IN ('cancelled', 'rejected')), 0) AS active_calendar_days,
       count(*) FILTER (WHERE a.status = 'cancelled')                       AS cancelled_rows
FROM   absences a
JOIN   absence_categories lac ON lac.id = a.leave_account_category_id
WHERE  a.user_id = 8
  AND  a.leave_account_category_id IN (8, 9)
  AND  EXTRACT(YEAR FROM a.start_date) = 2026
GROUP  BY lac.slug
ORDER  BY lac.slug;

-- ── After-state snapshot: user 10 and every assistant ─────────────────────
-- User 10: access must be false and base_days must be 0 for category 9.
-- Assistants (any role = 'assistant'): access_count must be 0 for both
-- categories, and base_days is expected to already have been 0 beforehand.
SELECT 'AFTER: Regenerationstage/Bildungsurlaub access and entitlement for user 10 and assistants' AS label;
SELECT u.id, u.role,
       (SELECT count(*) FROM user_absence_category_access acc
         WHERE acc.user_id = u.id AND acc.category_id = 9) AS access_regen,
       (SELECT base_days FROM user_leave_accounts ula
         WHERE ula.user_id = u.id AND ula.category_id = 9) AS base_days_regen,
       (SELECT count(*) FROM user_absence_category_access acc
         WHERE acc.user_id = u.id AND acc.category_id = 8) AS access_edu,
       (SELECT base_days FROM user_leave_accounts ula
         WHERE ula.user_id = u.id AND ula.category_id = 8) AS base_days_edu
FROM   users u
WHERE  u.id = 10 OR u.role = 'assistant'
ORDER  BY u.role, u.id;

-- All assertions above must pass and every SELECT result must match
-- expectations. If anything looks wrong, replace this COMMIT with ROLLBACK
-- in the interactive session before executing it.
COMMIT;

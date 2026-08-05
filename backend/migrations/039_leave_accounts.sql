-- Category-owned leave accounts replace the single organisation-wide annual
-- leave entitlement. The migration deliberately keeps the historical absence
-- category separate from the account charged for it: all existing vacation-cost
-- absences keep charging the canonical `vacation` account after the split.
--
-- Every statement is safe to repeat. Legacy backfills only run while their
-- source column/table still exists, so a later re-run cannot overwrite already
-- migrated account assignments or entitlement values.

ALTER TABLE absence_categories
    ADD COLUMN IF NOT EXISTS leave_account_default_days BIGINT,
    ADD COLUMN IF NOT EXISTS leave_account_carryover_expiry TEXT,
    ADD COLUMN IF NOT EXISTS leave_account_start_year INTEGER;

ALTER TABLE absences
    ADD COLUMN IF NOT EXISTS leave_account_category_id BIGINT;

-- Validate the legacy values before using them as category configuration. A
-- missing or blank value is the historical default; a malformed stored value
-- is data corruption and must not silently change users' entitlements.
DO $$
DECLARE
    canonical_category_count BIGINT;
    canonical_cost_type TEXT;
    raw_default_days TEXT;
    default_days TEXT;
    raw_expiry TEXT;
    expiry TEXT;
    expiry_month INTEGER;
    expiry_day INTEGER;
BEGIN
    SELECT COUNT(*), MIN(cost_type)
      INTO canonical_category_count, canonical_cost_type
      FROM absence_categories
     WHERE slug = 'vacation';

    IF canonical_category_count <> 1 THEN
        RAISE EXCEPTION
            'Migration 039 requires exactly one canonical absence category with slug ''vacation''; found %',
            canonical_category_count;
    END IF;

    IF canonical_cost_type <> 'vacation' THEN
        RAISE EXCEPTION
            'Migration 039 requires canonical absence category slug ''vacation'' to use cost_type ''vacation''; found %',
            canonical_cost_type;
    END IF;

    SELECT value INTO raw_default_days
      FROM app_settings
     WHERE key = 'default_annual_leave_days';
    default_days := NULLIF(BTRIM(raw_default_days), '');
    IF default_days IS NOT NULL THEN
        IF default_days !~ '^[0-9]{1,3}$' THEN
            RAISE EXCEPTION
                'Migration 039 found invalid default_annual_leave_days value %; expected an integer from 0 to 366',
                raw_default_days;
        END IF;
        IF default_days::BIGINT NOT BETWEEN 0 AND 366 THEN
            RAISE EXCEPTION
                'Migration 039 found invalid default_annual_leave_days value %; expected an integer from 0 to 366',
                raw_default_days;
        END IF;
    END IF;

    SELECT value INTO raw_expiry
      FROM app_settings
     WHERE key = 'carryover_expiry_date';
    expiry := NULLIF(BTRIM(raw_expiry), '');
    IF expiry IS NOT NULL THEN
        IF expiry !~ '^(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$' THEN
            RAISE EXCEPTION
                'Migration 039 found invalid carryover_expiry_date value %; expected a real MM-DD date',
                raw_expiry;
        END IF;
        expiry_month := SUBSTRING(expiry FROM 1 FOR 2)::INTEGER;
        expiry_day := SUBSTRING(expiry FROM 4 FOR 2)::INTEGER;
        IF (expiry_month = 2 AND expiry_day > 29)
           OR (expiry_month IN (4, 6, 9, 11) AND expiry_day > 30) THEN
            RAISE EXCEPTION
                'Migration 039 found invalid carryover_expiry_date value %; expected a real MM-DD date',
                raw_expiry;
        END IF;
    END IF;
END $$;

-- Resolve the migration year in the same configured application timezone the
-- application uses. Unknown and blank timezone settings fall back to
-- Europe/Berlin, matching `load_app_timezone`.
WITH configured_timezone AS (
    SELECT COALESCE(NULLIF(BTRIM((
        SELECT value FROM app_settings WHERE key = 'timezone'
    )), ''), 'Europe/Berlin') AS configured_name
), application_timezone AS (
    SELECT CASE
        WHEN configured_name IN (SELECT name FROM pg_timezone_names)
            THEN configured_name
        ELSE 'Europe/Berlin'
    END AS timezone_name
    FROM configured_timezone
), migration_values AS (
    SELECT
        COALESCE(NULLIF(BTRIM((
            SELECT value FROM app_settings WHERE key = 'default_annual_leave_days'
        )), ''), '30')::BIGINT AS default_days,
        COALESCE(NULLIF(BTRIM((
            SELECT value FROM app_settings WHERE key = 'carryover_expiry_date'
        )), ''), '03-31') AS carryover_expiry,
        EXTRACT(YEAR FROM timezone(timezone_name, CURRENT_TIMESTAMP))::INTEGER AS current_year
    FROM application_timezone
)
UPDATE absence_categories AS category
   SET leave_account_default_days = COALESCE(category.leave_account_default_days, values.default_days),
       leave_account_carryover_expiry = COALESCE(
           category.leave_account_carryover_expiry,
           values.carryover_expiry
       ),
       leave_account_start_year = COALESCE(
           category.leave_account_start_year,
           (
               SELECT COALESCE(
                   EXTRACT(YEAR FROM MIN(users.start_date))::INTEGER,
                   values.current_year
               )
               FROM users
           )
       )
  FROM migration_values AS values
 WHERE category.slug = 'vacation';

-- Existing non-canonical vacation-cost categories used to share the one
-- vacation bucket. They become separate accounts only from the migration year
-- onward and intentionally start with a zero entitlement.
WITH configured_timezone AS (
    SELECT COALESCE(NULLIF(BTRIM((
        SELECT value FROM app_settings WHERE key = 'timezone'
    )), ''), 'Europe/Berlin') AS configured_name
), application_timezone AS (
    SELECT CASE
        WHEN configured_name IN (SELECT name FROM pg_timezone_names)
            THEN configured_name
        ELSE 'Europe/Berlin'
    END AS timezone_name
    FROM configured_timezone
), migration_values AS (
    SELECT
        COALESCE(NULLIF(BTRIM((
            SELECT value FROM app_settings WHERE key = 'carryover_expiry_date'
        )), ''), '03-31') AS carryover_expiry,
        EXTRACT(YEAR FROM timezone(timezone_name, CURRENT_TIMESTAMP))::INTEGER AS current_year
    FROM application_timezone
)
UPDATE absence_categories AS category
   SET leave_account_default_days = COALESCE(category.leave_account_default_days, 0),
       leave_account_carryover_expiry = COALESCE(
           category.leave_account_carryover_expiry,
           values.carryover_expiry
       ),
       leave_account_start_year = COALESCE(category.leave_account_start_year, values.current_year)
  FROM migration_values AS values
 WHERE category.cost_type = 'vacation'
   AND category.slug <> 'vacation';

-- A non-leave-account category must never retain account metadata. This also
-- normalizes installations that experimented with the nullable columns before
-- applying the official migration.
UPDATE absence_categories
   SET leave_account_default_days = NULL,
       leave_account_carryover_expiry = NULL,
       leave_account_start_year = NULL
 WHERE cost_type <> 'vacation'
   AND (
       leave_account_default_days IS NOT NULL
       OR leave_account_carryover_expiry IS NOT NULL
       OR leave_account_start_year IS NOT NULL
   );

-- Map legacy vacation-cost absences to the canonical account only when the
-- target is still NULL. New absences always write their own account id, so a
-- repeated migration can never move an already booked absence.
UPDATE absences AS absence
   SET leave_account_category_id = canonical_category.id
  FROM absence_categories AS historical_category
  JOIN absence_categories AS canonical_category
    ON canonical_category.slug = 'vacation'
 WHERE absence.category_id = historical_category.id
   AND historical_category.cost_type = 'vacation'
   AND absence.leave_account_category_id IS NULL;

CREATE TABLE IF NOT EXISTS user_leave_accounts (
    user_id BIGINT NOT NULL,
    category_id BIGINT NOT NULL,
    base_days BIGINT NOT NULL,
    CONSTRAINT user_leave_accounts_pkey PRIMARY KEY (user_id, category_id),
    CONSTRAINT user_leave_accounts_user_id_fkey
        FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    CONSTRAINT user_leave_accounts_category_id_fkey
        FOREIGN KEY (category_id) REFERENCES absence_categories(id) ON DELETE CASCADE,
    CONSTRAINT user_leave_accounts_base_days_range
        CHECK (base_days BETWEEN 0 AND 366)
);

CREATE TABLE IF NOT EXISTS user_leave_account_year_overrides (
    user_id BIGINT NOT NULL,
    category_id BIGINT NOT NULL,
    year INTEGER NOT NULL,
    days BIGINT NOT NULL,
    CONSTRAINT user_leave_account_year_overrides_pkey
        PRIMARY KEY (user_id, category_id, year),
    CONSTRAINT user_leave_account_year_overrides_user_id_fkey
        FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    CONSTRAINT user_leave_account_year_overrides_category_id_fkey
        FOREIGN KEY (category_id) REFERENCES absence_categories(id) ON DELETE CASCADE,
    CONSTRAINT user_leave_account_year_overrides_year_range
        CHECK (year BETWEEN 2000 AND 2100),
    CONSTRAINT user_leave_account_year_overrides_days_range
        CHECK (days BETWEEN 0 AND 366)
);

CREATE INDEX IF NOT EXISTS idx_user_leave_accounts_category_user
    ON user_leave_accounts(category_id, user_id);
CREATE INDEX IF NOT EXISTS idx_user_leave_account_year_overrides_category_user_year
    ON user_leave_account_year_overrides(category_id, user_id, year);
CREATE INDEX IF NOT EXISTS idx_absences_leave_account_user_dates
    ON absences(user_id, leave_account_category_id, start_date, end_date);

-- Copy per-user base entitlement only while the legacy column still exists.
-- The canonical account receives its historical user value; any additional
-- historic vacation-cost category receives zero days as a newly separated
-- account. ON CONFLICT preserves a completed or manually repaired migration.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'users'
          AND column_name = 'annual_leave_days'
    ) THEN
        EXECUTE $sql$
            INSERT INTO user_leave_accounts (user_id, category_id, base_days)
            SELECT
                users.id,
                category.id,
                CASE WHEN category.slug = 'vacation' THEN users.annual_leave_days ELSE 0 END
            FROM users
            CROSS JOIN absence_categories AS category
            WHERE category.cost_type = 'vacation'
            ON CONFLICT (user_id, category_id) DO NOTHING
        $sql$;
    END IF;
END $$;

-- Copy historical yearly overrides into the canonical Vacation account while
-- the legacy source table remains available.
DO $$
BEGIN
    IF to_regclass('user_annual_leave') IS NOT NULL THEN
        EXECUTE $sql$
            INSERT INTO user_leave_account_year_overrides (user_id, category_id, year, days)
            SELECT legacy.user_id, canonical_category.id, legacy.year, legacy.days
            FROM user_annual_leave AS legacy
            JOIN absence_categories AS canonical_category
              ON canonical_category.slug = 'vacation'
            ON CONFLICT (user_id, category_id, year) DO NOTHING
        $sql$;
    END IF;
END $$;

-- Existing tables from an interrupted/manual deployment may have been created
-- without all named constraints. Add every invariant by name only once.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'absence_categories'::regclass
           AND conname = 'abs_cat_leave_account_default_days_range'
    ) THEN
        ALTER TABLE absence_categories
            ADD CONSTRAINT abs_cat_leave_account_default_days_range
            CHECK (
                leave_account_default_days IS NULL
                OR leave_account_default_days BETWEEN 0 AND 366
            );
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'absence_categories'::regclass
           AND conname = 'abs_cat_leave_account_carryover_expiry_format'
    ) THEN
        ALTER TABLE absence_categories
            ADD CONSTRAINT abs_cat_leave_account_carryover_expiry_format
            CHECK (
                leave_account_carryover_expiry IS NULL
                OR CASE
                    WHEN leave_account_carryover_expiry ~ '^(0[1-9]|1[0-2])-(0[1-9]|[12][0-9]|3[01])$'
                    THEN CASE SUBSTRING(leave_account_carryover_expiry FROM 1 FOR 2)::INTEGER
                        WHEN 2 THEN SUBSTRING(leave_account_carryover_expiry FROM 4 FOR 2)::INTEGER <= 29
                        WHEN 4 THEN SUBSTRING(leave_account_carryover_expiry FROM 4 FOR 2)::INTEGER <= 30
                        WHEN 6 THEN SUBSTRING(leave_account_carryover_expiry FROM 4 FOR 2)::INTEGER <= 30
                        WHEN 9 THEN SUBSTRING(leave_account_carryover_expiry FROM 4 FOR 2)::INTEGER <= 30
                        WHEN 11 THEN SUBSTRING(leave_account_carryover_expiry FROM 4 FOR 2)::INTEGER <= 30
                        ELSE TRUE
                    END
                    ELSE FALSE
                END
            );
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'absence_categories'::regclass
           AND conname = 'abs_cat_leave_account_fields_match_cost_type'
    ) THEN
        ALTER TABLE absence_categories
            ADD CONSTRAINT abs_cat_leave_account_fields_match_cost_type
            CHECK (
                (
                    cost_type = 'vacation'
                    AND leave_account_default_days IS NOT NULL
                    AND leave_account_carryover_expiry IS NOT NULL
                    AND leave_account_start_year IS NOT NULL
                )
                OR (
                    cost_type <> 'vacation'
                    AND leave_account_default_days IS NULL
                    AND leave_account_carryover_expiry IS NULL
                    AND leave_account_start_year IS NULL
                )
            );
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'absences'::regclass
           AND conname = 'absences_leave_account_category_fkey'
    ) THEN
        ALTER TABLE absences
            ADD CONSTRAINT absences_leave_account_category_fkey
            FOREIGN KEY (leave_account_category_id)
            REFERENCES absence_categories(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_accounts'::regclass
           AND contype = 'p'
    ) THEN
        ALTER TABLE user_leave_accounts
            ADD CONSTRAINT user_leave_accounts_pkey
            PRIMARY KEY (user_id, category_id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_accounts'::regclass
           AND conname = 'user_leave_accounts_user_id_fkey'
    ) THEN
        ALTER TABLE user_leave_accounts
            ADD CONSTRAINT user_leave_accounts_user_id_fkey
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_accounts'::regclass
           AND conname = 'user_leave_accounts_category_id_fkey'
    ) THEN
        ALTER TABLE user_leave_accounts
            ADD CONSTRAINT user_leave_accounts_category_id_fkey
            FOREIGN KEY (category_id) REFERENCES absence_categories(id) ON DELETE CASCADE;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_accounts'::regclass
           AND conname = 'user_leave_accounts_base_days_range'
    ) THEN
        ALTER TABLE user_leave_accounts
            ADD CONSTRAINT user_leave_accounts_base_days_range
            CHECK (base_days BETWEEN 0 AND 366);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_account_year_overrides'::regclass
           AND conname = 'user_leave_account_year_overrides_year_range'
    ) THEN
        ALTER TABLE user_leave_account_year_overrides
            ADD CONSTRAINT user_leave_account_year_overrides_year_range
            CHECK (year BETWEEN 2000 AND 2100);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_account_year_overrides'::regclass
           AND contype = 'p'
    ) THEN
        ALTER TABLE user_leave_account_year_overrides
            ADD CONSTRAINT user_leave_account_year_overrides_pkey
            PRIMARY KEY (user_id, category_id, year);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_account_year_overrides'::regclass
           AND conname = 'user_leave_account_year_overrides_user_id_fkey'
    ) THEN
        ALTER TABLE user_leave_account_year_overrides
            ADD CONSTRAINT user_leave_account_year_overrides_user_id_fkey
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_account_year_overrides'::regclass
           AND conname = 'user_leave_account_year_overrides_category_id_fkey'
    ) THEN
        ALTER TABLE user_leave_account_year_overrides
            ADD CONSTRAINT user_leave_account_year_overrides_category_id_fkey
            FOREIGN KEY (category_id) REFERENCES absence_categories(id) ON DELETE CASCADE;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conrelid = 'user_leave_account_year_overrides'::regclass
           AND conname = 'user_leave_account_year_overrides_days_range'
    ) THEN
        ALTER TABLE user_leave_account_year_overrides
            ADD CONSTRAINT user_leave_account_year_overrides_days_range
            CHECK (days BETWEEN 0 AND 366);
    END IF;
END $$;

-- Verify every data relationship before dropping its legacy source. The first
-- branch proves the historical assignment to canonical Vacation. On a later
-- re-run, after the source structures have gone, legitimate new accounts may
-- have their own account id; then only an unassigned vacation-cost absence is
-- invalid.
DO $$
DECLARE
    canonical_category_id BIGINT;
    missing_base_rows BIGINT;
    legacy_override_count BIGINT;
    migrated_override_count BIGINT;
    invalid_absence_count BIGINT;
    invalid_category_count BIGINT;
    legacy_base_column_exists BOOLEAN;
    legacy_override_table_exists BOOLEAN;
BEGIN
    SELECT id INTO canonical_category_id
      FROM absence_categories
     WHERE slug = 'vacation';

    SELECT COUNT(*) INTO missing_base_rows
      FROM users
      CROSS JOIN absence_categories AS category
     WHERE category.cost_type = 'vacation'
       AND NOT EXISTS (
           SELECT 1
           FROM user_leave_accounts AS account
           WHERE account.user_id = users.id
             AND account.category_id = category.id
       );
    IF missing_base_rows > 0 THEN
        RAISE EXCEPTION
            'Migration 039 could not create % required user leave-account base rows',
            missing_base_rows;
    END IF;

    SELECT EXISTS (
        SELECT 1
        FROM information_schema.columns
        WHERE table_schema = current_schema()
          AND table_name = 'users'
          AND column_name = 'annual_leave_days'
    ) INTO legacy_base_column_exists;
    legacy_override_table_exists := to_regclass('user_annual_leave') IS NOT NULL;

    IF legacy_override_table_exists THEN
        EXECUTE 'SELECT COUNT(*) FROM user_annual_leave'
           INTO legacy_override_count;
        SELECT COUNT(*) INTO migrated_override_count
          FROM user_leave_account_year_overrides AS account_override
          JOIN absence_categories AS category
            ON category.id = account_override.category_id
         WHERE category.slug = 'vacation';
        IF migrated_override_count <> legacy_override_count THEN
            RAISE EXCEPTION
                'Migration 039 migrated % Vacation yearly overrides but legacy table contains % rows',
                migrated_override_count,
                legacy_override_count;
        END IF;
    END IF;

    IF legacy_base_column_exists OR legacy_override_table_exists THEN
        SELECT COUNT(*) INTO invalid_absence_count
          FROM absences AS absence
          JOIN absence_categories AS category
            ON category.id = absence.category_id
         WHERE category.cost_type = 'vacation'
           AND absence.leave_account_category_id IS DISTINCT FROM canonical_category_id;
        IF invalid_absence_count > 0 THEN
            RAISE EXCEPTION
                'Migration 039 left % historical vacation-cost absences outside the canonical Vacation account',
                invalid_absence_count;
        END IF;
    ELSE
        SELECT COUNT(*) INTO invalid_absence_count
          FROM absences AS absence
          JOIN absence_categories AS category
            ON category.id = absence.category_id
         WHERE category.cost_type = 'vacation'
           AND absence.leave_account_category_id IS NULL;
        IF invalid_absence_count > 0 THEN
            RAISE EXCEPTION
                'Migration 039 found % vacation-cost absences without a leave account',
                invalid_absence_count;
        END IF;
    END IF;

    SELECT COUNT(*) INTO invalid_category_count
      FROM absence_categories
     WHERE (cost_type = 'vacation' AND (
                leave_account_default_days IS NULL
                OR leave_account_carryover_expiry IS NULL
                OR leave_account_start_year IS NULL
           ))
        OR (cost_type <> 'vacation' AND (
                leave_account_default_days IS NOT NULL
                OR leave_account_carryover_expiry IS NOT NULL
                OR leave_account_start_year IS NOT NULL
           ));
    IF invalid_category_count > 0 THEN
        RAISE EXCEPTION
            'Migration 039 found % absence categories with invalid leave-account fields',
            invalid_category_count;
    END IF;
END $$;

-- The old global values no longer have a source of truth. Remove them only
-- after the verification block above has completed successfully.
ALTER TABLE users DROP COLUMN IF EXISTS annual_leave_days;
DROP TABLE IF EXISTS user_annual_leave;
DELETE FROM app_settings
 WHERE key IN ('default_annual_leave_days', 'carryover_expiry_date');

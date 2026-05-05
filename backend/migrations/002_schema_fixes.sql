-- Enforce HH:MM or HH:MM:SS format on time_entries.start_time and end_time.
-- parse_time() accepts both formats; the regex matches either to avoid rejecting
-- existing rows that may have been stored with seconds. Ordering is validated by
-- the application on every write path (create, update, change-request approval).
ALTER TABLE time_entries
  ADD CONSTRAINT te_start_time_format CHECK (start_time ~ '^\d{2}:\d{2}(:\d{2})?$'),
  ADD CONSTRAINT te_end_time_format   CHECK (end_time   ~ '^\d{2}:\d{2}(:\d{2})?$');

-- Remove the empty-string default from sessions.csrf_token.
-- The application always supplies an explicit value on insert; the default
-- was a latent risk because a missing bind would silently store '' which
-- is indistinguishable from a legitimate token by CSRF middleware.
ALTER TABLE sessions ALTER COLUMN csrf_token DROP DEFAULT;

-- Add a primary key to login_attempts to prevent duplicate rows.
-- Duplicate rows would double-count failures in the sliding-window
-- rate-limit query, causing spurious account lockouts.
ALTER TABLE login_attempts ADD COLUMN id BIGSERIAL PRIMARY KEY;

-- Enforce consistency between holidays.year and holidays.holiday_date.
-- The application always derives year from the date, but the schema had
-- no constraint preventing a manual SQL insert from storing a mismatch.
ALTER TABLE holidays
  ADD CONSTRAINT holidays_year_matches_date
    CHECK (year = EXTRACT(YEAR FROM holiday_date)::INTEGER);

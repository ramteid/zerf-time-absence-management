CREATE TABLE IF NOT EXISTS users (
  id BIGSERIAL PRIMARY KEY,
  email TEXT NOT NULL UNIQUE CHECK (char_length(email) <= 254),
  password_hash TEXT NOT NULL,
  first_name TEXT NOT NULL,
  last_name TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('employee','team_lead','admin')),
  weekly_hours DOUBLE PRECISION NOT NULL CHECK (weekly_hours >= 0 AND weekly_hours <= 168),
  start_date DATE NOT NULL,
  active BOOLEAN NOT NULL DEFAULT TRUE,
  must_change_password BOOLEAN NOT NULL DEFAULT FALSE,
  approver_id BIGINT REFERENCES users(id),
  allow_reopen_without_approval BOOLEAN NOT NULL DEFAULT FALSE,
  dark_mode BOOLEAN NOT NULL DEFAULT FALSE,
  overtime_start_balance_min BIGINT NOT NULL DEFAULT 0,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  CONSTRAINT users_non_admin_has_approver
    CHECK (role = 'admin' OR approver_id IS NOT NULL),
  CONSTRAINT users_approver_not_self
    CHECK (approver_id IS NULL OR approver_id <> id)
);
CREATE INDEX IF NOT EXISTS idx_users_approver ON users(approver_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_first_last_name_unique
  ON users(first_name, last_name);

CREATE TABLE IF NOT EXISTS sessions (
  token TEXT PRIMARY KEY,
  user_id BIGINT NOT NULL REFERENCES users(id),
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  last_active_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  csrf_token TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_active ON sessions(last_active_at);

CREATE TABLE IF NOT EXISTS login_attempts (
  id BIGSERIAL PRIMARY KEY,
  email TEXT NOT NULL,
  attempted_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  success BOOLEAN NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_login_attempts_email ON login_attempts(email, attempted_at);

CREATE TABLE IF NOT EXISTS categories (
  id BIGSERIAL PRIMARY KEY,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  color TEXT NOT NULL CHECK (color ~ '^#[0-9A-Fa-f]{6}$'),
  sort_order BIGINT NOT NULL DEFAULT 0,
  active BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS time_entries (
  id BIGSERIAL PRIMARY KEY,
  user_id BIGINT NOT NULL REFERENCES users(id),
  entry_date DATE NOT NULL,
  start_time TEXT NOT NULL CONSTRAINT te_start_time_format CHECK (start_time ~ '^\d{2}:\d{2}(:\d{2})?$'),
  end_time TEXT NOT NULL CONSTRAINT te_end_time_format   CHECK (end_time   ~ '^\d{2}:\d{2}(:\d{2})?$'),
  category_id BIGINT NOT NULL REFERENCES categories(id),
  comment TEXT,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','submitted','approved','rejected')),
  submitted_at TIMESTAMPTZ,
  reviewed_by BIGINT REFERENCES users(id),
  reviewed_at TIMESTAMPTZ,
  rejection_reason TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_te_user_date ON time_entries(user_id, entry_date);
CREATE INDEX IF NOT EXISTS idx_te_status_date ON time_entries(status, entry_date DESC);

CREATE TABLE IF NOT EXISTS absences (
  id BIGSERIAL PRIMARY KEY,
  user_id BIGINT NOT NULL REFERENCES users(id),
  kind TEXT NOT NULL CHECK (kind IN ('vacation','sick','training','special_leave','unpaid','general_absence')),
  start_date DATE NOT NULL,
  end_date DATE NOT NULL,
  comment TEXT,
  status TEXT NOT NULL DEFAULT 'requested' CHECK (status IN ('requested','approved','rejected','cancelled')),
  reviewed_by BIGINT REFERENCES users(id),
  reviewed_at TIMESTAMPTZ,
  rejection_reason TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
  CHECK (end_date >= start_date)
);
CREATE INDEX IF NOT EXISTS idx_abs_user ON absences(user_id, start_date, end_date);
CREATE INDEX IF NOT EXISTS idx_abs_status_date ON absences(status, start_date DESC);

CREATE TABLE IF NOT EXISTS change_requests (
  id BIGSERIAL PRIMARY KEY,
  time_entry_id BIGINT NOT NULL REFERENCES time_entries(id),
  user_id BIGINT NOT NULL REFERENCES users(id),
  new_date DATE,
  new_start_time TEXT,
  new_end_time TEXT,
  new_category_id BIGINT REFERENCES categories(id),
  new_comment TEXT,
  reason TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','approved','rejected')),
  reviewed_by BIGINT REFERENCES users(id),
  reviewed_at TIMESTAMPTZ,
  rejection_reason TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_cr_user_created ON change_requests(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_cr_status_created ON change_requests(status, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_cr_entry_open
    ON change_requests(time_entry_id)
    WHERE status = 'open';

CREATE TABLE IF NOT EXISTS holidays (
  id BIGSERIAL PRIMARY KEY,
  holiday_date DATE NOT NULL UNIQUE,
  name TEXT NOT NULL,
  year INTEGER NOT NULL,
  is_auto BOOLEAN NOT NULL DEFAULT FALSE,
  local_name TEXT,
  CONSTRAINT holidays_year_matches_date
    CHECK (year = EXTRACT(YEAR FROM holiday_date)::INTEGER)
);

CREATE TABLE IF NOT EXISTS reopen_requests (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    week_start DATE NOT NULL,
    approver_id BIGINT NOT NULL REFERENCES users(id),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending','approved','auto_approved','rejected')),
    reviewed_at TIMESTAMPTZ,
    rejection_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CHECK (EXTRACT(ISODOW FROM week_start) = 1)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_reopen_requests_pending_unique
    ON reopen_requests(user_id, week_start)
    WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_reopen_requests_approver_status
    ON reopen_requests(approver_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reopen_requests_user
    ON reopen_requests(user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS notifications (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT,
    reference_type TEXT,
    reference_id BIGINT,
    is_read BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_notifications_user_unread
    ON notifications(user_id, is_read, created_at DESC);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO app_settings(key, value)
VALUES ('ui_language', 'en')
ON CONFLICT (key) DO NOTHING;

INSERT INTO app_settings(key, value)
VALUES ('carryover_expiry_date', '03-31')
ON CONFLICT (key) DO NOTHING;

CREATE TABLE IF NOT EXISTS audit_log (
  id BIGSERIAL PRIMARY KEY,
  user_id BIGINT NOT NULL REFERENCES users(id),
  action TEXT NOT NULL,
  table_name TEXT NOT NULL,
  record_id BIGINT NOT NULL,
  before_data TEXT,
  after_data TEXT,
  occurred_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_audit ON audit_log(table_name, record_id);

CREATE TABLE IF NOT EXISTS password_reset_tokens (
    token_hash  TEXT        PRIMARY KEY,
    user_id     BIGINT      NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_expires_at
    ON password_reset_tokens(expires_at);

CREATE TABLE IF NOT EXISTS user_annual_leave (
    user_id BIGINT  NOT NULL REFERENCES users(id),
    year    INTEGER NOT NULL CHECK (year >= 2000 AND year <= 2100),
    days    BIGINT  NOT NULL CHECK (days >= 0 AND days <= 366),
    PRIMARY KEY (user_id, year)
);

-- PostgreSQL requires index expressions to be IMMUTABLE. Casting timestamptz to
-- date is only STABLE (it depends on the session timezone), so we use a thin
-- IMMUTABLE wrapper that pins the conversion to UTC.
CREATE OR REPLACE FUNCTION notifications_created_date(ts TIMESTAMPTZ)
RETURNS DATE
LANGUAGE sql
IMMUTABLE
AS $$ SELECT (ts AT TIME ZONE 'UTC')::date $$;

CREATE UNIQUE INDEX IF NOT EXISTS uq_notifications_reminder_daily
    ON notifications (user_id, kind, notifications_created_date(created_at))
    WHERE kind = 'submission_reminder';

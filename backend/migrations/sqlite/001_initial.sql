CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT NOT NULL UNIQUE CHECK (length(email) <= 254),
  password_hash TEXT NOT NULL,
  first_name TEXT NOT NULL,
  last_name TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('employee','team_lead','admin')),
  weekly_hours REAL NOT NULL CHECK (weekly_hours >= 0 AND weekly_hours <= 168),
  annual_leave_days INTEGER NOT NULL CHECK (annual_leave_days >= 0 AND annual_leave_days <= 366),
  start_date TEXT NOT NULL,
  active INTEGER NOT NULL DEFAULT 1,
  must_change_password INTEGER NOT NULL DEFAULT 0,
  approver_id INTEGER REFERENCES users(id),
  allow_reopen_without_approval INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  CONSTRAINT users_employee_has_approver
    CHECK (role <> 'employee' OR active = 0 OR approver_id IS NOT NULL),
  CONSTRAINT users_approver_not_self
    CHECK (approver_id IS NULL OR approver_id <> id)
);
CREATE INDEX IF NOT EXISTS idx_users_approver ON users(approver_id);

CREATE TABLE IF NOT EXISTS sessions (
  token TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  last_active_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  csrf_token TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_sessions_active ON sessions(last_active_at);

CREATE TABLE IF NOT EXISTS login_attempts (
  email TEXT NOT NULL,
  attempted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  success INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_login_attempts_email ON login_attempts(email, attempted_at);

CREATE TABLE IF NOT EXISTS categories (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  color TEXT NOT NULL CHECK (length(color) = 7),
  sort_order INTEGER NOT NULL DEFAULT 0,
  active INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS time_entries (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  entry_date TEXT NOT NULL,
  start_time TEXT NOT NULL,
  end_time TEXT NOT NULL,
  category_id INTEGER NOT NULL REFERENCES categories(id),
  comment TEXT,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','submitted','approved','rejected')),
  submitted_at TEXT,
  reviewed_by INTEGER REFERENCES users(id),
  reviewed_at TEXT,
  rejection_reason TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_te_user_date ON time_entries(user_id, entry_date);
CREATE INDEX IF NOT EXISTS idx_te_status_date ON time_entries(status, entry_date DESC);

CREATE TABLE IF NOT EXISTS absences (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  kind TEXT NOT NULL CHECK (kind IN ('vacation','sick','training','special_leave','unpaid','general_absence')),
  start_date TEXT NOT NULL,
  end_date TEXT NOT NULL,
  comment TEXT,
  status TEXT NOT NULL DEFAULT 'requested' CHECK (status IN ('requested','approved','rejected','cancelled')),
  reviewed_by INTEGER REFERENCES users(id),
  reviewed_at TEXT,
  rejection_reason TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  CHECK (end_date >= start_date)
);
CREATE INDEX IF NOT EXISTS idx_abs_user ON absences(user_id, start_date, end_date);
CREATE INDEX IF NOT EXISTS idx_abs_status_date ON absences(status, start_date DESC);

CREATE TABLE IF NOT EXISTS change_requests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  time_entry_id INTEGER NOT NULL REFERENCES time_entries(id),
  user_id INTEGER NOT NULL REFERENCES users(id),
  new_date TEXT,
  new_start_time TEXT,
  new_end_time TEXT,
  new_category_id INTEGER REFERENCES categories(id),
  new_comment TEXT,
  reason TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','approved','rejected')),
  reviewed_by INTEGER REFERENCES users(id),
  reviewed_at TEXT,
  rejection_reason TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_cr_user_created ON change_requests(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_cr_status_created ON change_requests(status, created_at DESC);

CREATE TABLE IF NOT EXISTS holidays (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  holiday_date TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  year INTEGER NOT NULL,
  is_auto INTEGER NOT NULL DEFAULT 0,
  local_name TEXT
);

CREATE TABLE IF NOT EXISTS reopen_requests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  week_start TEXT NOT NULL,
  approver_id INTEGER NOT NULL REFERENCES users(id),
  status TEXT NOT NULL DEFAULT 'pending'
    CHECK (status IN ('pending','approved','auto_approved','rejected')),
  reviewed_at TEXT,
  rejection_reason TEXT,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_reopen_requests_pending_unique
  ON reopen_requests(user_id, week_start)
  WHERE status = 'pending';
CREATE INDEX IF NOT EXISTS idx_reopen_requests_approver_status
  ON reopen_requests(approver_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reopen_requests_user
  ON reopen_requests(user_id, created_at DESC);

CREATE TABLE IF NOT EXISTS notifications (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  body TEXT,
  reference_type TEXT,
  reference_id INTEGER,
  is_read INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_notifications_user_unread
  ON notifications(user_id, is_read, created_at DESC);

CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO app_settings(key, value)
VALUES ('ui_language', 'en')
ON CONFLICT (key) DO NOTHING;

INSERT INTO app_settings(key, value) VALUES ('country', 'DE') ON CONFLICT (key) DO NOTHING;
INSERT INTO app_settings(key, value) VALUES ('region', 'DE-BW') ON CONFLICT (key) DO NOTHING;

CREATE TABLE IF NOT EXISTS audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  action TEXT NOT NULL,
  table_name TEXT NOT NULL,
  record_id INTEGER NOT NULL,
  before_data TEXT,
  after_data TEXT,
  occurred_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_audit ON audit_log(table_name, record_id)

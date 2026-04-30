CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  email TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  first_name TEXT NOT NULL,
  last_name TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('employee','team_lead','admin')),
  weekly_hours REAL NOT NULL,
  annual_leave_days INTEGER NOT NULL,
  start_date DATE NOT NULL,
  active BOOLEAN NOT NULL DEFAULT 1,
  must_change_password BOOLEAN NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sessions (
  token TEXT PRIMARY KEY,
  user_id INTEGER NOT NULL REFERENCES users(id),
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  last_active_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS login_attempts (
  email TEXT NOT NULL,
  attempted_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  success BOOLEAN NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_login_attempts_email ON login_attempts(email, attempted_at);

CREATE TABLE IF NOT EXISTS categories (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  description TEXT,
  color TEXT NOT NULL,
  sort_order INTEGER NOT NULL DEFAULT 0,
  active BOOLEAN NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS time_entries (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  entry_date DATE NOT NULL,
  start_time TEXT NOT NULL,
  end_time TEXT NOT NULL,
  category_id INTEGER NOT NULL REFERENCES categories(id),
  comment TEXT,
  status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft','submitted','approved','rejected')),
  submitted_at DATETIME,
  reviewed_by INTEGER REFERENCES users(id),
  reviewed_at DATETIME,
  rejection_reason TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_te_user_date ON time_entries(user_id, entry_date);

CREATE TABLE IF NOT EXISTS absences (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  kind TEXT NOT NULL CHECK (kind IN ('vacation','sick','training','special_leave','unpaid')),
  start_date DATE NOT NULL,
  end_date DATE NOT NULL,
  half_day BOOLEAN NOT NULL DEFAULT 0,
  comment TEXT,
  status TEXT NOT NULL DEFAULT 'requested' CHECK (status IN ('requested','approved','rejected','cancelled')),
  reviewed_by INTEGER REFERENCES users(id),
  reviewed_at DATETIME,
  rejection_reason TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_abs_user ON absences(user_id, start_date, end_date);

CREATE TABLE IF NOT EXISTS change_requests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  time_entry_id INTEGER NOT NULL REFERENCES time_entries(id),
  user_id INTEGER NOT NULL REFERENCES users(id),
  new_date DATE,
  new_start_time TEXT,
  new_end_time TEXT,
  new_category_id INTEGER REFERENCES categories(id),
  new_comment TEXT,
  reason TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','approved','rejected')),
  reviewed_by INTEGER REFERENCES users(id),
  reviewed_at DATETIME,
  rejection_reason TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS holidays (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  holiday_date DATE NOT NULL UNIQUE,
  name TEXT NOT NULL,
  year INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  user_id INTEGER NOT NULL REFERENCES users(id),
  action TEXT NOT NULL,
  table_name TEXT NOT NULL,
  record_id INTEGER NOT NULL,
  before_data TEXT,
  after_data TEXT,
  occurred_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_audit ON audit_log(table_name, record_id);

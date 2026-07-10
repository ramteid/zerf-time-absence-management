ALTER TABLE timesheet_export_queue
  ADD COLUMN IF NOT EXISTS requires_start_date_review BOOLEAN NOT NULL DEFAULT FALSE;

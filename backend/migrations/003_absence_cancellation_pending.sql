-- Add 'cancellation_pending' as a valid absence status.
-- This allows users to request cancellation of an approved absence;
-- a team lead must then approve or reject the cancellation.
ALTER TABLE absences DROP CONSTRAINT IF EXISTS absences_status_check;
ALTER TABLE absences ADD CONSTRAINT absences_status_check
  CHECK (status IN ('requested','approved','rejected','cancelled','cancellation_pending'));

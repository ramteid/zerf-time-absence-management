-- Tighten the reporting model: every non-admin user must have an explicit
-- approver. The approver target remains validated by application logic as an
-- active team lead or admin.
DO $$
BEGIN
  IF EXISTS (
    SELECT 1 FROM users WHERE role <> 'admin' AND approver_id IS NULL
  ) THEN
    RAISE EXCEPTION
      'Cannot add users_non_admin_has_approver: assign approver_id for all non-admin users first.';
  END IF;
END $$;

ALTER TABLE users DROP CONSTRAINT IF EXISTS users_employee_has_approver;
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_non_admin_has_approver;
ALTER TABLE users
  ADD CONSTRAINT users_non_admin_has_approver
  CHECK (role = 'admin' OR approver_id IS NOT NULL);

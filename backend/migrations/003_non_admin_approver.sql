-- Tighten the reporting model: every non-admin user must have an explicit
-- approver. Existing orphaned non-admin users are assigned to the oldest active
-- admin, matching the previous fallback approval path.
DO $$
DECLARE
  fallback_admin_id BIGINT;
BEGIN
  IF EXISTS (
    SELECT 1 FROM users WHERE role <> 'admin' AND approver_id IS NULL
  ) THEN
    SELECT id INTO fallback_admin_id
    FROM users
    WHERE role = 'admin' AND active = TRUE
    ORDER BY id
    LIMIT 1;

    IF fallback_admin_id IS NULL THEN
      RAISE EXCEPTION
        'Cannot add users_non_admin_has_approver: no active admin exists for orphaned non-admin users.';
    END IF;

    UPDATE users
    SET approver_id = fallback_admin_id
    WHERE role <> 'admin' AND approver_id IS NULL;
  END IF;
END $$;

ALTER TABLE users DROP CONSTRAINT IF EXISTS users_employee_has_approver;
ALTER TABLE users DROP CONSTRAINT IF EXISTS users_non_admin_has_approver;
ALTER TABLE users
  ADD CONSTRAINT users_non_admin_has_approver
  CHECK (role = 'admin' OR approver_id IS NOT NULL);

-- Allow multiple active password-reset tokens per user to prevent overwrite DoS.
-- Previously user_id was UNIQUE, so triggering forgot-password invalidated existing link.
-- With multiple tokens, old links stay valid until expiry; cleanup removes expired.
-- Migration is idempotent: drop the unique constraint/index if present.
DO $$
BEGIN
    -- Drop unique constraint on user_id if it exists (default name password_reset_tokens_user_id_key)
    IF EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'password_reset_tokens_user_id_key'
    ) THEN
        ALTER TABLE password_reset_tokens DROP CONSTRAINT password_reset_tokens_user_id_key;
    END IF;
    -- Also drop if created as unique index
    IF EXISTS (
        SELECT 1 FROM pg_indexes WHERE indexname = 'password_reset_tokens_user_id_key'
    ) THEN
        DROP INDEX IF EXISTS password_reset_tokens_user_id_key;
    END IF;
    -- Recreate as non-unique index for lookup performance (if not exists)
    CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_user_id ON password_reset_tokens(user_id);
END $$;

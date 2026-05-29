# Security

## Backend

The backend is written in Rust, which eliminates memory-safety vulnerabilities (buffer overflows, use-after-free, data races) at compile time.

## Authentication

- **Password hashing**: Argon2id with per-user random salts.
- **Brute-force protection**: accounts lock for 15 minutes after 5 consecutive failed login attempts.
- **Password reset**: one-time tokens with a 1-hour expiry; users are forced to change their password on first login.

## Sessions

- Tokens are 256-bit cryptographically random values (CSPRNG).
- Stored as HttpOnly, Secure, SameSite=Strict cookies — not accessible to JavaScript.
- **Idle timeout**: 8 hours of inactivity.
- **Absolute timeout**: 7 days regardless of activity.
- Expired sessions and login attempts are purged hourly by a background task.

## CSRF Protection

Three independent layers:

1. `SameSite=Strict` on the session cookie.
2. `Origin`/`Referer` header validation on state-mutating requests.
3. `X-CSRF-Token` double-submit: a per-session CSRF token is returned on login and must accompany every mutating request.

## Data Encryption

### Database at rest

All PostgreSQL tables and WAL segments are transparently encrypted at the storage layer using [pg_tde](https://docs.percona.com/pg-tde/) (Percona Transparent Data Encryption). The pg_tde principal key is derived from `ZERF_DB_ENCRYPTION_KEY` and stored encrypted on disk. On container start the custom entrypoint decrypts the key blob into an in-memory tmpfs; the plaintext key is never written to disk.

### Backups

Every backup file (`.dump.enc`) is encrypted with AES-256-CBC (PBKDF2, 100 000 iterations) before being written to the backup volume. The same `ZERF_DB_ENCRYPTION_KEY` is used for both layers — losing it makes both the live database and all backups permanently unreadable.

## Audit Log

Every create, update, and delete operation is recorded in `audit_log` with before/after JSON snapshots of the affected row. Passwords, session tokens, and other secrets are never included in snapshots.

## Network Isolation

The PostgreSQL container is attached only to an internal Docker network and has no published ports. It is not reachable from the host or the internet — only the application container can connect to it.

## HTTPS

In the public deployment, Caddy handles TLS termination (automatic certificate provisioning via Let's Encrypt) and forwards requests to the backend over the internal Docker network. The backend runs behind Caddy and is not directly exposed.

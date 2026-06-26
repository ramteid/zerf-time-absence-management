# Upgrading Zerf

Operator-facing notes for upgrading an existing deployment. Read the section for
your target version before pulling a new image.

---

## Breaking change: PostgreSQL data directory now lives in a named volume

### What changed

Earlier releases mounted only `postgres_data:/data` for the database container.
The Percona PostgreSQL image declares `VOLUME [/data/db]` in its Dockerfile, so
Docker created a **fresh anonymous volume at `/data/db`** (the actual PGDATA) on
every container **recreation** — `docker compose pull && up`, `down && up`, a
container-name conflict, certain daemon changes, and so on. Each recreation
orphaned the previous data directory and started PostgreSQL on an empty one.

From this release the compose file mounts an explicit named volume
`zerf_postgres_db_data` at `/data/db`, so PGDATA is stable across recreations.

### ⚠️ Action required when upgrading an existing deployment

Your live data is still in the old anonymous volume. Because the upgrade switches
`/data/db` to the new (empty) named volume, you must copy your data across
**before** starting the new version — otherwise PostgreSQL comes up empty.

1. Find the anonymous volume your running container uses for PGDATA:

   ```bash
   docker inspect zerf-postgres \
     --format '{{range .Mounts}}{{if eq .Destination "/data/db"}}{{.Name}}{{end}}{{end}}'
   ```

2. Stop the stack (use your usual compose files):

   ```bash
   docker compose -f docker/docker-compose-local.yml --env-file .env down
   ```

3. Create the named volume and copy the data into it (replace `<ANON_VOL>` with
   the volume name from step 1):

   ```bash
   docker volume create zerf_postgres_db_data
   docker run --rm -v <ANON_VOL>:/src:ro -v zerf_postgres_db_data:/dst \
     alpine:3 sh -c 'cp -a /src/. /dst/'
   ```

4. Pull and start the new version:

   ```bash
   ./start_public.sh   # or ./start_local.sh
   ```

The source volume is never modified, so you can retry the copy if needed.

---

## Recovery: if you already upgraded and the database looks empty

**Your data is most likely NOT lost.** It is orphaned in the old anonymous
volume, and it is still decryptable because the pg_tde keyring
(`pg_tde_keyring.enc`, stored in the `zerf_postgres_data` volume) and your
`ZERF_DB_ENCRYPTION_KEY` are unchanged.

### Do this first — do not make it worse

- **Do NOT** delete or regenerate `pg_tde_keyring.enc`. Without the original
  keyring, the encrypted data directory is **permanently unrecoverable** — the
  pg_tde principal key only exists inside that keyring.
- **Do NOT** run `docker volume prune` — it would delete the orphaned volume that
  still holds your data.

### Recover the orphaned data directory

1. List volumes and identify the orphaned PGDATA. It contains `base/`,
   `pg_wal/`, and `PG_VERSION`; if several candidates exist, pick the one with
   the most recent modification time:

   ```bash
   docker volume ls
   docker run --rm -v <VOL>:/v:ro alpine:3 \
     sh -c 'cat /v/PG_VERSION; ls -lt --full-time /v/base | head'
   ```

2. Stop the stack, then copy that volume into `zerf_postgres_db_data` (same copy
   command as step 3 above).

3. Start the stack again and verify you can log in.

### If you also lost the keyring

If the keyring itself was deleted or overwritten (so the orphaned data directory
no longer decrypts), recover it from a backup. Since each backup also captures
the keyring (`zerf-<ts>.keyring.enc`), extract the one matching the orphaned data
directory's era and place it back as `pg_tde_keyring.enc` in the
`zerf_postgres_data` volume:

```bash
./scripts/restore.sh --keyring        # writes the chosen keyring to the cwd
```

Do **not** overwrite a keyring that still works — if the live database starts and
decrypts, its current keyring is the correct one.

### If the data directory is truly gone

Restore the most recent backup instead — backups are independent of the keyring
(they are logical dumps encrypted with `ZERF_DB_ENCRYPTION_KEY`):

```bash
./scripts/restore.sh
```

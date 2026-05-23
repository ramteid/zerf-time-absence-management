# Backend Architecture

## Overview

The backend is a single-crate Axum + SQLx application structured in three layers:

```
handlers → services → repository
```

- **Handlers** (`src/handlers/`): HTTP only. Extract request data, call services, return responses.
- **Services** (`src/services/`): Business logic. Own transactions, dispatch notifications.
- **Repository** (`src/repository/`): SQL only. No business rules.

Supporting modules:

- **Middleware** (`src/middleware/`): Auth middleware, `User` struct, session helpers.
- **Background** (`src/background/`): Scheduled loops (reminders, holidays).
- **State** (`src/state.rs`): `AppState` definition.
- **Router** (`src/router.rs`): Route declarations, `build_app`.

---

## Directory Layout

```
backend/src/
├── lib.rs                      # Module declarations + pub use re-exports only
├── main.rs                     # init, spawn background tasks, serve
├── state.rs                    # AppState struct
├── router.rs                   # build_api_router(), build_app()
│
├── config.rs                   # Environment config (unchanged)
├── db.rs                       # DatabasePool, migration runner (unchanged)
├── error.rs                    # AppError, AppResult (unchanged)
├── roles.rs                    # Role helpers (unchanged)
├── time_calc.rs                # Time duration helpers (unchanged)
├── i18n.rs                     # Translations, date formatting (unchanged)
├── audit.rs                    # Audit log dispatch (unchanged)
├── email.rs                    # Lettre SMTP wrapper (unchanged)
│
├── middleware/
│   ├── mod.rs
│   └── auth.rs                 # auth_middleware, User struct, cookie/token/CSRF helpers
│
├── handlers/                   # HTTP only — no sqlx, no business rules
│   ├── mod.rs
│   ├── absences.rs
│   ├── auth.rs
│   ├── categories.rs
│   ├── holidays.rs
│   ├── notifications.rs
│   ├── reopen_requests.rs
│   ├── reports.rs
│   ├── settings.rs
│   ├── time_entries.rs
│   └── users.rs
│
├── services/                   # Business logic — no axum HTTP types
│   ├── mod.rs
│   ├── absence_balance.rs      # Vacation balance, workdays, carryover, date validation
│   ├── absences.rs             # Absence lifecycle workflows
│   ├── auth.rs                 # Password hashing, session CRUD, token gen, cleanup_loop
│   ├── categories.rs           # ensure_initial (startup fn)
│   ├── holidays.rs             # Nager.Date API fetch, ensure_holidays, region helpers
│   ├── notifications.rs        # create, send_email, broadcaster, cleanup_old
│   ├── reopen_requests.rs      # Reopen workflow, auto-approval, notification dispatch
│   ├── reports.rs              # Report building, aggregation, CSV serialization
│   ├── settings.rs             # Settings validation, SMTP config, timezone helpers
│   ├── time_entries.rs         # Submit, batch-approve/reject workflows
│   └── users.rs                # User CRUD logic, role checks, password reset
│
├── background/                 # Scheduled loops spawned from main.rs
│   ├── mod.rs
│   ├── submission_reminders.rs # Wakes at 07:00 on the configured deadline day
│   ├── approval_reminders.rs   # Wakes every Monday at 07:00
│   └── holidays.rs             # Wakes every Monday at 12:00 to seed next-year holidays
│
└── repository/                 # SQL layer — pure SQL, no business rules
    ├── mod.rs                  # Db façade
    ├── absences.rs
    ├── audit.rs
    ├── categories.rs
    ├── holidays.rs
    ├── notifications.rs
    ├── reopen_requests.rs
    ├── reports.rs
    ├── sessions.rs
    ├── settings.rs
    ├── system_metadata.rs
    ├── time_entries.rs
    └── users.rs
```

---

## Layer Contracts

### Handlers (`src/handlers/*.rs`)

- **Allowed imports**: `axum`, `serde`, `crate::AppState`, `crate::error`, `crate::middleware::auth::User`, `crate::services`
- **Forbidden imports**: `sqlx`, `crate::repository`
- **Pattern**: extract → call `services::domain::fn(...)` → return `Json(result)`
- Define `#[derive(Deserialize)]` request structs and `#[derive(Serialize)]` response types in the handler file where they are used

### Services (`src/services/*.rs`)

- **Allowed imports**: `crate::AppState`, `crate::repository`, `crate::error`, `crate::i18n`, `crate::audit`, `crate::email`, `crate::config`
- **Forbidden imports**: `axum::extract`, `axum::response`, `axum::routing`, `axum::Json`
- Return `AppResult<T>` — never `impl IntoResponse` or HTTP status codes
- Own the transaction lifecycle: `begin()` → work → `commit()`
- Dispatch notifications **after** committing the transaction

### Middleware (`src/middleware/auth.rs`)

- Single source of the `User` type — all other modules import from `crate::middleware::auth`
- No business logic: token extraction, DB session lookup, user hydration only

### Repository (`src/repository/`)

- Pure SQL only — no `AppError::BadRequest`, no business rules
- Only produces `AppError::NotFound` via `From<sqlx::Error>`

---

## Design Principles

These principles apply to all future changes:

1. **Move code, don't rewrite it.** Fix only import paths when moving. Rewriting logic hides bugs and balloons diffs.
2. **No new abstractions.** Do not introduce traits, generics, or patterns not already present. Concrete functions beat abstract helpers.
3. **Don't split what doesn't need splitting.** Only split files that are genuinely unmanageable. Flat is better than nested.
4. **Explicit over clever.** A 5-line guard at the top of a function beats a generic helper used once.
5. **Keep existing logic intact.** Refactors move code — they don't improve business rules. Rule changes are a separate task.
6. **No new error types.** Keep `AppError` as-is. Typed error variants add complexity without benefit at this scale.
7. **Comments over extraction.** A business rule that fits in 5 lines gets a `//` comment — not a named helper called once.
8. **No performance micro-optimisations.** Don't change `clone()` patterns, reference lifetimes, or async patterns unless the current code is clearly wrong.
9. **Maintainability first.** Prefer boring, readable code. The simplest structure that compiles and passes tests is the right one.
10. **No code in `mod.rs` or `lib.rs`.** These files contain only `pub mod` declarations and `pub use` re-exports.

---

## Verification Checklist

Run after every significant change:

```bash
# 1. Zero compilation errors
cargo build

# 2. Zero clippy warnings
cargo clippy -- -D warnings

# 3. Unit tests (no Docker needed)
cargo test --lib

# 4. Integration tests (requires Docker or TEST_DATABASE_URL)
cargo test

# 5. No HTTP types in services layer
grep -rn "axum::extract\|axum::response\|axum::routing\|axum::Json" backend/src/services/
# Expected: no output

# 6. No direct sqlx usage in handlers
grep -rn "sqlx::" backend/src/handlers/
# Expected: no output

# 7. No business logic in repository (spot-check)
grep -rn "AppError::BadRequest\|AppError::Forbidden\|AppError::Conflict" backend/src/repository/
# Expected: no output
```

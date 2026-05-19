//! Zerf integration tests.
//!
//! Tests are organized by domain area. Each module focuses on a specific
//! feature or API surface. The `full_suite` module contains an end-to-end
//! sequential test that exercises the full happy path in a single container.
//!
//! # Requirements
//!
//! By default, a Docker daemon must be available for testcontainers to spin
//! up Postgres.
//!
//! If Docker is not available, start a local PostgreSQL instance manually and
//! set `TEST_DATABASE_URL` to a local Postgres admin database URL (for example,
//! `postgres://postgres:postgres@127.0.0.1:5432/postgres`). The test harness
//! then skips containers, creates an isolated database on that Postgres
//! instance, and runs migrations there.
//!
//! ```sh
//! cargo test --test integration
//! ```

#[path = "../common/mod.rs"]
mod common;
mod helpers;

mod absences;
mod audit;
mod admin;
mod approval_reminders;
mod auth;
mod carryover;
mod categories;
mod full_suite;
mod holidays;
mod notifications;
mod repository_paths;
mod reopen;
mod reports;
mod start_date;
mod submission_reminders;
mod team_settings;
mod time_entries;
mod tracks_time;
mod users;

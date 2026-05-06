//! Zerf integration tests.
//!
//! Tests are organized by domain area. Each module focuses on a specific
//! feature or API surface. The `full_suite` module contains an end-to-end
//! sequential test that exercises the full happy path in a single container.
//!
//! # Requirements
//!
//! A Docker daemon must be available for testcontainers to spin up Postgres.
//!
//! ```sh
//! cargo test --test integration
//! ```

#[path = "../common/mod.rs"]
mod common;
mod helpers;

mod absences;
mod admin;
mod auth;
mod change_requests;
mod full_suite;
mod notifications;
mod reopen;
mod reports;
mod start_date;
mod submission_reminders;
mod team_settings;
mod time_entries;
mod users;

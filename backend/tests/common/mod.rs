//! Shared test infrastructure for Zerf integration tests.
//!
//! Provides [`TestApp`] which spins up an ephemeral Postgres container via
//! testcontainers, runs migrations, seeds initial data, and starts the Axum
//! server on a random port. If `TEST_DATABASE_URL` is set, the harness skips
//! containers and provisions isolated databases on that existing, manually
//! started Postgres instance instead. Each test session gets a fully isolated
//! database.

pub mod app;
pub mod client;
pub mod helpers;

pub use app::TestApp;
pub use client::TestClient;

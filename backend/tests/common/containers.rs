//! The Postgres containers the integration tests run against, and the clean-up
//! that keeps them from piling up.
//!
//! testcontainers removes a container when its handle is dropped, which covers
//! every test that passes or panics. Three ways around that once left over a
//! hundred stopped containers, and their data volumes, on a developer machine:
//!
//! - a run interrupted by Ctrl-C or a timeout (SIGINT, SIGTERM) never drops the
//!   handles of the tests still running;
//! - a run that is killed outright (SIGKILL, out of memory, a reboot) cannot run
//!   any code at all;
//! - a container that is created but then fails to start never gets a handle.
//!
//! Each gets its own answer. The `watchdog` feature of testcontainers removes
//! the run's containers on SIGINT, SIGTERM and SIGQUIT. The data directory is a
//! tmpfs, so no container can leave a volume behind whatever happens to it.
//! And every run first removes the containers of earlier runs whose test
//! process is gone, which catches whatever the other two could not.

use std::path::Path;
use std::process::Command;
use std::sync::Once;

use testcontainers::core::Mount;
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, ImageExt};
use testcontainers_modules::postgres::Postgres;

/// Marks a container as one of these tests', so the sweep never touches
/// anything else on the machine.
const TEST_CONTAINER_LABEL: &str = "zerf.integration-test";
/// The test process that owns the container, as `<pid namespace>/<pid>`.
const OWNER_LABEL: &str = "zerf.integration-test.owner";

/// Start the Postgres container one test runs against.
pub async fn start_postgres() -> ContainerAsync<Postgres> {
    remove_leftovers_of_finished_runs_once();
    Postgres::default()
        .with_tag("17")
        .with_label(TEST_CONTAINER_LABEL, "true")
        .with_label(OWNER_LABEL, owner_of_this_process().unwrap_or_default())
        // In memory: a test database is thrown away anyway, and a tmpfs leaves
        // no volume behind even when the container itself is left over.
        .with_mount(Mount::tmpfs_mount("/var/lib/postgresql/data"))
        .start()
        .await
        .expect("failed to start Postgres container")
}

fn remove_leftovers_of_finished_runs_once() {
    static SWEEP: Once = Once::new();
    SWEEP.call_once(remove_leftovers_of_finished_runs);
}

/// Remove the test containers whose owning test process no longer exists.
///
/// A container is only removed when its owner ran in this same PID namespace
/// and that PID is gone. A run in another namespace (a dev container sharing
/// the Docker daemon) cannot be checked from here and is left alone, and so is
/// every run still in progress, such as a second test run started in parallel.
/// Best effort: without the Docker CLI or `/proc` this does nothing, and the
/// tests run as before.
fn remove_leftovers_of_finished_runs() {
    let Some(own_owner) = owner_of_this_process() else {
        return;
    };
    let Some((own_namespace, _)) = own_owner.split_once('/') else {
        return;
    };
    let listing = Command::new("docker")
        .args([
            "ps",
            "--all",
            "--filter",
            &format!("label={TEST_CONTAINER_LABEL}"),
            "--format",
            &format!("{{{{.ID}}}} {{{{.Label \"{OWNER_LABEL}\"}}}}"),
        ])
        .output();
    let Ok(listing) = listing else {
        return;
    };
    if !listing.status.success() {
        return;
    }
    let orphaned: Vec<String> = String::from_utf8_lossy(&listing.stdout)
        .lines()
        .filter_map(|line| {
            let (container_id, owner) = line.split_once(' ')?;
            let (namespace, pid) = owner.split_once('/')?;
            let owner_is_gone =
                namespace == own_namespace && !Path::new(&format!("/proc/{pid}")).exists();
            owner_is_gone.then(|| container_id.to_string())
        })
        .collect();
    if orphaned.is_empty() {
        return;
    }
    let _ = Command::new("docker")
        .args(["rm", "--force", "--volumes"])
        .args(&orphaned)
        .output();
}

/// This process as `<pid namespace>/<pid>`, or `None` where `/proc` is missing.
fn owner_of_this_process() -> Option<String> {
    let namespace = std::fs::read_link("/proc/self/ns/pid").ok()?;
    Some(format!(
        "{}/{}",
        namespace.to_string_lossy(),
        std::process::id()
    ))
}

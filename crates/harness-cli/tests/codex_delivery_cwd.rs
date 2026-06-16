//! Integration coverage for Codex persistent-delivery cwd (goal-multi-project P3,
//! Stage 3 — `codex-delivery-cwd` task).
//!
//! A persistent Codex worker must run from the SELECTED project's `project_root`
//! (where its `AGENTS.md` lives), NOT the long-running harness process cwd — a
//! `serve` that switched projects never `cd`s. When the member is pinned to a
//! specific `worktree_ref`, that pin still wins.
//!
//! These tests run the real `harness` binary FROM A CWD DIFFERENT FROM THE
//! PROJECT ROOT, with a fake `codex` on PATH that records the cwd it was spawned
//! in. We assert on the recorded cwd, not on delivery success.

#![cfg(unix)]

use std::path::Path;

mod fake_provider;
mod harness_env;

use fake_provider::{install_provider_shim, read_recorded_cwd, DeliveryDriver};
use harness_env::TempHome;

/// Canonicalize for a stable comparison against the shim's `pwd -P` output.
fn canon(p: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[test]
fn codex_delivery_without_worktree_runs_in_project_root_not_harness_cwd() {
    let home = TempHome::new("codex-cwd");
    let project_root = home.base().join("myproj");
    std::fs::create_dir_all(&project_root).unwrap();
    // Deliberately run the harness from a DIFFERENT cwd than the project root.
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let marker = home.base().join("codex_cwd.txt");
    let fake_bin = install_provider_shim(home.base(), "codex", &marker);

    let driver = DeliveryDriver::new(&project_root, &elsewhere, home.envs(), &fake_bin);
    driver.init_project();
    let member_id = driver.create_member("codex", None);
    driver.send_message(&member_id, "hello codex");
    let _ = driver.deliver(&member_id);

    let recorded = read_recorded_cwd(&marker);
    assert_eq!(
        recorded,
        canon(&project_root),
        "Codex must run in the selected project_root"
    );
    assert_ne!(
        recorded,
        canon(&elsewhere),
        "Codex must NOT run in the harness process cwd"
    );
}

#[test]
fn codex_delivery_with_worktree_keeps_the_pinned_workspace() {
    let home = TempHome::new("codex-pin");
    let project_root = home.base().join("myproj");
    std::fs::create_dir_all(&project_root).unwrap();
    let pinned = home.base().join("pinned-workspace");
    std::fs::create_dir_all(&pinned).unwrap();
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let marker = home.base().join("codex_cwd.txt");
    let fake_bin = install_provider_shim(home.base(), "codex", &marker);

    let driver = DeliveryDriver::new(&project_root, &elsewhere, home.envs(), &fake_bin);
    driver.init_project();
    let member_id = driver.create_member("codex", Some(&pinned));
    driver.send_message(&member_id, "hello codex");
    let _ = driver.deliver(&member_id);

    let recorded = read_recorded_cwd(&marker);
    assert_eq!(
        recorded,
        canon(&pinned),
        "member.worktree_ref must pin the workspace and win over project_root"
    );
    assert_ne!(
        recorded,
        canon(&project_root),
        "the pin must override the project_root default"
    );
}

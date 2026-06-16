//! Integration coverage for Claude persistent-delivery cwd (goal-multi-project
//! P3, Stage 3 — `claude-delivery-cwd` task).
//!
//! A persistent Claude worker must run from the SELECTED project's `project_root`
//! (where its `CLAUDE.md` / `.claude/` live and Claude Code keys per-project
//! memory), NOT the long-running harness process cwd — a `serve` that switched
//! projects never `cd`s. When the member is pinned to a `worktree_ref`, that pin
//! still wins.
//!
//! These tests run the real `harness` binary FROM A CWD DIFFERENT FROM THE
//! PROJECT ROOT, with a fake `claude` on PATH that records the cwd it was spawned
//! in (and, in the marker test, the `CLAUDE.md` it could read from that cwd).

#![cfg(unix)]

use std::path::Path;

mod fake_provider;
mod harness_env;

use fake_provider::{
    install_provider_shim, install_provider_shim_capturing, read_recorded_cwd, DeliveryDriver,
};
use harness_env::TempHome;

fn canon(p: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[test]
fn claude_delivery_without_worktree_runs_in_project_root_not_harness_cwd() {
    let home = TempHome::new("claude-cwd");
    let project_root = home.base().join("myproj");
    std::fs::create_dir_all(&project_root).unwrap();
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let marker = home.base().join("claude_cwd.txt");
    let fake_bin = install_provider_shim(home.base(), "claude", &marker);

    let driver = DeliveryDriver::new(&project_root, &elsewhere, home.envs(), &fake_bin);
    driver.init_project();
    let member_id = driver.create_member("claude", None);
    driver.send_message(&member_id, "hello claude");
    let _ = driver.deliver(&member_id);

    let recorded = read_recorded_cwd(&marker);
    assert_eq!(
        recorded,
        canon(&project_root),
        "Claude must run in the selected project_root"
    );
    assert_ne!(
        recorded,
        canon(&elsewhere),
        "Claude must NOT run in the harness process cwd"
    );
}

#[test]
fn claude_delivery_with_worktree_keeps_the_pinned_workspace() {
    let home = TempHome::new("claude-pin");
    let project_root = home.base().join("myproj");
    std::fs::create_dir_all(&project_root).unwrap();
    let pinned = home.base().join("pinned-workspace");
    std::fs::create_dir_all(&pinned).unwrap();
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let marker = home.base().join("claude_cwd.txt");
    let fake_bin = install_provider_shim(home.base(), "claude", &marker);

    let driver = DeliveryDriver::new(&project_root, &elsewhere, home.envs(), &fake_bin);
    driver.init_project();
    let member_id = driver.create_member("claude", Some(&pinned));
    driver.send_message(&member_id, "hello claude");
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

#[test]
fn claude_delivery_sees_the_selected_projects_claude_md_marker() {
    // The whole point of cwd routing: a worker started for the selected project can
    // READ that project's CLAUDE.md. We drop a uniquely-marked CLAUDE.md in the
    // project root, run the harness from elsewhere, and let the fake claude copy
    // whatever CLAUDE.md it sees from ITS cwd back to a marker file.
    let home = TempHome::new("claude-claudemd");
    let project_root = home.base().join("myproj");
    std::fs::create_dir_all(&project_root).unwrap();
    let unique = "MARKER-CLAUDE-MD-7f3a91";
    std::fs::write(
        project_root.join("CLAUDE.md"),
        format!("# Project memory\n{unique}\n"),
    )
    .unwrap();
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    // A DECOY CLAUDE.md in the harness process cwd: if the worker wrongly ran here,
    // it would copy this one instead and the unique marker would be absent.
    std::fs::write(elsewhere.join("CLAUDE.md"), "# Wrong memory\nDECOY\n").unwrap();

    let cwd_marker = home.base().join("claude_cwd.txt");
    let seen_claude_md = home.base().join("seen_claude_md.txt");
    let fake_bin = install_provider_shim_capturing(
        home.base(),
        "claude",
        &cwd_marker,
        Some(("CLAUDE.md", &seen_claude_md)),
    );

    let driver = DeliveryDriver::new(&project_root, &elsewhere, home.envs(), &fake_bin);
    driver.init_project();
    let member_id = driver.create_member("claude", None);
    driver.send_message(&member_id, "read your memory");
    let _ = driver.deliver(&member_id);

    let seen = std::fs::read_to_string(&seen_claude_md)
        .unwrap_or_else(|e| panic!("claude never read a CLAUDE.md ({e}); cwd routing failed"));
    assert!(
        seen.contains(unique),
        "Claude must see the SELECTED project's CLAUDE.md (marker {unique}); saw:\n{seen}"
    );
    assert!(
        !seen.contains("DECOY"),
        "Claude must NOT read the harness-cwd decoy CLAUDE.md; saw:\n{seen}"
    );
}

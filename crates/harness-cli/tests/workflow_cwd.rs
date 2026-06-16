//! Integration coverage for the store_root / project_root split in workflow cwd
//! (goal-multi-project, `worktree-root-split` task).
//!
//! A workflow worker's shared cwd + git-worktree base must root at the selected
//! project's `project_root` (where CLAUDE.md / AGENTS.md / memory live), NOT the
//! harness process cwd and NOT the centralized `~/.harness/projects/<id>/` store.
//! A long-running `serve` never `cd`s after a project switch, so reading the
//! process cwd would run workers in the wrong tree.
//!
//! These tests run the real `harness` binary FROM A CWD DIFFERENT FROM THE
//! PROJECT ROOT and assert the workflow roots at the project, provider-free: a
//! `writable` node against a non-git project is rejected before any provider spawn
//! with a message that names the PROJECT root — which would name the cwd instead if
//! the old `env::current_dir()` behavior were still in place.

use std::path::Path;
use std::process::Command;

mod harness_env;
use harness_env::TempHome;

fn init_project(home: &TempHome, project_root: &Path) {
    let out = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--project")
        .arg(project_root)
        .arg("init")
        .current_dir(home.base())
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run init");
    assert!(out.status.success(), "init failed: {:?}", out);
}

fn write_program(dir: &Path, body: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).expect("mk program dir");
    let path = dir.join("w.star");
    std::fs::write(&path, body).expect("write program");
    path
}

/// Run a NON-dry-run workflow `--project <root>` while the harness process cwd is
/// `cwd` (deliberately != `<root>`). Returns the parsed JSON result.
fn run_workflow(
    home: &TempHome,
    project_root: &Path,
    cwd: &Path,
    prog: &Path,
) -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_harness"))
        .arg("--project")
        .arg(project_root)
        .args(["workflow", "run-script"])
        .arg(prog)
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .output()
        .expect("run workflow");
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("run-script stdout was not JSON ({e}):\n{stdout}"))
}

#[test]
fn writable_node_roots_worktree_at_project_root_not_harness_cwd() {
    let home = TempHome::new("cwd-split");
    // A (non-git) project root and a DIFFERENT cwd to run the harness from.
    let project_root = home.base().join("myproj");
    std::fs::create_dir_all(&project_root).unwrap();
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    init_project(&home, &project_root);

    let prog = write_program(
        &home.base().join("prog"),
        r#"
workflow("cwdsplit", "a writable node must base its worktree at the project root, not the harness cwd")
phase("edit")
agent("edit a file", provider = "claude", writable = True, label = "editor")
"#,
    );
    let result = run_workflow(&home, &project_root, &elsewhere, &prog);

    let step = &result["steps"][0];
    let summary = step["output_summary"].as_str().unwrap_or_default();
    // The worktree base is the PROJECT root: the non-git rejection names it.
    assert!(
        summary.contains("myproj"),
        "worktree base must be the project root (named 'myproj'): {summary}"
    );
    // ...and NOT the harness process cwd. (If the old `env::current_dir()` base
    // were still in use, the message would name 'elsewhere' instead.)
    assert!(
        !summary.contains("elsewhere"),
        "worktree base must NOT be the harness process cwd ('elsewhere'): {summary}"
    );
}

#[test]
fn workflow_worktree_base_is_not_the_centralized_store() {
    // The centralized store (~/.harness/projects/<id>/) must never be used as a
    // repo/worktree root. The non-git rejection names the project root, which is
    // distinct from the store path under `<harness_home>/projects/`.
    let home = TempHome::new("cwd-store-distinct");
    let project_root = home.base().join("repo2");
    std::fs::create_dir_all(&project_root).unwrap();
    init_project(&home, &project_root);

    let prog = write_program(
        &home.base().join("prog"),
        r#"
workflow("storedistinct", "worktree base must be the repo, never the centralized JSONL store")
phase("edit")
agent("edit", provider = "claude", writable = True, label = "editor")
"#,
    );
    let result = run_workflow(&home, &project_root, home.base(), &prog);

    let summary = result["steps"][0]["output_summary"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        summary.contains("repo2"),
        "worktree base must be the project root: {summary}"
    );
    assert!(
        !summary.contains("/projects/"),
        "worktree base must NOT be the centralized store under <home>/projects/: {summary}"
    );
}

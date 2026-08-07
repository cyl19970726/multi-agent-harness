//! Multi-project #89 convergence invariant (goal-multi-project,
//! serve-project-switch-convergence task).
//!
//! A long-running `serve` and CLI commands from different cwds converge on the
//! active Execution Space. Switching a Project Binding changes provider cwd,
//! instructions, and Skills only; it never switches the coordination store.

use std::path::Path;

mod firm_env;
use firm_env::{current_project_id, current_space_id, run_firm, ServeHandle, TempHome};

fn init_project(home: &TempHome, name: &str) -> (std::path::PathBuf, String) {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_firm(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    (root, current_project_id(home))
}

/// `harness --store-source mission list` from `cwd`; return the resolved `root=<path>`.
fn resolved_store_root(home: &TempHome, cwd: &Path) -> String {
    let out = run_firm(home, cwd, &["--store-source", "mission", "list"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    stderr
        .lines()
        .find(|l| l.contains("store-source:"))
        .and_then(|l| l.split("root=").nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| panic!("no store-source line: {stderr}"))
}

#[test]
fn serve_and_cli_converge_on_space_while_project_binding_switches() {
    let home = TempHome::new("conv-switch");
    // Two distinct project roots in different directories.
    let (root_a, id_a) = init_project(&home, "repo-a");
    let (_root_b, id_b) = init_project(&home, "repo-b"); // b is active after init
    assert_ne!(id_a, id_b);

    // serve starts from repo-a's directory (cwd != where the CLI later runs).
    let serve = ServeHandle::spawn(&home, &root_a, &[]);

    let space_id = current_space_id(&home);
    // Switch the active Project Binding to A over the serve API.
    let (status, body) =
        serve.post_json("/v1/projects/switch", &serde_json::json!({"project": id_a}));
    assert_eq!(status, 200, "switch body: {body}");

    // A CLI command run from an unrelated cwd still resolves the same Execution
    // Space, not project A's compatibility store.
    let unrelated = home.base().join("unrelated").join("deep");
    std::fs::create_dir_all(&unrelated).unwrap();
    let cli_root = resolved_store_root(&home, &unrelated);

    let (_s, cur) = serve.get_json("/v1/spaces/current");
    let serve_root = cur["space"]["store_root"]
        .as_str()
        .expect("space store_root")
        .to_string();

    assert!(cli_root.ends_with(&space_id), "CLI space: {cli_root}");
    assert_eq!(
        std::fs::canonicalize(&cli_root).ok(),
        std::fs::canonicalize(&serve_root).ok(),
        "serve and CLI diverged: serve={serve_root} cli={cli_root}"
    );

    // It is a native Execution Space, not a project compatibility store.
    assert!(
        cli_root.contains("/execution-spaces/"),
        "not an Execution Space store: {cli_root}"
    );
    assert!(
        !cli_root.ends_with("repo-a/.firm") && !cli_root.ends_with("repo-b/.firm"),
        "resolved a repo-local store: {cli_root}"
    );
}

#[test]
fn cli_write_after_switch_is_visible_in_serve_snapshot() {
    let home = TempHome::new("conv-visible");
    let (root_a, id_a) = init_project(&home, "repo-a");
    let (_root_b, _id_b) = init_project(&home, "repo-b");

    let serve = ServeHandle::spawn(&home, &root_a, &[]);
    let (status, _b) =
        serve.post_json("/v1/projects/switch", &serde_json::json!({"project": id_a}));
    assert_eq!(status, 200);

    // CLI from a different cwd creates a Mission; it lands in project A's central store.
    let elsewhere = home.base().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let out = run_firm(
        &home,
        &elsewhere,
        &[
            "mission",
            "create",
            "--id",
            "converge-mission",
            "--title",
            "Converged",
            "--objective",
            "Prove project convergence",
        ],
    );
    assert!(out.status.success(), "mission create failed: {out:?}");

    // serve (started from root_a, default project now A) sees it in its snapshot.
    let (status, snap_a) = serve.get_json(&format!("/v1/snapshot?project={id_a}"));
    assert_eq!(status, 200);
    let ids: Vec<String> = snap_a["missions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| g["id"].as_str().map(|x| x.to_string()))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        ids.contains(&"converge-mission".to_string()),
        "serve snapshot missing the sibling CLI write: {ids:?}"
    );
}

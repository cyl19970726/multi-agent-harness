//! Multi-project #89 convergence invariant (goal-multi-project,
//! serve-project-switch-convergence task).
//!
//! The classic #89 invariant is: a long-running `serve` and a CLI command started
//! from a DIFFERENT cwd must resolve the SAME store, so a sibling write shows up in
//! serve's snapshot. Pre-multi-project this relied on a shared cwd walk-up. Now the
//! convergence point is the registry's *current project*: switching the active
//! project (over the serve API) must make a CLI command from any cwd resolve the
//! newly-active central store — `~/.harness/projects/<id>`, never a repo-local
//! `.harness`.

use std::path::Path;

mod harness_env;
use harness_env::{current_project_id, run_harness, ServeHandle, TempHome};

fn init_project(home: &TempHome, name: &str) -> (std::path::PathBuf, String) {
    let root = home.base().join(name);
    std::fs::create_dir_all(&root).unwrap();
    let out = run_harness(home, &root, &["init"]);
    assert!(out.status.success(), "init {name} failed: {out:?}");
    (root, current_project_id(home))
}

/// `harness --store-source mission list` from `cwd`; return the resolved `root=<path>`.
fn resolved_store_root(home: &TempHome, cwd: &Path) -> String {
    let out = run_harness(home, cwd, &["--store-source", "mission", "list"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    stderr
        .lines()
        .find(|l| l.contains("store-source:"))
        .and_then(|l| l.split("root=").nth(1))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| panic!("no store-source line: {stderr}"))
}

#[test]
fn serve_and_cli_from_different_cwds_converge_after_switch() {
    let home = TempHome::new("conv-switch");
    // Two distinct project roots in different directories.
    let (root_a, id_a) = init_project(&home, "repo-a");
    let (_root_b, id_b) = init_project(&home, "repo-b"); // b is active after init
    assert_ne!(id_a, id_b);

    // serve starts from repo-a's directory (cwd != where the CLI later runs).
    let serve = ServeHandle::spawn(&home, &root_a, &[]);

    // Switch the active project to A over the serve API.
    let (status, body) =
        serve.post_json("/v1/projects/switch", &serde_json::json!({"project": id_a}));
    assert_eq!(status, 200, "switch body: {body}");

    // A CLI command run from a completely UNRELATED cwd now resolves project A's
    // CENTRAL store (the convergence point is the registry, not the shared cwd).
    let unrelated = home.base().join("unrelated").join("deep");
    std::fs::create_dir_all(&unrelated).unwrap();
    let cli_root = resolved_store_root(&home, &unrelated);

    // serve's current endpoint reports the same project.
    let (_s, cur) = serve.get_json("/v1/projects/current");
    let serve_root = cur["store_root"].as_str().expect("store_root").to_string();

    // Both resolve project A's central store.
    assert!(
        cli_root.ends_with(&id_a),
        "CLI did not converge on project {id_a}: {cli_root}"
    );
    assert_eq!(
        std::fs::canonicalize(&cli_root).ok(),
        std::fs::canonicalize(&serve_root).ok(),
        "serve and CLI diverged: serve={serve_root} cli={cli_root}"
    );

    // It is the CENTRAL store under ~/.harness/projects/<id>, not a repo-local one.
    assert!(
        cli_root.contains("/projects/"),
        "not a central store: {cli_root}"
    );
    assert!(
        !cli_root.ends_with("repo-a/.harness") && !cli_root.ends_with("repo-b/.harness"),
        "resolved a repo-local .harness instead of central: {cli_root}"
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
    let out = run_harness(
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

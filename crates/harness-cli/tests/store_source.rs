//! Integration coverage for grace-period dual-read store resolution + the
//! `--store-source` debug flag (goal-multi-project, dual-read-logging task).
//!
//! The grace-period rule: prefer central when a project is selected/active, fall
//! back to a repo-local `.harness/` only when central is absent, IGNORE local stores
//! marked `MIGRATED_TO_CENTRAL` (redirecting to the central store they point at), and
//! ALWAYS log which store was chosen and why. `--store-source` makes the choice
//! observable so nothing silently mixes central and local data.

use std::path::Path;
use std::process::Command;

mod harness_env;
use harness_env::{run_harness, TempHome};

/// Run `harness --store-source <args...>` from `cwd` and return its Output. The
/// flag prints `store-source: <Source> root=<path>` to stderr, then runs the cmd.
fn run_with_store_source(home: &TempHome, cwd: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_harness"));
    cmd.arg("--store-source");
    for a in args {
        cmd.arg(a);
    }
    cmd.current_dir(cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .output()
        .expect("run harness --store-source")
}

/// The resolution line `store-source: <Source> root=<path>` from stderr (the line
/// carrying `root=`), distinct from any `store-source:` redirect/skip log lines.
fn store_source_line(out: &std::process::Output) -> Option<String> {
    String::from_utf8_lossy(&out.stderr)
        .lines()
        .find(|l| l.starts_with("store-source:") && l.contains("root="))
        .map(str::to_string)
}

/// Seed a repo-local `.harness/` store with one ledger row.
fn seed_local(repo: &Path) -> std::path::PathBuf {
    let local = repo.join(".harness");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join("goals.jsonl"), "{\"id\":\"g1\"}\n").unwrap();
    local
}

#[test]
fn store_source_reports_override() {
    let home = TempHome::new("ss-override");
    let explicit = home.base().join("explicit-store");
    // `--store` override → StoreFlag source, emitted before the command runs.
    let out = run_with_store_source(
        &home,
        home.base(),
        &["--store", explicit.to_str().unwrap(), "board"],
    );
    let line = store_source_line(&out).expect("store-source line");
    assert!(line.contains("StoreFlag"), "override source: {line}");
    // And a deprecation warning is emitted (no silent override).
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("deprecated"), "stderr: {stderr}");
}

#[test]
fn store_source_reports_global_default_when_nothing_selected() {
    let home = TempHome::new("ss-global");
    // Run from a dir OUTSIDE HOME so the walk-up finds no ancestor `.harness`
    // (HARNESS_HOME lives at <HOME>/.harness, so any cwd under HOME would walk up
    // to it). With no project selected and no local store, GlobalDefault must win.
    let out = run_with_store_source(&home, home.base(), &["board"]);
    let line = store_source_line(&out).expect("store-source line");
    assert!(line.contains("GlobalDefault"), "global source: {line}");
}

#[test]
fn store_source_reports_central_when_project_active() {
    let home = TempHome::new("ss-central");
    let repo = home.home().join("centralrepo");
    std::fs::create_dir_all(&repo).unwrap();
    run_harness(&home, &repo, &["project", "add", "--switch"]);
    // From a neutral cwd, the active project (central) wins — not any local store.
    let out = run_with_store_source(&home, home.base(), &["board"]);
    let line = store_source_line(&out).expect("store-source line");
    assert!(line.contains("RegistryCurrent"), "central source: {line}");
    assert!(
        line.contains("/projects/centralrepo"),
        "central root: {line}"
    );
}

#[test]
fn unmigrated_local_store_is_a_logged_compatibility_fallback() {
    let home = TempHome::new("ss-local");
    let repo = home.home().join("legacyrepo");
    std::fs::create_dir_all(&repo).unwrap();
    seed_local(&repo);
    // No project selected → fall back to the repo-local store, with a clear warning.
    let out = run_with_store_source(&home, &repo, &["board"]);
    let line = store_source_line(&out).expect("store-source line");
    assert!(line.contains("CwdWalkUp"), "local source: {line}");
    assert!(line.contains("legacyrepo/.harness"), "local root: {line}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("repo-local store"),
        "expected compatibility warning: {stderr}"
    );
}

#[test]
fn migrated_local_store_is_ignored_and_redirects_to_central() {
    let home = TempHome::new("ss-migrated");
    let repo = home.home().join("oncemigrated");
    std::fs::create_dir_all(&repo).unwrap();
    seed_local(&repo);
    // Migrate it (this writes the MIGRATED_TO_CENTRAL marker) but do NOT switch, so
    // no project is active afterward and resolution must fall through the walk-up.
    let mig = run_harness(&home, &repo, &["project", "migrate"]);
    assert!(mig.status.success(), "migrate failed: {mig:?}");

    // Resolving from the migrated repo must IGNORE the local store and read central.
    let out = run_with_store_source(&home, &repo, &["board"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("is migrated; reading central store"),
        "expected migration redirect log: {stderr}"
    );
    let line = store_source_line(&out).expect("store-source line");
    // The redirected root is the CENTRAL store, never the local one.
    assert!(
        line.contains("/projects/oncemigrated"),
        "redirected root: {line}"
    );
    assert!(
        !line.contains("oncemigrated/.harness"),
        "must not resolve the migrated local store: {line}"
    );
    // No "repo-local store" compatibility warning — we did not fall back to local.
    assert!(
        !stderr.contains("using repo-local store"),
        "must not warn local fallback for a migrated store: {stderr}"
    );
}

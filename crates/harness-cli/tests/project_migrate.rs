//! Integration coverage for `harness project migrate`
//! (goal-multi-project, project-migrate task).
//!
//! Migration moves a repo-local `.harness/` store into the centralized
//! `~/.harness/projects/<id>/` store: copying active JSONL ledgers plus prompts /
//! runtimes, writing `metadata.json` with `migrated_from`, and dropping a
//! `MIGRATED_TO_CENTRAL` marker in the old store that points at the central one. It
//! must preserve record counts, be idempotent, and fail safely.

use std::path::Path;

mod harness_env;
use harness_env::{run_harness, TempHome};

/// Build a legacy repo-local `.harness/` store under HOME with representative data
/// across every preserved surface, returning (repo_root, local_store).
fn seed_local_store(home: &TempHome, name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let repo = home.home().join(name);
    let local = repo.join(".harness");
    std::fs::create_dir_all(local.join("prompts")).unwrap();
    std::fs::create_dir_all(local.join("runtimes")).unwrap();
    // Active JSONL ledgers plus one retired provider-session ledger that must
    // not be copied into the new centralized store.
    std::fs::write(
        local.join("goals.jsonl"),
        "{\"id\":\"g1\"}\n{\"id\":\"g2\"}\n",
    )
    .unwrap();
    std::fs::write(local.join("tasks.jsonl"), "{\"id\":\"t1\"}\n").unwrap();
    std::fs::write(local.join("members.jsonl"), "{\"id\":\"m1\"}\n").unwrap();
    std::fs::write(local.join("provider_sessions.jsonl"), "{\"id\":\"ps1\"}\n").unwrap();
    std::fs::write(local.join("prompts").join("worker.md"), "prompt body").unwrap();
    std::fs::write(local.join("runtimes").join("rt.json"), "{}").unwrap();
    (repo, local)
}

/// Count non-empty lines across every `*.jsonl` file at a store root.
fn ledger_record_count(store: &Path) -> usize {
    let mut total = 0;
    for entry in std::fs::read_dir(store).unwrap().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            total += std::fs::read_to_string(&path)
                .unwrap()
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count();
        }
    }
    total
}

#[test]
fn migrate_preserves_records_and_payloads_and_marks_old_store() {
    let home = TempHome::new("mig-preserve");
    let (repo, local) = seed_local_store(&home, "legacyrepo");
    let before = ledger_record_count(&local);
    assert_eq!(before, 5, "seed includes one retired provider-session row");

    let out = run_harness(&home, &repo, &["project", "migrate"]);
    assert!(out.status.success(), "migrate failed: {out:?}");
    let result: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).expect("migrate stdout JSON");
    assert_eq!(result["migrated"], true);
    let project_id = result["project_id"].as_str().unwrap().to_string();
    assert_eq!(project_id, "legacyrepo");

    // The retired duplicate provider-session ledger is intentionally discarded.
    let central = home.projects_dir().join(&project_id);
    let after = ledger_record_count(&central);
    assert_eq!(
        after,
        before - 1,
        "retired provider session should be omitted"
    );
    assert_eq!(result["records_before"], before as u64);
    assert_eq!(result["records_after"], after as u64);

    // Specific ledgers + payloads landed in the central store.
    assert_eq!(
        std::fs::read_to_string(central.join("goals.jsonl")).unwrap(),
        "{\"id\":\"g1\"}\n{\"id\":\"g2\"}\n"
    );
    assert!(central.join("tasks.jsonl").exists());
    assert!(central.join("members.jsonl").exists());
    assert!(!central.join("provider_sessions.jsonl").exists());
    assert!(!central.join("provider-sessions").exists());
    assert!(central.join("prompts").join("worker.md").exists());
    assert!(central.join("runtimes").join("rt.json").exists());

    // metadata.json records migrated_from pointing at the old store.
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(central.join("metadata.json")).unwrap())
            .unwrap();
    assert_eq!(meta["project_id"], project_id);
    assert_eq!(
        meta["migrated_from"].as_str().unwrap(),
        local.display().to_string()
    );

    // The old store carries a MIGRATED_TO_CENTRAL marker pointing at the central store.
    let marker = std::fs::read_to_string(local.join("MIGRATED_TO_CENTRAL")).unwrap();
    assert_eq!(marker.trim(), central.display().to_string());
}

#[test]
fn migrate_is_idempotent() {
    let home = TempHome::new("mig-idempotent");
    let (repo, local) = seed_local_store(&home, "repo2");

    let first = run_harness(&home, &repo, &["project", "migrate"]);
    assert!(first.status.success(), "first migrate failed: {first:?}");
    let central = home.projects_dir().join(
        serde_json::from_str::<serde_json::Value>(&String::from_utf8_lossy(&first.stdout)).unwrap()
            ["project_id"]
            .as_str()
            .unwrap(),
    );
    let count_after_first = ledger_record_count(&central);

    // Second migrate is a no-op (the local store is marked migrated) and succeeds.
    let second = run_harness(&home, &repo, &["project", "migrate"]);
    assert!(second.status.success(), "second migrate failed: {second:?}");
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("already migrated"),
        "expected idempotent no-op message: {stdout}"
    );
    // Records were not duplicated by re-running.
    assert_eq!(
        ledger_record_count(&central),
        count_after_first,
        "re-migration duplicated records"
    );
    // Marker still present and intact.
    assert!(local.join("MIGRATED_TO_CENTRAL").exists());
}

#[test]
fn migrate_with_no_local_store_fails_safely() {
    let home = TempHome::new("mig-no-store");
    let bare = home.home().join("empty-repo");
    std::fs::create_dir_all(&bare).unwrap();
    let out = run_harness(&home, &bare, &["project", "migrate"]);
    assert!(
        !out.status.success(),
        "migrate without a local store should fail"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no local store"), "stderr: {stderr}");
}

#[test]
fn migrate_does_not_flip_active_project_without_switch() {
    let home = TempHome::new("mig-no-flip");
    // Make some OTHER project current first.
    let other = home.home().join("other");
    std::fs::create_dir_all(&other).unwrap();
    run_harness(&home, &other, &["project", "add", "--switch"]);

    let (repo, _local) = seed_local_store(&home, "migrated");
    let out = run_harness(&home, &repo, &["project", "migrate"]);
    assert!(out.status.success(), "migrate failed: {out:?}");

    // The active project is unchanged (migrate is non-disruptive by default).
    let current = run_harness(&home, home.base(), &["project", "current"]);
    let cur: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&current.stdout)).unwrap();
    assert_eq!(
        cur["id"], "other",
        "migrate must not flip the active project"
    );

    // With --switch it DOES become current.
    let (repo2, _l2) = seed_local_store(&home, "migrated2");
    let out = run_harness(&home, &repo2, &["project", "migrate", "--switch"]);
    assert!(out.status.success(), "migrate --switch failed: {out:?}");
    let current = run_harness(&home, home.base(), &["project", "current"]);
    let cur: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&current.stdout)).unwrap();
    assert_eq!(cur["id"], "migrated2");
}

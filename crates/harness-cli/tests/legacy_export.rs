//! R0 acceptance for the read-only, multi-source legacy archive.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod harness_env;
use harness_env::TempHome;

fn run(home: &TempHome, cwd: &Path, args: &[String]) -> Output {
    run_with_env(home, cwd, args, &[])
}

fn run_with_env(
    home: &TempHome,
    cwd: &Path,
    args: &[String],
    extra_env: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_harness"));
    command
        .args(args)
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("HARNESS_ROOT")
        .env_remove("HARNESS_PROJECT")
        .env_remove("HARNESS_WORKFLOW_CHILD_STORE_ROOT");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().expect("run harness")
}

fn initialize_project(home: &TempHome) -> (PathBuf, String, PathBuf) {
    initialize_project_named(home, "repo")
}

fn initialize_project_named(home: &TempHome, name: &str) -> (PathBuf, String, PathBuf) {
    let project = home.base().join(name);
    std::fs::create_dir_all(&project).unwrap();
    let output = run(home, &project, &["init".into()]);
    assert!(output.status.success(), "init failed: {output:?}");
    let registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.registry_path()).unwrap()).unwrap();
    let id = registry["current_project_id"].as_str().unwrap().to_string();
    let store = home.projects_dir().join(&id);
    (project, id, store)
}

fn force_project_id(
    home: &TempHome,
    project: &Path,
    old_id: &str,
    old_store: &Path,
    new_id: &str,
) -> PathBuf {
    let new_store = home.projects_dir().join(new_id);
    std::fs::rename(old_store, &new_store).unwrap();
    let mut registry: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.registry_path()).unwrap()).unwrap();
    registry["current_project_id"] = serde_json::json!(new_id);
    let entry = registry["projects"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == old_id)
        .unwrap();
    entry["id"] = serde_json::json!(new_id);
    entry["path"] = serde_json::json!(project.display().to_string());
    entry["store_root"] = serde_json::json!(new_store.display().to_string());
    std::fs::write(
        home.registry_path(),
        serde_json::to_vec_pretty(&registry).unwrap(),
    )
    .unwrap();
    std::fs::write(home.active_marker_path(), format!("{new_id}\n")).unwrap();
    let metadata_path = new_store.join("metadata.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&metadata_path).unwrap()).unwrap();
    metadata["project_id"] = serde_json::json!(new_id);
    std::fs::write(metadata_path, serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();
    new_store
}

fn seed_closed_legacy_store(store: &Path) -> BTreeMap<String, Vec<u8>> {
    let rows: [(&str, &[u8]); 12] = [
        (
            "goals.jsonl",
            b"{\"id\":\"g1\",\"goal_design_id\":\"gd1\",\"phases\":[{\"id\":\"p1\"}]}\n{\"id\":\"g1\",\"goal_design_id\":\"gd1\",\"phases\":[{\"id\":\"p1\"}],\"updated_at\":\"later\"}\n",
        ),
        (
            "tasks.jsonl",
            b"{\"id\":\"t2\",\"goal_id\":\"g1\",\"phase_id\":\"p1\"}\n{\"id\":\"t1\",\"goal_id\":\"g1\",\"phase_id\":\"p1\",\"depends_on_task_ids\":[\"t2\"]}\n{\"id\":\"t1\",\"goal_id\":\"g1\",\"phase_id\":\"p1\",\"depends_on_task_ids\":[\"t2\"],\"updated_at\":\"later\"}\n",
        ),
        (
            "goal_designs.jsonl",
            b"{\"id\":\"gd1\",\"goal_id\":\"g1\",\"task_graph\":[\"t1\",\"t2\"]}\n",
        ),
        (
            "goal_evaluations.jsonl",
            b"{\"id\":\"ge1\",\"goal_id\":\"g1\",\"follow_up_task_ids\":[\"t2\"],\"proposed_goal_ids\":[\"g1\"]}\n",
        ),
        (
            "goal_cases.jsonl",
            b"{\"case_id\":\"gc1\",\"source_goal_id\":\"g1\",\"goal_design_ref\":\"gd1\",\"evaluation_ref\":\"ge1\"}\n",
        ),
        (
            "goal_orchestration_runs.jsonl",
            b"{\"id\":\"gor1\",\"goal_id\":\"g1\",\"phase_runs\":[{\"phase_id\":\"p1\"}]}\n",
        ),
        (
            "messages.jsonl",
            b"{\"id\":\"m1\",\"task_id\":\"t1\",\"content\":\"linked\"}\n{\"id\":\"m2\",\"task_id\":null,\"result\":{\"task_id\":\"dynamic-payload-must-not-scan\"}}\n{\"id\":\"m1\",\"task_id\":null,\"content\":\"link cleared later\"}\n",
        ),
        (
            "evidence.jsonl",
            b"{\"id\":\"e1\",\"goal_id\":\"g1\",\"task_id\":\"t1\"}\n",
        ),
        (
            "provider_sessions.jsonl",
            b"{\"id\":\"ps1\",\"task_id\":\"t1\"}\n",
        ),
        (
            "workflow_runs.jsonl",
            b"{\"id\":\"wr1\",\"goal_id\":\"g1\",\"phase_id\":\"p1\"}\n",
        ),
        (
            "workflow_steps.jsonl",
            b"{\"id\":\"ws1\",\"task_id\":\"t1\"}\n",
        ),
        (
            "unrelated.jsonl",
            b"{\"id\":\"u1\",\"value\":\"must not be archived as a record\"}\n",
        ),
    ];
    let mut original = BTreeMap::new();
    for (name, bytes) in rows {
        std::fs::write(store.join(name), bytes).unwrap();
        original.insert(name.to_string(), bytes.to_vec());
    }
    original
}

fn export_args(id: &str, archive: &Path) -> Vec<String> {
    vec![
        "legacy-goal-task".into(),
        "export".into(),
        "--project".into(),
        id.into(),
        "--output".into(),
        archive.display().to_string(),
    ]
}

fn verify_args(archive: &Path) -> Vec<String> {
    let archive = std::fs::canonicalize(archive).unwrap_or_else(|_| archive.to_path_buf());
    verify_args_raw(&archive)
}

fn verify_args_raw(archive: &Path) -> Vec<String> {
    vec![
        "legacy-goal-task".into(),
        "verify".into(),
        "--archive".into(),
        archive.display().to_string(),
    ]
}

#[test]
fn export_preserves_bytes_latest_rows_and_verifies_closure() {
    let home = TempHome::new("legacy-export-ok");
    let (project, id, store) = initialize_project(&home);
    let original = seed_closed_legacy_store(&store);
    std::fs::create_dir_all(project.join("schemas")).unwrap();
    std::fs::write(
        project.join("schemas/goal.schema.json"),
        b"{\"title\":\"Historical Goal\"}\n",
    )
    .unwrap();
    std::fs::create_dir_all(store.join("provider-sessions/session-a")).unwrap();
    std::fs::write(
        store.join("provider-sessions/session-a/reply.txt"),
        b"reply",
    )
    .unwrap();
    let archive = home.base().join("archive-v1");

    let output = run(&home, &project, &export_args(&id, &archive));
    assert!(output.status.success(), "export failed: {output:?}");
    assert_eq!(
        std::fs::read(archive.join("sources/central/raw/goals.jsonl")).unwrap(),
        original["goals.jsonl"]
    );
    assert_eq!(
        std::fs::read(archive.join("sources/central/records/messages.jsonl")).unwrap(),
        b"{\"id\":\"m1\",\"task_id\":\"t1\",\"content\":\"linked\"}\n{\"id\":\"m1\",\"task_id\":null,\"content\":\"link cleared later\"}\n"
    );
    assert!(!archive
        .join("sources/central/records/unrelated.jsonl")
        .exists());
    assert!(
        std::fs::read_to_string(archive.join("sources/central/latest/messages.jsonl"))
            .unwrap()
            .contains("link cleared later")
    );

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(archive.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["format"], "legacy-goal-task-v1");
    assert_eq!(manifest["project"]["id"], id);
    assert_eq!(manifest["closure"]["unresolved_required_edges"], 0);
    assert_eq!(manifest["known_anomalies"].as_array().unwrap().len(), 0);
    let materials = manifest["interpretation_materials"].as_array().unwrap();
    let retired_skill = materials
        .iter()
        .find(|material| material["source_path"] == "skills/generic-agent-harness")
        .unwrap();
    assert_eq!(retired_skill["source_present"], false);
    assert_eq!(retired_skill["reason"], "not_present_in_source");
    assert_eq!(manifest["sources"].as_array().unwrap().len(), 1);
    assert_eq!(
        manifest["sources"][0]["snapshot_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert!(manifest["sources"][0]["snapshot_files"]
        .as_array()
        .unwrap()
        .iter()
        .any(|file| file["path"] == "provider-sessions/session-a/reply.txt"));
    let edges = std::fs::read_to_string(archive.join("edges.jsonl")).unwrap();
    assert!(edges.contains("\"field\":\"/evaluation_ref\""));
    assert!(edges.contains("\"target_kind\":\"goal_evaluation\""));
    assert!(!edges.contains("dynamic-payload-must-not-scan"));

    let verify = run(&home, &project, &verify_args(&archive));
    assert!(verify.status.success(), "verify failed: {verify:?}");
    let report: serde_json::Value = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(report["closure"], "verified");
    assert_eq!(report["known_anomalies"], 0);
}

#[test]
fn migrated_local_store_is_a_distinct_source_and_keeps_unique_rows() {
    let home = TempHome::new("legacy-export-multi-source");
    let (project, id, central) = initialize_project(&home);
    seed_closed_legacy_store(&central);
    let local = project.join(".harness");
    std::fs::create_dir(&local).unwrap();
    std::fs::write(
        local.join("MIGRATED_TO_CENTRAL"),
        central.display().to_string(),
    )
    .unwrap();
    let unique = b"{\"id\":\"local-only\",\"task_id\":\"t1\",\"content\":\"local only\"}\n";
    std::fs::write(local.join("messages.jsonl"), unique).unwrap();
    let archive = home.base().join("multi-source");
    let output = run(&home, &project, &export_args(&id, &archive));
    assert!(output.status.success(), "export failed: {output:?}");
    assert_eq!(
        std::fs::read(archive.join("sources/local/records/messages.jsonl")).unwrap(),
        unique
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(archive.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["sources"].as_array().unwrap().len(), 2);
    assert_ne!(
        manifest["sources"][0]["snapshot_sha256"],
        manifest["sources"][1]["snapshot_sha256"]
    );
    assert_eq!(manifest["source_comparisons"].as_array().unwrap().len(), 1);
    assert!(
        manifest["source_comparisons"][0]["shared_different"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(run(&home, &project, &verify_args(&archive))
        .status
        .success());
}

#[test]
fn exact_phase_verdict_kind_mismatch_is_recorded_and_reverified() {
    let home = TempHome::new("legacy-export-known-anomaly");
    let (project, old_id, old_store) = initialize_project_named(&home, "multi-agent-harness");
    let id = "multi-agent-harness".to_string();
    let store = force_project_id(&home, &project, &old_id, &old_store, &id);
    seed_closed_legacy_store(&store);
    std::fs::write(
        store.join("goals.jsonl"),
        b"{\"id\":\"g1\",\"goal_design_id\":\"gd1\",\"phases\":[{\"id\":\"p1\"}]}\n{\"id\":\"goal-custom-workflow-phase-runner-v1\",\"phases\":[]}\n",
    )
    .unwrap();
    let mut decisions = vec![b'\n'; 43];
    let raw = b"{\"id\":\"decision-1783272619378-p31551-2\",\"task_id\":\"goal-custom-workflow-phase-runner-v1\",\"decision\":\"phase direct-workflow-acceptance verdict: pass\",\"rationale\":\"phase-direct-workflow-acceptance verdict: intent met \xE2\x80\x94 custom workflow-mode phase direct run accepted\",\"evidence_ids\":[],\"created_at\":\"unix-ms:1783272619378\",\"decision_kind\":\"phase_verdict\",\"goal_id\":\"goal-custom-workflow-phase-runner-v1\",\"is_waiver\":false,\"follow_up_task_id\":null}\n";
    decisions.extend_from_slice(raw);
    std::fs::write(store.join("decisions.jsonl"), decisions).unwrap();
    let archive = home.base().join("known-anomaly");
    let output = run(&home, &project, &export_args(&id, &archive));
    assert!(output.status.success(), "export failed: {output:?}");
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["known_anomalies"], 1);
    assert_eq!(summary["unresolved_required_edges"], 0);
    assert_eq!(
        std::fs::read(archive.join("sources/central/records/decisions.jsonl")).unwrap(),
        raw
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(archive.join("manifest.json")).unwrap()).unwrap();
    let anomaly = &manifest["known_anomalies"][0];
    assert_eq!(anomaly["anomaly_kind"], "known_kind_mismatch");
    assert_eq!(anomaly["ledger"], "decisions.jsonl");
    assert_eq!(anomaly["line"], 44);
    assert_eq!(anomaly["record_id"], "decision-1783272619378-p31551-2");
    assert_eq!(anomaly["field"], "/task_id");
    assert_eq!(anomaly["target"], "goal-custom-workflow-phase-runner-v1");
    assert_eq!(
        anomaly["raw_line_sha256"],
        "66d5c9d0a7a133a6adb021c95ea7b7d9ded1f16d87b08dcead8edb58934c9a55"
    );
    let verify = run(&home, &project, &verify_args(&archive));
    assert!(verify.status.success(), "verify failed: {verify:?}");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&verify.stdout).unwrap()["known_anomalies"],
        1
    );

    let manifest_path = archive.join("manifest.json");
    let mut tampered: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    tampered["known_anomalies"][0]["raw_line_sha256"] = serde_json::json!("00");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&tampered).unwrap(),
    )
    .unwrap();
    let verify = run(&home, &project, &verify_args(&archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr).contains("known anomaly whitelist"));

    let drifted_archive = home.base().join("known-anomaly-drifted");
    let mut drifted = vec![b'\n'; 43];
    drifted.extend_from_slice(
        b"{\"id\":\"decision-1783272619378-p31551-2\",\"task_id\":\"goal-custom-workflow-phase-runner-v1\",\"goal_id\":\"goal-custom-workflow-phase-runner-v1\",\"decision_kind\":\"phase_verdict\"}\n",
    );
    std::fs::write(store.join("decisions.jsonl"), drifted).unwrap();
    let export = run(&home, &project, &export_args(&id, &drifted_archive));
    assert!(!export.status.success());
    assert!(String::from_utf8_lossy(&export.stderr)
        .contains("preauthorized known anomaly contract mismatch"));
    assert!(!drifted_archive.exists());
}

#[test]
fn near_miss_anomaly_and_other_dangling_edges_still_fail() {
    let home = TempHome::new("legacy-export-anomaly-near-miss");
    let (project, id, store) = initialize_project(&home);
    seed_closed_legacy_store(&store);
    let mut decisions = vec![b'\n'; 42];
    decisions.extend_from_slice(
        b"{\"id\":\"not-line-44\",\"task_id\":\"g1\",\"goal_id\":\"g1\",\"decision_kind\":\"phase_verdict\"}\n",
    );
    std::fs::write(store.join("decisions.jsonl"), decisions).unwrap();
    let archive = home.base().join("near-miss");
    let export = run(&home, &project, &export_args(&id, &archive));
    assert!(export.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&export.stdout).unwrap();
    assert_eq!(summary["known_anomalies"], 0);
    assert!(summary["unresolved_required_edges"].as_u64().unwrap() > 0);
    let verify = run(&home, &project, &verify_args(&archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr).contains("closure failed"));
}

#[test]
fn invalid_explicit_project_never_falls_back_to_workflow_child_store() {
    let home = TempHome::new("legacy-export-no-fallback");
    let (project, _id, store) = initialize_project(&home);
    seed_closed_legacy_store(&store);
    let archive = home.base().join("must-not-exist");
    let args = export_args("missing-project", &archive);
    let store_text = store.display().to_string();
    let output = run_with_env(
        &home,
        &project,
        &args,
        &[("HARNESS_WORKFLOW_CHILD_STORE_ROOT", &store_text)],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing fallback"));
    assert!(!archive.exists());

    let missing_args = vec![
        "legacy-goal-task".into(),
        "export".into(),
        "--output".into(),
        home.base()
            .join("missing-project-flag")
            .display()
            .to_string(),
    ];
    let output = run_with_env(
        &home,
        &project,
        &missing_args,
        &[("HARNESS_WORKFLOW_CHILD_STORE_ROOT", &store_text)],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exactly one --project"));
}

#[test]
fn explicit_project_wins_over_workflow_child_store() {
    let home = TempHome::new("legacy-export-project-wins");
    let (project, id, store) = initialize_project(&home);
    seed_closed_legacy_store(&store);
    let fake = home.base().join("workflow-child-store");
    std::fs::create_dir(&fake).unwrap();
    std::fs::write(fake.join("goals.jsonl"), b"{\"id\":\"wrong-source\"}\n").unwrap();
    let archive = home.base().join("project-wins");
    let fake_text = fake.display().to_string();
    let output = run_with_env(
        &home,
        &project,
        &export_args(&id, &archive),
        &[("HARNESS_WORKFLOW_CHILD_STORE_ROOT", &fake_text)],
    );
    assert!(output.status.success(), "export failed: {output:?}");
    let raw = std::fs::read_to_string(archive.join("sources/central/raw/goals.jsonl")).unwrap();
    assert!(raw.contains("g1"));
    assert!(!raw.contains("wrong-source"));
}

#[test]
fn destination_and_symlink_safety_fail_closed() {
    let home = TempHome::new("legacy-export-path-safety");
    let (project, id, store) = initialize_project(&home);
    seed_closed_legacy_store(&store);

    let existing = home.base().join("existing");
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("sentinel"), b"keep").unwrap();
    let output = run(&home, &project, &export_args(&id, &existing));
    assert!(!output.status.success());
    assert_eq!(std::fs::read(existing.join("sentinel")).unwrap(), b"keep");

    let inside_project = project.join("archive");
    let output = run(&home, &project, &export_args(&id, &inside_project));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside the project root"));

    let inside_store = store.join("archive");
    let output = run(&home, &project, &export_args(&id, &inside_store));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside every live source store"));

    #[cfg(unix)]
    {
        let outside_schemas = home.base().join("outside-schemas");
        std::fs::create_dir(&outside_schemas).unwrap();
        std::fs::write(outside_schemas.join("goal.schema.json"), b"{}\n").unwrap();
        std::os::unix::fs::symlink(&outside_schemas, project.join("schemas")).unwrap();
        let outside = home.base().join("symlink-interpretation-parent");
        let output = run(&home, &project, &export_args(&id, &outside));
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("parent/leaf"));
        std::fs::remove_file(project.join("schemas")).unwrap();

        std::os::unix::fs::symlink(&store, project.join(".harness")).unwrap();
        let outside = home.base().join("symlink-source");
        let output = run(&home, &project, &export_args(&id, &outside));
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("must not be a symlink"));
    }
}

#[cfg(unix)]
#[test]
fn verifier_rejects_archive_reached_through_parent_symlink() {
    let home = TempHome::new("legacy-export-verify-parent-symlink");
    let (project, id, store) = initialize_project(&home);
    seed_closed_legacy_store(&store);
    let archive_parent = home.base().join("real-archives");
    std::fs::create_dir(&archive_parent).unwrap();
    let archive = archive_parent.join("archive-v3");
    assert!(run(&home, &project, &export_args(&id, &archive))
        .status
        .success());
    assert!(run(&home, &project, &verify_args(&archive))
        .status
        .success());

    let alias = home.base().join("alias");
    std::os::unix::fs::symlink(&archive_parent, &alias).unwrap();
    let aliased_archive = alias.join("archive-v3");
    let verify = run(&home, &project, &verify_args_raw(&aliased_archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr)
        .contains("archive directory ancestor must not be a symlink"));
}

#[test]
fn goal_design_ref_is_always_a_required_edge() {
    let home = TempHome::new("legacy-export-required-goal-design-ref");
    let (project, id, store) = initialize_project(&home);
    seed_closed_legacy_store(&store);
    std::fs::write(
        store.join("goal_cases.jsonl"),
        b"{\"case_id\":\"gc-missing\",\"source_goal_id\":\"g1\",\"goal_design_ref\":\"missing-design\",\"evaluation_ref\":\"ge1\"}\n",
    )
    .unwrap();
    let archive = home.base().join("required-goal-design-ref");
    let export = run(&home, &project, &export_args(&id, &archive));
    assert!(export.status.success(), "export failed: {export:?}");
    let summary: serde_json::Value = serde_json::from_slice(&export.stdout).unwrap();
    assert!(summary["unresolved_required_edges"].as_u64().unwrap() > 0);
    let edges = std::fs::read_to_string(archive.join("edges.jsonl")).unwrap();
    assert!(edges.contains("\"field\":\"/goal_design_ref\""));
    assert!(edges.contains("\"closure_required\":true"));
    let verify = run(&home, &project, &verify_args(&archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr).contains("closure failed"));
}

#[test]
fn verifier_rejects_missing_sources_snapshot_mismatch_and_material_ambiguity() {
    let home = TempHome::new("legacy-export-manifest-contract");
    let (project, id, store) = initialize_project(&home);
    seed_closed_legacy_store(&store);
    let archive = home.base().join("manifest-contract");
    assert!(run(&home, &project, &export_args(&id, &archive))
        .status
        .success());
    let manifest_path = archive.join("manifest.json");
    let original = std::fs::read(&manifest_path).unwrap();

    let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    manifest["sources"] = serde_json::json!([]);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let verify = run(&home, &project, &verify_args(&archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr).contains("at least one source"));

    let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let raw_goals = manifest["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["path"] == "sources/central/raw/goals.jsonl")
        .unwrap();
    raw_goals["source_present"] = serde_json::json!(false);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let verify = run(&home, &project, &verify_args(&archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr).contains("source snapshot presence/hash"));

    let mut manifest: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let retired_skill = manifest["interpretation_materials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|material| material["source_path"] == "skills/generic-agent-harness")
        .unwrap();
    retired_skill["reason"] = serde_json::json!("silently_missing");
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let verify = run(&home, &project, &verify_args(&archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr).contains("not_present_in_source"));
}

#[test]
fn tampering_with_raw_or_anomaly_manifest_fails_verification() {
    let home = TempHome::new("legacy-export-tamper");
    let (project, id, store) = initialize_project(&home);
    seed_closed_legacy_store(&store);
    let archive = home.base().join("tampered");
    assert!(run(&home, &project, &export_args(&id, &archive))
        .status
        .success());
    std::fs::write(archive.join("sources/central/raw/goals.jsonl"), b"{}\n").unwrap();
    let verify = run(&home, &project, &verify_args(&archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr).contains("SHA-256 mismatch"));
}

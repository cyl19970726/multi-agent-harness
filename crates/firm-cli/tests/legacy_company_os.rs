//! DOC-108 Stage A acceptance for `harness legacy-company-os export|verify`:
//! the machine-wide retired Company OS record archive round-trips through the
//! CLI, preserves bytes, excludes secrets and provider-native locations, and
//! verifies offline.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod firm_env;
use firm_env::TempHome;

fn run(home: &TempHome, cwd: &Path, args: &[String]) -> Output {
    run_with_env(home, cwd, args, &[])
}

fn run_with_env(
    home: &TempHome,
    cwd: &Path,
    args: &[String],
    extra_env: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_firm"));
    command
        .args(args)
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY")
        .env_remove("FIRM_WORKFLOW_CHILD_STORE_ROOT");
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().expect("run harness")
}

fn write_json(path: &Path, value: serde_json::Value) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).expect("write json");
}

/// Seed every store kind with contract ledgers, excluded locations, and a
/// repo-local compatibility store. Rows: company documents 2 + approvals 1,
/// space missions 1 + mission_log 1 + waves 1, project members 1 +
/// team_messages 1, repo-local waves 1 → 9 rows over 8 present ledgers.
fn seed_home(home: &TempHome) -> PathBuf {
    let firm = home.firm_home();

    let company = firm.join("companies/acme");
    write_json(
        &company.join("metadata.json"),
        serde_json::json!({"company_id": "acme", "name": "Acme"}),
    );
    std::fs::write(
        company.join("company_os_documents.jsonl"),
        b"{\"id\":\"doc-1\"}\n{\"id\":\"doc-2\"}\n",
    )
    .unwrap();
    std::fs::write(
        company.join("company_os_approvals.jsonl"),
        b"{\"id\":\"approval-1\"}\n",
    )
    .unwrap();
    std::fs::write(company.join(".env"), b"TOKEN=never-exported\n").unwrap();
    std::fs::write(company.join("tokens.json"), b"{\"k\":\"never-exported\"}\n").unwrap();
    std::fs::create_dir_all(company.join("provider-sessions/session-1")).unwrap();
    std::fs::write(
        company.join("provider-sessions/session-1/codex.stream-json.ndjson"),
        b"{\"type\":\"never-exported\"}\n",
    )
    .unwrap();

    let space = firm.join("execution-spaces/s1");
    write_json(
        &space.join("metadata.json"),
        serde_json::json!({"space_id": "s1", "name": "S1"}),
    );
    std::fs::write(space.join("missions.jsonl"), b"{\"id\":\"mission-1\"}\n").unwrap();
    std::fs::write(space.join("mission_log.jsonl"), b"{\"id\":\"ml-1\"}\n").unwrap();
    std::fs::write(space.join("waves.jsonl"), b"{\"id\":\"wave-1\"}\n").unwrap();

    let repo = home.base().join("repo-p1");
    let project_store = home.projects_dir().join("p1");
    write_json(
        &project_store.join("metadata.json"),
        serde_json::json!({
            "project_id": "p1",
            "canonical_path": repo,
            "kind": "repo",
            "is_git_repo": false,
        }),
    );
    std::fs::write(
        project_store.join("members.jsonl"),
        b"{\"id\":\"member-1\"}\n",
    )
    .unwrap();
    std::fs::write(
        project_store.join("team_messages.jsonl"),
        b"{\"id\":\"tm-1\"}\n",
    )
    .unwrap();

    let local = repo.join(".harness");
    std::fs::create_dir_all(&local).unwrap();
    std::fs::write(local.join("waves.jsonl"), b"{\"id\":\"wave-local\"}\n").unwrap();

    let node = firm.join("nodes/node-1");
    std::fs::create_dir_all(&node).unwrap();
    std::fs::write(node.join("daemon.sock"), b"").unwrap();

    std::fs::write(firm.join("NODE_ID"), b"node-1\n").unwrap();
    repo
}

fn export_args(archive: &Path) -> Vec<String> {
    vec![
        "legacy-company-os".into(),
        "export".into(),
        "--output".into(),
        archive.display().to_string(),
    ]
}

fn verify_args(archive: &Path) -> Vec<String> {
    let archive = std::fs::canonicalize(archive).unwrap_or_else(|_| archive.to_path_buf());
    vec![
        "legacy-company-os".into(),
        "verify".into(),
        "--archive".into(),
        archive.display().to_string(),
    ]
}

#[test]
fn export_then_verify_round_trips_all_store_kinds() {
    let home = TempHome::new("legacy-company-os-round-trip");
    seed_home(&home);
    let archive = home.base().join("archive-v1");

    let output = run(&home, home.base(), &export_args(&archive));
    assert!(output.status.success(), "export failed: {output:?}");
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["format"], "legacy-company-os-v1");
    assert_eq!(summary["stores"], 6);
    // company 2 + space 3 + project 2 + repo-local 1 present ledgers.
    assert_eq!(summary["ledgers_present"], 8);
    assert_eq!(summary["rows"], 9);
    assert_eq!(summary["source_revision"].as_str().unwrap().len(), 40);
    assert_eq!(
        summary["firm_home"].as_str().unwrap(),
        std::fs::canonicalize(home.firm_home())
            .unwrap()
            .display()
            .to_string()
    );

    // Byte-exact preservation.
    assert_eq!(
        std::fs::read(archive.join("stores/company-acme/ledgers/company_os_documents.jsonl"))
            .unwrap(),
        b"{\"id\":\"doc-1\"}\n{\"id\":\"doc-2\"}\n"
    );
    assert_eq!(
        std::fs::read(archive.join("stores/repo-local-p1/ledgers/waves.jsonl")).unwrap(),
        b"{\"id\":\"wave-local\"}\n"
    );

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(archive.join("manifest.json")).unwrap()).unwrap();
    let stores = manifest["stores"].as_array().unwrap();
    let ids: Vec<&str> = stores.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(
        ids,
        [
            "company-acme",
            "node-node-1",
            "project-_global",
            "project-p1",
            "repo-local-p1",
            "space-s1"
        ]
    );
    let company = stores.iter().find(|s| s["id"] == "company-acme").unwrap();
    let excluded = company["excluded_locations"].as_array().unwrap();
    assert_eq!(excluded.len(), 3);
    let reasons: Vec<&str> = excluded
        .iter()
        .map(|e| e["reason"].as_str().unwrap())
        .collect();
    assert!(reasons.contains(&"secret_file"));
    assert!(reasons.contains(&"provider_native_transcript"));

    // Secret and transcript bytes never entered the archive.
    assert!(!archive
        .join("stores/company-acme/provider-sessions")
        .exists());
    let mut all_bytes = Vec::new();
    for entry in walk_archive(&archive) {
        all_bytes.extend_from_slice(&std::fs::read(entry).unwrap());
    }
    assert!(
        !all_bytes
            .windows("never-exported".len())
            .any(|w| w == b"never-exported"),
        "archive must not contain excluded secret/transcript bytes"
    );

    // Control-plane marker archived for offline audit.
    assert_eq!(
        std::fs::read(archive.join("markers/NODE_ID")).unwrap(),
        b"node-1\n"
    );

    let verify = run(&home, home.base(), &verify_args(&archive));
    assert!(verify.status.success(), "verify failed: {verify:?}");
    let report: serde_json::Value = serde_json::from_slice(&verify.stdout).unwrap();
    assert_eq!(report["restore_read"], "verified");
    assert_eq!(report["rows"], 9);
    assert_eq!(report["stores"], 6);
}

fn walk_archive(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn export_rejects_selectors_and_unsafe_destinations() {
    let home = TempHome::new("legacy-company-os-reject");
    seed_home(&home);
    let archive = home.base().join("archive-x");

    for selector in ["--store", "--project", "--space", "--company"] {
        let mut args = export_args(&archive);
        args.push(selector.into());
        args.push("anything".into());
        let output = run(&home, home.base(), &args);
        assert!(!output.status.success(), "{selector} must be rejected");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("is not allowed"),
            "{selector}: {output:?}"
        );
        assert!(!archive.exists());
    }

    // Existing destination is never overwritten.
    let existing = home.base().join("existing");
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("sentinel"), b"keep").unwrap();
    let output = run(&home, home.base(), &export_args(&existing));
    assert!(!output.status.success());
    assert_eq!(std::fs::read(existing.join("sentinel")).unwrap(), b"keep");

    // Destination inside the Firm home or an enumerated store is refused.
    // A project compatibility store is inside the Firm home and trips that
    // guard; the per-store guard is exercised through the repo-local
    // compatibility store, which lives outside the Firm home.
    let inside_home = home.firm_home().join("archive-inside");
    let output = run(&home, home.base(), &export_args(&inside_home));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("outside the Firm home"));
    let inside_store = home
        .base()
        .join("repo-p1")
        .join(".harness")
        .join("archive-inside");
    let output = run(&home, home.base(), &export_args(&inside_store));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("outside every enumerated source store")
    );
}

#[test]
fn export_ignores_workflow_child_store_env() {
    let home = TempHome::new("legacy-company-os-no-child-store");
    seed_home(&home);
    let decoy = home.base().join("workflow-child-store");
    std::fs::create_dir(&decoy).unwrap();
    std::fs::write(
        decoy.join("missions.jsonl"),
        b"{\"id\":\"wrong-source-sentinel\"}\n",
    )
    .unwrap();
    let archive = home.base().join("archive-no-child");
    let decoy_text = decoy.display().to_string();
    let output = run_with_env(
        &home,
        home.base(),
        &export_args(&archive),
        &[("FIRM_WORKFLOW_CHILD_STORE_ROOT", &decoy_text)],
    );
    assert!(output.status.success(), "export failed: {output:?}");
    let mut all_bytes = Vec::new();
    for entry in walk_archive(&archive) {
        all_bytes.extend_from_slice(&std::fs::read(entry).unwrap());
    }
    assert!(!all_bytes
        .windows("wrong-source-sentinel".len())
        .any(|w| w == b"wrong-source-sentinel"));
}

#[test]
fn verify_rejects_tampered_and_missing_archives_offline() {
    let home = TempHome::new("legacy-company-os-verify-fail");
    seed_home(&home);
    let archive = home.base().join("archive-tamper");
    let output = run(&home, home.base(), &export_args(&archive));
    assert!(output.status.success(), "export failed: {output:?}");

    std::fs::write(
        archive.join("stores/space-s1/ledgers/missions.jsonl"),
        b"{\"id\":\"tampered\"}\n",
    )
    .unwrap();
    let verify = run(&home, home.base(), &verify_args(&archive));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr).contains("SHA-256 mismatch"));

    let missing = home.base().join("missing-archive");
    let verify = run(&home, home.base(), &verify_args(&missing));
    assert!(!verify.status.success());
}

#[test]
fn export_with_explicit_firm_home_flag() {
    let home = TempHome::new("legacy-company-os-firm-home-flag");
    seed_home(&home);
    // A second, unrelated home proves --firm-home drives enumeration.
    let other = TempHome::new("legacy-company-os-other-home");
    let archive = home.base().join("archive-flag");
    let args = vec![
        "legacy-company-os".into(),
        "export".into(),
        "--firm-home".into(),
        home.firm_home().display().to_string(),
        "--output".into(),
        archive.display().to_string(),
    ];
    let output = run(&other, other.base(), &args);
    assert!(output.status.success(), "export failed: {output:?}");
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["stores"], 6);
    let verify = run(&other, other.base(), &verify_args(&archive));
    assert!(verify.status.success(), "verify failed: {verify:?}");
}

//! DOC-108 Stage A acceptance for `harness legacy-company-os export|verify`:
//! the machine-wide retired Company OS record archive round-trips through the
//! CLI, preserves bytes, excludes secrets and provider-native locations, and
//! verifies offline.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod firm_env;
use firm_env::TempHome;

fn run(home: &TempHome, cwd: &Path, args: &[String]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_firm"));
    command
        .args(args)
        .current_dir(cwd)
        .envs(home.envs())
        .env_remove("FIRM_ROOT")
        .env_remove("FIRM_PROJECT")
        .env_remove("FIRM_SPACE")
        .env_remove("FIRM_COMPANY");
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
    // A current-surface stray must surface in the manifest's uncontracted
    // audit list instead of being silently invisible.
    let company_store = home.firm_home().join("companies/acme");
    std::fs::write(
        company_store.join("current_surface.jsonl"),
        b"{\"id\":\"w\"}\n",
    )
    .unwrap();
    let archive = home.base().join("archive-v1");

    let output = run(&home, home.base(), &export_args(&archive));
    assert!(output.status.success(), "export failed: {output:?}");
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["format"], "legacy-company-os-v1");
    assert_eq!(summary["stores"], 6);
    assert_eq!(summary["uncontracted_ledgers"], 1);
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
    let acme = stores
        .iter()
        .find(|s| s["id"] == "company-acme")
        .expect("company store in manifest");
    assert_eq!(
        acme["uncontracted_ledgers"],
        serde_json::json!(["current_surface.jsonl"]),
        "the current-surface stray must be audited by name"
    );
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
fn export_ignores_unregistered_store_like_directory() {
    let home = TempHome::new("legacy-company-os-unregistered-directory");
    seed_home(&home);
    let decoy = home.base().join("unregistered-store");
    std::fs::create_dir(&decoy).unwrap();
    std::fs::write(
        decoy.join("missions.jsonl"),
        b"{\"id\":\"wrong-source-sentinel\"}\n",
    )
    .unwrap();
    let archive = home.base().join("archive-unregistered-directory");
    let output = run(&home, home.base(), &export_args(&archive));
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

    // Laundering a contracted ledger into the uncontracted audit list must be
    // rejected: re-export cleanly, then tamper only the manifest field.
    let archive2 = home.base().join("archive-launder");
    let output = run(&home, home.base(), &export_args(&archive2));
    assert!(output.status.success(), "export failed: {output:?}");
    let manifest_path = archive2.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest["stores"][0]["uncontracted_ledgers"] =
        serde_json::json!(["company_os_documents.jsonl"]);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let verify = run(&home, home.base(), &verify_args(&archive2));
    assert!(!verify.status.success());
    assert!(String::from_utf8_lossy(&verify.stderr)
        .contains("lists a contracted ledger as uncontracted"));
}

/// `LEDGER_CONTRACT` grows as retired surfaces are added, without bumping
/// `ARCHIVE_VERSION` (the on-disk shape is unchanged). An archive written by an
/// earlier exporter must still be rejected, but the rejection has to name the
/// exporter that produced it and say what to do about it.
#[test]
fn verify_names_the_exporter_behind_a_contract_length_mismatch() {
    let home = TempHome::new("legacy-company-os-contract-drift");
    seed_home(&home);
    let archive = home.base().join("archive-drift");
    let output = run(&home, home.base(), &export_args(&archive));
    assert!(output.status.success(), "export failed: {output:?}");

    // The freshly exported archive verifies: the drift message below is about
    // contract age, not a verifier that rejects everything.
    let verify = run(&home, home.base(), &verify_args(&archive));
    assert!(verify.status.success(), "clean verify failed: {verify:?}");

    let manifest_path = archive.join("manifest.json");
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();

    // Shape an archive as an earlier exporter would have left it: one fewer
    // contracted ledger, stamped with that exporter's own provenance.
    let mut manifest = original.clone();
    manifest["exporter_version"] = serde_json::json!("0.1.0-earlier");
    manifest["source_revision"] = serde_json::json!("1111111111111111111111111111111111111111");
    let store_id = manifest["stores"][0]["id"].as_str().unwrap().to_string();
    let ledgers = manifest["stores"][0]["ledgers"].as_array_mut().unwrap();
    ledgers.pop().expect("contracted ledgers in manifest");
    let short_len = ledgers.len();
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let verify = run(&home, home.base(), &verify_args(&archive));
    assert!(
        !verify.status.success(),
        "an older-contract archive must still fail verification: {verify:?}"
    );
    let stderr = String::from_utf8_lossy(&verify.stderr).to_string();
    assert!(
        stderr.contains(&format!(
            "store {store_id} has {short_len} ledger entries, contract requires {}",
            short_len + 1
        )),
        "the bare length facts must survive: {stderr}"
    );
    assert!(
        stderr.contains("written before this binary's ledger contract grew"),
        "the message must name the drift direction: {stderr}"
    );
    assert!(
        stderr.contains("exporter 0.1.0-earlier"),
        "the message must name the manifest's exporter version: {stderr}"
    );
    assert!(
        stderr.contains("source revision 1111111111111111111111111111111111111111"),
        "the message must name the manifest's source revision: {stderr}"
    );
    assert!(
        stderr.contains("re-export with the current binary"),
        "the message must say what to do next: {stderr}"
    );

    // Drift the other way: an archive from a newer exporter must not be told to
    // re-export with this older binary.
    let mut manifest = original;
    manifest["exporter_version"] = serde_json::json!("99.0.0-newer");
    let ledgers = manifest["stores"][0]["ledgers"].as_array_mut().unwrap();
    let extra = ledgers[0].clone();
    ledgers.push(extra);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let verify = run(&home, home.base(), &verify_args(&archive));
    assert!(!verify.status.success());
    let stderr = String::from_utf8_lossy(&verify.stderr).to_string();
    assert!(
        stderr.contains("written against a newer ledger contract")
            && stderr.contains("verify with the binary that produced it"),
        "a newer archive must not be told to re-export with this binary: {stderr}"
    );
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

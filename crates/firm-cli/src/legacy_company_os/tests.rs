use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(tag: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "legacy-company-os-test-{tag}-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp root");
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write_json(path: &Path, value: serde_json::Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create metadata parent");
    }
    fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).expect("write metadata");
}

/// Seed every store kind under one temp Firm home.
fn seed_home(root: &Path) -> (PathBuf, PathBuf) {
    let home = root.join("firm-home");
    write_json(
        &home.join("companies/acme/metadata.json"),
        serde_json::json!({"company_id": "acme", "name": "Acme"}),
    );
    write_json(
        &home.join("execution-spaces/s1/metadata.json"),
        serde_json::json!({"space_id": "s1", "name": "S1"}),
    );
    let repo = root.join("repo-p1");
    fs::create_dir_all(repo.join(".harness")).expect("repo-local store");
    fs::write(repo.join(".harness/goals.jsonl"), b"{\"id\":\"g1\"}\n").expect("seed ledger");
    write_json(
        &home.join("projects/p1/metadata.json"),
        serde_json::json!({
            "project_id": "p1",
            "canonical_path": repo,
            "kind": "repo",
            "is_git_repo": false,
        }),
    );
    fs::create_dir_all(home.join("nodes/node-1")).expect("node store");
    (home, repo)
}

#[test]
fn enumerates_all_five_store_kinds_sorted() {
    let root = TempRoot::new("enum-all");
    let (home, _repo) = seed_home(&root.0);
    let stores = enumerate_stores(&home).expect("enumerate");
    let ids: Vec<&str> = stores.iter().map(|s| s.id.as_str()).collect();
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
    let global = stores.iter().find(|s| s.id == "project-_global").unwrap();
    assert!(!global.present, "_global store was never materialized");
    let company = stores.iter().find(|s| s.id == "company-acme").unwrap();
    assert!(company.present);
    assert_eq!(company.identity.as_ref().unwrap()["company_id"], "acme");
}

#[test]
fn registry_and_scan_dedup_by_canonical_path() {
    let root = TempRoot::new("enum-dedup");
    let (home, repo) = seed_home(&root.0);
    // Registry entries pointing at the same on-disk stores the scans find.
    write_json(
        &home.join("companies/registry.json"),
        serde_json::json!({
            "format_version": 1,
            "current_company_id": "acme",
            "companies": [{
                "id": "acme",
                "name": "Acme",
                "store_root": home.join("companies/acme"),
            }],
        }),
    );
    write_json(
        &home.join("execution-spaces/registry.json"),
        serde_json::json!({
            "format_version": 1,
            "current_space_id": "s1",
            "spaces": [{
                "id": "s1",
                "name": "S1",
                "store_root": home.join("execution-spaces/s1"),
            }],
        }),
    );
    write_json(
        &home.join("projects/registry.json"),
        serde_json::json!({
            "format_version": 1,
            "current_project_id": "p1",
            "projects": [{
                "id": "p1",
                "path": repo,
                "store_root": home.join("projects/p1"),
                "kind": "repo",
            }],
        }),
    );
    let stores = enumerate_stores(&home).expect("enumerate");
    let ids: Vec<&str> = stores.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "company-acme",
            "node-node-1",
            "project-_global",
            "project-p1",
            "repo-local-p1",
            "space-s1"
        ],
        "registry + scan must not double-enumerate one physical store"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_enumerated_store_fails_closed() {
    let root = TempRoot::new("enum-symlink");
    let (home, _repo) = seed_home(&root.0);
    std::os::unix::fs::symlink(home.join("nodes/node-1"), home.join("nodes/node-alias"))
        .expect("symlink");
    let error = enumerate_stores(&home).expect_err("symlink store must fail");
    assert!(error.contains("must not be a symlink"), "{error}");
}

#[test]
fn unsafe_source_id_is_rejected() {
    let root = TempRoot::new("enum-unsafe-id");
    let home = root.0.join("firm-home");
    write_json(
        &home.join("companies/bad%2Fid/metadata.json"),
        serde_json::json!({"company_id": "bad/id", "name": "Bad"}),
    );
    let error = enumerate_stores(&home).expect_err("unsafe id must fail");
    assert!(error.contains("unsafe archive store source id"), "{error}");
}

#[test]
fn exclusion_contract_matches_names_not_content() {
    // Directory rules.
    assert_eq!(
        exclusion_for_name("provider-sessions", true),
        Some(ExclusionReason::ProviderNativeTranscript)
    );
    assert_eq!(
        exclusion_for_name("runtimes", true),
        Some(ExclusionReason::ProviderNativeRuntimeState)
    );
    // A file named like an excluded directory is not excluded by it.
    assert_eq!(exclusion_for_name("provider-sessions", false), None);
    // Secret files: exact and pattern-based.
    assert_eq!(
        exclusion_for_name(".env", false),
        Some(ExclusionReason::SecretFile)
    );
    assert_eq!(
        exclusion_for_name(".env.local", false),
        Some(ExclusionReason::SecretFile)
    );
    assert_eq!(
        exclusion_for_name("node.key", false),
        Some(ExclusionReason::SecretFile)
    );
    assert_eq!(
        exclusion_for_name("api.token", false),
        Some(ExclusionReason::SecretFile)
    );
    assert_eq!(
        exclusion_for_name("cert.pem", false),
        Some(ExclusionReason::SecretFile)
    );
    // IPC / locks.
    assert_eq!(
        exclusion_for_name("daemon.sock", false),
        Some(ExclusionReason::EphemeralIpcOrLock)
    );
    assert_eq!(
        exclusion_for_name("node-fabric.lock", false),
        Some(ExclusionReason::EphemeralIpcOrLock)
    );
    // Contract ledgers and ordinary current ledgers are never excluded.
    assert_eq!(
        exclusion_for_name("company_os_documents.jsonl", false),
        None
    );
    assert_eq!(exclusion_for_name("missions.jsonl", false), None);
    assert_eq!(exclusion_for_name("work_operations.jsonl", false), None);
    assert_eq!(exclusion_for_name("metadata.json", false), None);
}

/// Seed a store with contract ledgers, excluded locations, and a
/// non-contract non-excluded ledger.
fn seed_company_os_surface(home: &Path) {
    let store = home.join("companies/acme");
    fs::write(
        store.join("company_os_documents.jsonl"),
        b"{\"id\":\"doc-1\",\"title\":\"One\"}\n{\"id\":\"doc-2\",\"title\":\"Two\"}\n",
    )
    .expect("documents");
    fs::write(
        store.join("company_os_blocks.jsonl"),
        b"{\"id\":\"blk-1\",\"document_id\":\"doc-1\"}\n",
    )
    .expect("blocks");
    fs::write(
        store.join("missions.jsonl"),
        b"{\"id\":\"mission-1\",\"title\":\"M\"}\n",
    )
    .expect("missions");
    fs::write(
        store.join("team_messages.jsonl"),
        b"{\"id\":\"tm-1\",\"body\":\"hi\"}\n",
    )
    .expect("team messages");
    // Present-but-excluded locations (content must never be archived).
    fs::create_dir_all(store.join("provider-sessions/session-1")).expect("provider-sessions");
    fs::write(
        store.join("provider-sessions/session-1/codex.stream-json.ndjson"),
        b"{\"type\":\"transcript\"}\n",
    )
    .expect("transcript");
    fs::create_dir_all(store.join("runtimes/runtime-1")).expect("runtimes");
    fs::write(store.join(".env"), b"TOKEN=never-exported\n").expect(".env");
    fs::write(store.join("tokens.json"), b"{\"api\":\"never-exported\"}\n").expect("tokens");
    // Non-contract, non-excluded current ledger: neither archived nor
    // listed as excluded.
    fs::write(store.join("teams.jsonl"), b"{\"id\":\"team-1\"}\n").expect("teams");
}

#[test]
fn export_archives_contract_preserves_bytes_and_excludes_secrets() {
    let root = TempRoot::new("export-surface");
    let (home, _repo) = seed_home(&root.0);
    seed_company_os_surface(&home);
    fs::write(home.join("NODE_ID"), b"node-1\n").expect("node id");
    let output = root.0.join("archive-out");

    let summary = export_archive(&home, &output).expect("export");
    assert_eq!(summary.format, "legacy-company-os-v1");
    assert_eq!(summary.source_revision.len(), 40);
    assert!(summary.stores >= 5, "seeded kinds: {}", summary.stores);

    // Byte-exact preservation of a seeded ledger.
    assert_eq!(
        fs::read(output.join("stores/company-acme/ledgers/company_os_documents.jsonl"))
            .expect("archived documents"),
        b"{\"id\":\"doc-1\",\"title\":\"One\"}\n{\"id\":\"doc-2\",\"title\":\"Two\"}\n"
    );
    // Absent contract ledgers are still enumerated as empty files.
    assert_eq!(
        fs::read(output.join("stores/company-acme/ledgers/company_os_views.jsonl"))
            .expect("archived empty views"),
        b""
    );

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("manifest.json")).expect("manifest"))
            .expect("manifest json");
    let stores = manifest["stores"].as_array().unwrap();
    let company = stores
        .iter()
        .find(|s| s["id"] == "company-acme")
        .expect("company store");
    let ledgers = company["ledgers"].as_array().unwrap();
    assert_eq!(ledgers.len(), LEDGER_CONTRACT.len());
    let documents = ledgers
        .iter()
        .find(|l| l["ledger"] == "company_os_documents.jsonl")
        .unwrap();
    assert_eq!(documents["present"], true);
    assert_eq!(documents["rows"], 2);
    assert_eq!(documents["section"], "company_os");
    assert_eq!(documents["object_type"], "company_os_document");
    assert_eq!(documents["schema_version"], "company-os-ledger-v1");
    assert!(documents["source_path"].as_str().unwrap().starts_with('/'));
    let waves = ledgers
        .iter()
        .find(|l| l["ledger"] == "waves.jsonl")
        .unwrap();
    assert_eq!(waves["present"], false);
    assert_eq!(waves["rows"], 0);
    let team_messages = ledgers
        .iter()
        .find(|l| l["ledger"] == "team_messages.jsonl")
        .unwrap();
    assert_eq!(team_messages["section"], "retired_history");
    assert_eq!(team_messages["rows"], 1);

    // Excluded locations are listed with reasons; their content is absent.
    let excluded = company["excluded_locations"].as_array().unwrap();
    let reasons: std::collections::BTreeMap<&str, &str> = excluded
        .iter()
        .map(|e| {
            (
                e["path"].as_str().unwrap().rsplit('/').next().unwrap(),
                e["reason"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(reasons["provider-sessions"], "provider_native_transcript");
    assert_eq!(reasons["runtimes"], "provider_native_runtime_state");
    assert_eq!(reasons[".env"], "secret_file");
    assert_eq!(reasons["tokens.json"], "secret_file");
    let archived_text = fs::read_to_string(output.join("manifest.json")).unwrap();
    assert!(!archived_text.contains("never-exported"));
    assert!(!output
        .join("stores/company-acme/ledgers/teams.jsonl")
        .exists());
    assert!(!output
        .join("stores/company-acme/provider-sessions")
        .exists());

    // Control-plane markers are archived for offline audit.
    assert_eq!(
        fs::read(output.join("markers/NODE_ID")).expect("marker"),
        b"node-1\n"
    );
    // Sources are untouched.
    assert_eq!(
        fs::read(home.join("companies/acme/company_os_documents.jsonl")).unwrap(),
        b"{\"id\":\"doc-1\",\"title\":\"One\"}\n{\"id\":\"doc-2\",\"title\":\"Two\"}\n"
    );
    // Skeleton verify accepts the archive (full contract checks land
    // next). Canonicalize first: the verifier refuses symlinked
    // ancestors, and the macOS temp root lives under /var -> /private/var.
    let canonical_output = fs::canonicalize(&output).expect("canonical archive path");
    verify_archive(&canonical_output).expect("verify skeleton");
}

#[test]
fn export_fails_when_source_moves_mid_export() {
    let root = TempRoot::new("export-drift");
    let (home, _repo) = seed_home(&root.0);
    seed_company_os_surface(&home);
    let stores = enumerate_stores(&home).expect("enumerate");
    let before = snapshot_inputs(&stores, &home).expect("snapshot");
    fs::write(
        home.join("companies/acme/company_os_documents.jsonl"),
        b"{\"id\":\"doc-3\"}\n",
    )
    .expect("drift");
    let error =
        ensure_inputs_unchanged(&before, &stores, &home).expect_err("drift must fail the export");
    assert!(error.contains("refusing mixed-moment archive"), "{error}");
}

/// Export the seeded home to a fresh archive dir and return its
/// canonical path (verify refuses symlinked temp ancestors).
fn export_seeded(root: &Path, home: &Path, name: &str) -> PathBuf {
    let output = root.join(name);
    export_archive(home, &output).expect("export");
    fs::canonicalize(&output).expect("canonical archive")
}

fn read_manifest(archive: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(archive.join("manifest.json")).expect("manifest"))
        .expect("manifest json")
}

fn write_manifest(archive: &Path, manifest: &serde_json::Value) {
    let mut bytes = serde_json::to_vec_pretty(manifest).unwrap();
    bytes.push(b'\n');
    fs::write(archive.join("manifest.json"), bytes).expect("write manifest");
}

#[test]
fn verify_rejects_tampered_bytes_manifest_and_smuggled_exclusions() {
    let root = TempRoot::new("verify-tamper");
    let (home, _repo) = seed_home(&root.0);
    seed_company_os_surface(&home);

    // 1. Byte tampering in an archived ledger.
    let archive = export_seeded(&root.0, &home, "a1");
    fs::write(
        archive.join("stores/company-acme/ledgers/company_os_documents.jsonl"),
        b"{\"id\":\"doc-x\"}\n",
    )
    .unwrap();
    let error = verify_archive(&archive).expect_err("tampered bytes");
    assert!(error.contains("SHA-256 mismatch"), "{error}");

    // 2. A weakened echoed exclusion contract.
    let archive = export_seeded(&root.0, &home, "a2");
    let mut manifest = read_manifest(&archive);
    manifest["exclusion_contract"]
        .as_array_mut()
        .unwrap()
        .retain(|rule| rule["name"] != "tokens.json");
    write_manifest(&archive, &manifest);
    let error = verify_archive(&archive).expect_err("weakened contract");
    assert!(error.contains("exclusion contract"), "{error}");

    // 3. Doctored row count in a ledger entry (totals no longer
    //    recompute before the restore-read proof even runs).
    let archive = export_seeded(&root.0, &home, "a3");
    let mut manifest = read_manifest(&archive);
    let company = manifest["stores"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["id"] == "company-acme")
        .unwrap();
    company["ledgers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|l| l["ledger"] == "company_os_documents.jsonl")
        .unwrap()["rows"] = serde_json::json!(99);
    write_manifest(&archive, &manifest);
    let error = verify_archive(&archive).expect_err("doctored rows");
    assert!(
        error.contains("totals do not recompute") || error.contains("manifest mismatch"),
        "{error}"
    );

    // 4. A hand-added file smuggled from an excluded location.
    let archive = export_seeded(&root.0, &home, "a4");
    write_archive_file(
        &archive,
        "stores/company-acme/ledgers/sneaky.jsonl",
        b"{}\n",
    )
    .expect("plant file");
    let mut manifest = read_manifest(&archive);
    let store_path = manifest["stores"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "company-acme")
        .unwrap()["path"]
        .as_str()
        .unwrap()
        .to_string();
    manifest["files"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "path": "stores/company-acme/ledgers/sneaky.jsonl",
            "category": "legacy_ledger",
            "sha256": sha256_hex(b"{}\n"),
            "bytes": 3,
            "line_count": 1,
            "rows": 1,
            "source_path": format!("{store_path}/provider-sessions/sneaky.jsonl"),
        }));
    write_manifest(&archive, &manifest);
    let error = verify_archive(&archive).expect_err("smuggled exclusion");
    assert!(error.contains("excluded location"), "{error}");
}

#[test]
fn restore_read_recovers_rows_from_detached_copy() {
    let root = TempRoot::new("verify-restore");
    let (home, _repo) = seed_home(&root.0);
    seed_company_os_surface(&home);
    let archive = export_seeded(&root.0, &home, "a1");
    let summary = verify_archive(&archive).expect("verify");
    assert_eq!(summary.restore_read, "verified");
    // Seeded rows: acme documents 2 + blocks 1 + missions 1 +
    // team_messages 1, plus repo-local goals.jsonl is NOT in this
    // contract.
    assert_eq!(summary.rows, 5);
    // Deleting the live sources must not affect verification.
    fs::remove_dir_all(home.join("companies/acme")).expect("remove live store");
    let summary = verify_archive(&archive).expect("verify offline");
    assert_eq!(summary.rows, 5);
}

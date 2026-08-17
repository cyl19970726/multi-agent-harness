//! Read-only export + verification for the retired Company OS record surface
//! (DOC-108 Stage A: the machinery that makes later deletion safe).
//!
//! The archive preserves source JSONL bytes byte-for-byte. Per source record
//! store enumerated on this machine (Company Stores, Execution Space stores,
//! project and repo-local compatibility stores, machine node stores) the
//! manifest records the absolute source location, ledger/object type, schema
//! version, row count, byte count, and SHA-256 of every contracted legacy
//! ledger, plus the exporter version and the exact source revision of the
//! exporting binary. Secret and provider-native locations are listed as
//! excluded and are never exported.
//!
//! Hard boundaries: no secret/token export, no provider-native transcript
//! copying, no name-based mapping into current Work, and no deletion — this
//! stage mutates nothing outside the archive destination.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::legacy_export::{
    canonical_string, physical_line_count, reject_symlink_ancestors,
    reject_symlink_or_non_directory, resolve_with_existing_ancestor, sha256_hex,
    validate_relative_archive_path, write_archive_file, StagingDir,
};

const ARCHIVE_FORMAT: &str = "legacy-company-os-v1";
const ARCHIVE_VERSION: u32 = 1;
const EXPORTER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Same compile-time provenance as `--build-info`: build.rs embeds
/// `FIRM_BUILD_GIT_REV`, and a build outside a git checkout falls back to
/// "unknown" instead of failing.
fn source_revision() -> &'static str {
    option_env!("FIRM_BUILD_GIT_REV").unwrap_or("unknown")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSummary {
    pub format: String,
    pub archive: String,
    pub firm_home: String,
    pub exporter_version: String,
    pub source_revision: String,
    pub stores: usize,
    pub ledgers_present: u64,
    pub rows: u64,
    pub bytes: u64,
    pub files: usize,
    pub excluded_locations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifySummary {
    pub format: String,
    pub archive: String,
    pub stores: usize,
    pub ledgers_present: u64,
    pub rows: u64,
    pub files: usize,
    pub restore_read: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    exporter_version: String,
    source_revision: String,
    exported_at_unix_ms: u128,
    firm_home: String,
    stores: Vec<ManifestStore>,
    files: Vec<ManifestFile>,
    totals: ManifestTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestStore {
    /// Stable archive id: `<kind>:<source-id>`, path-safe.
    id: String,
    kind: String,
    /// Absolute source location at export time.
    path: String,
    /// Whether the store directory existed when enumerated.
    present: bool,
    /// Identity fields copied from the store's own metadata.json, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<serde_json::Value>,
    ledgers: Vec<ManifestLedger>,
    excluded_locations: Vec<ExcludedLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestLedger {
    ledger: String,
    section: String,
    object_type: String,
    /// Exporter read-contract tag for this ledger family. Source rows do not
    /// carry per-row schema versions; this names the archive contract under
    /// which the preserved bytes remain readable.
    schema_version: String,
    /// Absolute source location at export time.
    source_path: String,
    present: bool,
    rows: u64,
    bytes: u64,
    sha256: String,
    archive_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExcludedLocation {
    /// Absolute source location explicitly never exported.
    path: String,
    reason: String,
    present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestFile {
    path: String,
    category: String,
    sha256: String,
    bytes: u64,
    line_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rows: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestTotals {
    stores: u64,
    ledgers_present: u64,
    rows: u64,
    bytes: u64,
    excluded_locations_present: u64,
}

/// Create one immutable archive of the retired Company OS record surface.
/// Every source store is only ever opened for read; nothing is deleted.
pub fn export_archive(firm_home: &Path, output: &Path) -> Result<ExportSummary, String> {
    reject_symlink_or_non_directory(firm_home, "Firm home")?;
    if output.exists() {
        return Err(format!(
            "archive destination already exists (refusing to overwrite): {}",
            output.display()
        ));
    }
    reject_output_inside_firm_home(firm_home, output)?;
    let stores = enumerate_stores(firm_home)?;

    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("create archive parent: {e}"))?;
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let output_name = output
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "archive destination needs a valid final path component".to_string())?;
    let staging_path = parent.join(format!(
        ".{output_name}.partial-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir(&staging_path).map_err(|e| format!("create archive staging dir: {e}"))?;
    let mut staging = StagingDir {
        path: staging_path,
        keep: false,
    };

    let mut files: Vec<ManifestFile> = Vec::new();
    let mut manifest_stores: Vec<ManifestStore> = Vec::new();
    let mut ledgers_present = 0_u64;
    let mut total_rows = 0_u64;
    let mut total_bytes = 0_u64;
    let mut excluded_locations = 0_u64;
    for store in &stores {
        let archived = archive_store(store, &staging.path, &mut files)?;
        ledgers_present += archived.ledgers.iter().filter(|l| l.present).count() as u64;
        total_rows += archived.ledgers.iter().map(|l| l.rows).sum::<u64>();
        total_bytes += archived.ledgers.iter().map(|l| l.bytes).sum::<u64>();
        excluded_locations += archived
            .excluded_locations
            .iter()
            .filter(|e| e.present)
            .count() as u64;
        manifest_stores.push(archived);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let totals = ManifestTotals {
        stores: manifest_stores.len() as u64,
        ledgers_present,
        rows: total_rows,
        bytes: total_bytes,
        excluded_locations_present: excluded_locations,
    };
    let manifest = Manifest {
        format: ARCHIVE_FORMAT.into(),
        version: ARCHIVE_VERSION,
        exporter_version: EXPORTER_VERSION.into(),
        source_revision: source_revision().into(),
        exported_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        firm_home: canonical_string(firm_home),
        stores: manifest_stores,
        files,
        totals,
    };
    let mut manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| format!("serialize manifest: {e}"))?;
    manifest_bytes.push(b'\n');
    write_archive_file(&staging.path, "manifest.json", &manifest_bytes)?;

    fs::rename(&staging.path, output).map_err(|e| {
        format!(
            "publish archive {} -> {}: {e}",
            staging.path.display(),
            output.display()
        )
    })?;
    staging.keep = true;

    Ok(ExportSummary {
        format: ARCHIVE_FORMAT.into(),
        archive: canonical_string(output),
        firm_home: canonical_string(firm_home),
        exporter_version: EXPORTER_VERSION.into(),
        source_revision: source_revision().into(),
        stores: manifest.stores.len(),
        ledgers_present,
        rows: total_rows,
        bytes: total_bytes,
        files: manifest.files.len(),
        excluded_locations,
    })
}

/// Verify an archive without consulting any live store: manifest hashes,
/// byte/line counts, and the contract cross-checks.
pub fn verify_archive(archive: &Path) -> Result<VerifySummary, String> {
    reject_symlink_ancestors(archive, "archive directory")?;
    reject_symlink_or_non_directory(archive, "archive directory")?;
    let manifest_path = archive.join("manifest.json");
    let manifest_bytes =
        fs::read(&manifest_path).map_err(|e| format!("read {}: {e}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| format!("parse {}: {e}", manifest_path.display()))?;
    if manifest.format != ARCHIVE_FORMAT || manifest.version != ARCHIVE_VERSION {
        return Err(format!(
            "unsupported archive format/version: {}/{}",
            manifest.format, manifest.version
        ));
    }

    let mut entries = std::collections::BTreeMap::new();
    for entry in &manifest.files {
        validate_relative_archive_path(&entry.path)?;
        let path = archive.join(&entry.path);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("inspect archived file {}: {e}", path.display()))?;
        if !metadata.file_type().is_file() {
            return Err(format!(
                "manifest path is not a regular file: {}",
                entry.path
            ));
        }
        let bytes =
            fs::read(&path).map_err(|e| format!("read archived file {}: {e}", path.display()))?;
        let actual_hash = sha256_hex(&bytes);
        if actual_hash != entry.sha256 {
            return Err(format!(
                "SHA-256 mismatch for {}: manifest {}, actual {}",
                entry.path, entry.sha256, actual_hash
            ));
        }
        if bytes.len() as u64 != entry.bytes {
            return Err(format!(
                "byte-count mismatch for {}: manifest {}, actual {}",
                entry.path,
                entry.bytes,
                bytes.len()
            ));
        }
        let lines = physical_line_count(&bytes);
        if lines != entry.line_count {
            return Err(format!(
                "line-count mismatch for {}: manifest {}, actual {}",
                entry.path, entry.line_count, lines
            ));
        }
        if entries.insert(entry.path.clone(), entry).is_some() {
            return Err(format!("duplicate manifest path: {}", entry.path));
        }
    }

    Ok(VerifySummary {
        format: ARCHIVE_FORMAT.into(),
        archive: canonical_string(archive),
        stores: manifest.stores.len(),
        ledgers_present: manifest.totals.ledgers_present,
        rows: manifest.totals.rows,
        files: manifest.files.len(),
        restore_read: "pending_contract_checks".into(),
    })
}

/// One enumerated source record store on this machine.
#[derive(Debug, Clone)]
struct SourceStore {
    /// Archive id `<kind>-<source-id>`, validated path-safe.
    id: String,
    kind: &'static str,
    root: PathBuf,
    present: bool,
    identity: Option<serde_json::Value>,
}

/// Enumerate every source record store under the resolved Firm home: Company
/// Stores, Execution Space stores, project-derived compatibility stores,
/// repo-local compatibility stores (`<project_root>/.harness`), and machine
/// node stores. Registries and on-disk layouts are both consulted and deduped
/// by canonical path; the store id list is never hardcoded.
///
/// Scope note: the product resolves exactly one Firm home (`FIRM_HOME`, else
/// `~/.firm`, else the legacy `~/.harness` fallback). Stores under that home
/// plus repo-local compatibility stores of its known projects are the machine
/// surface the product can see; a second home the product itself would never
/// resolve is out of scope.
fn enumerate_stores(firm_home: &Path) -> Result<Vec<SourceStore>, String> {
    let mut stores = Vec::new();
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    if let Ok(home) = fs::canonicalize(firm_home) {
        seen.insert(home);
    }

    // 1. Company Stores (ADR 0040): the company layer merges registry entries
    //    and on-disk stores with metadata.json.
    for ctx in crate::company_store::list_companies(firm_home)
        .map_err(|e| format!("enumerate Company Stores: {e}"))?
    {
        let identity = store_identity(&ctx.store_root);
        push_store(
            &mut stores,
            &mut seen,
            "company",
            &ctx.id,
            ctx.store_root,
            identity,
        )?;
    }

    // 2. Execution Space stores (ADR 0042): registry entries, plus on-disk
    //    space stores the registry does not know (mirrors the company/project
    //    layers' registry+scan merge, which `list_spaces` does not do).
    for space in crate::execution_space::list_spaces(firm_home)
        .map_err(|e| format!("enumerate Execution Space stores: {e}"))?
    {
        let identity = store_identity(&space.store_root);
        push_store(
            &mut stores,
            &mut seen,
            "space",
            &space.id,
            space.store_root,
            identity,
        )?;
    }
    for dir in child_directories(&crate::execution_space::spaces_dir(firm_home))? {
        let identity = store_identity(&dir);
        let id = identity
            .as_ref()
            .and_then(|value| value.get("space_id"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| dir.file_name().and_then(|s| s.to_str()).map(str::to_string))
            .ok_or_else(|| format!("non-UTF-8 Execution Space dir name: {}", dir.display()))?;
        push_store(&mut stores, &mut seen, "space", &id, dir, identity)?;
    }

    // 3. Project-derived compatibility stores: the project layer merges
    //    registry entries, on-disk stores with metadata.json, and the reserved
    //    _global project.
    let projects = crate::project::list_projects(firm_home)
        .map_err(|e| format!("enumerate Project compatibility stores: {e}"))?;
    for ctx in &projects {
        let identity = store_identity(&ctx.store_root);
        push_store(
            &mut stores,
            &mut seen,
            "project",
            &ctx.id,
            ctx.store_root.clone(),
            identity,
        )?;
    }

    // 4. Repo-local compatibility stores (`<project_root>/.harness`), the
    //    pre-centralization layout. The reserved _global project is skipped:
    //    its root is HOME, and `<home>/.harness` is the legacy Firm home
    //    fallback — when that fallback is active it IS the resolved Firm home
    //    this enumeration already covers, and the canonical-path dedup above
    //    would drop a duplicate probe anyway.
    for ctx in &projects {
        if ctx.id == harness_core::GLOBAL_PROJECT_ID {
            continue;
        }
        let local = ctx.project_root.join(".harness");
        if !local.exists() {
            continue;
        }
        reject_symlink_or_non_directory(&local, "repo-local source store")?;
        let mut identity = store_identity(&local).unwrap_or_else(|| serde_json::json!({}));
        if let Some(target) = crate::project::read_migrated_marker(&local)
            .map_err(|e| format!("read migrated marker in {}: {e}", local.display()))?
        {
            identity["migrated_to_central"] =
                serde_json::Value::String(target.display().to_string());
        }
        push_store(
            &mut stores,
            &mut seen,
            "repo-local",
            &ctx.id,
            local,
            Some(identity),
        )?;
    }

    // 5. Machine node stores (`<firm_home>/nodes/<node_id>/`), the
    //    machine-scoped NodeDaemon surface.
    for dir in child_directories(&firm_home.join("nodes"))? {
        let id = dir
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("non-UTF-8 node store dir name: {}", dir.display()))?
            .to_string();
        let identity = store_identity(&dir);
        push_store(&mut stores, &mut seen, "node", &id, dir, identity)?;
    }

    stores.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(stores)
}

fn push_store(
    stores: &mut Vec<SourceStore>,
    seen: &mut BTreeSet<PathBuf>,
    kind: &'static str,
    source_id: &str,
    root: PathBuf,
    identity: Option<serde_json::Value>,
) -> Result<(), String> {
    validate_store_source_id(source_id)?;
    let present = root.is_dir();
    // Dedup by canonical path so one physical store reached through two
    // routes (registry + on-disk scan, or repo-local alias) is exported once.
    let dedup_key = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    if !seen.insert(dedup_key) {
        return Ok(());
    }
    stores.push(SourceStore {
        id: format!("{kind}-{source_id}"),
        kind,
        root,
        present,
        identity,
    });
    Ok(())
}

fn validate_store_source_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!("unsafe archive store source id: {value}"));
    }
    Ok(())
}

/// The store's own identity record (`metadata.json`), kept as opaque JSON:
/// company_id/name, space_id/name, or project_id/canonical_path/kind.
fn store_identity(root: &Path) -> Option<serde_json::Value> {
    let bytes = fs::read(root.join("metadata.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value.is_object().then_some(value)
}

fn child_directories(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(error) => return Err(format!("read directory {}: {error}", dir.display())),
    };
    for entry in read_dir {
        let entry =
            entry.map_err(|e| format!("read entry in {}: {e}", dir.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|e| format!("inspect enumerated store {}: {e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "enumerated store must not be a symlink: {}",
                path.display()
            ));
        }
        if metadata.is_dir() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Archive one enumerated store's contracted legacy ledgers into the staging
/// directory and return its manifest section.
fn archive_store(
    store: &SourceStore,
    _archive_root: &Path,
    _files: &mut Vec<ManifestFile>,
) -> Result<ManifestStore, String> {
    if store.present {
        reject_symlink_or_non_directory(&store.root, "enumerated source store")?;
    }
    // DOC-108 increment 3 wires the ledger contract + secret exclusion in.
    Ok(ManifestStore {
        id: store.id.clone(),
        kind: store.kind.into(),
        path: canonical_string(&store.root),
        present: store.present,
        identity: store.identity.clone(),
        ledgers: Vec::new(),
        excluded_locations: Vec::new(),
    })
}

fn reject_output_inside_firm_home(firm_home: &Path, output: &Path) -> Result<(), String> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = resolve_with_existing_ancestor(parent)?;
    let home = fs::canonicalize(firm_home)
        .map_err(|e| format!("canonicalize Firm home {}: {e}", firm_home.display()))?;
    if parent.starts_with(&home) {
        return Err(format!(
            "archive destination must be outside the Firm home: {}",
            output.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
        std::os::unix::fs::symlink(
            home.join("nodes/node-1"),
            home.join("nodes/node-alias"),
        )
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
}

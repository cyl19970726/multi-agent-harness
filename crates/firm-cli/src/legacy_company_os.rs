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
    id: String,
    kind: &'static str,
    root: PathBuf,
    present: bool,
    identity: Option<serde_json::Value>,
}

/// Enumerate every source record store on this machine: Company Stores,
/// Execution Space stores, project/repo-local compatibility stores, and
/// machine node stores. Never hardcodes a fixed store list; registries and
/// on-disk layouts are both consulted.
fn enumerate_stores(_firm_home: &Path) -> Result<Vec<SourceStore>, String> {
    // DOC-108 increment 2 wires the five store kinds in.
    Ok(Vec::new())
}

/// Archive one enumerated store's contracted legacy ledgers into the staging
/// directory and return its manifest section.
fn archive_store(
    _store: &SourceStore,
    _archive_root: &Path,
    _files: &mut Vec<ManifestFile>,
) -> Result<ManifestStore, String> {
    // DOC-108 increment 3 wires the ledger contract + secret exclusion in.
    unreachable!("no stores are enumerated before increment 2")
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

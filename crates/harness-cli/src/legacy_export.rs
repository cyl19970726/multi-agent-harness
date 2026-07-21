//! Read-only archive support for the retired Goal / Task-Graph records.
//!
//! The archive deliberately preserves source JSONL bytes. It does not deserialize
//! rows into the current Rust domain model, rename Tasks to WorkItems, or create
//! Mission/Wave compatibility projections.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const ARCHIVE_FORMAT: &str = "legacy-goal-task-v1";
const ARCHIVE_VERSION: u32 = 1;
const EXPORTER_VERSION: &str = env!("CARGO_PKG_VERSION");

// This is an authorization contract, not a pattern-based repair. The one
// historical mismatch accepted by R0 is pinned to the exact project, source
// row, identity, target, bytes, and semantic predicate observed during the
// migration audit. A new or changed mismatch is never accepted implicitly.
const AUTHORIZED_ANOMALY_PROJECT_ID: &str = "multi-agent-harness";
const AUTHORIZED_ANOMALY_SOURCE_ID: &str = "central";
const AUTHORIZED_ANOMALY_LEDGER: &str = "decisions.jsonl";
const AUTHORIZED_ANOMALY_LINE: u64 = 44;
const AUTHORIZED_ANOMALY_RECORD_ID: &str = "decision-1783272619378-p31551-2";
const AUTHORIZED_ANOMALY_FIELD: &str = "/task_id";
const AUTHORIZED_ANOMALY_TARGET: &str = "goal-custom-workflow-phase-runner-v1";
const AUTHORIZED_ANOMALY_RAW_SHA256: &str =
    "66d5c9d0a7a133a6adb021c95ea7b7d9ded1f16d87b08dcead8edb58934c9a55";
const AUTHORIZED_ANOMALY_DECISION_KIND: &str = "phase_verdict";

const LEGACY_LEDGERS: &[&str] = &[
    "goals.jsonl",
    "tasks.jsonl",
    "goal_designs.jsonl",
    "goal_evaluations.jsonl",
    "goal_cases.jsonl",
    "goal_orchestration_runs.jsonl",
];

const INTERPRETATION_PATHS: &[&str] = &[
    "schemas/goal.schema.json",
    "schemas/task.schema.json",
    "schemas/goal-design.schema.json",
    "schemas/goal-evaluation.schema.json",
    "schemas/goal-case.schema.json",
    "schemas/fixtures/goal",
    "schemas/fixtures/task",
    "schemas/fixtures/goal-design",
    "schemas/fixtures/goal-evaluation",
    "schemas/fixtures/goal-case",
    "examples/goal-cases",
    // These retired Skills are contract-required interpretation materials. The
    // repository intentionally no longer contains them, so the manifest must
    // state `not_present_in_source` rather than silently omitting them.
    "skills/generic-agent-harness",
    "skills/star-goal",
    "skills/star-planner",
];

type FileMeta = (String, Option<String>, Option<bool>, Option<Vec<u64>>);
type FileMetaMap = BTreeMap<String, FileMeta>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSummary {
    pub format: String,
    pub archive: String,
    pub project_id: String,
    pub source_stores: Vec<String>,
    pub files: usize,
    pub linked_rows: u64,
    pub edges: u64,
    pub unresolved_required_edges: u64,
    pub known_anomalies: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifySummary {
    pub format: String,
    pub archive: String,
    pub files: usize,
    pub edges: u64,
    pub closure: String,
    pub known_anomalies: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    exporter_version: String,
    exported_at_unix_ms: u128,
    project: ManifestProject,
    sources: Vec<ManifestSource>,
    source_comparisons: Vec<SourceComparison>,
    interpretation_materials: Vec<InterpretationMaterial>,
    files: Vec<ManifestFile>,
    known_anomalies: Vec<KnownAnomaly>,
    closure: ClosureSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct InterpretationMaterial {
    source_path: String,
    source_present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    archived_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestProject {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestSource {
    id: String,
    kind: String,
    path: String,
    snapshot_sha256: String,
    snapshot_files: Vec<SnapshotFile>,
    linked_ledgers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SnapshotFile {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SourceComparison {
    left: String,
    right: String,
    shared_same: u64,
    shared_different: u64,
    left_only: u64,
    right_only: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestFile {
    path: String,
    category: String,
    sha256: String,
    bytes: u64,
    line_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_present: Option<bool>,
    /// For a linked-row subset, archive line N came from source line
    /// `source_lines[N - 1]`. This retains provenance without changing row bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_lines: Option<Vec<u64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ClosureSummary {
    edge_count: u64,
    required_edge_count: u64,
    unresolved_required_edges: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct KnownAnomaly {
    anomaly_kind: String,
    source_id: String,
    ledger: String,
    line: u64,
    record_id: String,
    field: String,
    target: String,
    raw_line_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Edge {
    source_id: String,
    source_ledger: String,
    source_archive_path: String,
    source_archive_line: u64,
    source_store_line: u64,
    source_record_id: String,
    field: String,
    target_kind: String,
    target_id: String,
    closure_required: bool,
}

#[derive(Debug, Clone)]
struct ArchivedLedger {
    source_id: String,
    ledger: String,
    archive_path: String,
    bytes: Vec<u8>,
    source_lines: Vec<u64>,
}

#[derive(Debug, Clone)]
struct SourceSpec {
    id: String,
    kind: String,
    root: PathBuf,
    before: Vec<SnapshotFile>,
}

#[derive(Debug, Default)]
struct Inventory {
    goals: BTreeSet<String>,
    tasks: BTreeSet<String>,
    phases: BTreeSet<String>,
    goal_designs: BTreeSet<String>,
    goal_evaluations: BTreeSet<String>,
}

#[derive(Debug, Clone, Copy)]
enum TargetKind {
    Goal,
    GoalOrDescriptionRef,
    Task,
    TaskOrDescriptionRef,
    Phase,
    GoalDesign,
    GoalDesignRef,
    GoalEvaluation,
}

impl TargetKind {
    fn label(self) -> &'static str {
        match self {
            Self::Goal | Self::GoalOrDescriptionRef => "goal",
            Self::Task | Self::TaskOrDescriptionRef => "task",
            Self::Phase => "goal_phase",
            Self::GoalDesign | Self::GoalDesignRef => "goal_design",
            Self::GoalEvaluation => "goal_evaluation",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LinkRule {
    ledger: &'static str,
    /// JSON object keys separated by `/`; `*` is one array element. Rules are
    /// intentionally finite and never descend through dynamic payload fields.
    path: &'static str,
    target: TargetKind,
}

const LINK_RULES: &[LinkRule] = &[
    LinkRule {
        ledger: "goals.jsonl",
        path: "goal_design_id",
        target: TargetKind::GoalDesign,
    },
    LinkRule {
        ledger: "goals.jsonl",
        path: "knowledge/*/goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "goals.jsonl",
        path: "knowledge/*/phase_id",
        target: TargetKind::Phase,
    },
    LinkRule {
        ledger: "goals.jsonl",
        path: "knowledge/*/task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "tasks.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "tasks.jsonl",
        path: "parent_task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "tasks.jsonl",
        path: "depends_on_task_ids",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "tasks.jsonl",
        path: "phase_id",
        target: TargetKind::Phase,
    },
    LinkRule {
        ledger: "goal_designs.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "goal_designs.jsonl",
        path: "task_graph",
        target: TargetKind::TaskOrDescriptionRef,
    },
    LinkRule {
        ledger: "goal_evaluations.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "goal_evaluations.jsonl",
        path: "follow_up_task_ids",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "goal_evaluations.jsonl",
        path: "proposed_goal_ids",
        target: TargetKind::GoalOrDescriptionRef,
    },
    LinkRule {
        ledger: "goal_cases.jsonl",
        path: "source_goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "goal_cases.jsonl",
        path: "goal_design_ref",
        target: TargetKind::GoalDesignRef,
    },
    LinkRule {
        ledger: "goal_cases.jsonl",
        path: "evaluation_ref",
        target: TargetKind::GoalEvaluation,
    },
    LinkRule {
        ledger: "goal_orchestration_runs.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "goal_orchestration_runs.jsonl",
        path: "phase_runs/*/phase_id",
        target: TargetKind::Phase,
    },
    LinkRule {
        ledger: "agent_events.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "decisions.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "decisions.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "decisions.jsonl",
        path: "follow_up_task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "evidence.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "evidence.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "gaps.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "gaps.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "members.jsonl",
        path: "current_task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "messages.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "proposals.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "provider_child_threads.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "reviews.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "reviews.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "workflow_runs.jsonl",
        path: "goal_id",
        target: TargetKind::Goal,
    },
    LinkRule {
        ledger: "workflow_runs.jsonl",
        path: "phase_id",
        target: TargetKind::Phase,
    },
    LinkRule {
        ledger: "workflow_steps.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "team_runs.jsonl",
        path: "task_ids",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "member_runs.jsonl",
        path: "current_task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "team_messages.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "member_actions.jsonl",
        path: "task_id",
        target: TargetKind::Task,
    },
    LinkRule {
        ledger: "delegation_runs.jsonl",
        path: "parent_task_id",
        target: TargetKind::Task,
    },
];

struct StagingDir {
    path: PathBuf,
    keep: bool,
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Create one immutable archive. The source store is only ever opened for read.
pub fn export_archive(
    store_root: &Path,
    project_id: Option<&str>,
    project_root: Option<&Path>,
    output: &Path,
) -> Result<ExportSummary, String> {
    let project_root = project_root.ok_or_else(|| {
        "legacy export needs an explicit project root; refusing implicit source discovery"
            .to_string()
    })?;
    reject_symlink_or_non_directory(store_root, "primary source store")?;
    reject_symlink_or_non_directory(project_root, "project root")?;
    if output.exists() {
        return Err(format!(
            "archive destination already exists (refusing to overwrite): {}",
            output.display()
        ));
    }
    let mut sources = discover_sources(store_root, project_root)?;
    reject_output_inside_roots(
        &sources
            .iter()
            .map(|source| source.root.as_path())
            .collect::<Vec<_>>(),
        project_root,
        output,
    )?;
    for source in &mut sources {
        source.before = snapshot_directory(&source.root)?;
    }

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

    let resolved_project_id = project_id
        .map(str::to_string)
        .or_else(|| project_id_from_metadata(store_root))
        .unwrap_or_else(|| {
            store_root
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unidentified-project")
                .to_string()
        });

    let mut ledgers = Vec::new();
    let mut file_meta = FileMetaMap::new();
    let mut linked_rows = 0_u64;
    let mut manifest_sources = Vec::new();
    for source in &sources {
        let result = archive_source(source, &staging.path, &mut file_meta)?;
        linked_rows += result.linked_rows;
        ledgers.extend(result.ledgers);
        manifest_sources.push(ManifestSource {
            id: source.id.clone(),
            kind: source.kind.clone(),
            path: canonical_string(&source.root),
            snapshot_sha256: snapshot_hash(&source.before)?,
            snapshot_files: source.before.clone(),
            linked_ledgers: result.linked_ledgers,
        });
    }

    let inventory = build_inventory(&ledgers)?;
    let (edges, known_anomalies) = build_edges(&resolved_project_id, &ledgers, &inventory)?;
    validate_authorized_anomaly_contract(&resolved_project_id, &known_anomalies)?;
    let edges_bytes = jsonl_bytes(&edges)?;
    write_archive_file(&staging.path, "edges.jsonl", &edges_bytes)?;
    file_meta.insert(
        "edges.jsonl".into(),
        ("foreign_key_edges".into(), None, None, None),
    );

    let interpretation_materials =
        copy_interpretation_files(project_root, &staging.path, &mut file_meta)?;

    // Re-snapshot every source only after all reads and interpretation copies.
    // A difference means the archive could mix rows from different moments, so
    // the staging directory is discarded and nothing is published.
    for source in &sources {
        ensure_source_unchanged(source)?;
    }

    let mut files = Vec::new();
    for (path, (category, source_path, source_present, source_lines)) in file_meta {
        let bytes = fs::read(staging.path.join(&path))
            .map_err(|e| format!("read staged archive file {path}: {e}"))?;
        files.push(ManifestFile {
            path,
            category,
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
            line_count: physical_line_count(&bytes),
            source_path,
            source_present,
            source_lines,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    let required_edge_count = edges.iter().filter(|edge| edge.closure_required).count() as u64;
    let unresolved_required_edges = edges
        .iter()
        .filter(|edge| {
            edge.closure_required
                && !target_exists(edge, &inventory)
                && !known_anomalies
                    .iter()
                    .any(|anomaly| anomaly_matches_edge(anomaly, edge))
        })
        .count() as u64;
    let manifest = Manifest {
        format: ARCHIVE_FORMAT.into(),
        version: ARCHIVE_VERSION,
        exporter_version: EXPORTER_VERSION.into(),
        exported_at_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        project: ManifestProject {
            id: resolved_project_id.clone(),
            project_root: Some(canonical_string(project_root)),
        },
        source_comparisons: compare_manifest_sources(&manifest_sources),
        interpretation_materials,
        sources: manifest_sources,
        files,
        known_anomalies: known_anomalies.clone(),
        closure: ClosureSummary {
            edge_count: edges.len() as u64,
            required_edge_count,
            unresolved_required_edges,
        },
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
        project_id: resolved_project_id,
        source_stores: manifest
            .sources
            .iter()
            .map(|source| source.path.clone())
            .collect(),
        files: manifest.files.len(),
        linked_rows,
        edges: edges.len() as u64,
        unresolved_required_edges,
        known_anomalies: known_anomalies.len() as u64,
    })
}

/// Verify hashes, line counts, latest projections, edge regeneration, and
/// referential closure without consulting the live store.
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
    if manifest.sources.is_empty() {
        return Err("archive manifest must contain at least one source".into());
    }

    let mut entries = BTreeMap::new();
    for entry in &manifest.files {
        validate_relative_archive_path(&entry.path)?;
        reject_relative_symlink_components(archive, Path::new(&entry.path), "archive file")?;
        if entries.insert(entry.path.clone(), entry).is_some() {
            return Err(format!("duplicate manifest path: {}", entry.path));
        }
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
        if let Some(source_lines) = &entry.source_lines {
            if source_lines.len() as u64 != lines {
                return Err(format!(
                    "source-line map length mismatch for {}: {} mappings for {} lines",
                    entry.path,
                    source_lines.len(),
                    lines
                ));
            }
        }
    }

    let mut ledgers = Vec::new();
    let mut source_ids = BTreeSet::new();
    for source in &manifest.sources {
        validate_source_id(&source.id)?;
        if !source_ids.insert(source.id.clone()) {
            return Err(format!("duplicate archive source id: {}", source.id));
        }
        if source.snapshot_sha256 != snapshot_hash(&source.snapshot_files)? {
            return Err(format!(
                "source snapshot hash mismatch in manifest: {}",
                source.id
            ));
        }
        let mut snapshot_paths = BTreeMap::new();
        for snapshot_file in &source.snapshot_files {
            validate_relative_archive_path(&snapshot_file.path)?;
            if snapshot_paths
                .insert(snapshot_file.path.as_str(), snapshot_file)
                .is_some()
            {
                return Err(format!(
                    "duplicate source snapshot path for {}: {}",
                    source.id, snapshot_file.path
                ));
            }
        }
        for ledger in LEGACY_LEDGERS {
            let archive_path = format!("sources/{}/raw/{ledger}", source.id);
            let entry = entries.get(&archive_path).ok_or_else(|| {
                format!("manifest/archive missing required ledger: {archive_path}")
            })?;
            let bytes = fs::read(archive.join(&archive_path))
                .map_err(|e| format!("read {archive_path}: {e}"))?;
            validate_jsonl(&bytes, ledger)?;
            if entry.category != "raw_legacy_ledger" {
                return Err(format!(
                    "wrong category for source {}/{ledger}: {}",
                    source.id, entry.category
                ));
            }
            let snapshot_file = snapshot_paths.get(*ledger).copied();
            match (entry.source_present, snapshot_file) {
                (Some(true), Some(snapshot_file))
                    if snapshot_file.bytes == bytes.len() as u64
                        && snapshot_file.sha256 == sha256_hex(&bytes) => {}
                (Some(false), None) if bytes.is_empty() => {}
                _ => {
                    return Err(format!(
                        "raw legacy ledger does not match source snapshot presence/hash: {}/{}",
                        source.id, ledger
                    ));
                }
            }
            ledgers.push(ArchivedLedger {
                source_id: source.id.clone(),
                ledger: (*ledger).to_string(),
                archive_path,
                source_lines: (1..=physical_line_count(&bytes)).collect(),
                bytes,
            });
        }
        for ledger in &source.linked_ledgers {
            let archive_path = format!("sources/{}/records/{ledger}", source.id);
            let entry = entries
                .get(&archive_path)
                .ok_or_else(|| format!("manifest/archive missing linked rows: {archive_path}"))?;
            let bytes = fs::read(archive.join(&archive_path))
                .map_err(|e| format!("read {archive_path}: {e}"))?;
            if !snapshot_paths.contains_key(ledger.as_str()) {
                return Err(format!(
                    "linked ledger is absent from source snapshot: {}/{}",
                    source.id, ledger
                ));
            }
            validate_linked_records(&bytes, ledger, &archive_path)?;
            let source_lines = entry
                .source_lines
                .clone()
                .ok_or_else(|| format!("linked rows lack source-line map: {archive_path}"))?;
            ledgers.push(ArchivedLedger {
                source_id: source.id.clone(),
                ledger: ledger.clone(),
                archive_path,
                bytes,
                source_lines,
            });
        }
    }
    if manifest.source_comparisons != compare_manifest_sources(&manifest.sources) {
        return Err("source comparison summary does not match source snapshots".into());
    }
    validate_interpretation_materials(&manifest, &entries)?;

    for ledger in &ledgers {
        let latest_path = format!("sources/{}/latest/{}", ledger.source_id, ledger.ledger);
        if !entries.contains_key(&latest_path) {
            return Err(format!("latest projection missing: {latest_path}"));
        }
        let expected = latest_projection(&ledger.bytes, &ledger.ledger)?;
        let actual =
            fs::read(archive.join(&latest_path)).map_err(|e| format!("read {latest_path}: {e}"))?;
        if actual != expected {
            return Err(format!("latest projection mismatch: {latest_path}"));
        }
    }

    let inventory = build_inventory(&ledgers)?;
    let (expected_edges, expected_anomalies) =
        build_edges(&manifest.project.id, &ledgers, &inventory)?;
    validate_authorized_anomaly_contract(&manifest.project.id, &expected_anomalies)?;
    let expected_edge_bytes = jsonl_bytes(&expected_edges)?;
    let actual_edge_bytes =
        fs::read(archive.join("edges.jsonl")).map_err(|e| format!("read edges.jsonl: {e}"))?;
    if actual_edge_bytes != expected_edge_bytes {
        return Err("edges.jsonl does not match edges regenerated from archived rows".into());
    }

    let required_edge_count = expected_edges
        .iter()
        .filter(|edge| edge.closure_required)
        .count() as u64;
    if manifest.known_anomalies != expected_anomalies {
        return Err(
            "known anomaly whitelist does not match semantic predicates over raw rows".into(),
        );
    }
    let unresolved = expected_edges
        .iter()
        .filter(|edge| {
            edge.closure_required
                && !target_exists(edge, &inventory)
                && !expected_anomalies
                    .iter()
                    .any(|anomaly| anomaly_matches_edge(anomaly, edge))
        })
        .collect::<Vec<_>>();
    if manifest.closure.edge_count != expected_edges.len() as u64
        || manifest.closure.required_edge_count != required_edge_count
        || manifest.closure.unresolved_required_edges != unresolved.len() as u64
    {
        return Err("manifest closure counts do not match regenerated edges".into());
    }
    if !unresolved.is_empty() {
        let sample = unresolved
            .iter()
            .take(5)
            .map(|edge| {
                format!(
                    "{}/{}:{} {} -> {}:{}",
                    edge.source_id,
                    edge.source_ledger,
                    edge.source_store_line,
                    edge.field,
                    edge.target_kind,
                    edge.target_id
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "legacy foreign-key closure failed ({} unresolved): {sample}",
            unresolved.len()
        ));
    }

    Ok(VerifySummary {
        format: ARCHIVE_FORMAT.into(),
        archive: canonical_string(archive),
        files: manifest.files.len(),
        edges: expected_edges.len() as u64,
        closure: "verified".into(),
        known_anomalies: expected_anomalies.len() as u64,
    })
}

struct SourceArchiveResult {
    ledgers: Vec<ArchivedLedger>,
    linked_ledgers: Vec<String>,
    linked_rows: u64,
}

fn discover_sources(store_root: &Path, project_root: &Path) -> Result<Vec<SourceSpec>, String> {
    let primary = fs::canonicalize(store_root)
        .map_err(|e| format!("canonicalize source store {}: {e}", store_root.display()))?;
    let mut sources = vec![SourceSpec {
        id: "central".into(),
        kind: "resolved_project_store".into(),
        root: primary.clone(),
        before: Vec::new(),
    }];
    let local = project_root.join(".harness");
    if local.exists() {
        reject_symlink_or_non_directory(&local, "repo-local source store")?;
        let local = fs::canonicalize(&local)
            .map_err(|e| format!("canonicalize local source {}: {e}", local.display()))?;
        if local != primary {
            sources.push(SourceSpec {
                id: "local".into(),
                kind: if local.join("MIGRATED_TO_CENTRAL").is_file() {
                    "migrated_repo_local_store".into()
                } else {
                    "repo_local_store".into()
                },
                root: local,
                before: Vec::new(),
            });
        }
    }
    Ok(sources)
}

fn reject_symlink_or_non_directory(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("inspect {label} {}: {e}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{label} must not be a symlink: {}", path.display()));
    }
    if !metadata.is_dir() {
        return Err(format!("{label} is not a directory: {}", path.display()));
    }
    Ok(())
}

/// Reject a path reached through any symlink component. Canonicalizing first is
/// insufficient here because it erases precisely the aliasing the offline
/// verifier must report and refuse (for example `alias/archive-v3`).
fn reject_symlink_ancestors(path: &Path, label: &str) -> Result<(), String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|error| format!("resolve {label} {}: {error}", path.display()))?
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| format!("inspect {label} ancestor {}: {error}", current.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "{label} ancestor must not be a symlink: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn snapshot_directory(root: &Path) -> Result<Vec<SnapshotFile>, String> {
    let mut result = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|e| format!("snapshot read directory {}: {e}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("snapshot read entry {}: {e}", directory.display()))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries.into_iter().rev() {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .map_err(|e| format!("snapshot inspect {}: {e}", path.display()))?;
            if metadata.file_type().is_symlink() {
                return Err(format!(
                    "source snapshot refuses symlink: {}",
                    path.display()
                ));
            }
            if metadata.is_dir() {
                stack.push(path);
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(root)
                    .map_err(|_| format!("snapshot path escaped source: {}", path.display()))?;
                let relative = relative
                    .to_str()
                    .ok_or_else(|| format!("non-UTF-8 source path: {}", relative.display()))?
                    .to_string();
                validate_relative_archive_path(&relative)?;
                let bytes = fs::read(&path)
                    .map_err(|e| format!("snapshot read file {}: {e}", path.display()))?;
                result.push(SnapshotFile {
                    path: relative,
                    bytes: bytes.len() as u64,
                    sha256: sha256_hex(&bytes),
                });
            }
        }
    }
    result.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(result)
}

fn snapshot_hash(files: &[SnapshotFile]) -> Result<String, String> {
    let bytes = serde_json::to_vec(files)
        .map_err(|e| format!("serialize source snapshot for hashing: {e}"))?;
    Ok(sha256_hex(&bytes))
}

fn ensure_source_unchanged(source: &SourceSpec) -> Result<(), String> {
    let after = snapshot_directory(&source.root)?;
    if after != source.before {
        return Err(format!(
            "source changed during export; refusing mixed snapshot: {}",
            source.root.display()
        ));
    }
    Ok(())
}

fn compare_manifest_sources(sources: &[ManifestSource]) -> Vec<SourceComparison> {
    let mut comparisons = Vec::new();
    for left_index in 0..sources.len() {
        for right_index in (left_index + 1)..sources.len() {
            let left = &sources[left_index];
            let right = &sources[right_index];
            let left_files = left
                .snapshot_files
                .iter()
                .map(|file| (file.path.as_str(), file))
                .collect::<BTreeMap<_, _>>();
            let right_files = right
                .snapshot_files
                .iter()
                .map(|file| (file.path.as_str(), file))
                .collect::<BTreeMap<_, _>>();
            let mut shared_same = 0_u64;
            let mut shared_different = 0_u64;
            let mut left_only = 0_u64;
            for (path, left_file) in &left_files {
                match right_files.get(path) {
                    Some(right_file) if *right_file == *left_file => shared_same += 1,
                    Some(_) => shared_different += 1,
                    None => left_only += 1,
                }
            }
            let right_only = right_files
                .keys()
                .filter(|path| !left_files.contains_key(*path))
                .count() as u64;
            comparisons.push(SourceComparison {
                left: left.id.clone(),
                right: right.id.clone(),
                shared_same,
                shared_different,
                left_only,
                right_only,
            });
        }
    }
    comparisons
}

fn archive_source(
    source: &SourceSpec,
    archive_root: &Path,
    file_meta: &mut FileMetaMap,
) -> Result<SourceArchiveResult, String> {
    validate_source_id(&source.id)?;
    let prefix = format!("sources/{}", source.id);
    let mut ledgers = Vec::new();
    for ledger in LEGACY_LEDGERS {
        let source_path = source.root.join(ledger);
        let (bytes, present) = if source_path.is_file() {
            (
                fs::read(&source_path)
                    .map_err(|e| format!("read {}: {e}", source_path.display()))?,
                true,
            )
        } else {
            (Vec::new(), false)
        };
        validate_jsonl(&bytes, &format!("{}/{ledger}", source.id))?;
        let archive_path = format!("{prefix}/raw/{ledger}");
        write_archive_file(archive_root, &archive_path, &bytes)?;
        file_meta.insert(
            archive_path.clone(),
            (
                "raw_legacy_ledger".into(),
                Some(source_path.display().to_string()),
                Some(present),
                None,
            ),
        );
        ledgers.push(ArchivedLedger {
            source_id: source.id.clone(),
            ledger: (*ledger).to_string(),
            archive_path,
            source_lines: (1..=physical_line_count(&bytes)).collect(),
            bytes,
        });
    }

    let mut linked_ledgers = Vec::new();
    let mut linked_rows = 0_u64;
    let mut jsonl_paths = fs::read_dir(&source.root)
        .map_err(|e| format!("read source store {}: {e}", source.root.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("read source store entry {}: {e}", source.root.display()))?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("jsonl"))
        .collect::<Vec<_>>();
    jsonl_paths.sort();
    for source_path in jsonl_paths {
        let ledger = source_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("non-UTF-8 ledger name: {}", source_path.display()))?
            .to_string();
        if LEGACY_LEDGERS.contains(&ledger.as_str()) {
            continue;
        }
        let source_bytes =
            fs::read(&source_path).map_err(|e| format!("read {}: {e}", source_path.display()))?;
        let records = jsonl_records(&source_bytes, &format!("{}/{ledger}", source.id))?;
        let linked_ids = records
            .iter()
            .filter(|record| record_has_legacy_link(&ledger, &record.value))
            .filter_map(|record| record_identity(&record.value))
            .map(str::to_string)
            .collect::<BTreeSet<_>>();
        let mut selected = Vec::new();
        let mut source_lines = Vec::new();
        for record in records {
            if record_has_legacy_link(&ledger, &record.value)
                || record_identity(&record.value).is_some_and(|id| linked_ids.contains(id))
            {
                selected.extend_from_slice(record.raw);
                source_lines.push(record.line);
            }
        }
        if selected.is_empty() {
            continue;
        }
        linked_rows += source_lines.len() as u64;
        linked_ledgers.push(ledger.clone());
        let archive_path = format!("{prefix}/records/{ledger}");
        write_archive_file(archive_root, &archive_path, &selected)?;
        file_meta.insert(
            archive_path.clone(),
            (
                "linked_legacy_rows".into(),
                Some(source_path.display().to_string()),
                Some(true),
                Some(source_lines.clone()),
            ),
        );
        ledgers.push(ArchivedLedger {
            source_id: source.id.clone(),
            ledger,
            archive_path,
            bytes: selected,
            source_lines,
        });
    }
    linked_ledgers.sort();

    for ledger in &ledgers {
        let latest = latest_projection(&ledger.bytes, &ledger.ledger)?;
        let archive_path = format!("{prefix}/latest/{}", ledger.ledger);
        write_archive_file(archive_root, &archive_path, &latest)?;
        file_meta.insert(archive_path, ("latest_projection".into(), None, None, None));
    }
    Ok(SourceArchiveResult {
        ledgers,
        linked_ledgers,
        linked_rows,
    })
}

fn validate_linked_records(bytes: &[u8], ledger: &str, archive_path: &str) -> Result<(), String> {
    let records = jsonl_records(bytes, ledger)?;
    let linked_ids = records
        .iter()
        .filter(|record| record_has_legacy_link(ledger, &record.value))
        .filter_map(|record| record_identity(&record.value))
        .collect::<BTreeSet<_>>();
    if records.iter().any(|record| {
        !record_has_legacy_link(ledger, &record.value)
            && !record_identity(&record.value).is_some_and(|id| linked_ids.contains(id))
    }) {
        return Err(format!(
            "linked-row archive contains unrelated row: {archive_path}"
        ));
    }
    Ok(())
}

fn validate_source_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(format!("unsafe archive source id: {value}"));
    }
    Ok(())
}

fn project_id_from_metadata(store_root: &Path) -> Option<String> {
    let bytes = fs::read(store_root.join("metadata.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value
        .get("project_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn reject_output_inside_roots(
    store_roots: &[&Path],
    project_root: &Path,
    output: &Path,
) -> Result<(), String> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = resolve_with_existing_ancestor(parent)?;
    let project = fs::canonicalize(project_root)
        .map_err(|e| format!("canonicalize project root {}: {e}", project_root.display()))?;
    if parent.starts_with(&project) {
        return Err(format!(
            "archive destination must be outside the project root: {}",
            output.display()
        ));
    }
    for root in store_roots {
        let source = fs::canonicalize(root)
            .map_err(|e| format!("canonicalize source store {}: {e}", root.display()))?;
        if parent.starts_with(&source) {
            return Err(format!(
                "archive destination must be outside every live source store: {}",
                output.display()
            ));
        }
    }
    Ok(())
}

/// Resolve symlinks in the nearest existing ancestor, then append the normalized
/// not-yet-created suffix. This prevents `outside/symlink-to-store/new/archive`
/// and `outside/../store/new/archive` from bypassing the live-store guard.
fn resolve_with_existing_ancestor(path: &Path) -> Result<PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|e| format!("resolve path {}: {e}", path.display()))?
    };
    let normalized = normalize_path(&absolute);
    let mut ancestor = normalized.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| format!("archive path has no existing ancestor: {}", path.display()))?;
        suffix.push(name.to_os_string());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| format!("archive path has no existing ancestor: {}", path.display()))?;
    }
    let mut resolved = fs::canonicalize(ancestor)
        .map_err(|e| format!("canonicalize archive ancestor {}: {e}", ancestor.display()))?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn canonical_string(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

fn write_archive_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    validate_relative_archive_path(relative)?;
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create archive directory {}: {e}", parent.display()))?;
    }
    fs::write(&path, bytes).map_err(|e| format!("write archive file {}: {e}", path.display()))
}

fn validate_relative_archive_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(format!("invalid archive-relative path: {}", path.display()));
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(format!("unsafe archive-relative path: {}", path.display()));
    }
    Ok(())
}

fn copy_interpretation_files(
    project_root: &Path,
    archive_root: &Path,
    file_meta: &mut FileMetaMap,
) -> Result<Vec<InterpretationMaterial>, String> {
    let mut materials = Vec::new();
    for relative in INTERPRETATION_PATHS {
        let source = project_root.join(relative);
        reject_relative_symlink_components(
            project_root,
            Path::new(relative),
            "interpretation source",
        )?;
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                materials.push(InterpretationMaterial {
                    source_path: (*relative).to_string(),
                    source_present: false,
                    reason: Some("not_present_in_source".into()),
                    archived_files: Vec::new(),
                });
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "inspect interpretation source {}: {error}",
                    source.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "interpretation source must not be a symlink: {}",
                source.display()
            ));
        }
        let mut archived_files = Vec::new();
        if metadata.is_file() {
            archived_files.push(copy_interpretation_file(
                project_root,
                &source,
                archive_root,
                file_meta,
            )?);
        } else if metadata.is_dir() {
            let mut stack = vec![source.clone()];
            while let Some(dir) = stack.pop() {
                let mut children = fs::read_dir(&dir)
                    .map_err(|e| format!("read interpretation directory {}: {e}", dir.display()))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("read interpretation entry in {}: {e}", dir.display()))?;
                children.sort_by_key(|entry| entry.file_name());
                for entry in children.into_iter().rev() {
                    let kind = entry
                        .file_type()
                        .map_err(|e| format!("read file type {}: {e}", entry.path().display()))?;
                    if kind.is_symlink() {
                        return Err(format!(
                            "interpretation source must not contain symlink: {}",
                            entry.path().display()
                        ));
                    }
                    if kind.is_dir() {
                        stack.push(entry.path());
                    } else if kind.is_file() {
                        archived_files.push(copy_interpretation_file(
                            project_root,
                            &entry.path(),
                            archive_root,
                            file_meta,
                        )?);
                    } else {
                        return Err(format!(
                            "interpretation source contains unsupported entry: {}",
                            entry.path().display()
                        ));
                    }
                }
            }
        } else {
            return Err(format!(
                "interpretation source must be a regular file or directory: {}",
                source.display()
            ));
        }
        archived_files.sort();
        materials.push(InterpretationMaterial {
            source_path: (*relative).to_string(),
            source_present: true,
            reason: None,
            archived_files,
        });
    }
    Ok(materials)
}

fn reject_relative_symlink_components(
    root: &Path,
    relative: &Path,
    label: &str,
) -> Result<(), String> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    "{label} parent/leaf must not be a symlink: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!("inspect {label} {}: {error}", current.display()));
            }
        }
    }
    Ok(())
}

fn copy_interpretation_file(
    project_root: &Path,
    source: &Path,
    archive_root: &Path,
    file_meta: &mut FileMetaMap,
) -> Result<String, String> {
    let relative = source.strip_prefix(project_root).map_err(|_| {
        format!(
            "interpretation source escaped project root: {}",
            source.display()
        )
    })?;
    let relative_text = relative
        .to_str()
        .ok_or_else(|| format!("non-UTF-8 interpretation path: {}", relative.display()))?;
    let archive_path = format!("interpretation/{relative_text}");
    let bytes = fs::read(source)
        .map_err(|e| format!("read interpretation file {}: {e}", source.display()))?;
    write_archive_file(archive_root, &archive_path, &bytes)?;
    file_meta.insert(
        archive_path.clone(),
        (
            "interpretation_source".into(),
            Some(source.display().to_string()),
            Some(true),
            None,
        ),
    );
    Ok(archive_path)
}

fn validate_interpretation_materials(
    manifest: &Manifest,
    entries: &BTreeMap<String, &ManifestFile>,
) -> Result<(), String> {
    let expected_paths = INTERPRETATION_PATHS
        .iter()
        .map(|path| (*path).to_string())
        .collect::<Vec<_>>();
    let actual_paths = manifest
        .interpretation_materials
        .iter()
        .map(|material| material.source_path.clone())
        .collect::<Vec<_>>();
    if actual_paths != expected_paths {
        return Err(
            "interpretation material contract paths/order do not match exporter contract".into(),
        );
    }

    let mut claimed_files = BTreeSet::new();
    for material in &manifest.interpretation_materials {
        validate_relative_archive_path(&material.source_path)?;
        if material.source_present {
            if material.reason.is_some() || material.archived_files.is_empty() {
                return Err(format!(
                    "present interpretation material must have files and no absence reason: {}",
                    material.source_path
                ));
            }
            let prefix = format!("interpretation/{}", material.source_path);
            for archived_file in &material.archived_files {
                validate_relative_archive_path(archived_file)?;
                if archived_file != &prefix && !archived_file.starts_with(&format!("{prefix}/")) {
                    return Err(format!(
                        "interpretation file is outside its contract path: {archived_file}"
                    ));
                }
                if !claimed_files.insert(archived_file.clone()) {
                    return Err(format!(
                        "interpretation file is claimed more than once: {archived_file}"
                    ));
                }
                let entry = entries.get(archived_file).ok_or_else(|| {
                    format!("manifest/archive missing interpretation file: {archived_file}")
                })?;
                if entry.category != "interpretation_source" || entry.source_present != Some(true) {
                    return Err(format!(
                        "wrong metadata for interpretation file: {archived_file}"
                    ));
                }
            }
        } else if material.reason.as_deref() != Some("not_present_in_source")
            || !material.archived_files.is_empty()
        {
            return Err(format!(
                "absent interpretation material must state not_present_in_source and contain no files: {}",
                material.source_path
            ));
        }
    }

    let manifest_interpretation_files = manifest
        .files
        .iter()
        .filter(|entry| entry.category == "interpretation_source")
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    if manifest_interpretation_files != claimed_files {
        return Err("interpretation files are not fully covered by material contract".into());
    }
    Ok(())
}

struct JsonlRecord<'a> {
    line: u64,
    raw: &'a [u8],
    value: serde_json::Value,
}

fn jsonl_records<'a>(bytes: &'a [u8], label: &str) -> Result<Vec<JsonlRecord<'a>>, String> {
    let mut records = Vec::new();
    let mut start = 0_usize;
    for (index, end) in bytes
        .iter()
        .enumerate()
        .filter_map(|(index, byte)| (*byte == b'\n').then_some(index + 1))
        .chain((!bytes.is_empty() && bytes.last() != Some(&b'\n')).then_some(bytes.len()))
        .enumerate()
    {
        let line = index as u64 + 1;
        let raw = &bytes[start..end];
        let content = if raw.last() == Some(&b'\n') {
            &raw[..raw.len() - 1]
        } else {
            raw
        };
        if !content.iter().all(u8::is_ascii_whitespace) {
            let value = serde_json::from_slice(content)
                .map_err(|e| format!("invalid JSONL in {label} at line {line}: {e}"))?;
            records.push(JsonlRecord { line, raw, value });
        }
        start = end;
    }
    Ok(records)
}

fn validate_jsonl(bytes: &[u8], label: &str) -> Result<(), String> {
    jsonl_records(bytes, label).map(|_| ())
}

fn physical_line_count(bytes: &[u8]) -> u64 {
    if bytes.is_empty() {
        0
    } else {
        bytes.iter().filter(|byte| **byte == b'\n').count() as u64
            + u64::from(bytes.last() != Some(&b'\n'))
    }
}

fn latest_projection(bytes: &[u8], label: &str) -> Result<Vec<u8>, String> {
    let records = jsonl_records(bytes, label)?;
    let mut latest: BTreeMap<String, (usize, &[u8])> = BTreeMap::new();
    for (index, record) in records.iter().enumerate() {
        let identity = record_identity(&record.value)
            .map(str::to_string)
            .unwrap_or_else(|| format!("__line_{}", record.line));
        latest.insert(identity, (index, record.raw));
    }
    let mut rows = latest.into_values().collect::<Vec<_>>();
    rows.sort_by_key(|(index, _)| *index);
    let mut output = Vec::new();
    for (_, row) in rows {
        output.extend_from_slice(row);
    }
    Ok(output)
}

fn record_identity(value: &serde_json::Value) -> Option<&str> {
    value
        .get("id")
        .or_else(|| value.get("case_id"))
        .and_then(serde_json::Value::as_str)
}

fn build_inventory(ledgers: &[ArchivedLedger]) -> Result<Inventory, String> {
    let mut inventory = Inventory::default();
    for ledger in ledgers {
        if !LEGACY_LEDGERS.contains(&ledger.ledger.as_str()) {
            continue;
        }
        for record in jsonl_records(&ledger.bytes, &ledger.ledger)? {
            match ledger.ledger.as_str() {
                "goals.jsonl" => {
                    if let Some(id) = record_identity(&record.value) {
                        inventory.goals.insert(id.to_string());
                    }
                    if let Some(phases) = record.value.get("phases").and_then(|v| v.as_array()) {
                        for phase in phases {
                            if let Some(id) = phase.get("id").and_then(|v| v.as_str()) {
                                inventory.phases.insert(id.to_string());
                            }
                        }
                    }
                }
                "tasks.jsonl" => {
                    if let Some(id) = record_identity(&record.value) {
                        inventory.tasks.insert(id.to_string());
                    }
                }
                "goal_designs.jsonl" => {
                    if let Some(id) = record_identity(&record.value) {
                        inventory.goal_designs.insert(id.to_string());
                    }
                }
                "goal_evaluations.jsonl" => {
                    if let Some(id) = record_identity(&record.value) {
                        inventory.goal_evaluations.insert(id.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    Ok(inventory)
}

fn record_has_legacy_link(ledger: &str, value: &serde_json::Value) -> bool {
    let mut values = Vec::new();
    collect_link_values(ledger, value, &mut values);
    !values.is_empty()
}

fn build_edges(
    project_id: &str,
    ledgers: &[ArchivedLedger],
    inventory: &Inventory,
) -> Result<(Vec<Edge>, Vec<KnownAnomaly>), String> {
    let mut edges = Vec::new();
    let mut anomalies = Vec::new();
    let mut ordered = ledgers.to_vec();
    ordered.sort_by(|a, b| a.archive_path.cmp(&b.archive_path));
    for ledger in &ordered {
        for record in jsonl_records(&ledger.bytes, &ledger.ledger)? {
            let mut links = Vec::new();
            collect_link_values(&ledger.ledger, &record.value, &mut links);
            let source_record_id = record_identity(&record.value)
                .map(str::to_string)
                .unwrap_or_else(|| format!("{}:{}", ledger.ledger, record.line));
            let source_store_line = ledger
                .source_lines
                .get(record.line.saturating_sub(1) as usize)
                .copied()
                .unwrap_or(record.line);
            let authorized_anomaly = authorized_anomaly_for_record(
                project_id,
                ledger,
                &record,
                source_store_line,
                inventory,
            )?;
            for (field, kind, target_id) in links {
                let closure_required = match kind {
                    TargetKind::GoalOrDescriptionRef => inventory.goals.contains(&target_id),
                    TargetKind::TaskOrDescriptionRef => inventory.tasks.contains(&target_id),
                    _ => true,
                };
                let edge = Edge {
                    source_id: ledger.source_id.clone(),
                    source_ledger: ledger.ledger.clone(),
                    source_archive_path: ledger.archive_path.clone(),
                    source_archive_line: record.line,
                    source_store_line,
                    source_record_id: source_record_id.clone(),
                    field,
                    target_kind: kind.label().into(),
                    target_id,
                    closure_required,
                };
                edges.push(edge);
            }
            if let Some(anomaly) = authorized_anomaly {
                if !edges
                    .iter()
                    .any(|edge| anomaly_matches_edge(&anomaly, edge))
                {
                    return Err(
                        "preauthorized known anomaly row did not emit its exact edge".into(),
                    );
                }
                anomalies.push(anomaly);
            }
        }
    }
    edges.sort();
    anomalies.sort();
    Ok((edges, anomalies))
}

fn authorized_anomaly_for_record(
    project_id: &str,
    ledger: &ArchivedLedger,
    record: &JsonlRecord<'_>,
    source_store_line: u64,
    inventory: &Inventory,
) -> Result<Option<KnownAnomaly>, String> {
    if project_id != AUTHORIZED_ANOMALY_PROJECT_ID
        || ledger.source_id != AUTHORIZED_ANOMALY_SOURCE_ID
        || ledger.ledger != AUTHORIZED_ANOMALY_LEDGER
        || source_store_line != AUTHORIZED_ANOMALY_LINE
    {
        return Ok(None);
    }
    let value = &record.value;
    let exact = record_identity(value) == Some(AUTHORIZED_ANOMALY_RECORD_ID)
        && value.get("task_id").and_then(serde_json::Value::as_str)
            == Some(AUTHORIZED_ANOMALY_TARGET)
        && value.get("goal_id").and_then(serde_json::Value::as_str)
            == Some(AUTHORIZED_ANOMALY_TARGET)
        && value
            .get("decision_kind")
            .and_then(serde_json::Value::as_str)
            == Some(AUTHORIZED_ANOMALY_DECISION_KIND)
        && sha256_hex(record.raw) == AUTHORIZED_ANOMALY_RAW_SHA256
        && inventory.goals.contains(AUTHORIZED_ANOMALY_TARGET)
        && !inventory.tasks.contains(AUTHORIZED_ANOMALY_TARGET);
    if !exact {
        return Err(format!(
            "preauthorized known anomaly contract mismatch at {}/{AUTHORIZED_ANOMALY_LEDGER}:{AUTHORIZED_ANOMALY_LINE}",
            ledger.source_id
        ));
    }
    Ok(Some(KnownAnomaly {
        anomaly_kind: "known_kind_mismatch".into(),
        source_id: AUTHORIZED_ANOMALY_SOURCE_ID.into(),
        ledger: AUTHORIZED_ANOMALY_LEDGER.into(),
        line: AUTHORIZED_ANOMALY_LINE,
        record_id: AUTHORIZED_ANOMALY_RECORD_ID.into(),
        field: AUTHORIZED_ANOMALY_FIELD.into(),
        target: AUTHORIZED_ANOMALY_TARGET.into(),
        raw_line_sha256: AUTHORIZED_ANOMALY_RAW_SHA256.into(),
    }))
}

fn validate_authorized_anomaly_contract(
    project_id: &str,
    anomalies: &[KnownAnomaly],
) -> Result<(), String> {
    let expected = if project_id == AUTHORIZED_ANOMALY_PROJECT_ID {
        vec![KnownAnomaly {
            anomaly_kind: "known_kind_mismatch".into(),
            source_id: AUTHORIZED_ANOMALY_SOURCE_ID.into(),
            ledger: AUTHORIZED_ANOMALY_LEDGER.into(),
            line: AUTHORIZED_ANOMALY_LINE,
            record_id: AUTHORIZED_ANOMALY_RECORD_ID.into(),
            field: AUTHORIZED_ANOMALY_FIELD.into(),
            target: AUTHORIZED_ANOMALY_TARGET.into(),
            raw_line_sha256: AUTHORIZED_ANOMALY_RAW_SHA256.into(),
        }]
    } else {
        Vec::new()
    };
    if anomalies != expected {
        return Err(format!(
            "preauthorized known anomaly contract mismatch for project {project_id}"
        ));
    }
    Ok(())
}

fn anomaly_matches_edge(anomaly: &KnownAnomaly, edge: &Edge) -> bool {
    anomaly.anomaly_kind == "known_kind_mismatch"
        && anomaly.source_id == edge.source_id
        && anomaly.ledger == edge.source_ledger
        && anomaly.line == edge.source_store_line
        && anomaly.record_id == edge.source_record_id
        && anomaly.field == edge.field
        && anomaly.target == edge.target_id
}

fn collect_link_values(
    ledger: &str,
    value: &serde_json::Value,
    output: &mut Vec<(String, TargetKind, String)>,
) {
    for rule in LINK_RULES.iter().filter(|rule| rule.ledger == ledger) {
        let segments = rule.path.split('/').collect::<Vec<_>>();
        collect_rule_values(value, &segments, "", rule.target, output);
    }
}

fn collect_rule_values(
    value: &serde_json::Value,
    segments: &[&str],
    path: &str,
    target: TargetKind,
    output: &mut Vec<(String, TargetKind, String)>,
) {
    let Some((segment, remaining)) = segments.split_first() else {
        match value {
            serde_json::Value::String(value) if !value.trim().is_empty() => {
                output.push((path.to_string(), target, value.clone()));
            }
            serde_json::Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    if let Some(value) = value.as_str().filter(|value| !value.trim().is_empty()) {
                        output.push((format!("{path}/{index}"), target, value.to_string()));
                    }
                }
            }
            _ => {}
        }
        return;
    };
    if *segment == "*" {
        if let Some(values) = value.as_array() {
            for (index, child) in values.iter().enumerate() {
                collect_rule_values(child, remaining, &format!("{path}/{index}"), target, output);
            }
        }
    } else if let Some(child) = value.get(*segment) {
        collect_rule_values(
            child,
            remaining,
            &format!("{path}/{}", json_pointer_escape(segment)),
            target,
            output,
        );
    }
}

fn json_pointer_escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn target_exists(edge: &Edge, inventory: &Inventory) -> bool {
    match edge.target_kind.as_str() {
        "goal" => inventory.goals.contains(&edge.target_id),
        "task" => inventory.tasks.contains(&edge.target_id),
        "goal_phase" => inventory.phases.contains(&edge.target_id),
        "goal_design" => inventory.goal_designs.contains(&edge.target_id),
        "goal_evaluation" => inventory.goal_evaluations.contains(&edge.target_id),
        _ => false,
    }
}

fn jsonl_bytes<T: Serialize>(values: &[T]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    for value in values {
        serde_json::to_writer(&mut bytes, value)
            .map_err(|e| format!("serialize archive JSONL: {e}"))?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

// Small self-contained SHA-256 implementation. Keeping it here avoids making the
// archive contract depend on an external executable or a new crate dependency.
fn sha256_hex(input: &[u8]) -> String {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = input.to_vec();
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut state = INITIAL;
    for chunk in padded.chunks_exact(64) {
        let mut words = [0_u32; 64];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            let start = index * 4;
            *word = u32::from_be_bytes([
                chunk[start],
                chunk[start + 1],
                chunk[start + 2],
                chunk[start + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
    state.iter().map(|word| format!("{word:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_standard_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn latest_projection_retains_last_row_bytes() {
        let bytes = b"{\"id\":\"a\",\"v\":1}\n{\"id\":\"b\",\"v\":1}\n{\"id\":\"a\",\"v\":2}";
        assert_eq!(
            latest_projection(bytes, "test.jsonl").unwrap(),
            b"{\"id\":\"b\",\"v\":1}\n{\"id\":\"a\",\"v\":2}"
        );
    }

    #[test]
    fn only_contract_paths_become_edges() {
        let value = serde_json::json!({
            "id": "x",
            "goal_id": "g1",
            "phase_runs": [{"phase_id": "p1"}],
            "result": {"task_id": "dynamic-must-not-scan"}
        });
        let mut links = Vec::new();
        collect_link_values("goal_orchestration_runs.jsonl", &value, &mut links);
        assert_eq!(links.len(), 2);
        assert!(links
            .iter()
            .any(|(path, _, id)| path == "/goal_id" && id == "g1"));
        assert!(links
            .iter()
            .any(|(path, _, id)| path == "/phase_runs/0/phase_id" && id == "p1"));
        assert!(!links.iter().any(|(_, _, id)| id == "dynamic-must-not-scan"));
    }

    #[test]
    fn snapshot_detects_file_set_size_and_hash_changes() {
        let root = std::env::temp_dir().join(format!(
            "legacy-export-snapshot-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("ledger.jsonl"), b"one\n").unwrap();
        let before = snapshot_directory(&root).unwrap();

        fs::write(root.join("ledger.jsonl"), b"two\n").unwrap();
        let hash_changed = snapshot_directory(&root).unwrap();
        assert_ne!(before, hash_changed, "same size but new hash must differ");

        fs::write(root.join("new.jsonl"), b"{}\n").unwrap();
        let set_changed = snapshot_directory(&root).unwrap();
        assert_ne!(hash_changed, set_changed, "new file set must differ");

        fs::write(root.join("ledger.jsonl"), b"longer\n").unwrap();
        let size_changed = snapshot_directory(&root).unwrap();
        assert_ne!(set_changed, size_changed, "new size must differ");

        let source = SourceSpec {
            id: "test".into(),
            kind: "test".into(),
            root: root.clone(),
            before: set_changed,
        };
        let error = ensure_source_unchanged(&source).unwrap_err();
        assert!(error.contains("refusing mixed snapshot"));
        fs::remove_dir_all(root).unwrap();
    }
}

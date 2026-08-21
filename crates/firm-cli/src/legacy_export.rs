//! Read-only archive support for the retired Goal / Task-Graph records.
//!
//! The archive deliberately preserves source JSONL bytes. It does not deserialize
//! rows into the current Rust domain model, rename Tasks to Work, or create
//! Mission/Wave compatibility projections.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

mod digest;
mod export;
mod verify;

pub(crate) use digest::sha256_hex;
pub use export::export_archive;
pub use verify::verify_archive;

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
        ledger: "provider_dispatch_events.jsonl",
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
        ledger: "provider_launch_profiles.jsonl",
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

pub(crate) struct StagingDir {
    pub(crate) path: PathBuf,
    pub(crate) keep: bool,
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
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
    // Repo-local stores predate the centralized Firm home and deliberately
    // retain the historical `.harness` compatibility name.
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

pub(crate) fn reject_symlink_or_non_directory(path: &Path, label: &str) -> Result<(), String> {
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
pub(crate) fn reject_symlink_ancestors(path: &Path, label: &str) -> Result<(), String> {
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
pub(crate) fn resolve_with_existing_ancestor(path: &Path) -> Result<PathBuf, String> {
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

pub(crate) fn canonical_string(path: &Path) -> String {
    fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

pub(crate) fn write_archive_file(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    validate_relative_archive_path(relative)?;
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create archive directory {}: {e}", parent.display()))?;
    }
    fs::write(&path, bytes).map_err(|e| format!("write archive file {}: {e}", path.display()))
}

pub(crate) fn validate_relative_archive_path(path: &str) -> Result<(), String> {
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

pub(crate) fn reject_relative_symlink_components(
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

pub(crate) struct JsonlRecord<'a> {
    line: u64,
    raw: &'a [u8],
    value: serde_json::Value,
}

pub(crate) fn jsonl_records<'a>(
    bytes: &'a [u8],
    label: &str,
) -> Result<Vec<JsonlRecord<'a>>, String> {
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

pub(crate) fn physical_line_count(bytes: &[u8]) -> u64 {
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

#[cfg(test)]
mod tests;

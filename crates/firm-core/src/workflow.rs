use super::*;

// A MissionLogEntry is
// one immutable, monotonically revisioned Markdown record of Host judgment,
// re-plan, recovery narration, or closeout evidence. Unlike LegacyWave it has no
// lifecycle, gate, or "advance" operation — there is nothing to accept or
// reject, only entries to append and read. The Log is required reading, not
// optional narration: the recovery entrypoint and session re-entry injection
// are mandatory readers of its tail so a Host (or its replacement) resumes
// from durable judgment instead of re-deriving intent from provider-native
// state that a compaction can destroy.
// ---------------------------------------------------------------------------

/// The nature of one [`MissionLogEntry`]. There is deliberately no variant for
/// routine narration — every entry is one of these four material kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionLogEntryKind {
    /// A Host decision at a material point: a new Work tranche, a composition
    /// change, or a model/provider switch.
    Judgment,
    /// A material change to the Host's plan since the previous entry.
    Replan,
    /// Narration written while recovering a Mission, TeamRun, or Host session.
    Recovery,
    /// The evidence or outcome that justifies Mission closeout.
    CloseoutEvidence,
}

/// One immutable, append-only Mission Log row (ADR 0051). `revision` is
/// monotonic per `mission_id` and store-assigned; Legacy Wave indexes remain
/// historical compatibility data;
/// callers never choose it. There is no `updated_at` because a
/// [`MissionLogEntry`] is never revised in place — a correction is a new
/// entry, not a mutation of an old one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionLogEntry {
    pub id: String,
    pub mission_id: String,
    pub revision: u32,
    pub kind: MissionLogEntryKind,
    /// Markdown body. Must be non-empty: an append-only judgment log with a
    /// blank entry is indistinguishable from a failed log write.
    pub body: String,
    /// The actor that authored this entry (a Host identity, "host", or an
    /// explicit operator/agent id).
    pub actor: String,
    pub created_at: String,
}

impl Validate for MissionLogEntry {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "MissionLogEntry.id")?;
        require_non_empty(&self.mission_id, "MissionLogEntry.mission_id")?;
        require_non_empty(&self.body, "MissionLogEntry.body")?;
        require_non_empty(&self.actor, "MissionLogEntry.actor")?;
        require_non_empty(&self.created_at, "MissionLogEntry.created_at")
    }
}

// ---------------------------------------------------------------------------
// Dynamic workflow runtime objects (WP1)
//
// A `WorkflowRun` is a standalone object with its own id and lifecycle. Each
// `WorkflowStep` is the workflow-layer wrapper around one `agent()` call and references the
// provider-owned native session rather than re-recording the execution. Both
// journal to their own append-only JSONL with latest-wins
// projection, exactly like every other harness object.
// ---------------------------------------------------------------------------

/// Lifecycle of a [`WorkflowRun`]. WP1 only exercises Running -> Completed and
/// Running -> Failed; Pending/Paused are reserved for the scheduler/resume work
/// packages (WP2/WP4) so existing rows remain forward-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

/// Status of a single [`WorkflowStep`] (one `agent()` call). WP1 uses
/// Running -> Completed / Failed. Queued/Cached are reserved for the
/// scheduler/resume work packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStepStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cached,
}

/// Machine-readable class describing how a workflow run or step terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowTerminalReason {
    CanceledByOperator,
    DriverExited,
    OrphanReaped,
    LeafTimeout,
    IdleTimeout,
    ProviderFailed,
    VerdictFailed,
    Completed,
}

impl WorkflowTerminalReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CanceledByOperator => "canceled_by_operator",
            Self::DriverExited => "driver_exited",
            Self::OrphanReaped => "orphan_reaped",
            Self::LeafTimeout => "leaf_timeout",
            Self::IdleTimeout => "idle_timeout",
            Self::ProviderFailed => "provider_failed",
            Self::VerdictFailed => "verdict_failed",
            Self::Completed => "completed",
        }
    }
}

/// Durable lifecycle for a patch captured from a writable workflow leaf.
///
/// A patch starts as `pending_apply` when the worker's throwaway worktree
/// produced a diff. It then moves by latest-wins rows to `applied`, `rejected`,
/// or `conflict` after an explicit operator/Lead/workflow decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowPatchStatus {
    PendingApply,
    Applied,
    Rejected,
    Conflict,
}

/// Validation status of files recorded in a workflow artifact manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowArtifactManifestStatus {
    Current,
    Missing,
    Stale,
}

/// One run of a built-in (registered) workflow. The `workflow_name` selects the
/// registered Rust fn (option C in the design). `step_ids` orders the steps in
/// the sequence they were started, so the journal alone reconstructs the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: String,
    pub workflow_name: String,
    /// Project Binding that owns provider cwd, repository instructions, Skills,
    /// Git/worktree policy, and delivery paths for this run. The surrounding
    /// Execution Space owns this row but never substitutes for a workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_binding_id: Option<String>,
    pub status: WorkflowRunStatus,
    #[serde(default)]
    pub step_ids: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    /// Optional human-facing summary set when the run reaches a terminal state.
    #[serde(default)]
    pub summary: Option<String>,
    /// Optional JSON parameterization the run was authored with (the dynamic
    /// `run-script` path carries the Starlark `args` global). `None` for registry
    /// runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// How many agent steps this run spawned (the per-run agent count). Defaults
    /// to 0 for legacy rows that predate the field.
    #[serde(default)]
    pub agents_spawned: u64,
    /// The collected structured output of the run (e.g. each step's result),
    /// set when the run reaches a terminal state. `None` while running / legacy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_output: Option<serde_json::Value>,
    /// Who initiated this run — an agent member id (e.g. a Codex / Claude member)
    /// or "operator" for a human-triggered CLI run. `None` for legacy rows that
    /// predate the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiated_by: Option<String>,
    /// The mandatory `design_intent` a Starlark program declares via its
    /// `workflow(name, design_intent)` header — the WHY behind the run's shape.
    /// Every dynamic (`run-script`) run carries it; `None` for registry runs and
    /// legacy rows that predate the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design_intent: Option<String>,
    /// The authored source the dynamic path was run with — for `run-script` the
    /// raw Starlark program text, snapshotted as the small durable audit record
    /// of the run shape. `None` for registry runs / legacy rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spec: Option<serde_json::Value>,
    /// OS process id of the `harness workflow run-script`/`run` invocation that
    /// drives this run, stamped on the initial `running` row. The serve-side
    /// reaper uses it to detect an ABANDONED run: if the run is still `running`
    /// but this pid is no longer alive on the host, the driver died (killed /
    /// crashed / Ctrl-C) before journaling a terminal outcome, so the reaper
    /// flips it (and its non-terminal steps) to `failed`. `None` for legacy rows
    /// that predate the field — those fall back to a stale-activity timeout.
    /// Same-host only (the store, serve, and driver all run locally).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_pid: Option<u32>,
    /// True when this run was a `--dry-run` validation (mock driver, no provider
    /// spawned, no tokens spent), false for a real (live) run. A dry-run journals
    /// the SAME `workflow_name` into the SAME store, so without this marker a dry
    /// validation run is easily mistaken for a real one when reading the jsonl or
    /// the dashboard (issue #89 item 2). `#[serde(default)]` → legacy rows read as
    /// `false` (they predate the flag; dry-run journaling is newer).
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<WorkflowTerminalReason>,
    #[serde(default)]
    pub partial_output_available: bool,
}

/// One agent step inside a [`WorkflowRun`]. `phase` is the declarative grouping
/// marker (e.g. "audit", "synthesize"); `label` names the step within the phase.
/// `native_session` links to the provider-owned execution record. Harness keeps
/// the Workflow outcome and evidence here, but never mirrors the provider turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub id: String,
    pub run_id: String,
    pub phase: String,
    pub label: String,
    #[serde(default)]
    pub native_session: Option<NativeSessionRef>,
    pub status: WorkflowStepStatus,
    #[serde(default)]
    pub output_summary: Option<String>,
    /// Optional structured result for this step (beyond the human-facing
    /// `output_summary`). The dynamic IR path carries each `StepResult`'s
    /// structured payload here. `None` for legacy / summary-only steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    pub started_at: String,
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_reason: Option<WorkflowTerminalReason>,
    #[serde(default)]
    pub partial: bool,
}

/// A durable patch captured from a writable workflow step.
///
/// The actual unified diff lives at `patch_ref` so dashboard snapshots stay
/// compact while CLI `workflow patch show/apply` can still retrieve the complete
/// patch text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowPatch {
    pub id: String,
    pub run_id: String,
    pub step_id: String,
    pub label: String,
    pub phase: String,
    pub provider: String,
    pub status: WorkflowPatchStatus,
    #[serde(default)]
    pub changed_paths: Vec<String>,
    /// Absolute or store-relative path to the `.patch` file.
    pub patch_ref: String,
    #[serde(default)]
    pub base_sha: Option<String>,
    #[serde(default)]
    pub owned_paths: Vec<String>,
    #[serde(default)]
    pub persist_changes: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub conflict_detail: Option<String>,
    #[serde(default)]
    pub applied_at: Option<String>,
    #[serde(default)]
    pub rejected_at: Option<String>,
}

/// One file entry inside a workflow artifact manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowArtifactFile {
    /// Repo-relative path when under the project root, else the absolute path the
    /// workflow explicitly declared.
    pub path: String,
    #[serde(default)]
    pub exists: bool,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub hash: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

/// Durable manifest for files a workflow claims as artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowArtifactManifest {
    pub id: String,
    pub run_id: String,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub artifact_root: Option<String>,
    pub status: WorkflowArtifactManifestStatus,
    #[serde(default)]
    pub files: Vec<WorkflowArtifactFile>,
    #[serde(default)]
    pub write_roots: Vec<String>,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

impl Validate for WorkflowRun {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowRun.id")?;
        require_non_empty(&self.workflow_name, "WorkflowRun.workflow_name")?;
        if let Some(binding) = &self.project_binding_id {
            require_non_empty(binding, "WorkflowRun.project_binding_id")?;
        }
        require_non_empty(&self.created_at, "WorkflowRun.created_at")
    }
}

impl Validate for WorkflowStep {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowStep.id")?;
        require_non_empty(&self.run_id, "WorkflowStep.run_id")?;
        require_non_empty(&self.label, "WorkflowStep.label")?;
        require_non_empty(&self.started_at, "WorkflowStep.started_at")
    }
}

impl Validate for WorkflowPatch {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowPatch.id")?;
        require_non_empty(&self.run_id, "WorkflowPatch.run_id")?;
        require_non_empty(&self.step_id, "WorkflowPatch.step_id")?;
        require_non_empty(&self.label, "WorkflowPatch.label")?;
        require_non_empty(&self.patch_ref, "WorkflowPatch.patch_ref")?;
        require_non_empty(&self.created_at, "WorkflowPatch.created_at")
    }
}

impl Validate for WorkflowArtifactFile {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.path, "WorkflowArtifactFile.path")
    }
}

impl Validate for WorkflowArtifactManifest {
    fn validate(&self) -> Result<(), ValidationError> {
        require_non_empty(&self.id, "WorkflowArtifactManifest.id")?;
        require_non_empty(&self.run_id, "WorkflowArtifactManifest.run_id")?;
        require_non_empty(&self.created_at, "WorkflowArtifactManifest.created_at")?;
        for file in &self.files {
            file.validate()?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Agent Team v0 runtime ledger objects
//
// A team run is one execution of an agent team against an objective, hosted on

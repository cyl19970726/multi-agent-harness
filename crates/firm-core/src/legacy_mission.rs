use super::*;

// Mission plus Legacy Wave compatibility contracts (ADR 0026, superseded by
// ADR 0051)
//
// A Mission owns durable intent, context, one flat AgentTeam, and outcome.
// Each LegacyWave is a pre-ADR 0051 Host plan/judgment memo. Execution
// records remain independently addressable and are related through Mission,
// assignment messages, correlations, and optional source_plan_ref.
// ---------------------------------------------------------------------------

/// Lifecycle of a [`Mission`]. Execution progress belongs to the selected
/// TeamRun, WorkflowRun, Host, and provider-native sessions—not to a Legacy
/// Wave row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    #[default]
    Planned,
    Running,
    Blocked,
    Completed,
    Cancelled,
}

/// Durable operator intent. `desired_outcome` captures the intended result;
/// `outcome_summary` is filled only after execution has produced one. A Mission
/// does not contain a task graph or executor-specific state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mission {
    pub id: String,
    pub title: String,
    pub objective: String,
    /// Durable Markdown brief used by the Host. Material revisions are
    /// appended to Mission Log; older rows deserialize as an empty brief.
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub desired_outcome: Option<String>,
    #[serde(default)]
    pub status: MissionStatus,
    /// Pre-ADR 0051 ordered Wave identities. The historical wire key remains
    /// `wave_ids`; new Missions always keep it empty and current code must not
    /// use it as plan or lifecycle authority.
    #[serde(default, rename = "wave_ids", skip_serializing_if = "Vec::is_empty")]
    pub legacy_wave_ids: Vec<String>,
    #[serde(default)]
    pub outcome_summary: Option<String>,
    /// Actor that explicitly performed Mission closeout. Legacy Wave acceptance does
    /// not infer this responsibility.
    #[serde(default)]
    pub completed_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub completed_at: Option<String>,
}

/// Compatibility/projection hint retained only on pre-ADR 0051
/// [`LegacyWave`] rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyWaveExecutorKind {
    AgentTeam,
    DynamicWorkflow,
    Host,
}

/// Historical lifecycle of a [`LegacyWave`]. Current Mission planning has no
/// separate lifecycle object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LegacyWaveStatus {
    #[default]
    Planned,
    Running,
    Waiting,
    Completed,
    Blocked,
    Failed,
    Cancelled,
}

/// Historical acceptance state for a [`LegacyWave`]. Current Mission Log
/// entries have no gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LegacyWaveGateStatus {
    #[default]
    Pending,
    Accepted,
    Revise,
    Blocked,
}

/// One pre-ADR 0051 Host plan/judgment row. This type exists only to decode and
/// export historical `waves.jsonl`; current product code uses Mission plus
/// append-only MissionLogEntry and cannot create or mutate this row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyWave {
    pub id: String,
    pub mission_id: String,
    pub index: u32,
    pub title: String,
    pub objective: String,
    /// Versioned Markdown operational memo: the Host's current plan, judgment,
    /// assignments, carry-over, and important deviations.
    #[serde(default)]
    pub context: String,
    /// Monotonic revision within this Legacy Wave id. Append-only historical
    /// rows retain the
    /// prior revisions.
    #[serde(default)]
    pub revision: u32,
    /// Actor that authored the latest revision.
    #[serde(default)]
    pub updated_by: Option<String>,
    #[serde(default)]
    pub exit_criteria: Option<String>,
    #[serde(default)]
    pub status: LegacyWaveStatus,
    /// Historical direct-executor hint; new authoring uses `Host`.
    pub executor_kind: LegacyWaveExecutorKind,
    /// Historical direct-executor attempt references.
    #[serde(default)]
    pub executor_run_ids: Vec<String>,
    /// Historical accepted direct-executor attempt.
    #[serde(default)]
    pub accepted_run_id: Option<String>,
    #[serde(default)]
    pub plan_note: Option<String>,
    #[serde(default)]
    pub outcome_summary: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub gate_status: LegacyWaveGateStatus,
    #[serde(default)]
    pub gate_note: Option<String>,
    #[serde(default)]
    pub accepted_by: Option<String>,
    #[serde(default)]
    pub accepted_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

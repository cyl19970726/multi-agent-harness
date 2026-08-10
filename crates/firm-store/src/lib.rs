use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use firm_core::{
    content_hash_hex16, provider_interaction_response_id, AgentTeam, AgentTeamRun, Decision,
    DelegationRun, Evidence, ExecutionNode, ExecutionNodeStatus, Gap, GitHubLink, HostAttention,
    HostAttentionInbox, HostAttentionKind, HostAttentionStatus, HostBindingLease,
    HostBindingLeaseOwnerKind, HostBindingLeaseStatus, MemberAction, MessageTerminalSource,
    Mission, MissionLogEntry, MissionStatus, NodeDaemonLease, NodeDaemonLeaseStatus,
    NodeProjectRegistration, NodeProjectRegistrationStatus, PendingInteraction, Proposal,
    ProviderChildThread, ProviderCompatibilityAdmission, ProviderCompatibilityAdmissionLifecycle,
    ProviderCompatibilityBlockBoundary, ProviderCompatibilityBlockCause,
    ProviderCompatibilityStatus, ProviderDispatchEnvelope, ProviderDispatchEvent,
    ProviderDispatchIntent, ProviderExecutionStatus, ProviderIntegrationProfile,
    ProviderInteractionRequestBody, ProviderInteractionResponseBody, ProviderLaunchProfile,
    ProviderProcess, ProviderRuntimeProjection, ProviderWorkDispatch, ProviderWorkDispatchStatus,
    ProviderWorkDispatchUpdate, RegistryDeliveryAttempt, RegistryDeliveryStatus, RegistryMessage,
    Review, TeamActorKind, TeamDeliveryPolicy, TeamDeliveryStatus, TeamMemberCloseRequest,
    TeamMemberCloseStatus, TeamRunEvent, TeamRunStatus, TeamSupervisorLease,
    TeamSupervisorLeaseStatus, Validate, Vision, Wave, WaveGateStatus, WaveStatus, Work,
    WorkClaimMode, WorkCommandContext, WorkCondition, WorkConditionRecord, WorkDelegation,
    WorkDelegationEvent, WorkDelegationRevision, WorkDelegationState, WorkDelegationTransition,
    WorkEvent, WorkEventKind, WorkEvidence, WorkOperation, WorkOperationalDecision, WorkPhase,
    WorkRef, WorkReport, WorkResolution, WorkflowArtifactManifest, WorkflowPatch, WorkflowRun,
    WorkflowStep,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use thiserror::Error;

mod trust_kernel;
pub use trust_kernel::*;

mod company_os;
pub mod docs_v2;
pub use company_os::{
    ActionAuditReservation, ActionCommandClaimResult, CompanyActor, FinancialRecord,
};

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;
pub const PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER: &str =
    "provider_compatibility_admissions.jsonl";
fn work_event_order(left: &WorkEvent, right: &WorkEvent) -> std::cmp::Ordering {
    let left_ms = left
        .created_at
        .strip_prefix("unix-ms:")
        .and_then(|value| value.parse::<u128>().ok());
    let right_ms = right
        .created_at
        .strip_prefix("unix-ms:")
        .and_then(|value| value.parse::<u128>().ok());
    match (left_ms, right_ms) {
        (Some(left_ms), Some(right_ms)) => left_ms.cmp(&right_ms),
        _ => left.created_at.cmp(&right.created_at),
    }
    .then(left.sequence.cmp(&right.sequence))
    .then(left.id.cmp(&right.id))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureProviderCompatibilityAdmissionResult {
    pub admission: ProviderCompatibilityAdmission,
    pub created: bool,
}

fn canonical_provider_admission_evidence_refs(values: &[String]) -> Vec<String> {
    let mut canonical = values.to_vec();
    canonical.sort();
    canonical.dedup();
    canonical
}

fn canonical_work_candidate_revision(
    result_summary: &str,
    artifact_refs: &[String],
    check_refs: &[String],
    github_links: &[GitHubLink],
) -> String {
    let mut artifacts = artifact_refs.to_vec();
    artifacts.sort();
    artifacts.dedup();
    let mut checks = check_refs.to_vec();
    checks.sort();
    checks.dedup();
    let mut links = github_links
        .iter()
        .map(|link| serde_json::to_string(link).expect("GitHubLink serializes"))
        .collect::<Vec<_>>();
    links.sort();
    links.dedup();
    let bytes = serde_json::to_vec(&serde_json::json!({
        "result_summary": result_summary,
        "artifact_refs": artifacts,
        "check_refs": checks,
        "github_links": links,
    }))
    .expect("canonical Work candidate serializes");
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("work-content-fnv1a64:{hash:016x}")
}

fn provider_admission_replay_matches(
    existing: &ProviderCompatibilityAdmission,
    candidate: &ProviderCompatibilityAdmission,
) -> bool {
    existing.project_id == candidate.project_id
        && existing.store_id == candidate.store_id
        && existing.exact_key() == candidate.exact_key()
        && existing.policy == candidate.policy
        && existing.actor == candidate.actor
        && canonical_provider_admission_evidence_refs(&existing.evidence_refs)
            == canonical_provider_admission_evidence_refs(&candidate.evidence_refs)
        && existing.lifecycle == ProviderCompatibilityAdmissionLifecycle::Active
        && candidate.lifecycle == ProviderCompatibilityAdmissionLifecycle::Active
        && existing.predecessor_admission_id.is_none()
        && candidate.predecessor_admission_id.is_none()
        && existing.reason.is_none()
        && candidate.reason.is_none()
}

fn validate_provider_compatibility_admission_ledger(
    rows: &[ProviderCompatibilityAdmission],
) -> StoreResult<()> {
    type ScopedKey = (String, String, String, String, String, String);
    let mut ids = std::collections::BTreeSet::new();
    let mut active: std::collections::BTreeMap<ScopedKey, &ProviderCompatibilityAdmission> =
        std::collections::BTreeMap::new();

    for row in rows {
        if !ids.insert(row.id.clone()) {
            return Err(StoreError::Conflict(format!(
                "provider compatibility ledger contains duplicate admission id {}",
                row.id
            )));
        }
        let key = (
            row.project_id.clone(),
            row.store_id.clone(),
            row.provider.clone(),
            row.execution_mode.clone(),
            row.provider_version.clone(),
            row.adapter_contract_version.clone(),
        );
        match row.lifecycle {
            ProviderCompatibilityAdmissionLifecycle::Active => {
                if let Some(current) = active.get(&key) {
                    return Err(StoreError::Conflict(format!(
                        "provider compatibility ledger forks active tuple at {} and {}",
                        current.id, row.id
                    )));
                }
                active.insert(key, row);
            }
            ProviderCompatibilityAdmissionLifecycle::Revoked
            | ProviderCompatibilityAdmissionLifecycle::Superseded => {
                let predecessor_id = row
                    .predecessor_admission_id
                    .as_deref()
                    .expect("validated terminal admission has predecessor");
                let predecessor = active.get(&key).ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "provider compatibility terminal {} has no current active predecessor",
                        row.id
                    ))
                })?;
                if predecessor.id != predecessor_id {
                    return Err(StoreError::Conflict(format!(
                        "provider compatibility terminal {} names non-current predecessor {}; expected {}",
                        row.id, predecessor_id, predecessor.id
                    )));
                }
                if predecessor.policy != row.policy {
                    return Err(StoreError::Conflict(format!(
                        "provider compatibility terminal {} changes predecessor policy",
                        row.id
                    )));
                }
                active.remove(&key);
            }
        }
    }
    Ok(())
}

/// Normalize surface identifiers into their canonical form.
/// All surface comparisons and storage MUST route through this.
/// Aliases: kimi|kimi-cli|kimi-code → kimi; codex|codex-app|codex-app-server → codex;
/// claude|claude-code → claude. Unknown surfaces pass through unchanged.
pub fn canonical_surface(surface: &str) -> &str {
    match surface {
        "kimi" | "kimi-cli" | "kimi-code" => "kimi",
        "codex" | "codex-app" | "codex-app-server" => "codex",
        "claude" | "claude-code" => "claude",
        other => other,
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("timed out waiting for store write lock {0}")]
    LockTimeout(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid company os record: {0}")]
    CompanyOsValidation(String),
    #[error("company os reference not found: {0}")]
    CompanyOsMissingReference(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// Crash-atomic composite row for cross-Team delegation. The target Work
/// creation event and Delegation creation event are committed in one JSONL
/// record; ordinary Work readers fold the embedded operation as native Work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkDelegationOperation {
    delegation: WorkDelegation,
    event: WorkDelegationEvent,
    target_work_operation: WorkOperation,
}

/// Canonical semantic request identity for WorkDelegation creation. Entity ids
/// are included (callers derive omitted ids from the idempotency key), while
/// only envelope ids and timestamps are excluded. Every persisted creation
/// field that can change responsibility or target Work intent is therefore
/// conflict-significant.
fn work_delegation_request_fingerprint(
    delegation: &WorkDelegation,
    target_work: &Work,
    context: &WorkCommandContext,
) -> serde_json::Value {
    serde_json::json!({
        "delegation": {
            "id": delegation.id,
            "source_work_ref": delegation.source_work_ref,
            "source_work_version": delegation.source_work_version,
            "source_owner_member_id": delegation.source_owner_member_id,
            "created_by_member_run_id": delegation.created_by_member_run_id,
            "target_agent_team_id": delegation.target_agent_team_id,
            "target_work_ref": delegation.target_work_ref,
            "delegated_by_actor": delegation.delegated_by_actor,
            "state": delegation.state,
            "resolution_summary": delegation.resolution_summary,
            "blocker_reason": delegation.blocker_reason,
            "version": delegation.version,
        },
        "target_work": {
            "id": target_work.id,
            "team_run_id": target_work.team_run_id,
            "team_id": target_work.team_id,
            "parent_work_id": target_work.parent_work_id,
            "title": target_work.title,
            "context_markdown": target_work.context_markdown,
            "completion_criteria_markdown": target_work.completion_criteria_markdown,
            "phase": target_work.phase,
            "condition": target_work.condition,
            "resolution": target_work.resolution,
            "owner_member_id": target_work.owner_member_id,
            "active_member_run_id": target_work.active_member_run_id,
            "claim_mode": target_work.claim_mode,
            "eligible_member_ids": target_work.eligible_member_ids,
            "prerequisite_work_ids": target_work.prerequisite_work_ids,
            "priority": target_work.priority,
            "created_by_actor": target_work.created_by_actor,
            "created_by_member_id": target_work.created_by_member_id,
            "result_summary": target_work.result_summary,
            "blocker_reason": target_work.blocker_reason,
            "artifact_refs": target_work.artifact_refs,
            "check_refs": target_work.check_refs,
            "github_links": target_work.github_links,
            "version": target_work.version,
        },
        "performed_by_actor": context.performed_by_actor,
        "authority_actor": context.authority_actor,
        "causation_ref": context.causation_ref,
        "duplicate_ok": context.duplicate_ok,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageDeliveryClaimResult {
    Claimed(Box<RegistryMessage>),
    NotQueued,
    BlockedByDelivery(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamMessageDeliveryClaimResult {
    Claimed(Box<ProviderDispatchEnvelope>),
    NotQueued,
}

fn reject_raw_provider_interaction_append(value: &ProviderDispatchEnvelope) -> StoreResult<()> {
    if matches!(
        value.kind,
        ProviderDispatchIntent::ProviderInteractionRequest
            | ProviderDispatchIntent::ProviderInteractionResponse
    ) {
        return Err(StoreError::Conflict(
            "PROVIDER_INTERACTION_RAW_APPEND_FORBIDDEN: use append_team_message_checked for requests or record_provider_interaction_response for responses"
                .to_string(),
        ));
    }
    Ok(())
}

fn same_provider_interaction_response(
    existing: &ProviderDispatchEnvelope,
    retry: &ProviderDispatchEnvelope,
) -> bool {
    existing.team_run_id == retry.team_run_id
        && existing.work_id == retry.work_id
        && existing.source_plan_ref == retry.source_plan_ref
        && existing.sender == retry.sender
        && existing.sender_runtime_id == retry.sender_runtime_id
        && existing.recipients == retry.recipients
        && existing.recipient_runtime_ids == retry.recipient_runtime_ids
        && existing.kind == retry.kind
        && existing.body == retry.body
        && existing.correlation_id == retry.correlation_id
        && existing.causation_id == retry.causation_id
        && existing.response_intent == retry.response_intent
        && existing.evidence_refs == retry.evidence_refs
        && existing.deliveries.len() == retry.deliveries.len()
        && existing
            .deliveries
            .iter()
            .zip(&retry.deliveries)
            .all(|(existing, retry)| {
                existing.member_id == retry.member_id && existing.policy == retry.policy
            })
}

/// Returns true when the request row changed and must be appended.
fn acknowledge_provider_interaction_request(
    request: &mut ProviderDispatchEnvelope,
    acknowledged_at: &str,
) -> StoreResult<bool> {
    let host_deliveries = request
        .deliveries
        .iter_mut()
        .filter(|delivery| delivery.member_id == "host")
        .collect::<Vec<_>>();
    if host_deliveries.len() != 1 {
        return Err(StoreError::Conflict(
            "provider interaction request requires exactly one Host delivery".to_string(),
        ));
    }
    let delivery = host_deliveries.into_iter().next().expect("one delivery");
    if delivery.policy != TeamDeliveryPolicy::ManualAck {
        return Err(StoreError::Conflict(
            "provider interaction request Host delivery must use manual_ack".to_string(),
        ));
    }
    match delivery.status {
        TeamDeliveryStatus::Acknowledged => Ok(false),
        TeamDeliveryStatus::Delivered => {
            delivery.status = TeamDeliveryStatus::Acknowledged;
            delivery.updated_at = acknowledged_at.to_string();
            Ok(true)
        }
        status => Err(StoreError::Conflict(format!(
            "provider interaction request Host delivery cannot be acknowledged from {status:?}"
        ))),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkDeliveryClaimResult {
    Claimed(Box<ProviderWorkDispatch>),
    NotQueued,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAttentionClaimResult {
    Claimed(Box<HostAttention>),
    NotActionable,
}

#[derive(Debug, Clone)]
pub struct HarnessStore {
    root: PathBuf,
    provider_compatibility_scope: Option<(String, String)>,
}

impl HarnessStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            provider_compatibility_scope: None,
        }
    }

    /// Bind compatibility admissions to the Project Binding and execution
    /// store selected by the caller. The scope is deliberately explicit and
    /// is never inferred from a path hash: moving/migrating a store must not
    /// silently transfer operational authority.
    pub fn with_provider_compatibility_scope(
        mut self,
        project_id: impl Into<String>,
        store_id: impl Into<String>,
    ) -> Self {
        self.provider_compatibility_scope = Some((project_id.into(), store_id.into()));
        self
    }

    pub fn provider_compatibility_scope(&self) -> Option<(&str, &str)> {
        self.provider_compatibility_scope
            .as_ref()
            .map(|(project_id, store_id)| (project_id.as_str(), store_id.as_str()))
    }

    fn require_provider_compatibility_scope(&self) -> StoreResult<(&str, &str)> {
        self.provider_compatibility_scope().ok_or_else(|| {
            StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_SCOPE_REQUIRED: provider compatibility authority requires an explicitly configured project/store scope"
                    .to_string(),
            )
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn init(&self) -> StoreResult<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.root.join("prompts"))?;
        fs::create_dir_all(self.root.join("runtimes"))?;
        Ok(())
    }

    /// Hold the store's ordinary writer lock for the complete lifetime of a
    /// migration snapshot.
    ///
    /// The guard intentionally exposes no store operations. Callers must not
    /// invoke a write method on this same `HarnessStore` while it is alive:
    /// those methods acquire `.store.lock` themselves and would be a
    /// re-entrant lock attempt. Direct, read-only filesystem snapshots are the
    /// intended use while the guard is held.
    pub fn acquire_exclusive_migration_guard(&self) -> StoreResult<StoreExclusiveMigrationGuard> {
        Ok(StoreExclusiveMigrationGuard {
            _write_lock: self.acquire_write_lock()?,
        })
    }

    pub fn append_mission(&self, value: &Mission) -> StoreResult<()> {
        self.append_jsonl("missions.jsonl", value)
    }

    /// Compare-and-append one Mission revision. Team membership is not stored
    /// on Mission: `AgentTeam.mission_id` is the single relation authority.
    pub fn compare_and_append_mission(
        &self,
        expected: &Mission,
        next: &Mission,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("mission not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "mission {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        if next.id != current.id
            || next.created_at != current.created_at
            || next.wave_ids != current.wave_ids
        {
            return Err(StoreError::Conflict(
                "mission revision must preserve identity, creation time, and Wave membership"
                    .to_string(),
            ));
        }
        if next.status == MissionStatus::Running {
            let active_team_count = self
                .read_jsonl::<AgentTeam>("teams.jsonl")?
                .into_iter()
                .filter(|team| {
                    team.mission_id == next.id && team.status == firm_core::AgentTeamStatus::Active
                })
                .count();
            if active_team_count != 1 {
                return Err(StoreError::Conflict(format!(
                    "MISSION_REQUIRES_TEAM: running mission {} requires exactly one active AgentTeam, found {active_team_count}",
                    next.id
                )));
            }
        }
        self.append_jsonl_unlocked("missions.jsonl", next)
    }

    /// Insert a new native Mission under the store lock. Unlike the generic
    /// append method this rejects a concurrently-created duplicate id.
    pub fn insert_mission(&self, value: &Mission) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let missions = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        });
        if missions.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "mission already exists: {}",
                value.id
            )));
        }
        self.append_jsonl_unlocked("missions.jsonl", value)
    }

    pub fn append_wave(&self, value: &Wave) -> StoreResult<()> {
        self.append_jsonl("waves.jsonl", value)
    }

    /// Atomically allocate/validate one Wave index, append the Wave, and update
    /// its Mission's ordered membership. This prevents concurrent creates from
    /// duplicating an index or losing one `wave_ids` update.
    pub fn insert_wave_and_update_mission(
        &self,
        mut wave: Wave,
        requested_index: Option<u32>,
        mission_updated_at: &str,
    ) -> StoreResult<Wave> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut missions = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        });
        let mut mission = missions.remove(&wave.mission_id).ok_or_else(|| {
            StoreError::Conflict(format!("native mission not found: {}", wave.mission_id))
        })?;
        if matches!(
            mission.status,
            MissionStatus::Completed | MissionStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "mission {} is {:?} and cannot accept another Wave",
                mission.id, mission.status
            )));
        }
        let waves = latest_by_id(self.read_jsonl::<Wave>("waves.jsonl")?, |row| {
            row.id.clone()
        })
        .into_values()
        .collect::<Vec<_>>();
        if waves.iter().any(|existing| existing.id == wave.id) {
            return Err(StoreError::Conflict(format!(
                "wave already exists: {}",
                wave.id
            )));
        }
        wave.index = match requested_index {
            Some(index) => index,
            None => waves
                .iter()
                .filter(|existing| existing.mission_id == wave.mission_id)
                .map(|existing| existing.index)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "wave index space is exhausted for mission {}",
                        wave.mission_id
                    ))
                })?,
        };
        if wave.index == 0 {
            return Err(StoreError::Conflict(
                "wave index must be at least 1".to_string(),
            ));
        }
        if waves
            .iter()
            .any(|existing| existing.mission_id == wave.mission_id && existing.index == wave.index)
        {
            return Err(StoreError::Conflict(format!(
                "wave index {} already exists for mission {}",
                wave.index, wave.mission_id
            )));
        }

        let mut ordered = waves
            .iter()
            .filter(|existing| existing.mission_id == wave.mission_id)
            .map(|existing| (existing.index, existing.id.clone()))
            .collect::<Vec<_>>();
        ordered.push((wave.index, wave.id.clone()));
        ordered.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        mission.wave_ids = ordered.into_iter().map(|(_, id)| id).collect();
        mission.updated_at = mission_updated_at.to_string();

        self.append_jsonl_unlocked("waves.jsonl", &wave)?;
        self.append_jsonl_unlocked("missions.jsonl", &mission)?;
        Ok(wave)
    }

    /// Append one [`MissionLogEntry`] under the store lock, atomically
    /// allocating its monotonic `revision` the same way
    /// `insert_wave_and_update_mission` allocates a Wave index: read the
    /// current max for this `mission_id`, then `+ 1` (starting at 1). This
    /// is the Mission Log's ONLY write operation (ADR 0051) — there is no
    /// update or delete, so unlike Wave there is no compare-and-append
    /// variant to race against.
    pub fn append_mission_log_entry(
        &self,
        mut entry: MissionLogEntry,
    ) -> StoreResult<MissionLogEntry> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if entry.body.trim().is_empty() {
            return Err(StoreError::Conflict(
                "mission log entry body must not be empty".to_string(),
            ));
        }
        if entry.actor.trim().is_empty() {
            return Err(StoreError::Conflict(
                "mission log entry actor must not be empty".to_string(),
            ));
        }
        let missions = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        });
        if !missions.contains_key(&entry.mission_id) {
            return Err(StoreError::Conflict(format!(
                "mission not found: {}",
                entry.mission_id
            )));
        }
        let existing = self.read_jsonl::<MissionLogEntry>("mission_log.jsonl")?;
        if existing.iter().any(|row| row.id == entry.id) {
            return Err(StoreError::Conflict(format!(
                "mission log entry already exists: {}",
                entry.id
            )));
        }
        entry.revision = existing
            .iter()
            .filter(|row| row.mission_id == entry.mission_id)
            .map(|row| row.revision)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "mission log revision space is exhausted for mission {}",
                    entry.mission_id
                ))
            })?;
        self.append_jsonl_unlocked("mission_log.jsonl", &entry)?;
        Ok(entry)
    }

    /// Atomically close one Mission. Prior to ADR 0051 this required every
    /// ordered Wave to have an accepted, completed gate; Wave write commands
    /// (including the gate) are now retired, so a native post-cutover
    /// Mission always has empty `wave_ids` and closes on its own outcome —
    /// the Host records `kind = closeout_evidence` in the Mission Log
    /// beforehand by convention, not as a store-enforced precondition (ADR
    /// 0051 "Mission closeout evidence becomes a ... Log entry instead of a
    /// separate Wave-outcome convention"). A legacy Mission that already
    /// accumulated `wave_ids` before the cutover keeps the original
    /// Wave-gate requirement so its in-flight contract does not change
    /// underneath it; no NEW Mission can reach that branch since Wave create
    /// no longer populates membership. The Wave set is still checked under
    /// the same store lock as the Mission CAS so a concurrent Wave create
    /// (of a legacy, still-populated Mission) cannot race closeout.
    pub fn compare_and_close_mission(&self, expected: &Mission, next: &Mission) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("mission not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "mission {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        if !current.wave_ids.is_empty() {
            let waves = latest_by_id(self.read_jsonl::<Wave>("waves.jsonl")?, |wave| {
                wave.id.clone()
            });
            let mut actual_wave_ids = waves
                .values()
                .filter(|wave| wave.mission_id == current.id)
                .map(|wave| (wave.index, wave.id.clone()))
                .collect::<Vec<_>>();
            actual_wave_ids.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
            let actual_wave_ids = actual_wave_ids
                .into_iter()
                .map(|(_, id)| id)
                .collect::<Vec<_>>();
            if actual_wave_ids != current.wave_ids {
                return Err(StoreError::Conflict(format!(
                    "mission {} Wave membership changed or is inconsistent; retry closeout",
                    current.id
                )));
            }
            for wave_id in &current.wave_ids {
                let wave = waves.get(wave_id).ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "mission {} references missing Wave {wave_id}",
                        current.id
                    ))
                })?;
                if wave.mission_id != current.id
                    || wave.status != WaveStatus::Completed
                    || wave.gate_status != WaveGateStatus::Accepted
                {
                    return Err(StoreError::Conflict(format!(
                        "mission {} cannot close: Wave {} is status {:?} with gate {:?}",
                        current.id, wave.id, wave.status, wave.gate_status
                    )));
                }
            }
        }
        if next.id != current.id
            || next.status != MissionStatus::Completed
            || next.outcome_summary.as_deref().is_none_or(str::is_empty)
            || next.completed_by.as_deref().is_none_or(str::is_empty)
            || next.completed_at.as_deref().is_none_or(str::is_empty)
        {
            return Err(StoreError::Conflict(
                "mission closeout must preserve identity and record completed status, outcome, actor, and timestamp"
                    .to_string(),
            ));
        }
        self.append_jsonl_unlocked("missions.jsonl", next)
    }

    pub fn append_member(&self, value: &ProviderLaunchProfile) -> StoreResult<()> {
        self.append_jsonl("provider_launch_profiles.jsonl", value)
    }

    /// Compare-and-append a Team revision while preserving its Mission, Host,
    /// Node placement, and creation identity.
    pub fn append_team(&self, value: &AgentTeam) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<AgentTeam>("teams.jsonl")?, |team| {
            team.id.clone()
        })
        .remove(&value.id)
        .ok_or_else(|| StoreError::Conflict(format!("agent team not found: {}", value.id)))?;
        if value.id != current.id
            || value.created_at != current.created_at
            || value.mission_id != current.mission_id
            || value.host_agent_id != current.host_agent_id
            || value.node_id != current.node_id
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_IDENTITY_IMMUTABLE: AgentTeam {} cannot change Mission, Host, Node, id, or creation time",
                value.id
            )));
        }
        self.append_jsonl_unlocked("teams.jsonl", value)
    }

    /// Insert the one AgentTeam for a Mission. Mission uniqueness and active
    /// Node placement are checked under the same Store write boundary.
    pub fn insert_agent_team_with_unique_mission(&self, value: &AgentTeam) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let teams = latest_by_id(self.read_jsonl::<AgentTeam>("teams.jsonl")?, |team| {
            team.id.clone()
        });
        if teams.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "agent team already exists: {}",
                value.id
            )));
        }
        if teams
            .values()
            .any(|team| team.mission_id == value.mission_id)
        {
            return Err(StoreError::Conflict(format!(
                "MISSION_ALREADY_HAS_TEAM: Mission {} already has an AgentTeam",
                value.mission_id
            )));
        }
        let mission = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        })
        .remove(&value.mission_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_REQUIRES_MISSION: Mission {} not found",
                value.mission_id
            ))
        })?;
        if matches!(
            mission.status,
            MissionStatus::Completed | MissionStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "TEAM_REQUIRES_MISSION: Mission {} is terminal",
                value.mission_id
            )));
        }
        let node = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(&value.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("NODE_NOT_ACTIVE: {} not found", value.node_id))
        })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(StoreError::Conflict(format!(
                "NODE_NOT_ACTIVE: {} is {:?}",
                value.node_id, node.status
            )));
        }
        self.append_jsonl_unlocked("teams.jsonl", value)
    }

    /// Append a new active operational admission for one exact provider tuple.
    ///
    /// Admission ids are stable command ids: replaying an identical record is
    /// idempotent, while reusing an id for different content is a conflict.
    /// Only one row for an exact tuple may be active at a time.
    pub fn append_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        self.admit_provider_compatibility_admission(value)
    }

    pub fn admit_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        if value.lifecycle != ProviderCompatibilityAdmissionLifecycle::Active {
            return Err(StoreError::Conflict(
                "provider compatibility admission must have active lifecycle".to_string(),
            ));
        }
        self.append_provider_compatibility_admission_checked(value)
    }

    /// Atomically create or reuse the active admission represented by a
    /// command request.
    ///
    /// Generated ids and timestamps are deliberately excluded from replay
    /// identity. Evidence references are a set: ordering and duplicates do
    /// not change command semantics, and newly appended rows store them in
    /// sorted, deduplicated order. Any other difference remains a conflict.
    pub fn ensure_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<EnsureProviderCompatibilityAdmissionResult> {
        if value.lifecycle != ProviderCompatibilityAdmissionLifecycle::Active {
            return Err(StoreError::Conflict(
                "provider compatibility admission must have active lifecycle".to_string(),
            ));
        }
        let (project_id, store_id) = self.require_provider_compatibility_scope()?;
        if value.project_id != project_id || value.store_id != store_id {
            return Err(StoreError::Conflict(format!(
                "provider compatibility admission scope mismatch: current project/store is {project_id}/{store_id}, record is {}/{}",
                value.project_id, value.store_id
            )));
        }
        let mut candidate = value.clone();
        candidate.evidence_refs =
            canonical_provider_admission_evidence_refs(&candidate.evidence_refs);
        candidate
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;

        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let rows = self.provider_compatibility_admissions()?;

        if let Some(existing) = rows.iter().find(|row| row.id == candidate.id) {
            if existing == &candidate {
                return Ok(EnsureProviderCompatibilityAdmissionResult {
                    admission: existing.clone(),
                    created: false,
                });
            }
            return Err(StoreError::Conflict(format!(
                "provider compatibility admission id {} already has different content",
                candidate.id
            )));
        }

        let current = rows.iter().rev().find(|row| {
            row.project_id == candidate.project_id
                && row.store_id == candidate.store_id
                && row.exact_key() == candidate.exact_key()
        });
        if let Some(active) = current.filter(|row| row.is_active()) {
            if provider_admission_replay_matches(active, &candidate) {
                return Ok(EnsureProviderCompatibilityAdmissionResult {
                    admission: active.clone(),
                    created: false,
                });
            }
            return Err(StoreError::Conflict(format!(
                "provider compatibility tuple already has semantically different active admission {}",
                active.id
            )));
        }

        self.append_jsonl_unlocked(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER, &candidate)?;
        Ok(EnsureProviderCompatibilityAdmissionResult {
            admission: candidate,
            created: true,
        })
    }

    /// Compatibility alias for callers that name the operation, rather than
    /// the ledger record.
    pub fn admit_provider_compatibility(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        self.admit_provider_compatibility_admission(value)
    }

    pub fn revoke_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        if value.lifecycle != ProviderCompatibilityAdmissionLifecycle::Revoked {
            return Err(StoreError::Conflict(
                "provider compatibility revocation must have revoked lifecycle".to_string(),
            ));
        }
        self.append_provider_compatibility_admission_checked(value)
    }

    pub fn revoke_provider_compatibility(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        self.revoke_provider_compatibility_admission(value)
    }

    pub fn supersede_provider_compatibility_admission(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        if value.lifecycle != ProviderCompatibilityAdmissionLifecycle::Superseded {
            return Err(StoreError::Conflict(
                "provider compatibility supersession must have superseded lifecycle".to_string(),
            ));
        }
        self.append_provider_compatibility_admission_checked(value)
    }

    pub fn supersede_provider_compatibility(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        self.supersede_provider_compatibility_admission(value)
    }

    fn append_provider_compatibility_admission_checked(
        &self,
        value: &ProviderCompatibilityAdmission,
    ) -> StoreResult<()> {
        let (project_id, store_id) = self.require_provider_compatibility_scope()?;
        if value.project_id != project_id || value.store_id != store_id {
            return Err(StoreError::Conflict(format!(
                "provider compatibility admission scope mismatch: current project/store is {project_id}/{store_id}, record is {}/{}",
                value.project_id, value.store_id
            )));
        }
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let rows = self.provider_compatibility_admissions()?;

        if let Some(existing) = rows.iter().find(|row| row.id == value.id) {
            if existing == value {
                return Ok(());
            }
            return Err(StoreError::Conflict(format!(
                "provider compatibility admission id {} already has different content",
                value.id
            )));
        }

        let current = rows.iter().rev().find(|row| {
            row.project_id == value.project_id
                && row.store_id == value.store_id
                && row.exact_key() == value.exact_key()
        });
        match value.lifecycle {
            ProviderCompatibilityAdmissionLifecycle::Active => {
                if let Some(active) = current.filter(|row| row.is_active()) {
                    return Err(StoreError::Conflict(format!(
                        "provider compatibility tuple already has active admission {}",
                        active.id
                    )));
                }
            }
            ProviderCompatibilityAdmissionLifecycle::Revoked
            | ProviderCompatibilityAdmissionLifecycle::Superseded => {
                let predecessor_id = value
                    .predecessor_admission_id
                    .as_deref()
                    .expect("validated terminal admission has predecessor");
                let predecessor = current.filter(|row| row.is_active()).ok_or_else(|| {
                    StoreError::Conflict(
                        "provider compatibility transition has no current active predecessor"
                            .to_string(),
                    )
                })?;
                if predecessor.id != predecessor_id {
                    return Err(StoreError::Conflict(format!(
                        "provider compatibility predecessor is stale: expected {}, got {}",
                        predecessor.id, predecessor_id
                    )));
                }
                if predecessor.project_id != value.project_id
                    || predecessor.store_id != value.store_id
                    || predecessor.policy != value.policy
                {
                    return Err(StoreError::Conflict(
                        "provider compatibility transition must preserve predecessor scope and policy"
                            .to_string(),
                    ));
                }
            }
        }
        self.append_jsonl_unlocked(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER, value)
    }

    pub fn append_runtime(&self, value: &ProviderProcess) -> StoreResult<()> {
        self.append_jsonl("provider_processes.jsonl", value)
    }

    pub fn append_event(&self, value: &ProviderDispatchEvent) -> StoreResult<()> {
        self.append_jsonl("provider_dispatch_events.jsonl", value)
    }

    pub fn append_proposal(&self, value: &Proposal) -> StoreResult<()> {
        self.append_jsonl("proposals.jsonl", value)
    }

    pub fn append_message(&self, value: &RegistryMessage) -> StoreResult<()> {
        self.append_jsonl("messages.jsonl", value)
    }

    pub fn append_evidence(&self, value: &Evidence) -> StoreResult<()> {
        self.append_jsonl("evidence.jsonl", value)
    }

    pub fn append_decision(&self, value: &Decision) -> StoreResult<()> {
        self.append_jsonl("decisions.jsonl", value)
    }

    pub fn append_review(&self, value: &Review) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if self
            .read_jsonl::<Review>("reviews.jsonl")?
            .iter()
            .any(|review| review.id == value.id)
        {
            return Err(StoreError::Conflict(format!(
                "review already exists: {}",
                value.id
            )));
        }
        self.append_jsonl_unlocked("reviews.jsonl", value)
    }

    /// Record a Review bound to the exact current Work candidate. Identity and
    /// binding fields are derived from trusted Store context rather than caller
    /// supplied payload.
    pub fn append_gap(&self, value: &Gap) -> StoreResult<()> {
        self.append_jsonl("gaps.jsonl", value)
    }

    pub fn append_vision(&self, value: &Vision) -> StoreResult<()> {
        self.append_jsonl("visions.jsonl", value)
    }

    pub fn append_provider_child_thread(&self, value: &ProviderChildThread) -> StoreResult<()> {
        self.append_jsonl("provider_child_threads.jsonl", value)
    }

    pub fn append_workflow_run(&self, value: &WorkflowRun) -> StoreResult<()> {
        self.append_jsonl("workflow_runs.jsonl", value)
    }

    pub fn append_workflow_step(&self, value: &WorkflowStep) -> StoreResult<()> {
        self.append_jsonl("workflow_steps.jsonl", value)
    }

    pub fn append_workflow_patch(&self, value: &WorkflowPatch) -> StoreResult<()> {
        self.append_jsonl("workflow_patches.jsonl", value)
    }

    pub fn append_workflow_artifact_manifest(
        &self,
        value: &WorkflowArtifactManifest,
    ) -> StoreResult<()> {
        self.append_jsonl("workflow_artifact_manifests.jsonl", value)
    }

    pub fn append_team_run(&self, value: &AgentTeamRun) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(current) =
            latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
                run.id.clone()
            })
            .remove(&value.id)
        {
            if current == *value {
                return Ok(());
            }
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_REVISION_REQUIRES_CAS: raw TeamRun revision {} cannot change identity, Host binding, scope, lifecycle, or membership",
                value.id
            )));
        }
        self.append_jsonl_unlocked("team_runs.jsonl", value)
    }

    /// Compare-and-append one TeamRun revision.
    ///
    /// Host binding is mutable coordination metadata, but changing it must not
    /// silently overwrite a concurrent lifecycle/member update. Keep the
    /// identity, execution scope, and creation time stable while allowing the
    /// caller to revise addressability fields and `updated_at`.
    pub fn compare_and_append_team_run(
        &self,
        expected: &AgentTeamRun,
        next: &AgentTeamRun,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "team run {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        if next.member_run_ids != current.member_run_ids {
            return Err(StoreError::Conflict(
                "TEAM_MEMBERSHIP_REQUIRES_ADMISSION: Host binding revision cannot change member_run_ids"
                    .to_string(),
            ));
        }
        if next.id != current.id
            || next.created_at != current.created_at
            || next.agent_team_id != current.agent_team_id
            || next.execution_node_id != current.execution_node_id
            || next.project_binding_id != current.project_binding_id
            || next.previous_run_id != current.previous_run_id
            || next.execution_root != current.execution_root
            || next.member_run_ids != current.member_run_ids
            || next.status != current.status
            || next.objective != current.objective
            || next.budget_limit_usd != current.budget_limit_usd
            || next.completed_at != current.completed_at
        {
            return Err(StoreError::Conflict(
                "Host binding revision must preserve TeamRun identity, scope, members, lifecycle, and objective"
                    .to_string(),
            ));
        }
        self.append_jsonl_unlocked("team_runs.jsonl", next)
    }

    /// Acquire exclusive ownership of a TeamRun's current exact Host binding.
    /// A live owner is never preempted. Expiry, release, or a TeamRun rebind
    /// permits takeover and advances the durable generation.
    #[allow(clippy::too_many_arguments)]
    pub fn acquire_host_binding_lease(
        &self,
        team_run_id: &str,
        host_surface: &str,
        host_thread_id: &str,
        owner_kind: HostBindingLeaseOwnerKind,
        owner_id: &str,
        lease_id: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        require_non_empty_store(owner_id, "Host binding lease owner id")?;
        require_non_empty_store(lease_id, "Host binding lease id")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let run =
            self.require_exact_host_binding_unlocked(team_run_id, host_surface, host_thread_id)?;
        let current = self.latest_host_binding_lease_unlocked(team_run_id)?;
        if let Some(current) = current.as_ref() {
            let current_matches_binding = canonical_surface(&current.host_surface)
                == canonical_surface(&run.host_surface)
                && current.host_thread_id == host_thread_id;
            if current.is_effective_at(now_unix_ms) && current_matches_binding {
                if current.owner_kind == owner_kind
                    && current.owner_id == owner_id
                    && current.lease_id == lease_id
                {
                    return Ok(current.clone());
                }
                return Err(StoreError::Conflict(format!(
                    "HOST_BINDING_LEASE_HELD: TeamRun {team_run_id} binding is owned by {:?} {} generation {} until unix-ms:{}",
                    current.owner_kind, current.owner_id, current.generation, current.expires_unix_ms
                )));
            }
        }
        let generation = match current.as_ref() {
            Some(current) => current.generation.checked_add(1).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "HOST_BINDING_LEASE_GENERATION_EXHAUSTED: TeamRun {team_run_id}"
                ))
            })?,
            None => 1,
        };
        let lease = HostBindingLease {
            team_run_id: team_run_id.to_string(),
            host_surface: run.host_surface,
            host_thread_id: host_thread_id.to_string(),
            owner_kind,
            owner_id: owner_id.to_string(),
            generation,
            lease_id: lease_id.to_string(),
            acquired_unix_ms: now_unix_ms,
            heartbeat_unix_ms: now_unix_ms,
            expires_unix_ms: now_unix_ms.saturating_add(ttl_ms.max(1)),
            status: HostBindingLeaseStatus::Active,
            released_unix_ms: None,
        };
        lease
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("host_binding_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Renew an exact current lease. Every identity component is a CAS fence.
    pub fn renew_host_binding_lease(
        &self,
        expected: &HostBindingLease,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_exact_host_binding_unlocked(
            &expected.team_run_id,
            &expected.host_surface,
            &expected.host_thread_id,
        )?;
        let mut current =
            self.require_current_host_binding_lease_owner_unlocked(expected, now_unix_ms)?;
        current.heartbeat_unix_ms = now_unix_ms;
        current.expires_unix_ms = now_unix_ms.saturating_add(ttl_ms.max(1));
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("host_binding_leases.jsonl", &current)?;
        Ok(current)
    }

    /// Release an exact current lease. An exact retry is idempotent; every
    /// stale generation, lease id, or owner is rejected.
    pub fn release_host_binding_lease(
        &self,
        expected: &HostBindingLease,
        now_unix_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.require_exact_host_binding_unlocked(
            &expected.team_run_id,
            &expected.host_surface,
            &expected.host_thread_id,
        )?;
        let mut current = self
            .latest_host_binding_lease_unlocked(&expected.team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TeamRun {} has no Host binding lease",
                    expected.team_run_id
                ))
            })?;
        self.require_same_host_binding_lease_owner(&current, expected)?;
        if current.status == HostBindingLeaseStatus::Released {
            return Ok(current);
        }
        current.status = HostBindingLeaseStatus::Released;
        current.heartbeat_unix_ms = now_unix_ms;
        current.expires_unix_ms = now_unix_ms;
        current.released_unix_ms = Some(now_unix_ms);
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("host_binding_leases.jsonl", &current)?;
        Ok(current)
    }

    /// Latest persisted row, including released and expired rows. `None` is
    /// the explicit legacy/unleased state.
    pub fn latest_host_binding_lease(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<HostBindingLease>> {
        self.latest_host_binding_lease_unlocked(team_run_id)
    }

    /// Return the active lease only when it is live and still matches the
    /// TeamRun's current exact Host binding.
    pub fn effective_host_binding_lease_at(
        &self,
        team_run_id: &str,
        now_unix_ms: u64,
    ) -> StoreResult<Option<HostBindingLease>> {
        let run = self.require_team_run_unlocked(team_run_id)?;
        Ok(self
            .latest_host_binding_lease_unlocked(team_run_id)?
            .filter(|lease| {
                lease.is_effective_at(now_unix_ms)
                    && canonical_surface(&lease.host_surface)
                        == canonical_surface(&run.host_surface)
                    && run.host_thread_id.as_deref() == Some(lease.host_thread_id.as_str())
            }))
    }

    /// Materialize one deterministic HostBindingStale attention for every
    /// bound TeamRun whose current binding has no effective lease. Repeated
    /// scans of the same binding/generation are idempotent.
    pub fn reconcile_host_binding_stale_attentions(
        &self,
        now_unix_ms: u64,
        observed_at: &str,
    ) -> StoreResult<Vec<HostAttention>> {
        require_non_empty_store(observed_at, "Host binding stale observed_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_host_binding_stale_attentions_unlocked(now_unix_ms, observed_at)
    }

    /// Idempotently append one durable Host-attention fact.
    ///
    /// Runtime integration must derive `attention.id` from the causal event
    /// (for example `host-attention-<work-event-id>`). Replaying the same event
    /// returns the latest delivery/intake projection instead of resetting it
    /// to `actionable` or fabricating a ProviderDispatchEnvelope.
    pub fn ensure_host_attention(&self, attention: &HostAttention) -> StoreResult<HostAttention> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_host_attention_unlocked(attention)
    }

    /// Repair the only intentional two-ledger crash boundary: a WorkOperation
    /// may be fsynced immediately before its derived HostAttention row. The
    /// deterministic attention id makes this replay safe and lets Host reads or
    /// an explicit startup reconciliation materialize exactly the missing row.
    pub fn reconcile_work_host_attentions(&self) -> StoreResult<Vec<HostAttention>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()
    }

    /// Latest-wins Host-attention projection across all TeamRuns.
    pub fn host_attentions(&self) -> StoreResult<Vec<HostAttention>> {
        self.reconcile_work_host_attentions()?;
        Ok(self
            .latest_host_attentions_unlocked()?
            .into_values()
            .collect())
    }

    /// Read one TeamRun's Host-attention projection, including an explicit
    /// warning when no exact native Host task is bound.
    pub fn host_attention_inbox_for_team_run(
        &self,
        team_run_id: &str,
        include_all: bool,
    ) -> StoreResult<HostAttentionInbox> {
        self.reconcile_work_host_attentions()?;
        self.host_attention_inbox_for_team_run_unreconciled(team_run_id, include_all)
    }

    /// Aggregate only attentions owned by the exact provider-native Host task.
    /// Unbound TeamRuns and other tasks are excluded by construction.
    pub fn host_attention_inboxes_for_native_thread(
        &self,
        host_surface: &str,
        host_thread_id: &str,
        include_all: bool,
    ) -> StoreResult<Vec<HostAttentionInbox>> {
        if host_surface.trim().is_empty() || host_thread_id.trim().is_empty() {
            return Err(StoreError::Conflict(
                "Host surface and native thread id must not be empty".to_string(),
            ));
        }
        self.reconcile_work_host_attentions()?;
        let runs = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        });
        let mut inboxes = Vec::new();
        for run in runs.into_values().filter(|run| {
            canonical_surface(&run.host_surface) == canonical_surface(host_surface)
                && run.host_thread_id.as_deref() == Some(host_thread_id)
        }) {
            let inbox =
                self.host_attention_inbox_for_team_run_unreconciled(&run.id, include_all)?;
            if include_all || !inbox.attentions.is_empty() {
                inboxes.push(inbox);
            }
        }
        Ok(inboxes)
    }

    /// Fence one delivery attempt to the TeamRun's current exact Host binding.
    /// A claimed or delivered row cannot be claimed again, which prevents a
    /// managed idle wake and a safe-boundary hook from both starting delivery.
    pub fn claim_host_attention(
        &self,
        attention_id: &str,
        host_surface: &str,
        host_thread_id: &str,
        claim_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttentionClaimResult> {
        require_non_empty_store(attention_id, "Host attention id")?;
        require_non_empty_store(host_surface, "Host surface")?;
        require_non_empty_store(host_thread_id, "Host thread id")?;
        require_non_empty_store(claim_id, "Host attention claim id")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        self.require_exact_host_binding_unlocked(
            &attention.team_run_id,
            host_surface,
            host_thread_id,
        )?;
        if attention.status == HostAttentionStatus::Claimed
            && attention.claim_id.as_deref() == Some(claim_id)
            && attention.claimed_host_surface.as_deref() == Some(host_surface)
            && attention.claimed_host_thread_id.as_deref() == Some(host_thread_id)
        {
            return Ok(HostAttentionClaimResult::Claimed(Box::new(attention)));
        }
        if attention.status != HostAttentionStatus::Actionable {
            return Ok(HostAttentionClaimResult::NotActionable);
        }
        attention.status = HostAttentionStatus::Claimed;
        attention.attempt = attention.attempt.saturating_add(1);
        attention.claim_id = Some(claim_id.to_string());
        attention.claimed_host_surface = Some(host_surface.to_string());
        attention.claimed_host_thread_id = Some(host_thread_id.to_string());
        attention.claimed_host_lease_id = None;
        attention.claimed_host_lease_generation = None;
        attention.claimed_host_lease_owner_id = None;
        attention.provider_receipt_id = None;
        attention.last_failure_reason = None;
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(HostAttentionClaimResult::Claimed(Box::new(attention)))
    }

    /// Record provider-native delivery receipt for the currently-owned claim.
    pub fn complete_host_attention_claim(
        &self,
        attention_id: &str,
        claim_id: &str,
        provider_receipt_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(provider_receipt_id, "Host attention provider receipt")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        if attention.status == HostAttentionStatus::Delivered
            && attention.claim_id.as_deref() == Some(claim_id)
            && attention.provider_receipt_id.as_deref() == Some(provider_receipt_id)
        {
            return Ok(attention);
        }
        if attention.status != HostAttentionStatus::Claimed
            || attention.claim_id.as_deref() != Some(claim_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention claim {claim_id} no longer owns {attention_id}"
            )));
        }
        let surface = attention.claimed_host_surface.clone().ok_or_else(|| {
            StoreError::Conflict("claimed HostAttention has no Host surface".to_string())
        })?;
        let thread_id = attention.claimed_host_thread_id.clone().ok_or_else(|| {
            StoreError::Conflict("claimed HostAttention has no Host thread id".to_string())
        })?;
        self.require_exact_host_binding_unlocked(&attention.team_run_id, &surface, &thread_id)?;
        if attention.claimed_host_lease_id.is_some() {
            let now_unix_ms = parse_iso8601_to_unix_ms(updated_at).ok_or_else(|| {
                StoreError::Conflict(
                    "leased HostAttention completion requires unix-ms updated_at".to_string(),
                )
            })?;
            self.require_host_attention_lease_fence_unlocked(&attention, now_unix_ms)?;
        }
        attention.status = HostAttentionStatus::Delivered;
        attention.provider_receipt_id = Some(provider_receipt_id.to_string());
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// Return an uncertain/failed claim to the actionable state for retry.
    pub fn fail_host_attention_claim(
        &self,
        attention_id: &str,
        claim_id: &str,
        reason: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(reason, "Host attention failure reason")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        if attention.status != HostAttentionStatus::Claimed
            || attention.claim_id.as_deref() != Some(claim_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention claim {claim_id} no longer owns {attention_id}"
            )));
        }
        if attention.claimed_host_lease_id.is_some() {
            let now_unix_ms = parse_iso8601_to_unix_ms(updated_at).ok_or_else(|| {
                StoreError::Conflict(
                    "leased HostAttention failure requires unix-ms updated_at".to_string(),
                )
            })?;
            self.require_host_attention_lease_fence_unlocked(&attention, now_unix_ms)?;
        }
        attention.status = HostAttentionStatus::Actionable;
        attention.claim_id = None;
        attention.claimed_host_surface = None;
        attention.claimed_host_thread_id = None;
        attention.claimed_host_lease_id = None;
        attention.claimed_host_lease_generation = None;
        attention.claimed_host_lease_owner_id = None;
        attention.provider_receipt_id = None;
        attention.last_failure_reason = Some(reason.to_string());
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// ACK transport intake from the exact currently-bound Host task. This is
    /// intentionally independent of Work accept/request-changes commands.
    pub fn acknowledge_host_attention(
        &self,
        attention_id: &str,
        host_surface: &str,
        host_thread_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        self.require_exact_host_binding_unlocked(
            &attention.team_run_id,
            host_surface,
            host_thread_id,
        )?;
        if attention.status == HostAttentionStatus::Acknowledged {
            return Ok(attention);
        }
        if attention.status != HostAttentionStatus::Delivered
            || attention
                .claimed_host_surface
                .as_deref()
                .map(canonical_surface)
                != Some(canonical_surface(host_surface))
            || attention.claimed_host_thread_id.as_deref() != Some(host_thread_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention {attention_id} has not been delivered to this exact Host task"
            )));
        }
        attention.status = HostAttentionStatus::Acknowledged;
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// Mark a Host attention as requiring explicit human escalation. Only valid
    /// from `Actionable` or `Claimed` states. This is a terminal state set by
    /// the headless host dispatcher when an attention needs human decision
    /// (accept/merge/cancel) that the triage-only host cannot make.
    pub fn escalate_host_attention(
        &self,
        attention_id: &str,
        reason: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(attention_id, "Host attention id")?;
        require_non_empty_store(reason, "Host attention escalation reason")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        if attention.status == HostAttentionStatus::EscalationRequired {
            return Ok(attention);
        }
        if attention.status != HostAttentionStatus::Actionable
            && attention.status != HostAttentionStatus::Claimed
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention {attention_id} is not in a state that can be escalated (current: {:?})",
                attention.status
            )));
        }
        // Release any stale claim so the attention is cleanly terminal.
        attention.status = HostAttentionStatus::EscalationRequired;
        attention.claim_id = None;
        attention.claimed_host_surface = None;
        attention.claimed_host_thread_id = None;
        attention.claimed_host_lease_id = None;
        attention.claimed_host_lease_generation = None;
        attention.claimed_host_lease_owner_id = None;
        attention.provider_receipt_id = None;
        attention.last_failure_reason = Some(reason.to_string());
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// Return actionable Host attentions whose `created_at` timestamp is older
    /// than `older_than_unix_ms`. Used by the host dispatcher to find attentions
    /// eligible for headless triage.
    pub fn actionable_attentions_older_than(
        &self,
        older_than_unix_ms: u64,
    ) -> StoreResult<Vec<HostAttention>> {
        self.reconcile_work_host_attentions()?;
        let all = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| {
                if attention.status != HostAttentionStatus::Actionable {
                    return false;
                }
                // Parse the ISO 8601 created_at to a unix ms timestamp.
                // If parsing fails, treat the attention as eligible (fail open
                // so stale-but-malformed rows don't block dispatch forever).
                match crate::parse_iso8601_to_unix_ms(&attention.created_at) {
                    Some(ts) => ts < older_than_unix_ms,
                    None => true,
                }
            })
            .collect();
        Ok(all)
    }

    /// Atomically claim an aged actionable batch under the exact current
    /// Dispatcher lease. A live Interactive lease cannot satisfy this fence,
    /// and the store lock gives concurrent dispatchers one winner.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_dispatcher_host_attention_batch(
        &self,
        expected_lease: &HostBindingLease,
        older_than_unix_ms: u64,
        limit: usize,
        claim_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<Vec<HostAttention>> {
        require_non_empty_store(claim_id, "Host attention batch claim id")?;
        require_non_empty_store(updated_at, "Host attention batch updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()?;
        self.reconcile_host_binding_stale_attentions_unlocked(now_unix_ms, updated_at)?;
        let current =
            self.require_current_host_binding_lease_owner_unlocked(expected_lease, now_unix_ms)?;
        if current.owner_kind != HostBindingLeaseOwnerKind::Dispatcher {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_INTERACTIVE_SUPPRESSES_DISPATCH: TeamRun {} is owned by Interactive Host {}",
                current.team_run_id, current.owner_id
            )));
        }
        self.require_exact_host_binding_unlocked(
            &current.team_run_id,
            &current.host_surface,
            &current.host_thread_id,
        )?;
        self.requeue_fenced_host_attention_claims_unlocked(&current, now_unix_ms, updated_at)?;

        let mut eligible = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| {
                attention.team_run_id == current.team_run_id
                    && attention.status == HostAttentionStatus::Actionable
                    && parse_iso8601_to_unix_ms(&attention.created_at)
                        .map(|created| created < older_than_unix_ms)
                        .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        eligible.sort_by(|left, right| {
            compare_store_timestamps(&left.created_at, &right.created_at)
                .then(left.id.cmp(&right.id))
        });
        eligible.truncate(limit);
        for attention in &mut eligible {
            attention.status = HostAttentionStatus::Claimed;
            attention.attempt = attention.attempt.saturating_add(1);
            attention.claim_id = Some(claim_id.to_string());
            attention.claimed_host_surface = Some(current.host_surface.clone());
            attention.claimed_host_thread_id = Some(current.host_thread_id.clone());
            attention.claimed_host_lease_id = Some(current.lease_id.clone());
            attention.claimed_host_lease_generation = Some(current.generation);
            attention.claimed_host_lease_owner_id = Some(current.owner_id.clone());
            attention.provider_receipt_id = None;
            attention.last_failure_reason = None;
            attention.updated_at = updated_at.to_string();
            self.append_jsonl_unlocked("host_attentions.jsonl", attention)?;
        }
        Ok(eligible)
    }

    /// Create one TeamRun from its durable AgentTeam. Mission and Node cannot
    /// be supplied independently: they are derived and validated from Team.
    pub fn create_team_run_from_agent_team(
        &self,
        value: &AgentTeamRun,
        execution_space_id: &str,
    ) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        require_non_empty_store(execution_space_id, "Execution Space id")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let runs = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        });
        if runs.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "team run already exists: {}",
                value.id
            )));
        }
        let team = latest_by_id(self.read_jsonl::<AgentTeam>("teams.jsonl")?, |team| {
            team.id.clone()
        })
        .remove(&value.agent_team_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_RUN_REQUIRES_TEAM: AgentTeam {} not found",
                value.agent_team_id
            ))
        })?;
        if team.status != firm_core::AgentTeamStatus::Active {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_REQUIRES_TEAM: AgentTeam {} is {:?}",
                team.id, team.status
            )));
        }
        if value.execution_node_id != team.node_id {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_NODE_MISMATCH: TeamRun {} names {}, Team {} is placed on {}",
                value.id, value.execution_node_id, team.id, team.node_id
            )));
        }
        let mission = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        })
        .remove(&team.mission_id)
        .ok_or_else(|| StoreError::Conflict(format!("mission not found: {}", team.mission_id)))?;
        if matches!(
            mission.status,
            MissionStatus::Completed | MissionStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_REQUIRES_TEAM: Mission {} is terminal",
                mission.id
            )));
        }
        let node = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(&team.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("NODE_NOT_ACTIVE: {} not found", team.node_id))
        })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(StoreError::Conflict(format!(
                "NODE_NOT_ACTIVE: {} is {:?}",
                node.id, node.status
            )));
        }
        let registrations = latest_by_id(
            self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
            node_project_registration_identity,
        );
        let matching_registrations = registrations
            .values()
            .filter(|registration| {
                registration.node_id == team.node_id
                    && registration.execution_space_id == execution_space_id
                    && registration.project_binding_id == value.project_binding_id
                    && registration.status == NodeProjectRegistrationStatus::Active
            })
            .count();
        if matching_registrations != 1 {
            return Err(StoreError::Conflict(format!(
                "PROJECT_NOT_REGISTERED_ON_NODE: expected one active registration for {} on Node {}, found {matching_registrations}",
                value.project_binding_id, team.node_id
            )));
        }
        if let Some(previous_id) = value.previous_run_id.as_deref() {
            let previous = runs.get(previous_id).ok_or_else(|| {
                StoreError::Conflict(format!("previous team run not found: {previous_id}"))
            })?;
            if previous.agent_team_id != value.agent_team_id {
                return Err(StoreError::Conflict(format!(
                    "previous run {previous_id} belongs to AgentTeam {}",
                    previous.agent_team_id
                )));
            }
        }
        self.append_jsonl_unlocked("team_runs.jsonl", value)
    }

    /// Compare-and-append one Wave row. Used only for historical Wave reads and
    /// legacy maintenance; TeamRun lifecycle no longer writes Wave/Mission.
    /// concurrent attempt registration or gate cannot be silently overwritten.
    pub fn compare_and_append_wave(&self, expected: &Wave, next: &Wave) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<Wave>("waves.jsonl")?, |wave| {
            wave.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("wave not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "wave {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        let mut missions = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        });
        let mut mission = missions.remove(&next.mission_id).ok_or_else(|| {
            StoreError::Conflict(format!("native mission not found: {}", next.mission_id))
        })?;
        if matches!(
            mission.status,
            MissionStatus::Completed | MissionStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "mission {} is {:?} and its Waves are immutable",
                mission.id, mission.status
            )));
        }
        mission.status = match next.gate_status {
            WaveGateStatus::Blocked => MissionStatus::Blocked,
            WaveGateStatus::Accepted | WaveGateStatus::Revise | WaveGateStatus::Pending => {
                MissionStatus::Running
            }
        };
        mission.updated_at = next.updated_at.clone();
        self.append_jsonl_unlocked("waves.jsonl", next)?;
        self.append_jsonl_unlocked("missions.jsonl", &mission)
    }

    pub fn append_member_run(&self, value: &ProviderRuntimeProjection) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if value.provider_compatibility_block_cause.is_some() {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_BLOCK_AUTHORITY_REQUIRED: initial ProviderRuntimeProjection append cannot set a typed compatibility cause"
                    .to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let rows = self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?;
        let current = latest_by_id(rows, |row| row.id.clone()).remove(&value.id);
        if let Some(current) = current {
            if current == *value {
                return Ok(());
            }
            return Err(StoreError::Conflict(format!(
                "MEMBER_REVISION_REQUIRES_CAS: ProviderRuntimeProjection {} already exists; use compare_and_append_member_run",
                value.id
            )));
        } else {
            // Initial team creation predeclares every runtime id in the first
            // TeamRun row. Materializing one of those rows cannot extend or
            // rewrite membership, and raw later TeamRun revisions are barred
            // from changing the list.
            let first_run = self
                .read_jsonl::<AgentTeamRun>("team_runs.jsonl")?
                .into_iter()
                .find(|run| run.id == value.team_run_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!("team run not found: {}", value.team_run_id))
                })?;
            if !first_run.member_run_ids.iter().any(|id| id == &value.id) {
                return Err(StoreError::Conflict(format!(
                    "MEMBER_ADMISSION_REQUIRED: ProviderRuntimeProjection {} was not declared by initial TeamRun {}",
                    value.id, value.team_run_id
                )));
            }
            let latest_run = self.require_team_run_unlocked(&value.team_run_id)?;
            self.ensure_unique_member_identity_unlocked(&latest_run, value)?;
        }
        self.append_jsonl_unlocked("member_runs.jsonl", value)
    }

    /// Compare-and-append one existing ProviderRuntimeProjection revision. Raw append cannot
    /// mutate lifecycle authority; all legitimate close/reopen/runtime updates
    /// must prove the exact revision they observed.
    pub fn compare_and_append_member_run(
        &self,
        expected: &ProviderRuntimeProjection,
        next: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        ensure_member_provenance_unchanged(&current, next)?;
        ensure_member_lifecycle_revision(&current, next)?;
        ensure_provider_compatibility_cause_unchanged(&current, next)?;
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("member_runs.jsonl", next)
    }

    /// Atomically enter a compatibility-owned Blocked state. This is the only
    /// Store API allowed to introduce a typed compatibility cause.
    pub fn block_member_run_for_provider_compatibility(
        &self,
        expected: &ProviderRuntimeProjection,
        profile: &ProviderIntegrationProfile,
        cause: ProviderCompatibilityBlockCause,
        last_event_at: &str,
    ) -> StoreResult<ProviderRuntimeProjection> {
        cause
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if !current.coordination_is_active()
            || current.finished_at.is_some()
            || !matches!(
                current.status,
                firm_core::MemberRunStatus::Idle
                    | firm_core::MemberRunStatus::Queued
                    | firm_core::MemberRunStatus::Disconnected
            )
        {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_BLOCK_LIFECYCLE_INVALID: ProviderRuntimeProjection {} must have active coordination, unfinished runtime, and idle, queued, or disconnected status",
                current.id
            )));
        }
        if current.provider_compatibility_block_cause.is_some() {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_BLOCK_ALREADY_OWNED: ProviderRuntimeProjection {} already has a typed cause",
                current.id
            )));
        }
        ensure_compatibility_cause_matches_profile(&current, profile, &cause)?;
        if cause.compatibility_status != profile.compatibility_status {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_BLOCK_STATUS_MISMATCH: typed cause status does not match the observed provider profile"
                    .to_string(),
            ));
        }
        require_non_empty_store(last_event_at, "compatibility block last_event_at")?;
        let mut next = current.clone();
        next.provider_profile = Some(profile.clone());
        next.status = firm_core::MemberRunStatus::Blocked;
        next.provider_compatibility_block_cause = Some(cause);
        next.last_event_at = Some(last_event_at.to_string());
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("member_runs.jsonl", &next)?;
        Ok(next)
    }

    /// Atomically clear a compatibility-owned block after the current exact
    /// tuple is either source-reviewed or covered by an active admission.
    pub fn recover_member_run_from_provider_compatibility_block(
        &self,
        expected: &ProviderRuntimeProjection,
        profile: &ProviderIntegrationProfile,
        boundary: ProviderCompatibilityBlockBoundary,
        recovery_status: firm_core::MemberRunStatus,
        last_event_at: &str,
    ) -> StoreResult<ProviderRuntimeProjection> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        current
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if !current.coordination_is_active() || current.finished_at.is_some() {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_RECOVERY_LIFECYCLE_INVALID: ProviderRuntimeProjection {} must have active coordination and unfinished runtime",
                current.id
            )));
        }
        let cause = current
            .provider_compatibility_block_cause
            .as_ref()
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "PROVIDER_COMPATIBILITY_BLOCK_CAUSE_REQUIRED: ProviderRuntimeProjection {} has no typed compatibility cause",
                    current.id
                ))
            })?;
        if current.status != firm_core::MemberRunStatus::Blocked {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_BLOCK_STATE_MISMATCH: ProviderRuntimeProjection {} is not Blocked",
                current.id
            )));
        }
        let blocked_profile = current.provider_profile.as_ref().ok_or_else(|| {
            StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_BLOCK_PROFILE_REQUIRED: ProviderRuntimeProjection {} has no durable blocked provider profile",
                current.id
            ))
        })?;
        ensure_compatibility_cause_matches_profile(&current, blocked_profile, cause)?;
        if cause.boundary != boundary {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_RECOVERY_BOUNDARY_MISMATCH: typed cause boundary {:?} does not match current {:?} boundary",
                cause.boundary, boundary
            )));
        }
        if !matches!(
            recovery_status,
            firm_core::MemberRunStatus::Disconnected
                | firm_core::MemberRunStatus::Queued
                | firm_core::MemberRunStatus::Idle
        ) {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_RECOVERY_STATUS_INVALID: recovery target must be disconnected, queued, or idle"
                    .to_string(),
            ));
        }
        let authorized = if profile.compatibility_status == ProviderCompatibilityStatus::Current {
            profile.provider_version.as_ref().is_some_and(|version| {
                profile
                    .reviewed_provider_versions
                    .iter()
                    .any(|reviewed| reviewed == version)
            })
        } else if profile.compatibility_status == ProviderCompatibilityStatus::ReviewRequired {
            let (project_id, store_id) = self.provider_compatibility_scope().ok_or_else(|| {
                StoreError::Conflict(
                    "PROVIDER_COMPATIBILITY_SCOPE_REQUIRED: recovery requires an exact project/store scope"
                        .to_string(),
                )
            })?;
            let rows: Vec<ProviderCompatibilityAdmission> =
                self.read_jsonl(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER)?;
            for row in &rows {
                row.validate()
                    .map_err(|error| StoreError::Conflict(error.to_string()))?;
            }
            validate_provider_compatibility_admission_ledger(&rows)?;
            rows.into_iter()
                .rev()
                .find(|row| {
                    row.project_id == project_id
                        && row.store_id == store_id
                        && row.exact_key()
                            == (
                                profile.provider.as_str(),
                                profile.execution_mode.as_str(),
                                profile.provider_version.as_deref().unwrap_or(""),
                                profile.adapter_contract_version.as_deref().unwrap_or(""),
                            )
                })
                .is_some_and(|row| row.is_active())
        } else {
            false
        };
        if !authorized {
            return Err(StoreError::Conflict(format!(
                "PROVIDER_COMPATIBILITY_RECOVERY_NOT_AUTHORIZED: exact tuple for ProviderRuntimeProjection {} is not source-reviewed or actively admitted",
                current.id
            )));
        }
        require_non_empty_store(last_event_at, "compatibility recovery last_event_at")?;
        let mut next = current.clone();
        next.provider_profile = Some(profile.clone());
        next.status = recovery_status;
        next.provider_compatibility_block_cause = None;
        next.finished_at = None;
        next.last_event_at = Some(last_event_at.to_string());
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("member_runs.jsonl", &next)?;
        Ok(next)
    }

    /// Materialize one ProviderRuntimeProjection already declared by the immutable first
    /// TeamRun row. This is the compatibility path for initial team creation;
    /// later membership changes must use [`Self::admit_member_run`].
    pub fn materialize_initial_member_run(
        &self,
        value: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if value.provider_compatibility_block_cause.is_some() {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_BLOCK_AUTHORITY_REQUIRED: materialization cannot set a typed compatibility cause"
                    .to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if self
            .read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?
            .iter()
            .any(|row| row.id == value.id)
        {
            return Err(StoreError::Conflict(format!(
                "member run already exists: {}",
                value.id
            )));
        }
        let first_run = self
            .read_jsonl::<AgentTeamRun>("team_runs.jsonl")?
            .into_iter()
            .find(|run| run.id == value.team_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("team run not found: {}", value.team_run_id))
            })?;
        if !first_run.member_run_ids.iter().any(|id| id == &value.id) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_ADMISSION_REQUIRED: ProviderRuntimeProjection {} was not declared by initial TeamRun {}",
                value.id, value.team_run_id
            )));
        }
        let latest_run = self.require_team_run_unlocked(&value.team_run_id)?;
        self.ensure_unique_member_identity_unlocked(&latest_run, value)?;
        self.append_jsonl_unlocked("member_runs.jsonl", value)
    }

    /// Atomically admit exactly one new ProviderRuntimeProjection and publish the matching
    /// TeamRun membership revision. This Store API is an in-process authority
    /// boundary; callers at HTTP/MCP/provider transports must authenticate
    /// before invoking it.
    pub fn admit_member_run(
        &self,
        expected: &AgentTeamRun,
        next: &AgentTeamRun,
        member: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        member
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if member.provider_compatibility_block_cause.is_some() {
            return Err(StoreError::Conflict(
                "PROVIDER_COMPATIBILITY_BLOCK_AUTHORITY_REQUIRED: member admission cannot set a typed compatibility cause"
                    .to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "team run {} changed concurrently; retry member admission",
                expected.id
            )));
        }
        ensure_team_run_admission_revision(&current, next, member)?;
        if self
            .read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?
            .iter()
            .any(|row| row.id == member.id)
        {
            return Err(StoreError::Conflict(format!(
                "member run already exists: {}",
                member.id
            )));
        }
        self.ensure_member_admission_identity_unlocked(&current, member)?;
        self.append_jsonl_unlocked("team_runs.jsonl", next)?;
        self.append_jsonl_unlocked("member_runs.jsonl", member)
    }

    /// Insert a Work and its authoritative creation event/outbox as one
    /// crash-atomic JSONL row. Work commands intentionally refuse a legacy
    /// Assignment-message store so one Execution Space never has two ownership
    /// authorities.
    pub fn insert_work(&self, mut work: Work, context: WorkCommandContext) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            &work.id,
            WorkEventKind::Created,
        )? {
            return Ok(existing.work);
        }
        self.ensure_work_event_id_available_unlocked(&context.event_id)?;
        let team_run = self.require_team_run_unlocked(&work.team_run_id)?;
        if matches!(
            team_run.status,
            TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "team run {} is {:?} and cannot accept new Work",
                team_run.id, team_run.status
            )));
        }
        let run_team_id = durable_team_id(&team_run);
        match (work.team_id.as_deref(), run_team_id) {
            (Some(work_team_id), Some(run_team_id)) if work_team_id != run_team_id => {
                return Err(StoreError::Conflict(format!(
                    "TEAM_SCOPE_MISMATCH: Work names AgentTeam {work_team_id}, but TeamRun {} belongs to {run_team_id}",
                    team_run.id
                )));
            }
            (Some(_), Some(_)) => {}
            (None, Some(run_team_id)) => work.team_id = Some(run_team_id.to_string()),
            (Some(_), None) => {
                return Err(StoreError::Conflict(format!(
                    "TEAM_SCOPE_UNAVAILABLE: TeamRun {} has no durable AgentTeam identity",
                    team_run.id
                )));
            }
            _ => {}
        }
        if self.latest_works_unlocked()?.contains_key(work.id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "work already exists: {}",
                work.id
            )));
        }
        if !context.duplicate_ok {
            let normalized = normalize_work_title(&work.title);
            for existing in self.latest_works_unlocked()?.values() {
                if existing.team_run_id == work.team_run_id
                    && !existing.is_terminal()
                    && normalize_work_title(&existing.title) == normalized
                {
                    return Err(StoreError::Conflict(format!(
                        "DUPLICATE_TITLE: a non-terminal Work ({}) with title \"{}\" already exists in team run {}; pass --duplicate-ok to skip this guard",
                        existing.id, existing.title, work.team_run_id
                    )));
                }
            }
        }
        if work.title.trim().is_empty() || work.completion_criteria_markdown.trim().is_empty() {
            return Err(StoreError::Conflict(
                "work title and completion criteria are required".to_string(),
            ));
        }
        work.version = 1;
        work.phase = WorkPhase::Open;
        work.condition = WorkCondition::Normal;
        work.resolution = None;
        work.created_at = context.created_at.clone();
        work.updated_at = context.created_at.clone();
        if let Some(member_run_id) = work.active_member_run_id.as_deref() {
            let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
            self.ensure_member_can_receive_work_unlocked(&member)?;
            let stable_identity = member_identity(&member);
            if work
                .owner_member_id
                .as_deref()
                .is_some_and(|owner| owner != stable_identity)
            {
                return Err(StoreError::Conflict(
                    "owner_member_id does not match active ProviderRuntimeProjection stable identity".to_string(),
                ));
            }
            work.owner_member_id = Some(stable_identity);
        }
        work.created_by_actor = context.performed_by_actor.clone();
        match context.performed_by_actor.kind {
            firm_core::TeamActorKind::ProviderRuntimeProjection => {
                let member = self.require_member_run_unlocked(
                    &context.performed_by_actor.id,
                    &work.team_run_id,
                )?;
                if !member.coordination_is_active() {
                    return Err(StoreError::Conflict(
                        "only an active ProviderRuntimeProjection may create Work".to_string(),
                    ));
                }
                let own_identity = member_identity(&member);
                if work
                    .created_by_member_id
                    .as_deref()
                    .is_some_and(|creator| creator != own_identity)
                {
                    return Err(StoreError::Conflict(
                        "created_by_member_id does not match creator ProviderRuntimeProjection stable identity"
                            .to_string(),
                    ));
                }
                work.created_by_member_id = Some(own_identity.clone());
                if work
                    .owner_member_id
                    .as_deref()
                    .is_some_and(|owner| owner != own_identity)
                    || work
                        .active_member_run_id
                        .as_deref()
                        .is_some_and(|owner| owner != member.id)
                {
                    return Err(StoreError::Conflict(
                        "an ordinary Member may create only self-owned or unassigned Work"
                            .to_string(),
                    ));
                }
            }
            _ => {
                require_host_actor(&context.performed_by_actor)?;
                if work.created_by_member_id.is_some() {
                    return Err(StoreError::Conflict(
                        "only a ProviderRuntimeProjection actor may set created_by_member_id"
                            .to_string(),
                    ));
                }
            }
        }
        self.validate_work_relations_unlocked(&work)?;
        let deliveries =
            self.initial_work_deliveries_unlocked(&work, &context.event_id, &context.created_at)?;
        let operation = WorkOperation {
            event: WorkEvent {
                id: context.event_id,
                team_run_id: work.team_run_id.clone(),
                work_id: work.id.clone(),
                sequence: 1,
                kind: WorkEventKind::Created,
                expected_version: 0,
                resulting_version: 1,
                performed_by_actor: context.performed_by_actor,
                authority_actor: context.authority_actor,
                causation_ref: context.causation_ref,
                idempotency_key: context.idempotency_key,
                payload: serde_json::Value::Null,
                created_at: context.created_at,
            },
            work: work.clone(),
            condition_records: Vec::new(),
            reports: Vec::new(),
            evidence_records: Vec::new(),
            decisions: Vec::new(),
            deliveries,
            delivery_updates: Vec::new(),
            delegation_revisions: Vec::new(),
        };
        self.append_work_operation_unlocked(&operation)?;
        Ok(work)
    }

    /// Create a target Team's root Work and the cross-Team Delegation in one
    /// crash-atomic ledger row. No target Work becomes visible without the
    /// corresponding Delegation event and projection.
    pub fn create_work_delegation_with_target_work(
        &self,
        mut delegation: WorkDelegation,
        mut target_work: Work,
        context: WorkCommandContext,
    ) -> StoreResult<(WorkDelegation, Work)> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;

        let request_fingerprint =
            work_delegation_request_fingerprint(&delegation, &target_work, &context);

        if let Some(existing) = self
            .all_work_delegation_revisions_unlocked()?
            .into_iter()
            .find(|revision| revision.event.idempotency_key == context.idempotency_key)
        {
            if existing.event.payload.get("request_fingerprint") == Some(&request_fingerprint) {
                let target = self
                    .latest_works_unlocked()?
                    .remove(&existing.delegation.target_work_ref.work_id)
                    .ok_or_else(|| {
                        StoreError::Conflict(
                            "DELEGATION_CORRUPT: idempotent target Work is missing".to_string(),
                        )
                    })?;
                return Ok((existing.delegation, target));
            }
            return Err(StoreError::Conflict(format!(
                "IDEMPOTENCY_CONFLICT: key {} already belongs to Delegation {}",
                context.idempotency_key, existing.delegation.id
            )));
        }

        let source = self.current_work_unlocked(
            &delegation.source_work_ref.work_id,
            delegation.source_work_version,
        )?;
        if source.team_run_id != delegation.source_work_ref.team_run_id {
            return Err(StoreError::Conflict(
                "DELEGATION_STALE_SOURCE: source WorkRef does not match the authoritative Work"
                    .to_string(),
            ));
        }
        let source_owner = source.owner_member_id.clone().ok_or_else(|| {
            StoreError::Conflict(
                "DELEGATION_NOT_AUTHORIZED: source Work has no durable owner".to_string(),
            )
        })?;
        if delegation.source_owner_member_id != source_owner {
            return Err(StoreError::Conflict(
                "DELEGATION_STALE_SOURCE: source owner changed".to_string(),
            ));
        }
        match context.performed_by_actor.kind {
            TeamActorKind::Host | TeamActorKind::Operator | TeamActorKind::Service => {}
            TeamActorKind::ProviderRuntimeProjection => {
                let member = self.require_member_run_unlocked(
                    &context.performed_by_actor.id,
                    &source.team_run_id,
                )?;
                if member_identity(&member) != source_owner {
                    return Err(StoreError::Conflict(
                        "DELEGATION_NOT_AUTHORIZED: only source owner or Host may delegate"
                            .to_string(),
                    ));
                }
                delegation.created_by_member_run_id = Some(member.id);
            }
            TeamActorKind::AgentMember => {
                if context.performed_by_actor.id != source_owner {
                    return Err(StoreError::Conflict(
                        "DELEGATION_NOT_AUTHORIZED: only source owner or Host may delegate"
                            .to_string(),
                    ));
                }
            }
        }

        let target_team = latest_by_id(self.read_jsonl::<AgentTeam>("teams.jsonl")?, |team| {
            team.id.clone()
        })
        .remove(&delegation.target_agent_team_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "DELEGATION_TARGET_INVALID: AgentTeam {} not found",
                delegation.target_agent_team_id
            ))
        })?;
        if target_team.status != firm_core::AgentTeamStatus::Active {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_TARGET_INVALID: AgentTeam {} is {:?}",
                target_team.id, target_team.status
            )));
        }
        let target_run = self.require_team_run_unlocked(&target_work.team_run_id)?;
        if target_run.agent_team_id != target_team.id
            || target_run.execution_node_id != target_team.node_id
            || matches!(
                target_run.status,
                TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
            )
        {
            return Err(StoreError::Conflict(
                "DELEGATION_TARGET_INVALID: target TeamRun is not an active run of target Team"
                    .to_string(),
            ));
        }
        let source_team_id = source.team_id.clone().ok_or_else(|| {
            StoreError::Conflict(
                "DELEGATION_STALE_SOURCE: source Work has no AgentTeam provenance".to_string(),
            )
        })?;
        if source_team_id == target_team.id {
            return Err(StoreError::Conflict(
                "DELEGATION_TARGET_INVALID: cross-Team Delegation requires a different target Team"
                    .to_string(),
            ));
        }
        let latest_works = self.latest_works_unlocked()?;
        if latest_works.contains_key(target_work.id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "work already exists: {}",
                target_work.id
            )));
        }
        let target_ref = WorkRef {
            team_run_id: target_work.team_run_id.clone(),
            work_id: target_work.id.clone(),
        };
        // Delegation always creates a fresh target Work, so WorkRef-level cycle
        // detection would be vacuous. The meaningful graph is Team -> Team:
        // reject A -> B when a non-cancelled B -> ... -> A path already exists.
        let delegations = self.latest_work_delegations_unlocked()?;
        let mut outgoing = std::collections::BTreeMap::<String, Vec<String>>::new();
        for existing in delegations
            .values()
            .filter(|candidate| candidate.state != WorkDelegationState::Cancelled)
        {
            let existing_source = latest_works
                .get(&existing.source_work_ref.work_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "DELEGATION_CORRUPT: source Work {} is missing",
                        existing.source_work_ref.work_id
                    ))
                })?;
            let existing_source_team = existing_source.team_id.as_ref().ok_or_else(|| {
                StoreError::Conflict(format!(
                    "DELEGATION_CORRUPT: source Work {} has no AgentTeam provenance",
                    existing_source.id
                ))
            })?;
            outgoing
                .entry(existing_source_team.clone())
                .or_default()
                .push(existing.target_agent_team_id.clone());
        }
        let mut pending = vec![target_team.id.clone()];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(cursor) = pending.pop() {
            if !visited.insert(cursor.clone()) {
                continue;
            }
            if cursor == source_team_id {
                return Err(StoreError::Conflict(
                    "DELEGATION_CYCLE: cross-Team delegation graph must be acyclic".to_string(),
                ));
            }
            if let Some(next) = outgoing.get(&cursor) {
                pending.extend(next.iter().cloned());
            }
        }

        target_work.team_id = Some(target_team.id.clone());
        target_work.parent_work_id = None;
        target_work.phase = WorkPhase::Open;
        target_work.condition = WorkCondition::Normal;
        target_work.resolution = None;
        target_work.version = 1;
        target_work.created_at = context.created_at.clone();
        target_work.updated_at = context.created_at.clone();
        target_work.created_by_actor = context.performed_by_actor.clone();
        target_work
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_WORK_PROJECTION: {error}")))?;
        self.validate_work_relations_unlocked(&target_work)?;

        delegation.target_work_ref = target_ref;
        delegation.delegated_by_actor = context.performed_by_actor.clone();
        delegation.state = WorkDelegationState::Active;
        delegation.resolution_summary = None;
        delegation.blocker_reason = None;
        delegation.version = 1;
        delegation.created_at = context.created_at.clone();
        delegation.updated_at = context.created_at.clone();
        delegation
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION: {error}")))?;
        if self
            .latest_work_delegations_unlocked()?
            .contains_key(&delegation.id)
        {
            return Err(StoreError::Conflict(format!(
                "work delegation already exists: {}",
                delegation.id
            )));
        }

        let target_event_id = format!("{}:target-work", context.event_id);
        self.ensure_work_event_id_available_unlocked(&target_event_id)?;
        let target_work_operation = WorkOperation {
            event: WorkEvent {
                id: target_event_id,
                team_run_id: target_work.team_run_id.clone(),
                work_id: target_work.id.clone(),
                sequence: 1,
                kind: WorkEventKind::Created,
                expected_version: 0,
                resulting_version: 1,
                performed_by_actor: context.performed_by_actor.clone(),
                authority_actor: context.authority_actor.clone(),
                causation_ref: Some(firm_core::WorkCausationRef {
                    kind: "work_delegation".to_string(),
                    id: delegation.id.clone(),
                }),
                idempotency_key: format!("{}:target-work", context.idempotency_key),
                payload: serde_json::json!({
                    "delegation_id": delegation.id,
                    "source_work_ref": delegation.source_work_ref,
                }),
                created_at: context.created_at.clone(),
            },
            work: target_work.clone(),
            condition_records: Vec::new(),
            reports: Vec::new(),
            evidence_records: Vec::new(),
            decisions: Vec::new(),
            deliveries: self.initial_work_deliveries_unlocked(
                &target_work,
                &format!("{}:target-work", context.event_id),
                &context.created_at,
            )?,
            delivery_updates: Vec::new(),
            delegation_revisions: Vec::new(),
        };
        let event = WorkDelegationEvent {
            id: context.event_id,
            delegation_id: delegation.id.clone(),
            sequence: 1,
            transition: WorkDelegationTransition::Created,
            expected_version: 0,
            resulting_version: 1,
            performed_by_actor: context.performed_by_actor,
            causation_ref: context.causation_ref,
            idempotency_key: context.idempotency_key,
            payload: serde_json::json!({"request_fingerprint": request_fingerprint}),
            created_at: context.created_at,
        };
        event
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION_EVENT: {error}")))?;
        self.append_jsonl_unlocked(
            "work_delegation_operations.jsonl",
            &WorkDelegationOperation {
                delegation: delegation.clone(),
                event,
                target_work_operation,
            },
        )?;
        Ok((delegation, target_work))
    }

    pub fn assign_work(
        &self,
        work_id: &str,
        expected_version: u64,
        owner_member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Assigned,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.is_terminal()
            || current.phase != WorkPhase::Open
            || current.condition != WorkCondition::Normal
            || current.owner_member_id.is_some()
            || current.active_member_run_id.is_some()
        {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must be open to assign"
            )));
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let member = self.require_member_run_unlocked(owner_member_run_id, &current.team_run_id)?;
        self.ensure_member_can_receive_work_unlocked(&member)?;
        let owner_id = member_identity(&member);
        let mut next = current.clone();
        next.owner_member_id = Some(owner_id);
        next.active_member_run_id = Some(member.id.clone());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Assigned, context)
    }

    /// Rebind non-terminal Work to a replacement runtime generation of the
    /// same stable member identity. This is the sole safe Host primitive after
    /// a runtime dies: the version bump fences the old runtime, the Rebound
    /// event records both bindings, and a fresh ProviderWorkDispatch targets the new
    /// ProviderRuntimeProjection.
    ///
    /// A still-claimed delivery is an uncertain handoff and must first be
    /// completed, failed by its current lease owner, or reconciled by a
    /// successor. Provider-received/acknowledged deliveries remain immutable
    /// evidence and do not prevent a new-version rebind.
    pub fn rebind_work(
        &self,
        work_id: &str,
        expected_version: u64,
        new_member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Rebound,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is terminal and cannot be rebound"
            )));
        }
        let old_member_run_id = current.active_member_run_id.clone().ok_or_else(|| {
            StoreError::Conflict(format!("work {work_id} has no runtime binding to replace"))
        })?;
        let owner_member_id = current.owner_member_id.clone().ok_or_else(|| {
            StoreError::Conflict(format!("work {work_id} has no stable owner identity"))
        })?;
        let (previous, replacement) = if old_member_run_id == new_member_run_id {
            let revisions = self
                .read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?
                .into_iter()
                .filter(|member| {
                    member.id == old_member_run_id && member.team_run_id == current.team_run_id
                })
                .collect::<Vec<_>>();
            let replacement = revisions.last().cloned().ok_or_else(|| {
                StoreError::Conflict(format!("member run not found: {new_member_run_id}"))
            })?;
            if compare_store_timestamps(&replacement.started_at, &current.updated_at)
                != std::cmp::Ordering::Greater
            {
                return Err(StoreError::Conflict(format!(
                    "WORK_ALREADY_BOUND: ProviderRuntimeProjection {new_member_run_id} generation {} does not postdate Work version {}",
                    replacement.runtime_generation, current.version
                )));
            }
            let previous = revisions
                .iter()
                .rev()
                .skip(1)
                .find(|member| member.runtime_generation < replacement.runtime_generation)
                .cloned()
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "WORK_ALREADY_BOUND: ProviderRuntimeProjection {new_member_run_id} has no higher replacement runtime generation"
                    ))
                })?;
            (previous, replacement)
        } else {
            (
                self.require_member_run_unlocked(&old_member_run_id, &current.team_run_id)?,
                self.require_member_run_unlocked(new_member_run_id, &current.team_run_id)?,
            )
        };
        if previous.coordination_is_active()
            && !matches!(
                previous.status,
                firm_core::MemberRunStatus::Completed
                    | firm_core::MemberRunStatus::Failed
                    | firm_core::MemberRunStatus::Stopped
            )
        {
            return Err(StoreError::Conflict(format!(
                "OLD_RUNTIME_ACTIVE: ProviderRuntimeProjection {old_member_run_id} must be closed or terminal before Work rebind"
            )));
        }
        if self
            .latest_work_deliveries_unlocked()?
            .values()
            .any(|delivery| {
                delivery.work_id == work_id
                    && delivery.status == ProviderWorkDispatchStatus::Claimed
            })
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work has a claimed delivery".to_string(),
            ));
        }
        self.ensure_member_can_receive_work_unlocked(&replacement)?;
        let replacement_identity = member_identity(&replacement);
        if replacement_identity != owner_member_id {
            return Err(StoreError::Conflict(format!(
                "OWNER_MISMATCH: replacement ProviderRuntimeProjection {new_member_run_id} belongs to {replacement_identity}, expected {owner_member_id}"
            )));
        }

        let mut next = current.clone();
        next.active_member_run_id = Some(replacement.id.clone());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Rebound,
            context,
            serde_json::json!({
                "previous_member_run_id": old_member_run_id,
                "replacement_member_run_id": new_member_run_id,
                "previous_runtime_generation": previous.runtime_generation,
                "replacement_runtime_generation": replacement.runtime_generation,
                "owner_member_id": owner_member_id,
            }),
        )
    }

    /// Append an explicit full-projection repair after a stale mixed-version
    /// writer omitted immutable additive provenance. Raw sparse operations
    /// remain untouched; the recovered reducer state becomes a new `Updated`
    /// WorkOperation at the next version without changing lifecycle, owner, or
    /// runtime binding.
    pub fn reconcile_work_projection_provenance(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Updated,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let raw_current = latest_by_id(self.work_operations_unlocked()?, |operation| {
            operation.work.id.clone()
        })
        .remove(work_id)
        .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        if raw_current.work.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "VERSION_CONFLICT: work {work_id} is at version {}, expected {expected_version}",
                raw_current.work.version
            )));
        }
        let current = self.current_work_unlocked(work_id, expected_version)?;
        let mut recovered_fields = Vec::new();
        if raw_current.work.team_id.is_none() && current.team_id.is_some() {
            recovered_fields.push("team_id");
        }
        if raw_current.work.created_by_member_id.is_none() && current.created_by_member_id.is_some()
        {
            recovered_fields.push("created_by_member_id");
        }
        if recovered_fields.is_empty() {
            return Err(StoreError::Conflict(format!(
                "WORK_PROJECTION_PROVENANCE_CURRENT: Work {work_id} has no recoverable sparse provenance"
            )));
        }

        let mut next = current.clone();
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Updated,
            context,
            serde_json::json!({
                "reason": "mixed_version_projection_recovery",
                "recovered_fields": recovered_fields,
                "source_event_id": raw_current.event.id,
            }),
        )
    }

    /// Move a persistent Work onto a successor execution attempt of the same
    /// AgentTeam. Stable ownership, creator provenance, and
    /// Work identity remain unchanged; only the execution binding moves.
    pub fn retarget_work_execution(
        &self,
        work_id: &str,
        expected_version: u64,
        successor_team_run_id: &str,
        successor_member_run_id: Option<&str>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::ExecutionRetargeted,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is terminal and cannot be retargeted"
            )));
        }
        self.reconcile_work_host_attentions_unlocked()?;
        if self
            .latest_host_attentions_unlocked()?
            .values()
            .any(|attention| {
                attention.work_id == current.id
                    && attention.team_run_id == current.team_run_id
                    && attention.needs_host_action()
            })
        {
            return Err(StoreError::Conflict(format!(
                "HOST_ATTENTION_PENDING: Work {work_id} has unresolved attention owned by TeamRun {}; the exact Host must ACK intake before execution retarget",
                current.team_run_id
            )));
        }
        let team_id = current.team_id.clone().ok_or_else(|| {
            StoreError::Conflict(format!(
                "WORK_NOT_TEAM_SCOPED: promote Work {work_id} before retargeting execution"
            ))
        })?;
        if current.team_run_id == successor_team_run_id {
            return Err(StoreError::Conflict(format!(
                "Work {work_id} already targets TeamRun {successor_team_run_id}"
            )));
        }
        let successor = self.require_team_run_unlocked(successor_team_run_id)?;
        if matches!(
            successor.status,
            TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "successor TeamRun {} is {:?} and cannot execute Work",
                successor.id, successor.status
            )));
        }
        if durable_team_id(&successor) != Some(team_id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "TEAM_SCOPE_MISMATCH: successor TeamRun {} does not belong to AgentTeam {team_id}",
                successor.id
            )));
        }
        if let Some(previous_member_run_id) = current.active_member_run_id.as_deref() {
            let previous =
                self.require_member_run_unlocked(previous_member_run_id, &current.team_run_id)?;
            if previous.coordination_is_active()
                && !matches!(
                    previous.status,
                    firm_core::MemberRunStatus::Completed
                        | firm_core::MemberRunStatus::Failed
                        | firm_core::MemberRunStatus::Stopped
                )
            {
                return Err(StoreError::Conflict(format!(
                    "OLD_RUNTIME_ACTIVE: ProviderRuntimeProjection {previous_member_run_id} must be closed or terminal before execution retarget"
                )));
            }
        }
        if self
            .latest_work_deliveries_unlocked()?
            .values()
            .any(|delivery| {
                delivery.work_id == work_id
                    && delivery.status == ProviderWorkDispatchStatus::Claimed
            })
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work has a claimed delivery".to_string(),
            ));
        }

        let new_binding = match (current.owner_member_id.as_deref(), successor_member_run_id) {
            (None, None) => None,
            (None, Some(_)) => {
                return Err(StoreError::Conflict(
                    "unassigned Work cannot gain an execution binding during retarget".to_string(),
                ));
            }
            (Some(_), None) => {
                return Err(StoreError::Conflict(
                    "owned Work requires --successor-member-run-id during retarget".to_string(),
                ));
            }
            (Some(owner_id), Some(member_run_id)) => {
                let member =
                    self.require_member_run_unlocked(member_run_id, successor_team_run_id)?;
                self.ensure_member_can_receive_work_unlocked(&member)?;
                let successor_identity = member_identity(&member);
                if successor_identity != owner_id {
                    return Err(StoreError::Conflict(format!(
                        "OWNER_MISMATCH: successor ProviderRuntimeProjection {member_run_id} belongs to {successor_identity}, expected {owner_id}"
                    )));
                }
                Some(member.id)
            }
        };

        let previous_team_run_id = current.team_run_id.clone();
        let previous_member_run_id = current.active_member_run_id.clone();
        let mut next = current.clone();
        next.team_run_id = successor_team_run_id.to_string();
        next.active_member_run_id = new_binding.clone();
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::ExecutionRetargeted,
            context,
            serde_json::json!({
                "team_id": team_id,
                "previous_team_run_id": previous_team_run_id,
                "successor_team_run_id": successor_team_run_id,
                "previous_member_run_id": previous_member_run_id,
                "successor_member_run_id": new_binding,
            }),
        )
    }

    pub fn claim_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Claimed,
        )? {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.phase != WorkPhase::Open
            || current.condition != WorkCondition::Normal
            || current.owner_member_id.is_some()
            || current.claim_mode != WorkClaimMode::TeamClaim
        {
            return Err(StoreError::Conflict(format!(
                "CLAIM_LOST: work {work_id} is not an unowned team-claim Work"
            )));
        }
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if !matches!(
            member.status,
            firm_core::MemberRunStatus::Idle | firm_core::MemberRunStatus::Running
        ) || !member.coordination_is_active()
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: ProviderRuntimeProjection {member_run_id} is not available and active"
            )));
        }
        let owner_id = member_identity(&member);
        if !current.eligible_member_ids.is_empty()
            && !current.eligible_member_ids.iter().any(|id| id == &owner_id)
        {
            return Err(StoreError::Conflict(format!(
                "member {owner_id} is not eligible to claim work {work_id}"
            )));
        }
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        if !current.is_claim_ready(works.iter()) {
            return Err(StoreError::Conflict(format!("work {work_id} is not ready")));
        }
        if works.iter().any(|work| {
            work.team_run_id == current.team_run_id
                && work.phase == WorkPhase::Active
                && work.condition == WorkCondition::Normal
                && work.active_member_run_id.as_deref() == Some(member_run_id)
        }) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: ProviderRuntimeProjection {member_run_id} already has active Work"
            )));
        }
        let mut next = current.clone();
        next.owner_member_id = Some(owner_id);
        next.active_member_run_id = Some(member.id.clone());
        next.phase = WorkPhase::Active;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Claimed, context)
    }

    pub fn start_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Started,
        )? {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.phase != WorkPhase::Open
            || current.condition != WorkCondition::Normal
            || current.active_member_run_id.as_deref() != Some(member_run_id)
        {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} does not own open work {work_id}"
            )));
        }
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if !matches!(
            member.status,
            firm_core::MemberRunStatus::Idle | firm_core::MemberRunStatus::Running
        ) || !member.coordination_is_active()
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: ProviderRuntimeProjection {member_run_id} is not available and active"
            )));
        }
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        if !current.is_claim_ready(works.iter()) {
            return Err(StoreError::Conflict(format!("work {work_id} is not ready")));
        }
        if works.iter().any(|work| {
            work.team_run_id == current.team_run_id
                && work.phase == WorkPhase::Active
                && work.condition == WorkCondition::Normal
                && work.active_member_run_id.as_deref() == Some(member_run_id)
        }) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: ProviderRuntimeProjection {member_run_id} already has active Work"
            )));
        }
        let mut next = current.clone();
        next.phase = WorkPhase::Active;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Started, context)
    }

    pub fn block_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict("BLOCKER_REASON_REQUIRED".to_string()));
        }
        let condition_record = WorkConditionRecord {
            id: format!("work-condition-{}", context.event_id),
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            condition: WorkCondition::Blocked,
            owner_actor: context.performed_by_actor.clone(),
            impact: reason.to_string(),
            resume_condition: "blocker is resolved and evidence is recorded".to_string(),
            next_check_at: None,
            evidence_refs: Vec::new(),
            created_at: context.created_at.clone(),
            resolved_at: None,
            supersedes_condition_record_id: None,
        };
        self.transition_owned_work_with_payload(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Blocked,
            (WorkPhase::Active, WorkCondition::Normal),
            (WorkPhase::Active, WorkCondition::Blocked),
            serde_json::Value::Null,
            vec![condition_record],
            Vec::new(),
            |work| work.blocker_reason = Some(reason.to_string()),
        )
    }

    pub fn block_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict("BLOCKER_REASON_REQUIRED".to_string()));
        }
        let condition_record = WorkConditionRecord {
            id: format!("work-condition-{}", context.event_id),
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            condition: WorkCondition::Blocked,
            owner_actor: context.performed_by_actor.clone(),
            impact: reason.to_string(),
            resume_condition: "blocker is resolved and evidence is recorded".to_string(),
            next_check_at: None,
            evidence_refs: Vec::new(),
            created_at: context.created_at.clone(),
            resolved_at: None,
            supersedes_condition_record_id: None,
        };
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Blocked,
            (WorkPhase::Active, WorkCondition::Normal),
            (WorkPhase::Active, WorkCondition::Blocked),
            serde_json::Value::Null,
            vec![condition_record],
            Vec::new(),
            |work| work.blocker_reason = Some(reason.to_string()),
        )
    }

    pub fn resume_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        resolution: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if resolution.trim().is_empty() {
            return Err(StoreError::Conflict(
                "blocker resolution is required".to_string(),
            ));
        }
        let resolved_record =
            self.resolved_work_condition_record(work_id, expected_version, resolution, &context)?;
        self.transition_owned_work_with_payload(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Resumed,
            (WorkPhase::Active, WorkCondition::Blocked),
            (WorkPhase::Active, WorkCondition::Normal),
            serde_json::json!({ "resolution": resolution }),
            vec![resolved_record],
            Vec::new(),
            |work| work.blocker_reason = None,
        )
    }

    pub fn resume_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        resolution: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if resolution.trim().is_empty() {
            return Err(StoreError::Conflict(
                "blocker resolution is required".to_string(),
            ));
        }
        let resolved_record =
            self.resolved_work_condition_record(work_id, expected_version, resolution, &context)?;
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Resumed,
            (WorkPhase::Active, WorkCondition::Blocked),
            (WorkPhase::Active, WorkCondition::Normal),
            serde_json::json!({ "resolution": resolution }),
            vec![resolved_record],
            Vec::new(),
            |work| work.blocker_reason = None,
        )
    }

    fn resolved_work_condition_record(
        &self,
        work_id: &str,
        expected_version: u64,
        resolution: &str,
        context: &WorkCommandContext,
    ) -> StoreResult<WorkConditionRecord> {
        let active = self
            .work_condition_records()?
            .into_iter()
            .rev()
            .find(|record| {
                record.work_id == work_id
                    && record.condition == WorkCondition::Blocked
                    && record.resolved_at.is_none()
                    && record.work_version <= expected_version
            })
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "ACTIVE_WORK_CONDITION_REQUIRED: Work {work_id} has no unresolved blocker record"
                ))
            })?;
        Ok(WorkConditionRecord {
            id: format!("work-condition-resolution-{}", context.event_id),
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            condition: active.condition,
            owner_actor: context.performed_by_actor.clone(),
            impact: active.impact,
            resume_condition: resolution.to_string(),
            next_check_at: None,
            evidence_refs: active.evidence_refs,
            created_at: context.created_at.clone(),
            resolved_at: Some(context.created_at.clone()),
            supersedes_condition_record_id: Some(active.id),
        })
    }

    pub fn release_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.release_work_with_authority(work_id, expected_version, Some(member_run_id), context)
    }

    pub fn release_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.release_work_with_authority(work_id, expected_version, None, context)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        result_summary: &str,
        artifact_refs: Vec<String>,
        check_refs: Vec<String>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.submit_work_with_links(
            work_id,
            expected_version,
            member_run_id,
            result_summary,
            artifact_refs,
            check_refs,
            Vec::new(),
            context,
        )
    }

    /// [`submit_work`] plus an explicit GitHub issue/PR linkage snapshot
    /// (issue #369). The base method keeps its historical signature; links are
    /// merged into any links already attached at create time.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_work_with_links(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        result_summary: &str,
        artifact_refs: Vec<String>,
        check_refs: Vec<String>,
        github_links: Vec<GitHubLink>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.submit_work_with_revision_and_links(
            work_id,
            expected_version,
            member_run_id,
            result_summary,
            artifact_refs,
            check_refs,
            github_links,
            None,
            None,
            context,
        )
    }

    /// Submit one immutable candidate. `candidate_revision` is the preferred
    /// source revision for code delivery; when omitted the Store derives a
    /// deterministic digest from the complete submitted payload.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_work_with_revision_and_links(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        result_summary: &str,
        artifact_refs: Vec<String>,
        check_refs: Vec<String>,
        github_links: Vec<GitHubLink>,
        base_revision: Option<String>,
        candidate_revision: Option<String>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if result_summary.trim().is_empty() {
            return Err(StoreError::Conflict("RESULT_REQUIRED".to_string()));
        }
        let candidate_revision = candidate_revision
            .filter(|revision| !revision.trim().is_empty())
            .unwrap_or_else(|| {
                canonical_work_candidate_revision(
                    result_summary,
                    &artifact_refs,
                    &check_refs,
                    &github_links,
                )
            });
        if base_revision
            .as_deref()
            .is_some_and(|revision| revision.trim().is_empty())
        {
            return Err(StoreError::Conflict(
                "base revision must not be empty".to_string(),
            ));
        }
        let report_id = format!("work-report-{}", context.event_id);
        let evidence_id = format!("work-evidence-{}", context.event_id);
        let report = WorkReport {
            id: report_id,
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            report_revision: 1,
            submitted_by_actor: context.performed_by_actor.clone(),
            base_revision,
            candidate_revision,
            result_summary: result_summary.to_string(),
            artifact_refs: artifact_refs.clone(),
            check_refs: check_refs.clone(),
            evidence_refs: vec![evidence_id],
            known_risks: Vec::new(),
            created_at: context.created_at.clone(),
        };
        self.transition_owned_work_with_payload(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Submitted,
            (WorkPhase::Active, WorkCondition::Normal),
            (WorkPhase::Review, WorkCondition::Normal),
            serde_json::Value::Null,
            Vec::new(),
            vec![report],
            |work| {
                work.result_summary = Some(result_summary.to_string());
                work.artifact_refs = artifact_refs;
                work.check_refs = check_refs;
                // Issue links describe durable provenance. Pull-request links
                // describe this submission candidate and are replaced, so a
                // prior merged PR cannot satisfy a resubmitted candidate.
                let mut candidate_links = work
                    .github_links
                    .iter()
                    .filter(|link| link.kind == firm_core::GitHubLinkKind::Issue)
                    .cloned()
                    .collect::<Vec<_>>();
                for link in github_links {
                    if !candidate_links.contains(&link) {
                        candidate_links.push(link);
                    }
                }
                work.github_links = candidate_links;
                work.blocker_reason = None;
            },
        )
    }

    /// Refresh the GitHub linkage snapshot on a Work without touching its
    /// lifecycle (issue #369 Phase 2, daemon CI poll). Host/Service actor
    /// only. When the links are unchanged the current Work is returned without
    /// appending a `Updated` operation, so a steady-state poll never churns
    /// versions.
    pub fn update_work_github_links(
        &self,
        work_id: &str,
        expected_version: u64,
        github_links: Vec<GitHubLink>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Updated,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.github_links == github_links {
            return Ok(current);
        }
        let mut next = current.clone();
        next.github_links = github_links;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        let reports = if current.phase == WorkPhase::Review {
            let previous = self
                .work_operations_unlocked()?
                .into_iter()
                .flat_map(|operation| operation.reports)
                .filter(|report| {
                    report.work_id == current.id && report.work_version == current.version
                })
                .max_by_key(|report| report.report_revision)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "CURRENT_WORK_REPORT_REQUIRED: Work {work_id} version {} cannot refresh review evidence",
                        current.version
                    ))
                })?;
            vec![WorkReport {
                id: format!("work-report-{}", context.event_id),
                work_id: previous.work_id,
                work_version: next.version,
                report_revision: previous.report_revision.saturating_add(1),
                submitted_by_actor: previous.submitted_by_actor,
                base_revision: previous.base_revision,
                candidate_revision: previous.candidate_revision,
                result_summary: previous.result_summary,
                artifact_refs: previous.artifact_refs,
                check_refs: previous.check_refs,
                evidence_refs: vec![format!("work-evidence-{}", context.event_id)],
                known_risks: previous.known_risks,
                created_at: context.created_at.clone(),
            }]
        } else {
            Vec::new()
        };
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            WorkEventKind::Updated,
            context,
            serde_json::json!({ "reason": "github_ci_poll" }),
            Vec::new(),
            reports,
            Vec::new(),
        )
    }

    /// Host-side auto-submit when the daemon observes a linked pull request
    /// reach `MERGED` (issue #369 Phase 2). The Work must be `in_progress` and
    /// carry a `pull_request` link with `status == "MERGED"`; the fresh link
    /// snapshot is stored with the transition. Host acceptance still moves the
    /// Work from `review` to `done`; this only automates the submission step.
    pub fn submit_work_on_pr_merge(
        &self,
        work_id: &str,
        expected_version: u64,
        result_summary: &str,
        github_links: Vec<GitHubLink>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if result_summary.trim().is_empty() {
            return Err(StoreError::Conflict("RESULT_REQUIRED".to_string()));
        }
        if !github_links.iter().any(|link| {
            link.kind == firm_core::GitHubLinkKind::PullRequest
                && link.status.as_deref() == Some("MERGED")
        }) {
            return Err(StoreError::Conflict(
                "PR_MERGE_REQUIRED: auto-submit requires a pull_request link with status MERGED"
                    .to_string(),
            ));
        }
        let report_id = format!("work-report-{}", context.event_id);
        let evidence_id = format!("work-evidence-{}", context.event_id);
        let candidate_revision =
            canonical_work_candidate_revision(result_summary, &[], &[], &github_links);
        let report = WorkReport {
            id: report_id,
            work_id: work_id.to_string(),
            work_version: expected_version.saturating_add(1),
            report_revision: 1,
            submitted_by_actor: context.performed_by_actor.clone(),
            base_revision: None,
            candidate_revision,
            result_summary: result_summary.to_string(),
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            evidence_refs: vec![evidence_id],
            known_risks: Vec::new(),
            created_at: context.created_at.clone(),
        };
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Submitted,
            (WorkPhase::Active, WorkCondition::Normal),
            (WorkPhase::Review, WorkCondition::Normal),
            serde_json::json!({ "reason": "github_pr_merge_observed" }),
            Vec::new(),
            vec![report],
            |work| {
                work.result_summary = Some(result_summary.to_string());
                // The fresh observed PR snapshot replaces the prior candidate;
                // durable issue provenance is carried forward.
                let mut merged = work
                    .github_links
                    .iter()
                    .filter(|link| link.kind == firm_core::GitHubLinkKind::Issue)
                    .cloned()
                    .collect::<Vec<_>>();
                for link in github_links {
                    if !merged.contains(&link) {
                        merged.push(link);
                    }
                }
                work.github_links = merged;
                work.blocker_reason = None;
            },
        )
    }

    pub fn accept_work(
        &self,
        _work_id: &str,
        _expected_version: u64,
        _context: WorkCommandContext,
    ) -> StoreResult<Work> {
        Err(StoreError::Conflict(
            "LEGACY_WORK_ACCEPT_RETIRED: use the authenticated team-scoped member-trust Work acceptance command"
                .to_string(),
        ))
    }

    pub fn accept_work_with_summary(
        &self,
        _work_id: &str,
        _expected_version: u64,
        _summary: Option<&str>,
        _context: WorkCommandContext,
    ) -> StoreResult<Work> {
        Err(StoreError::Conflict(
            "LEGACY_WORK_ACCEPT_RETIRED: use the authenticated team-scoped member-trust Work acceptance command"
                .to_string(),
        ))
    }
    pub fn request_work_changes(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "changes-requested reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::ChangesRequested,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.phase != WorkPhase::Review || current.condition != WorkCondition::Normal {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must await Host acceptance"
            )));
        }
        let mut next = current.clone();
        next.phase = WorkPhase::Active;
        next.condition = WorkCondition::Normal;
        next.resolution = None;
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(
            current,
            next,
            WorkEventKind::ChangesRequested,
            context,
        )
    }

    pub fn cancel_work(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "cancellation reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Cancelled,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already terminal"
            )));
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let mut next = current.clone();
        next.phase = WorkPhase::Closed;
        next.condition = WorkCondition::Normal;
        next.resolution = Some(WorkResolution::Cancelled);
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Cancelled, context)
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_owned_work_with_payload(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
        kind: WorkEventKind,
        required_lifecycle: (WorkPhase, WorkCondition),
        resulting_lifecycle: (WorkPhase, WorkCondition),
        payload: serde_json::Value,
        condition_records: Vec<WorkConditionRecord>,
        reports: Vec<WorkReport>,
        mutate: impl FnOnce(&mut Work),
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) =
            self.idempotent_work_operation_unlocked(&context.idempotency_key, work_id, kind)?
        {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if (current.phase, current.condition) != required_lifecycle
            || current.active_member_run_id.as_deref() != Some(member_run_id)
        {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} does not own active work {work_id} in required state"
            )));
        }
        // A Closed or Retired ProviderRuntimeProjection no longer mutates its owned Work:
        // unfinished Work moves only via Host reassign/cancel or after an
        // explicit Reopen (docs/product/agent-team-works.md). This aligns
        // member-side transitions with insert/claim/start/receive, which
        // already require active coordination.
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if !member.coordination_is_active() {
            return Err(StoreError::Conflict(format!(
                "MEMBER_UNAVAILABLE: ProviderRuntimeProjection {member_run_id} coordination is {:?}; Reopen before mutating owned Work",
                member.coordination_status
            )));
        }
        let mut next = current.clone();
        mutate(&mut next);
        next.phase = resulting_lifecycle.0;
        next.condition = resulting_lifecycle.1;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            kind,
            context,
            payload,
            condition_records,
            reports,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
        kind: WorkEventKind,
        required_lifecycle: (WorkPhase, WorkCondition),
        resulting_lifecycle: (WorkPhase, WorkCondition),
        payload: serde_json::Value,
        condition_records: Vec<WorkConditionRecord>,
        reports: Vec<WorkReport>,
        mutate: impl FnOnce(&mut Work),
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) =
            self.idempotent_work_operation_unlocked(&context.idempotency_key, work_id, kind)?
        {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if (current.phase, current.condition) != required_lifecycle {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is not in required state"
            )));
        }
        if current.active_member_run_id.is_none() || current.owner_member_id.is_none() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} has no owner to retain"
            )));
        }
        let mut next = current.clone();
        mutate(&mut next);
        next.phase = resulting_lifecycle.0;
        next.condition = resulting_lifecycle.1;
        next.resolution = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            kind,
            context,
            payload,
            condition_records,
            reports,
            Vec::new(),
        )
    }

    fn release_work_with_authority(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: Option<&str>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Released,
        )? {
            return Ok(existing.work);
        }
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.phase != WorkPhase::Open || current.condition != WorkCondition::Normal {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must be open to release"
            )));
        }
        if current.active_member_run_id.is_none() || current.owner_member_id.is_none() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already unassigned"
            )));
        }
        match member_run_id {
            Some(member_run_id) => {
                require_member_actor(&context.performed_by_actor, member_run_id)?;
                if current.active_member_run_id.as_deref() != Some(member_run_id) {
                    return Err(StoreError::Conflict(format!(
                        "ProviderRuntimeProjection {member_run_id} does not own open work {work_id}"
                    )));
                }
            }
            None => require_host_actor(&context.performed_by_actor)?,
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let mut next = current.clone();
        next.owner_member_id = None;
        next.active_member_run_id = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Released, context)
    }

    fn append_work_transition_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            kind,
            context,
            serde_json::Value::Null,
        )
    }

    fn append_work_transition_with_payload_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
        payload: serde_json::Value,
    ) -> StoreResult<Work> {
        self.append_work_transition_with_records_unlocked(
            current,
            next,
            kind,
            context,
            payload,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn append_work_transition_with_records_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
        payload: serde_json::Value,
        condition_records: Vec<WorkConditionRecord>,
        reports: Vec<WorkReport>,
        decisions: Vec<WorkOperationalDecision>,
    ) -> StoreResult<Work> {
        self.ensure_work_event_id_available_unlocked(&context.event_id)?;
        let sequence = self
            .work_operations_unlocked()?
            .iter()
            .filter(|operation| operation.work.id == current.id)
            .count() as u64
            + 1;
        let prereq_event_id = context.event_id.clone();
        let prereq_created_at = context.created_at.clone();
        let deliveries = if matches!(
            kind,
            WorkEventKind::Assigned
                | WorkEventKind::ChangesRequested
                | WorkEventKind::Resumed
                | WorkEventKind::Rebound
                | WorkEventKind::ExecutionRetargeted
                | WorkEventKind::Accepted
                | WorkEventKind::Cancelled
        ) {
            self.initial_work_deliveries_unlocked(&next, &context.event_id, &context.created_at)?
        } else {
            Vec::new()
        };
        let mut next_delivery_update_sequence =
            self.next_work_delivery_update_sequence_unlocked()?;
        let delivery_updates = self
            .latest_work_deliveries_unlocked()?
            .into_values()
            .filter(|delivery| {
                delivery.work_id == current.id
                    && delivery.status == ProviderWorkDispatchStatus::Queued
                    && delivery.work_version < next.version
            })
            .map(|delivery| {
                let update_sequence = next_delivery_update_sequence;
                next_delivery_update_sequence = next_delivery_update_sequence.saturating_add(1);
                ProviderWorkDispatchUpdate {
                    delivery_id: delivery.id,
                    update_sequence,
                    status: ProviderWorkDispatchStatus::Invalidated,
                    attempt: delivery.attempt,
                    claim_id: delivery.claim_id,
                    claimed_by_supervisor_id: delivery.claimed_by_supervisor_id,
                    claimed_generation: delivery.claimed_generation,
                    provider_receipt_id: delivery.provider_receipt_id,
                    failure_reason: delivery.failure_reason,
                    updated_at: context.created_at.clone(),
                }
            })
            .collect();
        let evidence_records = reports
            .iter()
            .map(|report| {
                let evidence_id = report.evidence_refs.first().cloned().ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "WORK_REPORT_EVIDENCE_REQUIRED: report {} has no candidate evidence",
                        report.id
                    ))
                })?;
                Ok(WorkEvidence {
                    id: evidence_id,
                    work_id: report.work_id.clone(),
                    work_report_id: report.id.clone(),
                    work_version: report.work_version,
                    candidate_revision: report.candidate_revision.clone(),
                    source_type: "work_candidate_revision".to_string(),
                    source_ref: report.candidate_revision.clone(),
                    summary: format!(
                        "Exact candidate evidence for immutable WorkReport {}",
                        report.id
                    ),
                    created_at: report.created_at.clone(),
                })
            })
            .collect::<StoreResult<Vec<_>>>()?;
        let delegation_revisions =
            self.work_delegation_rollup_revisions_unlocked(&next, &context)?;
        let operation = WorkOperation {
            event: WorkEvent {
                id: context.event_id,
                team_run_id: next.team_run_id.clone(),
                work_id: next.id.clone(),
                sequence,
                kind,
                expected_version: current.version,
                resulting_version: next.version,
                performed_by_actor: context.performed_by_actor,
                authority_actor: context.authority_actor,
                causation_ref: context.causation_ref,
                idempotency_key: context.idempotency_key,
                payload,
                created_at: context.created_at,
            },
            work: next.clone(),
            condition_records,
            reports,
            evidence_records,
            decisions,
            deliveries,
            delivery_updates,
            delegation_revisions,
        };
        self.append_work_operation_unlocked(&operation)?;
        // When a work is accepted (Done), notify works that depend on it
        // as a prerequisite: create deliveries for their owner members.
        if kind == WorkEventKind::Accepted {
            let team_run_id = &next.team_run_id;
            let prerequisite_id = &next.id;
            let all_works = self.latest_works_unlocked()?;
            for dependent_work in all_works.values() {
                if dependent_work.team_run_id == *team_run_id
                    && dependent_work
                        .prerequisite_work_ids
                        .iter()
                        .any(|pid| pid == prerequisite_id)
                    && !dependent_work.is_terminal()
                {
                    if let Some(owner_member_id) = dependent_work.active_member_run_id.as_deref() {
                        if let Ok(member) =
                            self.require_member_run_unlocked(owner_member_id, team_run_id)
                        {
                            if self
                                .ensure_member_can_receive_work_unlocked(&member)
                                .is_ok()
                            {
                                let dep_delivery = ProviderWorkDispatch {
                                    id: format!(
                                        "work-delivery-prereq-{}-{}",
                                        prereq_event_id, dependent_work.id
                                    ),
                                    work_event_id: prereq_event_id.clone(),
                                    team_run_id: team_run_id.clone(),
                                    work_id: dependent_work.id.clone(),
                                    work_version: dependent_work.version,
                                    recipient_member_run_id: owner_member_id.to_string(),
                                    status: ProviderWorkDispatchStatus::Queued,
                                    attempt: 0,
                                    claim_id: None,
                                    claimed_by_supervisor_id: None,
                                    claimed_generation: None,
                                    provider_receipt_id: None,
                                    failure_reason: None,
                                    updated_at: prereq_created_at.clone(),
                                };
                                self.append_jsonl_unlocked("work_deliveries.jsonl", &dep_delivery)?;
                                // Also ensure HostAttention for prerequisite completion
                                let prereq_attention = HostAttention {
                                    id: format!("host-attention-prereq-{}", dep_delivery.id),
                                    team_run_id: team_run_id.clone(),
                                    kind: HostAttentionKind::WorkPrerequisiteCompleted,
                                    work_id: dependent_work.id.clone(),
                                    work_version: dependent_work.version,
                                    source_event_ref: prereq_event_id.clone(),
                                    member_run_id: Some(owner_member_id.to_string()),
                                    status: HostAttentionStatus::Actionable,
                                    attempt: 0,
                                    claim_id: None,
                                    claimed_host_surface: None,
                                    claimed_host_thread_id: None,
                                    claimed_host_lease_id: None,
                                    claimed_host_lease_generation: None,
                                    claimed_host_lease_owner_id: None,
                                    provider_receipt_id: None,
                                    last_failure_reason: None,
                                    created_at: prereq_created_at.clone(),
                                    updated_at: prereq_created_at.clone(),
                                };
                                prereq_attention
                                    .validate()
                                    .map_err(|error| StoreError::Conflict(error.to_string()))?;
                                self.append_jsonl_unlocked(
                                    "host_attentions.jsonl",
                                    &prereq_attention,
                                )?;
                            }
                        }
                    }
                }
            }
        }
        self.ensure_host_attention_for_work_operation_unlocked(&operation)?;
        Ok(next)
    }

    fn ensure_work_store_compatible_unlocked(&self) -> StoreResult<()> {
        // `assignment` is no longer a ProviderDispatchIntent, so a legacy row fails
        // deserialization before any Work mutation can be accepted. We do not
        // migrate or reinterpret that history: use a fresh Execution Space.
        let _ = self.read_jsonl::<ProviderDispatchEnvelope>("team_messages.jsonl")?;
        Ok(())
    }

    fn require_team_run_unlocked(&self, team_run_id: &str) -> StoreResult<AgentTeamRun> {
        latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(team_run_id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {team_run_id}")))
    }

    fn latest_host_binding_lease_unlocked(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<HostBindingLease>> {
        Ok(latest_by_id(
            self.read_jsonl::<HostBindingLease>("host_binding_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id))
    }

    fn require_same_host_binding_lease_owner(
        &self,
        current: &HostBindingLease,
        expected: &HostBindingLease,
    ) -> StoreResult<()> {
        if current.team_run_id != expected.team_run_id
            || canonical_surface(&current.host_surface) != canonical_surface(&expected.host_surface)
            || current.host_thread_id != expected.host_thread_id
            || current.owner_kind != expected.owner_kind
            || current.owner_id != expected.owner_id
            || current.generation != expected.generation
            || current.lease_id != expected.lease_id
        {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_LEASE_FENCED: stale lease owner/generation/id for TeamRun {}",
                expected.team_run_id
            )));
        }
        Ok(())
    }

    fn require_current_host_binding_lease_owner_unlocked(
        &self,
        expected: &HostBindingLease,
        now_unix_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        let current = self
            .latest_host_binding_lease_unlocked(&expected.team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TeamRun {} has no Host binding lease",
                    expected.team_run_id
                ))
            })?;
        self.require_same_host_binding_lease_owner(&current, expected)?;
        if !current.is_effective_at(now_unix_ms) {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_LEASE_FENCED: lease for TeamRun {} is released or expired",
                expected.team_run_id
            )));
        }
        Ok(current)
    }

    fn require_host_attention_lease_fence_unlocked(
        &self,
        attention: &HostAttention,
        now_unix_ms: u64,
    ) -> StoreResult<HostBindingLease> {
        let current = self
            .latest_host_binding_lease_unlocked(&attention.team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "HOST_ATTENTION_LEASE_FENCED: TeamRun {} has no Host binding lease",
                    attention.team_run_id
                ))
            })?;
        let matches = current.owner_kind == HostBindingLeaseOwnerKind::Dispatcher
            && attention.claimed_host_lease_id.as_deref() == Some(current.lease_id.as_str())
            && attention.claimed_host_lease_generation == Some(current.generation)
            && attention.claimed_host_lease_owner_id.as_deref() == Some(current.owner_id.as_str())
            && attention
                .claimed_host_surface
                .as_deref()
                .is_some_and(|surface| {
                    canonical_surface(surface) == canonical_surface(&current.host_surface)
                })
            && attention.claimed_host_thread_id.as_deref() == Some(current.host_thread_id.as_str())
            && current.is_effective_at(now_unix_ms);
        if !matches {
            return Err(StoreError::Conflict(format!(
                "HOST_ATTENTION_LEASE_FENCED: claim {} no longer owns attention {}",
                attention.claim_id.as_deref().unwrap_or("<missing>"),
                attention.id
            )));
        }
        Ok(current)
    }

    fn requeue_fenced_host_attention_claims_unlocked(
        &self,
        current: &HostBindingLease,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<()> {
        let run = self.require_team_run_unlocked(&current.team_run_id)?;
        let current_is_effective_exact_dispatcher = current.owner_kind
            == HostBindingLeaseOwnerKind::Dispatcher
            && current.is_effective_at(now_unix_ms)
            && canonical_surface(&current.host_surface) == canonical_surface(&run.host_surface)
            && run.host_thread_id.as_deref() == Some(current.host_thread_id.as_str());
        let attentions = self.latest_host_attentions_unlocked()?;
        for mut attention in attentions.into_values().filter(|attention| {
            attention.team_run_id == current.team_run_id
                && attention.status == HostAttentionStatus::Claimed
                && attention.claimed_host_lease_id.is_some()
                && (!current_is_effective_exact_dispatcher
                    || attention.claimed_host_lease_id.as_deref()
                        != Some(current.lease_id.as_str())
                    || attention.claimed_host_lease_generation != Some(current.generation)
                    || attention.claimed_host_lease_owner_id.as_deref()
                        != Some(current.owner_id.as_str())
                    || attention
                        .claimed_host_surface
                        .as_deref()
                        .map(canonical_surface)
                        != Some(canonical_surface(&current.host_surface))
                    || attention.claimed_host_thread_id.as_deref()
                        != Some(current.host_thread_id.as_str()))
        }) {
            attention.status = HostAttentionStatus::Actionable;
            attention.claim_id = None;
            attention.claimed_host_surface = None;
            attention.claimed_host_thread_id = None;
            attention.claimed_host_lease_id = None;
            attention.claimed_host_lease_generation = None;
            attention.claimed_host_lease_owner_id = None;
            attention.provider_receipt_id = None;
            attention.last_failure_reason =
                Some("previous Host binding lease no longer owns this attention".to_string());
            attention.updated_at = updated_at.to_string();
            self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        }
        Ok(())
    }

    fn reconcile_host_binding_stale_attentions_unlocked(
        &self,
        now_unix_ms: u64,
        observed_at: &str,
    ) -> StoreResult<Vec<HostAttention>> {
        let runs = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        });
        let leases = latest_by_id(
            self.read_jsonl::<HostBindingLease>("host_binding_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        );
        let mut projected = self.latest_host_attentions_unlocked()?;
        let mut stale = Vec::new();
        for run in runs.into_values() {
            if matches!(
                run.status,
                TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
            ) {
                continue;
            }
            let Some(thread_id) = run.host_thread_id.as_deref() else {
                continue;
            };
            let lease = leases.get(&run.id);
            let effective = lease.is_some_and(|lease| {
                lease.is_effective_at(now_unix_ms)
                    && canonical_surface(&lease.host_surface)
                        == canonical_surface(&run.host_surface)
                    && lease.host_thread_id == thread_id
            });
            if effective {
                continue;
            }
            let generation = lease.map(|lease| lease.generation).unwrap_or(0);
            let source_event_ref = format!(
                "host-binding-stale:{}:{}:{}:generation:{}",
                run.id, run.host_surface, thread_id, generation
            );
            let attention = HostAttention {
                id: format!("host-attention-{source_event_ref}"),
                team_run_id: run.id,
                kind: HostAttentionKind::HostBindingStale,
                work_id: String::new(),
                work_version: 0,
                source_event_ref,
                member_run_id: None,
                status: HostAttentionStatus::Actionable,
                attempt: 0,
                claim_id: None,
                claimed_host_surface: None,
                claimed_host_thread_id: None,
                claimed_host_lease_id: None,
                claimed_host_lease_generation: None,
                claimed_host_lease_owner_id: None,
                provider_receipt_id: None,
                last_failure_reason: None,
                created_at: observed_at.to_string(),
                updated_at: observed_at.to_string(),
            };
            if let Some(existing) = projected.get(&attention.id) {
                stale.push(existing.clone());
                continue;
            }
            attention
                .validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
            projected.insert(attention.id.clone(), attention.clone());
            stale.push(attention);
        }
        Ok(stale)
    }

    fn ensure_host_attention_unlocked(
        &self,
        attention: &HostAttention,
    ) -> StoreResult<HostAttention> {
        if attention.kind == HostAttentionKind::HostBindingStale {
            return Err(StoreError::Conflict(
                "HostBindingStale attention is derived by lease reconciliation".to_string(),
            ));
        }
        attention
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if attention.status != HostAttentionStatus::Actionable
            || attention.attempt != 0
            || attention.claim_id.is_some()
            || attention.claimed_host_surface.is_some()
            || attention.claimed_host_thread_id.is_some()
            || attention.claimed_host_lease_id.is_some()
            || attention.claimed_host_lease_generation.is_some()
            || attention.claimed_host_lease_owner_id.is_some()
            || attention.provider_receipt_id.is_some()
        {
            return Err(StoreError::Conflict(
                "new HostAttention must be actionable and unclaimed".to_string(),
            ));
        }

        let mut attentions = self.latest_host_attentions_unlocked()?;
        if let Some(existing) = attentions.remove(&attention.id) {
            if Self::same_host_attention_fact(&existing, attention) {
                return Ok(existing);
            }
            return Err(StoreError::Conflict(format!(
                "HostAttention id {} already names a different causal fact",
                attention.id
            )));
        }

        self.require_team_run_unlocked(&attention.team_run_id)?;
        let source_operation = self
            .work_operations_unlocked()?
            .into_iter()
            .find(|operation| operation.event.id == attention.source_event_ref);
        if let Some(operation) = source_operation {
            if operation.event.team_run_id != attention.team_run_id
                || operation.event.work_id != attention.work_id
                || operation.event.resulting_version != attention.work_version
            {
                return Err(StoreError::Conflict(format!(
                    "HostAttention {} does not match source WorkEvent {}",
                    attention.id, attention.source_event_ref
                )));
            }
        } else {
            // Member-runtime attention can be caused by a TeamRun/provider
            // event rather than a WorkEvent. Validate that its current Work
            // subject still resolves inside the named TeamRun.
            let work = self
                .latest_works_unlocked()?
                .remove(&attention.work_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!("work not found: {}", attention.work_id))
                })?;
            if work.team_run_id != attention.team_run_id {
                return Err(StoreError::Conflict(format!(
                    "Work {} does not belong to TeamRun {}",
                    attention.work_id, attention.team_run_id
                )));
            }
            if work.version < attention.work_version {
                return Err(StoreError::Conflict(format!(
                    "HostAttention references future Work version {} > {}",
                    attention.work_version, work.version
                )));
            }
        }
        if let Some(member_run_id) = attention.member_run_id.as_deref() {
            self.require_member_run_unlocked(member_run_id, &attention.team_run_id)?;
        }

        self.append_jsonl_unlocked("host_attentions.jsonl", attention)?;
        Ok(attention.clone())
    }

    fn same_host_attention_fact(left: &HostAttention, right: &HostAttention) -> bool {
        left.team_run_id == right.team_run_id
            && left.kind == right.kind
            && left.work_id == right.work_id
            && left.work_version == right.work_version
            && left.source_event_ref == right.source_event_ref
            && left.member_run_id == right.member_run_id
            && left.created_at == right.created_at
    }

    fn host_attention_for_work_operation(operation: &WorkOperation) -> Option<HostAttention> {
        let kind = match operation.event.kind {
            WorkEventKind::Submitted => HostAttentionKind::WorkReviewRequested,
            WorkEventKind::Blocked => HostAttentionKind::WorkBlocked,
            WorkEventKind::Accepted => HostAttentionKind::WorkAccepted,
            WorkEventKind::ChangesRequested => HostAttentionKind::WorkChangesRequested,
            WorkEventKind::Cancelled => HostAttentionKind::WorkCancelled,
            _ => return None,
        };
        Some(HostAttention {
            id: format!("host-attention-{}", operation.event.id),
            team_run_id: operation.event.team_run_id.clone(),
            kind,
            work_id: operation.event.work_id.clone(),
            work_version: operation.event.resulting_version,
            source_event_ref: operation.event.id.clone(),
            member_run_id: operation.work.active_member_run_id.clone(),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            claimed_host_lease_id: None,
            claimed_host_lease_generation: None,
            claimed_host_lease_owner_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: operation.event.created_at.clone(),
            updated_at: operation.event.created_at.clone(),
        })
    }

    fn ensure_host_attention_for_work_operation_unlocked(
        &self,
        operation: &WorkOperation,
    ) -> StoreResult<Option<HostAttention>> {
        Self::host_attention_for_work_operation(operation)
            .map(|attention| self.ensure_host_attention_unlocked(&attention))
            .transpose()
    }

    fn reconcile_work_host_attentions_unlocked(&self) -> StoreResult<Vec<HostAttention>> {
        let operations = self.work_operations_unlocked()?;
        let mut projected = self.latest_host_attentions_unlocked()?;
        let mut reconciled = Vec::new();
        for operation in &operations {
            let Some(attention) = Self::host_attention_for_work_operation(operation) else {
                continue;
            };
            if let Some(existing) = projected.get(&attention.id) {
                if !Self::same_host_attention_fact(existing, &attention) {
                    return Err(StoreError::Conflict(format!(
                        "HostAttention id {} already names a different causal fact",
                        attention.id
                    )));
                }
                reconciled.push(existing.clone());
                continue;
            }
            attention
                .validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            self.require_team_run_unlocked(&attention.team_run_id)?;
            if let Some(member_run_id) = attention.member_run_id.as_deref() {
                self.require_member_run_unlocked(member_run_id, &attention.team_run_id)?;
            }
            self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
            projected.insert(attention.id.clone(), attention.clone());
            reconciled.push(attention);
        }
        Ok(reconciled)
    }

    fn host_attention_inbox_for_team_run_unreconciled(
        &self,
        team_run_id: &str,
        include_all: bool,
    ) -> StoreResult<HostAttentionInbox> {
        let run = self.require_team_run_unlocked(team_run_id)?;
        let attentions = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| attention.team_run_id == team_run_id)
            .filter(|attention| include_all || attention.needs_host_action())
            .collect::<Vec<_>>();
        let warning = if run.host_thread_id.is_none() && !attentions.is_empty() {
            Some(format!(
                "UNBOUND_HOST: TeamRun {} has actionable Host attention but no exact native Host task; bind host_surface + host_thread_id before delivery",
                run.id
            ))
        } else {
            None
        };
        Ok(HostAttentionInbox {
            team_run_id: run.id,
            host_surface: run.host_surface,
            host_thread_id: run.host_thread_id,
            warning,
            attentions,
        })
    }

    fn latest_host_attentions_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, HostAttention>> {
        Ok(latest_by_id(
            self.read_jsonl::<HostAttention>("host_attentions.jsonl")?,
            |attention| attention.id.clone(),
        ))
    }

    fn require_host_attention_unlocked(&self, attention_id: &str) -> StoreResult<HostAttention> {
        self.latest_host_attentions_unlocked()?
            .remove(attention_id)
            .ok_or_else(|| StoreError::Conflict(format!("HostAttention not found: {attention_id}")))
    }

    fn require_exact_host_binding_unlocked(
        &self,
        team_run_id: &str,
        host_surface: &str,
        host_thread_id: &str,
    ) -> StoreResult<AgentTeamRun> {
        require_non_empty_store(host_surface, "Host surface")?;
        require_non_empty_store(host_thread_id, "Host thread id")?;
        let run = self.require_team_run_unlocked(team_run_id)?;
        if canonical_surface(&run.host_surface) != canonical_surface(host_surface)
            || run.host_thread_id.as_deref() != Some(host_thread_id)
        {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_MISMATCH: TeamRun {team_run_id} is not bound to {host_surface}/{host_thread_id}"
            )));
        }
        Ok(run)
    }

    fn require_member_run_unlocked(
        &self,
        member_run_id: &str,
        team_run_id: &str,
    ) -> StoreResult<ProviderRuntimeProjection> {
        let member = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        )
        .remove(member_run_id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {member_run_id}")))?;
        if member.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} does not belong to TeamRun {team_run_id}"
            )));
        }
        Ok(member)
    }

    /// Resolve a runtime only when the latest TeamRun explicitly names it as a
    /// member. A same-team ProviderRuntimeProjection row is not membership authority: the
    /// append-only ledger can contain stale or forged rows that were never
    /// admitted to the TeamRun.
    fn ensure_unique_member_identity_unlocked(
        &self,
        team_run: &AgentTeamRun,
        proposed: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        let identity = member_identity(proposed);
        let members = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        );
        if let Some(existing) = team_run
            .member_run_ids
            .iter()
            .filter_map(|id| members.get(id))
            .find(|member| member_identity(member) == identity)
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_IDENTITY_CONFLICT: stable identity {identity} is already admitted as ProviderRuntimeProjection {}",
                existing.id
            )));
        }
        Ok(())
    }

    fn ensure_member_admission_identity_unlocked(
        &self,
        team_run: &AgentTeamRun,
        proposed: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        let identity = member_identity(proposed);
        let members = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |row| row.id.clone(),
        );
        let candidates = team_run
            .member_run_ids
            .iter()
            .filter_map(|id| members.get(id))
            .filter(|member| member_identity(member) == identity)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(());
        }
        let max_generation = candidates
            .iter()
            .map(|member| member.runtime_generation)
            .max()
            .unwrap_or(0);
        if candidates
            .iter()
            .any(|member| member_is_active_reviewer_runtime(member))
            || proposed.runtime_generation <= max_generation
            || candidates.iter().any(|member| {
                member.provider != proposed.provider
                    || member.role != proposed.role
                    || member.agent_member_id != proposed.agent_member_id
            })
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_IDENTITY_CONFLICT: stable identity {identity} is already admitted and is not a closed lower-generation runtime"
            )));
        }
        Ok(())
    }

    /// A stable reviewer identity is trustworthy only when it resolves to one
    /// exact runtime in the latest TeamRun membership. Reject duplicate stable
    /// identities instead of choosing whichever ProviderRuntimeProjection happened to be
    /// loaded first.
    fn validate_work_relations_unlocked(&self, work: &Work) -> StoreResult<()> {
        let works = self.latest_works_unlocked()?;
        for prerequisite_id in &work.prerequisite_work_ids {
            let prerequisite = works.get(prerequisite_id).ok_or_else(|| {
                StoreError::Conflict(format!("prerequisite work not found: {prerequisite_id}"))
            })?;
            if !works_share_scope(prerequisite, work) || prerequisite.id == work.id {
                return Err(StoreError::Conflict(
                    "prerequisites must be distinct Works in the same durable Team scope"
                        .to_string(),
                ));
            }
        }
        if let Some(parent_id) = work.parent_work_id.as_deref() {
            let parent = works.get(parent_id).ok_or_else(|| {
                StoreError::Conflict(format!("parent work not found: {parent_id}"))
            })?;
            if !works_share_scope(parent, work) || parent.id == work.id {
                return Err(StoreError::Conflict(
                    "parent_work_id must reference a distinct Work in the same durable Team scope"
                        .to_string(),
                ));
            }
        }
        if let Some(member_run_id) = work.active_member_run_id.as_deref() {
            let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
            self.ensure_member_can_receive_work_unlocked(&member)?;
            if work.owner_member_id.as_deref() != Some(member_identity(&member).as_str()) {
                return Err(StoreError::Conflict(
                    "owner_member_id does not match active ProviderRuntimeProjection stable identity".to_string(),
                ));
            }
        } else if work.owner_member_id.is_some() {
            return Err(StoreError::Conflict(
                "owned Work requires an active_member_run_id binding".to_string(),
            ));
        }
        Ok(())
    }

    fn ensure_member_can_receive_work_unlocked(
        &self,
        member: &ProviderRuntimeProjection,
    ) -> StoreResult<()> {
        if !member.coordination_is_active()
            || matches!(
                member.status,
                firm_core::MemberRunStatus::Stopped | firm_core::MemberRunStatus::Failed
            )
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_UNAVAILABLE: ProviderRuntimeProjection {} cannot receive Work while {:?}/{:?}",
                member.id, member.coordination_status, member.status
            )));
        }
        Ok(())
    }

    fn initial_work_deliveries_unlocked(
        &self,
        work: &Work,
        event_id: &str,
        updated_at: &str,
    ) -> StoreResult<Vec<ProviderWorkDispatch>> {
        let Some(member_run_id) = work.active_member_run_id.as_deref() else {
            return Ok(Vec::new());
        };
        let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
        if self
            .ensure_member_can_receive_work_unlocked(&member)
            .is_err()
        {
            return Ok(Vec::new());
        }
        // Skip loopback deliveries for terminal work: the owning member
        // already knows their work is Done/Cancelled — self-notification is
        // redundant. Non-terminal events (Created, Assigned, ChangesRequested,
        // Resumed, Rebound) genuinely need delivery even to the owner.
        if work.is_terminal() {
            if let Some(ref owner_id) = work.owner_member_id {
                if owner_id == &member_identity(&member) {
                    return Ok(Vec::new());
                }
            }
        }
        Ok(vec![ProviderWorkDispatch {
            id: format!("work-delivery-{event_id}-{member_run_id}"),
            work_event_id: event_id.to_string(),
            team_run_id: work.team_run_id.clone(),
            work_id: work.id.clone(),
            work_version: work.version,
            recipient_member_run_id: member_run_id.to_string(),
            status: ProviderWorkDispatchStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: updated_at.to_string(),
        }])
    }

    fn current_work_unlocked(&self, work_id: &str, expected_version: u64) -> StoreResult<Work> {
        let current = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        if current.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "VERSION_CONFLICT: work {work_id} is version {}, expected {expected_version}",
                current.version
            )));
        }
        Ok(current)
    }

    fn ensure_deliveries_reassignable_unlocked(&self, work: &Work) -> StoreResult<()> {
        if self
            .latest_work_deliveries_unlocked()?
            .values()
            .any(|delivery| {
                delivery.work_id == work.id
                    && delivery.work_version == work.version
                    && work.active_member_run_id.as_deref()
                        == Some(delivery.recipient_member_run_id.as_str())
                    && matches!(
                        delivery.status,
                        ProviderWorkDispatchStatus::Claimed
                            | ProviderWorkDispatchStatus::ProviderReceived
                    )
            })
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work delivery was already accepted".to_string(),
            ));
        }
        Ok(())
    }

    /// Return an exact idempotent retry, while rejecting accidental reuse of
    /// the same key for a different Work or command. A bare key is not enough
    /// to identify an operation safely: without this fingerprint a retry of
    /// `start(work-a)` could silently return the result of `cancel(work-b)`.
    fn idempotent_work_operation_unlocked(
        &self,
        idempotency_key: &str,
        work_id: &str,
        kind: WorkEventKind,
    ) -> StoreResult<Option<WorkOperation>> {
        let existing = self
            .work_operations_with_recovered_provenance_unlocked()?
            .into_iter()
            .find(|operation| operation.event.idempotency_key == idempotency_key);
        let Some(existing) = existing else {
            return Ok(None);
        };
        if existing.event.work_id != work_id || existing.event.kind != kind {
            return Err(StoreError::Conflict(format!(
                "IDEMPOTENCY_CONFLICT: key {idempotency_key} already belongs to {:?} on Work {}",
                existing.event.kind, existing.event.work_id
            )));
        }
        // If the original process crashed after fsyncing the WorkOperation but
        // before its derived HostAttention row, the ordinary idempotent retry
        // repairs that gap before returning the already-applied Work result.
        self.ensure_host_attention_for_work_operation_unlocked(&existing)?;
        Ok(Some(existing))
    }

    fn work_operations_unlocked(&self) -> StoreResult<Vec<WorkOperation>> {
        let mut operations: Vec<WorkOperation> = self.read_jsonl("work_operations.jsonl")?;
        let mut delegated = self
            .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")?
            .into_iter()
            .map(|operation| operation.target_work_operation)
            .collect::<Vec<_>>();
        // WorkDelegation creation is crash-atomic in a separate composite
        // ledger, while later target transitions use the ordinary Work ledger.
        // Concatenating files would place every delegated Work's version 1
        // after its later versions and make the projection regress. Preserve
        // the ordinary ledger's exact append order (the durable `--since`
        // cursor), then insert each composite creation at its temporal slot
        // and always before any later revision of that same Work.
        delegated.sort_by(|left, right| work_event_order(&left.event, &right.event));
        for operation in delegated {
            let same_work = operations
                .iter()
                .position(|existing| existing.work.id == operation.work.id)
                .unwrap_or(operations.len());
            let temporal = operations
                .iter()
                .position(|existing| work_event_order(&operation.event, &existing.event).is_lt())
                .unwrap_or(operations.len());
            operations.insert(same_work.min(temporal), operation);
        }
        Ok(operations)
    }

    fn all_work_delegation_revisions_unlocked(&self) -> StoreResult<Vec<WorkDelegationRevision>> {
        let mut revisions = self
            .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")?
            .into_iter()
            .map(|operation| WorkDelegationRevision {
                delegation: operation.delegation,
                event: operation.event,
            })
            .collect::<Vec<_>>();
        revisions
            .extend(self.read_jsonl::<WorkDelegationRevision>("work_delegation_events.jsonl")?);
        revisions.extend(
            self.work_operations_unlocked()?
                .into_iter()
                .flat_map(|operation| operation.delegation_revisions),
        );
        revisions.extend(self.trust_work_delegation_revisions_unlocked()?);
        revisions.sort_by(|left, right| {
            left.delegation
                .id
                .cmp(&right.delegation.id)
                .then(left.event.sequence.cmp(&right.event.sequence))
        });
        Ok(revisions)
    }

    fn latest_work_delegations_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, WorkDelegation>> {
        let mut latest = std::collections::BTreeMap::<String, WorkDelegation>::new();
        for revision in self.all_work_delegation_revisions_unlocked()? {
            revision
                .delegation
                .validate()
                .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION: {error}")))?;
            revision.event.validate().map_err(|error| {
                StoreError::Conflict(format!("INVALID_DELEGATION_EVENT: {error}"))
            })?;
            if revision.event.delegation_id != revision.delegation.id
                || revision.event.resulting_version != revision.delegation.version
                || revision.event.expected_version.saturating_add(1)
                    != revision.event.resulting_version
            {
                return Err(StoreError::Conflict(format!(
                    "DELEGATION_LEDGER_CORRUPT: event {} does not match projection {} version {}",
                    revision.event.id, revision.delegation.id, revision.delegation.version
                )));
            }
            if let Some(current) = latest.get(&revision.delegation.id) {
                if revision.event.expected_version != current.version {
                    return Err(StoreError::Conflict(format!(
                        "DELEGATION_LEDGER_CORRUPT: Delegation {} expected version {}, current {}",
                        revision.delegation.id, revision.event.expected_version, current.version
                    )));
                }
            } else if revision.event.expected_version != 0 {
                return Err(StoreError::Conflict(format!(
                    "DELEGATION_LEDGER_CORRUPT: Delegation {} does not start at version 1",
                    revision.delegation.id
                )));
            }
            latest.insert(revision.delegation.id.clone(), revision.delegation);
        }
        Ok(latest)
    }

    /// Compute every Delegation transition caused by one authoritative target
    /// Work projection. Callers that are already committing a WorkOperation
    /// embed these revisions in that same row; the public reconciler uses the
    /// identical reducer to repair older split-ledger crash gaps.
    fn work_delegation_rollup_revisions_unlocked(
        &self,
        target: &Work,
        context: &WorkCommandContext,
    ) -> StoreResult<Vec<WorkDelegationRevision>> {
        let existing_revisions = self.all_work_delegation_revisions_unlocked()?;
        let current = self
            .latest_work_delegations_unlocked()?
            .into_values()
            .filter(|delegation| delegation.target_work_ref.work_id == target.id)
            .collect::<Vec<_>>();
        let mut revisions = Vec::new();
        for delegation in current {
            let desired = if target.phase == WorkPhase::Closed {
                match target.resolution {
                    Some(WorkResolution::Accepted) => Some((
                        WorkDelegationState::Completed,
                        WorkDelegationTransition::Completed,
                        target
                            .result_summary
                            .clone()
                            .unwrap_or_else(|| "target Work accepted".to_string()),
                        None,
                    )),
                    Some(WorkResolution::Failed) => Some((
                        WorkDelegationState::Failed,
                        WorkDelegationTransition::Failed,
                        target
                            .result_summary
                            .clone()
                            .or_else(|| target.blocker_reason.clone())
                            .unwrap_or_else(|| "target Work failed".to_string()),
                        None,
                    )),
                    Some(WorkResolution::Cancelled) => Some((
                        WorkDelegationState::Cancelled,
                        WorkDelegationTransition::Cancelled,
                        target
                            .result_summary
                            .clone()
                            .or_else(|| target.blocker_reason.clone())
                            .unwrap_or_else(|| "target Work cancelled".to_string()),
                        None,
                    )),
                    None => None,
                }
            } else if target.condition == WorkCondition::Blocked {
                Some((
                    WorkDelegationState::Blocked,
                    WorkDelegationTransition::Blocked,
                    String::new(),
                    Some(
                        target
                            .blocker_reason
                            .clone()
                            .unwrap_or_else(|| "target Work blocked".to_string()),
                    ),
                ))
            } else if delegation.state == WorkDelegationState::Blocked {
                Some((
                    WorkDelegationState::Active,
                    WorkDelegationTransition::Resumed,
                    String::new(),
                    None,
                ))
            } else {
                None
            };
            let Some((state, transition, resolution, blocker)) = desired else {
                continue;
            };
            if delegation.state == state
                || matches!(
                    delegation.state,
                    WorkDelegationState::Completed
                        | WorkDelegationState::Failed
                        | WorkDelegationState::Cancelled
                )
            {
                continue;
            }
            let mut next = delegation.clone();
            next.state = state;
            next.version = next.version.saturating_add(1);
            next.updated_at = context.created_at.clone();
            next.blocker_reason = blocker;
            next.resolution_summary = if resolution.is_empty() {
                None
            } else {
                Some(resolution)
            };
            let event = WorkDelegationEvent {
                id: format!("{}:delegation:{}", context.event_id, delegation.id),
                delegation_id: delegation.id.clone(),
                sequence: next.version,
                transition,
                expected_version: delegation.version,
                resulting_version: next.version,
                performed_by_actor: context.performed_by_actor.clone(),
                causation_ref: context.causation_ref.clone(),
                idempotency_key: format!(
                    "{}:delegation:{}",
                    context.idempotency_key, delegation.id
                ),
                payload: serde_json::json!({"target_work_version": target.version}),
                created_at: context.created_at.clone(),
            };
            next.validate()
                .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION: {error}")))?;
            event.validate().map_err(|error| {
                StoreError::Conflict(format!("INVALID_DELEGATION_EVENT: {error}"))
            })?;
            if existing_revisions.iter().any(|revision| {
                revision.event.id == event.id
                    || revision.event.idempotency_key == event.idempotency_key
            }) {
                return Err(StoreError::Conflict(format!(
                    "DELEGATION_EVENT_CONFLICT: {}",
                    event.id
                )));
            }
            revisions.push(WorkDelegationRevision {
                delegation: next,
                event,
            });
        }
        Ok(revisions)
    }

    fn append_work_delegation_transition_unlocked(
        &self,
        current: &WorkDelegation,
        next: WorkDelegation,
        event: WorkDelegationEvent,
    ) -> StoreResult<WorkDelegation> {
        let latest = self
            .latest_work_delegations_unlocked()?
            .remove(&current.id)
            .ok_or_else(|| StoreError::Conflict(format!("delegation not found: {}", current.id)))?;
        if latest != *current {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_VERSION_CONFLICT: {} changed concurrently",
                current.id
            )));
        }
        if next.id != current.id
            || next.source_work_ref != current.source_work_ref
            || next.source_work_version != current.source_work_version
            || next.source_owner_member_id != current.source_owner_member_id
            || next.created_by_member_run_id != current.created_by_member_run_id
            || next.target_agent_team_id != current.target_agent_team_id
            || next.target_work_ref != current.target_work_ref
            || next.delegated_by_actor != current.delegated_by_actor
            || next.created_at != current.created_at
            || next.version != current.version.saturating_add(1)
            || event.delegation_id != current.id
            || event.expected_version != current.version
            || event.resulting_version != next.version
        {
            return Err(StoreError::Conflict(
                "DELEGATION_TRANSITION_INVALID: immutable identity or CAS fields changed"
                    .to_string(),
            ));
        }
        let legal = matches!(
            (current.state, next.state),
            (WorkDelegationState::Active, WorkDelegationState::Blocked)
                | (WorkDelegationState::Blocked, WorkDelegationState::Active)
                | (WorkDelegationState::Active, WorkDelegationState::Completed)
                | (WorkDelegationState::Blocked, WorkDelegationState::Completed)
                | (WorkDelegationState::Active, WorkDelegationState::Failed)
                | (WorkDelegationState::Blocked, WorkDelegationState::Failed)
                | (WorkDelegationState::Active, WorkDelegationState::Cancelled)
                | (WorkDelegationState::Blocked, WorkDelegationState::Cancelled)
        );
        if !legal {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_TRANSITION_INVALID: {:?}->{:?}",
                current.state, next.state
            )));
        }
        next.validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION: {error}")))?;
        event
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_DELEGATION_EVENT: {error}")))?;
        if self
            .all_work_delegation_revisions_unlocked()?
            .iter()
            .any(|revision| {
                revision.event.id == event.id
                    || revision.event.idempotency_key == event.idempotency_key
            })
        {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_EVENT_CONFLICT: {}",
                event.id
            )));
        }
        self.append_jsonl_unlocked(
            "work_delegation_events.jsonl",
            &WorkDelegationRevision {
                delegation: next.clone(),
                event,
            },
        )?;
        Ok(next)
    }

    /// Fold immutable additive provenance through every WorkOperation.
    ///
    /// Mixed-version writers may deserialize a newer complete projection,
    /// discard unknown fields, and append a later row without `team_id` or
    /// `created_by_member_id`. Once either fact has been established, no Work
    /// command is allowed to remove or change it. Reads therefore recover a
    /// missing later value from ordered WorkOperation ledger history, while a
    /// conflicting non-null value remains corruption and is refused.
    fn work_operations_with_recovered_provenance_unlocked(
        &self,
    ) -> StoreResult<Vec<WorkOperation>> {
        let mut team_ids = std::collections::BTreeMap::<String, String>::new();
        let mut creator_ids = std::collections::BTreeMap::<String, String>::new();
        let mut recovered = Vec::new();
        for mut operation in self.work_operations_unlocked()? {
            let work_id = operation.work.id.clone();
            match (team_ids.get(&work_id), operation.work.team_id.as_deref()) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return Err(StoreError::Conflict(format!(
                        "WORK_PROJECTION_PROVENANCE_CONFLICT: Work {work_id} changed team_id from {expected} to {actual} in event {}",
                        operation.event.id
                    )));
                }
                (Some(expected), None) => operation.work.team_id = Some(expected.clone()),
                (None, Some(actual)) => {
                    team_ids.insert(work_id.clone(), actual.to_string());
                }
                _ => {}
            }
            match (
                creator_ids.get(&work_id),
                operation.work.created_by_member_id.as_deref(),
            ) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return Err(StoreError::Conflict(format!(
                        "WORK_PROJECTION_PROVENANCE_CONFLICT: Work {work_id} changed created_by_member_id from {expected} to {actual} in event {}",
                        operation.event.id
                    )));
                }
                (Some(expected), None) => {
                    operation.work.created_by_member_id = Some(expected.clone())
                }
                (None, Some(actual)) => {
                    creator_ids.insert(work_id, actual.to_string());
                }
                _ => {}
            }
            recovered.push(operation);
        }
        Ok(recovered)
    }

    /// Current-version writers must emit a complete projection. This guard is
    /// the refusal half of mixed-schema compatibility; the recovery fold above
    /// is the lossless-preservation half for sparse rows already appended by a
    /// stale binary.
    fn append_work_operation_unlocked(&self, operation: &WorkOperation) -> StoreResult<()> {
        operation
            .work
            .validate()
            .map_err(|error| StoreError::Conflict(format!("INVALID_WORK_PROJECTION: {error}")))?;
        let existing_operations = self.work_operations_unlocked()?;
        let existing_record_ids = existing_operations
            .iter()
            .flat_map(|row| {
                row.condition_records
                    .iter()
                    .map(|record| record.id.as_str())
                    .chain(row.reports.iter().map(|record| record.id.as_str()))
                    .chain(row.evidence_records.iter().map(|record| record.id.as_str()))
                    .chain(row.decisions.iter().map(|record| record.id.as_str()))
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut new_record_ids = std::collections::BTreeSet::new();
        for (id, work_id, validation) in operation
            .condition_records
            .iter()
            .map(|record| {
                (
                    record.id.as_str(),
                    record.work_id.as_str(),
                    record.validate(),
                )
            })
            .chain(operation.reports.iter().map(|record| {
                (
                    record.id.as_str(),
                    record.work_id.as_str(),
                    record.validate(),
                )
            }))
            .chain(operation.evidence_records.iter().map(|record| {
                (
                    record.id.as_str(),
                    record.work_id.as_str(),
                    record.validate(),
                )
            }))
            .chain(operation.decisions.iter().map(|record| {
                (
                    record.id.as_str(),
                    record.work_id.as_str(),
                    record.validate(),
                )
            }))
        {
            validation.map_err(|error| {
                StoreError::Conflict(format!("INVALID_WORK_RECORD {id}: {error}"))
            })?;
            if work_id != operation.work.id {
                return Err(StoreError::Conflict(format!(
                    "WORK_RECORD_SCOPE_MISMATCH: record {id} belongs to Work {work_id}, operation belongs to {}",
                    operation.work.id
                )));
            }
            if existing_record_ids.contains(id) || !new_record_ids.insert(id) {
                return Err(StoreError::Conflict(format!(
                    "WORK_RECORD_ID_CONFLICT: record id {id} is already in use"
                )));
            }
        }
        for report in &operation.reports {
            if report.work_version != operation.work.version {
                return Err(StoreError::Conflict(format!(
                    "WORK_REPORT_VERSION_MISMATCH: report {} binds Work version {}, operation produced {}",
                    report.id, report.work_version, operation.work.version
                )));
            }
            let matching_evidence = operation.evidence_records.iter().any(|evidence| {
                evidence.work_report_id == report.id
                    && evidence.work_version == report.work_version
                    && evidence.candidate_revision == report.candidate_revision
                    && report.evidence_refs.contains(&evidence.id)
            });
            if !matching_evidence {
                return Err(StoreError::Conflict(format!(
                    "WORK_REPORT_EVIDENCE_MISMATCH: report {} lacks exact candidate evidence",
                    report.id
                )));
            }
        }
        if let Some(current) = self
            .latest_works_unlocked()?
            .remove(operation.work.id.as_str())
        {
            if current.team_id.is_some() && operation.work.team_id != current.team_id {
                return Err(StoreError::Conflict(format!(
                    "WORK_PROJECTION_PROVENANCE_REGRESSION: Work {} event {} would drop or change team_id",
                    operation.work.id, operation.event.id
                )));
            }
            if current.created_by_member_id.is_some()
                && operation.work.created_by_member_id != current.created_by_member_id
            {
                return Err(StoreError::Conflict(format!(
                    "WORK_PROJECTION_PROVENANCE_REGRESSION: Work {} event {} would drop or change created_by_member_id",
                    operation.work.id, operation.event.id
                )));
            }
        }
        self.append_jsonl_unlocked("work_operations.jsonl", operation)
    }

    fn ensure_work_event_id_available_unlocked(&self, event_id: &str) -> StoreResult<()> {
        if self
            .work_operations_unlocked()?
            .iter()
            .any(|operation| operation.event.id == event_id)
        {
            return Err(StoreError::Conflict(format!(
                "WORK_EVENT_ID_CONFLICT: event id {event_id} is already in use"
            )));
        }
        Ok(())
    }

    fn next_work_delivery_update_sequence_unlocked(&self) -> StoreResult<u64> {
        let embedded_max = self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.delivery_updates)
            .map(|update| update.update_sequence)
            .max()
            .unwrap_or(0);
        let standalone_max = self
            .read_jsonl::<ProviderWorkDispatchUpdate>("work_delivery_updates.jsonl")?
            .into_iter()
            .map(|update| update.update_sequence)
            .max()
            .unwrap_or(0);
        Ok(embedded_max.max(standalone_max).saturating_add(1))
    }

    fn latest_works_unlocked(&self) -> StoreResult<std::collections::BTreeMap<String, Work>> {
        let mut latest = latest_by_id(
            self.work_operations_with_recovered_provenance_unlocked()?,
            |operation| operation.work.id.clone(),
        )
        .into_iter()
        .map(|(id, operation)| (id, operation.work))
        .collect::<std::collections::BTreeMap<_, _>>();
        for work in self.trust_work_projections_unlocked()? {
            match latest.get(&work.id) {
                Some(current) if current.version >= work.version => {}
                _ => {
                    latest.insert(work.id.clone(), work);
                }
            }
        }
        Ok(latest)
    }

    fn latest_work_deliveries_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, ProviderWorkDispatch>> {
        let mut deliveries = std::collections::BTreeMap::new();
        let mut legacy_updates = Vec::new();
        let mut sequenced_updates = Vec::new();
        let mut legacy_order = 0_u64;
        for operation in self.work_operations_unlocked()? {
            for delivery in operation.deliveries {
                deliveries.insert(delivery.id.clone(), delivery);
            }
            for update in operation.delivery_updates {
                if update.update_sequence == 0 {
                    legacy_updates.push((update.updated_at.clone(), legacy_order, update));
                    legacy_order = legacy_order.saturating_add(1);
                } else {
                    sequenced_updates.push(update);
                }
            }
        }
        for update in
            self.read_jsonl::<ProviderWorkDispatchUpdate>("work_delivery_updates.jsonl")?
        {
            if update.update_sequence == 0 {
                legacy_updates.push((update.updated_at.clone(), legacy_order, update));
                legacy_order = legacy_order.saturating_add(1);
            } else {
                sequenced_updates.push(update);
            }
        }
        // Rows written before update_sequence existed remain readable. Their
        // best available ordering evidence is timestamp plus stable file-scan
        // order. All new writes are then folded by the Store-assigned sequence,
        // independent of caller clocks or which JSONL file carries the update.
        legacy_updates.sort_by(|left, right| {
            compare_store_timestamps(&left.0, &right.0).then(left.1.cmp(&right.1))
        });
        sequenced_updates.sort_by_key(|update| update.update_sequence);
        for update in legacy_updates
            .into_iter()
            .map(|(_, _, update)| update)
            .chain(sequenced_updates)
        {
            if let Some(delivery) = deliveries.get_mut(&update.delivery_id) {
                apply_work_delivery_update(delivery, update);
            }
        }
        Ok(deliveries)
    }

    pub fn append_team_message(&self, value: &ProviderDispatchEnvelope) -> StoreResult<()> {
        reject_raw_provider_interaction_append(value)?;
        self.append_jsonl("team_messages.jsonl", value)
    }

    /// Append a manually-authored ProviderDispatchEnvelope under the global lock.
    pub fn append_team_message_checked(&self, value: &ProviderDispatchEnvelope) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        value
            .validate_provider_interaction_contract()
            .map_err(StoreError::Conflict)?;
        if value.kind == ProviderDispatchIntent::ProviderInteractionResponse {
            return Err(StoreError::Conflict(
                "PROVIDER_INTERACTION_RESPONSE_REQUIRES_ATOMIC_RECORD: use record_provider_interaction_response"
                    .to_string(),
            ));
        }
        let messages = latest_by_id(
            self.read_jsonl::<ProviderDispatchEnvelope>("team_messages.jsonl")?,
            |message| message.id.clone(),
        );
        if messages.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "team message already exists: {}",
                value.id
            )));
        }
        if value.kind == ProviderDispatchIntent::ProviderInteractionRequest {
            let body = ProviderInteractionRequestBody::parse_canonical_json(&value.body)
                .map_err(StoreError::Conflict)?;
            let member = self.require_member_run_unlocked(&body.member, &value.team_run_id)?;
            if !member.coordination_is_active() || member.runtime_generation != body.generation {
                return Err(StoreError::Conflict(format!(
                    "provider interaction request generation {} is not active on ProviderRuntimeProjection {} generation {}",
                    body.generation, body.member, member.runtime_generation,
                )));
            }
            let native_session = member.native_session.as_ref().ok_or_else(|| {
                StoreError::Conflict(format!(
                    "provider interaction request ProviderRuntimeProjection {} has no native session",
                    body.member
                ))
            })?;
            if member.provider != body.provider
                || native_session.provider != body.provider
                || native_session.native_session_id != body.session
            {
                return Err(StoreError::Conflict(format!(
                    "provider interaction request does not match ProviderRuntimeProjection {} provider/native session",
                    body.member
                )));
            }
            let host_deliveries = value
                .deliveries
                .iter()
                .filter(|delivery| delivery.member_id == "host")
                .collect::<Vec<_>>();
            if host_deliveries.len() != 1 {
                return Err(StoreError::Conflict(
                    "provider interaction request requires exactly one Host delivery".to_string(),
                ));
            }
            let host_delivery = host_deliveries[0];
            if host_delivery.policy != TeamDeliveryPolicy::ManualAck
                || host_delivery.status != TeamDeliveryStatus::Delivered
            {
                return Err(StoreError::Conflict(
                    "new provider interaction request Host delivery must be delivered manual_ack"
                        .to_string(),
                ));
            }
        }
        if let Some(work_id) = value.work_id.as_deref() {
            let work = self
                .latest_works_unlocked()?
                .remove(work_id)
                .ok_or_else(|| StoreError::Conflict(format!("Work not found: {work_id}")))?;
            if work.team_run_id != value.team_run_id {
                return Err(StoreError::Conflict(format!(
                    "Work {work_id} belongs to TeamRun {}, not {}",
                    work.team_run_id, value.team_run_id
                )));
            }
        }
        self.append_jsonl_unlocked("team_messages.jsonl", value)
    }

    /// Record one provider-interaction response and consume/acknowledge its
    /// request under the Store write lock. Causation is the idempotency key:
    /// an exact semantic retry returns the existing response, while a second
    /// answer or changed actor/routing conflicts.
    ///
    /// The response row is appended before the request ACK. If a process dies
    /// between those two JSONL writes, an exact retry observes the response and
    /// completes the ACK; it can never append a second semantic answer.
    pub fn record_provider_interaction_response(
        &self,
        response: &ProviderDispatchEnvelope,
        acknowledged_at: &str,
    ) -> StoreResult<ProviderDispatchEnvelope> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        response
            .validate_provider_interaction_contract()
            .map_err(StoreError::Conflict)?;
        if response.kind != ProviderDispatchIntent::ProviderInteractionResponse {
            return Err(StoreError::Conflict(
                "provider interaction atomic response boundary requires provider_interaction_response"
                    .to_string(),
            ));
        }
        let response_body = ProviderInteractionResponseBody::parse_canonical_json(&response.body)
            .map_err(StoreError::Conflict)?;
        let request_id = response.causation_id.as_deref().ok_or_else(|| {
            StoreError::Conflict(
                "provider interaction response requires request causation_id".to_string(),
            )
        })?;
        let messages = latest_by_id(
            self.read_jsonl::<ProviderDispatchEnvelope>("team_messages.jsonl")?,
            |message| message.id.clone(),
        );
        let mut request = messages.get(request_id).cloned().ok_or_else(|| {
            StoreError::Conflict(format!(
                "provider interaction request not found: {request_id}"
            ))
        })?;
        let stable_response_id =
            provider_interaction_response_id(request_id).map_err(StoreError::Conflict)?;
        if response.id != stable_response_id {
            return Err(StoreError::Conflict(format!(
                "provider interaction response id must be stable `{stable_response_id}`, got `{}`",
                response.id
            )));
        }
        if request.kind != ProviderDispatchIntent::ProviderInteractionRequest {
            return Err(StoreError::Conflict(format!(
                "provider interaction response causation {request_id} is not a request"
            )));
        }
        request
            .validate_provider_interaction_contract()
            .map_err(StoreError::Conflict)?;
        let request_body = ProviderInteractionRequestBody::parse_canonical_json(&request.body)
            .map_err(StoreError::Conflict)?;
        self.validate_provider_interaction_response_pair(
            &request,
            &request_body,
            response,
            &response_body,
        )?;
        let request_ack_changed =
            acknowledge_provider_interaction_request(&mut request, acknowledged_at)?;

        let prior_response = messages
            .values()
            .find(|message| {
                message.kind == ProviderDispatchIntent::ProviderInteractionResponse
                    && message.causation_id.as_deref() == Some(request_id)
            })
            .cloned();
        if let Some(existing) = prior_response {
            if !same_provider_interaction_response(&existing, response) {
                return Err(StoreError::Conflict(format!(
                    "PROVIDER_INTERACTION_RESPONSE_CONFLICT: request {request_id} already has a different response"
                )));
            }
            if request_ack_changed {
                self.append_jsonl_unlocked("team_messages.jsonl", &request)?;
            }
            return Ok(existing);
        }
        self.validate_provider_interaction_live_member(&request, &request_body)?;
        if messages.contains_key(&response.id) {
            return Err(StoreError::Conflict(format!(
                "team message already exists: {}",
                response.id
            )));
        }

        // Response-first makes a torn two-row append recoverable by the exact
        // retry path above. The global lock makes concurrent responders choose
        // one winner.
        self.append_jsonl_unlocked("team_messages.jsonl", response)?;
        if request_ack_changed {
            self.append_jsonl_unlocked("team_messages.jsonl", &request)?;
        }
        Ok(response.clone())
    }

    fn validate_provider_interaction_response_pair(
        &self,
        request: &ProviderDispatchEnvelope,
        request_body: &ProviderInteractionRequestBody,
        response: &ProviderDispatchEnvelope,
        response_body: &ProviderInteractionResponseBody,
    ) -> StoreResult<()> {
        if response.team_run_id != request.team_run_id
            || response.correlation_id != request.correlation_id
        {
            return Err(StoreError::Conflict(
                "provider interaction response must preserve request TeamRun and correlation_id"
                    .to_string(),
            ));
        }
        if response_body.interaction_type != request_body.interaction_type
            || response_body.session != request_body.session
            || response_body.member != request_body.member
            || response_body.generation != request_body.generation
        {
            return Err(StoreError::Conflict(
                "provider interaction response type/session/member/generation does not match request"
                    .to_string(),
            ));
        }
        if let Some(choice) = response_body.choice.as_deref() {
            if !request_body
                .options
                .iter()
                .any(|option| option.id == choice)
            {
                return Err(StoreError::Conflict(format!(
                    "provider interaction response choice `{choice}` is not a request option"
                )));
            }
        }
        if response_body.text.is_some() && canonical_surface(&request_body.provider) != "codex" {
            return Err(StoreError::Conflict(
                "free-text provider interaction responses are supported only for Codex requests"
                    .to_string(),
            ));
        }
        let target_deliveries = response
            .deliveries
            .iter()
            .filter(|delivery| delivery.member_id == request_body.member)
            .collect::<Vec<_>>();
        if !response
            .recipient_runtime_ids
            .iter()
            .any(|member| member == &request_body.member)
            || target_deliveries.len() != 1
        {
            return Err(StoreError::Conflict(format!(
                "provider interaction response requires exactly one delivery to ProviderRuntimeProjection {}",
                request_body.member
            )));
        }
        let target_delivery = target_deliveries[0];
        if target_delivery.policy != TeamDeliveryPolicy::Inject
            || target_delivery.status != TeamDeliveryStatus::Queued
            || target_delivery.attempt != 0
            || target_delivery.claim_id.is_some()
            || target_delivery.provider_receipt_id.is_some()
        {
            return Err(StoreError::Conflict(
                "new provider interaction response delivery must be unclaimed Inject+Queued"
                    .to_string(),
            ));
        }
        let run = self.require_team_run_unlocked(&request.team_run_id)?;
        let coordination_sender =
            response
                .sender
                .as_ref()
                .is_some_and(|sender| match sender.kind {
                    TeamActorKind::Host => {
                        response.sender_runtime_id == "host"
                            && (sender.id == "host"
                                || run
                                    .host_actor
                                    .as_ref()
                                    .is_some_and(|actor| actor.id == sender.id))
                    }
                    TeamActorKind::Operator => {
                        response.sender_runtime_id == format!("operator:{}", sender.id)
                    }
                    TeamActorKind::Service => {
                        response.sender_runtime_id == format!("service:{}", sender.id)
                    }
                    TeamActorKind::ProviderRuntimeProjection | TeamActorKind::AgentMember => false,
                });
        if !coordination_sender {
            return Err(StoreError::Conflict(
                "provider interaction response requires Host, Operator, or Service authorship"
                    .to_string(),
            ));
        }
        Ok(())
    }

    fn validate_provider_interaction_live_member(
        &self,
        request: &ProviderDispatchEnvelope,
        request_body: &ProviderInteractionRequestBody,
    ) -> StoreResult<()> {
        let member =
            self.require_member_run_unlocked(&request_body.member, &request.team_run_id)?;
        let same_live_generation = member.coordination_is_active()
            && member.runtime_generation == request_body.generation
            && member.provider == request_body.provider
            && member.native_session.as_ref().is_some_and(|native| {
                native.provider == request_body.provider
                    && native.native_session_id == request_body.session
            });
        if !same_live_generation {
            return Err(StoreError::Conflict(format!(
                "provider interaction request is stale for ProviderRuntimeProjection {} generation/session",
                request_body.member
            )));
        }
        Ok(())
    }

    pub fn insert_execution_node(&self, value: &ExecutionNode) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let nodes = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        );
        if nodes.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "execution node already exists: {}",
                value.id
            )));
        }
        self.append_jsonl_unlocked("execution_nodes.jsonl", value)
    }

    pub fn transition_execution_node(
        &self,
        expected: &ExecutionNode,
        next: &ExecutionNode,
    ) -> StoreResult<()> {
        next.validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(&expected.id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("execution node not found: {}", expected.id))
        })?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "execution node {} changed concurrently",
                expected.id
            )));
        }
        if next.id != current.id
            || next.display_name != current.display_name
            || next.created_at != current.created_at
            || !matches!(
                (current.status, next.status),
                (ExecutionNodeStatus::Active, ExecutionNodeStatus::Draining)
                    | (ExecutionNodeStatus::Draining, ExecutionNodeStatus::Retired)
            )
        {
            return Err(StoreError::Conflict(
                "NODE_TRANSITION_INVALID: allowed transitions are active->draining->retired"
                    .to_string(),
            ));
        }
        self.append_jsonl_unlocked("execution_nodes.jsonl", next)
    }

    pub fn register_node_project(
        &self,
        value: &NodeProjectRegistration,
        execution_space_id: &str,
    ) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        require_non_empty_store(execution_space_id, "Execution Space id")?;
        if value.execution_space_id != execution_space_id {
            return Err(StoreError::Conflict(format!(
                "EXECUTION_SPACE_SCOPE_MISMATCH: registration names {}, selected Store is {execution_space_id}",
                value.execution_space_id
            )));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let node = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(&value.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("NODE_NOT_ACTIVE: {} not found", value.node_id))
        })?;
        if node.status != ExecutionNodeStatus::Active {
            return Err(StoreError::Conflict(format!(
                "NODE_NOT_ACTIVE: {} is {:?}",
                node.id, node.status
            )));
        }
        let key = node_project_registration_identity(value);
        let registrations = latest_by_id(
            self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
            node_project_registration_identity,
        );
        if let Some(current) = registrations.get(&key) {
            if current == value {
                return Ok(());
            }
            if current.created_at != value.created_at {
                return Err(StoreError::Conflict(format!(
                    "node project registration identity already exists: {key}"
                )));
            }
        }
        self.append_jsonl_unlocked("node_project_registrations.jsonl", value)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn acquire_node_daemon_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        instance_id: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let node = latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_NOT_ACTIVE: {node_id} not found")))?;
        if node.status == ExecutionNodeStatus::Retired {
            return Err(StoreError::Conflict(format!(
                "NODE_NOT_ACTIVE: {node_id} is retired"
            )));
        }
        let current = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id);
        if let Some(current) = current.as_ref() {
            if current.status == NodeDaemonLeaseStatus::Active
                && current.expires_unix_ms > now_unix_ms
            {
                if current.daemon_id == daemon_id && current.instance_id == instance_id {
                    return Ok(current.clone());
                }
                return Err(StoreError::Conflict(format!(
                    "NODE_DAEMON_LEASE_HELD: Node {node_id} is held by {} generation {}",
                    current.daemon_id, current.generation
                )));
            }
        }
        let generation = current
            .as_ref()
            .map(|lease| lease.generation.saturating_add(1))
            .unwrap_or(1);
        let lease = NodeDaemonLease {
            node_id: node_id.to_string(),
            daemon_id: daemon_id.to_string(),
            generation,
            instance_id: instance_id.to_string(),
            status: NodeDaemonLeaseStatus::Active,
            acquired_unix_ms: now_unix_ms,
            renewed_unix_ms: now_unix_ms,
            expires_unix_ms: now_unix_ms.saturating_add(ttl_ms.max(1)),
            released_unix_ms: None,
        };
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn renew_node_daemon_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_DAEMON_GENERATION_FENCED: {node_id}")))?;
        if lease.status != NodeDaemonLeaseStatus::Active
            || lease.daemon_id != daemon_id
            || lease.generation != generation
            || lease.instance_id != instance_id
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_GENERATION_FENCED: {daemon_id} generation {generation} no longer owns Node {node_id}"
            )));
        }
        lease.renewed_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms.saturating_add(ttl_ms.max(1));
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn release_node_daemon_lease(
        &self,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: &str,
        now_unix_ms: u64,
    ) -> StoreResult<NodeDaemonLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| StoreError::Conflict(format!("NODE_DAEMON_GENERATION_FENCED: {node_id}")))?;
        if lease.daemon_id != daemon_id
            || lease.generation != generation
            || lease.instance_id != instance_id
        {
            return Err(StoreError::Conflict(format!(
                "NODE_DAEMON_GENERATION_FENCED: stale daemon cannot release Node {node_id}"
            )));
        }
        if lease.status == NodeDaemonLeaseStatus::Released {
            return Ok(lease);
        }
        lease.status = NodeDaemonLeaseStatus::Released;
        lease.renewed_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms;
        lease.released_unix_ms = Some(now_unix_ms);
        self.append_jsonl_unlocked("node_daemon_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Acquire the one durable Supervisor lease for a TeamRun. An active,
    /// unexpired lease held by another Supervisor rejects the attach before any
    /// provider side effect. Reacquisition after expiry increments generation.
    #[allow(clippy::too_many_arguments)]
    pub fn acquire_team_supervisor_under_node_lease(
        &self,
        team_run_id: &str,
        node_id: &str,
        node_daemon_id: &str,
        node_daemon_generation: u64,
        execution_space_id: &str,
        project_binding_id: &str,
        supervisor_id: &str,
        owner_process_id: u32,
        owner_locator: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let run = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(team_run_id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {team_run_id}")))?;
        if run.execution_node_id != node_id || run.project_binding_id != project_binding_id {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_NODE_MISMATCH: TeamRun {team_run_id} is bound to Node {} / project {}, not {node_id} / {project_binding_id}",
                run.execution_node_id, run.project_binding_id
            )));
        }
        let parent = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: Node {node_id} has no NodeDaemon lease"
            ))
        })?;
        if parent.status != NodeDaemonLeaseStatus::Active
            || parent.daemon_id != node_daemon_id
            || parent.generation != node_daemon_generation
            || parent.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: NodeDaemon {node_daemon_id} generation {node_daemon_generation} is not the active parent for Node {node_id}"
            )));
        }
        let current = self.latest_lease_for_run_unlocked(team_run_id)?;
        if let Some(current) = current.as_ref() {
            if current.status == TeamSupervisorLeaseStatus::Active
                && current.expires_unix_ms > now_unix_ms
                && current.supervisor_id != supervisor_id
            {
                return Err(StoreError::Conflict(format!(
                    "team run {team_run_id} is supervised by {} generation {} until unix-ms:{}",
                    current.supervisor_id, current.generation, current.expires_unix_ms
                )));
            }
            if current.status == TeamSupervisorLeaseStatus::Active
                && current.expires_unix_ms > now_unix_ms
                && current.supervisor_id == supervisor_id
            {
                return Ok(current.clone());
            }
        }
        let generation = current
            .as_ref()
            .map(|lease| lease.generation.saturating_add(1))
            .unwrap_or(1);
        let lease = TeamSupervisorLease {
            team_run_id: team_run_id.to_string(),
            node_id: node_id.to_string(),
            node_daemon_id: node_daemon_id.to_string(),
            node_daemon_generation,
            execution_space_id: execution_space_id.to_string(),
            project_binding_id: project_binding_id.to_string(),
            supervisor_id: supervisor_id.to_string(),
            generation,
            owner_process_id,
            owner_locator: owner_locator.to_string(),
            status: TeamSupervisorLeaseStatus::Active,
            acquired_unix_ms: now_unix_ms,
            heartbeat_unix_ms: now_unix_ms,
            expires_unix_ms: now_unix_ms.saturating_add(ttl_ms.max(1)),
            released_unix_ms: None,
        };
        // Acquisition is rare (one per Supervisor generation) while heartbeats
        // are ~1/s, so this is where compaction belongs.
        self.compact_supervisor_leases_unlocked()?;
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn renew_team_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        generation: u64,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no Supervisor lease to renew"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "Supervisor lease for team run {team_run_id} is no longer owned by {supervisor_id} generation {generation}"
            )));
        }
        let parent = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |parent| parent.node_id.clone(),
        )
        .remove(&lease.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: Node {} has no active parent",
                lease.node_id
            ))
        })?;
        if parent.status != NodeDaemonLeaseStatus::Active
            || parent.daemon_id != lease.node_daemon_id
            || parent.generation != lease.node_daemon_generation
            || parent.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: parent NodeDaemon generation is no longer active for TeamRun {team_run_id}"
            )));
        }
        lease.heartbeat_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms.saturating_add(ttl_ms.max(1));
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn release_team_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        generation: u64,
        now_unix_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no Supervisor lease to release"
            ))
        })?;
        if lease.supervisor_id != supervisor_id || lease.generation != generation {
            return Err(StoreError::Conflict(format!(
                "Supervisor lease for team run {team_run_id} belongs to {} generation {}, not {supervisor_id} generation {generation}",
                lease.supervisor_id, lease.generation
            )));
        }
        if lease.status == TeamSupervisorLeaseStatus::Released {
            return Ok(lease);
        }
        lease.status = TeamSupervisorLeaseStatus::Released;
        lease.heartbeat_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms;
        lease.released_unix_ms = Some(now_unix_ms);
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Persist a Host Close before touching the process-local provider handle.
    /// Repeated requests while one is pending are idempotent.
    pub fn latch_team_member_close(
        &self,
        value: &TeamMemberCloseRequest,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let member = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&value.member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection not found: {}",
                value.member_run_id
            ))
        })?;
        if member.team_run_id != value.team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} belongs to {}, not {}",
                value.member_run_id, member.team_run_id, value.team_run_id
            )));
        }
        if let Some(current) = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(&value.member_run_id)
        {
            if current.status == TeamMemberCloseStatus::Pending {
                return Ok(current);
            }
        }
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", value)?;
        Ok(value.clone())
    }

    /// Persist a Host Close only while the named Supervisor generation and
    /// its parent NodeDaemon generation still hold current durable authority.
    ///
    /// The child/parent lease checks and the Close append share the Store
    /// writer lock with lease renewal, release, and successor acquisition.
    /// This is the live-control admission linearization point: a stale
    /// generation can never pass an optimistic lease read and append a Close
    /// after another generation has taken over.
    pub fn latch_team_member_close_for_supervisor(
        &self,
        value: &TeamMemberCloseRequest,
        supervisor_id: &str,
        supervisor_generation: u64,
        now_unix_ms: u64,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(&value.team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TEAM_SUPERVISOR_LEASE_LOST: TeamRun {} has no Supervisor lease",
                    value.team_run_id
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_LEASE_LOST: TeamRun {} is not owned by {supervisor_id} generation {supervisor_generation}",
                value.team_run_id
            )));
        }
        let parent = latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |parent| parent.node_id.clone(),
        )
        .remove(&lease.node_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: Node {} has no active parent",
                lease.node_id
            ))
        })?;
        if parent.status != NodeDaemonLeaseStatus::Active
            || parent.daemon_id != lease.node_daemon_id
            || parent.generation != lease.node_daemon_generation
            || parent.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "TEAM_SUPERVISOR_PARENT_FENCED: parent NodeDaemon generation is no longer active for TeamRun {}",
                value.team_run_id
            )));
        }
        let member = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&value.member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection not found: {}",
                value.member_run_id
            ))
        })?;
        if member.team_run_id != value.team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} belongs to {}, not {}",
                value.member_run_id, member.team_run_id, value.team_run_id
            )));
        }
        if let Some(current) = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(&value.member_run_id)
        {
            if current.status == TeamMemberCloseStatus::Pending {
                return Ok(current);
            }
        }
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", value)?;
        Ok(value.clone())
    }

    /// Persist a Host Close only when no current Supervisor generation owns
    /// the TeamRun. The absence check and Close latch share the Store write
    /// lock with Supervisor acquisition, closing the race where a successor
    /// generation could acquire authority after a caller observed no lease
    /// but before the durable Close became visible.
    ///
    /// A successor that acquires after this method returns will observe the
    /// pending Close at the pre-provider-spawn fence and must not start the
    /// member. A generation that acquires first makes this method fail closed
    /// so the caller can route control through that exact live owner.
    pub fn latch_team_member_close_without_current_supervisor(
        &self,
        value: &TeamMemberCloseRequest,
        now_unix_ms: u64,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(lease) = self.latest_lease_for_run_unlocked(&value.team_run_id)? {
            if lease.status == TeamSupervisorLeaseStatus::Active
                && lease.expires_unix_ms > now_unix_ms
            {
                return Err(StoreError::Conflict(format!(
                    "TEAM_SUPERVISOR_LEASE_CURRENT: TeamRun {} is owned by {} generation {} until {}",
                    value.team_run_id,
                    lease.supervisor_id,
                    lease.generation,
                    lease.expires_unix_ms
                )));
            }
        }
        let member = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&value.member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection not found: {}",
                value.member_run_id
            ))
        })?;
        if member.team_run_id != value.team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} belongs to {}, not {}",
                value.member_run_id, member.team_run_id, value.team_run_id
            )));
        }
        if let Some(current) = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(&value.member_run_id)
        {
            if current.status == TeamMemberCloseStatus::Pending {
                return Ok(current);
            }
        }
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", value)?;
        Ok(value.clone())
    }

    /// Mark one durable Close as applied after the ProviderRuntimeProjection is stopped.
    pub fn complete_team_member_close(
        &self,
        team_run_id: &str,
        member_run_id: &str,
        request_id: &str,
        applied_at: &str,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut request = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection {member_run_id} has no durable Close request"
            ))
        })?;
        if request.team_run_id != team_run_id || request.id != request_id {
            return Err(StoreError::Conflict(format!(
                "Close request {request_id} does not own ProviderRuntimeProjection {member_run_id} in TeamRun {team_run_id}"
            )));
        }
        if request.status == TeamMemberCloseStatus::Applied {
            return Ok(request);
        }
        request.status = TeamMemberCloseStatus::Applied;
        request.applied_at = Some(applied_at.to_string());
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", &request)?;
        Ok(request)
    }

    /// Claim one queued ProviderDispatchEnvelope delivery under the same durable lock used
    /// for the Supervisor lease. A claim must be completed with a real provider
    /// receipt or explicitly reconciled; it is never auto-requeued on expiry.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        now_unix_ms: u64,
        claim_ttl_ms: u64,
        updated_at: &str,
    ) -> StoreResult<TeamMessageDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut message = match latest_by_id(
            self.read_jsonl::<ProviderDispatchEnvelope>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        {
            Some(message) if message.team_run_id == team_run_id => message,
            _ => return Ok(TeamMessageDeliveryClaimResult::NotQueued),
        };
        if message.kind == ProviderDispatchIntent::ProviderInteractionResponse {
            let body = ProviderInteractionResponseBody::parse_canonical_json(&message.body)
                .map_err(StoreError::Conflict)?;
            let member = self.require_member_run_unlocked(&body.member, team_run_id)?;
            let same_live_generation = member.coordination_is_active()
                && member.runtime_generation == body.generation
                && member
                    .native_session
                    .as_ref()
                    .is_some_and(|native| native.native_session_id == body.session);
            if !same_live_generation {
                return Err(StoreError::Conflict(format!(
                    "provider interaction response is stale for ProviderRuntimeProjection {} generation/session",
                    body.member
                )));
            }
        }
        let Some(delivery) = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
        else {
            return Ok(TeamMessageDeliveryClaimResult::NotQueued);
        };
        if delivery.status != TeamDeliveryStatus::Queued {
            return Ok(TeamMessageDeliveryClaimResult::NotQueued);
        }
        delivery.status = TeamDeliveryStatus::Claimed;
        delivery.attempt = delivery.attempt.saturating_add(1);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_by_supervisor_id = Some(supervisor_id.to_string());
        delivery.claimed_generation = Some(supervisor_generation);
        delivery.claimed_unix_ms = Some(now_unix_ms);
        delivery.claim_expires_unix_ms = Some(now_unix_ms.saturating_add(claim_ttl_ms.max(1)));
        delivery.provider_receipt_id = None;
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(TeamMessageDeliveryClaimResult::Claimed(Box::new(message)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_team_message_delivery_claim(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<ProviderDispatchEnvelope> {
        if provider_receipt_id.trim().is_empty() {
            return Err(StoreError::Conflict(
                "provider receipt id is required to complete a ProviderDispatchEnvelope delivery"
                    .to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut message = latest_by_id(
            self.read_jsonl::<ProviderDispatchEnvelope>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;
        if delivery.status == TeamDeliveryStatus::Delivered
            && delivery.claim_id.as_deref() == Some(claim_id)
        {
            if delivery.provider_receipt_id.as_deref() == Some(provider_receipt_id) {
                return Ok(message);
            }
            return Err(StoreError::Conflict(format!(
                "delivery claim {claim_id} for message {message_id} was already completed with a different provider receipt"
            )));
        }
        if delivery.status != TeamDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_by_supervisor_id.as_deref() != Some(supervisor_id)
            || delivery.claimed_generation != Some(supervisor_generation)
        {
            return Err(StoreError::Conflict(format!(
                "delivery claim {claim_id} no longer owns message {message_id} for {member_run_id}"
            )));
        }
        delivery.status = TeamDeliveryStatus::Delivered;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
    }

    /// Atomically acknowledge one already-delivered ProviderDispatchEnvelope recipient.
    ///
    /// ACK does not require a live Supervisor because the Host or operator may
    /// read and acknowledge mail while the provider runtime is idle or down.
    /// It does require a real delivered receipt and never advances a queued or
    /// uncertain claim.
    pub fn acknowledge_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        updated_at: &str,
    ) -> StoreResult<ProviderDispatchEnvelope> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut message = latest_by_id(
            self.read_jsonl::<ProviderDispatchEnvelope>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;
        match delivery.status {
            TeamDeliveryStatus::Acknowledged => return Ok(message),
            TeamDeliveryStatus::Delivered => {}
            TeamDeliveryStatus::Queued | TeamDeliveryStatus::Claimed => {
                return Err(StoreError::Conflict(format!(
                    "message {message_id} has not been delivered to {member_run_id}"
                )));
            }
            TeamDeliveryStatus::Failed | TeamDeliveryStatus::Expired => {
                return Err(StoreError::Conflict(format!(
                    "message {message_id} delivery to {member_run_id} cannot be acknowledged from {:?}",
                    delivery.status
                )));
            }
        }
        delivery.status = TeamDeliveryStatus::Acknowledged;
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
    }

    /// Resolve a claimed delivery after a crash. `provider_accepted=true`
    /// records a reviewed native receipt; false explicitly returns it to the
    /// queue. No automatic timeout path calls this method.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_team_message_delivery_claim(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        claim_id: &str,
        provider_accepted: bool,
        provider_receipt_id: Option<&str>,
        updated_at: &str,
    ) -> StoreResult<ProviderDispatchEnvelope> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut message = latest_by_id(
            self.read_jsonl::<ProviderDispatchEnvelope>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;
        if delivery.status != TeamDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
        {
            return Err(StoreError::Conflict(format!(
                "message {message_id} does not have active claim {claim_id} for {member_run_id}"
            )));
        }
        if provider_accepted {
            let receipt = provider_receipt_id.ok_or_else(|| {
                StoreError::Conflict(
                    "provider-accepted reconciliation requires a native receipt id".to_string(),
                )
            })?;
            delivery.status = TeamDeliveryStatus::Delivered;
            delivery.provider_receipt_id = Some(receipt.to_string());
        } else {
            delivery.status = TeamDeliveryStatus::Queued;
            delivery.claim_id = None;
            delivery.claimed_by_supervisor_id = None;
            delivery.claimed_generation = None;
            delivery.claimed_unix_ms = None;
            delivery.claim_expires_unix_ms = None;
            delivery.provider_receipt_id = None;
        }
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
    }

    /// Fail a ProviderDispatchEnvelope delivery that can never be completed because the
    /// target member has stopped / failed / been retired.
    ///
    /// Transitions from `Queued` (pre-bind failure) or `Claimed` (transport
    /// disconnect) to `Failed`. A delivery already at `Failed` with the same
    /// reason is idempotent.
    #[allow(clippy::too_many_arguments)]
    pub fn fail_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        reason: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<ProviderDispatchEnvelope> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "ProviderDispatchEnvelope delivery failure reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no active Supervisor lease"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not current"
            )));
        }

        let mut message = latest_by_id(
            self.read_jsonl::<ProviderDispatchEnvelope>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;

        // Idempotent: already failed with same reason.
        if delivery.status == TeamDeliveryStatus::Failed {
            if delivery
                .failure_reason
                .as_deref()
                .is_some_and(|existing| existing == reason)
            {
                return Ok(message);
            }
            return Err(StoreError::Conflict(format!(
                "message {message_id} delivery for {member_run_id} was already failed with a different reason"
            )));
        }

        // Allowed transitions: Queued→Failed (pre-bind), Claimed→Failed
        // (post-bind / transport disconnect).
        match delivery.status {
            TeamDeliveryStatus::Queued => {}
            TeamDeliveryStatus::Claimed => {
                // Only the owning Supervisor generation may fail its own claim.
                if delivery.claimed_by_supervisor_id.as_deref() != Some(supervisor_id)
                    || delivery.claimed_generation != Some(supervisor_generation)
                {
                    return Err(StoreError::Conflict(format!(
                        "message {message_id} delivery for {member_run_id} was claimed by a different Supervisor generation"
                    )));
                }
            }
            _ => {
                return Err(StoreError::Conflict(format!(
                    "message {message_id} delivery for {member_run_id} is already {:?}",
                    delivery.status
                )));
            }
        }

        delivery.status = TeamDeliveryStatus::Failed;
        delivery.claim_id = None;
        delivery.claimed_by_supervisor_id = None;
        delivery.claimed_generation = None;
        delivery.claimed_unix_ms = None;
        delivery.claim_expires_unix_ms = None;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = Some(reason.to_string());
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
    }

    pub fn append_member_action(&self, value: &MemberAction) -> StoreResult<()> {
        if value.action_type == "provider_control" {
            return Err(StoreError::Conflict(
                "PROVIDER_CONTROL_RAW_APPEND_FORBIDDEN: use append_member_action_if_member_run_current"
                    .to_string(),
            ));
        }
        self.append_jsonl("member_actions.jsonl", value)
    }

    /// Append a provider/control receipt only while the exact ProviderRuntimeProjection
    /// generation and native-session snapshot observed by the caller remains
    /// current. The full-row equality check intentionally binds generation and
    /// session without copying those runtime fields into `MemberAction`.
    ///
    /// Returns true only for the call that appended. Exact action-id retries,
    /// and the bounded provider-control receipt key
    /// `(member_run_id, action_type, title)`, converge to false under the same
    /// global lock. Lifecycle CAS and receipt append therefore cannot cross a
    /// check/append gap.
    pub fn append_member_action_if_member_run_current(
        &self,
        expected_member: &ProviderRuntimeProjection,
        action: &MemberAction,
    ) -> StoreResult<bool> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(
            self.read_jsonl::<ProviderRuntimeProjection>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&expected_member.id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "ProviderRuntimeProjection not found: {}",
                expected_member.id
            ))
        })?;
        if &current != expected_member {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} changed concurrently; provider receipt was not appended",
                expected_member.id
            )));
        }
        if !member_is_active_reviewer_runtime(&current) || current.native_session.is_none() {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} is not active in a native session; provider receipt was not appended",
                current.id
            )));
        }
        let run = self.require_team_run_unlocked(&current.team_run_id)?;
        if !run
            .member_run_ids
            .iter()
            .any(|member| member == &current.id)
        {
            return Err(StoreError::Conflict(format!(
                "ProviderRuntimeProjection {} is not admitted to TeamRun {}",
                current.id, current.team_run_id
            )));
        }
        if action.team_run_id != current.team_run_id || action.member_run_id != current.id {
            return Err(StoreError::Conflict(format!(
                "MemberAction {} is not bound to ProviderRuntimeProjection {} in TeamRun {}",
                action.id, current.id, current.team_run_id
            )));
        }
        let actions = self.read_jsonl::<MemberAction>("member_actions.jsonl")?;
        if let Some(existing) = actions.iter().find(|existing| existing.id == action.id) {
            if existing == action {
                return Ok(false);
            }
            return Err(StoreError::Conflict(format!(
                "MemberAction id already exists with different semantics: {}",
                action.id
            )));
        }
        if action.action_type == "provider_control"
            && actions.iter().any(|existing| {
                existing.member_run_id == action.member_run_id
                    && existing.action_type == action.action_type
                    && existing.title == action.title
            })
        {
            return Ok(false);
        }
        self.append_jsonl_unlocked("member_actions.jsonl", action)?;
        Ok(true)
    }

    pub fn append_pending_interaction(&self, value: &PendingInteraction) -> StoreResult<()> {
        self.append_jsonl("pending_interactions.jsonl", value)
    }

    pub fn append_delegation_run(&self, value: &DelegationRun) -> StoreResult<()> {
        self.append_jsonl("delegation_runs.jsonl", value)
    }

    pub fn append_team_run_event(&self, value: &TeamRunEvent) -> StoreResult<()> {
        self.append_jsonl("team_run_events.jsonl", value)
    }

    /// Allocate and append the next per-TeamRun event sequence under one store
    /// lock so concurrent HTTP/MCP/provider writers cannot duplicate `seq`.
    pub fn append_team_run_event_next(&self, mut value: TeamRunEvent) -> StoreResult<TeamRunEvent> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        value.seq = self
            .read_jsonl::<TeamRunEvent>("team_run_events.jsonl")?
            .into_iter()
            .filter(|event| event.team_run_id == value.team_run_id)
            .map(|event| event.seq)
            .max()
            .unwrap_or(0)
            + 1;
        self.append_jsonl_unlocked("team_run_events.jsonl", &value)?;
        Ok(value)
    }

    /// Idempotently append one semantic TeamRun event under the store lock.
    pub fn ensure_team_run_event_next(
        &self,
        stable_key: &str,
        mut value: TeamRunEvent,
    ) -> StoreResult<TeamRunEvent> {
        require_non_empty_store(stable_key, "TeamRun event stable key")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        value.id = format!("trev-stable-{}", content_hash_hex16(stable_key));
        let events = self.read_jsonl::<TeamRunEvent>("team_run_events.jsonl")?;
        if let Some(existing) = events.iter().find(|event| event.id == value.id) {
            if same_team_run_event_semantics(existing, &value) {
                return Ok(existing.clone());
            }
            return Err(StoreError::Conflict(format!(
                "TeamRunEvent id {} already names different causal semantics",
                value.id
            )));
        }
        value.seq = events
            .iter()
            .filter(|event| event.team_run_id == value.team_run_id)
            .map(|event| event.seq)
            .max()
            .unwrap_or(0)
            + 1;
        self.append_jsonl_unlocked("team_run_events.jsonl", &value)?;
        Ok(value)
    }

    /// Compare-and-append a TeamRun lifecycle row. Mission and Node authority
    /// are reached through the immutable AgentTeam relation and are never
    /// copied or updated by a run transition.
    pub fn compare_and_append_team_run_lifecycle(
        &self,
        expected: &AgentTeamRun,
        next: &AgentTeamRun,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "team run {} changed concurrently or is no longer startable",
                expected.id
            )));
        }
        if next.member_run_ids != current.member_run_ids {
            return Err(StoreError::Conflict(
                "TEAM_MEMBERSHIP_REQUIRES_ADMISSION: lifecycle revision cannot change member_run_ids; use admit_member_run"
                    .to_string(),
            ));
        }
        let mut allowed_lifecycle = current.clone();
        allowed_lifecycle.status = next.status;
        allowed_lifecycle.updated_at = next.updated_at.clone();
        allowed_lifecycle.completed_at = next.completed_at.clone();
        if *next != allowed_lifecycle {
            return Err(StoreError::Conflict(
                "TEAM_RUN_LIFECYCLE_SCOPE_IMMUTABLE: lifecycle CAS may only change status, updated_at, and completed_at"
                    .to_string(),
            ));
        }
        if next.status == TeamRunStatus::Completed {
            let unfinished = self
                .latest_works_unlocked()?
                .into_values()
                .filter(|work| work.team_run_id == next.id && !work.is_terminal())
                .collect::<Vec<_>>();
            if !unfinished.is_empty() {
                let detail = unfinished
                    .iter()
                    .map(|work| {
                        let phase = serde_json::to_string(&work.phase)
                            .unwrap_or_else(|_| format!("{:?}", work.phase));
                        let condition = serde_json::to_string(&work.condition)
                            .unwrap_or_else(|_| format!("{:?}", work.condition));
                        format!(
                            "{} ({}/{}, version {})",
                            work.id,
                            phase.trim_matches('"'),
                            condition.trim_matches('"'),
                            work.version
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(StoreError::Conflict(format!(
                    "team run {} cannot complete while Works remain non-terminal: {detail}; accept or cancel every Work first",
                    next.id
                )));
            }
        }

        self.append_jsonl_unlocked("team_runs.jsonl", next)?;
        Ok(())
    }

    pub fn claim_queued_message_delivery(
        &self,
        agent_member_id: &str,
        message_id: &str,
        delivery: RegistryDeliveryAttempt,
    ) -> StoreResult<MessageDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;

        let latest_messages = latest_by_id(
            self.read_jsonl::<RegistryMessage>("messages.jsonl")?,
            |message| message.id.clone(),
        );
        if let Some(active) = latest_messages.values().find(|message| {
            message.to_agent_id.as_deref() == Some(agent_member_id)
                && message
                    .delivery
                    .as_ref()
                    .is_some_and(delivery_blocks_another_claim)
        }) {
            let delivery_id = active
                .delivery
                .as_ref()
                .and_then(|delivery| delivery.delivery_id.clone())
                .unwrap_or_else(|| active.id.clone());
            return Ok(MessageDeliveryClaimResult::BlockedByDelivery(delivery_id));
        }
        let Some(mut message) = latest_messages.get(message_id).cloned() else {
            return Ok(MessageDeliveryClaimResult::NotQueued);
        };
        if message.to_agent_id.as_deref() != Some(agent_member_id)
            || message.delivery_status != RegistryDeliveryStatus::Queued
        {
            return Ok(MessageDeliveryClaimResult::NotQueued);
        }

        message.delivery_status = RegistryDeliveryStatus::Acknowledged;
        message.delivery = Some(delivery);
        self.append_jsonl_unlocked("messages.jsonl", &message)?;

        Ok(MessageDeliveryClaimResult::Claimed(Box::new(message)))
    }

    /// Raw append-only Mission ledger rows, in append order.
    pub fn missions(&self) -> StoreResult<Vec<Mission>> {
        self.read_jsonl("missions.jsonl")
    }

    /// Latest-row-wins Mission projection, ordered by id for deterministic
    /// dashboard/API consumers.
    pub fn latest_missions(&self) -> StoreResult<Vec<Mission>> {
        Ok(latest_by_id(self.missions()?, |mission| mission.id.clone())
            .into_values()
            .collect())
    }

    /// Raw append-only Wave ledger rows, in append order.
    pub fn waves(&self) -> StoreResult<Vec<Wave>> {
        self.read_jsonl("waves.jsonl")
    }

    /// Latest-row-wins Wave projection, ordered by Mission then Wave index for
    /// deterministic product reads. The id is a final tie-breaker for corrupt
    /// legacy rows; native authoring rejects duplicate Mission/index pairs.
    pub fn latest_waves(&self) -> StoreResult<Vec<Wave>> {
        let mut waves = latest_by_id(self.waves()?, |wave| wave.id.clone())
            .into_values()
            .collect::<Vec<_>>();
        waves.sort_by(|left, right| {
            left.mission_id
                .cmp(&right.mission_id)
                .then(left.index.cmp(&right.index))
                .then(left.id.cmp(&right.id))
        });
        Ok(waves)
    }

    /// Raw append-only Mission Log rows across every Mission, in append
    /// order. Prefer [`Self::mission_log_entries`] when scoping to one
    /// Mission; this is here for parity with `waves()`/`missions()`.
    pub fn mission_log(&self) -> StoreResult<Vec<MissionLogEntry>> {
        self.read_jsonl("mission_log.jsonl")
    }

    /// Every [`MissionLogEntry`] for one Mission, ordered by `revision`
    /// ascending. There is no latest-wins collapse: unlike Wave/Mission the
    /// Log has no mutable identity, every row is a permanent entry.
    pub fn mission_log_entries(&self, mission_id: &str) -> StoreResult<Vec<MissionLogEntry>> {
        let mut entries = self
            .mission_log()?
            .into_iter()
            .filter(|entry| entry.mission_id == mission_id)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.revision);
        Ok(entries)
    }

    /// The last `n` [`MissionLogEntry`] rows for one Mission, oldest-first
    /// within the returned slice (Unix `tail` ordering) so a reader sees them
    /// in the order they were written. Returns fewer than `n` rows if the
    /// Mission has fewer entries, and an empty Vec if it has none yet.
    pub fn mission_log_tail(
        &self,
        mission_id: &str,
        n: usize,
    ) -> StoreResult<Vec<MissionLogEntry>> {
        let entries = self.mission_log_entries(mission_id)?;
        let start = entries.len().saturating_sub(n);
        Ok(entries[start..].to_vec())
    }

    pub fn members(&self) -> StoreResult<Vec<ProviderLaunchProfile>> {
        self.read_jsonl("provider_launch_profiles.jsonl")
    }

    /// Raw append-only compatibility admission rows in causal order.
    /// Invalid JSON or semantically invalid rows fail the entire read closed.
    pub fn provider_compatibility_admissions(
        &self,
    ) -> StoreResult<Vec<ProviderCompatibilityAdmission>> {
        let rows: Vec<ProviderCompatibilityAdmission> =
            self.read_jsonl(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER)?;
        for row in &rows {
            row.validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
        }
        validate_provider_compatibility_admission_ledger(&rows)?;
        Ok(rows)
    }

    /// Latest-row-wins projection by the exact four-part compatibility key.
    pub fn latest_provider_compatibility_admissions(
        &self,
    ) -> StoreResult<Vec<ProviderCompatibilityAdmission>> {
        let mut latest = std::collections::BTreeMap::new();
        for row in self.provider_compatibility_admissions()? {
            latest.insert(
                (
                    row.project_id.clone(),
                    row.store_id.clone(),
                    row.provider.clone(),
                    row.execution_mode.clone(),
                    row.provider_version.clone(),
                    row.adapter_contract_version.clone(),
                ),
                row,
            );
        }
        Ok(latest.into_values().collect())
    }

    /// Return the active admission for one exact tuple. Terminal latest rows,
    /// other execution modes, and other contract versions never authorize it.
    pub fn effective_provider_compatibility_admission(
        &self,
        provider: &str,
        execution_mode: &str,
        provider_version: &str,
        adapter_contract_version: &str,
    ) -> StoreResult<Option<ProviderCompatibilityAdmission>> {
        let (project_id, store_id) = self.require_provider_compatibility_scope()?;
        Ok(self
            .provider_compatibility_admissions()?
            .into_iter()
            .rev()
            .find(|row| {
                row.project_id == project_id
                    && row.store_id == store_id
                    && row.exact_key()
                        == (
                            provider,
                            execution_mode,
                            provider_version,
                            adapter_contract_version,
                        )
            })
            .filter(ProviderCompatibilityAdmission::is_active))
    }

    pub fn teams(&self) -> StoreResult<Vec<AgentTeam>> {
        self.read_jsonl("teams.jsonl")
    }

    /// Latest-row-wins AgentTeam projection keyed by team id. This is the
    /// input for recursive topology validation and queries (ADR 0052).
    pub fn latest_teams(&self) -> StoreResult<std::collections::BTreeMap<String, AgentTeam>> {
        Ok(latest_by_id(self.teams()?, |team| team.id.clone()))
    }

    pub fn runtimes(&self) -> StoreResult<Vec<ProviderProcess>> {
        self.read_jsonl("provider_processes.jsonl")
    }

    pub fn events(&self) -> StoreResult<Vec<ProviderDispatchEvent>> {
        self.read_jsonl("provider_dispatch_events.jsonl")
    }

    pub fn proposals(&self) -> StoreResult<Vec<Proposal>> {
        self.read_jsonl("proposals.jsonl")
    }

    pub fn messages(&self) -> StoreResult<Vec<RegistryMessage>> {
        self.read_jsonl("messages.jsonl")
    }

    pub fn evidence(&self) -> StoreResult<Vec<Evidence>> {
        self.read_jsonl("evidence.jsonl")
    }

    pub fn decisions(&self) -> StoreResult<Vec<Decision>> {
        self.read_jsonl("decisions.jsonl")
    }

    pub fn reviews(&self) -> StoreResult<Vec<Review>> {
        self.read_jsonl("reviews.jsonl")
    }

    pub fn gaps(&self) -> StoreResult<Vec<Gap>> {
        self.read_jsonl("gaps.jsonl")
    }

    pub fn visions(&self) -> StoreResult<Vec<Vision>> {
        self.read_jsonl("visions.jsonl")
    }

    pub fn provider_child_threads(&self) -> StoreResult<Vec<ProviderChildThread>> {
        self.read_jsonl("provider_child_threads.jsonl")
    }

    pub fn workflow_runs(&self) -> StoreResult<Vec<WorkflowRun>> {
        self.read_jsonl("workflow_runs.jsonl")
    }

    pub fn workflow_steps(&self) -> StoreResult<Vec<WorkflowStep>> {
        self.read_jsonl("workflow_steps.jsonl")
    }

    pub fn workflow_patches(&self) -> StoreResult<Vec<WorkflowPatch>> {
        self.read_jsonl("workflow_patches.jsonl")
    }

    pub fn workflow_artifact_manifests(&self) -> StoreResult<Vec<WorkflowArtifactManifest>> {
        self.read_jsonl("workflow_artifact_manifests.jsonl")
    }

    pub fn team_runs(&self) -> StoreResult<Vec<AgentTeamRun>> {
        self.read_jsonl("team_runs.jsonl")
    }

    pub fn latest_execution_nodes(&self) -> StoreResult<Vec<ExecutionNode>> {
        Ok(latest_by_id(
            self.read_jsonl::<ExecutionNode>("execution_nodes.jsonl")?,
            |node| node.id.clone(),
        )
        .into_values()
        .collect())
    }

    pub fn latest_node_project_registrations(&self) -> StoreResult<Vec<NodeProjectRegistration>> {
        Ok(latest_by_id(
            self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
            node_project_registration_identity,
        )
        .into_values()
        .collect())
    }

    pub fn latest_node_daemon_lease(&self, node_id: &str) -> StoreResult<Option<NodeDaemonLease>> {
        Ok(latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .remove(node_id))
    }

    pub fn latest_node_daemon_leases(&self) -> StoreResult<Vec<NodeDaemonLease>> {
        Ok(latest_by_id(
            self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?,
            |lease| lease.node_id.clone(),
        )
        .into_values()
        .collect())
    }

    pub fn member_runs(&self) -> StoreResult<Vec<ProviderRuntimeProjection>> {
        let rows: Vec<ProviderRuntimeProjection> = self.read_jsonl("member_runs.jsonl")?;
        for row in &rows {
            row.validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
        }
        Ok(rows)
    }

    pub fn team_messages(&self) -> StoreResult<Vec<ProviderDispatchEnvelope>> {
        self.read_jsonl("team_messages.jsonl")
    }

    pub fn work_operations(&self) -> StoreResult<Vec<WorkOperation>> {
        self.work_operations_unlocked()
    }

    pub fn latest_works(&self) -> StoreResult<Vec<Work>> {
        Ok(self.latest_works_unlocked()?.into_values().collect())
    }

    pub fn work_delegation_events(&self) -> StoreResult<Vec<WorkDelegationEvent>> {
        Ok(self
            .all_work_delegation_revisions_unlocked()?
            .into_iter()
            .map(|revision| revision.event)
            .collect())
    }

    pub fn latest_work_delegations(&self) -> StoreResult<Vec<WorkDelegation>> {
        Ok(self
            .latest_work_delegations_unlocked()?
            .into_values()
            .collect())
    }

    /// Fold target Work state into Delegation state without changing the
    /// source Work. Repeated reconciliation is idempotent when no state change
    /// is required.
    pub fn transition_work_and_roll_up_delegation(
        &self,
        target_work_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Vec<WorkDelegation>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let target = self
            .latest_works_unlocked()?
            .remove(target_work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {target_work_id}")))?;
        let revisions = self.work_delegation_rollup_revisions_unlocked(&target, &context)?;
        let mut changed = Vec::new();
        for revision in revisions {
            let current = self
                .latest_work_delegations_unlocked()?
                .remove(&revision.delegation.id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "delegation not found: {}",
                        revision.delegation.id
                    ))
                })?;
            changed.push(self.append_work_delegation_transition_unlocked(
                &current,
                revision.delegation,
                revision.event,
            )?);
        }
        Ok(changed)
    }

    pub fn cancel_work_delegation(
        &self,
        delegation_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<WorkDelegation> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "DELEGATION_CANCEL_REASON_REQUIRED".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if let Some(existing) = self
            .all_work_delegation_revisions_unlocked()?
            .into_iter()
            .find(|revision| revision.event.idempotency_key == context.idempotency_key)
        {
            if existing.delegation.id == delegation_id
                && existing.event.transition == WorkDelegationTransition::Cancelled
                && existing.event.payload["reason"].as_str() == Some(reason)
            {
                return Ok(existing.delegation);
            }
            return Err(StoreError::Conflict(format!(
                "IDEMPOTENCY_CONFLICT: key {} already belongs to Delegation event {}",
                context.idempotency_key, existing.event.id
            )));
        }
        let current = self
            .latest_work_delegations_unlocked()?
            .remove(delegation_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("delegation not found: {delegation_id}"))
            })?;
        if current.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "DELEGATION_VERSION_CONFLICT: {} is version {}, expected {expected_version}",
                current.id, current.version
            )));
        }
        match context.performed_by_actor.kind {
            TeamActorKind::Host | TeamActorKind::Operator | TeamActorKind::Service => {}
            TeamActorKind::AgentMember
                if context.performed_by_actor.id == current.source_owner_member_id => {}
            TeamActorKind::ProviderRuntimeProjection => {
                let member = self.require_member_run_unlocked(
                    &context.performed_by_actor.id,
                    &current.source_work_ref.team_run_id,
                )?;
                if member_identity(&member) != current.source_owner_member_id {
                    return Err(StoreError::Conflict(
                        "DELEGATION_NOT_AUTHORIZED: only source owner or Host may cancel"
                            .to_string(),
                    ));
                }
            }
            _ => {
                return Err(StoreError::Conflict(
                    "DELEGATION_NOT_AUTHORIZED: only source owner or Host may cancel".to_string(),
                ))
            }
        }
        let mut next = current.clone();
        next.state = WorkDelegationState::Cancelled;
        next.version = next.version.saturating_add(1);
        next.updated_at = context.created_at.clone();
        next.blocker_reason = None;
        next.resolution_summary = Some(reason.to_string());
        let event = WorkDelegationEvent {
            id: context.event_id,
            delegation_id: current.id.clone(),
            sequence: next.version,
            transition: WorkDelegationTransition::Cancelled,
            expected_version: current.version,
            resulting_version: next.version,
            performed_by_actor: context.performed_by_actor,
            causation_ref: context.causation_ref,
            idempotency_key: context.idempotency_key,
            payload: serde_json::json!({"reason": reason}),
            created_at: context.created_at,
        };
        self.append_work_delegation_transition_unlocked(&current, next, event)
    }

    pub fn work_condition_records(&self) -> StoreResult<Vec<WorkConditionRecord>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.condition_records)
            .collect())
    }

    pub fn work_reports(&self) -> StoreResult<Vec<WorkReport>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.reports)
            .collect())
    }

    pub fn work_evidence(&self) -> StoreResult<Vec<WorkEvidence>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.evidence_records)
            .collect())
    }

    pub fn work_operational_decisions(&self) -> StoreResult<Vec<WorkOperationalDecision>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.decisions)
            .collect())
    }

    pub fn work_events(&self) -> StoreResult<Vec<WorkEvent>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .map(|operation| operation.event)
            .collect())
    }

    pub fn latest_work_deliveries(&self) -> StoreResult<Vec<ProviderWorkDispatch>> {
        Ok(self
            .latest_work_deliveries_unlocked()?
            .into_values()
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_work_delivery(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<WorkDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut deliveries = self.latest_work_deliveries_unlocked()?;
        let Some(mut delivery) = deliveries.remove(delivery_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        if delivery.team_run_id != team_run_id
            || delivery.recipient_member_run_id != member_run_id
            || !matches!(
                delivery.status,
                ProviderWorkDispatchStatus::Queued | ProviderWorkDispatchStatus::Failed
            )
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        let works = self.latest_works_unlocked()?;
        let Some(work) = works.get(&delivery.work_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        // A queued row is only actionable for the newest Work revision and
        // current runtime binding. `Open` is deliberately not required:
        // revisions created by resume/change-request/rebind can be delivered
        // while the Work is in progress, blocked, or under review.
        if work.team_run_id != team_run_id
            || work.version != delivery.work_version
            || work.active_member_run_id.as_deref() != Some(member_run_id)
            || work.is_terminal()
            || !work.prerequisites_satisfied(works.values())
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        // A provider receipt is published as soon as the native runtime
        // accepts a Work prompt. The member may not have executed `work start`
        // yet, so the Work can still be `open` during this hand-off window.
        // Treat that receipted (or still-claimed) Work as occupying the single
        // member execution slot, in addition to explicitly active lifecycle
        // states. A later revision of the *same* Work remains deliverable for
        // resume/change-request; only a different Work is fenced.
        if works.values().any(|other| {
            other.id != work.id
                && other.team_run_id == team_run_id
                && other.active_member_run_id.as_deref() == Some(member_run_id)
                && ((other.phase == WorkPhase::Active)
                    || (other.phase == WorkPhase::Open
                        && other.condition == WorkCondition::Normal
                        && deliveries.values().any(|existing| {
                            existing.work_id == other.id
                                && existing.recipient_member_run_id == member_run_id
                                && matches!(
                                    existing.status,
                                    ProviderWorkDispatchStatus::Claimed
                                        | ProviderWorkDispatchStatus::ProviderReceived
                                )
                        })))
        }) {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        let member = self.require_member_run_unlocked(member_run_id, team_run_id)?;
        if self
            .ensure_member_can_receive_work_unlocked(&member)
            .is_err()
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        delivery.status = ProviderWorkDispatchStatus::Claimed;
        delivery.attempt = delivery.attempt.saturating_add(1);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_by_supervisor_id = Some(supervisor_id.to_string());
        delivery.claimed_generation = Some(supervisor_generation);
        delivery.provider_receipt_id = None;
        delivery.failure_reason = None;
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(WorkDeliveryClaimResult::Claimed(Box::new(delivery)))
    }

    /// Claim a queued ProviderWorkDispatch for a terminal work notification.
    ///
    /// Like [`claim_work_delivery`] but permits terminal (Accepted /
    /// Cancelled) works, skips the prerequisite-satisfied check, and does not
    /// fence on another active work occupying the member slot. A terminal-work
    /// notification is informational (the supervisor turns it into a
    /// ProviderDispatchEnvelope), not an execution assignment.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_work_notification(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<WorkDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut deliveries = self.latest_work_deliveries_unlocked()?;
        let Some(mut delivery) = deliveries.remove(delivery_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        if delivery.team_run_id != team_run_id
            || delivery.recipient_member_run_id != member_run_id
            || !matches!(
                delivery.status,
                ProviderWorkDispatchStatus::Queued | ProviderWorkDispatchStatus::Failed
            )
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        let works = self.latest_works_unlocked()?;
        let Some(work) = works.get(&delivery.work_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        // Terminal works are allowed; the supervisor will turn this delivery
        // into a ProviderDispatchEnvelope, not a work-assignment prompt.
        if work.team_run_id != team_run_id
            || work.version != delivery.work_version
            || work.active_member_run_id.as_deref() != Some(member_run_id)
            || !work.is_terminal()
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        // No slot-occupancy fence: a terminal-work notification never blocks
        // an active execution assignment.
        delivery.status = ProviderWorkDispatchStatus::Claimed;
        delivery.attempt = delivery.attempt.saturating_add(1);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_by_supervisor_id = Some(supervisor_id.to_string());
        delivery.claimed_generation = Some(supervisor_generation);
        delivery.provider_receipt_id = None;
        delivery.failure_reason = None;
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(WorkDeliveryClaimResult::Claimed(Box::new(delivery)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<ProviderWorkDispatch> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not current"
            )));
        }
        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("ProviderWorkDispatch not found: {delivery_id}"))
            })?;
        let owns_claim = delivery.team_run_id == team_run_id
            && delivery.recipient_member_run_id == member_run_id
            && delivery.claim_id.as_deref() == Some(claim_id)
            && delivery.claimed_by_supervisor_id.as_deref() == Some(supervisor_id)
            && delivery.claimed_generation == Some(supervisor_generation);
        if delivery.status == ProviderWorkDispatchStatus::ProviderReceived && owns_claim {
            if delivery.provider_receipt_id.as_deref() != Some(provider_receipt_id) {
                return Err(StoreError::Conflict(format!(
                    "ProviderWorkDispatch claim {claim_id} was already completed with a different provider receipt"
                )));
            }
            return Ok(delivery);
        }
        if !owns_claim
            || delivery.recipient_member_run_id != member_run_id
            || delivery.status != ProviderWorkDispatchStatus::Claimed
        {
            return Err(StoreError::Conflict(format!(
                "ProviderWorkDispatch claim {claim_id} no longer owns {delivery_id}"
            )));
        }
        delivery.status = ProviderWorkDispatchStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: delivery.provider_receipt_id.clone(),
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(delivery)
    }

    /// Fail the currently-owned ProviderWorkDispatch claim. Only the Supervisor that
    /// owns the current, unexpired TeamRun lease and the exact durable claim
    /// may write this terminal delivery outcome. The failure reason is control
    /// evidence, not a copy of provider output.
    #[allow(clippy::too_many_arguments)]
    pub fn fail_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        reason: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<ProviderWorkDispatch> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "ProviderWorkDispatch failure reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no active Supervisor lease"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not current"
            )));
        }

        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("ProviderWorkDispatch not found: {delivery_id}"))
            })?;
        let owns_claim = delivery.team_run_id == team_run_id
            && delivery.recipient_member_run_id == member_run_id
            && delivery.claim_id.as_deref() == Some(claim_id)
            && delivery.claimed_by_supervisor_id.as_deref() == Some(supervisor_id)
            && delivery.claimed_generation == Some(supervisor_generation);
        if delivery.status == ProviderWorkDispatchStatus::Failed && owns_claim {
            if delivery.failure_reason.as_deref() != Some(reason) {
                return Err(StoreError::Conflict(format!(
                    "ProviderWorkDispatch claim {claim_id} was already failed with a different reason"
                )));
            }
            return Ok(delivery);
        }
        if delivery.status != ProviderWorkDispatchStatus::Claimed || !owns_claim {
            return Err(StoreError::Conflict(format!(
                "ProviderWorkDispatch claim {claim_id} no longer owns {delivery_id}"
            )));
        }

        delivery.status = ProviderWorkDispatchStatus::Failed;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = Some(reason.to_string());
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: None,
                failure_reason: delivery.failure_reason.clone(),
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        self.ensure_host_attention_unlocked(&HostAttention {
            id: format!("host-attention-wd-{}-failed", delivery.id),
            team_run_id: delivery.team_run_id.clone(),
            kind: HostAttentionKind::WorkDeliveryFailed,
            work_id: delivery.work_id.clone(),
            work_version: delivery.work_version,
            source_event_ref: format!("wd-update:{}", update_sequence),
            member_run_id: Some(delivery.recipient_member_run_id.clone()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            claimed_host_lease_id: None,
            claimed_host_lease_generation: None,
            claimed_host_lease_owner_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: delivery.updated_at.clone(),
            updated_at: delivery.updated_at.clone(),
        })?;
        Ok(delivery)
    }

    /// Requeue a ProviderWorkDispatch claim abandoned by an older Supervisor
    /// generation. This is intentionally explicit: an expired lease alone is
    /// not proof that the provider did not receive the Work.
    ///
    /// Only the current, unexpired successor lease may reconcile. A claim with
    /// a provider receipt, or a delivery already marked provider-received or
    /// acknowledged, is never rolled back.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_stale_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<ProviderWorkDispatch> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no active Supervisor lease"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }

        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("ProviderWorkDispatch not found: {delivery_id}"))
            })?;
        if delivery.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "ProviderWorkDispatch {delivery_id} belongs to {}, not {team_run_id}",
                delivery.team_run_id
            )));
        }
        if delivery.status == ProviderWorkDispatchStatus::Queued
            && delivery.claim_id.is_none()
            && delivery.claimed_by_supervisor_id.is_none()
            && delivery.claimed_generation.is_none()
            && delivery.provider_receipt_id.is_none()
        {
            return Ok(delivery);
        }
        if delivery.status != ProviderWorkDispatchStatus::Claimed {
            return Err(StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: ProviderWorkDispatch {delivery_id} is {:?} and cannot be requeued",
                delivery.status
            )));
        }
        if delivery.provider_receipt_id.is_some() {
            return Err(StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: ProviderWorkDispatch {delivery_id} has a provider receipt"
            )));
        }
        let claimed_generation = delivery.claimed_generation.ok_or_else(|| {
            StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: ProviderWorkDispatch {delivery_id} has no claimed generation"
            ))
        })?;
        if claimed_generation >= supervisor_generation {
            return Err(StoreError::Conflict(format!(
                "ProviderWorkDispatch {delivery_id} is not a stale claim from a predecessor Supervisor generation"
            )));
        }

        delivery.status = ProviderWorkDispatchStatus::Queued;
        delivery.claim_id = None;
        delivery.claimed_by_supervisor_id = None;
        delivery.claimed_generation = None;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = None;
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &ProviderWorkDispatchUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(delivery)
    }

    pub fn team_supervisor_leases(&self) -> StoreResult<Vec<TeamSupervisorLease>> {
        self.read_jsonl("team_supervisor_leases.jsonl")
    }

    pub fn latest_team_supervisor_lease(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<TeamSupervisorLease>> {
        Ok(latest_by_id(self.team_supervisor_leases()?, |lease| {
            lease.team_run_id.clone()
        })
        .remove(team_run_id))
    }

    pub fn team_member_close_requests(&self) -> StoreResult<Vec<TeamMemberCloseRequest>> {
        self.read_jsonl("team_member_close_requests.jsonl")
    }

    pub fn latest_team_member_close_request(
        &self,
        member_run_id: &str,
    ) -> StoreResult<Option<TeamMemberCloseRequest>> {
        Ok(latest_by_id(self.team_member_close_requests()?, |request| {
            request.member_run_id.clone()
        })
        .remove(member_run_id))
    }

    pub fn member_actions(&self) -> StoreResult<Vec<MemberAction>> {
        self.read_jsonl("member_actions.jsonl")
    }

    pub fn pending_interactions(&self) -> StoreResult<Vec<PendingInteraction>> {
        self.read_jsonl("pending_interactions.jsonl")
    }

    pub fn delegation_runs(&self) -> StoreResult<Vec<DelegationRun>> {
        self.read_jsonl("delegation_runs.jsonl")
    }

    pub fn team_run_events(&self) -> StoreResult<Vec<TeamRunEvent>> {
        self.read_jsonl("team_run_events.jsonl")
    }

    fn append_jsonl<T: Serialize>(&self, file_name: &str, value: &T) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.append_jsonl_unlocked(file_name, value)
    }

    fn append_jsonl_unlocked<T: Serialize>(&self, file_name: &str, value: &T) -> StoreResult<()> {
        let mut row = Vec::new();
        serde_json::to_writer(&mut row, value)?;
        row.push(b'\n');

        let path = self.root.join(file_name);
        let creates_ledger = !path.exists();
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(&row)?;
        file.flush()?;
        // Durability: fsync the row to stable storage before returning. Without
        // this a crash immediately after a claim append (the Running session row
        // + the Acknowledged message row in `claim_queued_message_delivery`) can
        // lose those rows from the OS page cache; latest-wins projection would
        // then revert the message to Queued and double-deliver it. `flush()`
        // only drains the userspace buffer, not the kernel cache, so we must
        // `sync_all`. Always called under the global flock, so write ordering
        // across files is preserved.
        file.sync_all()?;
        if creates_ledger {
            // The first fence/operation append creates a directory entry.
            // Syncing only the inode is insufficient across a system crash;
            // persist that new name before reporting the append durable.
            File::open(&self.root)?.sync_all()?;
        }
        Ok(())
    }

    /// Read only the trailing `window` bytes of a JSONL file, dropping the first
    /// (possibly partial) line unless the window covers the whole file.
    ///
    /// Only valid for latest-wins projections keyed by a field, where the answer
    /// is the LAST matching row. Callers must fall back to `read_jsonl` when the
    /// key is absent from the tail — absence in the window proves nothing.
    ///
    /// Motivation: Supervisor lease heartbeats append ~1 row/s per live run and
    /// every renewal re-parsed the entire file under the global write lock,
    /// making heartbeat cost O(N) and cumulative cost O(N²). Measured on
    /// `star-harness-dogfood`: 71,524 rows / 23 MB, 15,101 renewals in 4.77 h
    /// for one run, with observed renewal drift (p50 1135 ms against a 1000 ms
    /// sleep) already showing the parse cost.
    fn read_jsonl_tail<T: DeserializeOwned>(
        &self,
        file_name: &str,
        window: u64,
    ) -> StoreResult<Vec<T>> {
        let path = self.root.join(file_name);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let start = len.saturating_sub(window);
        // Whether the byte before `start` is a newline decides if `start`
        // already sits on a row boundary. Discarding unconditionally would drop
        // a COMPLETE row whenever the window happens to land there, which costs
        // a needless full-scan fallback (and would silently lose a row for any
        // future caller that does not have one).
        let starts_on_boundary = if start == 0 {
            true
        } else {
            file.seek(SeekFrom::Start(start - 1))?;
            let mut prev = [0u8; 1];
            std::io::Read::read_exact(&mut file, &mut prev)?;
            prev[0] == b'\n'
        };
        file.seek(SeekFrom::Start(start))?;
        let mut values = Vec::new();
        let mut lines = BufReader::new(file).lines();
        if !starts_on_boundary {
            // Discard the torn first line inside the window.
            let _ = lines.next();
        }
        for line in lines {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            values.push(serde_json::from_str(&line)?);
        }
        Ok(values)
    }

    /// Latest lease for one run: tail window first, full scan only on miss.
    fn latest_lease_for_run_unlocked(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<TeamSupervisorLease>> {
        const TAIL_WINDOW_BYTES: u64 = 256 * 1024;
        let tail = self.read_jsonl_tail::<TeamSupervisorLease>(
            "team_supervisor_leases.jsonl",
            TAIL_WINDOW_BYTES,
        )?;
        // rfind, not filter().next_back(): latest-wins means the LAST matching
        // row in the window, and rfind scans from the back so it stops at the
        // first hit instead of walking the whole window.
        if let Some(found) = tail
            .into_iter()
            .rfind(|lease| lease.team_run_id == team_run_id)
        {
            return Ok(Some(found));
        }
        Ok(latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id))
    }

    /// Collapse the lease file to one row per run (latest wins).
    ///
    /// Called on acquisition, which is rare (one per Supervisor generation),
    /// while heartbeats are frequent. Bounds the file at ~#runs rows so the
    /// tail window above always hits and the file stops growing without bound.
    /// Generation fencing is unaffected: the retained row is exactly the row a
    /// full-scan latest-wins projection would have produced.
    fn compact_supervisor_leases_unlocked(&self) -> StoreResult<()> {
        let path = self.root.join("team_supervisor_leases.jsonl");
        if !path.exists() {
            return Ok(());
        }
        let all = self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?;
        let latest = latest_by_id(all, |lease| lease.team_run_id.clone());
        let temp = self.root.join("team_supervisor_leases.jsonl.compact");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp)?;
            for lease in latest.values() {
                let mut row = Vec::new();
                serde_json::to_writer(&mut row, lease)?;
                row.push(b'\n');
                file.write_all(&row)?;
            }
            file.flush()?;
            file.sync_all()?;
        }
        fs::rename(&temp, &path)?;
        // fsync the PARENT DIRECTORY, not just the temp inode. POSIX allows a
        // crash to recover either the old or the new directory entry after a
        // rename; only syncing the directory makes the replacement durable.
        // Without it a system crash can resurrect the pre-compaction file and
        // with it an already-issued generation, violating the monotonic
        // higher-generation contract in ADR 0044.
        if let Ok(dir) = File::open(&self.root) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    fn acquire_write_lock(&self) -> StoreResult<StoreWriteLock> {
        let (timeout, poll_interval) = store_write_lock_policy();
        self.acquire_write_lock_with_policy(timeout, poll_interval)
    }

    fn acquire_write_lock_with_policy(
        &self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> StoreResult<StoreWriteLock> {
        let lock_path = self.root.join(".store.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        let deadline = Instant::now() + timeout;
        loop {
            match lock_file_exclusive(&file) {
                Ok(()) => return Ok(StoreWriteLock { file }),
                Err(error) if would_block_lock(&error) => {
                    if Instant::now() >= deadline {
                        return Err(StoreError::LockTimeout(lock_path.display().to_string()));
                    }
                    thread::sleep(
                        poll_interval.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
    }

    fn read_jsonl<T: DeserializeOwned>(&self, file_name: &str) -> StoreResult<Vec<T>> {
        let path = self.root.join(file_name);
        if !path.exists() {
            return Ok(Vec::new());
        }

        // A writer holds the store flock, but ordinary projections deliberately
        // do not: Dashboard/API reads must remain concurrent with one another.
        // `write_all` still may expose a short prefix before the trailing
        // newline becomes visible, so take a byte snapshot and retry only that
        // unmistakably incomplete final-row state. A complete snapshot is
        // immutable in memory even if another append starts immediately after.
        // The bounded retry preserves honest corruption reporting for a file
        // that remains truncated instead of silently dropping its final row.
        const INCOMPLETE_ROW_RETRY: Duration = Duration::from_secs(1);
        const INCOMPLETE_ROW_POLL: Duration = Duration::from_millis(5);
        let deadline = Instant::now() + INCOMPLETE_ROW_RETRY;
        let snapshot = loop {
            let bytes = fs::read(&path)?;
            if bytes.is_empty() || bytes.ends_with(b"\n") || Instant::now() >= deadline {
                break bytes;
            }
            thread::sleep(INCOMPLETE_ROW_POLL);
        };

        let mut values = Vec::new();
        for line in snapshot.split(|byte| *byte == b'\n') {
            if line.iter().all(|byte| byte.is_ascii_whitespace()) {
                continue;
            }
            values.push(serde_json::from_slice(line)?);
        }
        Ok(values)
    }
}

fn latest_by_id<T>(
    values: Vec<T>,
    mut id: impl FnMut(&T) -> String,
) -> std::collections::BTreeMap<String, T> {
    let mut latest = std::collections::BTreeMap::new();
    for value in values {
        latest.insert(id(&value), value);
    }
    latest
}

/// Normalize a Work title for duplicate detection: trim, lowercase, collapse
/// whitespace. Two titles that differ only in casing or spacing are treated as
/// the same logical Work within a team run.
fn normalize_work_title(title: &str) -> String {
    let trimmed = title.trim().to_lowercase();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    words.join(" ")
}

fn member_identity(member: &ProviderRuntimeProjection) -> String {
    member.agent_member_id.clone()
}

fn member_is_active_reviewer_runtime(member: &ProviderRuntimeProjection) -> bool {
    member.coordination_is_active()
        && !matches!(
            member.status,
            firm_core::MemberRunStatus::Completed
                | firm_core::MemberRunStatus::Failed
                | firm_core::MemberRunStatus::Stopped
        )
}

fn ensure_member_provenance_unchanged(
    current: &ProviderRuntimeProjection,
    next: &ProviderRuntimeProjection,
) -> StoreResult<()> {
    if next.id != current.id
        || next.team_run_id != current.team_run_id
        || next.slot_id != current.slot_id
        || next.agent_member_id != current.agent_member_id
        || next.role != current.role
        || next.provider != current.provider
    {
        return Err(StoreError::Conflict(format!(
            "MEMBER_PROVENANCE_IMMUTABLE: ProviderRuntimeProjection {} cannot change its team, stable identity, role, or provider",
            current.id
        )));
    }
    Ok(())
}

fn ensure_member_lifecycle_revision(
    current: &ProviderRuntimeProjection,
    next: &ProviderRuntimeProjection,
) -> StoreResult<()> {
    if next.runtime_generation < current.runtime_generation
        || next.runtime_generation > current.runtime_generation.saturating_add(1)
    {
        return Err(StoreError::Conflict(format!(
            "MEMBER_GENERATION_INVALID: ProviderRuntimeProjection {} generation must remain {} or advance exactly once",
            current.id, current.runtime_generation
        )));
    }
    if current.coordination_is_retired() && !next.coordination_is_retired() {
        return Err(StoreError::Conflict(format!(
            "MEMBER_RETIRED: ProviderRuntimeProjection {} cannot leave retired coordination",
            current.id
        )));
    }
    let reactivates_coordination =
        !current.coordination_is_active() && next.coordination_is_active();
    let restarts_terminal_runtime = !member_is_active_reviewer_runtime(current)
        && next.coordination_is_active()
        && member_is_active_reviewer_runtime(next);
    if (reactivates_coordination || restarts_terminal_runtime)
        && next.runtime_generation != current.runtime_generation.saturating_add(1)
    {
        return Err(StoreError::Conflict(format!(
            "MEMBER_REOPEN_REQUIRES_NEW_GENERATION: ProviderRuntimeProjection {} must advance runtime_generation when reactivated",
            current.id
        )));
    }
    Ok(())
}

fn ensure_provider_compatibility_cause_unchanged(
    current: &ProviderRuntimeProjection,
    next: &ProviderRuntimeProjection,
) -> StoreResult<()> {
    if current.provider_compatibility_block_cause != next.provider_compatibility_block_cause {
        return Err(StoreError::Conflict(format!(
            "PROVIDER_COMPATIBILITY_BLOCK_AUTHORITY_REQUIRED: generic ProviderRuntimeProjection CAS cannot set, replace, or clear the typed compatibility cause for {}",
            current.id
        )));
    }
    Ok(())
}

fn ensure_compatibility_cause_matches_profile(
    member: &ProviderRuntimeProjection,
    profile: &ProviderIntegrationProfile,
    cause: &ProviderCompatibilityBlockCause,
) -> StoreResult<()> {
    cause
        .validate()
        .map_err(|error| StoreError::Conflict(error.to_string()))?;
    let provider_version = profile.provider_version.as_deref().unwrap_or("unavailable");
    let adapter_contract_version = profile
        .adapter_contract_version
        .as_deref()
        .unwrap_or("unknown");
    if cause.member_run_id != member.id
        || profile.provider != member.provider
        || cause.exact_key()
            != (
                profile.provider.as_str(),
                profile.execution_mode.as_str(),
                provider_version,
                adapter_contract_version,
            )
    {
        return Err(StoreError::Conflict(format!(
            "PROVIDER_COMPATIBILITY_BLOCK_TUPLE_MISMATCH: typed cause does not match ProviderRuntimeProjection {} and its observed provider profile",
            member.id
        )));
    }
    Ok(())
}

fn ensure_team_run_admission_revision(
    current: &AgentTeamRun,
    next: &AgentTeamRun,
    member: &ProviderRuntimeProjection,
) -> StoreResult<()> {
    if member.team_run_id != current.id {
        return Err(StoreError::Conflict(format!(
            "TEAM_SCOPE_MISMATCH: ProviderRuntimeProjection {} belongs to {}, not {}",
            member.id, member.team_run_id, current.id
        )));
    }
    let mut expected_ids = current.member_run_ids.clone();
    if expected_ids.iter().any(|id| id == &member.id) {
        return Err(StoreError::Conflict(format!(
            "member run already admitted: {}",
            member.id
        )));
    }
    expected_ids.push(member.id.clone());
    if next.member_run_ids != expected_ids {
        return Err(StoreError::Conflict(
            "MEMBER_ADMISSION_INVALID: TeamRun revision must append exactly the admitted ProviderRuntimeProjection id"
                .to_string(),
        ));
    }
    let mut expected_next = current.clone();
    expected_next.member_run_ids = expected_ids;
    expected_next.updated_at = next.updated_at.clone();
    if *next != expected_next {
        return Err(StoreError::Conflict(
            "MEMBER_ADMISSION_INVALID: admission may only append one member id and update TeamRun.updated_at"
                .to_string(),
        ));
    }
    Ok(())
}

fn durable_team_id(run: &AgentTeamRun) -> Option<&str> {
    Some(run.agent_team_id.as_str())
}

fn node_project_registration_identity(value: &NodeProjectRegistration) -> String {
    node_project_registration_key(
        &value.node_id,
        &value.execution_space_id,
        &value.project_binding_id,
    )
}

fn node_project_registration_key(
    node_id: &str,
    execution_space_id: &str,
    project_binding_id: &str,
) -> String {
    format!("{node_id}\u{1f}{execution_space_id}\u{1f}{project_binding_id}")
}

fn works_share_scope(left: &Work, right: &Work) -> bool {
    match (left.team_id.as_deref(), right.team_id.as_deref()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => left.team_run_id == right.team_run_id,
        _ => false,
    }
}

fn compare_store_timestamps(left: &str, right: &str) -> std::cmp::Ordering {
    match (
        left.strip_prefix("unix-ms:")
            .and_then(|value| value.parse::<u128>().ok()),
        right
            .strip_prefix("unix-ms:")
            .and_then(|value| value.parse::<u128>().ok()),
    ) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

/// Parse a store timestamp in `unix-ms:<millis>` format into a unix
/// millisecond integer. Returns `None` for malformed or empty timestamps.
pub fn parse_iso8601_to_unix_ms(ts: &str) -> Option<u64> {
    ts.strip_prefix("unix-ms:")
        .and_then(|value| value.parse::<u64>().ok())
}

fn same_team_run_event_semantics(left: &TeamRunEvent, right: &TeamRunEvent) -> bool {
    left.team_run_id == right.team_run_id
        && left.source_kind == right.source_kind
        && left.member_run_id == right.member_run_id
        && left.delegation_run_id == right.delegation_run_id
        && left.entity_type == right.entity_type
        && left.entity_id == right.entity_id
        && left.operation == right.operation
        && left.summary == right.summary
}

fn apply_work_delivery_update(
    delivery: &mut ProviderWorkDispatch,
    update: ProviderWorkDispatchUpdate,
) {
    delivery.status = update.status;
    delivery.attempt = update.attempt;
    delivery.claim_id = update.claim_id;
    delivery.claimed_by_supervisor_id = update.claimed_by_supervisor_id;
    delivery.claimed_generation = update.claimed_generation;
    delivery.provider_receipt_id = update.provider_receipt_id;
    delivery.failure_reason = update.failure_reason;
    delivery.updated_at = update.updated_at;
}

fn require_non_empty_store(value: &str, label: &str) -> StoreResult<()> {
    if value.trim().is_empty() {
        Err(StoreError::Conflict(format!("{label} must not be empty")))
    } else {
        Ok(())
    }
}

fn require_host_actor(actor: &firm_core::TeamActorRef) -> StoreResult<()> {
    if matches!(
        actor.kind,
        firm_core::TeamActorKind::Host
            | firm_core::TeamActorKind::Operator
            | firm_core::TeamActorKind::Service
    ) {
        Ok(())
    } else {
        Err(StoreError::Conflict(
            "Host authority is required for this Work command".to_string(),
        ))
    }
}

fn require_member_actor(actor: &firm_core::TeamActorRef, member_run_id: &str) -> StoreResult<()> {
    if actor.kind == firm_core::TeamActorKind::ProviderRuntimeProjection
        && actor.id == member_run_id
    {
        Ok(())
    } else {
        Err(StoreError::Conflict(format!(
            "trusted ProviderRuntimeProjection actor {member_run_id} is required"
        )))
    }
}

fn delivery_blocks_another_claim(delivery: &RegistryDeliveryAttempt) -> bool {
    matches!(
        delivery.execution_status,
        Some(ProviderExecutionStatus::Queued | ProviderExecutionStatus::Running)
    ) || (delivery.execution_status == Some(ProviderExecutionStatus::Stale)
        && delivery.terminal_source != Some(MessageTerminalSource::Failed))
}

fn store_write_lock_policy() -> (Duration, Duration) {
    #[cfg(debug_assertions)]
    if let Ok(raw) = std::env::var("FIRM_TEST_STORE_WRITE_LOCK_TIMEOUT_MS") {
        if let Ok(timeout_ms) = raw.parse::<u64>() {
            if timeout_ms > 0 {
                return (Duration::from_millis(timeout_ms), Duration::from_millis(1));
            }
        }
    }
    (Duration::from_secs(10), Duration::from_millis(10))
}

fn lock_file_exclusive(file: &File) -> std::io::Result<()> {
    let result = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn unlock_file(file: &File) {
    let _ = unsafe { flock(file.as_raw_fd(), LOCK_UN) };
}

fn would_block_lock(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(11) | Some(35))
        || error.kind() == std::io::ErrorKind::WouldBlock
}

struct StoreWriteLock {
    file: File,
}

impl Drop for StoreWriteLock {
    fn drop(&mut self) {
        unlock_file(&self.file);
    }
}

/// Scoped exclusive source-store guard used by migration code.
///
/// This is public so migration surfaces outside `firm-store` can serialize
/// their snapshot with every normal [`HarnessStore`] writer while keeping the
/// underlying lock implementation private.
pub struct StoreExclusiveMigrationGuard {
    _write_lock: StoreWriteLock,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use firm_core::{
        DelegationMode, DelegationStatus, HostAttentionKind, MemberActionStatus,
        MemberExecutionDriver, MemberRunStatus, MemberWorkspaceSnapshot, Mission, MissionLogEntry,
        MissionLogEntryKind, MissionStatus, NativeSessionAvailability, NativeSessionRef,
        OrdinaryMessageBoundary, ProviderCompatibilityBlockBoundary,
        ProviderCompatibilityBlockSource, ProviderDispatchAttempt, ProviderDispatchIntent,
        ProviderEventFidelity, ProviderFeatureMode, ProviderInteractionMessageOption,
        ProviderInteractionMode, ProviderInteractionRequestBody, ProviderInteractionResponseBody,
        ProviderInteractionType, ProviderResponseIntent, RegistryMessageIntent, SenderKind,
        TeamActorKind, TeamActorRef, TeamDeliveryPolicy, TeamDeliveryStatus, TeamRecipientKind,
        TeamRecipientRef, TeamRunEventSourceKind, TeamRunStatus, Wave, WaveExecutorKind,
        WaveGateStatus, WaveStatus, WorkPriority,
    };

    use super::*;

    fn lock_policy_test_store(label: &str) -> HarnessStore {
        let root = std::env::temp_dir().join(format!(
            "firm-store-lock-policy-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let store = HarnessStore::new(root);
        store.init().expect("init lock-policy store");
        store
    }

    fn hold_store_lock(store: &HarnessStore) -> File {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .truncate(false)
            .write(true)
            .open(store.root().join(".store.lock"))
            .expect("open store lock");
        lock_file_exclusive(&file).expect("hold store lock");
        file
    }

    #[test]
    fn write_lock_contention_exhaustion_is_bounded_and_typed() {
        let store = lock_policy_test_store("timeout");
        let held = hold_store_lock(&store);
        let started = Instant::now();
        let error = match store
            .acquire_write_lock_with_policy(Duration::from_millis(25), Duration::from_millis(2))
        {
            Ok(_) => panic!("held lock must exhaust the short test policy"),
            Err(error) => error,
        };
        let elapsed = started.elapsed();
        assert!(matches!(error, StoreError::LockTimeout(_)));
        assert!(elapsed >= Duration::from_millis(20), "elapsed={elapsed:?}");
        assert!(elapsed < Duration::from_millis(500), "elapsed={elapsed:?}");
        unlock_file(&held);
        drop(held);
        std::fs::remove_dir_all(store.root()).expect("cleanup store");
    }

    #[test]
    fn write_lock_contention_retries_until_the_owner_releases() {
        let store = Arc::new(lock_policy_test_store("release"));
        let held = hold_store_lock(&store);
        let contender = Arc::clone(&store);
        let waiter = std::thread::spawn(move || {
            contender.acquire_write_lock_with_policy(
                Duration::from_millis(500),
                Duration::from_millis(2),
            )
        });
        std::thread::sleep(Duration::from_millis(25));
        unlock_file(&held);
        drop(held);
        let acquired = waiter
            .join()
            .expect("contention waiter")
            .expect("waiter acquires after release");
        drop(acquired);
        std::fs::remove_dir_all(store.root()).expect("cleanup store");
    }

    fn provider_compatibility_admission(
        id: &str,
        execution_mode: &str,
        adapter_contract_version: &str,
    ) -> ProviderCompatibilityAdmission {
        ProviderCompatibilityAdmission {
            id: id.to_string(),
            project_id: "project-1".to_string(),
            store_id: "store-1".to_string(),
            provider: "claude".to_string(),
            execution_mode: execution_mode.to_string(),
            provider_version: "2.1.220".to_string(),
            adapter_contract_version: adapter_contract_version.to_string(),
            policy: firm_core::ProviderCompatibilityAdmissionPolicy::Strict,
            actor: "operator-1".to_string(),
            evidence_refs: vec!["evidence-1".to_string()],
            admitted_at: "unix-ms:1".to_string(),
            lifecycle: ProviderCompatibilityAdmissionLifecycle::Active,
            predecessor_admission_id: None,
            reason: None,
        }
    }

    fn provider_compatibility_test_profile() -> ProviderIntegrationProfile {
        ProviderIntegrationProfile {
            provider: "kimi".into(),
            execution_mode: "kimi_acp".into(),
            execution_driver: MemberExecutionDriver::HostDriven,
            provider_version: Some("2.1.220".into()),
            adapter_contract_version: Some("kimi-acp-v1".into()),
            reviewed_provider_versions: Vec::new(),
            compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
            adapter_reviewed_at: None,
            compatibility_note: None,
            interaction_mode: ProviderInteractionMode::EndRoundAndFollowUp,
            ordinary_message_boundary: OrdinaryMessageBoundary::InTurn,
            plan_mode: ProviderFeatureMode::Emulated,
            goal_mode: ProviderFeatureMode::Emulated,
            tool_event_fidelity: ProviderEventFidelity::Structured,
            artifact_event_fidelity: ProviderEventFidelity::Structured,
            supports_cancel: true,
            supports_resume: true,
            observes_native_subagents: false,
            observes_background_tasks: false,
            thinking_transient_only: true,
        }
    }

    fn provider_admission_test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "firm-store-provider-admission-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ))
    }

    fn provider_admission_test_store(label: &str) -> HarnessStore {
        HarnessStore::new(provider_admission_test_root(label))
            .with_provider_compatibility_scope("project-1", "store-1")
    }

    #[test]
    fn provider_compatibility_admission_is_exact_and_preserves_policy() {
        let store = provider_admission_test_store("exact");
        let strict = provider_compatibility_admission("strict", "sdk", "contract-v1");
        let mut advisory =
            provider_compatibility_admission("advisory", "interactive", "contract-v2");
        advisory.policy = firm_core::ProviderCompatibilityAdmissionPolicy::Advisory;
        store.admit_provider_compatibility(&strict).expect("strict");
        store
            .admit_provider_compatibility(&advisory)
            .expect("advisory");

        assert_eq!(
            store
                .effective_provider_compatibility_admission(
                    "claude",
                    "sdk",
                    "2.1.220",
                    "contract-v1"
                )
                .expect("lookup"),
            Some(strict)
        );
        assert!(store
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v2")
            .expect("contract isolation")
            .is_none());
        assert_eq!(
            store
                .latest_provider_compatibility_admissions()
                .unwrap()
                .into_iter()
                .find(|row| row.id == "advisory")
                .expect("advisory projection")
                .policy,
            firm_core::ProviderCompatibilityAdmissionPolicy::Advisory
        );
    }

    #[test]
    fn typed_provider_block_is_store_owned_and_recovery_is_exact() {
        let root = provider_admission_test_root("typed-block");
        let store =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
        let (_run, initial, _work) = seed_host_attention_fixture(&store, "typed-block", None);
        let profile = provider_compatibility_test_profile();
        let cause = ProviderCompatibilityBlockCause {
            schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
            id: "cause-1".into(),
            member_run_id: initial.id.clone(),
            provider: "kimi".into(),
            execution_mode: "kimi_acp".into(),
            provider_version: "2.1.220".into(),
            adapter_contract_version: "kimi-acp-v1".into(),
            boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
            compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
            source: ProviderCompatibilityBlockSource::AdapterCompatibility,
            probe_error: None,
            caused_at: "unix-ms:2".into(),
        };

        let mut forged = initial.clone();
        forged.status = MemberRunStatus::Blocked;
        forged.provider_compatibility_block_cause = Some(cause.clone());
        assert!(store
            .compare_and_append_member_run(&initial, &forged)
            .expect_err("generic CAS cannot forge typed cause")
            .to_string()
            .contains("AUTHORITY_REQUIRED"));

        let blocked = store
            .block_member_run_for_provider_compatibility(&initial, &profile, cause, "unix-ms:2")
            .expect("dedicated typed block");
        let mut cleared = blocked.clone();
        cleared.status = MemberRunStatus::Idle;
        cleared.provider_compatibility_block_cause = None;
        assert!(store
            .compare_and_append_member_run(&blocked, &cleared)
            .expect_err("generic CAS cannot clear typed cause")
            .to_string()
            .contains("AUTHORITY_REQUIRED"));

        let mut wrong = profile.clone();
        wrong.provider_version = Some("2.1.221".into());
        assert!(store
            .recover_member_run_from_provider_compatibility_block(
                &blocked,
                &wrong,
                ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:3"
            )
            .expect_err("an unadmitted new tuple cannot recover")
            .to_string()
            .contains("NOT_AUTHORIZED"));

        let mut admission =
            provider_compatibility_admission("typed-recovery", "kimi_acp", "kimi-acp-v1");
        admission.provider = "kimi".into();
        store
            .admit_provider_compatibility_admission(&admission)
            .expect("exact admission");
        assert!(store
            .recover_member_run_from_provider_compatibility_block(
                &blocked,
                &profile,
                ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:3",
            )
            .expect_err("a Start cause cannot recover at Resume")
            .to_string()
            .contains("BOUNDARY_MISMATCH"));
        let recovered = store
            .recover_member_run_from_provider_compatibility_block(
                &blocked,
                &profile,
                ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:3",
            )
            .expect("exact typed recovery");
        assert_eq!(recovered.id, initial.id);
        assert_eq!(recovered.status, MemberRunStatus::Idle);
        assert!(recovered.provider_compatibility_block_cause.is_none());
        assert!(store
            .recover_member_run_from_provider_compatibility_block(
                &blocked,
                &profile,
                ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:4"
            )
            .expect_err("stale recovery loses CAS")
            .to_string()
            .contains("changed concurrently"));

        let mut operator_blocked = recovered.clone();
        operator_blocked.status = MemberRunStatus::Blocked;
        store
            .compare_and_append_member_run(&recovered, &operator_blocked)
            .expect("ordinary operator block remains representable");
        assert!(store
            .recover_member_run_from_provider_compatibility_block(
                &operator_blocked,
                &profile,
                ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:5"
            )
            .expect_err("operator block has no typed cause")
            .to_string()
            .contains("CAUSE_REQUIRED"));

        let (_run2, initial2, _work2) =
            seed_host_attention_fixture(&store, "typed-source-reviewed", None);
        let mut review_pending = provider_compatibility_test_profile();
        review_pending.provider_version = Some("3.3.3".into());
        let source_cause = ProviderCompatibilityBlockCause {
            schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
            id: "cause-source-review".into(),
            member_run_id: initial2.id.clone(),
            provider: "kimi".into(),
            execution_mode: "kimi_acp".into(),
            provider_version: "3.3.3".into(),
            adapter_contract_version: "kimi-acp-v1".into(),
            boundary: ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
            compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
            source: ProviderCompatibilityBlockSource::AdapterCompatibility,
            probe_error: None,
            caused_at: "unix-ms:6".into(),
        };
        let source_blocked = store
            .block_member_run_for_provider_compatibility(
                &initial2,
                &review_pending,
                source_cause,
                "unix-ms:6",
            )
            .expect("block pending source review");
        let mut source_reviewed = review_pending;
        source_reviewed.provider_version = Some("3.3.4".into());
        source_reviewed.compatibility_status = ProviderCompatibilityStatus::Current;
        source_reviewed.reviewed_provider_versions = vec!["3.3.4".into()];
        assert!(store
            .recover_member_run_from_provider_compatibility_block(
                &source_blocked,
                &source_reviewed,
                ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:7",
            )
            .expect_err("a Resume cause cannot recover at Start")
            .to_string()
            .contains("BOUNDARY_MISMATCH"));
        let source_recovered = store
            .recover_member_run_from_provider_compatibility_block(
                &source_blocked,
                &source_reviewed,
                ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:7",
            )
            .expect("exact source review authorizes recovery without an admission");
        assert!(source_recovered
            .provider_compatibility_block_cause
            .is_none());
        assert_eq!(
            source_recovered
                .provider_profile
                .as_ref()
                .and_then(|profile| profile.provider_version.as_deref()),
            Some("3.3.4"),
            "recovery atomically replaces the durable blocked profile with the authorized refreshed tuple"
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn provider_compatibility_block_lifecycle_rejects_hostile_member_history() {
        use firm_core::MemberCoordinationStatus;

        for (index, mutate) in [
            (
                0,
                (
                    MemberRunStatus::Completed,
                    MemberCoordinationStatus::Active,
                    Some("done"),
                ),
            ),
            (
                1,
                (
                    MemberRunStatus::Failed,
                    MemberCoordinationStatus::Active,
                    Some("done"),
                ),
            ),
            (
                2,
                (
                    MemberRunStatus::Stopped,
                    MemberCoordinationStatus::Active,
                    Some("done"),
                ),
            ),
            (
                3,
                (
                    MemberRunStatus::Idle,
                    MemberCoordinationStatus::Closed,
                    None,
                ),
            ),
            (
                4,
                (
                    MemberRunStatus::Idle,
                    MemberCoordinationStatus::Retired,
                    None,
                ),
            ),
            (
                5,
                (
                    MemberRunStatus::Idle,
                    MemberCoordinationStatus::Active,
                    Some("hostile"),
                ),
            ),
        ] {
            let root = provider_admission_test_root(&format!("hostile-lifecycle-{index}"));
            let store =
                HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
            let (_run, initial, _work) =
                seed_host_attention_fixture(&store, &format!("hostile-{index}"), None);
            let mut hostile = initial.clone();
            hostile.status = mutate.0;
            hostile.coordination_status = mutate.1;
            hostile.finished_at = mutate.2.map(str::to_string);
            store
                .compare_and_append_member_run(&initial, &hostile)
                .expect("seed hostile but structurally valid history");
            let profile = provider_compatibility_test_profile();
            let cause = ProviderCompatibilityBlockCause {
                schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
                id: format!("hostile-cause-{index}"),
                member_run_id: hostile.id.clone(),
                provider: "kimi".into(),
                execution_mode: "kimi_acp".into(),
                provider_version: "2.1.220".into(),
                adapter_contract_version: "kimi-acp-v1".into(),
                boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
                source: ProviderCompatibilityBlockSource::AdapterCompatibility,
                probe_error: None,
                caused_at: "unix-ms:2".into(),
            };
            assert!(store
                .block_member_run_for_provider_compatibility(&hostile, &profile, cause, "unix-ms:3")
                .expect_err("terminal/closed/retired/finished history cannot be blocked")
                .to_string()
                .contains("LIFECYCLE_INVALID"));
            std::fs::remove_dir_all(root).expect("cleanup");
        }
    }

    #[test]
    fn provider_compatibility_recovery_authorizes_refreshed_tuple_and_preserves_refusals() {
        let root = provider_admission_test_root("refreshed-recovery");
        let store =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");

        let (_run, initial, _work) =
            seed_host_attention_fixture(&store, "unavailable-to-current", None);
        let mut unavailable = provider_compatibility_test_profile();
        unavailable.provider_version = None;
        unavailable.compatibility_status = ProviderCompatibilityStatus::Unavailable;
        let unavailable_cause = ProviderCompatibilityBlockCause {
            schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
            id: "unavailable-cause".into(),
            member_run_id: initial.id.clone(),
            provider: "kimi".into(),
            execution_mode: "kimi_acp".into(),
            provider_version: "unavailable".into(),
            adapter_contract_version: "kimi-acp-v1".into(),
            boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
            compatibility_status: ProviderCompatibilityStatus::Unavailable,
            source: ProviderCompatibilityBlockSource::ProbeFailure,
            probe_error: Some("runner missing".into()),
            caused_at: "unix-ms:2".into(),
        };
        let unavailable_blocked = store
            .block_member_run_for_provider_compatibility(
                &initial,
                &unavailable,
                unavailable_cause,
                "unix-ms:2",
            )
            .expect("durably block unavailable tuple");
        let mut current = provider_compatibility_test_profile();
        current.compatibility_status = ProviderCompatibilityStatus::Current;
        current.reviewed_provider_versions = vec!["2.1.220".into()];
        let current_recovered = store
            .recover_member_run_from_provider_compatibility_block(
                &unavailable_blocked,
                &current,
                ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:3",
            )
            .expect("source-reviewed refreshed tuple recovers old unavailable cause");
        assert_eq!(current_recovered.provider_profile.as_ref(), Some(&current));

        let (_run2, initial2, _work2) =
            seed_host_attention_fixture(&store, "drift-to-admitted", None);
        let old_drift = provider_compatibility_test_profile();
        let old_cause = ProviderCompatibilityBlockCause {
            schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
            id: "old-drift-cause".into(),
            member_run_id: initial2.id.clone(),
            provider: "kimi".into(),
            execution_mode: "kimi_acp".into(),
            provider_version: "2.1.220".into(),
            adapter_contract_version: "kimi-acp-v1".into(),
            boundary: ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
            compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
            source: ProviderCompatibilityBlockSource::AdapterCompatibility,
            probe_error: None,
            caused_at: "unix-ms:4".into(),
        };
        let drift_blocked = store
            .block_member_run_for_provider_compatibility(
                &initial2,
                &old_drift,
                old_cause,
                "unix-ms:4",
            )
            .expect("durably block old drift tuple");
        let mut new_drift = old_drift.clone();
        new_drift.provider_version = Some("2.1.221".into());

        let mut wrong_scope =
            provider_compatibility_admission("wrong-scope", "kimi_acp", "kimi-acp-v1");
        wrong_scope.project_id = "other-project".into();
        wrong_scope.provider = "kimi".into();
        wrong_scope.provider_version = "2.1.221".into();
        store
            .append_jsonl(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER, &wrong_scope)
            .expect("seed a valid admission belonging to another scope");
        assert!(store
            .recover_member_run_from_provider_compatibility_block(
                &drift_blocked,
                &new_drift,
                ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:5",
            )
            .expect_err("wrong-scope admission cannot recover")
            .to_string()
            .contains("NOT_AUTHORIZED"));
        assert_eq!(
            store
                .member_runs()
                .expect("read durable member")
                .into_iter()
                .rfind(|row| row.id == drift_blocked.id),
            Some(drift_blocked.clone()),
            "refused recovery leaves the durable blocked row unchanged"
        );

        let mut exact = provider_compatibility_admission("new-exact", "kimi_acp", "kimi-acp-v1");
        exact.provider = "kimi".into();
        exact.provider_version = "2.1.221".into();
        store
            .admit_provider_compatibility_admission(&exact)
            .expect("append exact admission");
        let admitted_recovered = store
            .recover_member_run_from_provider_compatibility_block(
                &drift_blocked,
                &new_drift,
                ProviderCompatibilityBlockBoundary::ResumePersistentExecution,
                MemberRunStatus::Idle,
                "unix-ms:6",
            )
            .expect("new exact admission authorizes atomic recovery");
        assert_eq!(
            admitted_recovered.provider_profile.as_ref(),
            Some(&new_drift)
        );
        assert!(admitted_recovered
            .provider_compatibility_block_cause
            .is_none());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn provider_compatibility_recovery_rejects_closed_retired_or_finished_block() {
        use firm_core::MemberCoordinationStatus;

        for (index, coordination, finished_at) in [
            (0, MemberCoordinationStatus::Closed, None),
            (1, MemberCoordinationStatus::Retired, None),
            (
                2,
                MemberCoordinationStatus::Active,
                Some("hostile-finished"),
            ),
        ] {
            let root = provider_admission_test_root(&format!("hostile-recovery-{index}"));
            let store =
                HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
            let (_run, initial, _work) =
                seed_host_attention_fixture(&store, &format!("hostile-recovery-{index}"), None);
            let profile = provider_compatibility_test_profile();
            let cause = ProviderCompatibilityBlockCause {
                schema_version: ProviderCompatibilityBlockCause::SCHEMA_VERSION,
                id: format!("recovery-cause-{index}"),
                member_run_id: initial.id.clone(),
                provider: "kimi".into(),
                execution_mode: "kimi_acp".into(),
                provider_version: "2.1.220".into(),
                adapter_contract_version: "kimi-acp-v1".into(),
                boundary: ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                compatibility_status: ProviderCompatibilityStatus::ReviewRequired,
                source: ProviderCompatibilityBlockSource::AdapterCompatibility,
                probe_error: None,
                caused_at: "unix-ms:2".into(),
            };
            let blocked = store
                .block_member_run_for_provider_compatibility(&initial, &profile, cause, "unix-ms:2")
                .expect("seed typed block");
            let mut hostile = blocked.clone();
            hostile.coordination_status = coordination;
            hostile.finished_at = finished_at.map(str::to_string);
            store
                .compare_and_append_member_run(&blocked, &hostile)
                .expect("seed hostile blocked history without changing typed cause");
            let mut admission = provider_compatibility_admission(
                &format!("hostile-recovery-admission-{index}"),
                "kimi_acp",
                "kimi-acp-v1",
            );
            admission.provider = "kimi".into();
            store
                .admit_provider_compatibility_admission(&admission)
                .expect("admit tuple");
            assert!(store
                .recover_member_run_from_provider_compatibility_block(
                    &hostile,
                    &profile,
                    ProviderCompatibilityBlockBoundary::StartPersistentExecution,
                    MemberRunStatus::Idle,
                    "unix-ms:3",
                )
                .expect_err("closed/retired/finished block cannot recover")
                .to_string()
                .contains("LIFECYCLE_INVALID"));
            std::fs::remove_dir_all(root).expect("cleanup");
        }
    }

    #[test]
    fn provider_compatibility_admission_replay_is_idempotent_and_id_conflict_fails() {
        let store = provider_admission_test_store("replay");
        let admission = provider_compatibility_admission("stable", "sdk", "contract-v1");
        store
            .append_provider_compatibility_admission(&admission)
            .expect("first append");
        store
            .append_provider_compatibility_admission(&admission)
            .expect("identical replay");
        assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 1);

        let mut conflict = admission;
        conflict.actor = "another-operator".to_string();
        assert!(matches!(
            store.append_provider_compatibility_admission(&conflict),
            Err(StoreError::Conflict(_))
        ));
    }

    #[test]
    fn provider_compatibility_command_replay_reuses_canonical_active_record() {
        let store = provider_admission_test_store("command-replay");
        let mut first = provider_compatibility_admission("generated-one", "sdk", "contract-v1");
        first.evidence_refs = vec![
            "evidence-b".into(),
            "evidence-a".into(),
            "evidence-b".into(),
        ];
        let created = store
            .ensure_provider_compatibility_admission(&first)
            .expect("create admission");
        assert!(created.created);
        assert_eq!(
            created.admission.evidence_refs,
            ["evidence-a", "evidence-b"]
        );

        let mut replay = first;
        replay.id = "generated-two".into();
        replay.admitted_at = "unix-ms:999".into();
        replay.evidence_refs = vec!["evidence-a".into(), "evidence-b".into()];
        let reused = store
            .ensure_provider_compatibility_admission(&replay)
            .expect("reuse admission");
        assert!(!reused.created);
        assert_eq!(reused.admission.id, created.admission.id);
        assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 1);
    }

    #[test]
    fn concurrent_provider_compatibility_command_replay_appends_once() {
        let store = Arc::new(provider_admission_test_store("concurrent-command-replay"));
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for (id, admitted_at, evidence_refs) in [
            ("generated-one", "unix-ms:10", vec!["b", "a", "b"]),
            ("generated-two", "unix-ms:20", vec!["a", "b"]),
        ] {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                let mut admission = provider_compatibility_admission(id, "sdk", "contract-v1");
                admission.admitted_at = admitted_at.into();
                admission.evidence_refs = evidence_refs.into_iter().map(String::from).collect();
                barrier.wait();
                store.ensure_provider_compatibility_admission(&admission)
            }));
        }
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("join").expect("ensure"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.created).count(), 1);
        assert_eq!(results[0].admission.id, results[1].admission.id);
        assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 1);
    }

    #[test]
    fn provider_compatibility_command_replay_rejects_semantic_drift() {
        for (tag, mutate) in [
            (
                "policy",
                (|row: &mut ProviderCompatibilityAdmission| {
                    row.policy = firm_core::ProviderCompatibilityAdmissionPolicy::Advisory;
                }) as fn(&mut ProviderCompatibilityAdmission),
            ),
            ("actor", |row: &mut ProviderCompatibilityAdmission| {
                row.actor = "another-operator".into();
            }),
            ("evidence", |row: &mut ProviderCompatibilityAdmission| {
                row.evidence_refs = vec!["different-evidence".into()];
            }),
        ] {
            let store = provider_admission_test_store(tag);
            let first = provider_compatibility_admission("first", "sdk", "contract-v1");
            store
                .ensure_provider_compatibility_admission(&first)
                .expect("seed admission");
            let mut drifted = first;
            drifted.id = "second".into();
            drifted.admitted_at = "unix-ms:2".into();
            mutate(&mut drifted);
            assert!(matches!(
                store.ensure_provider_compatibility_admission(&drifted),
                Err(StoreError::Conflict(_))
            ));
            assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 1);
        }
    }

    #[test]
    fn provider_compatibility_command_replay_creates_after_terminal_row() {
        let store = provider_admission_test_store("command-after-terminal");
        let active = provider_compatibility_admission("active", "sdk", "contract-v1");
        store
            .ensure_provider_compatibility_admission(&active)
            .expect("seed active");
        let mut revoked = active.clone();
        revoked.id = "revoked".into();
        revoked.lifecycle = ProviderCompatibilityAdmissionLifecycle::Revoked;
        revoked.predecessor_admission_id = Some(active.id.clone());
        revoked.reason = Some("operator revoked".into());
        store
            .revoke_provider_compatibility_admission(&revoked)
            .expect("revoke active");

        let mut replacement = active;
        replacement.id = "replacement".into();
        replacement.admitted_at = "unix-ms:3".into();
        let result = store
            .ensure_provider_compatibility_admission(&replacement)
            .expect("create replacement");
        assert!(result.created);
        assert_eq!(result.admission.id, "replacement");
        assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 3);
    }

    #[test]
    fn provider_compatibility_revoke_and_supersede_fence_stale_predecessors() {
        for lifecycle in [
            ProviderCompatibilityAdmissionLifecycle::Revoked,
            ProviderCompatibilityAdmissionLifecycle::Superseded,
        ] {
            let store = provider_admission_test_store("transition");
            let active = provider_compatibility_admission("active", "sdk", "contract-v1");
            store.admit_provider_compatibility(&active).unwrap();
            let mut transition = active.clone();
            transition.id = "transition".to_string();
            transition.lifecycle = lifecycle;
            transition.predecessor_admission_id = Some(active.id.clone());
            transition.reason = Some("contract changed".to_string());
            let mut wrong_predecessor = transition.clone();
            wrong_predecessor.id = "wrong-predecessor".to_string();
            wrong_predecessor.predecessor_admission_id = Some("another-active".to_string());
            assert!(matches!(
                store.append_provider_compatibility_admission_checked(&wrong_predecessor),
                Err(StoreError::Conflict(_))
            ));
            match lifecycle {
                ProviderCompatibilityAdmissionLifecycle::Revoked => store
                    .revoke_provider_compatibility(&transition)
                    .expect("revoke"),
                ProviderCompatibilityAdmissionLifecycle::Superseded => store
                    .supersede_provider_compatibility(&transition)
                    .expect("supersede"),
                ProviderCompatibilityAdmissionLifecycle::Active => unreachable!(),
            }
            store
                .append_provider_compatibility_admission_checked(&transition)
                .expect("terminal replay is idempotent");
            assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 2);
            assert!(store
                .effective_provider_compatibility_admission(
                    "claude",
                    "sdk",
                    "2.1.220",
                    "contract-v1"
                )
                .unwrap()
                .is_none());

            let mut stale = transition;
            stale.id = "stale".to_string();
            assert!(matches!(
                store.append_provider_compatibility_admission_checked(&stale),
                Err(StoreError::Conflict(_))
            ));
        }
    }

    #[test]
    fn concurrent_distinct_provider_compatibility_admissions_do_not_lose_rows() {
        let store = Arc::new(provider_admission_test_store("concurrent"));
        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for (id, mode) in [("one", "sdk"), ("two", "interactive")] {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                let admission = provider_compatibility_admission(id, mode, "contract-v1");
                barrier.wait();
                store.admit_provider_compatibility(&admission)
            }));
        }
        barrier.wait();
        for worker in workers {
            worker.join().expect("join").expect("append");
        }
        assert_eq!(store.provider_compatibility_admissions().unwrap().len(), 2);
    }

    #[test]
    fn malformed_provider_compatibility_ledger_fails_closed_and_roots_are_isolated() {
        let first_root = provider_admission_test_root("first-root");
        let second_root = provider_admission_test_root("second-root");
        let first = HarnessStore::new(&first_root)
            .with_provider_compatibility_scope("project-1", "store-1");
        let second = HarnessStore::new(second_root)
            .with_provider_compatibility_scope("project-1", "store-1");
        let admission = provider_compatibility_admission("one", "sdk", "contract-v1");
        first.admit_provider_compatibility(&admission).unwrap();
        assert!(second
            .provider_compatibility_admissions()
            .unwrap()
            .is_empty());

        std::fs::write(
            first_root.join(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER),
            b"{not-json}\n",
        )
        .unwrap();
        assert!(matches!(
            first.provider_compatibility_admissions(),
            Err(StoreError::Json(_))
        ));
        assert!(first
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1")
            .is_err());
        let mut replay = admission;
        replay.id = "two".into();
        replay.admitted_at = "unix-ms:2".into();
        assert!(matches!(
            first.ensure_provider_compatibility_admission(&replay),
            Err(StoreError::Json(_))
        ));
    }

    #[test]
    fn provider_compatibility_scope_is_exact_on_the_same_physical_store() {
        let root = provider_admission_test_root("scope");
        let writer =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
        let admission = provider_compatibility_admission("scoped", "sdk", "contract-v1");
        writer.admit_provider_compatibility(&admission).unwrap();

        let other_project =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-2", "store-1");
        assert!(other_project
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
            .unwrap()
            .is_none());
        let migrated_store =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-2");
        assert!(migrated_store
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
            .unwrap()
            .is_none());
    }

    #[test]
    fn provider_compatibility_authority_requires_configured_exact_scope() {
        let root = provider_admission_test_root("scope-required");
        let unscoped = HarnessStore::new(&root);
        let active = provider_compatibility_admission("unscoped-active", "sdk", "contract-v1");
        assert!(unscoped
            .admit_provider_compatibility(&active)
            .expect_err("unscoped Store cannot mint an admission")
            .to_string()
            .contains("SCOPE_REQUIRED"));
        assert!(unscoped
            .append_provider_compatibility_admission_checked(&active)
            .expect_err("the internal checked seam is also scope-fenced")
            .to_string()
            .contains("SCOPE_REQUIRED"));

        let mut revoked = active.clone();
        revoked.id = "unscoped-revoked".into();
        revoked.lifecycle = ProviderCompatibilityAdmissionLifecycle::Revoked;
        revoked.predecessor_admission_id = Some(active.id.clone());
        revoked.reason = Some("operator revoke".into());
        assert!(unscoped
            .revoke_provider_compatibility(&revoked)
            .expect_err("unscoped Store cannot revoke an admission")
            .to_string()
            .contains("SCOPE_REQUIRED"));

        let mut superseded = revoked.clone();
        superseded.id = "unscoped-superseded".into();
        superseded.lifecycle = ProviderCompatibilityAdmissionLifecycle::Superseded;
        assert!(unscoped
            .supersede_provider_compatibility(&superseded)
            .expect_err("unscoped Store cannot supersede an admission")
            .to_string()
            .contains("SCOPE_REQUIRED"));
        assert!(unscoped
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
            .expect_err("unscoped Store cannot return effective authority")
            .to_string()
            .contains("SCOPE_REQUIRED"));

        let wrong_scope =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-2", "store-1");
        assert!(wrong_scope
            .admit_provider_compatibility(&active)
            .expect_err("configured scope must exactly match the row")
            .to_string()
            .contains("scope mismatch"));

        unscoped.init().unwrap();
        let mut hostile_row = active.clone();
        hostile_row.id = "manually-seeded-foreign-scope".into();
        hostile_row.project_id = "foreign-project".into();
        std::fs::write(
            root.join(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER),
            format!("{}\n", serde_json::to_string(&hostile_row).unwrap()),
        )
        .unwrap();
        let scoped =
            HarnessStore::new(&root).with_provider_compatibility_scope("project-1", "store-1");
        assert!(scoped
            .effective_provider_compatibility_admission("claude", "sdk", "2.1.220", "contract-v1",)
            .expect("foreign ledger rows remain readable audit data")
            .is_none());

        let exact_root = provider_admission_test_root("scope-exact");
        let exact = HarnessStore::new(&exact_root)
            .with_provider_compatibility_scope("project-1", "store-1");
        exact
            .admit_provider_compatibility(&active)
            .expect("exact configured scope can mint authority");
        assert_eq!(
            exact
                .effective_provider_compatibility_admission(
                    "claude",
                    "sdk",
                    "2.1.220",
                    "contract-v1",
                )
                .unwrap()
                .as_ref()
                .map(|row| row.id.as_str()),
            Some("unscoped-active")
        );
    }

    #[test]
    fn provider_compatibility_ledger_semantic_corruption_fails_closed() {
        let root = provider_admission_test_root("semantic-corruption");
        let store = HarnessStore::new(&root);
        store.init().unwrap();
        let active = provider_compatibility_admission("active", "sdk", "contract-v1");
        let mut terminal = active.clone();
        terminal.id = "terminal".to_string();
        terminal.lifecycle = ProviderCompatibilityAdmissionLifecycle::Revoked;
        terminal.predecessor_admission_id = Some(active.id.clone());
        terminal.reason = Some("operator revoke".to_string());

        let cases = [
            vec![active.clone(), active.clone()],
            {
                let mut unknown = terminal.clone();
                unknown.predecessor_admission_id = Some("unknown".to_string());
                vec![active.clone(), unknown]
            },
            {
                let mut drift = terminal.clone();
                drift.policy = firm_core::ProviderCompatibilityAdmissionPolicy::Advisory;
                vec![active.clone(), drift]
            },
            {
                let mut drift = terminal.clone();
                drift.store_id = "store-2".to_string();
                vec![active.clone(), drift]
            },
            vec![active.clone(), terminal.clone(), {
                let mut fork = terminal.clone();
                fork.id = "fork".to_string();
                fork
            }],
        ];

        for rows in cases {
            let text = rows
                .iter()
                .map(|row| serde_json::to_string(row).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            std::fs::write(root.join(PROVIDER_COMPATIBILITY_ADMISSIONS_LEDGER), text).unwrap();
            assert!(matches!(
                store.provider_compatibility_admissions(),
                Err(StoreError::Conflict(_))
            ));
        }
    }

    #[test]
    fn exclusive_migration_guard_blocks_normal_store_writers_until_drop() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-migration-guard-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("init store");
        let guard = store
            .acquire_exclusive_migration_guard()
            .expect("migration guard");
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let writer_store = store.clone();
        let writer = std::thread::spawn(move || {
            started_tx.send(()).expect("signal writer start");
            let result = writer_store.append_mission(&Mission {
                id: "mission-after-migration".into(),
                title: "Blocked writer".into(),
                objective: "Prove the migration guard shares the writer lock".into(),
                context: String::new(),
                desired_outcome: None,
                status: MissionStatus::Planned,
                wave_ids: Vec::new(),
                outcome_summary: None,
                completed_by: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            });
            done_tx.send(result).expect("signal writer completion");
        });

        started_rx.recv().expect("writer started");
        assert!(
            done_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "normal writer must remain blocked while migration guard is alive"
        );
        assert!(
            !root.join("missions.jsonl").exists(),
            "blocked writer must not mutate the ledger"
        );

        drop(guard);
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("writer unblocked after guard drop")
            .expect("writer append succeeds");
        writer.join().expect("writer thread");
        assert_eq!(store.missions().expect("missions").len(), 1);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn mission_and_wave_ledgers_keep_history_and_project_latest_rows() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-mission-wave-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        let mission = Mission {
            id: "mission-1".into(),
            title: "Ship Mission/Wave".into(),
            objective: "Add the migration foundation".into(),
            context: String::new(),
            desired_outcome: Some("A compatible, durable contract".into()),
            status: MissionStatus::Planned,
            wave_ids: vec!["wave-1".into()],
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        let mut updated_mission = mission.clone();
        updated_mission.status = MissionStatus::Running;
        updated_mission.updated_at = "unix-ms:2".into();
        store.append_mission(&mission).expect("append mission");
        store
            .append_mission(&updated_mission)
            .expect("append updated mission");

        let wave = Wave {
            id: "wave-1".into(),
            mission_id: "mission-1".into(),
            index: 1,
            title: "Contract".into(),
            objective: "Define the additive contract".into(),
            context: String::new(),
            revision: 1,
            updated_by: Some("host".into()),
            exit_criteria: Some("Schema and store rows validate".into()),
            status: WaveStatus::Running,
            executor_kind: WaveExecutorKind::AgentTeam,
            executor_run_ids: vec!["team-run-1".into()],
            accepted_run_id: None,
            plan_note: None,
            outcome_summary: None,
            artifact_refs: vec!["schemas/mission.schema.json".into()],
            gate_status: WaveGateStatus::Pending,
            gate_note: None,
            accepted_by: None,
            accepted_at: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        };
        let mut accepted_wave = wave.clone();
        accepted_wave.status = WaveStatus::Completed;
        accepted_wave.accepted_run_id = Some("team-run-1".into());
        accepted_wave.gate_status = WaveGateStatus::Accepted;
        accepted_wave.accepted_by = Some("host".into());
        accepted_wave.accepted_at = Some("unix-ms:2".into());
        accepted_wave.updated_at = "unix-ms:2".into();
        store.append_wave(&wave).expect("append wave");
        store
            .append_wave(&accepted_wave)
            .expect("append accepted wave");

        assert_eq!(store.missions().expect("raw missions").len(), 2);
        assert_eq!(store.waves().expect("raw waves").len(), 2);
        assert_eq!(
            store.latest_missions().expect("latest missions"),
            vec![updated_mission]
        );
        assert_eq!(
            store.latest_waves().expect("latest waves"),
            vec![accepted_wave]
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    /// ADR 0051 changed `compare_and_close_mission` to skip the Wave-gate
    /// check entirely for a Mission whose `wave_ids` is empty (the only
    /// shape a NEW Mission can have now, since Wave create is retired). This
    /// proves the OTHER branch is untouched: a Mission that already
    /// accumulated `wave_ids` before the cutover still requires every one
    /// of them to be an accepted, completed Wave -- its in-flight contract
    /// does not silently change underneath it.
    #[test]
    fn mission_close_with_legacy_wave_ids_still_requires_accepted_gate() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-legacy-mission-close-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        let mission = Mission {
            id: "mission-legacy".into(),
            title: "Pre-cutover Mission".into(),
            objective: "Already has Wave membership from before ADR 0051".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Running,
            wave_ids: vec!["wave-legacy".into()],
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_mission(&mission).expect("append mission");
        let pending_wave = Wave {
            id: "wave-legacy".into(),
            mission_id: "mission-legacy".into(),
            index: 1,
            title: "Legacy Wave".into(),
            objective: "Not yet accepted".into(),
            context: String::new(),
            revision: 1,
            updated_by: Some("host".into()),
            exit_criteria: None,
            status: WaveStatus::Running,
            executor_kind: WaveExecutorKind::Host,
            executor_run_ids: Vec::new(),
            accepted_run_id: None,
            plan_note: None,
            outcome_summary: None,
            artifact_refs: Vec::new(),
            gate_status: WaveGateStatus::Pending,
            gate_note: None,
            accepted_by: None,
            accepted_at: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        };
        store.append_wave(&pending_wave).expect("append wave");

        let mut closed = mission.clone();
        closed.status = MissionStatus::Completed;
        closed.outcome_summary = Some("done".into());
        closed.completed_by = Some("host".into());
        closed.completed_at = Some("unix-ms:2".into());
        closed.updated_at = "unix-ms:2".into();
        let error = store
            .compare_and_close_mission(&mission, &closed)
            .expect_err("a pending legacy Wave must still block closeout");
        assert!(
            error.to_string().contains("cannot close: Wave"),
            "error: {error}"
        );

        let mut accepted_wave = pending_wave.clone();
        accepted_wave.status = WaveStatus::Completed;
        accepted_wave.gate_status = WaveGateStatus::Accepted;
        accepted_wave.accepted_by = Some("host".into());
        accepted_wave.accepted_at = Some("unix-ms:2".into());
        accepted_wave.updated_at = "unix-ms:2".into();
        store
            .compare_and_append_wave(&pending_wave, &accepted_wave)
            .expect("accept the legacy wave");

        // compare_and_append_wave folds the gate outcome into Mission.phase
        // as a side effect (line ~754 above), so the CAS baseline for close
        // must be the freshly stored row, not the pre-gate local `mission`.
        let after_gate = store
            .latest_missions()
            .expect("latest missions")
            .into_iter()
            .find(|row| row.id == "mission-legacy")
            .expect("mission row after gate acceptance");
        let mut closed_after_gate = after_gate.clone();
        closed_after_gate.status = MissionStatus::Completed;
        closed_after_gate.outcome_summary = Some("done".into());
        closed_after_gate.completed_by = Some("host".into());
        closed_after_gate.completed_at = Some("unix-ms:3".into());
        closed_after_gate.updated_at = "unix-ms:3".into();
        store
            .compare_and_close_mission(&after_gate, &closed_after_gate)
            .expect("an accepted legacy Wave allows closeout, same as before ADR 0051");
        assert_eq!(
            store
                .latest_missions()
                .expect("latest missions")
                .into_iter()
                .find(|row| row.id == "mission-legacy")
                .expect("closed mission row")
                .status,
            MissionStatus::Completed
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn native_wave_attempt_and_event_updates_are_concurrency_safe() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-native-concurrency-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = Arc::new(HarnessStore::new(&root));
        store
            .insert_mission(&Mission {
                id: "mission-concurrent".into(),
                title: "Concurrent Mission".into(),
                objective: "Keep native joins lossless".into(),
                context: String::new(),
                desired_outcome: None,
                status: MissionStatus::Planned,
                wave_ids: Vec::new(),
                outcome_summary: None,
                completed_by: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            })
            .expect("insert mission");

        let wave_barrier = Arc::new(Barrier::new(2));
        let wave_handles = ["wave-a", "wave-b"].map(|id| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&wave_barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.insert_wave_and_update_mission(
                    Wave {
                        id: id.into(),
                        mission_id: "mission-concurrent".into(),
                        index: 0,
                        title: id.into(),
                        objective: "one ordered wave".into(),
                        context: String::new(),
                        revision: 1,
                        updated_by: Some("host".into()),
                        exit_criteria: None,
                        status: WaveStatus::Planned,
                        executor_kind: WaveExecutorKind::AgentTeam,
                        executor_run_ids: Vec::new(),
                        accepted_run_id: None,
                        plan_note: None,
                        outcome_summary: None,
                        artifact_refs: Vec::new(),
                        gate_status: WaveGateStatus::Pending,
                        gate_note: None,
                        accepted_by: None,
                        accepted_at: None,
                        created_at: "unix-ms:2".into(),
                        updated_at: "unix-ms:2".into(),
                    },
                    None,
                    "unix-ms:2",
                )
            })
        });
        for handle in wave_handles {
            handle.join().expect("wave thread").expect("insert wave");
        }
        let waves = store.latest_waves().expect("latest waves");
        assert_eq!(
            waves.iter().map(|wave| wave.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let mission = store.latest_missions().expect("latest missions").remove(0);
        assert_eq!(
            mission.wave_ids,
            vec![waves[0].id.clone(), waves[1].id.clone()]
        );

        let mut max_index_wave = waves[0].clone();
        max_index_wave.id = "wave-max-index".into();
        max_index_wave.index = u32::MAX;
        max_index_wave.executor_run_ids.clear();
        store
            .insert_wave_and_update_mission(max_index_wave.clone(), Some(u32::MAX), "unix-ms:2")
            .expect("insert maximum explicit wave index");
        let mut overflow_wave = max_index_wave;
        overflow_wave.id = "wave-overflow".into();
        let error = store
            .insert_wave_and_update_mission(overflow_wave, None, "unix-ms:2")
            .expect_err("implicit wave index must not overflow");
        assert!(
            error.to_string().contains("index space is exhausted"),
            "error: {error}"
        );

        let event_run_id = "team-run-concurrent-events".to_string();

        let event_barrier = Arc::new(Barrier::new(8));
        let event_handles = (0..8)
            .map(|index| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&event_barrier);
                let event_run_id = event_run_id.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append_team_run_event_next(TeamRunEvent {
                        id: format!("event-{index}"),
                        seq: 0,
                        team_run_id: event_run_id,
                        source_kind: TeamRunEventSourceKind::Host,
                        member_run_id: None,
                        delegation_run_id: None,
                        entity_type: "message".into(),
                        entity_id: format!("message-{index}"),
                        operation: "created".into(),
                        summary: "concurrent".into(),
                        occurred_at: "unix-ms:4".into(),
                    })
                })
            })
            .collect::<Vec<_>>();
        for handle in event_handles {
            handle.join().expect("event thread").expect("append event");
        }
        let mut seqs = store
            .team_run_events()
            .expect("events")
            .into_iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>();
        seqs.sort_unstable();
        assert_eq!(seqs, (1..=8).collect::<Vec<_>>());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    /// A minimal native Mission for Mission Log tests below.
    fn mission_log_test_mission(id: &str) -> Mission {
        Mission {
            id: id.into(),
            title: "Ship the Mission Log cutover".into(),
            objective: "Prove append-only Mission Log semantics".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Planned,
            wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        }
    }

    #[test]
    fn mission_log_entries_round_trip_with_ordered_revisions_and_tail() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-mission-log-round-trip-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store
            .insert_mission(&mission_log_test_mission("mission-log-1"))
            .expect("insert mission");

        let kinds = [
            MissionLogEntryKind::Judgment,
            MissionLogEntryKind::Replan,
            MissionLogEntryKind::Recovery,
            MissionLogEntryKind::CloseoutEvidence,
        ];
        for (index, kind) in kinds.iter().enumerate() {
            let appended = store
                .append_mission_log_entry(MissionLogEntry {
                    id: format!("entry-{index}"),
                    mission_id: "mission-log-1".into(),
                    revision: 0, // store-assigned; must be overwritten below
                    kind: *kind,
                    body: format!("entry body {index}"),
                    actor: "host".into(),
                    created_at: format!("unix-ms:{index}"),
                })
                .unwrap_or_else(|error| panic!("append entry {index}: {error}"));
            // Store-assigned, monotonic starting at 1 -- the CLI's placeholder
            // `revision: 0` is never trusted back.
            assert_eq!(appended.revision, (index + 1) as u32);
        }

        // A second Mission's entries never leak into the first Mission's
        // ledger, exactly like Wave's per-mission index scoping.
        store
            .insert_mission(&mission_log_test_mission("mission-log-2"))
            .expect("insert other mission");
        store
            .append_mission_log_entry(MissionLogEntry {
                id: "entry-other-mission".into(),
                mission_id: "mission-log-2".into(),
                revision: 0,
                kind: MissionLogEntryKind::Judgment,
                body: "unrelated mission's judgment".into(),
                actor: "host".into(),
                created_at: "unix-ms:9".into(),
            })
            .expect("append other-mission entry");

        let entries = store
            .mission_log_entries("mission-log-1")
            .expect("mission log entries");
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.revision)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(entries[0].kind, MissionLogEntryKind::Judgment);
        assert_eq!(entries[3].kind, MissionLogEntryKind::CloseoutEvidence);

        // tail(2) returns the last two, oldest-of-the-tail first (Unix `tail`
        // ordering), never the unrelated Mission's row.
        let tail = store
            .mission_log_tail("mission-log-1", 2)
            .expect("mission log tail");
        assert_eq!(
            tail.iter().map(|entry| entry.revision).collect::<Vec<_>>(),
            vec![3, 4]
        );

        // tail(n) larger than the ledger returns every row, not an error.
        let full_tail = store
            .mission_log_tail("mission-log-1", 100)
            .expect("mission log tail overshoot");
        assert_eq!(full_tail.len(), 4);

        // A Mission with no entries yet has an empty tail, not an error --
        // the CLI/skill treat this as "no mission log yet", not a failure.
        store
            .insert_mission(&mission_log_test_mission("mission-log-empty"))
            .expect("insert empty mission");
        assert_eq!(
            store
                .mission_log_tail("mission-log-empty", 3)
                .expect("empty tail"),
            Vec::new()
        );

        // The raw cross-mission ledger sees every row in append order.
        assert_eq!(store.mission_log().expect("raw mission log").len(), 5);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn mission_log_entry_rejects_empty_body_empty_actor_and_missing_mission() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-mission-log-validation-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store
            .insert_mission(&mission_log_test_mission("mission-log-validate"))
            .expect("insert mission");

        let base = MissionLogEntry {
            id: "entry-invalid".into(),
            mission_id: "mission-log-validate".into(),
            revision: 0,
            kind: MissionLogEntryKind::Judgment,
            body: "   ".into(),
            actor: "host".into(),
            created_at: "unix-ms:1".into(),
        };
        let empty_body_error = store
            .append_mission_log_entry(base.clone())
            .expect_err("whitespace-only body must be rejected");
        assert!(
            empty_body_error
                .to_string()
                .contains("body must not be empty"),
            "error: {empty_body_error}"
        );

        let mut empty_actor = base.clone();
        empty_actor.body = "a real judgment".into();
        empty_actor.actor = "  ".into();
        let empty_actor_error = store
            .append_mission_log_entry(empty_actor)
            .expect_err("whitespace-only actor must be rejected");
        assert!(
            empty_actor_error
                .to_string()
                .contains("actor must not be empty"),
            "error: {empty_actor_error}"
        );

        let mut missing_mission = base.clone();
        missing_mission.body = "a real judgment".into();
        missing_mission.mission_id = "mission-log-does-not-exist".into();
        let missing_mission_error = store
            .append_mission_log_entry(missing_mission)
            .expect_err("unknown mission must be rejected");
        assert!(
            missing_mission_error
                .to_string()
                .contains("mission not found"),
            "error: {missing_mission_error}"
        );

        // No invalid attempt above left a row behind.
        assert_eq!(
            store
                .mission_log_entries("mission-log-validate")
                .expect("mission log entries")
                .len(),
            0
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn mission_log_entry_revision_is_monotonic_under_concurrent_append() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-mission-log-concurrency-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = Arc::new(HarnessStore::new(&root));
        store
            .insert_mission(&mission_log_test_mission("mission-log-concurrent"))
            .expect("insert mission");

        let barrier = Arc::new(Barrier::new(4));
        let handles = ["a", "b", "c", "d"].map(|tag| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.append_mission_log_entry(MissionLogEntry {
                    id: format!("entry-concurrent-{tag}"),
                    mission_id: "mission-log-concurrent".into(),
                    revision: 0,
                    kind: MissionLogEntryKind::Judgment,
                    body: format!("concurrent judgment {tag}"),
                    actor: "host".into(),
                    created_at: "unix-ms:1".into(),
                })
            })
        });
        let mut revisions = Vec::new();
        for handle in handles {
            revisions.push(
                handle
                    .join()
                    .expect("append thread")
                    .expect("append entry")
                    .revision,
            );
        }
        revisions.sort_unstable();
        // Four concurrent appends against the same Mission never collide or
        // skip: the store lock serializes the max-plus-one allocation exactly
        // like insert_wave_and_update_mission's index allocation.
        assert_eq!(revisions, vec![1, 2, 3, 4]);
        let stored_revisions = store
            .mission_log_entries("mission-log-concurrent")
            .expect("mission log entries")
            .into_iter()
            .map(|entry| entry.revision)
            .collect::<Vec<_>>();
        assert_eq!(stored_revisions, vec![1, 2, 3, 4]);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn concurrent_appends_write_complete_jsonl_rows() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-concurrent-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = Arc::new(HarnessStore::new(&root));
        let worker_count = 8;
        let appends_per_worker = 25;
        let barrier = Arc::new(Barrier::new(worker_count));
        let mut handles = Vec::new();

        for worker in 0..worker_count {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                for index in 0..appends_per_worker {
                    let mission = Mission {
                        id: format!("mission-{worker}-{index}"),
                        title: "Concurrent".into(),
                        objective: "Exercise concurrent append integrity".into(),
                        context: String::new(),
                        desired_outcome: None,
                        status: MissionStatus::Running,
                        wave_ids: Vec::new(),
                        outcome_summary: None,
                        completed_by: None,
                        created_at: "2026-05-26T00:00:00Z".into(),
                        updated_at: "2026-05-26T00:00:00Z".into(),
                        completed_at: None,
                    };
                    store.append_mission(&mission).expect("append mission");
                }
            }));
        }

        for handle in handles {
            handle.join().expect("worker thread");
        }

        let missions = store.missions().expect("read missions");
        assert_eq!(missions.len(), worker_count * appends_per_worker);
        let ids = missions
            .iter()
            .map(|mission| mission.id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), worker_count * appends_per_worker);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_uses_unlocked_existing_lock_file() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-stale-lock-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("init store");
        std::fs::write(root.join(".store.lock"), "left by interrupted writer\n")
            .expect("write existing lock file");
        let mission = Mission {
            id: "mission-stale-lock".into(),
            title: "Stale lock".into(),
            objective: "Verify an unlocked existing lock file is reusable".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Running,
            wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "2026-05-26T00:00:00Z".into(),
            updated_at: "2026-05-26T00:00:00Z".into(),
            completed_at: None,
        };

        store
            .append_mission(&mission)
            .expect("append with unlocked lock file");
        assert_eq!(store.missions().expect("read missions"), vec![mission]);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn claim_queued_message_is_atomic_and_blocks_second_claim() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-claim-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store
            .append_message(&test_message("message-1", "agent-1"))
            .expect("append message 1");
        store
            .append_message(&test_message("message-2", "agent-1"))
            .expect("append message 2");

        let claim = store
            .claim_queued_message_delivery("agent-1", "message-1", test_delivery("delivery-1"))
            .expect("claim message");
        assert!(matches!(claim, MessageDeliveryClaimResult::Claimed(_)));

        let latest_message = store
            .messages()
            .expect("messages")
            .into_iter()
            .rev()
            .find(|message| message.id == "message-1")
            .expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            RegistryDeliveryStatus::Acknowledged
        );
        assert_eq!(
            latest_message
                .delivery
                .and_then(|delivery| delivery.delivery_id),
            Some("delivery-1".into())
        );

        let second_claim = store
            .claim_queued_message_delivery("agent-1", "message-2", test_delivery("delivery-2"))
            .expect("second claim");
        assert_eq!(
            second_claim,
            MessageDeliveryClaimResult::BlockedByDelivery("delivery-1".into())
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    /// Durability: a claim writes and fsyncs the Acknowledged message row with
    /// its Running delivery attempt, and a *separate* store handle opened
    /// against the same root (no shared in-memory state, mirroring a process
    /// restart after a crash) reads them back. This guards the double-delivery
    /// regression: if the Acknowledged row were lost, latest-wins would revert
    /// the message to Queued and it would be claimable again.
    #[test]
    fn claim_appends_survive_reopen() {
        let root = std::env::temp_dir().join(format!(
            "firm-store-durability-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store
            .append_message(&test_message("message-d", "agent-d"))
            .expect("append message");

        let claim = store
            .claim_queued_message_delivery("agent-d", "message-d", test_delivery("delivery-d"))
            .expect("claim message");
        assert!(matches!(claim, MessageDeliveryClaimResult::Claimed(_)));

        // Reopen with a fresh handle: only on-disk (fsynced) state is visible.
        let reopened = HarnessStore::new(&root);

        let message = reopened
            .messages()
            .expect("read messages")
            .into_iter()
            .rev()
            .find(|message| message.id == "message-d")
            .expect("acknowledged message row survives reopen");
        assert_eq!(
            message.delivery_status,
            RegistryDeliveryStatus::Acknowledged,
            "acknowledged status must survive a restart so the message is not re-delivered"
        );

        let delivery = message.delivery.expect("delivery attempt survives reopen");
        assert_eq!(delivery.delivery_id.as_deref(), Some("delivery-d"));
        assert_eq!(
            delivery.execution_status,
            Some(ProviderExecutionStatus::Running)
        );

        // The reopened store must refuse to re-claim: because both the
        // Acknowledged message row and its Running delivery attempt survived
        // the fsync, the re-claim is rejected (the active attempt for this
        // agent blocks delivery; were the row lost it would return Claimed and
        // double-deliver). Either rejection variant proves no double-delivery.
        let reclaim = reopened
            .claim_queued_message_delivery("agent-d", "message-d", test_delivery("delivery-d2"))
            .expect("reclaim attempt");
        assert!(
            !matches!(reclaim, MessageDeliveryClaimResult::Claimed(_)),
            "fsynced claim state must prevent a second delivery, got {reclaim:?}"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn team_test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "firm-store-team-test-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ))
    }

    fn seed_host_attention_fixture(
        store: &HarnessStore,
        run_id: &str,
        host_thread_id: Option<&str>,
    ) -> (AgentTeamRun, ProviderRuntimeProjection, Work) {
        let run = AgentTeamRun {
            id: run_id.into(),
            agent_team_id: format!("team-{run_id}"),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: host_thread_id.map(str::to_string),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "prove exact Host attention".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec![format!("member-{run_id}")],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("seed TeamRun");
        let member = ProviderRuntimeProjection {
            id: format!("member-{run_id}"),
            team_run_id: run_id.into(),
            slot_id: None,
            agent_member_id: format!("agent-{run_id}"),
            name: "builder".into(),
            role: "builder".into(),
            provider: "kimi".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Idle,
            native_session: None,
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            started_at: "unix-ms:1".into(),
            last_event_at: None,
            finished_at: None,
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };
        store
            .append_member_run(&member)
            .expect("seed ProviderRuntimeProjection");
        let work = store
            .insert_work(
                Work {
                    id: format!("work-{run_id}"),
                    team_run_id: run_id.into(),
                    team_id: None,
                    parent_work_id: None,
                    title: "deliver exact Host attention".into(),
                    context_markdown: String::new(),
                    completion_criteria_markdown: "Host receives exact durable attention".into(),
                    phase: WorkPhase::Open,
                    condition: WorkCondition::Normal,
                    resolution: None,
                    owner_member_id: None,
                    active_member_run_id: Some(member.id.clone()),
                    claim_mode: WorkClaimMode::HostAssign,
                    eligible_member_ids: Vec::new(),
                    prerequisite_work_ids: Vec::new(),
                    priority: WorkPriority::Normal,
                    created_by_member_id: None,
                    created_by_actor: TeamActorRef {
                        kind: TeamActorKind::Host,
                        id: "host".into(),
                        display_name: None,
                        authn_source: None,
                    },
                    result_summary: None,
                    blocker_reason: None,
                    artifact_refs: Vec::new(),
                    check_refs: Vec::new(),
                    github_links: Vec::new(),
                    version: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                WorkCommandContext {
                    event_id: format!("work-event-{run_id}"),
                    performed_by_actor: TeamActorRef {
                        kind: TeamActorKind::Host,
                        id: "host".into(),
                        display_name: None,
                        authn_source: None,
                    },
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("create-work-{run_id}"),
                    created_at: "unix-ms:2".into(),
                    duplicate_ok: false,
                },
            )
            .expect("seed Work");
        (run, member, work)
    }

    fn seed_test_host_attention(
        store: &HarnessStore,
        run: &AgentTeamRun,
        member: &ProviderRuntimeProjection,
        work: &Work,
        id: &str,
        created_at: &str,
    ) -> HostAttention {
        let attention = HostAttention {
            id: id.into(),
            team_run_id: run.id.clone(),
            kind: HostAttentionKind::WorkReviewRequested,
            work_id: work.id.clone(),
            work_version: work.version,
            source_event_ref: format!("source-{id}"),
            member_run_id: Some(member.id.clone()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            claimed_host_lease_id: None,
            claimed_host_lease_generation: None,
            claimed_host_lease_owner_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: created_at.into(),
            updated_at: created_at.into(),
        };
        store
            .ensure_host_attention(&attention)
            .expect("seed Host attention");
        attention
    }

    #[test]
    fn host_binding_lease_acquire_renew_release_takeover_and_stale_fence() {
        let root = team_test_root("host-binding-lease-lifecycle");
        let store = HarnessStore::new(&root);
        let (run, _, _) = seed_host_attention_fixture(&store, "lease-lifecycle", Some("thread-a"));

        assert_eq!(
            store.latest_host_binding_lease(&run.id).unwrap(),
            None,
            "legacy binding is explicitly unleased"
        );
        let first = store
            .acquire_host_binding_lease(
                &run.id,
                "codex-app",
                "thread-a",
                HostBindingLeaseOwnerKind::Interactive,
                "human-a",
                "lease-a",
                100,
                50,
            )
            .expect("acquire interactive lease");
        assert_eq!(first.generation, 1);
        assert_eq!(
            store.effective_host_binding_lease_at(&run.id, 149).unwrap(),
            Some(first.clone())
        );
        assert!(store
            .effective_host_binding_lease_at(&run.id, 150)
            .unwrap()
            .is_none());

        let second = store
            .acquire_host_binding_lease(
                &run.id,
                "codex-app",
                "thread-a",
                HostBindingLeaseOwnerKind::Dispatcher,
                "dispatcher-b",
                "lease-b",
                150,
                100,
            )
            .expect("expired takeover");
        assert_eq!(second.generation, 2);
        assert!(store.renew_host_binding_lease(&first, 151, 100).is_err());
        let renewed = store
            .renew_host_binding_lease(&second, 175, 100)
            .expect("renew exact lease");
        assert_eq!(renewed.expires_unix_ms, 275);
        let released = store
            .release_host_binding_lease(&renewed, 180)
            .expect("release exact lease");
        assert_eq!(released.status, HostBindingLeaseStatus::Released);
        assert!(store.renew_host_binding_lease(&renewed, 181, 100).is_err());
        assert_eq!(
            store
                .release_host_binding_lease(&released, 999)
                .expect("release retry")
                .released_unix_ms,
            Some(180)
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn host_binding_stale_attention_is_derived_and_idempotent() {
        let root = team_test_root("host-binding-stale-attention");
        let store = HarnessStore::new(&root);
        let (run, _, _) = seed_host_attention_fixture(&store, "lease-stale", Some("thread-a"));

        let first = store
            .reconcile_host_binding_stale_attentions(100, "unix-ms:100")
            .expect("derive unleased attention");
        let retry = store
            .reconcile_host_binding_stale_attentions(101, "unix-ms:101")
            .expect("repeat scan");
        assert_eq!(first.len(), 1);
        assert_eq!(retry.len(), 1);
        assert_eq!(first[0].id, retry[0].id);
        assert_eq!(first[0].kind, HostAttentionKind::HostBindingStale);
        assert_eq!(
            store
                .host_attentions()
                .unwrap()
                .into_iter()
                .filter(|attention| attention.kind == HostAttentionKind::HostBindingStale)
                .count(),
            1
        );

        let lease = store
            .acquire_host_binding_lease(
                &run.id,
                "codex",
                "thread-a",
                HostBindingLeaseOwnerKind::Interactive,
                "human",
                "lease-live",
                110,
                10,
            )
            .unwrap();
        assert!(store
            .reconcile_host_binding_stale_attentions(119, "unix-ms:119")
            .unwrap()
            .is_empty());
        let expired = store
            .reconcile_host_binding_stale_attentions(120, "unix-ms:120")
            .unwrap();
        assert_eq!(expired.len(), 1);
        assert_ne!(expired[0].id, first[0].id);
        assert_eq!(lease.generation, 1);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn terminal_team_runs_do_not_materialize_host_binding_stale_attention() {
        for status in [
            TeamRunStatus::Completed,
            TeamRunStatus::Failed,
            TeamRunStatus::Cancelled,
        ] {
            let root = team_test_root(&format!("terminal-stale-{status:?}"));
            let store = HarnessStore::new(&root);
            let (run, _, _) = seed_host_attention_fixture(
                &store,
                &format!("terminal-stale-{status:?}"),
                Some("thread-a"),
            );
            let mut terminal = run;
            terminal.status = status;
            terminal.completed_at = Some("unix-ms:2".into());
            terminal.updated_at = "unix-ms:2".into();
            append_sparse_row(
                &root,
                "team_runs.jsonl",
                &serde_json::to_string(&terminal).expect("serialize terminal run"),
            );
            assert!(store
                .reconcile_host_binding_stale_attentions(100, "unix-ms:100")
                .expect("reconcile")
                .is_empty());
            assert!(!store
                .host_attentions()
                .unwrap()
                .iter()
                .any(|attention| attention.kind == HostAttentionKind::HostBindingStale));
            std::fs::remove_dir_all(root).expect("remove temp store");
        }
    }

    #[test]
    fn host_binding_interactive_suppresses_dispatch_and_atomic_batch_has_one_winner() {
        let root = team_test_root("host-binding-dispatch-race");
        let store = HarnessStore::new(&root);
        let (run, member, work) =
            seed_host_attention_fixture(&store, "lease-dispatch", Some("thread-a"));
        seed_test_host_attention(
            &store,
            &run,
            &member,
            &work,
            "attention-dispatch-race",
            "unix-ms:10",
        );
        let interactive = store
            .acquire_host_binding_lease(
                &run.id,
                "codex-app",
                "thread-a",
                HostBindingLeaseOwnerKind::Interactive,
                "human",
                "lease-human",
                100,
                10,
            )
            .unwrap();
        let suppressed = store.claim_dispatcher_host_attention_batch(
            &interactive,
            100,
            10,
            "suppressed",
            101,
            "unix-ms:101",
        );
        assert!(suppressed
            .unwrap_err()
            .to_string()
            .contains("INTERACTIVE_SUPPRESSES_DISPATCH"));

        let dispatcher = store
            .acquire_host_binding_lease(
                &run.id,
                "codex-app",
                "thread-a",
                HostBindingLeaseOwnerKind::Dispatcher,
                "dispatcher",
                "lease-dispatcher",
                110,
                100,
            )
            .expect("take over expired interactive lease");
        let store = std::sync::Arc::new(store);
        let handles = (0..2)
            .map(|index| {
                let store = std::sync::Arc::clone(&store);
                let dispatcher = dispatcher.clone();
                std::thread::spawn(move || {
                    store
                        .claim_dispatcher_host_attention_batch(
                            &dispatcher,
                            100,
                            10,
                            &format!("batch-{index}"),
                            111,
                            "unix-ms:111",
                        )
                        .expect("batch claim")
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("claim thread"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|batch| !batch.is_empty()).count(), 1);
        assert_eq!(results.iter().map(Vec::len).sum::<usize>(), 1);
        let claimed = results.into_iter().flatten().next().unwrap();
        assert_eq!(claimed.claimed_host_lease_generation, Some(2));

        let released = store
            .release_host_binding_lease(&dispatcher, 112)
            .expect("release dispatcher");
        assert!(store
            .complete_host_attention_claim(
                &claimed.id,
                claimed.claim_id.as_deref().unwrap(),
                "receipt",
                "unix-ms:113",
            )
            .unwrap_err()
            .to_string()
            .contains("LEASE_FENCED"));
        assert_eq!(released.status, HostBindingLeaseStatus::Released);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn host_attention_is_durable_exact_bound_and_semantically_separate() {
        let root = team_test_root("host-attention");
        let store = HarnessStore::new(&root);
        let (run, member, work) = seed_host_attention_fixture(&store, "run-a", None);
        let attention = HostAttention {
            id: "host-attention-work-review-a".into(),
            team_run_id: run.id.clone(),
            kind: HostAttentionKind::WorkReviewRequested,
            work_id: work.id.clone(),
            work_version: work.version,
            source_event_ref: "work-event-review-a".into(),
            member_run_id: Some(member.id.clone()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            claimed_host_lease_id: None,
            claimed_host_lease_generation: None,
            claimed_host_lease_owner_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: "unix-ms:3".into(),
            updated_at: "unix-ms:3".into(),
        };
        store
            .ensure_host_attention(&attention)
            .expect("append attention");
        assert!(
            store.team_messages().expect("messages").is_empty(),
            "Work state attention must not fabricate ProviderDispatchEnvelope conversation"
        );
        let unbound = store
            .host_attention_inbox_for_team_run(&run.id, false)
            .expect("unbound projection");
        assert_eq!(unbound.attentions.len(), 1);
        assert!(unbound.warning.as_deref().is_some_and(|warning| {
            warning.contains("UNBOUND_HOST") && warning.contains(&run.id)
        }));
        assert!(store
            .host_attention_inboxes_for_native_thread("codex-app", "other-task", false)
            .expect("other task")
            .is_empty());

        let mut bound = run.clone();
        bound.host_thread_id = Some("codex-task-a".into());
        bound.updated_at = "unix-ms:4".into();
        store
            .compare_and_append_team_run(&run, &bound)
            .expect("bind exact Host task");
        let exact = store
            .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false)
            .expect("exact Host inbox");
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].attentions[0].id, attention.id);
        assert!(store
            .host_attention_inboxes_for_native_thread("codex-app", "codex-task-b", false)
            .expect("other exact task")
            .is_empty());

        let claimed = store
            .claim_host_attention(
                &attention.id,
                "codex-app",
                "codex-task-a",
                "claim-a",
                "unix-ms:5",
            )
            .expect("claim attention");
        assert!(matches!(claimed, HostAttentionClaimResult::Claimed(_)));
        assert!(matches!(
            store
                .claim_host_attention(
                    &attention.id,
                    "codex-app",
                    "codex-task-a",
                    "claim-a",
                    "unix-ms:5",
                )
                .expect("idempotent claim"),
            HostAttentionClaimResult::Claimed(_)
        ));
        assert!(store
            .claim_host_attention(
                &attention.id,
                "codex-app",
                "codex-task-a",
                "claim-b",
                "unix-ms:5",
            )
            .is_ok_and(|result| result == HostAttentionClaimResult::NotActionable));

        let delivered = store
            .complete_host_attention_claim(
                &attention.id,
                "claim-a",
                "codex-turn-start-1",
                "unix-ms:6",
            )
            .expect("record provider receipt");
        assert_eq!(delivered.status, HostAttentionStatus::Delivered);
        assert!(delivered.needs_host_action());
        assert_eq!(
            store
                .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false,)
                .expect("delivered still actionable")[0]
                .attentions
                .len(),
            1
        );

        let acknowledged = store
            .acknowledge_host_attention(&attention.id, "codex-app", "codex-task-a", "unix-ms:7")
            .expect("Host intake ACK");
        assert_eq!(acknowledged.status, HostAttentionStatus::Acknowledged);
        assert!(store
            .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false)
            .expect("actionable inbox after ACK")
            .is_empty());
        assert_eq!(
            store.latest_works().expect("Work remains")[0].phase,
            WorkPhase::Open,
            "attention ACK must not accept or request changes on Work"
        );
        assert_eq!(
            store
                .ensure_host_attention(&attention)
                .expect("causal replay remains idempotent")
                .status,
            HostAttentionStatus::Acknowledged,
            "replaying projection must not reset Host intake"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn submitted_and_blocked_work_reconcile_exactly_one_host_attention_each() {
        let root = team_test_root("work-host-attention-reconciliation");
        let store = HarnessStore::new(&root);
        let (_review_run, review_member, review_work) =
            seed_host_attention_fixture(&store, "review-run", Some("review-host-task"));
        let started_review = store
            .start_work(
                &review_work.id,
                review_work.version,
                &review_member.id,
                member_work_context(
                    &review_member.id,
                    "work-event-review-started",
                    "work-command-review-started",
                    "unix-ms:3",
                ),
            )
            .expect("start review Work");
        let submitted = store
            .submit_work(
                &started_review.id,
                started_review.version,
                &review_member.id,
                "ready for exact Host review",
                Vec::new(),
                vec!["cargo:test".into()],
                member_work_context(
                    &review_member.id,
                    "work-event-review-submitted",
                    "work-command-review-submitted",
                    "unix-ms:4",
                ),
            )
            .expect("submit Work");

        let (_blocked_run, blocked_member, blocked_work) =
            seed_host_attention_fixture(&store, "blocked-run", Some("blocked-host-task"));
        let started_blocked = store
            .start_work(
                &blocked_work.id,
                blocked_work.version,
                &blocked_member.id,
                member_work_context(
                    &blocked_member.id,
                    "work-event-blocked-started",
                    "work-command-blocked-started",
                    "unix-ms:5",
                ),
            )
            .expect("start blocked Work");
        let blocked = store
            .block_work(
                &started_blocked.id,
                started_blocked.version,
                &blocked_member.id,
                "needs Host decision",
                member_work_context(
                    &blocked_member.id,
                    "work-event-blocked",
                    "work-command-blocked",
                    "unix-ms:6",
                ),
            )
            .expect("block Work");

        let attentions = store.host_attentions().expect("derived Host attentions");
        assert_eq!(attentions.len(), 2);
        let review_attention = attentions
            .iter()
            .find(|attention| attention.work_id == submitted.id)
            .expect("review attention");
        assert_eq!(
            review_attention.id,
            "host-attention-work-event-review-submitted"
        );
        assert_eq!(
            review_attention.kind,
            HostAttentionKind::WorkReviewRequested
        );
        assert_eq!(review_attention.work_version, submitted.version);
        let blocked_attention = attentions
            .iter()
            .find(|attention| attention.work_id == blocked.id)
            .expect("blocked attention");
        assert_eq!(blocked_attention.id, "host-attention-work-event-blocked");
        assert_eq!(blocked_attention.kind, HostAttentionKind::WorkBlocked);
        assert_eq!(blocked_attention.work_version, blocked.version);
        assert!(
            store.team_messages().expect("TeamMessages").is_empty(),
            "Work-state attention must not fabricate conversation"
        );

        // Simulate the process dying after work_operations.jsonl was fsynced
        // but before host_attentions.jsonl reached disk.
        std::fs::remove_file(root.join("host_attentions.jsonl"))
            .expect("remove derived ledger to simulate crash gap");
        let reconciled = store
            .reconcile_work_host_attentions()
            .expect("repair crash gap from WorkEvent truth");
        assert_eq!(reconciled.len(), 2);
        let repaired_bytes = std::fs::read(root.join("host_attentions.jsonl"))
            .expect("repaired Host-attention ledger");
        assert_eq!(
            repaired_bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .count(),
            2
        );
        store
            .reconcile_work_host_attentions()
            .expect("idempotent second reconciliation");
        assert_eq!(
            std::fs::read(root.join("host_attentions.jsonl")).expect("stable ledger"),
            repaired_bytes,
            "reconciliation must not append duplicates"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn jsonl_read_retries_a_concurrently_incomplete_final_row() {
        let root = team_test_root("concurrent-partial-row");
        let store = HarnessStore::new(&root);
        store.init().expect("initialize store");
        let path = root.join("concurrent.jsonl");
        let (partial_ready_tx, partial_ready_rx) = std::sync::mpsc::channel();

        let writer_store = store.clone();
        let writer = std::thread::spawn(move || {
            let _lock = writer_store.acquire_write_lock().expect("writer lock");
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("open concurrent ledger");
            file.write_all(br#"{"id":"row-1""#)
                .expect("write partial row");
            file.flush().expect("flush partial row");
            partial_ready_tx.send(()).expect("announce partial row");
            std::thread::sleep(Duration::from_millis(30));
            file.write_all(b",\"value\":1}\n")
                .expect("finish concurrent row");
            file.sync_all().expect("sync concurrent row");
        });

        partial_ready_rx.recv().expect("wait for partial row");
        let rows = store
            .read_jsonl::<serde_json::Value>("concurrent.jsonl")
            .expect("reader waits for the complete final row");
        writer.join().expect("writer completes");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], "row-1");
        assert_eq!(rows[0]["value"], 1);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn seed_lease_run(store: &HarnessStore, id: &str) {
        store
            .append_team_run(&AgentTeamRun {
                id: id.into(),
                agent_team_id: format!("team-{id}"),
                execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
                project_binding_id: "project-test".into(),
                previous_run_id: None,
                host_surface: "codex-app".into(),
                host_thread_id: None,
                host_actor: None,
                host_control_mode: Default::default(),
                objective: "lease test".into(),
                execution_root: None,
                status: TeamRunStatus::Running,
                member_run_ids: Vec::new(),
                budget_limit_usd: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            })
            .expect("seed run");
    }

    trait TestSupervisorLeaseExt {
        fn acquire_test_supervisor_lease(
            &self,
            team_run_id: &str,
            supervisor_id: &str,
            owner_process_id: u32,
            owner_locator: &str,
            now_unix_ms: u64,
            ttl_ms: u64,
        ) -> StoreResult<TeamSupervisorLease>;
    }

    impl TestSupervisorLeaseExt for HarnessStore {
        fn acquire_test_supervisor_lease(
            &self,
            team_run_id: &str,
            supervisor_id: &str,
            owner_process_id: u32,
            owner_locator: &str,
            now_unix_ms: u64,
            ttl_ms: u64,
        ) -> StoreResult<TeamSupervisorLease> {
            let run = self
                .team_runs()?
                .into_iter()
                .rev()
                .find(|run| run.id == team_run_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!("team run not found: {team_run_id}"))
                })?;
            if !self
                .latest_execution_nodes()?
                .iter()
                .any(|node| node.id == run.execution_node_id)
            {
                self.insert_execution_node(&ExecutionNode {
                    id: run.execution_node_id.clone(),
                    display_name: "test-node".into(),
                    status: ExecutionNodeStatus::Active,
                    created_at: "unix-ms:1".into(),
                    updated_at: "unix-ms:1".into(),
                })?;
            }
            let parent = self.acquire_node_daemon_lease(
                &run.execution_node_id,
                "daemon-test",
                "instance-test",
                now_unix_ms,
                u64::MAX / 2,
            )?;
            self.acquire_team_supervisor_under_node_lease(
                team_run_id,
                &run.execution_node_id,
                &parent.daemon_id,
                parent.generation,
                "space-test",
                &run.project_binding_id,
                supervisor_id,
                owner_process_id,
                owner_locator,
                now_unix_ms,
                ttl_ms,
            )
        }
    }

    /// The tail-window fast path must not change which lease a reader sees,
    /// even when the target row sits far in front of the window.
    #[test]
    fn supervisor_lease_tail_read_agrees_with_full_scan() {
        let root = team_test_root("lease-tail");
        let store = HarnessStore::new(&root);
        seed_lease_run(&store, "run-a");
        seed_lease_run(&store, "run-b");
        store
            .acquire_test_supervisor_lease("run-a", "sup-a", 1, "a", 1_000, 15_000)
            .expect("acquire a");
        store
            .acquire_test_supervisor_lease("run-b", "sup-b", 2, "b", 1_000, 15_000)
            .expect("acquire b");
        // Push run-a's latest row well outside the 256 KiB tail window.
        for tick in 0..4_000u64 {
            store
                .renew_team_supervisor_lease("run-b", "sup-b", 1, 2_000 + tick, 15_000)
                .expect("renew b");
        }

        let tail = store
            .latest_lease_for_run_unlocked("run-a")
            .expect("tail read")
            .expect("run-a lease present");
        let full = latest_by_id(
            store
                .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
                .expect("full scan"),
            |lease| lease.team_run_id.clone(),
        )
        .remove("run-a")
        .expect("run-a in full scan");
        assert_eq!(tail.supervisor_id, full.supervisor_id);
        assert_eq!(tail.generation, full.generation);
        assert_eq!(tail.expires_unix_ms, full.expires_unix_ms);
    }

    /// The tail window may land exactly on a row boundary. Discarding the first
    /// line unconditionally would drop a COMPLETE row; reviewer-reported.
    #[test]
    fn supervisor_lease_tail_keeps_a_row_when_window_lands_on_a_boundary() {
        let root = team_test_root("lease-boundary");
        let store = HarnessStore::new(&root);
        seed_lease_run(&store, "run-a");
        store
            .acquire_test_supervisor_lease("run-a", "sup-a", 1, "a", 1_000, 15_000)
            .expect("acquire");
        for tick in 0..20u64 {
            store
                .renew_team_supervisor_lease("run-a", "sup-a", 1, 1_001 + tick, 15_000)
                .expect("renew");
        }
        let path = root.join("team_supervisor_leases.jsonl");
        let bytes = std::fs::read(&path).expect("read lease file");
        let total = bytes.len() as u64;
        // Start the window exactly at the first byte of the LAST row, i.e. one
        // past the second-to-last newline. The file ends with a newline, so the
        // last newline is the row terminator, not the row start.
        let last_terminator = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .expect("trailing newline");
        let row_start = bytes[..last_terminator]
            .iter()
            .rposition(|&b| b == b'\n')
            .expect("a previous row") as u64
            + 1;
        let window = total - row_start;
        let rows = store
            .read_jsonl_tail::<TeamSupervisorLease>("team_supervisor_leases.jsonl", window)
            .expect("tail read");
        assert_eq!(
            rows.len(),
            1,
            "a window landing on a row boundary must keep that row, got {}",
            rows.len()
        );
    }

    /// Compaction on acquire bounds the file at one row per run and must keep
    /// generation fencing intact.
    #[test]
    fn supervisor_lease_acquire_compacts_and_keeps_fencing() {
        let root = team_test_root("lease-compact");
        let store = HarnessStore::new(&root);
        seed_lease_run(&store, "run-a");
        store
            .acquire_test_supervisor_lease("run-a", "sup-1", 1, "a", 1_000, 10)
            .expect("acquire gen 1");
        for tick in 0..500u64 {
            store
                .renew_team_supervisor_lease("run-a", "sup-1", 1, 1_001 + tick, 10)
                .expect("renew");
        }
        let before = store
            .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
            .expect("read")
            .len();
        assert!(before > 500, "history should be long before compaction");

        // The lease has expired, so a different Supervisor takes generation 2.
        let gen2 = store
            .acquire_test_supervisor_lease("run-a", "sup-2", 3, "b", 900_000, 15_000)
            .expect("acquire gen 2");
        assert_eq!(gen2.generation, 2);

        // Compaction runs before the new row is appended, so one run yields the
        // collapsed prior row plus the freshly acquired lease. The invariant is
        // that the file is bounded by run count rather than by heartbeat count.
        let after = store
            .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
            .expect("read")
            .len();
        assert_eq!(
            after, 2,
            "compaction must bound the file at ~one row per run, got {after} (was {before})"
        );

        // The fenced-out generation must still be rejected after compaction.
        assert!(
            store
                .renew_team_supervisor_lease("run-a", "sup-1", 1, 900_001, 15_000)
                .is_err(),
            "stale generation must not renew"
        );
        let live = store
            .latest_lease_for_run_unlocked("run-a")
            .expect("tail")
            .expect("present");
        assert_eq!(live.supervisor_id, "sup-2");
        assert_eq!(live.generation, 2);
    }

    fn append_sparse_row(root: &Path, file_name: &str, row: &str) {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join(file_name))
            .expect("open jsonl for sparse row");
        writeln!(file, "{row}").expect("write sparse row");
        file.sync_all().expect("sync sparse row");
    }

    #[test]
    fn append_and_read_team_run_jsonl_rejects_legacy_sparse_rows() {
        let root = team_test_root("team-run");
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: "tr-1".into(),
            agent_team_id: "td-1".into(),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-example".into(),
            previous_run_id: Some("tr-0".into()),
            host_surface: "codex-app".into(),
            host_thread_id: Some("thread-1".into()),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "Ship the feature".into(),
            execution_root: Some("/projects/example/worktrees/feature".into()),
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-1".into()],
            budget_limit_usd: Some(12.5),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:2".into(),
            completed_at: None,
        };

        store.append_team_run(&run).expect("append team run");
        // Required Team/Node/Project authority makes legacy sparse rows unreadable
        // after the clean cutover.
        append_sparse_row(
            &root,
            "team_runs.jsonl",
            r#"{"id":"tr-sparse","host_surface":"kimi-cli","objective":"obj","status":"planning","created_at":"unix-ms:3","updated_at":"unix-ms:3"}"#,
        );

        let error = store
            .team_runs()
            .expect_err("legacy sparse TeamRun must not compatibility-read");
        assert!(matches!(error, StoreError::Json(_)));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_member_run_jsonl() {
        let root = team_test_root("member-run");
        let store = HarnessStore::new(&root);
        let member_run = ProviderRuntimeProjection {
            id: "mr-1".into(),
            team_run_id: "tr-1".into(),
            slot_id: Some("slot-1".into()),
            agent_member_id: "agent-worker-1".into(),
            name: "worker-1".into(),
            role: "worker".into(),
            provider: "kimi".into(),
            model: Some("kimi-k2".into()),
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Running,
            native_session: None,
            provider_cwd_hint: Some("/projects/example/worktrees/worker-1".into()),
            provider_environment_observation: Some(MemberWorkspaceSnapshot {
                cwd: "/projects/example/worktrees/worker-1".into(),
                project_binding_id: Some("project-example".into()),
                resolution_source: Some("member_worktree".into()),
                git_head: Some("0123456789abcdef".into()),
                git_branch: Some("feature/worker-1".into()),
                instruction_roots: vec!["/projects/example".into()],
                skill_roots: vec!["/projects/example/.agents/skills".into()],
            }),
            owned_paths: vec!["src/".into()],
            started_at: "unix-ms:1".into(),
            last_event_at: Some("unix-ms:2".into()),
            finished_at: None,
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };

        store
            .append_team_run(&AgentTeamRun {
                id: "tr-1".into(),
                agent_team_id: "team-tr-1".into(),
                execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
                project_binding_id: "project-test".into(),
                previous_run_id: None,
                host_surface: "test".into(),
                host_thread_id: None,
                host_actor: None,
                host_control_mode: Default::default(),
                objective: "member JSONL test".into(),
                execution_root: None,
                status: TeamRunStatus::Planning,
                member_run_ids: vec![member_run.id.clone(), "mr-sparse".into()],
                budget_limit_usd: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            })
            .expect("declare initial TeamRun membership");

        store
            .append_member_run(&member_run)
            .expect("append member run");
        append_sparse_row(
            &root,
            "member_runs.jsonl",
            r#"{"id":"mr-sparse","team_run_id":"tr-1","name":"w","role":"worker","provider":"codex","status":"idle","started_at":"unix-ms:3"}"#,
        );

        let error = store.member_runs().expect_err(
            "ProviderRuntimeProjection without agent_member_id must not compatibility-read",
        );
        assert!(matches!(error, StoreError::Json(_)));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_team_message_jsonl() {
        let root = team_test_root("team-message");
        let store = HarnessStore::new(&root);
        let message = ProviderDispatchEnvelope {
            id: "tm-1".into(),
            team_run_id: "tr-1".into(),
            work_id: None,
            source_plan_ref: Some("wave-2".into()),
            sender: None,
            sender_runtime_id: "host".into(),
            recipients: vec![TeamRecipientRef {
                kind: TeamRecipientKind::Host,
                id: "host".into(),
            }],
            recipient_runtime_ids: vec!["mr-1".into()],
            kind: ProviderDispatchIntent::Message,
            body: "Please review task-1".into(),
            correlation_id: "corr-1".into(),
            causation_id: None,
            response_intent: None,
            evidence_refs: vec!["ev-1".into()],
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "mr-1".into(),
                policy: TeamDeliveryPolicy::Inject,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("test-receipt".into()),
                failure_reason: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:1".into(),
        };

        store
            .append_team_message(&message)
            .expect("append team message");
        append_sparse_row(
            &root,
            "team_messages.jsonl",
            r#"{"id":"tm-sparse","team_run_id":"tr-1","sender_runtime_id":"host","kind":"message","body":"hi","correlation_id":"corr-2","created_at":"unix-ms:3"}"#,
        );

        let messages = store.team_messages().expect("read team messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], message);
        let sparse = &messages[1];
        assert_eq!(sparse.id, "tm-sparse");
        assert_eq!(sparse.kind, ProviderDispatchIntent::Message);
        assert!(sparse.recipient_runtime_ids.is_empty());
        assert!(sparse.causation_id.is_none());
        assert!(sparse.evidence_refs.is_empty());
        assert!(sparse.deliveries.is_empty());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn seed_provider_interaction_bridge(
        store: &HarnessStore,
        run_id: &str,
    ) -> (ProviderInteractionRequestBody, ProviderDispatchEnvelope) {
        let member_id = format!("member-{run_id}");
        let session_id = format!("session-{run_id}");
        store
            .append_team_run(&AgentTeamRun {
                id: run_id.to_string(),
                agent_team_id: format!("team-{run_id}"),
                execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
                project_binding_id: "project-test".into(),
                previous_run_id: None,
                host_surface: "codex-app".into(),
                host_thread_id: Some("host-thread".into()),
                host_actor: None,
                host_control_mode: Default::default(),
                objective: "provider interaction bridge".into(),
                execution_root: None,
                status: TeamRunStatus::Running,
                member_run_ids: vec![member_id.clone()],
                budget_limit_usd: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            })
            .expect("seed TeamRun");
        store
            .append_member_run(&ProviderRuntimeProjection {
                id: member_id.clone(),
                team_run_id: run_id.to_string(),
                slot_id: None,
                agent_member_id: format!("agent-{member_id}"),
                name: "provider member".into(),
                role: "worker".into(),
                provider: "codex".into(),
                model: None,
                provider_controls: Default::default(),
                provider_profile: None,
                provider_capacity: None,
                provider_compatibility_block_cause: None,
                coordination_status: Default::default(),
                runtime_generation: 2,
                status: MemberRunStatus::Waiting,
                native_session: Some(NativeSessionRef {
                    provider: "codex".into(),
                    execution_mode: "codex_app_server".into(),
                    native_session_id: session_id.clone(),
                    native_locator_kind: "thread".into(),
                    provider_version: None,
                    adapter_contract_version: "test".into(),
                    availability: NativeSessionAvailability::Available,
                    supports_resume: true,
                    last_verified_at: None,
                    parent_native_session_id: None,
                }),
                provider_cwd_hint: None,
                provider_environment_observation: None,
                owned_paths: Vec::new(),
                zero_output_streak: 0,
                last_consumed_work_version: None,
                started_at: "unix-ms:1".into(),
                last_event_at: Some("unix-ms:2".into()),
                finished_at: None,
            })
            .expect("seed ProviderRuntimeProjection");
        let body = ProviderInteractionRequestBody {
            interaction_type: ProviderInteractionType::Question,
            prompt: "Select a safe action".into(),
            options: vec![
                ProviderInteractionMessageOption {
                    id: "continue".into(),
                    label: "Continue".into(),
                    intent: Some("approve".into()),
                },
                ProviderInteractionMessageOption {
                    id: "stop".into(),
                    label: "Stop".into(),
                    intent: Some("deny".into()),
                },
            ],
            provider: "codex".into(),
            provider_request_id: format!("provider-request-{run_id}"),
            method: "item/tool/requestUserInput".into(),
            session: session_id,
            member: member_id.clone(),
            generation: 2,
        };
        let request = ProviderDispatchEnvelope {
            id: format!("request-{run_id}"),
            team_run_id: run_id.to_string(),
            work_id: None,
            source_plan_ref: None,
            sender: Some(TeamActorRef {
                kind: TeamActorKind::ProviderRuntimeProjection,
                id: member_id.clone(),
                display_name: None,
                authn_source: Some("provider_reverse_rpc".into()),
            }),
            sender_runtime_id: member_id,
            recipients: vec![TeamRecipientRef {
                kind: TeamRecipientKind::Host,
                id: "host".into(),
            }],
            recipient_runtime_ids: vec!["host".into()],
            kind: ProviderDispatchIntent::ProviderInteractionRequest,
            body: body.to_canonical_json().expect("request body"),
            correlation_id: body.correlation_id(),
            causation_id: None,
            response_intent: Some(ProviderResponseIntent::ResponseRequired),
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "host".into(),
                policy: TeamDeliveryPolicy::ManualAck,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("host-surface-receipt".into()),
                failure_reason: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        store
            .append_team_message_checked(&request)
            .expect("append request");
        (body, request)
    }

    fn provider_interaction_response(
        request_body: &ProviderInteractionRequestBody,
        request: &ProviderDispatchEnvelope,
        choice: &str,
    ) -> ProviderDispatchEnvelope {
        let body = ProviderInteractionResponseBody {
            interaction_type: request_body.interaction_type,
            choice: Some(choice.to_string()),
            text: None,
            session: request_body.session.clone(),
            member: request_body.member.clone(),
            generation: request_body.generation,
        };
        ProviderDispatchEnvelope {
            id: provider_interaction_response_id(&request.id).expect("stable response id"),
            team_run_id: request.team_run_id.clone(),
            work_id: None,
            source_plan_ref: None,
            sender: Some(TeamActorRef {
                kind: TeamActorKind::Host,
                id: "host".into(),
                display_name: None,
                authn_source: Some("test_host".into()),
            }),
            sender_runtime_id: "host".into(),
            recipients: vec![TeamRecipientRef {
                kind: TeamRecipientKind::ProviderRuntimeProjection,
                id: request_body.member.clone(),
            }],
            recipient_runtime_ids: vec![request_body.member.clone()],
            kind: ProviderDispatchIntent::ProviderInteractionResponse,
            body: body.to_canonical_json().expect("response body"),
            correlation_id: request.correlation_id.clone(),
            causation_id: Some(request.id.clone()),
            response_intent: Some(ProviderResponseIntent::Informational),
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: request_body.member.clone(),
                policy: TeamDeliveryPolicy::Inject,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: "unix-ms:3".into(),
            }],
            created_at: "unix-ms:3".into(),
        }
    }

    #[test]
    fn provider_interaction_response_atomically_acks_and_is_strictly_idempotent() {
        let root = team_test_root("provider-interaction-idempotent");
        let store = HarnessStore::new(&root);
        let (request_body, request) =
            seed_provider_interaction_bridge(&store, "run-interaction-idempotent");
        let response = provider_interaction_response(&request_body, &request, "continue");
        let first = store
            .record_provider_interaction_response(&response, "unix-ms:4")
            .expect("record response");
        assert_eq!(first, response);

        let exact_retry = response.clone();
        let retried = store
            .record_provider_interaction_response(&exact_retry, "unix-ms:9")
            .expect("same stable id and semantic reply returns existing");
        assert_eq!(retried.id, response.id);

        let messages = latest_by_id(store.team_messages().expect("messages"), |message| {
            message.id.clone()
        });
        let request_after = messages.get(&request.id).expect("request remains");
        assert_eq!(
            request_after.deliveries[0].status,
            TeamDeliveryStatus::Acknowledged
        );
        let response_after = messages.get(&response.id).expect("response remains");
        assert_eq!(
            response_after.deliveries[0].status,
            TeamDeliveryStatus::Queued,
            "Host answer is not provider delivery truth"
        );
        assert_eq!(
            messages
                .values()
                .filter(
                    |message| message.kind == ProviderDispatchIntent::ProviderInteractionResponse
                )
                .count(),
            1
        );

        store
            .acquire_test_supervisor_lease(
                &request.team_run_id,
                "supervisor-interaction",
                42,
                "test",
                100,
                1_000,
            )
            .expect("lease response delivery");
        let claimed = store
            .claim_team_message_delivery(
                &request.team_run_id,
                &response.id,
                &request_body.member,
                "supervisor-interaction",
                1,
                "claim-interaction-response",
                101,
                1_000,
                "unix-ms:5",
            )
            .expect("claim response");
        assert!(matches!(
            claimed,
            TeamMessageDeliveryClaimResult::Claimed(_)
        ));
        store
            .complete_team_message_delivery_claim(
                &request.team_run_id,
                &response.id,
                &request_body.member,
                "supervisor-interaction",
                1,
                "claim-interaction-response",
                "native-response-receipt",
                102,
                "unix-ms:6",
            )
            .expect("provider accepted response");
        let retry_after_delivery = store
            .record_provider_interaction_response(&response, "unix-ms:7")
            .expect("semantic retry survives mutable delivery projection");
        assert_eq!(
            retry_after_delivery.deliveries[0].status,
            TeamDeliveryStatus::Delivered
        );
        assert_eq!(
            retry_after_delivery.deliveries[0]
                .provider_receipt_id
                .as_deref(),
            Some("native-response-receipt")
        );
        let current_member = latest_by_id(store.member_runs().expect("members"), |member| {
            member.id.clone()
        })
        .remove(&request_body.member)
        .expect("current member");
        let mut closed_member = current_member.clone();
        closed_member.coordination_status = firm_core::MemberCoordinationStatus::Closed;
        closed_member.status = MemberRunStatus::Stopped;
        closed_member.finished_at = Some("unix-ms:8".into());
        store
            .compare_and_append_member_run(&current_member, &closed_member)
            .expect("close member");
        let retry_after_close = store
            .record_provider_interaction_response(&response, "unix-ms:9")
            .expect("exact retry remains valid after member close");
        assert_eq!(retry_after_close.id, response.id);

        let conflict = provider_interaction_response(&request_body, &request, "stop");
        assert!(store
            .record_provider_interaction_response(&conflict, "unix-ms:10")
            .expect_err("different answer conflicts")
            .to_string()
            .contains("RESPONSE_CONFLICT"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn provider_interaction_response_rejects_unknown_choice_and_predelivery() {
        let root = team_test_root("provider-interaction-invalid-response");
        let store = HarnessStore::new(&root);
        let (request_body, request) =
            seed_provider_interaction_bridge(&store, "run-interaction-invalid");
        let mut unstable_id = provider_interaction_response(&request_body, &request, "continue");
        unstable_id.id = "caller-selected-response-id".into();
        assert!(store
            .record_provider_interaction_response(&unstable_id, "unix-ms:4")
            .expect_err("response id is request-derived")
            .to_string()
            .contains("must be stable"));
        let unknown = provider_interaction_response(&request_body, &request, "invented");
        assert!(store
            .record_provider_interaction_response(&unknown, "unix-ms:4")
            .expect_err("unknown choice")
            .to_string()
            .contains("not a request option"));

        let mut predelivered = provider_interaction_response(&request_body, &request, "continue");
        predelivered.deliveries[0].status = TeamDeliveryStatus::Delivered;
        predelivered.deliveries[0].provider_receipt_id = Some("forged".into());
        assert!(store
            .record_provider_interaction_response(&predelivered, "unix-ms:4")
            .expect_err("cannot claim provider receipt early")
            .to_string()
            .contains("Inject+Queued"));
        let mut extra_route = provider_interaction_response(&request_body, &request, "continue");
        extra_route
            .recipient_runtime_ids
            .push("other-member".into());
        extra_route.deliveries.push(ProviderDispatchAttempt {
            member_id: "other-member".into(),
            policy: TeamDeliveryPolicy::Inject,
            status: TeamDeliveryStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: "unix-ms:4".into(),
        });
        assert!(store
            .record_provider_interaction_response(&extra_route, "unix-ms:4")
            .expect_err("response cannot fan out")
            .to_string()
            .contains("route only"));

        let mut unacknowledgeable = request.clone();
        unacknowledgeable.deliveries[0].status = TeamDeliveryStatus::Failed;
        {
            let _lock = store.acquire_write_lock().expect("fault injection lock");
            store
                .append_jsonl_unlocked("team_messages.jsonl", &unacknowledgeable)
                .expect("simulate failed Host delivery through private ledger primitive");
        }
        let valid = provider_interaction_response(&request_body, &request, "continue");
        assert!(store
            .record_provider_interaction_response(&valid, "unix-ms:4")
            .expect_err("ACK failure must preflight before response append")
            .to_string()
            .contains("cannot be acknowledged"));
        assert_eq!(
            store
                .team_messages()
                .expect("messages")
                .iter()
                .filter(
                    |message| message.kind == ProviderDispatchIntent::ProviderInteractionResponse
                )
                .count(),
            0,
            "failed ACK must not leave a partial response row"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn raw_provider_interaction_appends_are_forbidden_but_trusted_seams_work() {
        let root = team_test_root("provider-interaction-raw-append");
        let store = HarnessStore::new(&root);
        let (request_body, request) =
            seed_provider_interaction_bridge(&store, "run-interaction-raw-append");

        let mut raw_request = request.clone();
        raw_request.id = "raw-provider-request".into();
        raw_request.body = r#"{"type":"question","unknown":true}"#.into();
        assert!(store
            .append_team_message(&raw_request)
            .expect_err("raw provider request must be forbidden")
            .to_string()
            .contains("RAW_APPEND_FORBIDDEN"));

        let queued_response = provider_interaction_response(&request_body, &request, "continue");
        assert!(store
            .append_team_message(&queued_response)
            .expect_err("even valid queued response requires atomic record seam")
            .to_string()
            .contains("RAW_APPEND_FORBIDDEN"));

        let mut delivered_response = queued_response.clone();
        delivered_response.deliveries[0].status = TeamDeliveryStatus::Delivered;
        delivered_response.deliveries[0].provider_receipt_id = Some("forged-receipt".into());
        assert!(store
            .append_team_message(&delivered_response)
            .expect_err("raw Delivered response must be forbidden")
            .to_string()
            .contains("RAW_APPEND_FORBIDDEN"));

        let recorded = store
            .record_provider_interaction_response(&queued_response, "unix-ms:4")
            .expect("trusted response seam remains legal");
        assert_eq!(recorded.id, queued_response.id);
        assert_eq!(recorded.deliveries[0].status, TeamDeliveryStatus::Queued);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn concurrent_provider_interaction_answers_have_one_winner() {
        let root = team_test_root("provider-interaction-race");
        let store = Arc::new(HarnessStore::new(&root));
        let (request_body, request) =
            seed_provider_interaction_bridge(&store, "run-interaction-race");
        let barrier = Arc::new(Barrier::new(3));
        let mut joins = Vec::new();
        for choice in ["continue", "stop"] {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let response = provider_interaction_response(&request_body, &request, choice);
            joins.push(std::thread::spawn(move || {
                barrier.wait();
                store.record_provider_interaction_response(&response, "unix-ms:4")
            }));
        }
        barrier.wait();
        let results = joins
            .into_iter()
            .map(|join| join.join().expect("responder"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        assert_eq!(
            store
                .team_messages()
                .expect("messages")
                .iter()
                .filter(
                    |message| message.kind == ProviderDispatchIntent::ProviderInteractionResponse
                )
                .count(),
            1
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn provider_interaction_response_claim_fences_a_closed_generation() {
        let root = team_test_root("provider-interaction-stale-claim");
        let store = HarnessStore::new(&root);
        let (request_body, request) =
            seed_provider_interaction_bridge(&store, "run-interaction-stale-claim");
        let response = provider_interaction_response(&request_body, &request, "continue");
        store
            .record_provider_interaction_response(&response, "unix-ms:4")
            .expect("record queued response");

        let current = latest_by_id(store.member_runs().expect("members"), |member| {
            member.id.clone()
        })
        .remove(&request_body.member)
        .expect("member");
        let mut closed = current.clone();
        closed.coordination_status = firm_core::MemberCoordinationStatus::Closed;
        closed.status = MemberRunStatus::Stopped;
        closed.finished_at = Some("unix-ms:5".into());
        store
            .compare_and_append_member_run(&current, &closed)
            .expect("close member");
        store
            .acquire_test_supervisor_lease(
                &request.team_run_id,
                "supervisor-stale-claim",
                43,
                "test",
                100,
                1_000,
            )
            .expect("lease");
        assert!(store
            .claim_team_message_delivery(
                &request.team_run_id,
                &response.id,
                &request_body.member,
                "supervisor-stale-claim",
                1,
                "stale-claim",
                101,
                1_000,
                "unix-ms:6",
            )
            .expect_err("closed generation cannot receive provider response")
            .to_string()
            .contains("stale"));
        assert_eq!(
            store
                .record_provider_interaction_response(&response, "unix-ms:7")
                .expect("exact command retry still converges")
                .id,
            response.id
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn provider_control_action(run_id: &str, member_id: &str) -> MemberAction {
        MemberAction {
            id: format!("provider-control-{run_id}"),
            seq: 1,
            team_run_id: run_id.to_string(),
            member_run_id: member_id.to_string(),
            task_id: None,
            provider_call_id: Some("permission-request-1".into()),
            action_type: "provider_control".into(),
            status: MemberActionStatus::Succeeded,
            provider_status: Some("acknowledged".into()),
            semantic_status: Some("safe_auto_allow".into()),
            title: "Kimi full-access tool permission acknowledged".into(),
            summary: "bounded safe auto-allow receipt".into(),
            evidence_refs: Vec::new(),
            started_at: "unix-ms:3".into(),
            completed_at: Some("unix-ms:3".into()),
        }
    }

    #[test]
    fn concurrent_current_member_receipts_append_exactly_once() {
        let root = team_test_root("member-action-current-race");
        let store = Arc::new(HarnessStore::new(&root));
        let (_, request) = seed_provider_interaction_bridge(&store, "run-action-race");
        let expected = latest_by_id(store.member_runs().expect("members"), |member| {
            member.id.clone()
        })
        .remove(&request.sender_runtime_id)
        .expect("member");
        let action = provider_control_action(&request.team_run_id, &expected.id);
        let barrier = Arc::new(Barrier::new(3));
        let handles = (0..2)
            .map(|index| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                let expected = expected.clone();
                let mut action = action.clone();
                action.id = format!("{}-{index}", action.id);
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append_member_action_if_member_run_current(&expected, &action)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .expect("receipt thread")
                    .expect("receipt call")
            })
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|appended| **appended).count(), 1);
        assert_eq!(store.member_actions().expect("actions").len(), 1);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn member_close_or_session_cas_wins_before_receipt_with_zero_action() {
        for mutation in ["close", "session"] {
            let root = team_test_root(&format!("member-action-current-{mutation}"));
            let store = HarnessStore::new(&root);
            let (_, request) =
                seed_provider_interaction_bridge(&store, &format!("run-action-{mutation}"));
            let expected = latest_by_id(store.member_runs().expect("members"), |member| {
                member.id.clone()
            })
            .remove(&request.sender_runtime_id)
            .expect("member");
            let action = provider_control_action(&request.team_run_id, &expected.id);
            assert!(store
                .append_member_action(&action)
                .expect_err("raw provider control is forbidden before lifecycle change")
                .to_string()
                .contains("PROVIDER_CONTROL_RAW_APPEND_FORBIDDEN"));
            assert!(store.member_actions().expect("actions").is_empty());
            let mut next = expected.clone();
            if mutation == "close" {
                next.coordination_status = firm_core::MemberCoordinationStatus::Closed;
                next.status = MemberRunStatus::Stopped;
                next.finished_at = Some("unix-ms:4".into());
            } else {
                next.native_session
                    .as_mut()
                    .expect("native session")
                    .native_session_id = "replacement-session".into();
            }
            store
                .compare_and_append_member_run(&expected, &next)
                .expect("lifecycle/session CAS wins first");
            let mut raw_after = action.clone();
            raw_after.id.push_str("-after");
            assert!(store
                .append_member_action(&raw_after)
                .expect_err("raw provider control is forbidden after lifecycle change")
                .to_string()
                .contains("PROVIDER_CONTROL_RAW_APPEND_FORBIDDEN"));
            assert!(store
                .append_member_action_if_member_run_current(&expected, &action)
                .expect_err("stale expected member must fail")
                .to_string()
                .contains("changed concurrently"));
            assert!(store.member_actions().expect("actions").is_empty());
            std::fs::remove_dir_all(root).expect("remove temp store");
        }
    }

    #[test]
    #[ignore = "retired projection-message Handoff contract; canonical completion is WorkReport + GateEvaluation"]
    fn response_required_mail_is_fenced_until_newer_correlation_reaches_provider() {
        let root = team_test_root("handoff-mail-fence");
        let store = HarnessStore::new(&root);
        let correction = ProviderDispatchEnvelope {
            id: "tm-correction".into(),
            team_run_id: "tr-fence".into(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "host".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec!["mr-kimi".into()],
            kind: ProviderDispatchIntent::Message,
            body: "Use the corrected requirement".into(),
            correlation_id: "corr-fence".into(),
            causation_id: Some("tm-assignment".into()),
            // Explicit response intent: this correction must reach the
            // provider before any Handoff, so it fences (ADR 0046 §4).
            response_intent: Some(ProviderResponseIntent::ResponseRequired),
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "mr-kimi".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: "unix-ms:1".into(),
            }],
            created_at: "unix-ms:1".into(),
        };
        store
            .append_team_message_checked(&correction)
            .expect("append correction");
        let handoff = ProviderDispatchEnvelope {
            id: "tm-handoff".into(),
            team_run_id: "tr-fence".into(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "mr-kimi".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec!["host".into()],
            kind: ProviderDispatchIntent::Message,
            body: "done".into(),
            correlation_id: "corr-fence".into(),
            causation_id: Some("tm-assignment".into()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "host".into(),
                policy: TeamDeliveryPolicy::ManualAck,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("harness-control-plane".into()),
                failure_reason: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        let queued_error = store
            .append_team_message_checked(&handoff)
            .expect_err("queued correction must fence stale handoff");
        assert!(queued_error.to_string().contains("queued or claimed"));

        let mut claimed = correction.clone();
        claimed.deliveries[0].status = TeamDeliveryStatus::Claimed;
        claimed.deliveries[0].claim_id = Some("claim-1".into());
        store
            .append_team_message(&claimed)
            .expect("persist claim projection");
        let claimed_error = store
            .append_team_message_checked(&handoff)
            .expect_err("uncertain claimed correction must also fence handoff");
        assert!(claimed_error.to_string().contains("queued or claimed"));

        let mut delivered = claimed;
        delivered.deliveries[0].status = TeamDeliveryStatus::Delivered;
        delivered.deliveries[0].attempt = 1;
        delivered.deliveries[0].provider_receipt_id = Some("kimi-session:turn-2".into());
        store
            .append_team_message(&delivered)
            .expect("persist provider receipt");
        store
            .append_team_message_checked(&handoff)
            .expect("handoff is valid after provider receipt");

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    #[ignore = "retired projection-message Handoff contract; canonical response intent is covered by trust-kernel delivery tests"]
    fn informational_mail_neither_fences_handoff_nor_requires_response() {
        let root = team_test_root("handoff-informational-fence");
        let store = HarnessStore::new(&root);
        // Acknowledgement-only peer mail: kind `message` with no explicit
        // intent is informational by default (ADR 0046 §4).
        let ack_only = ProviderDispatchEnvelope {
            id: "tm-ack".into(),
            team_run_id: "tr-info".into(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "mr-peer".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec!["mr-kimi".into()],
            kind: ProviderDispatchIntent::Message,
            body: "ACK: noted, no reply needed".into(),
            correlation_id: "corr-info".into(),
            causation_id: Some("tm-assignment".into()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "mr-kimi".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: "unix-ms:1".into(),
            }],
            created_at: "unix-ms:1".into(),
        };
        assert!(!ack_only.requires_response());
        store
            .append_team_message_checked(&ack_only)
            .expect("append informational mail");
        let handoff = ProviderDispatchEnvelope {
            id: "tm-handoff".into(),
            team_run_id: "tr-info".into(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "mr-kimi".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec!["host".into()],
            kind: ProviderDispatchIntent::Message,
            body: "done".into(),
            correlation_id: "corr-info".into(),
            causation_id: Some("tm-assignment".into()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "host".into(),
                policy: TeamDeliveryPolicy::ManualAck,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("harness-control-plane".into()),
                failure_reason: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        // Informational mail never starts a provider round on its own, so it
        // must not fence a Handoff either — otherwise the Handoff would
        // deadlock behind mail that is intentionally never driven.
        store
            .append_team_message_checked(&handoff)
            .expect("informational mail must not fence handoff");

        // The same pending delivery with explicit response intent fences.
        let question = ProviderDispatchEnvelope {
            id: "tm-question".into(),
            correlation_id: "corr-info-q".into(),
            causation_id: None,
            response_intent: Some(ProviderResponseIntent::ResponseRequired),
            created_at: "unix-ms:3".into(),
            ..ack_only.clone()
        };
        assert!(question.requires_response());
        store
            .append_team_message_checked(&question)
            .expect("append response-required question");
        let fenced = ProviderDispatchEnvelope {
            id: "tm-handoff-q".into(),
            correlation_id: "corr-info-q".into(),
            causation_id: Some("tm-assignment-q".into()),
            created_at: "unix-ms:4".into(),
            ..handoff.clone()
        };
        let error = store
            .append_team_message_checked(&fenced)
            .expect_err("response-required question must fence stale handoff");
        assert!(error.to_string().contains("queued or claimed"));

        // Safety regression guard: a Host mid-round correction is ordinary
        // `message` mail with no explicit intent, but it is sender-aware
        // response-required, so it MUST still fence a same-correlation Handoff
        // — otherwise a member could hand off work that never absorbed the
        // correction.
        let host_correction = ProviderDispatchEnvelope {
            id: "tm-host-correction".into(),
            sender_runtime_id: "host".into(),
            correlation_id: "corr-info-host".into(),
            causation_id: None,
            response_intent: None,
            body: "Revise: drop the extra scope before handing off".into(),
            created_at: "unix-ms:5".into(),
            ..ack_only.clone()
        };
        assert!(
            host_correction.requires_response(),
            "Host ordinary mail defaults to response_required"
        );
        store
            .append_team_message_checked(&host_correction)
            .expect("append host correction");
        let stale = ProviderDispatchEnvelope {
            id: "tm-handoff-host".into(),
            correlation_id: "corr-info-host".into(),
            causation_id: Some("tm-assignment-host".into()),
            created_at: "unix-ms:6".into(),
            ..handoff.clone()
        };
        let error = store
            .append_team_message_checked(&stale)
            .expect_err("pending Host correction must fence stale handoff");
        assert!(error.to_string().contains("queued or claimed"));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    #[ignore = "retired projection-message Handoff contract; canonical result submission is idempotently fenced by WorkReport"]
    fn concurrent_same_turn_handoffs_allow_exactly_one_append() {
        let root = team_test_root("same-turn-handoff");
        let store = Arc::new(HarnessStore::new(&root));
        let assignment = ProviderDispatchEnvelope {
            id: "tm-assignment".into(),
            team_run_id: "tr-converge".into(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "host".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec!["mr-codex".into()],
            kind: ProviderDispatchIntent::Message,
            body: "Review the convergence fix".into(),
            correlation_id: "corr-converge".into(),
            causation_id: None,
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "mr-codex".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("codex-turn-1".into()),
                failure_reason: None,
                updated_at: "unix-ms:1".into(),
            }],
            created_at: "unix-ms:1".into(),
        };
        store
            .append_team_message_checked(&assignment)
            .expect("append conversation anchor");
        let handoff = ProviderDispatchEnvelope {
            id: "tm-handoff-a".into(),
            team_run_id: assignment.team_run_id.clone(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "mr-codex".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec!["host".into()],
            kind: ProviderDispatchIntent::Message,
            body: "## RESULT\ndone".into(),
            correlation_id: assignment.correlation_id.clone(),
            causation_id: Some(assignment.id.clone()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "host".into(),
                policy: TeamDeliveryPolicy::ManualAck,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("harness-control-plane".into()),
                failure_reason: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        let barrier = Arc::new(Barrier::new(2));
        let handles = ["tm-handoff-a", "tm-handoff-b"]
            .into_iter()
            .map(|id| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                let mut candidate = handoff.clone();
                candidate.id = id.into();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append_team_message_checked(&candidate)
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("handoff writer"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let conflict = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one same-turn conflict");
        assert!(conflict.to_string().contains("already handed off"));
        assert_eq!(
            store
                .team_messages()
                .expect("messages")
                .into_iter()
                .filter(|message| message.kind == ProviderDispatchIntent::Message)
                .count(),
            1
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn durable_supervisor_lease_and_message_claim_are_cross_process_safe() {
        let root = team_test_root("supervisor-claim");
        let store = Arc::new(HarnessStore::new(&root));
        let run = AgentTeamRun {
            id: "tr-claim".into(),
            agent_team_id: "team-claim".into(),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: Some("thread-claim".into()),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "claim exactly once".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-claim".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("append run");

        let first = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-a", 101, "test:a", 100, 1_000)
            .expect("first Supervisor");
        assert_eq!(first.generation, 1);
        let conflict = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-b", 202, "test:b", 101, 1_000)
            .expect_err("second active Supervisor must be rejected");
        assert!(conflict.to_string().contains("supervisor-a"));
        let second = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-b", 202, "test:b", 1_101, 1_000)
            .expect("expired lease may be replaced");
        assert_eq!(second.generation, 2);

        let message = ProviderDispatchEnvelope {
            id: "tm-claim".into(),
            team_run_id: run.id.clone(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "host".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec!["mr-claim".into()],
            kind: ProviderDispatchIntent::Message,
            body: "only once".into(),
            correlation_id: "corr-claim".into(),
            causation_id: None,
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "mr-claim".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        store
            .append_team_message_checked(&message)
            .expect("append queued message");
        let early_ack = store
            .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:2")
            .expect_err("queued delivery cannot be acknowledged");
        assert!(early_ack.to_string().contains("has not been delivered"));

        let barrier = Arc::new(Barrier::new(2));
        let handles = ["claim-a", "claim-b"].map(|claim_id| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let run_id = run.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .claim_team_message_delivery(
                        &run_id,
                        "tm-claim",
                        "mr-claim",
                        "supervisor-b",
                        2,
                        claim_id,
                        1_102,
                        1_000,
                        "unix-ms:3",
                    )
                    .expect("claim call")
            })
        });
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("claim thread"))
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, TeamMessageDeliveryClaimResult::Claimed(_)))
                .count(),
            1
        );
        let claimed = results
            .into_iter()
            .find_map(|result| match result {
                TeamMessageDeliveryClaimResult::Claimed(message) => Some(*message),
                TeamMessageDeliveryClaimResult::NotQueued => None,
            })
            .expect("one claim");
        let claim_id = claimed.deliveries[0].claim_id.clone().expect("claim id");
        let stale_completion = store
            .complete_team_message_delivery_claim(
                &run.id,
                &message.id,
                "mr-claim",
                "supervisor-a",
                1,
                &claim_id,
                "native-turn-stale",
                1_103,
                "unix-ms:4",
            )
            .expect_err("a stale Supervisor generation cannot complete another lease's claim");
        assert!(stale_completion
            .to_string()
            .contains("Supervisor lease is not owned"));
        let delivered = store
            .complete_team_message_delivery_claim(
                &run.id,
                &message.id,
                "mr-claim",
                "supervisor-b",
                2,
                &claim_id,
                "native-turn-1",
                1_103,
                "unix-ms:4",
            )
            .expect("complete claim");
        assert_eq!(
            delivered.deliveries[0].status,
            TeamDeliveryStatus::Delivered
        );
        assert_eq!(
            delivered.deliveries[0].provider_receipt_id.as_deref(),
            Some("native-turn-1")
        );
        store
            .complete_team_message_delivery_claim(
                &run.id,
                &message.id,
                "mr-claim",
                "supervisor-b",
                2,
                &claim_id,
                "native-turn-1",
                1_104,
                "unix-ms:4",
            )
            .expect("exact completion receipt is idempotent");
        let different_receipt = store
            .complete_team_message_delivery_claim(
                &run.id,
                &message.id,
                "mr-claim",
                "supervisor-b",
                2,
                &claim_id,
                "native-turn-different",
                1_104,
                "unix-ms:4",
            )
            .expect_err("completed claim cannot change provider receipt");
        assert!(different_receipt
            .to_string()
            .contains("different provider receipt"));
        let acknowledged = store
            .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:5")
            .expect("acknowledge delivered message");
        assert_eq!(
            acknowledged.deliveries[0].status,
            TeamDeliveryStatus::Acknowledged
        );
        let acknowledged_again = store
            .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:6")
            .expect("ACK is idempotent");
        assert_eq!(
            acknowledged_again.deliveries[0].status,
            TeamDeliveryStatus::Acknowledged
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    /// When a member fails before binding (pre-bind), queued ProviderDispatchEnvelope deliveries
    /// transition to Failed so they do not stay permanently actionable in the inbox.
    #[test]
    fn fail_queued_delivery_clears_pre_bind_mail_and_is_idempotent() {
        let root = team_test_root("pre-bind-mail-fail");
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: "tr-fail-mail".into(),
            agent_team_id: "team-fail-mail".into(),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: None,
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "fail orphaned mail".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-orphan".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("append run");

        let lease = store
            .acquire_test_supervisor_lease(
                &run.id,
                "supervisor-pre-bind",
                300,
                "test:pre-bind",
                100,
                5_000,
            )
            .expect("acquire Supervisor lease");

        let message = ProviderDispatchEnvelope {
            id: "tm-orphan".into(),
            team_run_id: run.id.clone(),
            work_id: None,
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "host".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec!["mr-orphan".into()],
            kind: ProviderDispatchIntent::Message,
            body: "orphaned work assignment".into(),
            correlation_id: "corr-orphan".into(),
            causation_id: None,
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: "mr-orphan".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        store
            .append_team_message_checked(&message)
            .expect("append queued message");

        // Pre-bind failure: member never bound, delivery is still Queued.
        let msgs = store.team_messages().expect("read messages");
        let queued = msgs
            .iter()
            .find(|m| m.id == "tm-orphan")
            .expect("tm-orphan present");
        assert_eq!(
            queued.deliveries[0].status,
            TeamDeliveryStatus::Queued,
            "starts queued"
        );

        // Fail the delivery.
        let failed = store
            .fail_team_message_delivery(
                &run.id,
                &message.id,
                "mr-orphan",
                &lease.supervisor_id,
                lease.generation,
                "pre-bind member terminated",
                200,
                "unix-ms:3",
            )
            .expect("fail queued delivery");

        assert_eq!(failed.deliveries[0].status, TeamDeliveryStatus::Failed);
        assert_eq!(
            failed.deliveries[0].failure_reason.as_deref(),
            Some("pre-bind member terminated")
        );
        assert!(failed.deliveries[0].claim_id.is_none());
        assert!(failed.deliveries[0].provider_receipt_id.is_none());

        // Idempotent: same reason succeeds.
        let again = store
            .fail_team_message_delivery(
                &run.id,
                &message.id,
                "mr-orphan",
                &lease.supervisor_id,
                lease.generation,
                "pre-bind member terminated",
                201,
                "unix-ms:4",
            )
            .expect("idempotent fail with same reason");

        assert_eq!(again.deliveries[0].status, TeamDeliveryStatus::Failed);

        // Different reason is rejected.
        let conflict = store
            .fail_team_message_delivery(
                &run.id,
                &message.id,
                "mr-orphan",
                &lease.supervisor_id,
                lease.generation,
                "different reason",
                202,
                "unix-ms:5",
            )
            .expect_err("different failure reason must be rejected");
        assert!(conflict.to_string().contains("different reason"));

        // RegistryMessage survives store reopen.
        drop(store);
        let reopened = HarnessStore::new(&root);
        let msgs_after = reopened.team_messages().expect("read after reopen");
        let reloaded = latest_by_id(msgs_after, |m| m.id.clone())
            .remove("tm-orphan")
            .expect("tm-orphan survived reopen");
        assert_eq!(reloaded.deliveries[0].status, TeamDeliveryStatus::Failed);
        assert_eq!(
            reloaded.deliveries[0].failure_reason.as_deref(),
            Some("pre-bind member terminated")
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn member_close_request_survives_store_reopen_and_is_idempotent() {
        let root = team_test_root("durable-member-close");
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: "tr-close".into(),
            agent_team_id: "team-close".into(),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: Some("thread-close".into()),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "close once".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-close".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        let member = ProviderRuntimeProjection {
            id: "mr-close".into(),
            team_run_id: run.id.clone(),
            slot_id: None,
            agent_member_id: "agent-mr-close".into(),
            name: "Builder".into(),
            role: "builder".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Running,
            native_session: None,
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            started_at: "unix-ms:1".into(),
            last_event_at: None,
            finished_at: None,
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };
        store.append_team_run(&run).expect("append run");
        store.append_member_run(&member).expect("append member");

        let request = TeamMemberCloseRequest {
            id: "close-1".into(),
            team_run_id: run.id.clone(),
            member_run_id: member.id.clone(),
            requested_by: "host".into(),
            reason: "accepted".into(),
            status: TeamMemberCloseStatus::Pending,
            requested_at: "unix-ms:2".into(),
            applied_at: None,
        };
        let latched = store
            .latch_team_member_close(&request)
            .expect("latch Close");
        let repeated = store
            .latch_team_member_close(&TeamMemberCloseRequest {
                id: "close-duplicate".into(),
                ..request.clone()
            })
            .expect("repeat Close");
        assert_eq!(latched.id, repeated.id);

        let reopened = HarnessStore::new(&root);
        let pending = reopened
            .latest_team_member_close_request(&member.id)
            .expect("read Close after reopen")
            .expect("durable Close");
        assert_eq!(pending.status, TeamMemberCloseStatus::Pending);
        let applied = reopened
            .complete_team_member_close(&run.id, &member.id, &pending.id, "unix-ms:3")
            .expect("apply Close");
        assert_eq!(applied.status, TeamMemberCloseStatus::Applied);
        assert_eq!(applied.applied_at.as_deref(), Some("unix-ms:3"));
        let applied_again = reopened
            .complete_team_member_close(&run.id, &member.id, &pending.id, "unix-ms:4")
            .expect("Close apply is idempotent");
        assert_eq!(applied_again.applied_at.as_deref(), Some("unix-ms:3"));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_member_action_jsonl() {
        let root = team_test_root("member-action");
        let store = HarnessStore::new(&root);
        let action = MemberAction {
            id: "ma-1".into(),
            seq: 7,
            team_run_id: "tr-1".into(),
            member_run_id: "mr-1".into(),
            task_id: Some("task-1".into()),
            provider_call_id: Some("tool-1".into()),
            action_type: "tool_completed".into(),
            status: MemberActionStatus::Succeeded,
            provider_status: Some("completed".into()),
            semantic_status: Some("succeeded".into()),
            title: "cargo test".into(),
            summary: "all green".into(),
            evidence_refs: vec!["ev-1".into()],
            started_at: "unix-ms:1".into(),
            completed_at: Some("unix-ms:2".into()),
        };

        store
            .append_member_action(&action)
            .expect("append member action");
        append_sparse_row(
            &root,
            "member_actions.jsonl",
            r#"{"id":"ma-sparse","seq":8,"team_run_id":"tr-1","member_run_id":"mr-1","action_type":"blocked","status":"started","title":"t","summary":"s","started_at":"unix-ms:3"}"#,
        );

        let actions = store.member_actions().expect("read member actions");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], action);
        let sparse = &actions[1];
        assert_eq!(sparse.id, "ma-sparse");
        assert_eq!(sparse.seq, 8);
        assert!(sparse.task_id.is_none());
        assert!(sparse.evidence_refs.is_empty());
        assert!(sparse.completed_at.is_none());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_delegation_run_jsonl() {
        let root = team_test_root("delegation-run");
        let store = HarnessStore::new(&root);
        let delegation = DelegationRun {
            id: "dr-1".into(),
            team_run_id: "tr-1".into(),
            parent_member_run_id: "mr-1".into(),
            parent_task_id: Some("task-1".into()),
            mode: DelegationMode::HarnessWorker,
            provider: "claude".into(),
            provider_child_thread_id: None,
            workflow_run_id: Some("wfr-1".into()),
            objective: "Research X".into(),
            status: DelegationStatus::Running,
            evidence_ids: vec!["ev-1".into()],
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:2".into(),
        };

        store
            .append_delegation_run(&delegation)
            .expect("append delegation run");
        append_sparse_row(
            &root,
            "delegation_runs.jsonl",
            r#"{"id":"dr-sparse","team_run_id":"tr-1","parent_member_run_id":"mr-1","mode":"provider_native","provider":"codex","objective":"obj","status":"planned","created_at":"unix-ms:3","updated_at":"unix-ms:3"}"#,
        );

        let delegations = store.delegation_runs().expect("read delegation runs");
        assert_eq!(delegations.len(), 2);
        assert_eq!(delegations[0], delegation);
        let sparse = &delegations[1];
        assert_eq!(sparse.id, "dr-sparse");
        assert_eq!(sparse.mode, DelegationMode::ProviderNative);
        assert_eq!(sparse.status, DelegationStatus::Planned);
        assert!(sparse.parent_task_id.is_none());
        assert!(sparse.provider_child_thread_id.is_none());
        assert!(sparse.workflow_run_id.is_none());
        assert!(sparse.evidence_ids.is_empty());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_team_run_event_jsonl() {
        let root = team_test_root("team-run-event");
        let store = HarnessStore::new(&root);
        let event = TeamRunEvent {
            id: "tre-1".into(),
            seq: 3,
            team_run_id: "tr-1".into(),
            source_kind: TeamRunEventSourceKind::Member,
            member_run_id: Some("mr-1".into()),
            delegation_run_id: None,
            entity_type: "action".into(),
            entity_id: "ma-1".into(),
            operation: "completed".into(),
            summary: "tool completed".into(),
            occurred_at: "unix-ms:1".into(),
        };

        store
            .append_team_run_event(&event)
            .expect("append team run event");
        append_sparse_row(
            &root,
            "team_run_events.jsonl",
            r#"{"id":"tre-sparse","seq":4,"team_run_id":"tr-1","source_kind":"host","entity_type":"team_run","entity_id":"tr-1","operation":"created","summary":"run started","occurred_at":"unix-ms:3"}"#,
        );

        let events = store.team_run_events().expect("read team run events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], event);
        let sparse = &events[1];
        assert_eq!(sparse.id, "tre-sparse");
        assert_eq!(sparse.source_kind, TeamRunEventSourceKind::Host);
        assert!(sparse.member_run_id.is_none());
        assert!(sparse.delegation_run_id.is_none());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn ensure_team_run_event_is_idempotent_and_rejects_semantic_mismatch() {
        let root = team_test_root("ensure-team-run-event");
        let store = HarnessStore::new(&root);
        let event = TeamRunEvent {
            id: "caller-id-is-ignored".into(),
            seq: 0,
            team_run_id: "tr-1".into(),
            source_kind: TeamRunEventSourceKind::Host,
            member_run_id: None,
            delegation_run_id: None,
            entity_type: "host_attention".into(),
            entity_id: "attention-1".into(),
            operation: "dispatch_ready".into(),
            summary: "attention-1 actionable attempt 0".into(),
            occurred_at: "unix-ms:1".into(),
        };
        let first = store
            .ensure_team_run_event_next("tr-1:attention-1:actionable:0", event.clone())
            .expect("first event");
        let mut retry = event.clone();
        retry.occurred_at = "unix-ms:2".into();
        let second = store
            .ensure_team_run_event_next("tr-1:attention-1:actionable:0", retry)
            .expect("same causal transition");
        assert_eq!(first, second);
        assert_eq!(store.team_run_events().unwrap().len(), 1);

        let mut mismatch = event;
        mismatch.summary = "different causal meaning".into();
        assert!(matches!(
            store.ensure_team_run_event_next("tr-1:attention-1:actionable:0", mismatch),
            Err(StoreError::Conflict(_))
        ));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn work_test_fixture(
        name: &str,
    ) -> (
        PathBuf,
        HarnessStore,
        AgentTeamRun,
        ProviderRuntimeProjection,
        ProviderRuntimeProjection,
    ) {
        let root = team_test_root(name);
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: format!("tr-{name}"),
            agent_team_id: format!("team-{name}"),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: Some(format!("host-{name}")),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "prove Works".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec![format!("mr-{name}-a"), format!("mr-{name}-b")],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        let member = |suffix: &str| ProviderRuntimeProjection {
            id: format!("mr-{name}-{suffix}"),
            team_run_id: run.id.clone(),
            slot_id: Some(format!("slot-{suffix}")),
            agent_member_id: format!("agent-{suffix}"),
            name: format!("Member {suffix}"),
            role: "builder".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Idle,
            native_session: None,
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            started_at: "unix-ms:1".into(),
            last_event_at: None,
            finished_at: None,
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };
        let member_a = member("a");
        let member_b = member("b");
        store.append_team_run(&run).expect("append team run");
        store.append_member_run(&member_a).expect("append member a");
        store.append_member_run(&member_b).expect("append member b");
        (root, store, run, member_a, member_b)
    }

    fn host_work_context(id: &str, key: &str, at: &str) -> WorkCommandContext {
        WorkCommandContext {
            event_id: id.into(),
            performed_by_actor: firm_core::TeamActorRef {
                kind: firm_core::TeamActorKind::Host,
                id: "host".into(),
                display_name: Some("Host".into()),
                authn_source: Some("test".into()),
            },
            authority_actor: None,
            causation_ref: None,
            idempotency_key: key.into(),
            created_at: at.into(),
            duplicate_ok: false,
        }
    }

    fn member_work_context(
        member_run_id: &str,
        id: &str,
        key: &str,
        at: &str,
    ) -> WorkCommandContext {
        WorkCommandContext {
            event_id: id.into(),
            performed_by_actor: firm_core::TeamActorRef {
                kind: firm_core::TeamActorKind::ProviderRuntimeProjection,
                id: member_run_id.into(),
                display_name: None,
                authn_source: Some("bound-runtime:test".into()),
            },
            authority_actor: None,
            causation_ref: None,
            idempotency_key: key.into(),
            created_at: at.into(),
            duplicate_ok: false,
        }
    }

    fn admit_replacement_for_test(store: &HarnessStore, member: &ProviderRuntimeProjection) {
        let current = store
            .team_runs()
            .expect("TeamRun history")
            .into_iter()
            .rev()
            .find(|run| run.id == member.team_run_id)
            .expect("replacement TeamRun");
        let mut next = current.clone();
        next.member_run_ids.push(member.id.clone());
        next.updated_at = member.started_at.clone();
        store
            .admit_member_run(&current, &next, member)
            .expect("atomically admit replacement runtime");
    }

    fn unassigned_test_work(run_id: &str, id: &str) -> Work {
        Work {
            id: id.into(),
            team_run_id: run_id.into(),
            team_id: None,
            created_by_member_id: None,
            parent_work_id: None,
            title: format!("Implement Work core — {id}"),
            context_markdown: "Build the smallest correct slice.".into(),
            completion_criteria_markdown: "Tests pass and state is reconstructable.".into(),
            phase: WorkPhase::Open,
            condition: WorkCondition::Normal,
            resolution: None,
            owner_member_id: None,
            active_member_run_id: None,
            claim_mode: WorkClaimMode::TeamClaim,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: firm_core::WorkPriority::High,
            created_by_actor: host_work_context("ignored", "ignored", "unix-ms:1")
                .performed_by_actor,
            result_summary: None,
            blocker_reason: None,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            version: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn delegation_test_fixture(
        name: &str,
    ) -> (
        PathBuf,
        HarnessStore,
        AgentTeamRun,
        ProviderRuntimeProjection,
        AgentTeamRun,
        ProviderRuntimeProjection,
    ) {
        let root = team_test_root(name);
        let store = HarnessStore::new(&root);
        store.init().expect("initialize delegation store");
        let node_id = "00000000-0000-4000-8000-000000000001";
        store
            .insert_execution_node(&ExecutionNode {
                id: node_id.into(),
                display_name: "delegation-test-node".into(),
                status: ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            })
            .expect("insert Node");
        store
            .register_node_project(
                &NodeProjectRegistration {
                    node_id: node_id.into(),
                    execution_space_id: "delegation-test-space".into(),
                    project_binding_id: "project-test".into(),
                    status: NodeProjectRegistrationStatus::Active,
                    created_at: "unix-ms:1".into(),
                    updated_at: "unix-ms:1".into(),
                },
                "delegation-test-space",
            )
            .expect("register project");

        let make_member = |team_run_id: &str, suffix: &str| ProviderRuntimeProjection {
            id: format!("member-{name}-{suffix}"),
            team_run_id: team_run_id.to_string(),
            slot_id: Some(format!("slot-{suffix}")),
            agent_member_id: format!("agent-{suffix}"),
            name: format!("Member {suffix}"),
            role: "builder".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Idle,
            native_session: None,
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            started_at: "unix-ms:1".into(),
            last_event_at: None,
            finished_at: None,
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };
        let mut rows = Vec::new();
        for suffix in ["a", "b"] {
            let mission_id = format!("mission-{name}-{suffix}");
            let team_id = format!("team-{name}-{suffix}");
            let run_id = format!("run-{name}-{suffix}");
            let member = make_member(&run_id, suffix);
            store
                .insert_mission(&Mission {
                    id: mission_id.clone(),
                    title: format!("Mission {suffix}"),
                    objective: format!("Prove Team {suffix} delegation"),
                    context: String::new(),
                    desired_outcome: None,
                    status: MissionStatus::Running,
                    wave_ids: Vec::new(),
                    outcome_summary: None,
                    completed_by: None,
                    created_at: "unix-ms:1".into(),
                    updated_at: "unix-ms:1".into(),
                    completed_at: None,
                })
                .expect("insert Mission");
            store
                .insert_agent_team_with_unique_mission(&AgentTeam {
                    id: team_id.clone(),
                    name: format!("Team {suffix}"),
                    description: "Flat delegation test Team".into(),
                    mission_id,
                    host_agent_id: format!("host-{suffix}"),
                    node_id: node_id.into(),
                    status: firm_core::AgentTeamStatus::Active,
                    member_ids: vec![format!("agent-{suffix}")],
                    created_at: "unix-ms:1".into(),
                    updated_at: "unix-ms:1".into(),
                })
                .expect("insert Team");
            let run = AgentTeamRun {
                id: run_id,
                agent_team_id: team_id,
                execution_node_id: node_id.into(),
                project_binding_id: "project-test".into(),
                previous_run_id: None,
                host_surface: "test".into(),
                host_thread_id: None,
                host_actor: None,
                host_control_mode: Default::default(),
                objective: format!("Run Team {suffix}"),
                execution_root: None,
                status: TeamRunStatus::Running,
                member_run_ids: vec![member.id.clone()],
                budget_limit_usd: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            };
            store
                .create_team_run_from_agent_team(&run, "delegation-test-space")
                .expect("create TeamRun");
            store
                .append_member_run(&member)
                .expect("append ProviderRuntimeProjection");
            rows.push((run, member));
        }
        let (run_a, member_a) = rows.remove(0);
        let (run_b, member_b) = rows.remove(0);
        (root, store, run_a, member_a, run_b, member_b)
    }

    fn assigned_delegation_work(
        run: &AgentTeamRun,
        member: &ProviderRuntimeProjection,
        id: &str,
    ) -> Work {
        let mut work = unassigned_test_work(&run.id, id);
        work.claim_mode = WorkClaimMode::HostAssign;
        work.owner_member_id = Some(member.agent_member_id.clone());
        work.active_member_run_id = Some(member.id.clone());
        work
    }

    fn delegation_request(id: &str, source: &Work, target_team_id: &str) -> WorkDelegation {
        WorkDelegation {
            id: id.to_string(),
            source_work_ref: WorkRef {
                team_run_id: source.team_run_id.clone(),
                work_id: source.id.clone(),
            },
            source_work_version: source.version,
            source_owner_member_id: source
                .owner_member_id
                .clone()
                .expect("delegation source owner"),
            created_by_member_run_id: None,
            target_agent_team_id: target_team_id.to_string(),
            target_work_ref: WorkRef {
                team_run_id: String::new(),
                work_id: String::new(),
            },
            delegated_by_actor: host_work_context("unused", "unused", "unix-ms:1")
                .performed_by_actor,
            state: WorkDelegationState::Active,
            resolution_summary: None,
            blocker_reason: None,
            version: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn work_delegation_is_atomic_idempotent_and_prevents_flat_team_cycles() {
        let (root, store, run_a, member_a, run_b, member_b) =
            delegation_test_fixture("delegation-atomic-cycle");
        let source = store
            .insert_work(
                assigned_delegation_work(&run_a, &member_a, "source-a"),
                host_work_context("work-source-a", "create-source-a", "unix-ms:2"),
            )
            .expect("create source Work");
        let request = delegation_request("delegation-a-b", &source, &run_b.agent_team_id);
        let target_request = assigned_delegation_work(&run_b, &member_b, "target-b");
        let create_context = host_work_context(
            "delegation-create-a-b",
            "delegate-source-a-to-b",
            "unix-ms:3",
        );
        let (created, target) = store
            .create_work_delegation_with_target_work(
                request.clone(),
                target_request.clone(),
                create_context.clone(),
            )
            .expect("create Delegation and target Work atomically");
        assert_eq!(created.version, 1);
        assert_eq!(created.target_work_ref.work_id, target.id);
        assert_eq!(
            target.team_id.as_deref(),
            Some(run_b.agent_team_id.as_str())
        );
        assert_eq!(
            store
                .read_jsonl::<WorkDelegationOperation>("work_delegation_operations.jsonl")
                .expect("atomic operations")
                .len(),
            1
        );

        let retry = store
            .create_work_delegation_with_target_work(
                request.clone(),
                target_request.clone(),
                create_context.clone(),
            )
            .expect("same command retry is idempotent");
        assert_eq!(retry, (created.clone(), target.clone()));
        assert_eq!(store.latest_work_delegations().unwrap().len(), 1);

        let mut changed_target_intent = target_request.clone();
        changed_target_intent.title = "different delegated outcome".into();
        let fingerprint_conflict = store
            .create_work_delegation_with_target_work(
                request.clone(),
                changed_target_intent,
                create_context,
            )
            .expect_err("idempotency key cannot hide changed target Work intent");
        assert!(fingerprint_conflict
            .to_string()
            .contains("IDEMPOTENCY_CONFLICT"));

        let mut changed_entity_ids = request.clone();
        changed_entity_ids.id = "different-delegation-id".into();
        changed_entity_ids.target_work_ref.work_id = "different-target-work-id".into();
        let mut changed_target_id = target_request;
        changed_target_id.id = "different-target-work-id".into();
        let identity_conflict = store
            .create_work_delegation_with_target_work(
                changed_entity_ids,
                changed_target_id,
                host_work_context(
                    "delegation-created-retry-envelope",
                    "delegate-source-a-to-b",
                    "unix-ms:4",
                ),
            )
            .expect_err("idempotency key cannot hide changed explicit entity ids");
        assert!(identity_conflict
            .to_string()
            .contains("IDEMPOTENCY_CONFLICT"));

        let mut conflicting = request.clone();
        conflicting.source_work_ref.work_id = "different-source".into();
        let conflict = store
            .create_work_delegation_with_target_work(
                conflicting,
                assigned_delegation_work(&run_b, &member_b, "unused-target"),
                host_work_context("ignored", "delegate-source-a-to-b", "unix-ms:4"),
            )
            .expect_err("one idempotency key cannot change intent");
        assert!(conflict.to_string().contains("IDEMPOTENCY_CONFLICT"));

        let reverse = delegation_request("delegation-b-a", &target, &run_a.agent_team_id);
        let reverse_target = assigned_delegation_work(&run_a, &member_a, "target-a-reverse");
        let cycle = store
            .create_work_delegation_with_target_work(
                reverse,
                reverse_target,
                host_work_context(
                    "delegation-create-b-a",
                    "delegate-target-b-to-a",
                    "unix-ms:5",
                ),
            )
            .expect_err("A -> B -> A Team cycle must be rejected");
        assert!(cycle.to_string().contains("DELEGATION_CYCLE"));
        assert!(!store
            .latest_works()
            .unwrap()
            .iter()
            .any(|work| work.id == "target-a-reverse"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    #[ignore = "legacy Work acceptance route is retired; exact replacement: member_execution_trust::canonical_acceptance_rolls_up_delegation_in_the_same_operation"]
    fn work_delegation_rolls_up_target_condition_and_resolution_without_mutating_source() {
        let (root, store, run_a, member_a, run_b, member_b) =
            delegation_test_fixture("delegation-rollup");
        let source = store
            .insert_work(
                assigned_delegation_work(&run_a, &member_a, "source-rollup"),
                host_work_context("work-source-rollup", "create-source-rollup", "unix-ms:2"),
            )
            .expect("create source Work");
        let (delegation, target) = store
            .create_work_delegation_with_target_work(
                delegation_request("delegation-rollup", &source, &run_b.agent_team_id),
                assigned_delegation_work(&run_b, &member_b, "target-rollup"),
                host_work_context(
                    "delegation-create-rollup",
                    "delegate-source-rollup",
                    "unix-ms:3",
                ),
            )
            .expect("create Delegation");
        let started = store
            .start_work(
                &target.id,
                target.version,
                &member_b.id,
                member_work_context(
                    &member_b.id,
                    "target-start",
                    "target-start-command",
                    "unix-ms:4",
                ),
            )
            .expect("start target");
        let blocked = store
            .block_work(
                &target.id,
                started.version,
                &member_b.id,
                "waiting for an external contract",
                member_work_context(
                    &member_b.id,
                    "target-block",
                    "target-block-command",
                    "unix-ms:5",
                ),
            )
            .expect("block target");
        let blocked_rollup = store
            .latest_work_delegations()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == delegation.id)
            .expect("atomic blocked rollup");
        assert_eq!(blocked_rollup.state, WorkDelegationState::Blocked);
        assert_eq!(
            blocked_rollup.blocker_reason.as_deref(),
            Some("waiting for an external contract")
        );
        assert!(store
            .transition_work_and_roll_up_delegation(
                &target.id,
                host_work_context("rollup-blocked", "rollup-blocked-command", "unix-ms:6"),
            )
            .expect("already-atomic blocker reconciliation is a no-op")
            .is_empty());

        let resumed = store
            .resume_work(
                &target.id,
                blocked.version,
                &member_b.id,
                "contract arrived",
                member_work_context(
                    &member_b.id,
                    "target-resume",
                    "target-resume-command",
                    "unix-ms:7",
                ),
            )
            .expect("resume target");
        let resumed_rollup = store
            .latest_work_delegations()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == delegation.id)
            .expect("atomic resumed rollup");
        assert_eq!(resumed_rollup.state, WorkDelegationState::Active);

        let submitted = store
            .submit_work(
                &target.id,
                resumed.version,
                &member_b.id,
                "target result ready",
                vec!["artifact://target".into()],
                vec!["check://target".into()],
                member_work_context(
                    &member_b.id,
                    "target-submit",
                    "target-submit-command",
                    "unix-ms:9",
                ),
            )
            .expect("submit target");
        let accepted = store
            .accept_work(
                &target.id,
                submitted.version,
                host_work_context("target-accept", "target-accept-command", "unix-ms:10"),
            )
            .expect("accept target");
        let completed = store
            .latest_work_delegations()
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == delegation.id)
            .expect("atomic completed rollup");
        assert_eq!(completed.state, WorkDelegationState::Completed);
        assert_eq!(completed.version, delegation.version + 3);
        assert_eq!(
            completed.resolution_summary.as_deref(),
            accepted.result_summary.as_deref()
        );
        assert!(store
            .transition_work_and_roll_up_delegation(
                &target.id,
                host_work_context(
                    "rollup-completed-retry",
                    "rollup-completed-retry-command",
                    "unix-ms:12",
                ),
            )
            .expect("terminal rollup retry is a no-op")
            .is_empty());
        let source_after = store
            .latest_works()
            .unwrap()
            .into_iter()
            .find(|work| work.id == source.id)
            .expect("source remains visible");
        assert_eq!(
            source_after, source,
            "target result never mutates source Work"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_delegation_cancel_is_cas_fenced_and_idempotent() {
        let (root, store, run_a, member_a, run_b, member_b) =
            delegation_test_fixture("delegation-cancel");
        let source = store
            .insert_work(
                assigned_delegation_work(&run_a, &member_a, "source-cancel"),
                host_work_context("work-source-cancel", "create-source-cancel", "unix-ms:2"),
            )
            .expect("create source Work");
        let (delegation, _) = store
            .create_work_delegation_with_target_work(
                delegation_request("delegation-cancel", &source, &run_b.agent_team_id),
                assigned_delegation_work(&run_b, &member_b, "target-cancel"),
                host_work_context(
                    "delegation-create-cancel",
                    "delegate-source-cancel",
                    "unix-ms:3",
                ),
            )
            .expect("create Delegation");
        let stale = store
            .cancel_work_delegation(
                &delegation.id,
                0,
                "target no longer needed",
                host_work_context(
                    "delegation-cancel-stale",
                    "cancel-delegation-stale",
                    "unix-ms:4",
                ),
            )
            .expect_err("stale expected version is fenced");
        assert!(stale.to_string().contains("DELEGATION_VERSION_CONFLICT"));
        let context = host_work_context(
            "delegation-cancel-event",
            "cancel-delegation-command",
            "unix-ms:5",
        );
        let cancelled = store
            .cancel_work_delegation(
                &delegation.id,
                delegation.version,
                "target no longer needed",
                context.clone(),
            )
            .expect("cancel Delegation");
        assert_eq!(cancelled.state, WorkDelegationState::Cancelled);
        assert_eq!(cancelled.version, 2);
        assert_eq!(
            store
                .cancel_work_delegation(
                    &delegation.id,
                    delegation.version,
                    "target no longer needed",
                    context,
                )
                .expect("same cancel command replays idempotently"),
            cancelled
        );
        let conflict = store
            .cancel_work_delegation(
                &delegation.id,
                delegation.version,
                "different reason",
                host_work_context("ignored", "cancel-delegation-command", "unix-ms:6"),
            )
            .expect_err("same key cannot change cancel reason");
        assert!(conflict.to_string().contains("IDEMPOTENCY_CONFLICT"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn completed_team_run(run: &AgentTeamRun, at: &str) -> AgentTeamRun {
        let mut completed = run.clone();
        completed.status = TeamRunStatus::Completed;
        completed.updated_at = at.into();
        completed.completed_at = Some(at.into());
        completed
    }

    #[test]
    fn legacy_raw_work_operation_is_rejected_without_a_read_fallback() {
        let root = team_test_root("legacy-raw-work-operation");
        let store = HarnessStore::new(&root);
        store.init().expect("initialize legacy replay store");
        let raw_operation = serde_json::json!({
            "event": {
                "id": "work-event-legacy-raw-1",
                "team_run_id": "team-run-legacy-raw-1",
                "work_id": "work-legacy-raw-1",
                "sequence": 1,
                "kind": "created",
                "expected_version": 0,
                "resulting_version": 1,
                "performed_by_actor": { "kind": "host", "id": "host" },
                "idempotency_key": "create-work-legacy-raw-1",
                "created_at": "unix-ms:1"
            },
            "work": {
                "id": "work-legacy-raw-1",
                "team_run_id": "team-run-legacy-raw-1",
                "title": "Replay a historical WorkOperation",
                "context_markdown": "Raw JSONL compatibility row",
                "completion_criteria_markdown": "Both Store projections remain readable",
                "status": "open",
                "claim_mode": "team_claim",
                "priority": "normal",
                "created_by_actor": { "kind": "host", "id": "host" },
                "version": 1,
                "created_at": "unix-ms:1",
                "updated_at": "unix-ms:1"
            }
        });
        std::fs::write(
            root.join("work_operations.jsonl"),
            format!("{raw_operation}\n"),
        )
        .expect("write historical WorkOperation bytes");

        let error = store
            .work_operations()
            .expect_err("legacy Work status must not gain a read fallback");
        assert!(
            error.to_string().contains("unknown field `status`"),
            "legacy Work rows must fail with an actionable schema error: {error}"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn team_run_completion_guard_is_store_authoritative() {
        let (root, store, run, _, _) = work_test_fixture("completion-guard");
        store
            .insert_work(
                unassigned_test_work(&run.id, "work-open"),
                host_work_context("we-open", "create-open", "unix-ms:2"),
            )
            .expect("create open Work");

        let error = store
            .compare_and_append_team_run_lifecycle(&run, &completed_team_run(&run, "unix-ms:3"))
            .expect_err("Store must reject completion while Work is non-terminal");
        assert!(
            error
                .to_string()
                .contains("Works remain non-terminal: work-open (open/normal, version 1)"),
            "completion guard should identify the authoritative unfinished Work: {error}"
        );
        assert_eq!(
            store
                .team_runs()
                .expect("read TeamRuns")
                .into_iter()
                .rev()
                .find(|candidate| candidate.id == run.id)
                .expect("TeamRun remains present")
                .status,
            TeamRunStatus::Running,
            "a rejected completion must not append a terminal TeamRun row"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn team_run_completion_and_work_create_serialize_without_invalid_state() {
        for iteration in 0..16 {
            let (root, store, run, _, _) =
                work_test_fixture(&format!("completion-create-race-{iteration}"));
            let barrier = Arc::new(Barrier::new(3));

            let completion_store = store.clone();
            let completion_run = run.clone();
            let completion_barrier = Arc::clone(&barrier);
            let completion = std::thread::spawn(move || {
                completion_barrier.wait();
                completion_store.compare_and_append_team_run_lifecycle(
                    &completion_run,
                    &completed_team_run(&completion_run, "unix-ms:3"),
                )
            });

            let work_store = store.clone();
            let work_run_id = run.id.clone();
            let work_barrier = Arc::clone(&barrier);
            let create = std::thread::spawn(move || {
                work_barrier.wait();
                work_store.insert_work(
                    unassigned_test_work(&work_run_id, "work-racing"),
                    host_work_context("we-racing", "create-racing", "unix-ms:2"),
                )
            });

            barrier.wait();
            let completion_result = completion.join().expect("completion thread");
            let create_result = create.join().expect("Work create thread");
            assert_ne!(
                completion_result.is_ok(),
                create_result.is_ok(),
                "the write lock must serialize the race so exactly one operation succeeds"
            );

            let latest_run = store
                .team_runs()
                .expect("read TeamRuns")
                .into_iter()
                .rev()
                .find(|candidate| candidate.id == run.id)
                .expect("TeamRun remains present");
            let has_nonterminal_work = store
                .latest_works()
                .expect("read Works")
                .into_iter()
                .any(|work| work.team_run_id == run.id && !work.is_terminal());
            assert!(
                latest_run.status != TeamRunStatus::Completed || !has_nonterminal_work,
                "completed TeamRun plus non-terminal Work is forbidden regardless of race winner"
            );

            std::fs::remove_dir_all(root).expect("remove temp store");
        }
    }

    #[test]
    fn blocked_work_can_be_resumed_by_owner_or_host_with_a_recorded_resolution() {
        let (root, store, run, member, _) = work_test_fixture("work-resume");
        let mut assigned = unassigned_test_work(&run.id, "work-resume-owner");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-resume-1", "create-resume-1", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let started = store
            .start_work(
                &assigned.id,
                assigned.version,
                &member.id,
                member_work_context(&member.id, "we-resume-2", "start-resume-1", "unix-ms:3"),
            )
            .expect("start Work");
        let blocked = store
            .block_work(
                &started.id,
                started.version,
                &member.id,
                "dependency unavailable",
                member_work_context(&member.id, "we-resume-3", "block-resume-1", "unix-ms:4"),
            )
            .expect("owner blocks Work");
        let empty = store
            .resume_work(
                &blocked.id,
                blocked.version,
                &member.id,
                "  ",
                member_work_context(&member.id, "ignored", "empty-resolution", "unix-ms:5"),
            )
            .expect_err("resume requires a resolution");
        assert!(empty.to_string().contains("resolution is required"));
        let resumed = store
            .resume_work(
                &blocked.id,
                blocked.version,
                &member.id,
                "dependency restored",
                member_work_context(&member.id, "we-resume-4", "resume-owner", "unix-ms:5"),
            )
            .expect("owner resumes Work");
        assert_eq!(resumed.phase, WorkPhase::Active);
        assert!(resumed.blocker_reason.is_none());
        let resumed_event = store
            .work_events()
            .expect("events")
            .into_iter()
            .find(|event| event.id == "we-resume-4")
            .expect("resumed event");
        assert_eq!(resumed_event.kind, WorkEventKind::Resumed);
        assert_eq!(resumed_event.payload["resolution"], "dependency restored");
        let condition_records = store.work_condition_records().expect("condition records");
        let blocked_record = condition_records
            .iter()
            .find(|record| {
                record.condition == WorkCondition::Blocked && record.resolved_at.is_none()
            })
            .expect("active blocker record");
        let resolved_record = condition_records
            .iter()
            .find(|record| record.resolved_at.is_some())
            .expect("resolved blocker record");
        assert_eq!(
            resolved_record.supersedes_condition_record_id.as_deref(),
            Some(blocked_record.id.as_str())
        );
        assert_eq!(resolved_record.work_version, resumed.version);
        assert!(store
            .latest_work_deliveries()
            .expect("deliveries")
            .iter()
            .any(|delivery| {
                delivery.work_id == resumed.id
                    && delivery.work_version == resumed.version
                    && delivery.status == ProviderWorkDispatchStatus::Queued
            }));

        let blocked_by_host = store
            .block_work_as_host(
                &resumed.id,
                resumed.version,
                "Host paused integration",
                host_work_context("we-resume-5", "block-host", "unix-ms:6"),
            )
            .expect("Host blocks Work");
        let resumed_by_host = store
            .resume_work_as_host(
                &blocked_by_host.id,
                blocked_by_host.version,
                "integration boundary cleared",
                host_work_context("we-resume-6", "resume-host", "unix-ms:7"),
            )
            .expect("Host resumes Work");
        assert_eq!(resumed_by_host.phase, WorkPhase::Active);
        assert_eq!(resumed_by_host.active_member_run_id, Some(member.id));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn release_clears_safe_open_ownership_and_rejects_an_in_flight_delivery() {
        let (root, store, run, member, _) = work_test_fixture("work-release");
        let mut assigned = unassigned_test_work(&run.id, "work-release-safe");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-release-1", "create-release-1", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let released = store
            .release_work(
                &assigned.id,
                assigned.version,
                &member.id,
                member_work_context(&member.id, "we-release-2", "release-owner", "unix-ms:3"),
            )
            .expect("owner releases queued Work");
        assert_eq!(released.phase, WorkPhase::Open);
        assert!(released.owner_member_id.is_none());
        assert!(released.active_member_run_id.is_none());
        assert!(store
            .latest_work_deliveries()
            .expect("deliveries")
            .iter()
            .any(|delivery| {
                delivery.work_id == released.id
                    && delivery.status == ProviderWorkDispatchStatus::Invalidated
            }));

        let mut in_flight = unassigned_test_work(&run.id, "work-release-in-flight");
        in_flight.active_member_run_id = Some(member.id.clone());
        in_flight.claim_mode = WorkClaimMode::HostAssign;
        let in_flight = store
            .insert_work(
                in_flight,
                host_work_context("we-release-3", "create-release-2", "unix-ms:4"),
            )
            .expect("create second assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == in_flight.id)
            .expect("queued delivery");
        let lease = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-1", 11, "test:release", 100, 100)
            .expect("lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-release",
                101,
                "unix-ms:5",
            )
            .expect("claim delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
        };
        let error = store
            .release_work_as_host(
                &in_flight.id,
                in_flight.version,
                host_work_context("we-release-4", "release-host", "unix-ms:6"),
            )
            .expect_err("in-flight Work cannot be released");
        assert!(error.to_string().contains("RECONCILIATION_REQUIRED"));

        let _received = store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "native-receipt-release",
                102,
                "unix-ms:7",
            )
            .expect("record provider receipt");
        let received_error = store
            .release_work_as_host(
                &in_flight.id,
                in_flight.version,
                host_work_context("we-release-5", "release-received", "unix-ms:8"),
            )
            .expect_err("provider-received Work cannot be released");
        assert!(received_error
            .to_string()
            .contains("RECONCILIATION_REQUIRED"));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn historical_provider_receipt_does_not_lock_later_work_revisions() {
        let (root, store, run, member, peer) = work_test_fixture("historical-receipt");
        let mut assigned = unassigned_test_work(&run.id, "work-historical-receipt");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-history-1", "history-create", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == assigned.id)
            .expect("initial delivery");
        let lease = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-history", 3, "test", 100, 100)
            .expect("lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-history",
                101,
                "unix-ms:3",
            )
            .expect("claim delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
        };
        store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "native-receipt-history",
                102,
                "unix-ms:4",
            )
            .expect("provider receives revision 1");

        let mut failed_previous = member.clone();
        failed_previous.status = MemberRunStatus::Failed;
        failed_previous.finished_at = Some("unix-ms:5".into());
        store
            .compare_and_append_member_run(&member, &failed_previous)
            .expect("record runtime failure");
        let mut replacement = member.clone();
        replacement.id = "member-history-generation-2".into();
        replacement.runtime_generation += 1;
        replacement.status = MemberRunStatus::Idle;
        replacement.started_at = "unix-ms:6".into();
        replacement.finished_at = None;
        admit_replacement_for_test(&store, &replacement);

        let rebound = store
            .rebind_work(
                &assigned.id,
                assigned.version,
                &replacement.id,
                host_work_context("we-history-2", "history-rebind", "unix-ms:7"),
            )
            .expect("rebind advances Work beyond historical receipt");
        let released = store
            .release_work_as_host(
                &rebound.id,
                rebound.version,
                host_work_context("we-history-3", "history-release", "unix-ms:8"),
            )
            .expect("historical receipt must not block release of newer revision");
        let reassigned = store
            .assign_work(
                &released.id,
                released.version,
                &peer.id,
                host_work_context("we-history-4", "history-assign", "unix-ms:9"),
            )
            .expect("historical receipt must not block later assignment");
        assert_eq!(
            reassigned.active_member_run_id.as_deref(),
            Some(peer.id.as_str())
        );
        assert!(store
            .latest_work_deliveries()
            .expect("deliveries")
            .iter()
            .any(|candidate| {
                candidate.id == delivery.id
                    && candidate.status == ProviderWorkDispatchStatus::ProviderReceived
                    && candidate.provider_receipt_id.as_deref() == Some("native-receipt-history")
            }));
        let reassigned_delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|candidate| {
                candidate.work_id == reassigned.id
                    && candidate.work_version == reassigned.version
                    && candidate.recipient_member_run_id == peer.id
            })
            .expect("reassigned delivery");
        let reassigned_claim = match store
            .claim_work_delivery(
                &run.id,
                &reassigned_delivery.id,
                &peer.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-reassigned-history",
                103,
                "unix-ms:10",
            )
            .expect("claim reassigned delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("reassigned delivery must be claimed"),
        };
        store
            .complete_work_delivery_claim(
                &run.id,
                &reassigned_delivery.id,
                &peer.id,
                &lease.supervisor_id,
                lease.generation,
                reassigned_claim.claim_id.as_deref().expect("claim id"),
                "native-receipt-reassigned",
                104,
                "unix-ms:11",
            )
            .expect("provider receives reassigned revision");
        let started = store
            .start_work(
                &reassigned.id,
                reassigned.version,
                &peer.id,
                member_work_context(&peer.id, "we-history-5", "history-start", "unix-ms:12"),
            )
            .expect("member advances beyond its provider receipt");
        let cancelled = store
            .cancel_work(
                &started.id,
                started.version,
                "Host no longer needs this Work",
                host_work_context("we-history-6", "history-cancel", "unix-ms:13"),
            )
            .expect("historical receipts must not block cancellation");
        assert_eq!(cancelled.resolution, Some(WorkResolution::Cancelled));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn delivery_projection_folds_cross_file_updates_by_store_sequence() {
        let (root, store, run, member, _) = work_test_fixture("delivery-fold-sequence");
        let mut assigned = unassigned_test_work(&run.id, "work-fold-sequence");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-fold-1", "fold-create", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == assigned.id)
            .expect("initial delivery");
        let first = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-fold-1", 4, "test", 100, 10)
            .expect("first lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &first.supervisor_id,
                first.generation,
                "claim-fold",
                101,
                // Caller timestamps are deliberately non-monotonic. The
                // Store sequence, not this string, is authoritative.
                "unix-ms:999",
            )
            .expect("claim delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
        };
        assert_eq!(claimed.status, ProviderWorkDispatchStatus::Claimed);
        let successor = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-fold-2", 5, "test", 111, 100)
            .expect("successor lease");
        store
            .reconcile_stale_work_delivery_claim(
                &run.id,
                &delivery.id,
                &successor.supervisor_id,
                successor.generation,
                112,
                "unix-ms:998",
            )
            .expect("standalone update requeues delivery");
        let released = store
            .release_work_as_host(
                &assigned.id,
                assigned.version,
                host_work_context("we-fold-2", "fold-release", "unix-ms:1"),
            )
            .expect("embedded update invalidates the later-requeued delivery");
        assert_eq!(released.version, 2);
        let projected = store
            .latest_work_deliveries()
            .expect("project deliveries")
            .into_iter()
            .find(|candidate| candidate.id == delivery.id)
            .expect("delivery remains as evidence");
        assert_eq!(projected.status, ProviderWorkDispatchStatus::Invalidated);
        let standalone_updates = store
            .read_jsonl::<ProviderWorkDispatchUpdate>("work_delivery_updates.jsonl")
            .expect("standalone updates");
        let embedded_updates = store
            .work_operations()
            .expect("operations")
            .into_iter()
            .flat_map(|operation| operation.delivery_updates)
            .collect::<Vec<_>>();
        assert!(standalone_updates
            .iter()
            .all(|update| update.update_sequence > 0));
        assert!(embedded_updates
            .iter()
            .all(|update| update.update_sequence > 0));
        assert!(
            embedded_updates
                .iter()
                .map(|update| update.update_sequence)
                .max()
                .expect("embedded sequence")
                > standalone_updates
                    .iter()
                    .map(|update| update.update_sequence)
                    .max()
                    .expect("standalone sequence")
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_event_id_reuse_is_rejected_before_delivery_identity_can_collide() {
        let (root, store, run, member, _) = work_test_fixture("event-id-uniqueness");
        let mut first = unassigned_test_work(&run.id, "work-event-id-first");
        first.active_member_run_id = Some(member.id.clone());
        first.claim_mode = WorkClaimMode::HostAssign;
        store
            .insert_work(
                first,
                host_work_context("same-work-event", "event-first", "unix-ms:2"),
            )
            .expect("first event and delivery");
        let mut second = unassigned_test_work(&run.id, "work-event-id-second");
        second.active_member_run_id = Some(member.id.clone());
        second.claim_mode = WorkClaimMode::HostAssign;
        let error = store
            .insert_work(
                second,
                host_work_context("same-work-event", "event-second", "unix-ms:3"),
            )
            .expect_err("caller event id reuse must be rejected");
        assert!(error.to_string().contains("WORK_EVENT_ID_CONFLICT"));
        assert_eq!(store.work_operations().expect("operations").len(), 1);
        assert_eq!(store.latest_work_deliveries().expect("deliveries").len(), 1);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn successor_supervisor_reconciles_a_stale_work_delivery_claim_before_reclaim() {
        let (root, store, run, member, _) = work_test_fixture("work-delivery-reconcile");
        let mut assigned = unassigned_test_work(&run.id, "work-reconcile");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        store
            .insert_work(
                assigned,
                host_work_context("we-reconcile-1", "create-reconcile", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == "work-reconcile")
            .expect("queued delivery");
        let first = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-1", 11, "test:first", 100, 10)
            .expect("first lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &first.supervisor_id,
                first.generation,
                "claim-generation-1",
                101,
                "unix-ms:3",
            )
            .expect("first claim")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
        };
        assert_eq!(claimed.attempt, 1);

        let second = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-2", 22, "test:successor", 111, 100)
            .expect("successor lease");
        assert_eq!(second.generation, 2);
        let requeued = store
            .reconcile_stale_work_delivery_claim(
                &run.id,
                &delivery.id,
                &second.supervisor_id,
                second.generation,
                112,
                "unix-ms:4",
            )
            .expect("successor reconciles stale claim");
        assert_eq!(requeued.status, ProviderWorkDispatchStatus::Queued);
        assert_eq!(requeued.attempt, 1);
        assert!(requeued.claim_id.is_none());

        let reclaimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &second.supervisor_id,
                second.generation,
                "claim-generation-2",
                113,
                "unix-ms:5",
            )
            .expect("successor reclaims delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be reclaimable"),
        };
        assert_eq!(reclaimed.attempt, 2);
        assert_eq!(reclaimed.claimed_generation, Some(second.generation));
        let received = store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &second.supervisor_id,
                second.generation,
                reclaimed.claim_id.as_deref().expect("second claim id"),
                "native-receipt-reconcile",
                114,
                "unix-ms:6",
            )
            .expect("record provider receipt");
        assert_eq!(
            received.status,
            ProviderWorkDispatchStatus::ProviderReceived
        );
        assert_eq!(
            store
                .complete_work_delivery_claim(
                    &run.id,
                    &delivery.id,
                    &member.id,
                    &second.supervisor_id,
                    second.generation,
                    reclaimed.claim_id.as_deref().expect("second claim id"),
                    "native-receipt-reconcile",
                    115,
                    "unix-ms:6-retry",
                )
                .expect("same provider receipt retry is idempotent"),
            received
        );
        let different_receipt = store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &second.supervisor_id,
                second.generation,
                reclaimed.claim_id.as_deref().expect("second claim id"),
                "different-native-receipt",
                116,
                "unix-ms:6-retry-2",
            )
            .expect_err("a retry cannot rewrite receipt identity");
        assert!(different_receipt
            .to_string()
            .contains("different provider receipt"));
        let third = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-3", 33, "test:third", 212, 100)
            .expect("third lease");
        let uncertain = store
            .reconcile_stale_work_delivery_claim(
                &run.id,
                &delivery.id,
                &third.supervisor_id,
                third.generation,
                213,
                "unix-ms:7",
            )
            .expect_err("provider-received delivery is never rolled back");
        assert!(uncertain.to_string().contains("cannot be requeued"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    #[ignore = "legacy Work acceptance route is retired; canonical exact-candidate acceptance is covered by member_execution_trust"]
    fn work_delivery_waits_for_prerequisites_and_current_lease_can_fail_its_claim() {
        let (root, store, run, member_a, member_b) = work_test_fixture("work-delivery-ready");
        let prerequisite = store
            .insert_work(
                unassigned_test_work(&run.id, "work-prerequisite"),
                host_work_context("we-ready-1", "ready-create-prereq", "unix-ms:2"),
            )
            .expect("create prerequisite");
        let claimed_prerequisite = store
            .claim_work(
                &prerequisite.id,
                prerequisite.version,
                &member_b.id,
                member_work_context(
                    &member_b.id,
                    "we-ready-2",
                    "ready-claim-prereq",
                    "unix-ms:3",
                ),
            )
            .expect("claim prerequisite");

        let mut dependent = unassigned_test_work(&run.id, "work-dependent");
        dependent.claim_mode = WorkClaimMode::HostAssign;
        dependent.active_member_run_id = Some(member_a.id.clone());
        dependent.prerequisite_work_ids = vec![prerequisite.id.clone()];
        let dependent = store
            .insert_work(
                dependent,
                host_work_context("we-ready-3", "ready-create-dependent", "unix-ms:4"),
            )
            .expect("create dependent");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == dependent.id)
            .expect("dependent delivery");
        let lease = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-ready", 7, "test", 100, 100)
            .expect("lease");
        assert_eq!(
            store
                .claim_work_delivery(
                    &run.id,
                    &delivery.id,
                    &member_a.id,
                    &lease.supervisor_id,
                    lease.generation,
                    "claim-before-ready",
                    101,
                    "unix-ms:5",
                )
                .expect("not ready is not an error"),
            WorkDeliveryClaimResult::NotQueued
        );

        let submitted = store
            .submit_work(
                &prerequisite.id,
                claimed_prerequisite.version,
                &member_b.id,
                "prerequisite complete",
                Vec::new(),
                vec!["check://ready".into()],
                member_work_context(
                    &member_b.id,
                    "we-ready-4",
                    "ready-submit-prereq",
                    "unix-ms:6",
                ),
            )
            .expect("submit prerequisite");
        store
            .accept_work(
                &submitted.id,
                submitted.version,
                host_work_context("we-ready-5", "ready-accept-prereq", "unix-ms:7"),
            )
            .expect("accept prerequisite");

        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member_a.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-after-ready",
                102,
                "unix-ms:8",
            )
            .expect("claim ready delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must now be claimable"),
        };
        let failed = store
            .fail_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member_a.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "provider transport exited before receipt",
                103,
                "unix-ms:9",
            )
            .expect("current lease fails claim");
        assert_eq!(failed.status, ProviderWorkDispatchStatus::Failed);
        assert_eq!(
            failed.failure_reason.as_deref(),
            Some("provider transport exited before receipt")
        );
        assert_eq!(
            store
                .fail_work_delivery_claim(
                    &run.id,
                    &delivery.id,
                    &member_a.id,
                    &lease.supervisor_id,
                    lease.generation,
                    claimed.claim_id.as_deref().expect("claim id"),
                    "provider transport exited before receipt",
                    104,
                    "unix-ms:10",
                )
                .expect("same failure retry is idempotent"),
            failed
        );
        let retried = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member_a.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-after-transport-failure",
                105,
                "unix-ms:11",
            )
            .expect("failed pre-receipt delivery remains retryable")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => {
                panic!("failed pre-receipt delivery must be retryable")
            }
        };
        assert_eq!(retried.status, ProviderWorkDispatchStatus::Claimed);
        assert_eq!(retried.attempt, 2);
        assert_eq!(
            retried.claim_id.as_deref(),
            Some("claim-after-transport-failure")
        );
        assert!(retried.failure_reason.is_none());
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn host_rebind_fences_old_runtime_and_preserves_provider_receipt_evidence() {
        let (root, store, run, member, peer) = work_test_fixture("work-rebind-runtime");
        let mut assigned = unassigned_test_work(&run.id, "work-rebind");
        assigned.claim_mode = WorkClaimMode::HostAssign;
        assigned.active_member_run_id = Some(member.id.clone());
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-rebind-1", "rebind-create", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == assigned.id)
            .expect("initial delivery");
        let lease = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-rebind", 9, "test", 100, 100)
            .expect("lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-rebind",
                101,
                "unix-ms:3",
            )
            .expect("claim initial delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("initial delivery must be queued"),
        };
        store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "provider-receipt-before-crash",
                102,
                "unix-ms:4",
            )
            .expect("provider receipt");
        let started = store
            .start_work(
                &assigned.id,
                assigned.version,
                &member.id,
                member_work_context(&member.id, "we-rebind-2", "rebind-start", "unix-ms:5"),
            )
            .expect("start before runtime crash");

        let mut failed_previous = member.clone();
        failed_previous.status = MemberRunStatus::Failed;
        failed_previous.finished_at = Some("unix-ms:6".into());
        store
            .compare_and_append_member_run(&member, &failed_previous)
            .expect("record previous runtime failure");

        let mut replacement = member.clone();
        replacement.id = "member-a-generation-2".into();
        replacement.runtime_generation = member.runtime_generation + 1;
        replacement.status = MemberRunStatus::Idle;
        replacement.started_at = "unix-ms:7".into();
        replacement.finished_at = None;
        admit_replacement_for_test(&store, &replacement);
        let owner_mismatch = store
            .rebind_work(
                &started.id,
                started.version,
                &peer.id,
                host_work_context("ignored", "rebind-peer", "unix-ms:8"),
            )
            .expect_err("Host cannot change stable owner through rebind");
        assert!(owner_mismatch.to_string().contains("OWNER_MISMATCH"));
        let rebound = store
            .rebind_work(
                &started.id,
                started.version,
                &replacement.id,
                host_work_context("we-rebind-3", "rebind-runtime", "unix-ms:9"),
            )
            .expect("Host rebinds stable owner to replacement runtime");
        assert_eq!(rebound.phase, WorkPhase::Active);
        assert_eq!(rebound.owner_member_id, started.owner_member_id);
        assert_eq!(
            rebound.active_member_run_id.as_deref(),
            Some(replacement.id.as_str())
        );
        let deliveries = store.latest_work_deliveries().expect("deliveries");
        assert!(deliveries.iter().any(|candidate| {
            candidate.id == delivery.id
                && candidate.status == ProviderWorkDispatchStatus::ProviderReceived
                && candidate.provider_receipt_id.as_deref() == Some("provider-receipt-before-crash")
        }));
        let replacement_delivery = deliveries
            .iter()
            .find(|candidate| {
                candidate.work_id == rebound.id
                    && candidate.work_version == rebound.version
                    && candidate.recipient_member_run_id == replacement.id
            })
            .expect("fresh delivery for replacement");
        assert!(matches!(
            store
                .claim_work_delivery(
                    &run.id,
                    &replacement_delivery.id,
                    &replacement.id,
                    &lease.supervisor_id,
                    lease.generation,
                    "claim-replacement",
                    103,
                    "unix-ms:11",
                )
                .expect("in-progress revision is deliverable"),
            WorkDeliveryClaimResult::Claimed(_)
        ));
        let fenced = store
            .submit_work(
                &started.id,
                started.version,
                &member.id,
                "stale runtime result",
                Vec::new(),
                Vec::new(),
                member_work_context(&member.id, "ignored", "stale-submit", "unix-ms:12"),
            )
            .expect_err("old runtime version is fenced");
        assert!(fenced.to_string().contains("VERSION_CONFLICT"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn rebind_redelivers_same_member_run_id_at_a_higher_runtime_generation() {
        let (root, store, run, member, _) = work_test_fixture("same-id-generation-rebind");
        let mut assigned = unassigned_test_work(&run.id, "work-same-id-rebind");
        assigned.claim_mode = WorkClaimMode::HostAssign;
        assigned.owner_member_id = Some(member.agent_member_id.clone());
        assigned.active_member_run_id = Some(member.id.clone());
        let created = store
            .insert_work(
                assigned,
                member_work_context(
                    &member.id,
                    "event-create-same-id-rebind",
                    "command-create-same-id-rebind",
                    "unix-ms:3",
                ),
            )
            .expect("create assigned Work");

        let mut failed = member.clone();
        failed.status = MemberRunStatus::Failed;
        failed.finished_at = Some("unix-ms:4".into());
        store
            .compare_and_append_member_run(&member, &failed)
            .expect("record failed generation");
        let mut replacement = member.clone();
        replacement.runtime_generation += 1;
        replacement.status = MemberRunStatus::Idle;
        replacement.started_at = "unix-ms:5".into();
        replacement.finished_at = None;
        store
            .compare_and_append_member_run(&failed, &replacement)
            .expect("append same-id replacement generation");

        let rebound = store
            .rebind_work(
                &created.id,
                created.version,
                &replacement.id,
                host_work_context(
                    "event-rebind-same-id-generation",
                    "command-rebind-same-id-generation",
                    "unix-ms:6",
                ),
            )
            .expect("higher same-id generation must fence and redeliver Work");
        assert_eq!(rebound.active_member_run_id, created.active_member_run_id);
        assert_eq!(rebound.team_id, created.team_id);
        assert_eq!(rebound.created_by_member_id, created.created_by_member_id);
        let operation = store
            .work_operations()
            .unwrap()
            .into_iter()
            .find(|operation| operation.event.kind == WorkEventKind::Rebound)
            .expect("Rebound operation");
        assert_eq!(operation.event.payload["previous_runtime_generation"], 1);
        assert_eq!(operation.event.payload["replacement_runtime_generation"], 2);
        assert!(store
            .latest_work_deliveries()
            .unwrap()
            .iter()
            .any(|delivery| {
                delivery.work_id == rebound.id
                    && delivery.work_version == rebound.version
                    && delivery.recipient_member_run_id == replacement.id
                    && delivery.status == ProviderWorkDispatchStatus::Queued
            }));
        assert!(store
            .rebind_work(
                &rebound.id,
                rebound.version,
                &replacement.id,
                host_work_context(
                    "event-repeat-same-id-generation",
                    "command-repeat-same-id-generation",
                    "unix-ms:7",
                ),
            )
            .expect_err("same runtime generation cannot rebound twice")
            .to_string()
            .contains("WORK_ALREADY_BOUND"));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn sparse_mixed_version_rebound_recovers_and_repersists_work_provenance() {
        let (root, store, run, member, _) = work_test_fixture("sparse-rebound-provenance");

        let mut assigned = unassigned_test_work(&run.id, "work-sparse-rebound");
        assigned.claim_mode = WorkClaimMode::HostAssign;
        assigned.owner_member_id = Some(member.agent_member_id.clone());
        assigned.active_member_run_id = Some(member.id.clone());
        let created = store
            .insert_work(
                assigned,
                member_work_context(
                    &member.id,
                    "event-create-sparse-rebound",
                    "command-create-sparse-rebound",
                    "unix-ms:3",
                ),
            )
            .expect("Member creates Team-scoped Work");
        assert_eq!(created.team_id.as_deref(), Some(run.agent_team_id.as_str()));
        assert_eq!(
            created.created_by_member_id,
            Some(member.agent_member_id.clone())
        );

        let mut replacement = member.clone();
        replacement.id = "member-sparse-rebound-generation-2".into();
        replacement.runtime_generation += 1;
        replacement.started_at = "unix-ms:4".into();
        let mut failed_previous = member.clone();
        failed_previous.status = MemberRunStatus::Failed;
        failed_previous.finished_at = Some("unix-ms:4".into());
        store
            .compare_and_append_member_run(&member, &failed_previous)
            .expect("close previous runtime before replacement");
        admit_replacement_for_test(&store, &replacement);

        let rebound_context = host_work_context(
            "event-sparse-mixed-writer-rebound",
            "command-sparse-mixed-writer-rebound",
            "unix-ms:5",
        );
        let mut sparse_work = created.clone();
        sparse_work.active_member_run_id = Some(replacement.id.clone());
        sparse_work.team_id = None;
        sparse_work.created_by_member_id = None;
        sparse_work.version += 1;
        sparse_work.updated_at = rebound_context.created_at.clone();
        let sparse_operation = WorkOperation {
            event: WorkEvent {
                id: rebound_context.event_id,
                team_run_id: sparse_work.team_run_id.clone(),
                work_id: sparse_work.id.clone(),
                sequence: 2,
                kind: WorkEventKind::Rebound,
                expected_version: created.version,
                resulting_version: sparse_work.version,
                performed_by_actor: rebound_context.performed_by_actor,
                authority_actor: rebound_context.authority_actor,
                causation_ref: rebound_context.causation_ref,
                idempotency_key: rebound_context.idempotency_key,
                payload: serde_json::json!({
                    "previous_member_run_id": member.id.clone(),
                    "replacement_member_run_id": replacement.id.clone(),
                }),
                created_at: rebound_context.created_at,
            },
            work: sparse_work,
            condition_records: Vec::new(),
            reports: Vec::new(),
            evidence_records: Vec::new(),
            decisions: Vec::new(),
            deliveries: Vec::new(),
            delivery_updates: Vec::new(),
            delegation_revisions: Vec::new(),
        };
        let refused = store
            .append_work_operation_unlocked(&sparse_operation)
            .expect_err("current writer must refuse provenance regression");
        assert!(refused
            .to_string()
            .contains("WORK_PROJECTION_PROVENANCE_REGRESSION"));

        // Model the already-observed stale HTTP writer: it omitted both keys
        // entirely, bypassing code this newer binary did not yet contain.
        let mut sparse_json = serde_json::to_value(&sparse_operation).expect("operation JSON");
        let sparse_projection = sparse_json["work"]
            .as_object_mut()
            .expect("Work projection object");
        sparse_projection.remove("team_id");
        sparse_projection.remove("created_by_member_id");
        store
            .append_jsonl("work_operations.jsonl", &sparse_json)
            .expect("simulate stale mixed-version append");
        let raw = store.work_operations().expect("raw WorkOperations");
        assert!(raw.last().expect("sparse rebound").work.team_id.is_none());
        assert!(raw
            .last()
            .expect("sparse rebound")
            .work
            .created_by_member_id
            .is_none());

        let recovered = store.latest_works().expect("recovered Works").remove(0);
        assert_eq!(recovered.team_id, created.team_id);
        assert_eq!(recovered.created_by_member_id, created.created_by_member_id);
        let repair_context = host_work_context(
            "event-reconcile-sparse-rebound",
            "command-reconcile-sparse-rebound",
            "unix-ms:6",
        );
        let repaired = store
            .reconcile_work_projection_provenance(
                &recovered.id,
                recovered.version,
                repair_context.clone(),
            )
            .expect("explicit reconciliation re-persists recovered provenance");
        assert_eq!(repaired.phase, WorkPhase::Open);
        assert_eq!(
            repaired.active_member_run_id.as_deref(),
            Some(replacement.id.as_str())
        );
        assert_eq!(repaired.team_id, created.team_id);
        assert_eq!(repaired.created_by_member_id, created.created_by_member_id);
        assert_eq!(
            store
                .reconcile_work_projection_provenance(
                    &recovered.id,
                    recovered.version,
                    repair_context,
                )
                .expect("repair retry is idempotent"),
            repaired
        );
        let raw = store.work_operations().expect("repaired WorkOperations");
        assert_eq!(raw.last().expect("repair operation").work, repaired);
        assert_eq!(raw.last().unwrap().event.kind, WorkEventKind::Updated);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn unavailable_members_and_idempotency_key_reuse_are_rejected() {
        let (root, store, run, member, _) = work_test_fixture("work-command-guards");
        let first = store
            .insert_work(
                unassigned_test_work(&run.id, "work-idempotent-a"),
                host_work_context("we-guard-1", "shared-key", "unix-ms:2"),
            )
            .expect("first command");
        let other_work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-idempotent-b"),
                host_work_context("ignored", "shared-key", "unix-ms:3"),
            )
            .expect_err("same key cannot identify a different Work");
        assert!(other_work.to_string().contains("IDEMPOTENCY_CONFLICT"));
        let other_command = store
            .assign_work(
                &first.id,
                first.version,
                &member.id,
                host_work_context("ignored", "shared-key", "unix-ms:4"),
            )
            .expect_err("same key cannot identify a different command");
        assert!(other_command.to_string().contains("IDEMPOTENCY_CONFLICT"));

        let mut failed_member = member.clone();
        failed_member.status = MemberRunStatus::Failed;
        failed_member.finished_at = Some("unix-ms:5".into());
        store
            .compare_and_append_member_run(&member, &failed_member)
            .expect("record failed member");
        let mut assigned_to_failed = unassigned_test_work(&run.id, "work-failed-member");
        assigned_to_failed.claim_mode = WorkClaimMode::HostAssign;
        assigned_to_failed.active_member_run_id = Some(failed_member.id.clone());
        let failed = store
            .insert_work(
                assigned_to_failed,
                host_work_context("we-guard-2", "create-failed", "unix-ms:6"),
            )
            .expect_err("failed member cannot receive owned Work");
        assert!(failed.to_string().contains("MEMBER_UNAVAILABLE"));

        let mut stopped_member = failed_member.clone();
        stopped_member.status = MemberRunStatus::Stopped;
        store
            .compare_and_append_member_run(&failed_member, &stopped_member)
            .expect("record stopped member");
        let stopped_work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-assign-stopped"),
                host_work_context("we-guard-3", "create-for-stopped", "unix-ms:7"),
            )
            .expect("create unassigned Work");
        let stopped = store
            .assign_work(
                &stopped_work.id,
                stopped_work.version,
                &stopped_member.id,
                host_work_context("we-guard-4", "assign-stopped", "unix-ms:8"),
            )
            .expect_err("stopped member cannot be assigned");
        assert!(stopped.to_string().contains("MEMBER_UNAVAILABLE"));

        let mut closed_member = stopped_member.clone();
        closed_member.status = MemberRunStatus::Idle;
        closed_member.coordination_status = firm_core::MemberCoordinationStatus::Closed;
        store
            .compare_and_append_member_run(&stopped_member, &closed_member)
            .expect("record closed coordination");
        let unassigned = store
            .insert_work(
                unassigned_test_work(&run.id, "work-assign-closed"),
                host_work_context("we-guard-5", "create-unassigned", "unix-ms:9"),
            )
            .expect("create unassigned Work");
        let closed = store
            .assign_work(
                &unassigned.id,
                unassigned.version,
                &closed_member.id,
                host_work_context("we-guard-6", "assign-closed", "unix-ms:10"),
            )
            .expect_err("closed member cannot be assigned");
        assert!(closed.to_string().contains("MEMBER_UNAVAILABLE"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn closed_member_cannot_mutate_owned_work_until_reopen() {
        let (root, store, run, member, _) = work_test_fixture("closed-member-owned-work");
        let created = store
            .insert_work(
                unassigned_test_work(&run.id, "work-owned-closed"),
                host_work_context("we-closed-1", "create-owned", "unix-ms:2"),
            )
            .expect("create Work");
        let assigned = store
            .assign_work(
                &created.id,
                created.version,
                &member.id,
                host_work_context("we-closed-2", "assign-owned", "unix-ms:3"),
            )
            .expect("assign Work");
        let started = store
            .start_work(
                &assigned.id,
                assigned.version,
                &member.id,
                member_work_context(&member.id, "we-closed-3", "start-owned", "unix-ms:4"),
            )
            .expect("start Work");

        // Close lands mid-execution: coordination flips Closed while the Work
        // stays owned and InProgress.
        let mut closed_member = member.clone();
        closed_member.coordination_status = firm_core::MemberCoordinationStatus::Closed;
        closed_member.status = MemberRunStatus::Stopped;
        store
            .compare_and_append_member_run(&member, &closed_member)
            .expect("record closed coordination");

        let blocked = store
            .block_work(
                &started.id,
                started.version,
                &member.id,
                "still blocked",
                member_work_context(&member.id, "we-closed-4", "block-owned", "unix-ms:5"),
            )
            .expect_err("closed member cannot block owned Work");
        assert!(
            blocked.to_string().contains("MEMBER_UNAVAILABLE"),
            "unexpected error: {blocked}"
        );
        let submitted = store
            .submit_work(
                &started.id,
                started.version,
                &member.id,
                "result from a closed runtime",
                Vec::new(),
                Vec::new(),
                member_work_context(&member.id, "we-closed-5", "submit-owned", "unix-ms:6"),
            )
            .expect_err("closed member cannot submit owned Work");
        assert!(
            submitted.to_string().contains("MEMBER_UNAVAILABLE"),
            "unexpected error: {submitted}"
        );
        // The Work projection is untouched by both rejections.
        let current = store
            .latest_works()
            .expect("latest works")
            .into_iter()
            .find(|work| work.id == started.id)
            .expect("owned Work");
        assert_eq!(current.phase, WorkPhase::Active);
        assert_eq!(current.version, started.version);

        // Reopen (coordination Active, next runtime generation) restores the
        // member-side transition path for the same durable Work.
        let mut reopened_member = closed_member.clone();
        reopened_member.coordination_status = firm_core::MemberCoordinationStatus::Active;
        reopened_member.status = MemberRunStatus::Idle;
        reopened_member.runtime_generation += 1;
        store
            .compare_and_append_member_run(&closed_member, &reopened_member)
            .expect("record reopened member");
        let submitted = store
            .submit_work(
                &started.id,
                started.version,
                &member.id,
                "result after reopen",
                Vec::new(),
                Vec::new(),
                member_work_context(&member.id, "we-closed-6", "submit-reopened", "unix-ms:7"),
            )
            .expect("reopened member submits owned Work");
        assert_eq!(submitted.phase, WorkPhase::Review);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn concurrent_work_claim_has_exactly_one_winner_and_idempotent_retry() {
        let (root, store, run, member_a, member_b) = work_test_fixture("work-claim-race");
        store
            .insert_work(
                unassigned_test_work(&run.id, "work-race"),
                host_work_context("we-race-1", "create-race", "unix-ms:2"),
            )
            .expect("create Work");
        let store = Arc::new(store);
        let barrier = Arc::new(Barrier::new(3));
        let handles = [member_a, member_b]
            .into_iter()
            .enumerate()
            .map(|(index, member)| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    store.claim_work(
                        "work-race",
                        1,
                        &member.id,
                        member_work_context(
                            &member.id,
                            &format!("we-race-{}", index + 2),
                            &format!("claim-race-{index}"),
                            "unix-ms:3",
                        ),
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("claim thread"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        let winner = results.into_iter().find_map(Result::ok).expect("winner");
        let retry_member = winner
            .active_member_run_id
            .as_deref()
            .expect("active member");
        let retried = store
            .claim_work(
                "work-race",
                1,
                retry_member,
                member_work_context(
                    retry_member,
                    "ignored",
                    if retry_member.ends_with("-a") {
                        "claim-race-0"
                    } else {
                        "claim-race-1"
                    },
                    "unix-ms:4",
                ),
            )
            .expect("idempotent retry");
        assert_eq!(retried, winner);
        assert!(
            store
                .latest_work_deliveries()
                .expect("deliveries")
                .is_empty(),
            "the winning Member already possesses self-claimed Work in its bound runtime"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn member_created_work_is_limited_to_self_or_unassigned() {
        let (root, store, run, member_a, member_b) = work_test_fixture("member-work-authority");

        let mut peer_owned = unassigned_test_work(&run.id, "work-peer-owned");
        peer_owned.active_member_run_id = Some(member_b.id.clone());
        peer_owned.claim_mode = WorkClaimMode::HostAssign;
        let error = store
            .insert_work(
                peer_owned,
                member_work_context(
                    &member_a.id,
                    "we-member-peer",
                    "member-create-peer",
                    "unix-ms:2",
                ),
            )
            .expect_err("ordinary Member must not assign peer-owned Work");
        assert!(
            error
                .to_string()
                .contains("only self-owned or unassigned Work"),
            "error: {error}"
        );

        let mut self_owned = unassigned_test_work(&run.id, "work-self-owned");
        self_owned.active_member_run_id = Some(member_a.id.clone());
        self_owned.claim_mode = WorkClaimMode::HostAssign;
        let self_owned = store
            .insert_work(
                self_owned,
                member_work_context(
                    &member_a.id,
                    "we-member-self",
                    "member-create-self",
                    "unix-ms:3",
                ),
            )
            .expect("Member creates self-owned Work");
        assert_eq!(
            self_owned.active_member_run_id.as_deref(),
            Some(member_a.id.as_str())
        );
        assert_eq!(self_owned.owner_member_id.as_deref(), Some("agent-a"));

        let unassigned = store
            .insert_work(
                unassigned_test_work(&run.id, "work-unassigned-child"),
                member_work_context(
                    &member_a.id,
                    "we-member-open",
                    "member-create-open",
                    "unix-ms:4",
                ),
            )
            .expect("Member creates unassigned Work");
        assert!(unassigned.owner_member_id.is_none());
        assert!(unassigned.active_member_run_id.is_none());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn team_message_work_link_must_resolve_inside_the_same_team_run() {
        let (root, store, run, member, _) = work_test_fixture("message-work-link");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-discussed"),
                host_work_context("we-discussed", "create-discussed", "unix-ms:2"),
            )
            .expect("create discussed Work");
        let message = ProviderDispatchEnvelope {
            id: "tm-work-discussion".into(),
            team_run_id: run.id.clone(),
            work_id: Some(work.id.clone()),
            source_plan_ref: None,
            sender: None,
            sender_runtime_id: "host".into(),
            recipients: Vec::new(),
            recipient_runtime_ids: vec![member.id.clone()],
            kind: ProviderDispatchIntent::Message,
            body: "Clarify the evidence for this Work.".into(),
            correlation_id: "corr-work-discussion".into(),
            causation_id: None,
            response_intent: Some(ProviderResponseIntent::ResponseRequired),
            evidence_refs: Vec::new(),
            deliveries: vec![ProviderDispatchAttempt {
                member_id: member.id.clone(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: "unix-ms:3".into(),
            }],
            created_at: "unix-ms:3".into(),
        };
        store
            .append_team_message_checked(&message)
            .expect("same-TeamRun Work discussion");

        let mut foreign = message;
        foreign.id = "tm-cross-run-work".into();
        foreign.team_run_id = "another-team-run".into();
        let error = store
            .append_team_message_checked(&foreign)
            .expect_err("cross-TeamRun Work link must be rejected");
        assert!(
            error.to_string().contains("belongs to TeamRun"),
            "error: {error}"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn legacy_assignment_message_is_not_readable_by_works_store() {
        let (root, store, run, _, _) = work_test_fixture("legacy-work-store");
        append_sparse_row(
            &root,
            "team_messages.jsonl",
            &format!(
                r#"{{"id":"legacy-assignment","team_run_id":"{}","sender_runtime_id":"host","kind":"assignment","body":"legacy","correlation_id":"legacy","created_at":"unix-ms:1"}}"#,
                run.id
            ),
        );
        let error = store
            .insert_work(
                unassigned_test_work(&run.id, "work-rejected"),
                host_work_context("we-rejected", "create-rejected", "unix-ms:2"),
            )
            .expect_err("legacy store must be rejected");
        assert!(error.to_string().contains("assignment"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    // ── duplicate-title guard ──────────────────────────────────────────

    fn work_with_title(run_id: &str, id: &str, title: &str) -> Work {
        let mut work = unassigned_test_work(run_id, id);
        work.title = title.to_string();
        work
    }

    #[test]
    fn duplicate_title_guard_refuses_non_terminal_match() {
        let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-guard");
        let ctx1 = host_work_context("dup-ctx-1", "create-first", "unix-ms:3");
        store
            .insert_work(
                work_with_title(&run.id, "work-audit-1", "Audit Company Docs"),
                ctx1,
            )
            .expect("create first Work");

        let ctx2 = host_work_context("dup-ctx-2", "create-dup", "unix-ms:4");
        let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
        let error = store
            .insert_work(dup, ctx2)
            .expect_err("duplicate title must fail");
        assert!(
            error.to_string().contains("DUPLICATE_TITLE"),
            "error: {error}"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn duplicate_title_guard_allows_when_flag_is_duplicate_ok() {
        let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-flag");
        let ctx1 = host_work_context("dup-ctx-flag-1", "create-first", "unix-ms:3");
        store
            .insert_work(
                work_with_title(&run.id, "work-audit-1", "Audit Company Docs"),
                ctx1,
            )
            .expect("create first Work");

        let mut ctx2 = host_work_context("dup-ctx-flag-2", "create-dup-ok", "unix-ms:4");
        ctx2.duplicate_ok = true;
        let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
        let created = store
            .insert_work(dup, ctx2)
            .expect("duplicate-ok must allow same-title Work");
        assert_eq!(created.title, "Audit Company Docs");
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    #[ignore = "legacy Work acceptance route is retired; canonical exact-candidate acceptance is covered by member_execution_trust"]
    fn duplicate_title_guard_allows_when_existing_is_done() {
        let (root, store, run, member_a, _member_b) = work_test_fixture("dup-title-done");
        let ctx1 = host_work_context("dup-ctx-done-1", "create-first", "unix-ms:3");
        let mut work = work_with_title(&run.id, "work-audit-1", "Audit Company Docs");
        work.claim_mode = WorkClaimMode::HostAssign;
        work.active_member_run_id = Some(member_a.id.clone());
        work.owner_member_id = Some(member_a.agent_member_id.clone());
        let first = store.insert_work(work, ctx1).expect("create first Work");

        // Start → Submit → Accept to make the work Done.
        let first = store
            .start_work(
                &first.id,
                first.version,
                &member_a.id,
                member_work_context(&member_a.id, "start", "start-key", "unix-ms:4"),
            )
            .expect("start");
        let first = store
            .submit_work(
                &first.id,
                first.version,
                &member_a.id,
                "All tests pass.",
                Vec::new(),
                Vec::new(),
                member_work_context(&member_a.id, "submit", "submit-key", "unix-ms:5"),
            )
            .expect("submit");
        store
            .accept_work(
                &first.id,
                first.version,
                host_work_context("accept", "accept-key", "unix-ms:6"),
            )
            .expect("accept first Work");

        let ctx2 = host_work_context("dup-ctx-done-2", "create-after-done", "unix-ms:7");
        let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
        store
            .insert_work(dup, ctx2)
            .expect("terminal existing Work must not block new same-title");
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn duplicate_title_guard_normalizes_casing_and_spacing() {
        let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-normalize");
        let ctx1 = host_work_context("dup-norm-1", "create-first", "unix-ms:3");
        store
            .insert_work(
                work_with_title(&run.id, "work-norm-1", "audit company docs"),
                ctx1,
            )
            .expect("create first Work");

        let ctx2 = host_work_context("dup-norm-2", "create-dup-norm", "unix-ms:4");
        let dup = work_with_title(&run.id, "work-norm-2", "AUDIT   COMPANY   DOCS");
        let error = store
            .insert_work(dup, ctx2)
            .expect_err("different casing/spacing must still be detected");
        assert!(
            error.to_string().contains("DUPLICATE_TITLE"),
            "error: {error}"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn test_message(id: &str, agent_id: &str) -> RegistryMessage {
        RegistryMessage {
            id: id.into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some(agent_id.into()),
            channel: Some("assignment".into()),
            kind: RegistryMessageIntent::Message,
            delivery_status: RegistryDeliveryStatus::Queued,
            content: "Do the task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
            sender_kind: SenderKind::Agent,
        }
    }

    fn test_delivery(delivery_id: &str) -> RegistryDeliveryAttempt {
        RegistryDeliveryAttempt {
            delivery_id: Some(delivery_id.into()),
            execution_status: Some(ProviderExecutionStatus::Running),
            native_session: None,
            started_at: Some("unix-ms:1".into()),
            provider_request_id: None,
            provider_thread_id: None,
            provider_turn_id: None,
            terminal_source: None,
            delivered_at: None,
            last_error: None,
        }
    }

    fn temp_store(label: &str) -> (PathBuf, HarnessStore) {
        let root = std::env::temp_dir().join(format!(
            "firm-store-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        let store = HarnessStore::new(&root);
        (root, store)
    }

    // ── Lane B: upstream event push — Work lifecycle → Host attention ──

    #[test]
    fn work_submit_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-submit-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-submit-ha-1"),
                host_work_context("we-submit-1", "create-submit-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-submit-2", "claim-submit-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let _submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec!["check://y".into()],
                member_work_context(&member.id, "we-submit-3", "submit-submit-ha", "unix-ms:4"),
            )
            .expect("submit Work");
        let attentions = store.host_attentions().expect("host attentions");
        let review = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested);
        assert!(
            review.is_some(),
            "bound run must emit WorkReviewRequested on submit"
        );
        assert_eq!(review.unwrap().team_run_id, run.id);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn test_github_link(status: &str, ci_status: Option<&str>) -> firm_core::GitHubLink {
        firm_core::GitHubLink {
            kind: firm_core::GitHubLinkKind::PullRequest,
            owner: "cyl19970726".into(),
            repo: "multi-agent-harness".into(),
            number: 365,
            url: "https://github.com/cyl19970726/multi-agent-harness/pull/365".into(),
            status: Some(status.into()),
            ci_status: ci_status.map(str::to_string),
            ci_url: Some(
                "https://github.com/cyl19970726/multi-agent-harness/actions/runs/1".into(),
            ),
        }
    }

    #[test]
    fn update_work_github_links_refreshes_snapshot_without_churn() {
        let (root, store, run, _member, _) = work_test_fixture("github-update");
        let created = store
            .insert_work(
                unassigned_test_work(&run.id, "github-update-1"),
                host_work_context("we-gu-1", "create-github-update", "unix-ms:2"),
            )
            .expect("create Work");
        assert!(created.github_links.is_empty());

        let refreshed = store
            .update_work_github_links(
                &created.id,
                created.version,
                vec![test_github_link("MERGED", Some("success"))],
                host_work_context("we-gu-2", "poll-github-update-1", "unix-ms:3"),
            )
            .expect("refresh snapshot");
        assert_eq!(refreshed.version, created.version + 1);
        assert_eq!(refreshed.github_links.len(), 1);
        assert_eq!(
            refreshed.github_links[0].ci_status.as_deref(),
            Some("success")
        );

        // Steady-state poll with identical links must not churn versions.
        let unchanged = store
            .update_work_github_links(
                &created.id,
                refreshed.version,
                vec![test_github_link("MERGED", Some("success"))],
                host_work_context("we-gu-3", "poll-github-update-2", "unix-ms:4"),
            )
            .expect("steady-state poll is a no-op");
        assert_eq!(unchanged.version, refreshed.version);

        // A changed CI outcome appends one more Updated operation.
        let re_polled = store
            .update_work_github_links(
                &created.id,
                unchanged.version,
                vec![test_github_link("MERGED", Some("failure"))],
                host_work_context("we-gu-4", "poll-github-update-3", "unix-ms:5"),
            )
            .expect("changed CI refreshes");
        assert_eq!(re_polled.version, unchanged.version + 1);
        assert_eq!(
            re_polled.github_links[0].ci_status.as_deref(),
            Some("failure")
        );

        // Stale expected version is rejected.
        let stale = store.update_work_github_links(
            &created.id,
            created.version,
            vec![test_github_link("MERGED", Some("success"))],
            host_work_context("we-gu-5", "poll-github-update-4", "unix-ms:6"),
        );
        assert!(
            stale.is_err() && stale.unwrap_err().to_string().contains("VERSION_CONFLICT"),
            "stale poll must conflict"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    #[ignore = "legacy Work acceptance route is retired; canonical report-bound acceptance is covered by member_execution_trust"]
    fn review_link_refresh_derives_a_report_bound_to_the_new_work_version() {
        let (root, store, run, member, _) = work_test_fixture("github-review-report-refresh");
        let created = store
            .insert_work(
                unassigned_test_work(&run.id, "github-review-report-refresh-1"),
                host_work_context("we-grr-1", "create-grr", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &created.id,
                created.version,
                &member.id,
                member_work_context(&member.id, "we-grr-2", "claim-grr", "unix-ms:3"),
            )
            .expect("claim Work");
        let submitted = store
            .submit_work_with_revision_and_links(
                &claimed.id,
                claimed.version,
                &member.id,
                "candidate",
                vec!["artifact://candidate".into()],
                vec!["check://candidate".into()],
                Vec::new(),
                Some("base-sha".into()),
                Some("candidate-sha".into()),
                member_work_context(&member.id, "we-grr-3", "submit-grr", "unix-ms:4"),
            )
            .expect("submit Work");
        let refreshed = store
            .update_work_github_links(
                &submitted.id,
                submitted.version,
                vec![test_github_link("MERGED", Some("success"))],
                host_work_context("we-grr-4", "refresh-grr", "unix-ms:5"),
            )
            .expect("refresh review links");

        let reports = store.work_reports().expect("reports");
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].work_version, submitted.version);
        assert_eq!(reports[1].work_version, refreshed.version);
        assert_eq!(reports[1].candidate_revision, "candidate-sha");
        assert_eq!(reports[1].report_revision, reports[0].report_revision + 1);
        let evidence = store.work_evidence().expect("evidence");
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[1].work_report_id, reports[1].id);
        assert_eq!(evidence[1].work_version, refreshed.version);

        let accepted = store
            .accept_work(
                &refreshed.id,
                refreshed.version,
                host_work_context("we-grr-5", "accept-grr", "unix-ms:6"),
            )
            .expect("current derived report authorizes acceptance");
        assert_eq!(accepted.phase, WorkPhase::Closed);
        assert_eq!(
            store.work_operational_decisions().unwrap()[0]
                .work_report_id
                .as_deref(),
            Some(reports[1].id.as_str())
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn submit_work_on_pr_merge_transitions_in_progress_work_to_review() {
        let (root, store, run, member, _) = work_test_fixture("github-merge-submit");
        let created = store
            .insert_work(
                unassigned_test_work(&run.id, "github-merge-submit-1"),
                host_work_context("we-ms-1", "create-github-merge", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &created.id,
                created.version,
                &member.id,
                member_work_context(&member.id, "we-ms-2", "claim-github-merge", "unix-ms:3"),
            )
            .expect("claim Work");
        assert_eq!(claimed.phase, WorkPhase::Active);

        // Refuses when no MERGED pull_request link is present.
        let not_merged = store.submit_work_on_pr_merge(
            &claimed.id,
            claimed.version,
            "auto-submit",
            vec![test_github_link("OPEN", Some("success"))],
            host_work_context("we-ms-3", "submit-merge-reject", "unix-ms:4"),
        );
        assert!(
            not_merged.is_err()
                && not_merged
                    .unwrap_err()
                    .to_string()
                    .contains("PR_MERGE_REQUIRED"),
            "auto-submit without a MERGED link must be refused"
        );

        // Observed merge transitions InProgress -> Review with the fresh
        // snapshot stored.
        let submitted = store
            .submit_work_on_pr_merge(
                &claimed.id,
                claimed.version,
                "auto-submitted by GitHub merge observation",
                vec![test_github_link("MERGED", Some("success"))],
                host_work_context("we-ms-4", "submit-merge-ok", "unix-ms:5"),
            )
            .expect("auto-submit on merge");
        assert_eq!(submitted.phase, WorkPhase::Review);
        assert_eq!(
            submitted.result_summary.as_deref(),
            Some("auto-submitted by GitHub merge observation")
        );
        assert_eq!(submitted.github_links[0].status.as_deref(), Some("MERGED"));
        assert_eq!(
            submitted.github_links[0].ci_status.as_deref(),
            Some("success")
        );

        // A review Work is not auto-submittable again.
        let re_submit = store.submit_work_on_pr_merge(
            &submitted.id,
            submitted.version,
            "again",
            vec![test_github_link("MERGED", Some("success"))],
            host_work_context("we-ms-5", "submit-merge-again", "unix-ms:6"),
        );
        assert!(
            re_submit.is_err()
                && re_submit
                    .unwrap_err()
                    .to_string()
                    .contains("required state"),
            "review Work must not be auto-submitted twice"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_block_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-block-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-block-ha-1"),
                host_work_context("we-block-1", "create-block-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-block-2", "claim-block-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let _blocked = store
            .block_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "dependency missing",
                member_work_context(&member.id, "we-block-3", "block-block-ha", "unix-ms:4"),
            )
            .expect("block Work");
        let attentions = store.host_attentions().expect("host attentions");
        let blocked = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkBlocked);
        assert!(
            blocked.is_some(),
            "bound run must emit WorkBlocked on block"
        );
        assert_eq!(blocked.unwrap().team_run_id, run.id);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    #[ignore = "legacy Work acceptance route is retired; canonical acceptance side effects are covered by member_execution_trust"]
    fn work_accept_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-accept-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-accept-ha-1"),
                host_work_context("we-accept-1", "create-accept-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-accept-2", "claim-accept-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://z".into()],
                vec![],
                member_work_context(&member.id, "we-accept-3", "submit-accept-ha", "unix-ms:4"),
            )
            .expect("submit Work");
        let _accepted = store
            .accept_work_with_summary(
                &submitted.id,
                submitted.version,
                Some("Host accepted"),
                host_work_context("we-accept-4", "accept-accept-ha", "unix-ms:5"),
            )
            .expect("accept Work");
        let attentions = store.host_attentions().expect("host attentions");
        let accepted = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkAccepted);
        assert!(
            accepted.is_some(),
            "bound run must emit WorkAccepted on accept"
        );
        assert_eq!(accepted.unwrap().team_run_id, run.id);
        // WorkReviewRequested should still be present from the earlier submit
        let review = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested);
        assert!(
            review.is_some(),
            "WorkReviewRequested must persist after accept"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_changes_requested_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-cr-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-cr-ha-1"),
                host_work_context("we-cr-1", "create-cr-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-cr-2", "claim-cr-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec![],
                member_work_context(&member.id, "we-cr-3", "submit-cr-ha", "unix-ms:4"),
            )
            .expect("submit Work");
        let _changes = store
            .request_work_changes(
                &submitted.id,
                submitted.version,
                "needs more tests",
                host_work_context("we-cr-4", "request-changes-cr-ha", "unix-ms:5"),
            )
            .expect("request changes");
        let attentions = store.host_attentions().expect("host attentions");
        let cr = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkChangesRequested);
        assert!(
            cr.is_some(),
            "bound run must emit WorkChangesRequested on request changes"
        );
        assert_eq!(cr.unwrap().team_run_id, run.id);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_cancel_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-cancel-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-cancel-ha-1"),
                host_work_context("we-cancel-1", "create-cancel-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-cancel-2", "claim-cancel-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let _cancelled = store
            .cancel_work(
                &claimed.id,
                claimed.version,
                "no longer needed",
                host_work_context("we-cancel-3", "cancel-cancel-ha", "unix-ms:4"),
            )
            .expect("cancel Work");
        let attentions = store.host_attentions().expect("host attentions");
        let cancelled = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkCancelled);
        assert!(
            cancelled.is_some(),
            "bound run must emit WorkCancelled on cancel"
        );
        assert_eq!(cancelled.unwrap().team_run_id, run.id);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn host_attention_dedup_ignores_duplicate_event() {
        let (root, store, run, member, _) = work_test_fixture("work-dedup-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-dedup-ha-1"),
                host_work_context("we-dedup-1", "create-dedup-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-dedup-2", "claim-dedup-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let ctx = member_work_context(&member.id, "we-dedup-3", "submit-dedup-ha", "unix-ms:4");
        let _submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec![],
                ctx.clone(),
            )
            .expect("first submit");
        // Second submit with same idempotency key should be a no-op (dedup).
        let _again = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec![],
                ctx,
            )
            .expect("idempotent second submit");
        let attentions = store.host_attentions().expect("host attentions");
        let review_count = attentions
            .iter()
            .filter(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested)
            .count();
        assert_eq!(
            review_count, 1,
            "dedup must emit exactly one WorkReviewRequested"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_transitions_dont_fail_for_unbound_run() {
        let root = team_test_root("work-unbound-ha");
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: "tr-work-unbound-ha".into(),
            agent_team_id: "team-work-unbound-ha".into(),
            execution_node_id: "00000000-0000-4000-8000-000000000001".into(),
            project_binding_id: "project-test".into(),
            previous_run_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: None,
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "prove unbound graceful".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-work-unbound-ha".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("append unbound run");
        let member = ProviderRuntimeProjection {
            id: "mr-work-unbound-ha".into(),
            team_run_id: run.id.clone(),
            slot_id: Some("slot-unbound".into()),
            agent_member_id: "agent-unbound".into(),
            name: "Member Unbound".into(),
            role: "builder".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            provider_compatibility_block_cause: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Idle,
            native_session: None,
            provider_cwd_hint: None,
            provider_environment_observation: None,
            owned_paths: Vec::new(),
            started_at: "unix-ms:1".into(),
            last_event_at: None,
            finished_at: None,
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };
        store.append_member_run(&member).expect("append member");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-unbound-ha-1"),
                host_work_context("we-ub-1", "create-ub-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-ub-2", "claim-ub-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let _submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec![],
                member_work_context(&member.id, "we-ub-3", "submit-ub-ha", "unix-ms:4"),
            )
            .expect("submit Work with unbound run");
        let attentions = store.host_attentions().expect("host attentions");
        // HostAttention is still emitted at the store level even for unbound runs;
        // the runtime delivery layer gates on binding, not the store.
        let review = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested);
        assert!(
            review.is_some(),
            "WorkReviewRequested must still be emitted for unbound runs"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_delivery_failure_emits_host_attention() {
        let (root, store, run, member, _) = work_test_fixture("work-wdf-ha");
        let mut assigned = unassigned_test_work(&run.id, "work-wdf-ha-1");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-wdf-1", "create-wdf-ha", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|d| d.work_id == assigned.id)
            .expect("delivery");
        let lease = store
            .acquire_test_supervisor_lease(&run.id, "supervisor-wdf", 7, "test", 100, 100)
            .expect("lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-wdf",
                100,
                "unix-ms:3",
            )
            .expect("claim")
        {
            WorkDeliveryClaimResult::Claimed(d) => d,
            _ => panic!("delivery must be claimed"),
        };
        let failed = store
            .fail_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "provider crash",
                101,
                "unix-ms:4",
            )
            .expect("fail delivery");
        assert_eq!(failed.status, ProviderWorkDispatchStatus::Failed);
        let attentions = store.host_attentions().expect("host attentions");
        let wdf = attentions
            .iter()
            .find(|a| a.work_id == assigned.id && a.kind == HostAttentionKind::WorkDeliveryFailed);
        assert!(
            wdf.is_some(),
            "must emit WorkDeliveryFailed for failed delivery claim"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn node_project_registration_is_fenced_to_selected_execution_space() {
        let (root, store) = temp_store("node-project-space-fence");
        let node_id = "00000000-0000-4000-8000-000000000001";
        store
            .insert_execution_node(&ExecutionNode {
                id: node_id.into(),
                display_name: "space-fenced-node".into(),
                status: ExecutionNodeStatus::Active,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
            })
            .expect("insert Node");
        let registration = NodeProjectRegistration {
            node_id: node_id.into(),
            execution_space_id: "other-space".into(),
            project_binding_id: "project-test".into(),
            status: NodeProjectRegistrationStatus::Active,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        };
        let mismatch = store
            .register_node_project(&registration, "selected-space")
            .expect_err("cross-space registration must be rejected");
        assert!(mismatch
            .to_string()
            .contains("EXECUTION_SPACE_SCOPE_MISMATCH"));
        assert!(store
            .latest_node_project_registrations()
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(root).expect("remove temp store");
    }
}

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use firm_core::agentfirm_api::{CanonicalWorkDelivery, WorkDeliveryStatus};
use firm_core::{
    content_hash_hex16, AgentTeam, AgentTeamRun, Decision, DelegationRun, Evidence, ExecutionNode,
    ExecutionNodeStatus, Gap, GitHubLink, HostAttention, HostAttentionInbox, HostAttentionKind,
    HostAttentionStatus, HostBindingLease, HostBindingLeaseOwnerKind, HostBindingLeaseStatus,
    LegacyWave, MemberAction, MessageTerminalSource, Mission, MissionLogEntry, NodeDaemonLease,
    NodeDaemonLeaseStatus, NodeProjectRegistration, NodeProjectRegistrationStatus, Proposal,
    ProviderChildThread, ProviderCompatibilityAdmission, ProviderCompatibilityAdmissionLifecycle,
    ProviderCompatibilityBlockBoundary, ProviderCompatibilityBlockCause,
    ProviderCompatibilityStatus, ProviderExecutionStatus, ProviderIntegrationProfile,
    ProviderLaunchProfile, ProviderProcess, ProviderRuntimeProjection, RegistryDeliveryAttempt,
    RegistryDeliveryStatus, RegistryMessage, Review, TeamActorKind, TeamActorRef,
    TeamMemberCloseRequest, TeamMemberCloseStatus, TeamMessageProjection, TeamRunEvent,
    TeamRunStatus, TeamSupervisorLease, TeamSupervisorLeaseStatus, Validate, Vision, Work,
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

mod collaboration;
mod collaboration_fabric;
pub use collaboration::*;
pub use collaboration_fabric::*;
pub mod remote_fabric_store;

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
    #[error(
        "CURRENT_WORK_DELIVERY_SNAPSHOT_UNSTABLE: canonical trust authority changed throughout the bounded stabilization window"
    )]
    CurrentWorkDeliverySnapshotUnstable,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid company os record: {0}")]
    CompanyOsValidation(String),
    #[error("company os reference not found: {0}")]
    CompanyOsMissingReference(String),
}

impl StoreError {
    /// Preserve the typed Trust Kernel decision across the compatibility
    /// `Conflict(String)` wire boundary. Policy callers must inspect this
    /// value instead of classifying the display message.
    pub fn trust_error(&self) -> Option<firm_core::agentfirm_api::TrustError> {
        match self {
            Self::Conflict(value) => serde_json::from_str(value).ok(),
            _ => None,
        }
    }
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
            "team_id": target_work.accountable_team_id,
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
    Claimed(Box<TeamMessageProjection>),
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
    process_write_lock: Arc<ProcessWriteLock>,
}

mod store_current_work_delivery;
mod store_host_attention;
mod store_host_attention_internals;
mod store_host_runtime_binding;
mod store_jsonl;
mod store_node_runtime;
mod store_read_models;
mod store_store_base;
mod store_team_admission;
mod store_team_journal;
mod store_work_application;
mod store_work_graph;
mod store_work_mutations;
mod store_work_projection;
mod store_work_state;

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
        || next.provider_cwd_hint != current.provider_cwd_hint
    {
        return Err(StoreError::Conflict(format!(
            "MEMBER_PROVENANCE_IMMUTABLE: ProviderRuntimeProjection {} cannot change its team, stable identity, role, provider, or workspace root",
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

fn require_non_empty_store(value: &str, label: &str) -> StoreResult<()> {
    if value.trim().is_empty() {
        Err(StoreError::Conflict(format!("{label} must not be empty")))
    } else {
        Ok(())
    }
}

fn require_host_actor(actor: &firm_core::TeamActorRef) -> StoreResult<()> {
    if actor.kind == firm_core::TeamActorKind::Host && !actor.id.trim().is_empty() {
        Ok(())
    } else {
        Err(StoreError::Conflict(
            "Host authority is required for this Work command".to_string(),
        ))
    }
}

impl HarnessStore {
    /// Resolve and verify the one current Host authority for a TeamRun. A
    /// compatibility Operator/Service actor or the historical literal `host`
    /// cannot authorize current Work writes unless that is the exact durable
    /// AgentMember id on both the TeamRun and active Host membership.
    pub fn exact_team_run_host_actor(&self, team_run_id: &str) -> StoreResult<TeamActorRef> {
        let run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|run| run.id == team_run_id)
            .ok_or_else(|| StoreError::Conflict(format!("TeamRun not found: {team_run_id}")))?;
        let actor = run.host_actor.clone().ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_RUN_HOST_AUTHORITY_REQUIRED: TeamRun {team_run_id} has no exact Host actor"
            ))
        })?;
        require_host_actor(&actor)?;
        let team = self
            .latest_teams()?
            .remove(&run.agent_team_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TEAM_RUN_REQUIRES_TEAM: AgentTeam {} not found",
                    run.agent_team_id
                ))
            })?;
        let execution_space_id = self.current_team_run_execution_space(&run)?;
        let membership = self.team_host_membership(&execution_space_id, &team.id, true)?;
        if actor.id != team.host_agent_id || membership.agent_member_id != actor.id {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_HOST_AUTHORITY_MISMATCH: TeamRun {team_run_id} Host actor {}, AgentTeam Host {}, and active Host membership {} must identify the same AgentMember",
                actor.id, team.host_agent_id, membership.agent_member_id
            )));
        }
        Ok(actor)
    }

    pub(crate) fn require_exact_team_run_host_actor(
        &self,
        actor: &TeamActorRef,
        team_run_id: &str,
    ) -> StoreResult<()> {
        let run = self.require_team_run_unlocked(team_run_id)?;
        let expected = run.host_actor.clone().ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_RUN_HOST_AUTHORITY_REQUIRED: TeamRun {team_run_id} has no exact Host actor"
            ))
        })?;
        require_host_actor(&expected)?;
        let team = self
            .latest_teams()?
            .remove(&run.agent_team_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "TEAM_RUN_REQUIRES_TEAM: AgentTeam {} not found",
                    run.agent_team_id
                ))
            })?;
        let execution_space_id = match self.current_team_run_execution_space_unlocked(&run) {
            Ok(scope) => scope,
            Err(StoreError::Conflict(message))
                if message.starts_with("MEMBER_RUN_MATERIALIZATION_INCOMPLETE:") =>
            {
                let registrations = latest_by_id(
                    self.read_jsonl::<NodeProjectRegistration>("node_project_registrations.jsonl")?,
                    node_project_registration_identity,
                )
                .into_values()
                .filter(|registration| {
                    registration.node_id == run.execution_node_id
                        && registration.project_binding_id == run.project_binding_id
                        && registration.status == NodeProjectRegistrationStatus::Active
                })
                .map(|registration| registration.execution_space_id)
                .collect::<std::collections::BTreeSet<_>>();
                if registrations.len() != 1 {
                    return Err(StoreError::Conflict(message));
                }
                registrations.into_iter().next().expect("one registration")
            }
            Err(error) => return Err(error),
        };
        let membership = self.team_host_membership(&execution_space_id, &team.id, true)?;
        if expected.id != team.host_agent_id || membership.agent_member_id != expected.id {
            return Err(StoreError::Conflict(format!(
                "TEAM_RUN_HOST_AUTHORITY_MISMATCH: TeamRun {team_run_id} does not bind one exact active Host AgentMember"
            )));
        }
        if actor.kind == expected.kind && actor.id == expected.id {
            Ok(())
        } else {
            Err(StoreError::Conflict(format!(
                "TEAM_RUN_HOST_AUTHORITY_MISMATCH: Work command actor {:?}:{} is not exact Host {:?}:{} for TeamRun {team_run_id}",
                actor.kind, actor.id, expected.kind, expected.id
            )))
        }
    }

    pub(crate) fn require_work_delegation_actor_unlocked(
        &self,
        actor: &TeamActorRef,
        source_team_run_id: &str,
        source_owner_member_id: &str,
        action: &str,
    ) -> StoreResult<Option<String>> {
        match actor.kind {
            TeamActorKind::Host => {
                self.require_exact_team_run_host_actor(actor, source_team_run_id)?;
                Ok(None)
            }
            TeamActorKind::Operator | TeamActorKind::Service => Err(StoreError::Conflict(
                "DELEGATION_NOT_AUTHORIZED: Operator/Service cannot impersonate the exact TeamRun Host"
                    .to_string(),
            )),
            TeamActorKind::ProviderRuntimeProjection => {
                let member = self.require_member_run_unlocked(&actor.id, source_team_run_id)?;
                if member_identity(&member) != source_owner_member_id {
                    return Err(StoreError::Conflict(format!(
                        "DELEGATION_NOT_AUTHORIZED: only source owner or Host may {action}"
                    )));
                }
                Ok(Some(member.id))
            }
            TeamActorKind::AgentMember if actor.id == source_owner_member_id => Ok(None),
            TeamActorKind::AgentMember => Err(StoreError::Conflict(format!(
                "DELEGATION_NOT_AUTHORIZED: only source owner or Host may {action}"
            ))),
        }
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

#[derive(Debug, Default)]
struct ProcessWriteLockState {
    next_ticket: u64,
    serving_ticket: u64,
    cancelled_tickets: std::collections::BTreeSet<u64>,
}

#[derive(Debug, Default)]
struct ProcessWriteLock {
    state: Mutex<ProcessWriteLockState>,
    available: Condvar,
}

impl ProcessWriteLock {
    fn acquire(self: &Arc<Self>, deadline: Instant) -> Option<ProcessWritePermit> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let ticket = state.next_ticket;
        state.next_ticket = state.next_ticket.checked_add(1)?;
        loop {
            if state.serving_ticket == ticket {
                return Some(ProcessWritePermit {
                    lock: Arc::clone(self),
                    ticket,
                });
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                state.cancelled_tickets.insert(ticket);
                advance_cancelled_process_write_tickets(&mut state);
                self.available.notify_all();
                return None;
            }
            let (next, _) = self
                .available
                .wait_timeout(state, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state = next;
        }
    }

    fn release(&self, ticket: u64) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        debug_assert_eq!(state.serving_ticket, ticket);
        state.serving_ticket = state.serving_ticket.saturating_add(1);
        advance_cancelled_process_write_tickets(&mut state);
        self.available.notify_all();
    }
}

fn advance_cancelled_process_write_tickets(state: &mut ProcessWriteLockState) {
    while state.cancelled_tickets.remove(&state.serving_ticket) {
        state.serving_ticket = state.serving_ticket.saturating_add(1);
    }
}

struct ProcessWritePermit {
    lock: Arc<ProcessWriteLock>,
    ticket: u64,
}

impl Drop for ProcessWritePermit {
    fn drop(&mut self) {
        self.lock.release(self.ticket);
    }
}

fn process_write_lock_for(root: &Path) -> Arc<ProcessWriteLock> {
    static LOCKS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Weak<ProcessWriteLock>>>> =
        OnceLock::new();
    let locks = LOCKS.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut locks = locks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let root = canonical_store_lock_root(root);
    if let Some(lock) = locks.get(&root).and_then(Weak::upgrade) {
        return lock;
    }
    locks.retain(|_, lock| lock.strong_count() > 0);
    let lock = Arc::new(ProcessWriteLock::default());
    locks.insert(root, Arc::downgrade(&lock));
    lock
}

/// Resolve every spelling of one physical Store root to one process-local
/// writer queue. The Store directory may not exist yet, so normalize the
/// absolute path first, canonicalize its nearest existing ancestor, then
/// append the still-missing suffix. This makes pre-init and post-init handles,
/// `..` aliases, and symlinked existing ancestors share the same ticket lock.
fn canonical_store_lock_root(root: &Path) -> PathBuf {
    use std::path::Component;

    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(root)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    if let Ok(canonical) = fs::canonicalize(&normalized) {
        return canonical;
    }

    let mut cursor = normalized.as_path();
    let mut missing = Vec::new();
    while let Some(name) = cursor.file_name() {
        missing.push(name.to_os_string());
        let Some(parent) = cursor.parent() else {
            break;
        };
        cursor = parent;
        if let Ok(mut canonical) = fs::canonicalize(cursor) {
            for part in missing.iter().rev() {
                canonical.push(part);
            }
            return canonical;
        }
    }
    normalized
}

struct StoreWriteLock {
    file: File,
    _process_write_permit: ProcessWritePermit,
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
#[path = "lib_tests/mod.rs"]
mod tests;

use crate::{HarnessStore, StoreError, StoreResult};
use firm_core::agentfirm_api::{
    integration_plan_module_v1, ActorKind, AgentMember, AgentMemberOrganizationStatus,
    CanonicalMutationEvent, CanonicalOperation, DeliveryClaim, DeliveryReconcileOutcome,
    FailureAnalysis, GateEvaluation, GateRequirement, GateRequirementSource, GateVerdict,
    GateWaiver, GateWaiverState, MemberCoordinationStatus, MemberRun, MemberRuntimeStatus,
    MemberWorkspaceBinding, MessageDelivery, MessageDeliveryStatus, MutationContext,
    ProviderReceipt, TeamMessage, TrustError, TrustErrorCode, WorkDelivery, WorkDeliveryStatus,
    WorkFinding, WorkModuleBinding, WorkReport, WorkReportKind, WorkspaceLifecycle, WorkspaceMode,
    WorkspaceOwnership, WorkspaceSafetyProof,
};
use firm_core::Work;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const TRUST_OPERATIONS_LEDGER: &str = "agentfirm_trust_operations.jsonl";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TrustOperationEnvelope {
    execution_space_id: String,
    authenticated_actor_kind: ActorKind,
    authenticated_actor_id: String,
    command_name: String,
    operation: CanonicalOperation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanonicalMutationResult<T> {
    pub projection: T,
    pub event: CanonicalMutationEvent,
    pub replayed: bool,
}

fn trust_conflict(error: TrustError) -> StoreError {
    StoreError::Conflict(serde_json::to_string(&error).unwrap_or_else(|_| error.message.clone()))
}

fn trust_error(
    code: TrustErrorCode,
    message: impl Into<String>,
    resource_kind: &str,
    resource_id: &str,
    current_version: Option<u64>,
) -> StoreError {
    trust_conflict(TrustError {
        code,
        message: message.into(),
        retryable: false,
        resource_kind: resource_kind.to_string(),
        resource_id: resource_id.to_string(),
        current_version,
    })
}

fn required(value: &str, field: &str) -> StoreResult<()> {
    if value.trim().is_empty() {
        return Err(trust_error(
            TrustErrorCode::InvalidStateTransition,
            format!("{field} must not be empty"),
            "request",
            field,
            None,
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct ObservedWorkspaceSafety {
    canonical_root: PathBuf,
    git_common_dir: Option<PathBuf>,
    dirty: bool,
    conflicted: bool,
    link_escape_free: bool,
    dirty_fingerprint: Option<String>,
}

fn canonical_git_path(root: &Path, value: &str) -> StoreResult<PathBuf> {
    let path = Path::new(value);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    std::fs::canonicalize(absolute).map_err(StoreError::Io)
}

fn git_output(root: &Path, args: &[&str]) -> StoreResult<Option<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(StoreError::Io)?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_string(),
    ))
}

fn workspace_tree_link_escape_free(root: &Path) -> StoreResult<bool> {
    let canonical_root = std::fs::canonicalize(root).map_err(StoreError::Io)?;
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).map_err(StoreError::Io)? {
            let entry = entry.map_err(StoreError::Io)?;
            let child = entry.path();
            let metadata = std::fs::symlink_metadata(&child).map_err(StoreError::Io)?;
            if metadata.file_type().is_symlink() {
                let Ok(target) = std::fs::canonicalize(&child) else {
                    return Ok(false);
                };
                if !target.starts_with(&canonical_root) {
                    return Ok(false);
                }
            } else if metadata.is_dir() && entry.file_name() != ".git" {
                stack.push(child);
            }
        }
    }
    Ok(true)
}

fn observe_workspace_safety(root: &Path) -> StoreResult<ObservedWorkspaceSafety> {
    let canonical_root = std::fs::canonicalize(root).map_err(StoreError::Io)?;
    let link_escape_free = workspace_tree_link_escape_free(root)?;
    let git_common_dir = git_output(root, &["rev-parse", "--git-common-dir"])?
        .filter(|value| !value.is_empty())
        .map(|value| canonical_git_path(root, &value))
        .transpose()?;
    let porcelain = git_output(
        root,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    )?;
    let conflicts = git_output(root, &["diff", "--name-only", "--diff-filter=U"])?;
    let dirty = porcelain.as_deref().is_some_and(|value| !value.is_empty());
    let conflicted = conflicts.as_deref().is_some_and(|value| !value.is_empty());
    let dirty_fingerprint = porcelain.filter(|value| !value.is_empty()).map(|value| {
        let mut digest = Sha256::new();
        digest.update(value.as_bytes());
        format!("sha256:{:x}", digest.finalize())
    });
    Ok(ObservedWorkspaceSafety {
        canonical_root,
        git_common_dir,
        dirty,
        conflicted,
        link_escape_free,
        dirty_fingerprint,
    })
}

fn now_string() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("unix-ms:{millis}")
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), canonicalize(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

pub fn canonical_json_fingerprint(value: &Value) -> String {
    let bytes = serde_json::to_vec(&canonicalize(value)).expect("canonical JSON serialization");
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn event_projection<T: for<'de> Deserialize<'de>>(
    envelope: &TrustOperationEnvelope,
) -> StoreResult<T> {
    serde_json::from_value(envelope.operation.resulting_projection.clone())
        .map_err(StoreError::from)
}

fn gate_requirement_is_satisfied(
    requirement: &GateRequirement,
    requirements: &BTreeMap<String, GateRequirement>,
    evaluations: &[GateEvaluation],
    waivers: &[GateWaiver],
    visiting: &mut BTreeSet<String>,
) -> bool {
    if !visiting.insert(requirement.id.clone()) {
        return false;
    }
    let dependencies_satisfied = requirement.dependency_requirement_ids.iter().all(|id| {
        requirements.get(id).is_some_and(|dependency| {
            gate_requirement_is_satisfied(dependency, requirements, evaluations, waivers, visiting)
        })
    });
    visiting.remove(&requirement.id);
    if !dependencies_satisfied {
        return false;
    }
    let mut dependency_ids = requirement.dependency_requirement_ids.clone();
    dependency_ids.sort();
    let dependency_fingerprint = canonical_json_fingerprint(
        &serde_json::to_value(dependency_ids).expect("dependency ids serialize"),
    );
    evaluations.iter().any(|evaluation| {
        evaluation.requirement_id == requirement.id
            && evaluation.work_id == requirement.work_id
            && evaluation.work_revision == requirement.work_revision
            && evaluation.work_report_id == requirement.work_report_id
            && evaluation.candidate_fingerprint == requirement.candidate_fingerprint
            && evaluation.config_fingerprint == requirement.config_fingerprint
            && evaluation.evaluator_version == requirement.evaluator_version
            && evaluation.dependency_fingerprint == dependency_fingerprint
            && evaluation.verdict == GateVerdict::Passed
    }) || waivers.iter().any(|waiver| {
        waiver.requirement_id == requirement.id
            && waiver.work_id == requirement.work_id
            && waiver.work_revision == requirement.work_revision
            && waiver.candidate_fingerprint == requirement.candidate_fingerprint
            && waiver.state == GateWaiverState::Active
    })
}

impl HarnessStore {
    fn trust_team_work_unlocked(
        &self,
        team_id: &str,
        work_id: &str,
        work_revision: u64,
    ) -> StoreResult<Work> {
        let work = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "Work not found in the selected Execution Space",
                    "work",
                    work_id,
                    None,
                )
            })?;
        if work.team_id.as_deref() != Some(team_id) || work.version != work_revision {
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "Team-scoped Work authority or exact Work revision does not match",
                "work",
                work_id,
                Some(work.version),
            ));
        }
        Ok(work)
    }

    fn trust_operation_envelopes_unlocked(&self) -> StoreResult<Vec<TrustOperationEnvelope>> {
        self.read_jsonl(TRUST_OPERATIONS_LEDGER)
    }

    pub fn canonical_operations(&self) -> StoreResult<Vec<CanonicalOperation>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .map(|envelope| envelope.operation)
            .collect())
    }

    pub(crate) fn trust_work_projections_unlocked(&self) -> StoreResult<Vec<Work>> {
        let mut works = Vec::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.operation.event.aggregate_kind == "work" {
                works.push(event_projection::<Work>(&envelope)?);
            }
            for record in envelope.operation.immutable_side_records {
                if let Ok(work) = serde_json::from_value::<Work>(record) {
                    works.push(work);
                }
            }
        }
        Ok(works)
    }

    fn latest_trust_envelopes_unlocked(
        &self,
        execution_space_id: &str,
        aggregate_kind: &str,
    ) -> StoreResult<BTreeMap<String, TrustOperationEnvelope>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.execution_space_id == execution_space_id
                && envelope.operation.event.aggregate_kind == aggregate_kind
            {
                latest.insert(envelope.operation.event.aggregate_id.clone(), envelope);
            }
        }
        Ok(latest)
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_trust_projection_unlocked<T: Serialize + for<'de> Deserialize<'de> + Clone>(
        &self,
        context: &MutationContext,
        aggregate_kind: &str,
        aggregate_id: &str,
        transition: &str,
        request_payload: Value,
        resulting_projection: &T,
        immutable_side_records: Vec<Value>,
        initial_outbox_records: Vec<Value>,
    ) -> StoreResult<CanonicalMutationResult<T>> {
        required(&context.execution_space_id, "execution_space_id")?;
        required(&context.authenticated_actor.id, "authenticated_actor.id")?;
        required(&context.command_name, "command_name")?;
        required(&context.idempotency_key, "idempotency_key")?;
        required(aggregate_kind, "aggregate_kind")?;
        required(aggregate_id, "aggregate_id")?;
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = canonical_json_fingerprint(&request_payload);

        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotency key was already used with a different canonical payload",
                    aggregate_kind,
                    aggregate_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            if replay.operation.event.aggregate_kind != aggregate_kind
                || replay.operation.event.aggregate_id != aggregate_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotent replay changed aggregate identity",
                    aggregate_kind,
                    aggregate_id,
                    None,
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
        }

        let latest = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == aggregate_kind
                    && envelope.operation.event.aggregate_id == aggregate_id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence);
        let current_version = latest
            .map(|envelope| envelope.operation.event.resulting_version)
            .unwrap_or(0);
        if context.expected_version != current_version {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                format!(
                    "expected version {}, current version is {current_version}",
                    context.expected_version
                ),
                aggregate_kind,
                aggregate_id,
                Some(current_version),
            ));
        }
        let store_sequence = existing
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let resulting_version = current_version + 1;
        let event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: aggregate_kind.to_string(),
            aggregate_id: aggregate_id.to_string(),
            sequence: latest
                .map(|envelope| envelope.operation.event.sequence)
                .unwrap_or(0)
                + 1,
            store_sequence,
            transition: transition.to_string(),
            expected_version: current_version,
            resulting_version,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        let operation = CanonicalOperation {
            event: event.clone(),
            resulting_projection: serde_json::to_value(resulting_projection)?,
            immutable_side_records,
            initial_outbox_records,
        };
        self.append_jsonl_unlocked(
            TRUST_OPERATIONS_LEDGER,
            &TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: context.command_name.clone(),
                operation,
            },
        )?;
        Ok(CanonicalMutationResult {
            projection: resulting_projection.clone(),
            event,
            replayed: false,
        })
    }

    fn commit_trust_work_acceptance_unlocked(
        &self,
        context: &MutationContext,
        request_payload: Value,
        work: &Work,
        immutable_side_records: Vec<Value>,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        let existing = self.trust_operation_envelopes_unlocked()?;
        let fingerprint = canonical_json_fingerprint(&request_payload);
        if let Some(replay) = existing.iter().find(|envelope| {
            envelope.execution_space_id == context.execution_space_id
                && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                && envelope.authenticated_actor_id == context.authenticated_actor.id
                && envelope.command_name == context.command_name
                && envelope.operation.event.idempotency_key == context.idempotency_key
        }) {
            if replay.operation.event.canonical_request_fingerprint != fingerprint
                || replay.operation.event.aggregate_kind != "work"
                || replay.operation.event.aggregate_id != work.id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotency key was already used for a different Work acceptance",
                    "work",
                    &work.id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(replay)?,
                event: replay.operation.event.clone(),
                replayed: true,
            });
        }
        let previous = existing
            .iter()
            .filter(|envelope| {
                envelope.execution_space_id == context.execution_space_id
                    && envelope.operation.event.aggregate_kind == "work"
                    && envelope.operation.event.aggregate_id == work.id
            })
            .max_by_key(|envelope| envelope.operation.event.sequence);
        let store_sequence = existing
            .iter()
            .map(|envelope| envelope.operation.event.store_sequence)
            .max()
            .unwrap_or(0)
            + 1;
        let event = CanonicalMutationEvent {
            id: format!("trust-event-{store_sequence}"),
            aggregate_kind: "work".into(),
            aggregate_id: work.id.clone(),
            sequence: previous
                .map(|envelope| envelope.operation.event.sequence)
                .unwrap_or(0)
                + 1,
            store_sequence,
            transition: "accepted".into(),
            expected_version: context.expected_version,
            resulting_version: work.version,
            performed_by_actor: context.authenticated_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            canonical_request_fingerprint: fingerprint,
            payload: request_payload,
            created_at: now_string(),
        };
        let operation = CanonicalOperation {
            event: event.clone(),
            resulting_projection: serde_json::to_value(work)?,
            immutable_side_records,
            initial_outbox_records: Vec::new(),
        };
        self.append_jsonl_unlocked(
            TRUST_OPERATIONS_LEDGER,
            &TrustOperationEnvelope {
                execution_space_id: context.execution_space_id.clone(),
                authenticated_actor_kind: context.authenticated_actor.kind,
                authenticated_actor_id: context.authenticated_actor.id.clone(),
                command_name: context.command_name.clone(),
                operation,
            },
        )?;
        Ok(CanonicalMutationResult {
            projection: work.clone(),
            event,
            replayed: false,
        })
    }

    pub fn trust_agent_members(&self, execution_space_id: &str) -> StoreResult<Vec<AgentMember>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "agent_member")?
            .values()
            .map(event_projection)
            .collect()
    }

    /// Company/read-model projection only. One HarnessStore is one Execution
    /// Space in normal operation; this fold exists for callers that were given
    /// only the physical store and must not resurrect a second identity ledger.
    pub fn all_trust_agent_members(&self) -> StoreResult<Vec<AgentMember>> {
        let mut latest = BTreeMap::new();
        for envelope in self.trust_operation_envelopes_unlocked()? {
            if envelope.operation.event.aggregate_kind == "agent_member" {
                latest.insert(
                    (
                        envelope.execution_space_id.clone(),
                        envelope.operation.event.aggregate_id.clone(),
                    ),
                    envelope,
                );
            }
        }
        latest.values().map(event_projection).collect()
    }

    pub fn create_trust_agent_member(
        &self,
        context: &MutationContext,
        mut member: AgentMember,
    ) -> StoreResult<CanonicalMutationResult<AgentMember>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(&member.id, "AgentMember.id")?;
        required(&member.name, "AgentMember.name")?;
        required(&member.role, "AgentMember.role")?;
        required(&member.workspace_policy, "AgentMember.workspace_policy")?;
        if member.version != 1 || context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "AgentMember create requires absent CAS and version 1",
                "agent_member",
                &member.id,
                Some(0),
            ));
        }
        if member.created_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "created_by must equal the authenticated actor",
                "agent_member",
                &member.id,
                None,
            ));
        }
        member.updated_at = member.created_at.clone();
        let payload = serde_json::to_value(&member)?;
        self.commit_trust_projection_unlocked(
            context,
            "agent_member",
            &member.id,
            "created",
            payload,
            &member,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn transition_trust_agent_member(
        &self,
        context: &MutationContext,
        member_id: &str,
        next_status: AgentMemberOrganizationStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<AgentMember>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut current = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "agent_member")?
            .remove(member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "AgentMember not found",
                    "agent_member",
                    member_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<AgentMember>(&envelope))?;
        let allowed = matches!(
            (current.organization_status, next_status),
            (
                AgentMemberOrganizationStatus::Active,
                AgentMemberOrganizationStatus::Paused
            ) | (
                AgentMemberOrganizationStatus::Paused,
                AgentMemberOrganizationStatus::Active
            ) | (
                AgentMemberOrganizationStatus::Active,
                AgentMemberOrganizationStatus::Retired
            ) | (
                AgentMemberOrganizationStatus::Paused,
                AgentMemberOrganizationStatus::Retired
            )
        );
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "AgentMember transition is not allowed",
                "agent_member",
                member_id,
                Some(current.version),
            ));
        }
        current.organization_status = next_status;
        current.version += 1;
        current.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "agent_member",
            member_id,
            match next_status {
                AgentMemberOrganizationStatus::Active => "resumed",
                AgentMemberOrganizationStatus::Paused => "paused",
                AgentMemberOrganizationStatus::Retired => "retired",
            },
            serde_json::json!({"status": next_status, "updated_at": updated_at}),
            &current,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_member_runs(&self, execution_space_id: &str) -> StoreResult<Vec<MemberRun>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "member_run")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_member_run_scope(&self, member_run_id: &str) -> StoreResult<Option<String>> {
        Ok(self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .rev()
            .find(|envelope| {
                envelope.operation.event.aggregate_kind == "member_run"
                    && envelope.operation.event.aggregate_id == member_run_id
            })
            .map(|envelope| envelope.execution_space_id))
    }

    pub fn create_trust_member_run(
        &self,
        context: &MutationContext,
        run: MemberRun,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(&run.id, "MemberRun.id")?;
        required(&run.agent_member_id, "MemberRun.agent_member_id")?;
        required(&run.team_run_id, "MemberRun.team_run_id")?;
        if run.version != 1 || run.runtime_generation != 1 || context.expected_version != 0 {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "MemberRun create requires absent CAS, version 1 and generation 1",
                "member_run",
                &run.id,
                Some(0),
            ));
        }
        let member = self
            .trust_agent_members(&context.execution_space_id)?
            .into_iter()
            .find(|member| member.id == run.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun references a missing AgentMember",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        match member.organization_status {
            AgentMemberOrganizationStatus::Active => {}
            AgentMemberOrganizationStatus::Paused => {
                return Err(trust_error(
                    TrustErrorCode::AgentMemberPaused,
                    "paused AgentMember cannot start a MemberRun",
                    "agent_member",
                    &member.id,
                    Some(member.version),
                ))
            }
            AgentMemberOrganizationStatus::Retired => {
                return Err(trust_error(
                    TrustErrorCode::AgentMemberRetired,
                    "retired AgentMember cannot start a MemberRun",
                    "agent_member",
                    &member.id,
                    Some(member.version),
                ))
            }
        }
        let team_run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|candidate| candidate.id == run.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun references a missing TeamRun",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        let team = self
            .latest_teams()?
            .remove(&team_run.agent_team_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "TeamRun references a missing AgentTeam",
                    "member_run",
                    &run.id,
                    None,
                )
            })?;
        if team.host_agent_id != run.agent_member_id
            && !team.member_ids.contains(&run.agent_member_id)
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentMember does not belong to the Team",
                "member_run",
                &run.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "member_run",
            &run.id,
            "created",
            serde_json::to_value(&run)?,
            &run,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn transition_trust_member_run(
        &self,
        context: &MutationContext,
        member_run_id: &str,
        next: MemberCoordinationStatus,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut run = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "member_run")?
            .remove(member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun not found",
                    "member_run",
                    member_run_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<MemberRun>(&envelope))?;
        let transition = match (run.coordination_status, next) {
            (MemberCoordinationStatus::Active, MemberCoordinationStatus::Closed) => "closed",
            (MemberCoordinationStatus::Closed, MemberCoordinationStatus::Active) => "reopened",
            (MemberCoordinationStatus::Active, MemberCoordinationStatus::Retired)
            | (MemberCoordinationStatus::Closed, MemberCoordinationStatus::Retired) => "retired",
            _ => {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun coordination transition is not allowed",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ))
            }
        };
        if transition == "reopened" {
            let session = run.native_session.as_ref().ok_or_else(|| {
                trust_error(
                    TrustErrorCode::NativeSessionMissing,
                    "reopen requires a resumable NativeSessionRef",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                )
            })?;
            if !session.supports_resume
                || !matches!(
                    session.availability,
                    firm_core::agentfirm_api::NativeSessionAvailability::Available
                        | firm_core::agentfirm_api::NativeSessionAvailability::Stale
                )
            {
                return Err(trust_error(
                    TrustErrorCode::NativeSessionIncompatible,
                    "NativeSessionRef is not safely resumable",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ));
            }
            run.runtime_generation += 1;
            run.runtime_status = MemberRuntimeStatus::Idle;
        } else if transition == "closed" {
            run.runtime_status = MemberRuntimeStatus::Stopped;
        } else {
            run.runtime_status = MemberRuntimeStatus::Stopped;
            run.finished_at = Some(updated_at.to_string());
        }
        run.coordination_status = next;
        run.version += 1;
        run.last_event_at = Some(updated_at.to_string());

        let mut side_records = Vec::new();
        if transition == "reopened" {
            if let Some(binding_id) = run.workspace_binding_id.as_deref() {
                let mut binding = self
                    .trust_workspace_bindings(&context.execution_space_id)?
                    .into_iter()
                    .find(|binding| binding.id == binding_id)
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::WorkspacePathUnsafe,
                            "reopen requires the bound workspace projection",
                            "workspace_binding",
                            binding_id,
                            None,
                        )
                    })?;
                if matches!(
                    binding.lifecycle,
                    WorkspaceLifecycle::Missing
                        | WorkspaceLifecycle::CleanupBlocked
                        | WorkspaceLifecycle::Removed
                ) {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceCleanupBlocked,
                        "reopen cannot reattach an unavailable workspace",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
                let observed = observe_workspace_safety(Path::new(&binding.canonical_root))?;
                if !observed.link_escape_free {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceLinkEscape,
                        "reopen workspace contains a symbolic-link escape",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
                if observed.conflicted || observed.dirty {
                    return Err(trust_error(
                        if observed.conflicted {
                            TrustErrorCode::WorkspaceConflicted
                        } else {
                            TrustErrorCode::WorkspaceDirty
                        },
                        "reopen requires a clean conflict-free workspace",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
                binding.lifecycle = WorkspaceLifecycle::Attached;
                binding.attached_member_generation = Some(run.runtime_generation);
                binding.dirty_fingerprint = None;
                binding.blocked_reason = None;
                binding.version += 1;
                binding.updated_at = updated_at.to_string();
                side_records.push(serde_json::to_value(binding)?);
            }
        }
        if transition == "closed" || transition == "retired" {
            for mut delivery in self.trust_message_deliveries(&context.execution_space_id)? {
                if delivery.recipient_member_run_id != member_run_id {
                    continue;
                }
                if transition == "closed" && delivery.status == MessageDeliveryStatus::Queued {
                    delivery.freeze_generation = Some(run.runtime_generation);
                    delivery.version += 1;
                    delivery.updated_at = updated_at.to_string();
                    side_records.push(serde_json::to_value(delivery)?);
                } else if transition == "retired"
                    && matches!(
                        delivery.status,
                        MessageDeliveryStatus::Queued | MessageDeliveryStatus::Claimed
                    )
                {
                    delivery.status = MessageDeliveryStatus::Invalidated;
                    delivery.version += 1;
                    delivery.updated_at = updated_at.to_string();
                    side_records.push(serde_json::to_value(delivery)?);
                }
            }
            for mut delivery in self.trust_work_deliveries(&context.execution_space_id)? {
                if delivery.recipient_member_run_id != member_run_id {
                    continue;
                }
                if transition == "closed" && delivery.status == WorkDeliveryStatus::Queued {
                    delivery.freeze_generation = Some(run.runtime_generation);
                    delivery.version += 1;
                    delivery.updated_at = updated_at.to_string();
                    side_records.push(serde_json::to_value(delivery)?);
                } else if transition == "retired"
                    && matches!(
                        delivery.status,
                        WorkDeliveryStatus::Queued | WorkDeliveryStatus::Claimed
                    )
                {
                    delivery.status = WorkDeliveryStatus::Invalidated;
                    delivery.version += 1;
                    delivery.updated_at = updated_at.to_string();
                    side_records.push(serde_json::to_value(delivery)?);
                }
            }
        }
        self.commit_trust_projection_unlocked(
            context,
            "member_run",
            member_run_id,
            transition,
            serde_json::json!({"coordination_status": next, "updated_at": updated_at}),
            &run,
            side_records,
            Vec::new(),
        )
    }

    pub fn resume_trust_native_session(
        &self,
        context: &MutationContext,
        member_run_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberRun>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut run = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "member_run")?
            .remove(member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun not found",
                    "member_run",
                    member_run_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<MemberRun>(&envelope))?;
        self.claimable_member_run(
            &context.execution_space_id,
            member_run_id,
            run.runtime_generation,
        )?;
        let session = run.native_session.as_ref().ok_or_else(|| {
            trust_error(
                TrustErrorCode::NativeSessionMissing,
                "resume-native-session requires NativeSessionRef",
                "member_run",
                member_run_id,
                Some(run.version),
            )
        })?;
        if !session.supports_resume
            || !matches!(
                session.availability,
                firm_core::agentfirm_api::NativeSessionAvailability::Available
                    | firm_core::agentfirm_api::NativeSessionAvailability::Stale
            )
        {
            return Err(trust_error(
                TrustErrorCode::NativeSessionIncompatible,
                "NativeSessionRef is not safely resumable",
                "member_run",
                member_run_id,
                Some(run.version),
            ));
        }
        run.runtime_status = MemberRuntimeStatus::Starting;
        run.version += 1;
        run.last_event_at = Some(updated_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "member_run",
            member_run_id,
            "native_session_resume_requested",
            serde_json::json!({"updated_at": updated_at}),
            &run,
            Vec::new(),
            Vec::new(),
        )
    }

    fn trust_side_records<T: for<'de> Deserialize<'de>>(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<T>> {
        let mut rows = Vec::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            for value in envelope
                .operation
                .initial_outbox_records
                .into_iter()
                .chain(envelope.operation.immutable_side_records)
            {
                if let Ok(row) = serde_json::from_value::<T>(value) {
                    rows.push(row);
                }
            }
        }
        Ok(rows)
    }

    fn trust_gate_requirements_unlocked(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<BTreeMap<String, GateRequirement>> {
        let mut latest = BTreeMap::new();
        for envelope in self
            .trust_operation_envelopes_unlocked()?
            .into_iter()
            .filter(|envelope| envelope.execution_space_id == execution_space_id)
        {
            for value in &envelope.operation.immutable_side_records {
                if let Ok(requirement) = serde_json::from_value::<GateRequirement>(value.clone()) {
                    latest.insert(requirement.id.clone(), requirement);
                }
            }
            if envelope.operation.event.aggregate_kind == "gate_requirement" {
                let requirement = event_projection::<GateRequirement>(&envelope)?;
                latest.insert(requirement.id.clone(), requirement);
            }
        }
        Ok(latest)
    }

    pub fn trust_message_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MessageDelivery>> {
        let mut latest = BTreeMap::new();
        for delivery in self.trust_side_records::<MessageDelivery>(execution_space_id)? {
            latest.insert(delivery.id.clone(), delivery);
        }
        Ok(latest.into_values().collect())
    }

    pub fn trust_team_messages(&self, execution_space_id: &str) -> StoreResult<Vec<TeamMessage>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "team_message")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_work_deliveries(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<WorkDelivery>> {
        let mut latest = BTreeMap::new();
        for delivery in self.trust_side_records::<WorkDelivery>(execution_space_id)? {
            latest.insert(delivery.id.clone(), delivery);
        }
        Ok(latest.into_values().collect())
    }

    pub fn create_trust_team_message_with_deliveries(
        &self,
        context: &MutationContext,
        message: TeamMessage,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<TeamMessage>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(&message.id, "TeamMessage.id")?;
        required(&message.team_run_id, "TeamMessage.team_run_id")?;
        required(&message.body, "TeamMessage.body")?;
        required(&message.correlation_id, "TeamMessage.correlation_id")?;
        if message.sender != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "message sender must equal authenticated actor",
                "team_message",
                &message.id,
                None,
            ));
        }
        if message.recipients.is_empty() {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "message requires at least one recipient",
                "team_message",
                &message.id,
                None,
            ));
        }
        if !self
            .team_runs()?
            .into_iter()
            .any(|run| run.id == message.team_run_id)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "message references a missing TeamRun",
                "team_message",
                &message.id,
                None,
            ));
        }
        let runs = self.trust_member_runs(&context.execution_space_id)?;
        if message.sender.kind == ActorKind::AgentMember
            && !runs.iter().any(|run| {
                run.team_run_id == message.team_run_id
                    && run.agent_member_id == message.sender.id
                    && run.coordination_status == MemberCoordinationStatus::Active
            })
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "AgentMember sender has no active MemberRun in the TeamRun",
                "team_message",
                &message.id,
                None,
            ));
        }
        let mut seen = BTreeSet::new();
        let mut deliveries = Vec::new();
        for recipient in &message.recipients {
            if recipient.kind != ActorKind::AgentMember || !seen.insert(recipient.id.clone()) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "message recipients must be unique AgentMember references",
                    "team_message",
                    &message.id,
                    None,
                ));
            }
            let matching = runs
                .iter()
                .filter(|run| {
                    run.team_run_id == message.team_run_id
                        && run.agent_member_id == recipient.id
                        && run.coordination_status != MemberCoordinationStatus::Retired
                })
                .collect::<Vec<_>>();
            if matching.len() != 1 {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "recipient must resolve to exactly one non-retired MemberRun in the TeamRun",
                    "team_message",
                    &message.id,
                    None,
                ));
            }
            let run = matching[0];
            deliveries.push(MessageDelivery {
                id: format!("{}:{}", message.id, run.id),
                message_id: message.id.clone(),
                recipient_member_run_id: run.id.clone(),
                status: MessageDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_supervisor_generation: None,
                claimed_member_generation: None,
                claim_expires_at: None,
                freeze_generation: (run.coordination_status == MemberCoordinationStatus::Closed)
                    .then_some(run.runtime_generation),
                provider_receipt_id: None,
                failure_code: None,
                failure_detail: None,
                version: 1,
                updated_at: updated_at.to_string(),
            });
        }
        self.commit_trust_projection_unlocked(
            context,
            "team_message",
            &message.id,
            "created",
            serde_json::to_value(&message)?,
            &message,
            Vec::new(),
            deliveries
                .into_iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
        )
    }

    pub fn create_trust_work_deliveries(
        &self,
        context: &MutationContext,
        work_event_id: &str,
        work_id: &str,
        work_revision: u64,
        recipient_member_run_ids: &[String],
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Vec<WorkDelivery>>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(work_event_id, "work_event_id")?;
        required(work_id, "work_id")?;
        if recipient_member_run_ids.is_empty() {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "WorkEvent requires at least one delivery recipient",
                "work_event",
                work_event_id,
                None,
            ));
        }
        let runs = self.trust_member_runs(&context.execution_space_id)?;
        let mut unique = BTreeSet::new();
        let mut deliveries = Vec::new();
        for run_id in recipient_member_run_ids {
            if !unique.insert(run_id.clone()) {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery recipients must be unique",
                    "work_event",
                    work_event_id,
                    None,
                ));
            }
            let run = runs.iter().find(|run| run.id == *run_id).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery recipient MemberRun does not exist",
                    "work_event",
                    work_event_id,
                    None,
                )
            })?;
            match run.coordination_status {
                MemberCoordinationStatus::Active | MemberCoordinationStatus::Closed => {}
                MemberCoordinationStatus::Retired => {
                    return Err(trust_error(
                        TrustErrorCode::MemberRunRetired,
                        "retired MemberRun rejects new WorkDelivery",
                        "member_run",
                        run_id,
                        Some(run.version),
                    ))
                }
            }
            deliveries.push(WorkDelivery {
                id: format!("{work_event_id}:{run_id}"),
                work_event_id: work_event_id.to_string(),
                work_id: work_id.to_string(),
                work_revision,
                recipient_member_run_id: run_id.clone(),
                status: WorkDeliveryStatus::Queued,
                attempt: 1,
                claim_id: None,
                claimed_supervisor_generation: None,
                claimed_member_generation: None,
                claim_expires_at: None,
                freeze_generation: (run.coordination_status == MemberCoordinationStatus::Closed)
                    .then_some(run.runtime_generation),
                provider_receipt_id: None,
                failure_code: None,
                failure_detail: None,
                version: 1,
                updated_at: updated_at.to_string(),
            });
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_event_delivery_batch",
            work_event_id,
            "deliveries_created",
            serde_json::json!({
                "work_event_id": work_event_id,
                "work_id": work_id,
                "work_revision": work_revision,
                "recipients": recipient_member_run_ids,
            }),
            &deliveries,
            Vec::new(),
            deliveries
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<_, _>>()?,
        )
    }

    fn claimable_member_run(
        &self,
        execution_space_id: &str,
        member_run_id: &str,
        member_generation: u64,
    ) -> StoreResult<MemberRun> {
        let run = self
            .trust_member_runs(execution_space_id)?
            .into_iter()
            .find(|run| run.id == member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "delivery references a missing MemberRun",
                    "member_run",
                    member_run_id,
                    None,
                )
            })?;
        match run.coordination_status {
            MemberCoordinationStatus::Closed => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunClosed,
                    "closed MemberRun cannot claim delivery",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ))
            }
            MemberCoordinationStatus::Retired => {
                return Err(trust_error(
                    TrustErrorCode::MemberRunRetired,
                    "retired MemberRun cannot claim delivery",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ))
            }
            MemberCoordinationStatus::Active => {}
        }
        if run.runtime_generation != member_generation {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "delivery claim used a stale MemberRun generation",
                "member_run",
                member_run_id,
                Some(run.version),
            ));
        }
        let member = self
            .trust_agent_members(execution_space_id)?
            .into_iter()
            .find(|member| member.id == run.agent_member_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MemberRun AgentMember is missing",
                    "agent_member",
                    &run.agent_member_id,
                    None,
                )
            })?;
        match member.organization_status {
            AgentMemberOrganizationStatus::Active => Ok(run),
            AgentMemberOrganizationStatus::Paused => Err(trust_error(
                TrustErrorCode::AgentMemberPaused,
                "paused AgentMember cannot claim delivery",
                "agent_member",
                &member.id,
                Some(member.version),
            )),
            AgentMemberOrganizationStatus::Retired => Err(trust_error(
                TrustErrorCode::AgentMemberRetired,
                "retired AgentMember cannot claim delivery",
                "agent_member",
                &member.id,
                Some(member.version),
            )),
        }
    }

    pub fn claim_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: DeliveryClaim,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Queued {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "only queued MessageDelivery may be claimed",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let run = self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            claim.member_generation,
        )?;
        if delivery
            .freeze_generation
            .is_some_and(|generation| generation >= run.runtime_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "delivery remains frozen for the closed generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim.claim_id.clone());
        delivery.claimed_supervisor_generation = Some(claim.supervisor_generation);
        delivery.claimed_member_generation = Some(claim.member_generation);
        delivery.claim_expires_at = Some(claim.claim_expires_at.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            "claimed",
            serde_json::to_value(&claim)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn receive_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        receipt: ProviderReceipt,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(receipt.claim_id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "provider receipt does not match the active claim",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            receipt.member_generation,
        )?;
        if delivery.claimed_supervisor_generation != Some(receipt.supervisor_generation)
            || delivery.claimed_member_generation != Some(receipt.member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt used a stale supervisor or member generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(receipt.provider_receipt_id.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            "provider_received",
            serde_json::to_value(&receipt)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn acknowledge_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim_id: &str,
        member_generation: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::ProviderReceived
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.provider_receipt_id.is_none()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryReceiptMissing,
                "acknowledgement requires the exact claim and provider receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            member_generation,
        )?;
        if delivery.claimed_member_generation != Some(member_generation) {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "ack used a stale member generation",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Acknowledged;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            "acknowledged",
            serde_json::json!({"claim_id": claim_id, "member_generation": member_generation}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn reconcile_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        outcome: DeliveryReconcileOutcome,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(evidence_ref, "evidence_ref")?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Claimed
            || delivery.provider_receipt_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "reconcile applies only to an uncertain claimed delivery without receipt",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let transition = match outcome {
            DeliveryReconcileOutcome::Acknowledged => {
                delivery.status = MessageDeliveryStatus::Acknowledged;
                "reconciled_acknowledged"
            }
            DeliveryReconcileOutcome::RetrySafeFailure => {
                delivery.status = MessageDeliveryStatus::Failed;
                delivery.failure_code = Some("RECONCILED_RETRY_SAFE".into());
                delivery.failure_detail = Some(evidence_ref.to_string());
                "reconciled_retry_safe_failure"
            }
        };
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            transition,
            serde_json::json!({"outcome": outcome, "evidence_ref": evidence_ref}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn retry_trust_message_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MessageDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_message_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "MessageDelivery not found",
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != MessageDeliveryStatus::Failed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "only failed MessageDelivery can be retried",
                "message_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = MessageDeliveryStatus::Queued;
        delivery.attempt += 1;
        delivery.claim_id = None;
        delivery.claimed_supervisor_generation = None;
        delivery.claimed_member_generation = None;
        delivery.claim_expires_at = None;
        delivery.provider_receipt_id = None;
        delivery.failure_code = None;
        delivery.failure_detail = None;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "message_delivery",
            delivery_id,
            "retried",
            serde_json::json!({"attempt": delivery.attempt}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn claim_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        claim: DeliveryClaim,
        current_work_revision: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.work_revision != current_work_revision {
            delivery.status = WorkDeliveryStatus::Invalidated;
            delivery.failure_code = Some("WORK_REVISION_STALE".into());
            delivery.version += 1;
            delivery.updated_at = updated_at.to_string();
            let _ = self.commit_trust_projection_unlocked(
                context,
                "work_delivery",
                delivery_id,
                "invalidated_stale_revision",
                serde_json::json!({"current_work_revision": current_work_revision}),
                &delivery,
                vec![serde_json::to_value(&delivery)?],
                Vec::new(),
            )?;
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "WorkDelivery revision is stale and was invalidated",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        if delivery.status != WorkDeliveryStatus::Queued {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "only queued WorkDelivery may be claimed",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        let run = self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            claim.member_generation,
        )?;
        if delivery
            .freeze_generation
            .is_some_and(|generation| generation >= run.runtime_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "WorkDelivery remains frozen for the closed generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Claimed;
        delivery.claim_id = Some(claim.claim_id.clone());
        delivery.claimed_supervisor_generation = Some(claim.supervisor_generation);
        delivery.claimed_member_generation = Some(claim.member_generation);
        delivery.claim_expires_at = Some(claim.claim_expires_at.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery",
            delivery_id,
            "claimed",
            serde_json::to_value(&claim)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn receive_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        receipt: ProviderReceipt,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(receipt.claim_id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryClaimConflict,
                "provider receipt does not match the active WorkDelivery claim",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        self.claimable_member_run(
            &context.execution_space_id,
            &delivery.recipient_member_run_id,
            receipt.member_generation,
        )?;
        if delivery.claimed_supervisor_generation != Some(receipt.supervisor_generation)
            || delivery.claimed_member_generation != Some(receipt.member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::MemberRunGenerationFenced,
                "provider receipt used a stale generation",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(receipt.provider_receipt_id.clone());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery",
            delivery_id,
            "provider_received",
            serde_json::to_value(&receipt)?,
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn reconcile_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        evidence_ref: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(evidence_ref, "evidence_ref")?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Claimed || delivery.provider_receipt_id.is_some()
        {
            return Err(trust_error(
                TrustErrorCode::DeliveryRecoveryUncertain,
                "reconcile applies only to an uncertain claimed WorkDelivery",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Failed;
        delivery.failure_code = Some("RECONCILED_RETRY_SAFE".into());
        delivery.failure_detail = Some(evidence_ref.to_string());
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "work_delivery",
            delivery_id,
            "reconciled_retry_safe_failure",
            serde_json::json!({"evidence_ref": evidence_ref}),
            &delivery,
            vec![serde_json::to_value(&delivery)?],
            Vec::new(),
        )
    }

    pub fn retry_trust_work_delivery(
        &self,
        context: &MutationContext,
        delivery_id: &str,
        current_work_revision: u64,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<WorkDelivery>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut delivery = self
            .trust_work_deliveries(&context.execution_space_id)?
            .into_iter()
            .find(|delivery| delivery.id == delivery_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "WorkDelivery not found",
                    "work_delivery",
                    delivery_id,
                    None,
                )
            })?;
        if delivery.status != WorkDeliveryStatus::Failed
            || delivery.work_revision != current_work_revision
        {
            return Err(trust_error(
                TrustErrorCode::WorkRevisionStale,
                "WorkDelivery retry requires failed status and exact current Work revision",
                "work_delivery",
                delivery_id,
                Some(delivery.version),
            ));
        }
        delivery.status = WorkDeliveryStatus::Queued;
        delivery.attempt += 1;
        delivery.claim_id = None;
        delivery.claimed_supervisor_generation = None;
        delivery.claimed_member_generation = None;
        delivery.claim_expires_at = None;
        delivery.provider_receipt_id = None;
        delivery.failure_code = None;
        delivery.failure_detail = None;
        delivery.version += 1;
        delivery.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(context, "work_delivery", delivery_id, "retried", serde_json::json!({"attempt": delivery.attempt, "work_revision": current_work_revision}), &delivery, vec![serde_json::to_value(&delivery)?], Vec::new())
    }

    pub fn create_trust_work_report(
        &self,
        context: &MutationContext,
        team_id: &str,
        report: WorkReport,
    ) -> StoreResult<CanonicalMutationResult<WorkReport>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let source_work_revision = if report.kind == WorkReportKind::Result {
            report.work_revision.checked_sub(1).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "result report must name the resulting non-zero Work revision",
                    "work_report",
                    &report.id,
                    None,
                )
            })?
        } else {
            report.work_revision
        };
        let current_work =
            self.trust_team_work_unlocked(team_id, &report.work_id, source_work_revision)?;
        if report.authored_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkReport.authored_by must equal the authenticated actor",
                "work_report",
                &report.id,
                None,
            ));
        }
        if report.kind == WorkReportKind::Result
            && (report.candidate.is_none()
                || report
                    .candidate_fingerprint
                    .as_deref()
                    .unwrap_or("")
                    .is_empty()
                || report.evidence_refs.is_empty())
        {
            return Err(trust_error(
                TrustErrorCode::ReportEvidenceMissing,
                "result report requires exact CandidateRef, fingerprint and evidence",
                "work_report",
                &report.id,
                None,
            ));
        }
        if report.kind == WorkReportKind::Result {
            if current_work.phase != firm_core::WorkPhase::Active
                || current_work.condition != firm_core::WorkCondition::Normal
                || report.work_revision != current_work.version + 1
            {
                return Err(trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "result report may submit only normal active Work and must name the resulting Work revision",
                    "work_report",
                    &report.id,
                    Some(current_work.version),
                ));
            }
            if current_work.owner_member_id.as_deref()
                != Some(context.authenticated_actor.id.as_str())
            {
                return Err(trust_error(
                    TrustErrorCode::UnauthorizedActor,
                    "only the accountable AgentMember may submit a result report",
                    "work_report",
                    &report.id,
                    Some(current_work.version),
                ));
            }
        }
        if let (Some(candidate), Some(fingerprint)) = (
            report.candidate.as_ref(),
            report.candidate_fingerprint.as_ref(),
        ) {
            let expected = canonical_json_fingerprint(&serde_json::to_value(candidate)?);
            if fingerprint != &expected {
                return Err(trust_error(
                    TrustErrorCode::ReportEvidenceMissing,
                    "candidate_fingerprint does not match canonical CandidateRef",
                    "work_report",
                    &report.id,
                    None,
                ));
            }
        }
        if report.kind == WorkReportKind::Failure && report.failure_analysis_ref.is_none() {
            return Err(trust_error(
                TrustErrorCode::FailureAnalysisMissing,
                "failure report requires FailureAnalysis",
                "work_report",
                &report.id,
                None,
            ));
        }
        if let Some(analysis_id) = report.failure_analysis_ref.as_deref() {
            let analysis = self
                .latest_trust_envelopes_unlocked(&context.execution_space_id, "failure_analysis")?
                .remove(analysis_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::FailureAnalysisMissing,
                        "failure report references a missing FailureAnalysis",
                        "work_report",
                        &report.id,
                        None,
                    )
                })
                .and_then(|envelope| event_projection::<FailureAnalysis>(&envelope))?;
            if analysis.work_id != report.work_id || analysis.work_revision != report.work_revision
            {
                return Err(trust_error(
                    TrustErrorCode::FailureAnalysisMissing,
                    "FailureAnalysis does not match the report Work revision",
                    "work_report",
                    &report.id,
                    None,
                ));
            }
        }
        let mut resolved_requirements = Vec::new();
        if report.kind == WorkReportKind::Result {
            let candidate_fingerprint = report
                .candidate_fingerprint
                .as_ref()
                .expect("result validation requires candidate fingerprint");
            let bindings = self
                .latest_trust_envelopes_unlocked(
                    &context.execution_space_id,
                    "work_module_binding",
                )?
                .into_values()
                .map(|envelope| event_projection::<WorkModuleBinding>(&envelope))
                .collect::<StoreResult<Vec<_>>>()?;
            for binding in bindings.into_iter().filter(|binding| {
                binding.work_id == report.work_id
                    && binding.work_revision == source_work_revision
                    && binding.module_id == "integration-plan"
                    && binding.module_version == 1
            }) {
                let definition = integration_plan_module_v1();
                for (index, template) in definition.default_gate_templates.iter().enumerate() {
                    let resolved_config = serde_json::json!({
                        "module_binding_id": binding.id,
                        "module_binding_version": binding.version,
                        "module_config_fingerprint": binding.config_fingerprint,
                        "template": template,
                    });
                    resolved_requirements.push(GateRequirement {
                        id: format!("gate:{}:{}:{index}", report.id, binding.id),
                        work_id: report.work_id.clone(),
                        work_revision: report.work_revision,
                        work_report_id: report.id.clone(),
                        candidate_fingerprint: candidate_fingerprint.clone(),
                        source: GateRequirementSource::Module,
                        source_binding_id: Some(binding.id.clone()),
                        gate_type: template
                            .get("gate_type")
                            .and_then(Value::as_str)
                            .unwrap_or("integration-plan-completeness")
                            .to_string(),
                        gate_contract_version: template
                            .get("gate_contract_version")
                            .and_then(Value::as_str)
                            .unwrap_or("1")
                            .to_string(),
                        evaluator_ref: definition.implementation_ref.clone(),
                        evaluator_version: definition.module_version.to_string(),
                        config_fingerprint: canonical_json_fingerprint(&resolved_config),
                        resolved_config,
                        required: template
                            .get("required")
                            .and_then(Value::as_bool)
                            .unwrap_or(true),
                        dependency_requirement_ids: Vec::new(),
                        requirement_set_fingerprint: String::new(),
                        created_at: report.created_at.clone(),
                        version: 1,
                    });
                }
            }
            let mut requirement_ids = resolved_requirements
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect::<Vec<_>>();
            requirement_ids.sort();
            let set_fingerprint =
                canonical_json_fingerprint(&serde_json::to_value(requirement_ids)?);
            for requirement in &mut resolved_requirements {
                requirement.requirement_set_fingerprint = set_fingerprint.clone();
            }
        }
        let mut side_records = resolved_requirements
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
        if report.kind == WorkReportKind::Result {
            let mut submitted_work = current_work;
            submitted_work.phase = firm_core::WorkPhase::Review;
            submitted_work.condition = firm_core::WorkCondition::Normal;
            submitted_work.version = report.work_revision;
            submitted_work.result_summary = Some(report.summary.clone());
            submitted_work.updated_at = report.created_at.clone();
            side_records.push(serde_json::to_value(submitted_work)?);
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_report",
            &report.id,
            "created",
            serde_json::to_value(&report)?,
            &report,
            side_records,
            Vec::new(),
        )
    }

    pub fn create_trust_finding(
        &self,
        context: &MutationContext,
        team_id: &str,
        finding: WorkFinding,
    ) -> StoreResult<CanonicalMutationResult<WorkFinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        self.trust_team_work_unlocked(team_id, &finding.work_id, finding.work_revision)?;
        self.commit_trust_projection_unlocked(
            context,
            "work_finding",
            &finding.id,
            "created",
            serde_json::to_value(&finding)?,
            &finding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_failure_analysis(
        &self,
        context: &MutationContext,
        team_id: &str,
        analysis: FailureAnalysis,
    ) -> StoreResult<CanonicalMutationResult<FailureAnalysis>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        self.trust_team_work_unlocked(team_id, &analysis.work_id, analysis.work_revision)?;
        self.commit_trust_projection_unlocked(
            context,
            "failure_analysis",
            &analysis.id,
            "created",
            serde_json::to_value(&analysis)?,
            &analysis,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn bind_trust_work_module(
        &self,
        context: &MutationContext,
        team_id: &str,
        binding: WorkModuleBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkModuleBinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        self.trust_team_work_unlocked(team_id, &binding.work_id, binding.work_revision)?;
        if binding.config_fingerprint != canonical_json_fingerprint(&binding.resolved_config) {
            return Err(trust_error(
                TrustErrorCode::ModuleConfigInvalid,
                "module config_fingerprint does not match resolved_config",
                "work_module_binding",
                &binding.id,
                None,
            ));
        }
        if binding.module_id == "integration-plan"
            && binding.module_version == 1
            && (!binding.resolved_config.is_object()
                || ![
                    "base_revision",
                    "target_revision",
                    "work_boundaries",
                    "candidate_boundaries",
                    "interfaces",
                    "convergence_points",
                    "merge_order",
                    "conflict_owner",
                    "per_merge_checks",
                    "combined_verification",
                    "rollback_plan",
                ]
                .into_iter()
                .all(|key| binding.resolved_config.get(key).is_some()))
        {
            return Err(trust_error(
                TrustErrorCode::ModuleConfigInvalid,
                "integration-plan@1 config is incomplete",
                "work_module_binding",
                &binding.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_module_binding",
            &binding.id,
            "attached",
            serde_json::to_value(&binding)?,
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_gate_requirement(
        &self,
        context: &MutationContext,
        team_id: &str,
        mut requirement: GateRequirement,
    ) -> StoreResult<CanonicalMutationResult<GateRequirement>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        self.trust_team_work_unlocked(team_id, &requirement.work_id, requirement.work_revision)?;
        let existing = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .into_values()
            .collect::<Vec<_>>();
        if existing.iter().any(|item| item.id == requirement.id) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "GateRequirement id already exists",
                "gate_requirement",
                &requirement.id,
                Some(1),
            ));
        }
        let mut graph = existing
            .iter()
            .map(|item| (item.id.clone(), item.dependency_requirement_ids.clone()))
            .collect::<BTreeMap<_, _>>();
        graph.insert(
            requirement.id.clone(),
            requirement.dependency_requirement_ids.clone(),
        );
        fn reaches(
            graph: &BTreeMap<String, Vec<String>>,
            current: &str,
            target: &str,
            seen: &mut BTreeSet<String>,
        ) -> bool {
            if current == target {
                return true;
            }
            if !seen.insert(current.to_string()) {
                return false;
            }
            graph
                .get(current)
                .into_iter()
                .flatten()
                .any(|next| reaches(graph, next, target, seen))
        }
        if requirement
            .dependency_requirement_ids
            .iter()
            .any(|dependency| reaches(&graph, dependency, &requirement.id, &mut BTreeSet::new()))
        {
            return Err(trust_error(
                TrustErrorCode::GateDependencyCycle,
                "gate requirement introduces a dependency cycle",
                "gate_requirement",
                &requirement.id,
                None,
            ));
        }
        let mut same_set = existing
            .into_iter()
            .filter(|item| {
                item.work_id == requirement.work_id
                    && item.work_revision == requirement.work_revision
                    && item.work_report_id == requirement.work_report_id
                    && item.candidate_fingerprint == requirement.candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let mut required_ids = same_set
            .iter()
            .filter(|item| item.required)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        if requirement.required {
            required_ids.push(requirement.id.clone());
        }
        required_ids.sort();
        let set_fingerprint = canonical_json_fingerprint(&serde_json::to_value(required_ids)?);
        requirement.requirement_set_fingerprint = set_fingerprint.clone();
        for existing in &mut same_set {
            if existing.required {
                existing.requirement_set_fingerprint = set_fingerprint.clone();
                existing.version += 1;
            }
        }
        self.commit_trust_projection_unlocked(
            context,
            "gate_requirement",
            &requirement.id,
            "created",
            serde_json::to_value(&requirement)?,
            &requirement,
            same_set
                .into_iter()
                .filter(|item| item.required)
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
            Vec::new(),
        )
    }

    pub fn create_trust_gate_evaluation(
        &self,
        context: &MutationContext,
        evaluation: GateEvaluation,
    ) -> StoreResult<CanonicalMutationResult<GateEvaluation>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let requirements = self.trust_gate_requirements_unlocked(&context.execution_space_id)?;
        let requirement = requirements
            .get(&evaluation.requirement_id)
            .cloned()
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "gate requirement not found",
                    "gate_evaluation",
                    &evaluation.id,
                    None,
                )
            })?;
        let mut dependency_ids = requirement.dependency_requirement_ids.clone();
        dependency_ids.sort();
        let expected_dependency_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(dependency_ids)?);
        let prior_evaluations = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let waivers = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        if requirement.dependency_requirement_ids.iter().any(|id| {
            requirements.get(id).is_none_or(|dependency| {
                !gate_requirement_is_satisfied(
                    dependency,
                    &requirements,
                    &prior_evaluations,
                    &waivers,
                    &mut BTreeSet::new(),
                )
            })
        }) {
            return Err(trust_error(
                TrustErrorCode::GateEvaluationRequired,
                "gate dependencies must be satisfied before evaluation",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        if requirement.work_id != evaluation.work_id
            || requirement.work_revision != evaluation.work_revision
            || requirement.work_report_id != evaluation.work_report_id
            || requirement.candidate_fingerprint != evaluation.candidate_fingerprint
            || requirement.config_fingerprint != evaluation.config_fingerprint
            || requirement.evaluator_version != evaluation.evaluator_version
            || evaluation.dependency_fingerprint != expected_dependency_fingerprint
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "evaluation does not exactly match the frozen requirement",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "gate_evaluation",
            &evaluation.id,
            "evaluated",
            serde_json::to_value(&evaluation)?,
            &evaluation,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_gate_waiver(
        &self,
        context: &MutationContext,
        waiver: GateWaiver,
    ) -> StoreResult<CanonicalMutationResult<GateWaiver>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        if waiver.state != GateWaiverState::Active
            || context.authority_actor.as_ref() != Some(&waiver.authority_actor)
            || context.authenticated_actor != waiver.performed_by_actor
        {
            return Err(trust_error(
                TrustErrorCode::GateWaiverUnauthorized,
                "waiver authority and authenticated actor must match the mutation context",
                "gate_waiver",
                &waiver.id,
                None,
            ));
        }
        let requirement = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .remove(&waiver.requirement_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "waiver references a missing gate requirement",
                    "gate_waiver",
                    &waiver.id,
                    None,
                )
            })?;
        if requirement.work_id != waiver.work_id
            || requirement.work_revision != waiver.work_revision
            || requirement.candidate_fingerprint != waiver.candidate_fingerprint
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "waiver does not exactly match the frozen requirement",
                "gate_waiver",
                &waiver.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "gate_waiver",
            &waiver.id,
            "created",
            serde_json::to_value(&waiver)?,
            &waiver,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn revoke_trust_gate_waiver(
        &self,
        context: &MutationContext,
        waiver_id: &str,
        revoked_at: &str,
    ) -> StoreResult<CanonicalMutationResult<GateWaiver>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut waiver = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .remove(waiver_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "gate waiver not found",
                    "gate_waiver",
                    waiver_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<GateWaiver>(&envelope))?;
        if waiver.state != GateWaiverState::Active
            || context.authority_actor.as_ref() != Some(&waiver.authority_actor)
            || context.authenticated_actor != waiver.performed_by_actor
        {
            return Err(trust_error(
                TrustErrorCode::GateWaiverUnauthorized,
                "only the exact authorized actor may revoke an active waiver",
                "gate_waiver",
                waiver_id,
                Some(waiver.version),
            ));
        }
        waiver.state = GateWaiverState::Revoked;
        waiver.version += 1;
        waiver.revoked_at = Some(revoked_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "gate_waiver",
            waiver_id,
            "revoked",
            serde_json::json!({"revoked_at": revoked_at}),
            &waiver,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_gate_satisfied(
        &self,
        execution_space_id: &str,
        work_id: &str,
        work_revision: u64,
        report_id: &str,
        candidate_fingerprint: &str,
    ) -> StoreResult<()> {
        let requirements = self
            .trust_gate_requirements_unlocked(execution_space_id)?
            .into_values()
            .filter(|requirement| {
                requirement.work_id == work_id
                    && requirement.work_revision == work_revision
                    && requirement.work_report_id == report_id
                    && requirement.candidate_fingerprint == candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let mut requirement_ids = requirements
            .iter()
            .filter(|requirement| requirement.required)
            .map(|requirement| requirement.id.clone())
            .collect::<Vec<_>>();
        requirement_ids.sort();
        let expected_set_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(requirement_ids)?);
        if requirements
            .iter()
            .filter(|requirement| requirement.required)
            .any(|requirement| requirement.requirement_set_fingerprint != expected_set_fingerprint)
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "gate requirement set fingerprint is stale",
                "work",
                work_id,
                Some(work_revision),
            ));
        }
        let bindings = self
            .latest_trust_envelopes_unlocked(execution_space_id, "work_module_binding")?
            .into_values()
            .map(|envelope| event_projection::<WorkModuleBinding>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        for requirement in &requirements {
            if requirement.source == GateRequirementSource::Module {
                let binding = requirement
                    .source_binding_id
                    .as_deref()
                    .and_then(|id| bindings.iter().find(|binding| binding.id == id))
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::GateRequirementStale,
                            "module-derived gate lost its source binding",
                            "work",
                            work_id,
                            Some(work_revision),
                        )
                    })?;
                if binding.work_id != requirement.work_id
                    || binding.work_revision != requirement.work_revision
                    || binding.config_fingerprint
                        != requirement
                            .resolved_config
                            .get("module_config_fingerprint")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    || binding.version
                        != requirement
                            .resolved_config
                            .get("module_binding_version")
                            .and_then(Value::as_u64)
                            .unwrap_or_default()
                {
                    return Err(trust_error(
                        TrustErrorCode::GateRequirementStale,
                        "module-derived gate no longer matches its frozen source binding",
                        "work",
                        work_id,
                        Some(work_revision),
                    ));
                }
            }
        }
        let evaluations = self
            .latest_trust_envelopes_unlocked(execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let waivers = self
            .latest_trust_envelopes_unlocked(execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let requirement_map = requirements
            .iter()
            .cloned()
            .map(|requirement| (requirement.id.clone(), requirement))
            .collect::<BTreeMap<_, _>>();
        for requirement in requirements
            .into_iter()
            .filter(|requirement| requirement.required)
        {
            if !gate_requirement_is_satisfied(
                &requirement,
                &requirement_map,
                &evaluations,
                &waivers,
                &mut BTreeSet::new(),
            ) {
                return Err(trust_error(
                    TrustErrorCode::GateEvaluationRequired,
                    "required gate has no exact valid evaluation or waiver",
                    "work",
                    work_id,
                    Some(work_revision),
                ));
            }
        }
        Ok(())
    }

    pub fn accept_trust_work(
        &self,
        context: &MutationContext,
        team_id: &str,
        work_id: &str,
        report_id: &str,
        candidate_fingerprint: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "team_id": team_id,
            "work_id": work_id,
            "work_report_id": report_id,
            "candidate_fingerprint": candidate_fingerprint,
            "updated_at": updated_at,
        });
        let request_fingerprint = canonical_json_fingerprint(&request_payload);
        if let Some(replay) =
            self.trust_operation_envelopes_unlocked()?
                .into_iter()
                .find(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                        && envelope.authenticated_actor_id == context.authenticated_actor.id
                        && envelope.command_name == context.command_name
                        && envelope.operation.event.idempotency_key == context.idempotency_key
                })
        {
            if replay.operation.event.canonical_request_fingerprint != request_fingerprint
                || replay.operation.event.aggregate_kind != "work"
                || replay.operation.event.aggregate_id != work_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotency key was already used for a different Work acceptance",
                    "work",
                    work_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(&replay)?,
                event: replay.operation.event,
                replayed: true,
            });
        }
        let current = self.trust_team_work_unlocked(team_id, work_id, context.expected_version)?;
        if current.phase != firm_core::WorkPhase::Review
            || current.condition != firm_core::WorkCondition::Normal
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Work must be in normal review before acceptance",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        if current.owner_member_id.as_deref() == Some(context.authenticated_actor.id.as_str()) {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "the accountable Work owner cannot accept its own candidate",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        let report = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "work_report")?
            .remove(report_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::ReportEvidenceMissing,
                    "exact result WorkReport not found",
                    "work",
                    work_id,
                    Some(current.version),
                )
            })
            .and_then(|envelope| event_projection::<WorkReport>(&envelope))?;
        if report.kind != WorkReportKind::Result
            || report.work_id != current.id
            || report.work_revision != current.version
            || report.candidate.is_none()
            || report.candidate_fingerprint.as_deref() != Some(candidate_fingerprint)
            || report.evidence_refs.is_empty()
        {
            return Err(trust_error(
                TrustErrorCode::ReportEvidenceMissing,
                "acceptance requires the exact result Report, Candidate and evidence",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        self.trust_gate_satisfied(
            &context.execution_space_id,
            work_id,
            current.version,
            report_id,
            candidate_fingerprint,
        )?;
        let requirements = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .into_values()
            .filter(|requirement| {
                requirement.work_id == work_id
                    && requirement.work_revision == current.version
                    && requirement.work_report_id == report_id
                    && requirement.candidate_fingerprint == candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let requirement_ids = requirements
            .iter()
            .map(|requirement| requirement.id.as_str())
            .collect::<BTreeSet<_>>();
        let evaluations = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .filter(|evaluation| requirement_ids.contains(evaluation.requirement_id.as_str()))
            .collect::<Vec<_>>();
        let waivers = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .filter(|waiver| requirement_ids.contains(waiver.requirement_id.as_str()))
            .collect::<Vec<_>>();
        let mut next = current;
        next.phase = firm_core::WorkPhase::Closed;
        next.condition = firm_core::WorkCondition::Normal;
        next.resolution = Some(firm_core::WorkResolution::Accepted);
        next.result_summary = Some(report.summary.clone());
        next.version += 1;
        next.updated_at = updated_at.to_string();
        let side_records = std::iter::once(serde_json::to_value(&report)?)
            .chain(
                requirements
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .chain(
                evaluations
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .chain(
                waivers
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .collect();
        self.commit_trust_work_acceptance_unlocked(context, request_payload, &next, side_records)
    }

    pub fn create_trust_workspace_binding(
        &self,
        context: &MutationContext,
        mut binding: MemberWorkspaceBinding,
    ) -> StoreResult<CanonicalMutationResult<MemberWorkspaceBinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        required(
            &binding.canonical_root,
            "MemberWorkspaceBinding.canonical_root",
        )?;
        if binding.version != 1 || binding.lifecycle != WorkspaceLifecycle::Requested {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "workspace binding create requires requested lifecycle and version 1",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let path = std::path::Path::new(&binding.canonical_root);
        if !path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "canonical_root must be an absolute normalized path",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let run = self
            .trust_member_runs(&context.execution_space_id)?
            .into_iter()
            .find(|run| run.id == binding.member_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace binding references a missing MemberRun",
                    "workspace_binding",
                    &binding.id,
                    None,
                )
            })?;
        if run.team_run_id != binding.team_run_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace binding TeamRun does not match MemberRun",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let team_run = self
            .team_runs()?
            .into_iter()
            .rev()
            .find(|team_run| team_run.id == binding.team_run_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace TeamRun is missing",
                    "workspace_binding",
                    &binding.id,
                    None,
                )
            })?;
        if team_run.project_binding_id != binding.project_binding_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace ProjectBinding does not match TeamRun placement",
                "workspace_binding",
                &binding.id,
                None,
            ));
        }
        let mut cursor = std::path::PathBuf::new();
        for component in path.components() {
            cursor.push(component.as_os_str());
            match std::fs::symlink_metadata(&cursor) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceLinkEscape,
                        "workspace canonical path contains a symbolic-link component",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
        if path.exists() {
            let observed = observe_workspace_safety(path)?;
            if observed.canonical_root != path {
                return Err(trust_error(
                    TrustErrorCode::WorkspacePathUnsafe,
                    "canonical_root must equal the filesystem canonical path",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if !observed.link_escape_free {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceLinkEscape,
                    "workspace tree contains a symbolic-link escape",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if matches!(
                binding.mode,
                WorkspaceMode::Worktree | WorkspaceMode::SharedLive
            ) && observed.git_common_dir.is_none()
            {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "worktree/shared_live workspace must resolve a Git common directory",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            if let (Some(expected), Some(actual)) = (
                binding.git_common_dir.as_deref(),
                observed.git_common_dir.as_ref(),
            ) {
                let expected = canonical_git_path(path, expected)?;
                if &expected != actual {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceRepositoryMismatch,
                        "workspace Git common directory does not match the binding",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
            }
            binding.git_common_dir = observed
                .git_common_dir
                .map(|value| value.display().to_string());
            binding.dirty_fingerprint = observed.dirty_fingerprint;
        }
        if binding.mode == WorkspaceMode::SharedLive {
            if binding.ownership != WorkspaceOwnership::SharedProject {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "shared_live requires shared_project ownership",
                    "workspace_binding",
                    &binding.id,
                    None,
                ));
            }
            let member = self
                .trust_agent_members(&context.execution_space_id)?
                .into_iter()
                .find(|member| member.id == run.agent_member_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::InvalidStateTransition,
                        "workspace AgentMember is missing",
                        "workspace_binding",
                        &binding.id,
                        None,
                    )
                })?;
            if member.permission_ceiling != firm_core::agentfirm_api::PermissionCeiling::ReadOnly {
                if context.authority_actor.is_none() {
                    return Err(trust_error(
                        TrustErrorCode::UnauthorizedActor,
                        "writable shared_live requires explicit Host authority",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
                if self
                    .trust_workspace_bindings(&context.execution_space_id)?
                    .iter()
                    .any(|existing| {
                        existing.canonical_root == binding.canonical_root
                            && existing.lifecycle == WorkspaceLifecycle::Attached
                    })
                {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceGenerationFenced,
                        "shared_live writable workspace already has an attached writer",
                        "workspace_binding",
                        &binding.id,
                        None,
                    ));
                }
            }
        }
        self.commit_trust_projection_unlocked(
            context,
            "workspace_binding",
            &binding.id,
            "requested",
            serde_json::to_value(&binding)?,
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_workspace_bindings(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<MemberWorkspaceBinding>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "workspace_binding")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn transition_trust_workspace_binding(
        &self,
        context: &MutationContext,
        binding_id: &str,
        next: WorkspaceLifecycle,
        proof: &WorkspaceSafetyProof,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<MemberWorkspaceBinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut binding = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "workspace_binding")?
            .remove(binding_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::InvalidStateTransition,
                    "workspace binding not found",
                    "workspace_binding",
                    binding_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<MemberWorkspaceBinding>(&envelope))?;
        if proof.canonical_root != binding.canonical_root {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "safety proof canonical path differs from binding",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.project_binding_id != binding.project_binding_id {
            return Err(trust_error(
                TrustErrorCode::WorkspaceRepositoryMismatch,
                "workspace ProjectBinding does not match",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        let root = Path::new(&binding.canonical_root);
        let observed = if root.exists() {
            Some(observe_workspace_safety(root)?)
        } else {
            None
        };
        if next == WorkspaceLifecycle::Removed {
            if observed.is_some() {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceCleanupBlocked,
                    "workspace cleanup cannot complete while canonical_root still exists",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
        } else if next != WorkspaceLifecycle::Preparing && observed.is_none() {
            return Err(trust_error(
                TrustErrorCode::WorkspacePathUnsafe,
                "workspace path is missing for the requested lifecycle transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if let Some(observed) = observed.as_ref() {
            if observed.canonical_root != root {
                return Err(trust_error(
                    TrustErrorCode::WorkspacePathUnsafe,
                    "workspace path no longer equals its canonical filesystem path",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if !observed.link_escape_free || !proof.link_escape_free {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceLinkEscape,
                    "workspace contains a symlink/reparse escape",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if matches!(
                binding.mode,
                WorkspaceMode::Worktree | WorkspaceMode::SharedLive
            ) && observed.git_common_dir.is_none()
            {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "workspace no longer resolves the required Git repository",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if let Some(expected) = binding.git_common_dir.as_deref() {
                let expected = canonical_git_path(root, expected)?;
                if observed.git_common_dir.as_ref() != Some(&expected)
                    || proof
                        .git_common_dir
                        .as_deref()
                        .map(|value| canonical_git_path(root, value))
                        .transpose()?
                        .as_ref()
                        != Some(&expected)
                {
                    return Err(trust_error(
                        TrustErrorCode::WorkspaceRepositoryMismatch,
                        "workspace Git identity differs from binding or safety proof",
                        "workspace_binding",
                        binding_id,
                        Some(binding.version),
                    ));
                }
            }
            if !proof.repository_matches {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceRepositoryMismatch,
                    "workspace safety proof did not affirm the bound repository",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if observed.conflicted != proof.is_conflicted {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceConflicted,
                    "workspace conflict proof differs from the filesystem observation",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            if observed.dirty != proof.is_dirty {
                return Err(trust_error(
                    TrustErrorCode::WorkspaceDirty,
                    "workspace dirty proof differs from the filesystem observation",
                    "workspace_binding",
                    binding_id,
                    Some(binding.version),
                ));
            }
            binding.dirty_fingerprint = observed.dirty_fingerprint.clone();
        } else if !proof.link_escape_free {
            return Err(trust_error(
                TrustErrorCode::WorkspaceLinkEscape,
                "workspace safety proof did not establish a link-safe path",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if binding
            .attached_member_generation
            .is_some_and(|generation| generation != proof.observed_member_generation)
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceGenerationFenced,
                "workspace safety proof used a stale MemberRun generation",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.is_conflicted
            && next != WorkspaceLifecycle::Conflicted
            && next != WorkspaceLifecycle::CleanupBlocked
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceConflicted,
                "conflicted workspace cannot make the requested transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if proof.is_dirty
            && next != WorkspaceLifecycle::Dirty
            && next != WorkspaceLifecycle::CleanupBlocked
        {
            return Err(trust_error(
                TrustErrorCode::WorkspaceDirty,
                "dirty workspace cannot make the requested transition",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        let allowed = matches!(
            (binding.lifecycle, next),
            (WorkspaceLifecycle::Requested, WorkspaceLifecycle::Preparing)
                | (WorkspaceLifecycle::Preparing, WorkspaceLifecycle::Ready)
                | (WorkspaceLifecycle::Ready, WorkspaceLifecycle::Attached)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Dirty)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Conflicted)
                | (WorkspaceLifecycle::Attached, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Ready, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Dirty, WorkspaceLifecycle::Archived)
                | (WorkspaceLifecycle::Conflicted, WorkspaceLifecycle::Archived)
                | (
                    WorkspaceLifecycle::Ready,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Attached,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Dirty,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (
                    WorkspaceLifecycle::Conflicted,
                    WorkspaceLifecycle::CleanupBlocked
                )
                | (WorkspaceLifecycle::Dirty, WorkspaceLifecycle::Attached)
                | (WorkspaceLifecycle::Conflicted, WorkspaceLifecycle::Attached)
                | (
                    WorkspaceLifecycle::CleanupBlocked,
                    WorkspaceLifecycle::Archived
                )
                | (WorkspaceLifecycle::Archived, WorkspaceLifecycle::Removed)
        );
        if !allowed {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "workspace lifecycle transition is not allowed",
                "workspace_binding",
                binding_id,
                Some(binding.version),
            ));
        }
        if next == WorkspaceLifecycle::Attached {
            let run = self.claimable_member_run(
                &context.execution_space_id,
                &binding.member_run_id,
                proof.observed_member_generation,
            )?;
            binding.attached_member_generation = Some(run.runtime_generation);
        }
        if next == WorkspaceLifecycle::CleanupBlocked {
            binding.blocked_reason = Some(
                if proof.is_conflicted {
                    "WORKSPACE_CONFLICTED"
                } else if proof.is_dirty {
                    "WORKSPACE_DIRTY"
                } else {
                    "WORKSPACE_CLEANUP_BLOCKED"
                }
                .to_string(),
            );
        } else {
            binding.blocked_reason = None;
        }
        binding.lifecycle = next;
        binding.version += 1;
        binding.updated_at = updated_at.to_string();
        self.commit_trust_projection_unlocked(
            context,
            "workspace_binding",
            binding_id,
            "lifecycle_transitioned",
            serde_json::json!({"next": next, "proof": proof}),
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use firm_core::agentfirm_api::{ActorRef, AgentMemberOrganizationStatus, PermissionCeiling};
    use std::fs;

    fn actor(id: &str) -> ActorRef {
        ActorRef {
            kind: ActorKind::Human,
            id: id.into(),
        }
    }

    fn context(actor_id: &str, command: &str, key: &str, expected: u64) -> MutationContext {
        MutationContext {
            execution_space_id: "space-test".into(),
            authenticated_actor: actor(actor_id),
            authority_actor: None,
            command_name: command.into(),
            idempotency_key: key.into(),
            expected_version: expected,
        }
    }

    fn member(id: &str) -> AgentMember {
        AgentMember {
            id: id.into(),
            name: "Member".into(),
            description: "Canonical durable member".into(),
            role: "implementer".into(),
            capabilities: vec!["code".into()],
            skill_refs: Vec::new(),
            provider_profile_ref: Some("codex-default".into()),
            model_preference: None,
            workspace_policy: "managed-worktree".into(),
            permission_ceiling: PermissionCeiling::WorkspaceWrite,
            organization_status: AgentMemberOrganizationStatus::Active,
            version: 1,
            created_by: actor("host"),
            created_at: "t1".into(),
            updated_at: "t1".into(),
        }
    }

    #[test]
    fn canonical_operation_is_atomic_scoped_and_exactly_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "firm-trust-kernel-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = HarnessStore::new(&root);
        let first = store
            .create_trust_agent_member(
                &context("host", "agent_member.create", "same", 0),
                member("member-1"),
            )
            .expect("create");
        assert!(!first.replayed);
        let replay = store
            .create_trust_agent_member(
                &context("host", "agent_member.create", "same", 0),
                member("member-1"),
            )
            .expect("replay");
        assert!(replay.replayed);
        assert_eq!(first.event.id, replay.event.id);
        assert_eq!(store.canonical_operations().unwrap().len(), 1);

        let mut changed = member("member-1");
        changed.role = "reviewer".into();
        let error = store
            .create_trust_agent_member(&context("host", "agent_member.create", "same", 0), changed)
            .expect_err("payload drift conflicts")
            .to_string();
        assert!(error.contains("IDEMPOTENCY_KEY_REUSED"), "{error}");

        let mut other_member = member("member-2");
        other_member.created_by = actor("another");
        store
            .create_trust_agent_member(
                &context("another", "agent_member.create", "same", 0),
                other_member,
            )
            .expect("same key in another authenticated actor scope");
        assert_eq!(store.canonical_operations().unwrap().len(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}

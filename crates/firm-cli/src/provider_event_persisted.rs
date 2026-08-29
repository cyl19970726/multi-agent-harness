use std::path::Path;

use harness_core::agentfirm_api::{ActorKind, ActorRef, TeamMembershipRole, TeamMembershipStatus};
use harness_core::{NativeSessionRef, NodeDaemonLeaseStatus};
use harness_provider_events::{
    read_persisted_file_page, read_persisted_file_page_after, read_persisted_jsonl_snapshot,
    read_persisted_jsonl_snapshot_after, PersistedFileBoundary, PersistedOrderingKey,
    PersistedProjectionContext, PersistedReaderSource, ProviderKind, ProviderNativeEventRecord,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{current_unix_ms_u64, CliError, CliResult, HarnessStore};

const PERSISTED_SESSION_READ_SCHEMA: &str = "agentfirm.native_session_read.v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PersistedSessionReadMode {
    Snapshot,
    Older,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedSessionCursor {
    pub source_generation: String,
    pub ordering_key: PersistedOrderingKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedSessionViewer {
    pub actor: ActorRef,
    #[serde(default)]
    pub authority_actors: Vec<ActorRef>,
    /// Valid only on the machine-local AF_UNIX control path. Remote fabric
    /// callers must present an exact AgentMember or Team Host identity.
    #[serde(default)]
    pub local_operator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedSessionReadRequest {
    pub execution_space_id: String,
    pub project_binding_id: String,
    pub team_id: String,
    pub team_run_id: String,
    pub agent_member_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    pub native_session_fingerprint: String,
    pub node_id: String,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub mode: PersistedSessionReadMode,
    #[serde(default)]
    pub cursor: Option<PersistedSessionCursor>,
    pub limit: usize,
    pub viewer: PersistedSessionViewer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedSessionReadResponse {
    pub schema_version: String,
    pub native_source_ref: String,
    pub source_generation: String,
    pub snapshot_watermark: Option<PersistedOrderingKey>,
    pub records: Vec<ProviderNativeEventRecord>,
    pub has_more: bool,
    pub next_before: Option<PersistedSessionCursor>,
    pub incomplete_tail: bool,
    pub source_reset: bool,
}

pub(crate) fn native_session_fingerprint(
    session: &harness_core::agentfirm_api::NativeSessionRef,
) -> CliResult<String> {
    // Availability, resumability observations and verification timestamps may
    // advance between the RoleView and NodeDaemon reads. The fence identifies
    // the provider-owned conversation and reviewed adapter contract only.
    let bytes = serde_json::to_vec(&(
        &session.provider,
        &session.execution_mode,
        &session.native_session_id,
        &session.native_locator_kind,
        &session.provider_version,
        &session.adapter_contract_version,
        &session.parent_native_session_id,
    ))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

pub(crate) fn read_persisted_session_for_daemon(
    firm_home: &Path,
    daemon_node_id: &str,
    daemon_id: &str,
    daemon_generation: u64,
    daemon_instance_id: Option<&str>,
    request: &PersistedSessionReadRequest,
) -> CliResult<PersistedSessionReadResponse> {
    validate_request_shape(request)?;
    if request.node_id != daemon_node_id
        || request.node_daemon_id != daemon_id
        || request.node_daemon_generation != daemon_generation
    {
        return Err(CliError::Usage(
            "NODE_DAEMON_GENERATION_FENCED: persisted Session read targets another daemon".into(),
        ));
    }
    let space = crate::execution_space::context_for_id(firm_home, &request.execution_space_id)
        .map_err(|error| CliError::Usage(error.to_string()))?
        .ok_or_else(|| {
            CliError::Usage("EXECUTION_SPACE_SCOPE_MISMATCH: Execution Space not registered".into())
        })?;
    let store = HarnessStore::new(&space.store_root);
    let now = current_unix_ms_u64();
    let lease = store
        .latest_node_daemon_lease(&request.node_id)?
        .ok_or_else(|| CliError::Usage("NODE_DAEMON_GENERATION_FENCED: lease missing".into()))?;
    if lease.daemon_id != daemon_id
        || lease.generation != daemon_generation
        || daemon_instance_id.is_some_and(|instance_id| lease.instance_id != instance_id)
        || lease.status != NodeDaemonLeaseStatus::Active
        || lease.expires_unix_ms <= now
    {
        return Err(CliError::Usage(
            "NODE_DAEMON_GENERATION_FENCED: persisted Session read used stale daemon authority"
                .into(),
        ));
    }
    let registered = store
        .latest_node_project_registrations()?
        .iter()
        .any(|registration| {
            registration.node_id == request.node_id
                && registration.execution_space_id == request.execution_space_id
                && registration.project_binding_id == request.project_binding_id
                && registration.status == harness_core::NodeProjectRegistrationStatus::Active
        });
    if !registered {
        return Err(CliError::Usage(
            "PROJECT_BINDING_SCOPE_MISMATCH: project is not registered on the target daemon".into(),
        ));
    }
    let team = store
        .latest_teams()?
        .remove(&request.team_id)
        .filter(|team| team.node_id == request.node_id)
        .ok_or_else(|| {
            CliError::Usage("TEAM_NODE_SCOPE_MISMATCH: Team is not on this Node".into())
        })?;
    let run = store
        .team_runs()?
        .into_iter()
        .rev()
        .find(|run| run.id == request.team_run_id)
        .filter(|run| {
            run.agent_team_id == request.team_id
                && run.execution_node_id == request.node_id
                && run.project_binding_id == request.project_binding_id
        })
        .ok_or_else(|| {
            CliError::Usage("TEAM_RUN_SCOPE_MISMATCH: TeamRun/project placement differs".into())
        })?;
    let _ = (team, run);
    let memberships = store.fabric_team_memberships(&request.execution_space_id)?;
    let target_is_current_team_member = target_is_active_team_member(&memberships, request);
    if !target_is_current_team_member {
        return Err(CliError::Usage(
            "NATIVE_SESSION_TEAM_SCOPE_MISMATCH: Session owner is not an active member of the selected Team"
                .into(),
        ));
    }
    authorize_viewer(&memberships, request)?;
    let sessions = store
        .fabric_agent_sessions(&request.execution_space_id)?
        .into_iter()
        .filter(|session| session.id == request.agent_session_id)
        .collect::<Vec<_>>();
    let [session] = sessions.as_slice() else {
        return Err(CliError::Usage(
            "AGENT_SESSION_SCOPE_MISMATCH: exact AgentSession is missing or ambiguous".into(),
        ));
    };
    if session.agent_member_id != request.agent_member_id
        || session.execution_space_id != request.execution_space_id
        || session.node_id != request.node_id
        || session.node_daemon_id != request.node_daemon_id
        || session.node_daemon_generation != request.node_daemon_generation
        || session.runtime_generation != request.agent_session_generation
    {
        return Err(CliError::Usage(
            "AGENT_SESSION_GENERATION_FENCED: persisted Session placement changed".into(),
        ));
    }
    let native = session.native_session_ref.as_ref().ok_or_else(|| {
        CliError::Usage("PROVIDER_NATIVE_SESSION_UNAVAILABLE: Session has no native binding".into())
    })?;
    if native_session_fingerprint(native)? != request.native_session_fingerprint {
        return Err(CliError::Usage(
            "NATIVE_SESSION_FINGERPRINT_MISMATCH: native Session identity changed".into(),
        ));
    }
    let root_native: NativeSessionRef = serde_json::from_value(serde_json::to_value(native)?)?;
    let provider = provider_kind(&root_native.provider)
        .ok_or_else(|| CliError::Usage("provider persisted adapter is unavailable".into()))?;
    let source = reader_source(provider, &root_native);
    let limit = request.limit.clamp(1, super::MAX_SESSION_PAGE_SIZE);
    let page = if provider == ProviderKind::DeepseekHarness {
        let content = crate::native_session::read_deepseek_session_jsonl(&root_native)?;
        match (&request.mode, request.cursor.as_ref()) {
            (PersistedSessionReadMode::Snapshot, None) => {
                read_persisted_jsonl_snapshot(&source, &content, None, limit)
            }
            (PersistedSessionReadMode::Older, Some(cursor)) => {
                read_persisted_jsonl_snapshot(&source, &content, Some(cursor.ordering_key), limit)
            }
            (PersistedSessionReadMode::After, Some(cursor)) => {
                read_persisted_jsonl_snapshot_after(&source, &content, cursor.ordering_key, limit)
            }
            _ => return Err(invalid_cursor()),
        }
    } else {
        let Some((allowed_root, transcript_path)) =
            crate::native_session::locate_read_boundary(&root_native, &request.execution_space_id)?
        else {
            return Err(CliError::Usage(
                "PROVIDER_NATIVE_SESSION_UNAVAILABLE: persisted source is unavailable".into(),
            ));
        };
        let boundary = PersistedFileBoundary {
            allowed_root,
            transcript_path,
        };
        match (&request.mode, request.cursor.as_ref()) {
            (PersistedSessionReadMode::Snapshot, None) => {
                read_persisted_file_page(&source, &boundary, None, limit)
            }
            (PersistedSessionReadMode::Older, Some(cursor)) => {
                read_persisted_file_page(&source, &boundary, Some(cursor.ordering_key), limit)
            }
            (PersistedSessionReadMode::After, Some(cursor)) => {
                read_persisted_file_page_after(&source, &boundary, cursor.ordering_key, limit)
            }
            _ => return Err(invalid_cursor()),
        }
    }
    .map_err(|error| CliError::Usage(error.to_string()))?;
    let cursor_generation = request
        .cursor
        .as_ref()
        .map(|cursor| cursor.source_generation.as_str());
    if cursor_generation.is_some_and(|generation| generation != page.source_generation) {
        return Ok(PersistedSessionReadResponse {
            schema_version: PERSISTED_SESSION_READ_SCHEMA.into(),
            native_source_ref: page.native_source_ref,
            source_generation: page.source_generation,
            snapshot_watermark: page.snapshot_watermark,
            records: Vec::new(),
            has_more: false,
            next_before: None,
            incomplete_tail: page.incomplete_tail,
            source_reset: true,
        });
    }
    let response_watermark = if request.mode == PersistedSessionReadMode::After && page.has_more {
        page.rows.last().map(|row| row.ordering_key)
    } else {
        page.snapshot_watermark
    };
    let mut projector = page
        .projector(PersistedProjectionContext {
            native_source_ref: page.native_source_ref.clone(),
            agent_member_id: request.agent_member_id.clone(),
            agent_session_id: request.agent_session_id.clone(),
            agent_session_generation: request.agent_session_generation,
            observed_at: format!("unix-ms:{now}"),
        })
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let records = page
        .rows
        .into_iter()
        .map(|row| {
            projector
                .project(row)
                .map_err(|error| CliError::Usage(error.to_string()))
        })
        .collect::<CliResult<Vec<_>>>()?;
    Ok(PersistedSessionReadResponse {
        schema_version: PERSISTED_SESSION_READ_SCHEMA.into(),
        native_source_ref: page.native_source_ref,
        source_generation: page.source_generation.clone(),
        snapshot_watermark: response_watermark,
        records,
        has_more: page.has_more,
        next_before: page.next_before.map(|ordering_key| PersistedSessionCursor {
            source_generation: page.source_generation,
            ordering_key,
        }),
        incomplete_tail: page.incomplete_tail,
        source_reset: false,
    })
}

fn validate_request_shape(request: &PersistedSessionReadRequest) -> CliResult<()> {
    if request.execution_space_id.trim().is_empty()
        || request.project_binding_id.trim().is_empty()
        || request.team_id.trim().is_empty()
        || request.team_run_id.trim().is_empty()
        || request.agent_member_id.trim().is_empty()
        || request.agent_session_id.trim().is_empty()
        || request.agent_session_generation == 0
        || request.native_session_fingerprint.trim().is_empty()
        || request.node_id.trim().is_empty()
        || request.node_daemon_id.trim().is_empty()
        || request.node_daemon_generation == 0
    {
        return Err(CliError::Usage(
            "INVALID_NATIVE_SESSION_READ: exact scope is incomplete".into(),
        ));
    }
    Ok(())
}

fn target_is_active_team_member(
    memberships: &[harness_core::agentfirm_api::TeamMembership],
    request: &PersistedSessionReadRequest,
) -> bool {
    memberships.iter().any(|membership| {
        membership.team_id == request.team_id
            && membership.agent_member_id == request.agent_member_id
            && membership.state == TeamMembershipStatus::Active
    })
}

fn authorize_viewer(
    memberships: &[harness_core::agentfirm_api::TeamMembership],
    request: &PersistedSessionReadRequest,
) -> CliResult<()> {
    if request.viewer.local_operator {
        return Ok(());
    }
    // Remote transport authenticates the closed business actor only. Additional
    // payload actors are never authority and are rejected instead of being
    // treated as an unverified delegation chain.
    if !request.viewer.authority_actors.is_empty() {
        return Err(CliError::Usage(
            "NATIVE_SESSION_READ_NOT_AUTHORIZED: remote authority actors are not transport-bound"
                .into(),
        ));
    }
    let actor = &request.viewer.actor;
    let authorized = actor.kind == ActorKind::AgentMember
        && (actor.id == request.agent_member_id
            || memberships.iter().any(|membership| {
                membership.team_id == request.team_id
                    && membership.role == TeamMembershipRole::Host
                    && membership.state == TeamMembershipStatus::Active
                    && membership.agent_member_id == actor.id
            }));
    if !authorized {
        return Err(CliError::Usage(
            "NATIVE_SESSION_READ_NOT_AUTHORIZED: requires the exact Session owner or Team Host"
                .into(),
        ));
    }
    Ok(())
}

fn provider_kind(provider: &str) -> Option<ProviderKind> {
    match provider {
        "codex" => Some(ProviderKind::Codex),
        "claude" | "claude-code" | "claude_code" => Some(ProviderKind::Claude),
        "kimi" | "kimi-code" | "kimi_code" => Some(ProviderKind::Kimi),
        "pi" => Some(ProviderKind::Pi),
        "deepseek" | "deepseek_harness" => Some(ProviderKind::DeepseekHarness),
        _ => None,
    }
}

fn reader_source(provider: ProviderKind, native: &NativeSessionRef) -> PersistedReaderSource {
    let (source_family, format_version_fence) = match provider {
        ProviderKind::Codex => ("codex_rollout_jsonl", "codex.rollout.session_meta.v1"),
        ProviderKind::Claude => ("claude_native_session_jsonl", "claude.stream_json.v1"),
        ProviderKind::Kimi => ("kimi_wire_jsonl", "kimi.wire.current.v1"),
        ProviderKind::Pi => (
            "managed_pi_session_jsonl",
            "pi.session_jsonl.thinking_off.v1",
        ),
        ProviderKind::DeepseekHarness => (
            "deepseek_official_session_reader",
            "deepseek.session_reader.v1",
        ),
    };
    PersistedReaderSource {
        provider,
        native_session_id: native.native_session_id.clone(),
        source_family: source_family.into(),
        format_version_fence: format_version_fence.into(),
    }
}

fn invalid_cursor() -> CliError {
    CliError::Usage(
        "INVALID_NATIVE_SESSION_CURSOR: snapshot forbids a cursor; older/after require one".into(),
    )
}

pub(crate) fn local_operator_session_read_request(
    store: &HarnessStore,
    execution_space_id: &str,
    project_binding_id: &str,
    team_id: &str,
    agent_member_id: &str,
    limit: usize,
) -> CliResult<PersistedSessionReadRequest> {
    let team = store
        .latest_teams()?
        .remove(team_id)
        .filter(|team| !team.node_id.trim().is_empty())
        .ok_or_else(|| CliError::Usage("TEAM_NOT_FOUND: exact Team is unavailable".into()))?;
    let run = store
        .team_runs()?
        .into_iter()
        .rev()
        .find(|run| run.agent_team_id == team_id && run.project_binding_id == project_binding_id)
        .ok_or_else(|| {
            CliError::Usage("TEAM_RUN_SCOPE_MISMATCH: exact project TeamRun is unavailable".into())
        })?;
    let sessions = store
        .fabric_agent_sessions(execution_space_id)?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == agent_member_id
                && session.node_id == team.node_id
                && session.lifecycle != harness_core::agentfirm_api::AgentSessionStatus::Closed
        })
        .collect::<Vec<_>>();
    let [session] = sessions.as_slice() else {
        return Err(CliError::Usage(
            "AGENT_SESSION_SCOPE_MISMATCH: exact current AgentSession is missing or ambiguous"
                .into(),
        ));
    };
    let native = session.native_session_ref.as_ref().ok_or_else(|| {
        CliError::Usage("PROVIDER_NATIVE_SESSION_UNAVAILABLE: Session has no native binding".into())
    })?;
    let lease = store
        .latest_node_daemon_lease(&team.node_id)?
        .ok_or_else(|| CliError::Usage("NODE_DAEMON_GENERATION_FENCED: lease missing".into()))?;
    Ok(PersistedSessionReadRequest {
        execution_space_id: execution_space_id.into(),
        project_binding_id: project_binding_id.into(),
        team_id: team_id.into(),
        team_run_id: run.id,
        agent_member_id: agent_member_id.into(),
        agent_session_id: session.id.clone(),
        agent_session_generation: session.runtime_generation,
        native_session_fingerprint: native_session_fingerprint(native)?,
        node_id: team.node_id,
        node_daemon_id: lease.daemon_id,
        node_daemon_generation: lease.generation,
        mode: PersistedSessionReadMode::Snapshot,
        cursor: None,
        limit: limit.clamp(1, super::MAX_SESSION_PAGE_SIZE),
        viewer: PersistedSessionViewer {
            actor: ActorRef {
                kind: ActorKind::Service,
                id: "local-dashboard-operator".into(),
            },
            authority_actors: Vec::new(),
            local_operator: true,
        },
    })
}

/// Exact source-side envelope for the existing NodeGateway route journal. The
/// target applies the payload through the same NodeDaemon reader used by the
/// AF_UNIX path and returns the bounded response in TargetApplicationResult.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct RemotePersistedSessionRouteAuthority {
    pub operation_id: String,
    pub company_id: String,
    pub source_node_id: String,
    pub source_execution_space_id: String,
    pub source_gateway_generation: u64,
    pub source_node_daemon_id: String,
    pub source_node_daemon_generation: u64,
    pub control_plane_generation: u64,
    pub target_team_revision: u64,
    pub expected_target_revision: u64,
    pub node_actor: harness_fabric::AuthenticatedActor,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
}

#[allow(dead_code)]
#[allow(clippy::result_large_err)] // FabricError is the closed route API error type.
pub(crate) fn build_remote_persisted_session_read_operation(
    request: &PersistedSessionReadRequest,
    authority: &RemotePersistedSessionRouteAuthority,
) -> Result<harness_fabric::RoutedOperation, harness_fabric::FabricError> {
    if request.viewer.local_operator
        || !request.viewer.authority_actors.is_empty()
        || authority.operation_id.trim().is_empty()
        || authority.source_gateway_generation == 0
        || authority.source_node_daemon_generation == 0
        || authority.control_plane_generation == 0
        || authority.target_team_revision == 0
        || authority.expires_at_unix_ms <= authority.created_at_unix_ms
    {
        return Err(harness_fabric::FabricError::none(
            harness_fabric::FabricErrorCode::InvalidPayload,
            "remote native Session route authority is incomplete",
        ));
    }
    let business_actor_kind = match request.viewer.actor.kind {
        ActorKind::Human => "human",
        ActorKind::AgentMember => "agent_member",
        ActorKind::Service => "service",
        ActorKind::External => {
            return Err(harness_fabric::FabricError::none(
                harness_fabric::FabricErrorCode::UnauthorizedActor,
                "external actors cannot read provider-native Sessions",
            ))
        }
    };
    let payload = serde_json::to_value(request).map_err(|error| {
        harness_fabric::FabricError::none(
            harness_fabric::FabricErrorCode::InvalidPayload,
            error.to_string(),
        )
    })?;
    let payload_digest = format!("sha256:{}", harness_fabric::json_digest(&payload)?);
    let body = serde_json::to_value(harness_fabric::CollaborationBusinessReference {
        business_kind: "native_session_read".into(),
        required_capability: "collaboration.native_session_read".into(),
        business_actor_kind: business_actor_kind.into(),
        business_actor_id: request.viewer.actor.id.clone(),
        target_team_id: request.team_id.clone(),
        target_team_revision: authority.target_team_revision,
        placement_generation: 1,
        expected_revision: authority.expected_target_revision,
        payload_digest,
        payload,
    })
    .map_err(|error| {
        harness_fabric::FabricError::none(
            harness_fabric::FabricErrorCode::InvalidPayload,
            error.to_string(),
        )
    })?;
    let mut authorization_context = std::collections::BTreeMap::from([
        ("target_team_id".into(), request.team_id.clone()),
        (
            "target_team_revision".into(),
            authority.target_team_revision.to_string(),
        ),
        ("placement_generation".into(), "1".into()),
        (
            "required_capability".into(),
            "collaboration.native_session_read".into(),
        ),
        ("business_actor_kind".into(), business_actor_kind.into()),
        ("business_actor_id".into(), request.viewer.actor.id.clone()),
    ]);
    authorization_context.insert(
        "business_actor_session_id".into(),
        request.agent_session_id.clone(),
    );
    let operation = harness_fabric::RoutedOperation {
        id: authority.operation_id.clone(),
        company_id: authority.company_id.clone(),
        kind: harness_fabric::COLLABORATION_BUSINESS_OPERATION_KIND.into(),
        source_authority: harness_fabric::OperationSourceAuthority::Node,
        source_node_id: Some(authority.source_node_id.clone()),
        target_node_id: request.node_id.clone(),
        source_gateway_generation: Some(authority.source_gateway_generation),
        source_node_daemon_id: Some(authority.source_node_daemon_id.clone()),
        source_node_daemon_generation: Some(authority.source_node_daemon_generation),
        control_plane_generation: authority.control_plane_generation,
        source_execution_space_id: Some(authority.source_execution_space_id.clone()),
        target_execution_space_id: Some(request.execution_space_id.clone()),
        actor: authority.node_actor.clone(),
        actor_runtime_generation: Some(request.agent_session_generation),
        authorization_context,
        idempotency_key: authority.operation_id.clone(),
        ordering_key: format!("native-session-read:{}", request.agent_session_id),
        correlation_id: authority.operation_id.clone(),
        causation_id: None,
        expected_target_revision: Some(authority.expected_target_revision),
        body_schema: harness_fabric::COLLABORATION_BUSINESS_OPERATION_SCHEMA.into(),
        body_digest: harness_fabric::json_digest(&body)?,
        body,
        priority: harness_fabric::OperationPriority::Normal,
        created_at_unix_ms: authority.created_at_unix_ms,
        expires_at_unix_ms: authority.expires_at_unix_ms,
        protocol_version: harness_fabric::FABRIC_PROTOCOL_VERSION,
        schema_version: harness_fabric::FABRIC_SCHEMA_VERSION.into(),
        canonicalization_version: harness_fabric::FABRIC_CANONICALIZATION_VERSION.into(),
    };
    operation.closed_body()?;
    Ok(operation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote_request(viewer_id: &str) -> PersistedSessionReadRequest {
        PersistedSessionReadRequest {
            execution_space_id: "space-target".into(),
            project_binding_id: "project-target".into(),
            team_id: "team-target".into(),
            team_run_id: "run-target".into(),
            agent_member_id: "member-target".into(),
            agent_session_id: "session-target".into(),
            agent_session_generation: 3,
            native_session_fingerprint: format!("sha256:{}", "a".repeat(64)),
            node_id: "node-target".into(),
            node_daemon_id: "node-daemon:node-target".into(),
            node_daemon_generation: 5,
            mode: PersistedSessionReadMode::Snapshot,
            cursor: None,
            limit: 80,
            viewer: PersistedSessionViewer {
                actor: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: viewer_id.into(),
                },
                authority_actors: Vec::new(),
                local_operator: false,
            },
        }
    }

    fn membership(
        team_id: &str,
        member_id: &str,
        role: TeamMembershipRole,
    ) -> harness_core::agentfirm_api::TeamMembership {
        harness_core::agentfirm_api::TeamMembership {
            id: format!("membership:{team_id}:{member_id}"),
            team_id: team_id.into(),
            agent_member_id: member_id.into(),
            node_id: "node-target".into(),
            role,
            state: TeamMembershipStatus::Active,
            membership_generation: 1,
            default_subscription_refs: Vec::new(),
            created_by: ActorRef {
                kind: ActorKind::Service,
                id: "node-daemon:node-target".into(),
            },
            revision: 1,
            joined_at: "unix-ms:1".into(),
            left_at: None,
        }
    }

    #[test]
    fn remote_viewer_cannot_inject_host_authority_or_cross_team_session_owner() {
        let memberships = vec![
            membership("team-target", "host-target", TeamMembershipRole::Host),
            membership("team-other", "member-target", TeamMembershipRole::Member),
            membership("team-target", "viewer-other", TeamMembershipRole::Member),
        ];
        let mut request = remote_request("viewer-other");
        request.viewer.authority_actors.push(ActorRef {
            kind: ActorKind::AgentMember,
            id: "host-target".into(),
        });
        assert!(authorize_viewer(&memberships, &request).is_err());
        assert!(!target_is_active_team_member(&memberships, &request));

        request.viewer.authority_actors.clear();
        request.viewer.actor.id = "host-target".into();
        assert!(authorize_viewer(&memberships, &request).is_ok());
        assert!(
            !target_is_active_team_member(&memberships, &request),
            "Host authority cannot move a Session owner across Team scope"
        );

        let mut exact = memberships;
        exact.push(membership(
            "team-target",
            "member-target",
            TeamMembershipRole::Member,
        ));
        assert!(target_is_active_team_member(&exact, &request));
    }

    #[test]
    fn remote_read_uses_the_closed_node_gateway_application_envelope() {
        let request = remote_request("member-target");
        let authority = RemotePersistedSessionRouteAuthority {
            operation_id: "native-read-op-1".into(),
            company_id: "company-1".into(),
            source_node_id: "node-source".into(),
            source_execution_space_id: "space-source".into(),
            source_gateway_generation: 2,
            source_node_daemon_id: "node-daemon:node-source".into(),
            source_node_daemon_generation: 4,
            control_plane_generation: 7,
            target_team_revision: 2,
            expected_target_revision: 2,
            node_actor: harness_fabric::AuthenticatedActor {
                company_id: "company-1".into(),
                actor_id: "node-source".into(),
                actor_kind: harness_fabric::ActorKind::Service,
                role_bindings: std::collections::BTreeSet::from(["fabric_submit".into()]),
                session_id: "node-daemon:node-source:4".into(),
                issued_at_unix_ms: 10,
                expires_at_unix_ms: 100,
            },
            created_at_unix_ms: 10,
            expires_at_unix_ms: 100,
        };
        let operation = build_remote_persisted_session_read_operation(&request, &authority)
            .expect("closed remote read operation");
        let harness_fabric::ClosedOperationBody::CollaborationBusiness(reference) =
            operation.closed_body().expect("closed body")
        else {
            panic!("remote read must use collaboration application envelope")
        };
        assert_eq!(reference.business_kind, "native_session_read");
        assert_eq!(
            reference.required_capability,
            "collaboration.native_session_read"
        );
        assert_eq!(reference.payload["viewer"]["local_operator"], false);

        let mut local_only = request;
        local_only.viewer.local_operator = true;
        assert!(build_remote_persisted_session_read_operation(&local_only, &authority).is_err());
    }
}

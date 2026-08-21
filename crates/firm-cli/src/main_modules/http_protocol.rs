use super::*;

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentFirmHttpCredential {
    pub(crate) token: String,
    pub(crate) actor: harness_core::agentfirm_api::ActorRef,
    #[serde(default)]
    pub(crate) authority_actors: Vec<harness_core::agentfirm_api::ActorRef>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeCommandHttpRequest {
    #[serde(default)]
    pub(super) target_node_id: Option<String>,
    pub(super) command: harness_core::agentfirm_api::RuntimeCommandKind,
    pub(super) expires_unix_ms: u64,
    pub(super) payload: serde_json::Value,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeStartSessionIntent {
    pub(super) agent_member_id: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeSessionIntent {
    pub(super) session_id: String,
    pub(super) session_generation: u64,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeDispatchIntent {
    pub(super) session_id: String,
    pub(super) session_generation: u64,
    pub(super) delivery_id: String,
    pub(super) claim_id: String,
    pub(super) dispatch_mode: harness_core::agentfirm_api::RuntimeDispatchMode,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeAuthorMessageIntent {
    pub(super) draft: harness_core::agentfirm_api::MessageDraft,
    #[serde(default)]
    pub(super) remote_transfer: Option<fabric_runtime::QueueCollaborationMessageRequest>,
}

/// Server-resolved peer-Team Message admission. The authority's target half is
/// always read from the durable target subscription when the target Execution
/// Space is registered on this Node; a genuinely remote target requires the
/// caller's exact subscription revision and is fenced fail-closed by the
/// target Node before any delivery mutation. `requires_remote_route` is true
/// only when the target Team is placed on a distinct Node.
#[derive(Debug)]
pub(crate) struct ResolvedPeerTeamMessage {
    pub(crate) authority: harness_core::collaboration::PeerTeamMessageAdmissionAuthority,
    pub(crate) requires_remote_route: bool,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_peer_team_message_admission_authority(
    store: &HarnessStore,
    firm_home: &std::path::Path,
    execution_space_id: &str,
    local_node_id: &str,
    actor: &harness_core::agentfirm_api::ActorRef,
    draft: &harness_core::agentfirm_api::MessageDraft,
    request: Option<&fabric_runtime::QueueCollaborationMessageRequest>,
) -> Result<ResolvedPeerTeamMessage, String> {
    use harness_core::agentfirm_api::{
        ActorKind, AgentSessionStatus, MessageRecipientKind, TeamMembershipStatus,
    };
    use harness_core::collaboration::PeerTeamMessageAdmissionAuthority;

    let scope = draft
        .collaboration_scope
        .as_ref()
        .ok_or_else(|| "peer-Team Message requires an exact CollaborationScope".to_string())?;
    let source_team_id = draft
        .team_id
        .as_deref()
        .ok_or_else(|| "peer-Team Message requires its source Team".to_string())?;
    if actor.kind != ActorKind::AgentMember
        || scope.source_team_id != source_team_id
        || scope.source_team_id == scope.target_team_id
        || scope.delegation_id.is_some()
        || scope.expected_delegation_revision.is_some()
        || scope.source_work_ref.is_some()
        || scope.target_work_ref.is_some()
        || draft.recipients.len() != 1
        || draft.target_ref != draft.recipients[0]
    {
        return Err(
            "ordinary peer-Team admission requires one exact recipient and cannot carry WorkDelegation authority"
                .into(),
        );
    }
    // A Work link is context only; the Store validates that it names a current
    // Work of the source Team. Delegation-scoped Work references stay closed.
    let member_target_id = match draft.recipients[0].kind {
        MessageRecipientKind::Team if draft.recipients[0].id == scope.target_team_id => None,
        MessageRecipientKind::AgentMember => Some(draft.recipients[0].id.clone()),
        _ => {
            return Err(
                "ordinary peer-Team admission targets one peer Team or one peer TeamMembership"
                    .into(),
            )
        }
    };
    // Target topology: route facts come from the remote transfer request, or
    // from the same-Space durable Team when the caller authors locally.
    let (target_execution_space_id, target_node_id, caller_team_revision) = match request {
        Some(request) => {
            if scope.target_team_id != request.target_team_id {
                return Err("peer-Team route target disagrees with the CollaborationScope".into());
            }
            if request.target_node_id == local_node_id {
                return Err(if request.target_execution_space_id == execution_space_id {
                    "peer-Team route facts describe this Node and Execution Space; author locally without remote_transfer"
                        .into()
                } else {
                    "peer-Team fabric routing requires a distinct target Node; same-Node cross-Execution-Space targets are not routable"
                        .into()
                });
            }
            (
                request.target_execution_space_id.clone(),
                request.target_node_id.clone(),
                Some(request.target_team_revision),
            )
        }
        None => {
            let target_teams = store
                .agent_teams(execution_space_id)
                .map_err(|error| error.to_string())?
                .into_iter()
                .filter(|team| team.id == scope.target_team_id)
                .collect::<Vec<_>>();
            let [target_team] = target_teams.as_slice() else {
                return Err(
                    "peer-Team target Team is not in this Execution Space; supply exact remote_transfer route facts for a remote Node"
                        .into(),
                );
            };
            if target_team.status != harness_core::AgentTeamStatus::Active {
                return Err("peer-Team target Team is not Active".into());
            }
            if target_team.node_id != local_node_id {
                return Err(
                    "peer-Team target Team is placed on another Node; supply exact remote_transfer route facts"
                        .into(),
                );
            }
            (
                execution_space_id.to_string(),
                local_node_id.to_string(),
                None,
            )
        }
    };
    let requires_remote_route = request.is_some();
    let source_teams = store
        .agent_teams(execution_space_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|team| team.id == source_team_id)
        .collect::<Vec<_>>();
    if source_teams.len() != 1 {
        return Err("peer-Team source Team is missing or ambiguous".into());
    }
    let source_team = &source_teams[0];
    if source_team.status != harness_core::AgentTeamStatus::Active
        || source_team.node_id != local_node_id
    {
        return Err("peer-Team source Team is not active on the current Node".into());
    }
    let memberships = store
        .fabric_team_memberships(execution_space_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|membership| {
            membership.team_id == source_team_id
                && membership.agent_member_id == actor.id
                && membership.node_id == local_node_id
                && membership.state == TeamMembershipStatus::Active
        })
        .collect::<Vec<_>>();
    if memberships.len() != 1 {
        return Err(
            "peer-Team author must resolve to one exact active source TeamMembership".into(),
        );
    }
    let sessions = store
        .fabric_agent_sessions(execution_space_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|session| {
            session.agent_member_id == actor.id
                && session.node_id == local_node_id
                && session.lifecycle != AgentSessionStatus::Closed
        })
        .collect::<Vec<_>>();
    if sessions.len() != 1 {
        return Err("peer-Team author must resolve to one exact current local AgentSession".into());
    }
    let membership = &memberships[0];
    let session = &sessions[0];
    // The authoring session must be a child of the exact current NodeDaemon
    // generation; otherwise the daemon cannot honestly bind this author.
    let lease = store
        .latest_node_daemon_lease(local_node_id)
        .map_err(|error| error.to_string())?
        .filter(|lease| {
            lease.status == harness_core::NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| {
            "peer-Team authoring requires the current active NodeDaemon lease on this Node"
                .to_string()
        })?;
    if session.node_daemon_id != lease.daemon_id
        || session.node_daemon_generation != lease.generation
    {
        return Err(
            "peer-Team author session is not bound to the exact current NodeDaemon generation"
                .into(),
        );
    }
    // Read the durable target subscription from the target Store whenever it
    // is registered on this Node. Never hardcode a subscription revision: the
    // target fence advances on every target Team lifecycle transition.
    let target_store = if target_execution_space_id == execution_space_id {
        Some(store.clone())
    } else {
        match execution_space::context_for_id(firm_home, &target_execution_space_id)
            .map_err(|error| error.to_string())?
        {
            Some(space) => Some(HarnessStore::new(space.store_root)),
            None => None,
        }
    };
    let source_policy_ref = "peer-team-message-admission.v1".to_string();
    let source_policy_revision = 1;
    let source_required_capability = "message.peer_team.author".to_string();
    let target_required_capability = "collaboration.peer_message_deliver".to_string();
    let (
        target_team_revision,
        target_membership_id,
        target_membership_generation,
        target_agent_member_id,
        target_subscription_id,
        target_subscription_revision,
        target_authorization_policy_ref,
        target_policy_revision,
    );
    if let Some(target_store) = target_store.as_ref() {
        let target_teams = target_store
            .agent_teams(&target_execution_space_id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|team| team.id == scope.target_team_id)
            .collect::<Vec<_>>();
        let [target_team] = target_teams.as_slice() else {
            return Err("peer-Team target Team is missing or ambiguous on its Node".into());
        };
        if target_team.status != harness_core::AgentTeamStatus::Active
            || target_team.node_id != target_node_id
        {
            return Err("peer-Team target Team is not Active on the claimed target Node".into());
        }
        if let Some(caller_revision) = caller_team_revision {
            if caller_revision != target_team.revision {
                return Err(format!(
                    "peer-Team caller target Team revision {caller_revision} is stale; current revision is {}",
                    target_team.revision
                ));
            }
        }
        target_team_revision = target_team.revision;
        let subscriptions = target_store
            .fabric_message_subscriptions(&target_execution_space_id)
            .map_err(|error| error.to_string())?;
        let subscription = match member_target_id.as_deref() {
            None => {
                target_membership_id = None;
                target_membership_generation = None;
                target_agent_member_id = None;
                target_subscription_id = format!("team-inbox:{}", scope.target_team_id);
                target_authorization_policy_ref = "collaboration.peer_message_deliver".to_string();
                subscriptions
                    .iter()
                    .filter(|subscription| subscription.id == target_subscription_id)
                    .collect::<Vec<_>>()
            }
            Some(member_id) => {
                let target_memberships = target_store
                    .fabric_team_memberships(&target_execution_space_id)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .filter(|membership| {
                        membership.team_id == scope.target_team_id
                            && membership.agent_member_id == member_id
                            && membership.node_id == target_node_id
                            && membership.state == TeamMembershipStatus::Active
                    })
                    .collect::<Vec<_>>();
                let [target_membership] = target_memberships.as_slice() else {
                    return Err(
                        "peer-Team direct target must resolve to one exact active target TeamMembership"
                            .into(),
                    );
                };
                target_membership_id = Some(target_membership.id.clone());
                target_membership_generation = Some(target_membership.membership_generation);
                target_agent_member_id = Some(member_id.to_string());
                target_subscription_id = format!("direct:{}:{}", member_id, target_membership.id);
                target_authorization_policy_ref = "team.direct.active-members".to_string();
                subscriptions
                    .iter()
                    .filter(|subscription| subscription.id == target_subscription_id)
                    .collect::<Vec<_>>()
            }
        };
        let [subscription] = subscription.as_slice() else {
            return Err(
                "peer-Team durable target subscription is missing or ambiguous on its Node".into(),
            );
        };
        if subscription.status != harness_core::agentfirm_api::MessageSubscriptionStatus::Active {
            return Err("peer-Team durable target subscription is not Active".into());
        }
        target_subscription_revision = subscription.revision;
        target_policy_revision = subscription.policy_revision;
    } else {
        // The target Store is not visible from this Node. Only a Team target
        // can ride caller-declared route facts; a direct TeamMembership target
        // needs the durable membership generation, which is never guessed.
        if member_target_id.is_some() {
            return Err(
                "peer-Team direct TeamMembership targets require the target Execution Space registered on this Node"
                    .into(),
            );
        }
        let subscription_revision = request
            .and_then(|request| request.target_subscription_revision)
            .filter(|revision| *revision > 0)
            .ok_or_else(|| {
                "peer-Team remote target requires the caller's current target subscription revision; the target Node fences staleness fail-closed"
                    .to_string()
            })?;
        target_team_revision = caller_team_revision.unwrap_or(0);
        if target_team_revision == 0 {
            return Err("peer-Team remote target requires the current target Team revision".into());
        }
        target_membership_id = None;
        target_membership_generation = None;
        target_agent_member_id = None;
        target_subscription_id = format!("team-inbox:{}", scope.target_team_id);
        target_subscription_revision = subscription_revision;
        target_authorization_policy_ref = "collaboration.peer_message_deliver".to_string();
        // The team-inbox policy revision is a creation-time protocol constant
        // of the current build; the target recomputes and fences the digest.
        target_policy_revision = 1;
    }
    let mut authority = PeerTeamMessageAdmissionAuthority {
        // DOC-108: ordinary peer-Team messaging must not depend on the
        // retired Company registry. Without an explicit remote_transfer
        // Company label, the collaboration scope is the local Execution
        // Space. The label feeds only the self-consistent admission digests;
        // remote targets fence against their own inbound policy revision.
        company_id: match request {
            Some(request) => request.company_id.clone(),
            None => format!("space:{execution_space_id}"),
        },
        source_execution_space_id: execution_space_id.into(),
        source_team_id: source_team_id.into(),
        source_team_revision: source_team.revision,
        source_membership_id: membership.id.clone(),
        source_membership_generation: membership.membership_generation,
        source_agent_member_id: actor.id.clone(),
        source_session_id: session.id.clone(),
        source_session_generation: session.runtime_generation,
        source_node_id: local_node_id.into(),
        source_node_daemon_id: session.node_daemon_id.clone(),
        source_node_daemon_generation: session.node_daemon_generation,
        target_execution_space_id,
        target_team_id: scope.target_team_id.clone(),
        target_team_revision,
        target_node_id,
        target_membership_id,
        target_membership_generation,
        target_agent_member_id,
        source_policy_ref,
        source_policy_revision,
        source_policy_digest: String::new(),
        source_required_capability,
        target_subscription_id,
        target_subscription_revision,
        target_authorization_policy_ref,
        target_policy_revision,
        target_policy_digest: String::new(),
        target_required_capability,
        authority_digest: String::new(),
    };
    authority.source_policy_digest = harness_store::peer_team_source_policy_digest(&authority);
    authority.target_policy_digest = harness_store::peer_team_target_policy_digest(&authority);
    authority.authority_digest = harness_store::peer_team_message_authority_digest(&authority);
    Ok(ResolvedPeerTeamMessage {
        authority,
        requires_remote_route,
    })
}

pub(super) fn runtime_command_capability(
    command: harness_core::agentfirm_api::RuntimeCommandKind,
) -> &'static str {
    use harness_core::agentfirm_api::RuntimeCommandKind;
    match command {
        RuntimeCommandKind::AuthorMessage => "message.author",
        RuntimeCommandKind::StartSession => "agent_session.start",
        RuntimeCommandKind::StopSession => "agent_session.stop",
        RuntimeCommandKind::ResumeSession => "agent_session.resume",
        RuntimeCommandKind::DispatchProvider => "provider.dispatch",
        RuntimeCommandKind::CancelProviderTurn => "provider.cancel",
        RuntimeCommandKind::OpenRuntime => "runtime.open",
        RuntimeCommandKind::ResumeNativeSession => "runtime.native_session.resume",
        RuntimeCommandKind::ReleaseRuntime => "runtime.release",
        RuntimeCommandKind::CloseMember => "member.close",
        RuntimeCommandKind::ReopenMember => "member.reopen",
        RuntimeCommandKind::RetireMember => "member.retire",
        RuntimeCommandKind::DeleteNativeSession => "runtime.native_session.delete",
        RuntimeCommandKind::StartCycle => "cycle.start",
        RuntimeCommandKind::InjectCurrentCycle => "cycle.inject_current",
        RuntimeCommandKind::QueueAtNativeBoundary => "cycle.queue_native_boundary",
        RuntimeCommandKind::InterruptCurrentCycle => "cycle.interrupt_current",
        RuntimeCommandKind::CancelPendingInput => "cycle.pending_input.cancel",
        RuntimeCommandKind::InspectContinuation => "continuation.inspect",
        RuntimeCommandKind::ActivateContinuation => "continuation.activate",
        RuntimeCommandKind::InhibitContinuation => "continuation.inhibit",
        RuntimeCommandKind::ResumeContinuation => "continuation.resume",
        RuntimeCommandKind::ReplaceContinuationCondition => "continuation.condition.replace",
        RuntimeCommandKind::ClearContinuation => "continuation.clear",
        RuntimeCommandKind::QuiesceExecutionLane => "execution_lane.quiesce",
        RuntimeCommandKind::DrainRuntime => "runtime.drain",
        RuntimeCommandKind::StopBackgroundTask => "background_task.stop",
        RuntimeCommandKind::TransferExecutionDriver => "driver.transfer",
        RuntimeCommandKind::InspectCommandEffect => "command_effect.inspect",
        RuntimeCommandKind::ReconcileUnknownEffect => "command_effect.reconcile",
        RuntimeCommandKind::ReattachLiveRuntime => "runtime.reattach",
        RuntimeCommandKind::AbortIfNotApplied => "command_effect.abort_if_not_applied",
    }
}

pub(super) fn runtime_control_actor_is_authorized(
    actor: &harness_core::agentfirm_api::ActorRef,
    target_identity_id: &str,
    target_node_id: &str,
    target_daemon_id: &str,
) -> Result<bool, StoreError> {
    use harness_core::agentfirm_api::ActorKind;
    if actor.kind == ActorKind::AgentMember && actor.id == target_identity_id {
        return Ok(true);
    }
    if actor.kind == ActorKind::Service
        && (actor.id == target_node_id || actor.id == target_daemon_id)
    {
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn resolve_agentfirm_http_credential(
    presented_token: Option<&str>,
) -> Result<AgentFirmHttpCredential, String> {
    let encoded = std::env::var("AGENTFIRM_HTTP_CREDENTIALS_JSON")
        .map_err(|_| "member-trust HTTP credential registry is not configured".to_string())?;
    let credentials = serde_json::from_str::<Vec<AgentFirmHttpCredential>>(&encoded)
        .map_err(|error| format!("member-trust HTTP credential registry is invalid: {error}"))?;
    let mut unique_tokens = std::collections::BTreeSet::new();
    if credentials
        .iter()
        .any(|credential| !unique_tokens.insert(credential.token.clone()))
    {
        return Err("member-trust HTTP credential registry contains a duplicate token".into());
    }
    if credentials.iter().any(|credential| {
        credential.actor.id.trim().is_empty()
            || credential.token.trim().is_empty()
            || credential
                .authority_actors
                .iter()
                .any(|authority| authority.id.trim().is_empty())
    }) {
        return Err("member-trust HTTP credential registry contains an empty identity".into());
    }
    let token = presented_token
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "member-trust HTTP credential is missing".to_string())?;
    let credential = credentials
        .into_iter()
        .find(|credential| credential.token == token)
        .ok_or_else(|| "member-trust HTTP credential is invalid".to_string())?;
    Ok(credential)
}

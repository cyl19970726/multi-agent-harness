use super::*;

#[cfg(test)]
pub(super) fn compatibility_team_actor(id: &str, authn_source: &str) -> TeamActorRef {
    TeamActorRef {
        kind: if id == "host" {
            TeamActorKind::Host
        } else if id.starts_with("operator") {
            TeamActorKind::Operator
        } else if id.starts_with("service:") {
            TeamActorKind::Service
        } else if id.starts_with("agent-member:") {
            TeamActorKind::AgentMember
        } else {
            TeamActorKind::ProviderRuntimeProjection
        },
        id: id.to_string(),
        display_name: None,
        authn_source: Some(authn_source.to_string()),
    }
}

pub(super) fn compatibility_team_recipient(id: &str) -> TeamRecipientRef {
    TeamRecipientRef {
        kind: if id == "host" {
            TeamRecipientKind::Host
        } else if id.starts_with("agent-member:") {
            TeamRecipientKind::AgentMember
        } else {
            TeamRecipientKind::ProviderRuntimeProjection
        },
        id: id.to_string(),
    }
}

#[cfg(any())]
pub(super) fn parse_team_actor_kind(value: &str) -> CliResult<TeamActorKind> {
    match value {
        "host" => Ok(TeamActorKind::Host),
        "agent_member" => Ok(TeamActorKind::AgentMember),
        "operator" => Ok(TeamActorKind::Operator),
        "service" => Ok(TeamActorKind::Service),
        _ => Err(CliError::Usage(format!(
            "unknown actor kind `{value}` (host|agent_member|operator|service)"
        ))),
    }
}

pub(super) fn team_event_source_for_actor(actor: &TeamActorRef) -> TeamRunEventSourceKind {
    match actor.kind {
        TeamActorKind::Host => TeamRunEventSourceKind::Host,
        TeamActorKind::ProviderRuntimeProjection | TeamActorKind::AgentMember => {
            TeamRunEventSourceKind::Member
        }
        TeamActorKind::Operator => TeamRunEventSourceKind::Operator,
        TeamActorKind::Service => TeamRunEventSourceKind::Service,
    }
}

pub(super) fn ensure_member_coordination_open(member: &ProviderRuntimeProjection) -> CliResult<()> {
    if !member.coordination_is_active()
        || matches!(
            member.status,
            MemberRunStatus::Completed | MemberRunStatus::Failed | MemberRunStatus::Stopped
        )
    {
        return Err(CliError::Usage(format!(
            "member {} coordination is {} and runtime status is {}; explicitly Reopen a closed member or create a replacement for a retired/terminal member",
            member.id,
            serde_snake_label(&member.coordination_status),
            serde_snake_label(&member.status)
        )));
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) enum TeamMessageDeliveryMode {
    Routed,
    InjectDelivered,
}

/// Route a message inside a team run and fold it into the event log. Shared
/// by the `team-run send` CLI arm and POST /v1/team-runs/{id}/messages. v0
/// does not drive the member state machine: a handoff/blocker from a member is
/// only recorded as an event — the member's ProviderRuntimeProjection row is left untouched.
#[allow(clippy::too_many_arguments)]
// Historical TeamRun message-writer fixture retained only as migration
// evidence. It must not compile into either production or test authority.
#[cfg(any())]
pub(super) fn send_team_message(
    store: &HarnessStore,
    team_run_id: &str,
    sender_runtime_id: &str,
    recipient_runtime_ids: Vec<String>,
    kind: ProviderDispatchIntent,
    body: &str,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    source_plan_ref: Option<String>,
    response_intent: Option<ProviderResponseIntent>,
) -> CliResult<TeamMessageProjection> {
    send_team_message_as(
        store,
        team_run_id,
        compatibility_team_actor(
            sender_runtime_id,
            if sender_runtime_id == "host" {
                "host_cli"
            } else {
                "member_runtime"
            },
        ),
        recipient_runtime_ids,
        kind,
        body,
        correlation_id,
        causation_id,
        source_plan_ref,
        response_intent,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(any())]
pub(super) fn send_team_message_as(
    store: &HarnessStore,
    team_run_id: &str,
    sender: TeamActorRef,
    recipient_runtime_ids: Vec<String>,
    kind: ProviderDispatchIntent,
    body: &str,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    source_plan_ref: Option<String>,
    response_intent: Option<ProviderResponseIntent>,
) -> CliResult<TeamMessageProjection> {
    send_team_message_as_work(
        store,
        team_run_id,
        sender,
        recipient_runtime_ids,
        kind,
        body,
        None,
        correlation_id,
        causation_id,
        source_plan_ref,
        response_intent,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(any())]
pub(super) fn send_team_message_as_work(
    store: &HarnessStore,
    team_run_id: &str,
    sender: TeamActorRef,
    recipient_runtime_ids: Vec<String>,
    kind: ProviderDispatchIntent,
    body: &str,
    work_id: Option<String>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    source_plan_ref: Option<String>,
    response_intent: Option<ProviderResponseIntent>,
) -> CliResult<TeamMessageProjection> {
    let message = prepare_team_message_as(
        store,
        team_run_id,
        &sender,
        recipient_runtime_ids,
        kind,
        body,
        work_id,
        correlation_id,
        causation_id,
        TeamMessageDeliveryMode::Routed,
        response_intent,
    )?;
    publish_team_message(store, &sender, message)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_team_message_as(
    store: &HarnessStore,
    team_run_id: &str,
    sender: &TeamActorRef,
    recipient_runtime_ids: Vec<String>,
    kind: ProviderDispatchIntent,
    body: &str,
    work_id: Option<String>,
    correlation_id: Option<String>,
    causation_id: Option<String>,
    delivery_mode: TeamMessageDeliveryMode,
    response_intent: Option<ProviderResponseIntent>,
) -> CliResult<TeamMessageProjection> {
    // Fail fast on an unknown run id rather than journaling an orphan message.
    let run = latest_team_run(store, team_run_id)?;
    if body.trim().is_empty() {
        return Err(CliError::Usage(
            "team message body must not be empty".to_string(),
        ));
    }
    let valid_member = |id: &str| id == "host" || run.member_run_ids.iter().any(|row| row == id);
    let member_runs = latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == team_run_id)
        .collect::<Vec<_>>();
    let sender_runtime_id = match sender.kind {
        TeamActorKind::Host => {
            if sender.id != "host"
                && run
                    .host_actor
                    .as_ref()
                    .is_none_or(|actor| actor.id != sender.id)
            {
                return Err(CliError::Usage(format!(
                    "Host actor {} is not bound to team run {team_run_id}",
                    sender.id
                )));
            }
            "host".to_string()
        }
        TeamActorKind::ProviderRuntimeProjection => {
            let member = member_runs
                .iter()
                .find(|member| member.id == sender.id)
                .ok_or_else(|| {
                    CliError::Usage(format!(
                        "message sender {} does not belong to team run {team_run_id}",
                        sender.id
                    ))
                })?;
            ensure_member_coordination_open(member)?;
            sender.id.clone()
        }
        TeamActorKind::AgentMember => {
            let linked = member_runs
                .iter()
                .filter(|member| member.agent_member_id == sender.id)
                .collect::<Vec<_>>();
            match linked.as_slice() {
                [member] => {
                    ensure_member_coordination_open(member)?;
                    member.id.clone()
                }
                [] => {
                    return Err(CliError::Usage(format!(
                        "Agent identity {} has no ProviderRuntimeProjection in team run {team_run_id}",
                        sender.id
                    )))
                }
                _ => {
                    return Err(CliError::Usage(format!(
                        "Agent identity {} has several MemberRuns in team run {team_run_id}; author as an explicit member_run",
                        sender.id
                    )))
                }
            }
        }
        TeamActorKind::Operator => format!("operator:{}", sender.id),
        TeamActorKind::Service => format!("service:{}", sender.id),
    };
    if recipient_runtime_ids.is_empty() {
        return Err(CliError::Usage(
            "team message requires at least one recipient".to_string(),
        ));
    }
    let mut recipients = std::collections::HashSet::new();
    for recipient in &recipient_runtime_ids {
        if !valid_member(recipient) {
            return Err(CliError::Usage(format!(
                "message recipient {recipient} does not belong to team run {team_run_id}"
            )));
        }
        if !recipients.insert(recipient.as_str()) {
            return Err(CliError::Usage(format!(
                "duplicate message recipient: {recipient}"
            )));
        }
        if recipient != "host" {
            let member = member_runs
                .iter()
                .find(|member| member.id == recipient.as_str())
                .ok_or_else(|| {
                    CliError::Usage(format!(
                        "message recipient {recipient} has no ProviderRuntimeProjection projection in team run {team_run_id}"
                    ))
                })?;
            ensure_member_coordination_open(member)?;
        }
    }
    if let Some(work_id) = work_id.as_deref() {
        let work = store
            .latest_works()?
            .into_iter()
            .find(|work| work.id == work_id)
            .ok_or_else(|| CliError::Usage(format!("unknown Work: {work_id}")))?;
        if work.team_run_id != team_run_id {
            return Err(CliError::Usage(format!(
                "Work {work_id} belongs to TeamRun {}, not {team_run_id}",
                work.team_run_id
            )));
        }
    }
    let (correlation_id, causation_id) =
        resolve_team_message_lineage(store, team_run_id, &kind, correlation_id, causation_id)?;
    let message = TeamMessageProjection {
        id: generated_id("tmsg"),
        team_run_id: team_run_id.to_string(),
        work_id,
        // Retained only as a deserialization field for pre-ADR-0051 rows.
        // Current messages never bind their provenance to a Legacy Wave.
        source_plan_ref: None,
        sender: Some(sender.clone()),
        sender_runtime_id: sender_runtime_id.clone(),
        recipients: recipient_runtime_ids
            .iter()
            .map(|member_id| compatibility_team_recipient(member_id))
            .collect(),
        recipient_runtime_ids: recipient_runtime_ids.clone(),
        kind,
        body: body.to_string(),
        correlation_id,
        causation_id,
        response_intent,
        evidence_refs: Vec::new(),
        deliveries: recipient_runtime_ids
            .iter()
            .map(|member_id| ProviderDispatchAttempt {
                member_id: member_id.clone(),
                // The Host control plane receives member-originated mail at
                // creation time. Provider members, by contrast, consume
                // ordinary coordination mail at their next available round.
                policy: match delivery_mode {
                    TeamMessageDeliveryMode::InjectDelivered => TeamDeliveryPolicy::Inject,
                    TeamMessageDeliveryMode::Routed
                        if member_id == "host" && sender.kind != TeamActorKind::Host =>
                    {
                        TeamDeliveryPolicy::ManualAck
                    }
                    TeamMessageDeliveryMode::Routed => TeamDeliveryPolicy::Queue,
                },
                status: match delivery_mode {
                    TeamMessageDeliveryMode::InjectDelivered => TeamDeliveryStatus::Delivered,
                    TeamMessageDeliveryMode::Routed
                        if member_id == "host" && sender.kind != TeamActorKind::Host =>
                    {
                        TeamDeliveryStatus::Delivered
                    }
                    TeamMessageDeliveryMode::Routed => TeamDeliveryStatus::Queued,
                },
                attempt: match delivery_mode {
                    TeamMessageDeliveryMode::InjectDelivered => 1,
                    TeamMessageDeliveryMode::Routed
                        if member_id == "host" && sender.kind != TeamActorKind::Host =>
                    {
                        1
                    }
                    TeamMessageDeliveryMode::Routed => 0,
                },
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: now_string(),
            })
            .collect(),
        created_at: now_string(),
    };
    Ok(message)
}

pub(super) fn publish_team_message(
    store: &HarnessStore,
    sender: &TeamActorRef,
    mut message: TeamMessageProjection,
) -> CliResult<TeamMessageProjection> {
    use harness_core::agentfirm_api::{
        ActorKind, ActorRef, MessageAddressKind, MessageDraft, MessageKind, MessageRecipientKind,
        MessageRecipientRef, ResponseIntent, RuntimeCommandKind,
    };
    let run = latest_team_run(store, &message.team_run_id)?;
    let execution_space_id = team_run_execution_space_id(store, &run)?;
    let registration = store
        .latest_node_project_registrations()?
        .into_iter()
        .find(|registration| {
            registration.node_id == run.execution_node_id
                && registration.execution_space_id == execution_space_id
                && registration.project_binding_id == run.project_binding_id
                && registration.status == NodeProjectRegistrationStatus::Active
        })
        .ok_or_else(|| CliError::Usage("EXECUTION_SPACE_SCOPE_MISMATCH".into()))?;
    let lease = store
        .latest_node_daemon_lease(&run.execution_node_id)?
        .filter(|lease| {
            lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| CliError::Usage("NODE_DAEMON_UNAVAILABLE".into()))?;
    let member_runs = latest_member_runs_in_append_order(store)?;
    let authenticated_actor = match sender.kind {
        TeamActorKind::AgentMember => ActorRef {
            kind: ActorKind::AgentMember,
            id: sender.id.clone(),
        },
        TeamActorKind::ProviderRuntimeProjection => {
            let stable = member_runs
                .iter()
                .find(|member| member.id == sender.id)
                .ok_or_else(|| CliError::Usage("AGENT_IDENTITY_NOT_FOUND".into()))?;
            ActorRef {
                kind: ActorKind::AgentMember,
                id: stable.agent_member_id.clone(),
            }
        }
        TeamActorKind::Service => ActorRef {
            kind: ActorKind::Service,
            id: sender.id.clone(),
        },
        TeamActorKind::Host => ActorRef {
            kind: ActorKind::AgentMember,
            id: if sender.id == "host" {
                store
                    .latest_teams()?
                    .remove(&run.agent_team_id)
                    .map(|team| team.host_agent_id)
                    .ok_or_else(|| {
                        CliError::Usage("TeamRun references a missing AgentTeam".into())
                    })?
            } else {
                sender.id.clone()
            },
        },
        TeamActorKind::Operator => ActorRef {
            kind: ActorKind::Human,
            id: sender.id.clone(),
        },
    };
    let recipients = message
        .recipient_runtime_ids
        .iter()
        .map(|recipient| {
            if recipient == "host" {
                Ok(MessageRecipientRef {
                    kind: MessageRecipientKind::AgentMember,
                    id: store
                        .latest_teams()?
                        .remove(&run.agent_team_id)
                        .map(|team| team.host_agent_id)
                        .ok_or_else(|| {
                            CliError::Usage("TeamRun references a missing AgentTeam".into())
                        })?,
                })
            } else {
                let stable = member_runs
                    .iter()
                    .find(|member| member.id == *recipient && member.team_run_id == run.id)
                    .ok_or_else(|| CliError::Usage(format!("member run not found: {recipient}")))?;
                Ok(MessageRecipientRef {
                    kind: MessageRecipientKind::AgentMember,
                    id: stable.agent_member_id.clone(),
                })
            }
        })
        .collect::<CliResult<Vec<_>>>()?;
    let target_ref = recipients
        .first()
        .cloned()
        .ok_or_else(|| CliError::Usage("Message requires a recipient".into()))?;
    let address_kind =
        if recipients.len() == 1 && target_ref.kind == MessageRecipientKind::AgentMember {
            MessageAddressKind::DirectAgent
        } else {
            MessageAddressKind::AuthorizedBroadcast
        };
    let kind = match message.kind {
        ProviderDispatchIntent::ProviderInteractionRequest => {
            MessageKind::ProviderInteractionRequest
        }
        ProviderDispatchIntent::ProviderInteractionResponse => {
            MessageKind::ProviderInteractionResponse
        }
        ProviderDispatchIntent::Control => MessageKind::RequestDecision,
        ProviderDispatchIntent::Message => {
            if message.causation_id.is_some() {
                MessageKind::Reply
            } else {
                MessageKind::Message
            }
        }
    };
    let response_intent = match message.effective_response_intent() {
        ProviderResponseIntent::Informational => ResponseIntent::Informational,
        ProviderResponseIntent::ResponseRequired => ResponseIntent::ResponseRequired,
    };
    let payload = serde_json::json!({
        "draft": MessageDraft {
            address_kind,
            target_ref,
            recipients,
            team_id: Some(run.agent_team_id.clone()),
            team_run_id: Some(run.id.clone()),
            work_id: message.work_id.clone(),
            collaboration_scope: None,
            kind,
            body: message.body.clone(),
            correlation_id: message.correlation_id.clone(),
            causation_id: message.causation_id.clone(),
            response_intent,
            evidence_refs: message.evidence_refs.clone(),
            schema_version: 1,
        }
    });
    let command = harness_core::agentfirm_api::ControlCommandEnvelope {
        id: format!("runtime-command:message:{}", message.id),
        execution_space_id: registration.execution_space_id.clone(),
        target_node_id: run.execution_node_id.clone(),
        target_node_daemon_id: lease.daemon_id,
        target_node_daemon_generation: lease.generation,
        authenticated_actor,
        command: RuntimeCommandKind::AuthorMessage,
        required_capability: "message.author".into(),
        idempotency_key: message.id.clone(),
        expected_version: 0,
        expires_unix_ms: current_unix_ms_u64().saturating_add(30_000),
        binding: Default::default(),
        precondition: Default::default(),
        postcondition: runtime_command_postcondition_for(RuntimeCommandKind::AuthorMessage),
        payload_fingerprint: harness_store::canonical_json_fingerprint(&payload),
        payload,
        issued_at: now_string(),
    };
    let firm_home = execution_space::firm_home().map_err(execution_space_err)?;
    let response = supervisor_daemon::runtime_command_via_socket(
        &firm_home,
        &run.execution_node_id,
        &command,
    )?;
    if response["ok"].as_bool() != Some(true) {
        return Err(CliError::Usage(format!(
            "NodeDaemon rejected Message: {}",
            response["error"].as_str().unwrap_or("unknown error")
        )));
    }
    if let Some(id) = response["result"]["id"].as_str() {
        message.id = id.to_string();
    }
    message.deliveries.clear();
    for delivery in store.fabric_message_deliveries(&registration.execution_space_id)? {
        if delivery.message_id == message.id {
            let Some(member_id) = delivery.recipient_agent_member_id.clone() else {
                continue;
            };
            message.deliveries.push(ProviderDispatchAttempt {
                member_id,
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: delivery.attempt,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at,
            });
        }
    }
    let seq = next_team_run_seq(store, &message.team_run_id)?;
    append_team_run_event(
        store,
        &message.team_run_id,
        seq,
        team_event_source_for_actor(sender),
        matches!(
            sender.kind,
            TeamActorKind::ProviderRuntimeProjection | TeamActorKind::AgentMember
        )
        .then(|| message.sender_runtime_id.clone()),
        "message",
        &message.id,
        "created",
        &format!(
            "{} from {} to [{}]",
            team_message_kind_label(&message.kind),
            sender.id,
            message.recipient_runtime_ids.join(",")
        ),
    )?;
    Ok(message)
}

/// Latest-wins read model for one TeamRun recipient.
///
/// This deliberately reads only Harness-owned coordination mail. Provider
/// transcripts, tools, commands, child agents, and turn lifecycle remain in
/// the provider-native session. Without `include_all`, only mail the recipient
/// can act on now is returned; `include_all` returns the recipient's complete
/// received message at its latest stored state.
pub(crate) fn team_run_inbox(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
    include_all: bool,
) -> CliResult<Vec<TeamMessageProjection>> {
    let run = latest_team_run(store, team_run_id)?;
    if member_run_id != "host" {
        if !run
            .member_run_ids
            .iter()
            .any(|member_id| member_id == member_run_id)
        {
            return Err(CliError::Usage(format!(
                "inbox recipient {member_run_id} does not belong to team run {team_run_id}"
            )));
        }
        let member = latest_member_runs_in_append_order(store)?
            .into_iter()
            .find(|member| member.id == member_run_id)
            .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
        if !include_all && !member.coordination_is_active() {
            return Ok(Vec::new());
        }
    }
    let mut messages = canonical_team_messages_for_run(store, team_run_id)?
        .into_iter()
        .filter(|message| {
            message
                .recipient_runtime_ids
                .iter()
                .any(|id| id == member_run_id)
        })
        .filter(|message| {
            include_all
                || message.deliveries.iter().any(|delivery| {
                    delivery.member_id == member_run_id
                        && matches!(
                            delivery.status,
                            TeamDeliveryStatus::Queued | TeamDeliveryStatus::Delivered
                        )
                })
        })
        .collect::<Vec<_>>();
    messages.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(messages)
}

pub(super) fn require_external_interactive_inbox_scope(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
) -> CliResult<()> {
    let bound_team_run_id = env::var("FIRM_TEAM_RUN_ID")
        .or_else(|_| env::var("HARNESS_TEAM_RUN_ID"))
        .map_err(|_| {
            CliError::Usage(
                "team-run inbox is reserved for the explicitly bound external_interactive session; managed members must use `member inbox`"
                    .into(),
            )
        })?;
    let bound_member_run_id = env::var("FIRM_MEMBER_RUN_ID")
        .or_else(|_| env::var("HARNESS_MEMBER_RUN_ID"))
        .map_err(|_| {
            CliError::Usage(
                "team-run inbox is reserved for the explicitly bound external_interactive session; managed members must use `member inbox`"
                    .into(),
            )
        })?;
    if bound_team_run_id != team_run_id || bound_member_run_id != member_run_id {
        return Err(CliError::Usage(
            "UNAUTHORIZED_ACTOR: team-run inbox cannot select another TeamRun or MemberRun".into(),
        ));
    }
    let member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id && member.team_run_id == team_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    if !member.is_external_interactive() {
        return Err(CliError::Usage(
            "RETIRED_RUNTIME_READER: managed members must read their exact-self Inbox through `member inbox`"
                .into(),
        ));
    }
    Ok(())
}

pub(super) fn canonical_team_messages_for_run(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<Vec<TeamMessageProjection>> {
    let run = latest_team_run(store, team_run_id)?;
    let execution_space_id = team_run_execution_space_id(store, &run)?;
    let member_runs = latest_member_runs_in_append_order(store)?;
    let identity_to_runtime = member_runs
        .iter()
        .filter(|member| member.team_run_id == team_run_id)
        .map(|member| (member.agent_member_id.clone(), member.id.clone()))
        .collect::<BTreeMap<_, _>>();
    let host_identity = match latest_teams(store)?.remove(&run.agent_team_id) {
        Some(team) => team.host_agent_id,
        // Pre-cutover migration fact (DOC-108): the Team exists only in the
        // retired legacy ledger; tolerate it as read-only legacy context. A
        // team id absent from both ledgers still fails closed.
        None => legacy_team_definitions_by_id(store)?
            .get(&run.agent_team_id)
            .and_then(|team| team.get("host_agent_id"))
            .and_then(|host| host.as_str())
            .map(str::to_owned)
            .ok_or_else(|| CliError::Usage("TeamRun references a missing AgentTeam".into()))?,
    };
    let host_runtime_id = projected_host_runtime_id(&run, &host_identity, &identity_to_runtime)?;
    let mut projected = Vec::new();
    let deliveries = store.fabric_message_deliveries(&execution_space_id)?;
    for message in store
        .fabric_messages(&execution_space_id)?
        .into_iter()
        .filter(|message| message.team_run_id.as_deref() == Some(team_run_id))
    {
        let recipient_rows = deliveries
            .iter()
            .filter(|delivery| delivery.message_id == message.id)
            .filter_map(|delivery| {
                let recipient_agent_member_id = delivery.recipient_agent_member_id.as_deref()?;
                let runtime_id = if recipient_agent_member_id == host_identity {
                    host_runtime_id.clone()
                } else {
                    identity_to_runtime
                        .get(recipient_agent_member_id)
                        .cloned()?
                };
                Some((runtime_id, delivery))
            })
            .collect::<Vec<_>>();
        if recipient_rows.is_empty()
            && message.recipients.iter().any(|recipient| {
                recipient.kind
                    == harness_core::agentfirm_api::MessageRecipientKind::ControlPlaneActor
                    && recipient.id == host_identity
            })
        {
            let mut row = project_canonical_inbox_message(&message, &host_runtime_id, None);
            row.sender_runtime_id = if message.sender_actor_ref.id == host_identity {
                host_runtime_id.clone()
            } else {
                identity_to_runtime
                    .get(&message.sender_actor_ref.id)
                    .cloned()
                    .unwrap_or_else(|| message.sender_actor_ref.id.clone())
            };
            projected.push(row);
            continue;
        }
        let Some((first_runtime, first_delivery)) = recipient_rows.first() else {
            continue;
        };
        let mut row =
            project_canonical_inbox_message(&message, first_runtime, Some(first_delivery));
        row.sender_runtime_id = if message.sender_actor_ref.id == host_identity {
            host_runtime_id.clone()
        } else {
            identity_to_runtime
                .get(&message.sender_actor_ref.id)
                .cloned()
                .unwrap_or_else(|| message.sender_actor_ref.id.clone())
        };
        row.recipient_runtime_ids = recipient_rows
            .iter()
            .map(|(runtime_id, _)| runtime_id.clone())
            .collect();
        row.recipients = row
            .recipient_runtime_ids
            .iter()
            .map(|id| TeamRecipientRef {
                kind: TeamRecipientKind::ProviderRuntimeProjection,
                id: id.clone(),
            })
            .collect();
        row.deliveries = recipient_rows
            .iter()
            .filter_map(|(runtime_id, delivery)| {
                project_canonical_inbox_message(&message, runtime_id, Some(delivery))
                    .deliveries
                    .into_iter()
                    .next()
            })
            .collect();
        projected.push(row);
    }
    projected.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(projected)
}

fn projected_host_runtime_id(
    run: &AgentTeamRun,
    host_identity: &str,
    identity_to_runtime: &BTreeMap<String, String>,
) -> CliResult<String> {
    match run.host_control_mode {
        HostControlMode::Managed => identity_to_runtime
            .get(host_identity)
            .cloned()
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "MANAGED_HOST_MEMBER_RUN_MISSING: TeamRun {} has no MemberRun for Host AgentMember {}",
                    run.id, host_identity
                ))
            }),
        HostControlMode::ExternalInteractive => Ok("host".to_string()),
    }
}

/// Resolve the one Execution Space that owns a TeamRun's canonical runtime
/// projections. MemberRun materialization is the frozen run-scoped binding;
/// Node registrations are only a fail-closed fallback for a pre-materialized
/// run and must themselves be unambiguous.
pub(super) fn team_run_execution_space_id(
    store: &HarnessStore,
    run: &AgentTeamRun,
) -> CliResult<String> {
    store_conflict_as_usage(store.current_team_run_execution_space(run))
}

pub(super) fn team_run_unacknowledged_message_count(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<usize> {
    let run = latest_team_run(store, team_run_id)?;
    let host_identity = latest_teams(store)?
        .remove(&run.agent_team_id)
        .map(|team| team.host_agent_id)
        .or_else(|| {
            legacy_team_definitions_by_id(store)
                .ok()?
                .get(&run.agent_team_id)?
                .get("host_agent_id")?
                .as_str()
                .map(str::to_owned)
        })
        .ok_or_else(|| CliError::Usage("TeamRun references a missing AgentTeam".into()))?;
    let identity_to_runtime = latest_member_runs_in_append_order(store)?
        .into_iter()
        .filter(|member| member.team_run_id == team_run_id)
        .map(|member| (member.agent_member_id, member.id))
        .collect::<BTreeMap<_, _>>();
    let host_runtime_id = projected_host_runtime_id(&run, &host_identity, &identity_to_runtime)?;
    Ok(canonical_team_messages_for_run(store, team_run_id)?
        .iter()
        .filter(|message| has_actionable_unacknowledged_host_delivery(message, &host_runtime_id))
        .count())
}

pub(super) fn project_canonical_inbox_message(
    message: &harness_core::agentfirm_api::Message,
    member_run_id: &str,
    delivery: Option<&harness_core::agentfirm_api::CanonicalMessageDelivery>,
) -> TeamMessageProjection {
    let sender_kind = match message.sender_actor_ref.kind {
        harness_core::agentfirm_api::ActorKind::AgentMember => TeamActorKind::AgentMember,
        harness_core::agentfirm_api::ActorKind::Service => TeamActorKind::Service,
        harness_core::agentfirm_api::ActorKind::Human
        | harness_core::agentfirm_api::ActorKind::External => TeamActorKind::Operator,
    };
    let delivery_projection = delivery.map(|delivery| ProviderDispatchAttempt {
        member_id: member_run_id.to_string(),
        policy: TeamDeliveryPolicy::Queue,
        status: match delivery.status {
            harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Queued
            | harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Routed => {
                TeamDeliveryStatus::Queued
            }
            harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Claimed => {
                TeamDeliveryStatus::Claimed
            }
            harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::ProviderReceived => {
                TeamDeliveryStatus::Delivered
            }
            harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Acknowledged => {
                TeamDeliveryStatus::Acknowledged
            }
            harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Failed
            | harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Invalidated => {
                TeamDeliveryStatus::Failed
            }
            harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Expired => {
                TeamDeliveryStatus::Expired
            }
        },
        attempt: delivery.attempt,
        claim_id: delivery.claim_id.clone(),
        claimed_by_supervisor_id: None,
        claimed_generation: delivery.claimed_node_daemon_generation,
        claimed_unix_ms: None,
        claim_expires_unix_ms: None,
        provider_receipt_id: delivery.provider_receipt_id.clone(),
        failure_reason: delivery.failure_detail.clone(),
        updated_at: delivery.updated_at.clone(),
    });
    let deliveries = delivery_projection.into_iter().chain(
        (delivery.is_none() && member_run_id == "host").then(|| ProviderDispatchAttempt {
            member_id: "host".into(),
            policy: TeamDeliveryPolicy::Queue,
            status: TeamDeliveryStatus::Delivered,
            attempt: 1,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: Some(message.source_authority_generation),
            claimed_unix_ms: None,
            claim_expires_unix_ms: None,
            provider_receipt_id: Some("control-plane-visible".into()),
            failure_reason: None,
            updated_at: message.created_at.clone(),
        }),
    );
    TeamMessageProjection {
        id: message.id.clone(),
        team_run_id: message.team_run_id.clone().unwrap_or_default(),
        work_id: message.work_id.clone(),
        source_plan_ref: None,
        sender: Some(TeamActorRef {
            kind: sender_kind,
            id: message.sender_actor_ref.id.clone(),
            display_name: None,
            authn_source: Some("canonical_message_fabric".into()),
        }),
        sender_runtime_id: message.sender_actor_ref.id.clone(),
        recipients: vec![TeamRecipientRef {
            kind: TeamRecipientKind::ProviderRuntimeProjection,
            id: member_run_id.to_string(),
        }],
        recipient_runtime_ids: vec![member_run_id.to_string()],
        kind: match message.kind {
            harness_core::agentfirm_api::MessageKind::Message
            | harness_core::agentfirm_api::MessageKind::Reply => ProviderDispatchIntent::Message,
            harness_core::agentfirm_api::MessageKind::RequestDecision => {
                ProviderDispatchIntent::Control
            }
            harness_core::agentfirm_api::MessageKind::ProviderInteractionRequest => {
                ProviderDispatchIntent::ProviderInteractionRequest
            }
            harness_core::agentfirm_api::MessageKind::ProviderInteractionResponse => {
                ProviderDispatchIntent::ProviderInteractionResponse
            }
        },
        body: message.body.clone(),
        correlation_id: message.correlation_id.clone(),
        causation_id: message.causation_id.clone(),
        response_intent: Some(match message.response_intent {
            harness_core::agentfirm_api::ResponseIntent::Informational => {
                ProviderResponseIntent::Informational
            }
            harness_core::agentfirm_api::ResponseIntent::ResponseRequired => {
                ProviderResponseIntent::ResponseRequired
            }
        }),
        evidence_refs: message.evidence_refs.clone(),
        deliveries: deliveries.collect(),
        created_at: message.created_at.clone(),
    }
}

/// Aggregate Host mail for the exact provider-native Host thread bound to each
/// TeamRun. A plugin in one desktop task must never receive mail owned by
/// another task merely because both use the same project store.
pub(crate) fn host_inbox_for_native_thread(
    store: &HarnessStore,
    host_surface: &str,
    host_thread_id: &str,
    include_all: bool,
) -> CliResult<Vec<serde_json::Value>> {
    if host_surface.trim().is_empty() || host_thread_id.trim().is_empty() {
        return Err(CliError::Usage(
            "Host surface and native thread id must not be empty".to_string(),
        ));
    }
    let attention_inboxes = store.host_attention_inboxes_for_native_thread(
        host_surface,
        host_thread_id,
        include_all,
    )?;
    let attentions_by_run: std::collections::HashMap<&str, &Vec<HostAttention>> = attention_inboxes
        .iter()
        .map(|inbox| (inbox.team_run_id.as_str(), &inbox.attentions))
        .collect();
    let mut entries = Vec::new();
    for run in latest_team_runs_in_append_order(store)? {
        if canonical_surface(&run.host_surface) != canonical_surface(host_surface)
            || run.host_thread_id.as_deref() != Some(host_thread_id)
        {
            continue;
        }
        let messages = team_run_inbox(store, &run.id, "host", include_all)?;
        let attentions = attentions_by_run.get(run.id.as_str());
        let has_content = !messages.is_empty() || attentions.is_some_and(|a| !a.is_empty());
        if include_all || has_content {
            let entry_attentions = attentions.map_or(&[] as &[HostAttention], |a| a.as_slice());
            entries.push(serde_json::json!({
                "team_run_id": run.id,
                "team_run_status": run.status,
                "mission_id": team_run_mission_id(store, &run)?,
                "messages": messages,
                "attentions": entry_attentions,
            }));
        }
    }
    Ok(entries)
}

/// Resolve and verify manual conversation lineage. An explicit correlation
/// must already identify a message in this run; a causation-only reply inherits
/// its direct cause's correlation. Work ownership is intentionally absent.
///
/// Omitted lineage retains the v0 generated-default behavior and makes no
/// claim of Work ownership. Every validation happens before the append, so
/// bad cross-run, unknown, or mismatched lineage is atomic.
pub(super) fn resolve_team_message_lineage(
    store: &HarnessStore,
    team_run_id: &str,
    _kind: &ProviderDispatchIntent,
    supplied_correlation_id: Option<String>,
    supplied_causation_id: Option<String>,
) -> CliResult<(String, Option<String>)> {
    let messages = canonical_team_messages_for_run(store, team_run_id)?;
    let has_explicit_correlation = supplied_correlation_id.is_some();

    if let Some(correlation_id) = supplied_correlation_id.as_deref() {
        if correlation_id.trim().is_empty() {
            return Err(CliError::Usage(
                "--correlation-id must not be empty".to_string(),
            ));
        }
    }

    let cause = if let Some(causation_id) = supplied_causation_id.as_deref() {
        if causation_id.trim().is_empty() {
            return Err(CliError::Usage(
                "--causation-id must not be empty".to_string(),
            ));
        }
        Some(
            messages
            .iter()
            .find(|message| message.team_run_id == team_run_id && message.id == causation_id)
            .cloned()
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "causation_id `{causation_id}` does not identify a message in team run {team_run_id}"
                ))
            })?,
        )
    } else {
        None
    };

    if let (Some(correlation_id), Some(cause)) =
        (supplied_correlation_id.as_deref(), cause.as_ref())
    {
        if cause.correlation_id != correlation_id {
            return Err(CliError::Usage(format!(
                "causation_id `{causation_id}` has correlation_id `{}`, not `{correlation_id}`",
                cause.correlation_id,
                causation_id = supplied_causation_id.as_deref().unwrap_or_default(),
            )));
        }
    }

    let correlation_id = supplied_correlation_id
        .or_else(|| cause.as_ref().map(|message| message.correlation_id.clone()))
        .unwrap_or_else(|| generated_id("corr"));

    if has_explicit_correlation
        && !messages.iter().any(|message| {
            message.team_run_id == team_run_id && message.correlation_id == correlation_id
        })
    {
        return Err(CliError::Usage(format!(
            "correlation_id `{correlation_id}` does not identify a conversation in team run {team_run_id}"
        )));
    }

    Ok((correlation_id, supplied_causation_id))
}

/// Load the latest row for a team run id, or a clear not-found error.
pub(super) fn latest_team_run(store: &HarnessStore, id: &str) -> CliResult<AgentTeamRun> {
    latest_team_runs_in_append_order(store)?
        .into_iter()
        .find(|run| run.id == id)
        .ok_or_else(|| CliError::Usage(format!("team run not found: {id}")))
}

/// Resolve the optional legacy Mission provenance of a TeamRun's durable
/// AgentTeam. Post-DEV-35 Teams never require a Mission, so this returns
/// `None` for mission-less Teams; the empty-string compatibility projection
/// is never exposed as an identifier.
pub(crate) fn team_run_mission_id(
    store: &HarnessStore,
    run: &AgentTeamRun,
) -> CliResult<Option<String>> {
    latest_teams(store)?
        .remove(&run.agent_team_id)
        .map(|team| {
            team.legacy_mission_id
                .filter(|mission_id| !mission_id.trim().is_empty())
        })
        .ok_or_else(|| {
            CliError::Usage(format!(
                "AgentTeam {} for TeamRun {} not found",
                run.agent_team_id, run.id
            ))
        })
}

pub(super) fn team_run_display_json(
    store: &HarnessStore,
    run: &AgentTeamRun,
) -> CliResult<serde_json::Value> {
    // A current status projection is also an authority decision: never show a
    // partially materialized Legacy TeamRun as controllable current state.
    team_run_execution_space_id(store, run)?;
    Ok(serde_json::to_value(run)?)
}

/// Parse a team run status from its snake_case wire name.
pub(super) fn parse_team_run_status(s: &str) -> CliResult<TeamRunStatus> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown team run status `{s}` (planning|running|waiting|reviewing|completed|failed|cancelled)"
        ))
    })
}

/// Transition a team-run attempt. Only these moves are legal:
/// `running|reviewing → completed` (the Host records the attempt outcome) and
/// `planning|waiting|reviewing → cancelled`. Cancelling a reviewing
/// attempt is the explicit rejection path that permits a later retry without
/// falsely making the failed attempt acceptance-eligible. Anything else is a usage error
/// (HTTP 400) so an attempt cannot skip review or resurrect after termination.
/// Completing a running TeamRun deliberately does not close any ProviderRuntimeProjection:
/// persistent member runtimes may carry work into a later run of the same Team. A running attempt
/// still cannot be status-cancelled until provider execution has a real
/// cooperative interruption path.
/// Completing an attempt records only this attempt's outcome. It neither closes
/// the Mission nor appends a Mission Log closeout entry.
/// Appends the new AgentTeamRun row (latest-wins) and folds a TeamRunEvent so
/// the dashboard timeline narrates the gate decision. Shared by
/// POST /v1/team-runs/{id}/transition and the `team-run complete|cancel` arms.
pub(crate) fn transition_team_run(
    store: &HarnessStore,
    team_run_id: &str,
    target: TeamRunStatus,
) -> CliResult<AgentTeamRun> {
    let current = latest_team_run(store, team_run_id)?;
    team_run_execution_space_id(store, &current)?;
    let previous_status = current.status;
    let allowed = matches!(
        (previous_status, target),
        (TeamRunStatus::Running, TeamRunStatus::Completed)
            | (TeamRunStatus::Reviewing, TeamRunStatus::Completed)
            | (TeamRunStatus::Planning, TeamRunStatus::Cancelled)
            | (TeamRunStatus::Waiting, TeamRunStatus::Cancelled)
            | (TeamRunStatus::Reviewing, TeamRunStatus::Cancelled)
    );
    if !allowed {
        return Err(CliError::Usage(format!(
            "invalid team-run transition: {} → {} (allowed: running|reviewing → completed, planning|waiting|reviewing → cancelled; running cancellation requires provider interruption)",
            serde_snake_label(&previous_status),
            serde_snake_label(&target),
        )));
    }
    let mut next = current.clone();
    next.status = target;
    next.updated_at = now_string();
    if target == TeamRunStatus::Completed {
        next.completed_at = Some(now_string());
    }
    store_conflict_as_usage(store.compare_and_append_team_run_lifecycle(&current, &next))?;
    let seq = next_team_run_seq(store, team_run_id)?;
    let (operation, summary) = match target {
        TeamRunStatus::Completed => (
            "completed",
            format!(
                "team-run attempt completed: {} → completed; member runtimes remain Host-owned",
                serde_snake_label(&previous_status)
            ),
        ),
        _ => (
            "updated",
            format!(
                "team run cancelled: {} → cancelled",
                serde_snake_label(&previous_status)
            ),
        ),
    };
    append_team_run_event(
        store,
        team_run_id,
        seq,
        TeamRunEventSourceKind::Host,
        None,
        "team_run",
        &next.id,
        operation,
        &summary,
    )?;
    Ok(next)
}

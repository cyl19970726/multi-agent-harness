use super::*;

pub(super) fn execute_canonical_role_action(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    route: CanonicalRoute<'_>,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    match route {
        CanonicalRoute::Message {
            team_run_id,
            operation,
        } => {
            let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid message intent: {error}"),
                    "team_run",
                    team_run_id,
                    None,
                )
            })?;
            let (_run, team) = team_for_run(store, team_run_id)?;
            let actor_is_host = is_host(&auth, &team.host_agent_id);
            let actor_member_run = resolve_member_run(store, &auth, team_run_id).ok();
            if !actor_is_host && actor_member_run.is_none() {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "message sender must be the exact Team Host or one active Team Member",
                    "team_run",
                    team_run_id,
                    None,
                ));
            }
            let team_revision = store
                .teams()?
                .into_iter()
                .filter(|candidate| candidate.id == team.id)
                .count() as u64;
            if auth.expected_version != team_revision {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    "Team Message requires the exact current Team revision",
                    "team",
                    &team.id,
                    Some(team_revision),
                ));
            }
            let (
                recipient_ids,
                message_body,
                work_id,
                evidence_refs,
                response_required,
                correlation_id,
                causation_id,
                message_kind,
            ) = match (operation, intent) {
                (
                    "send",
                    RoleActionIntent::SendMessage {
                        recipient_ids,
                        body,
                        work_id,
                        evidence_refs,
                        response_required,
                    },
                ) => (
                    recipient_ids,
                    body,
                    work_id,
                    evidence_refs,
                    response_required,
                    deterministic_id("correlation", &auth),
                    None,
                    MessageKind::Message,
                ),
                (
                    "reply",
                    RoleActionIntent::ReplyMessage {
                        recipient_ids,
                        body,
                        correlation_id,
                        causation_id,
                        work_id,
                        evidence_refs,
                        response_required,
                    },
                ) => (
                    recipient_ids,
                    body,
                    work_id,
                    evidence_refs,
                    response_required,
                    correlation_id,
                    Some(causation_id),
                    MessageKind::Reply,
                ),
                (
                    "request-decision",
                    RoleActionIntent::RequestDecision {
                        body,
                        work_id,
                        evidence_refs,
                    },
                ) => (
                    vec![team.host_agent_id.clone()],
                    body,
                    work_id,
                    evidence_refs,
                    true,
                    deterministic_id("decision", &auth),
                    None,
                    MessageKind::RequestDecision,
                ),
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match message route",
                        "team_run",
                        team_run_id,
                        None,
                    ))
                }
            };
            if let Some(work_id) = work_id.as_deref() {
                let work = current_work(store, team_run_id, work_id)?;
                if !actor_is_host {
                    require_exact_work_member(store, &auth, &work)?;
                }
            }
            if message_body.trim().is_empty() || recipient_ids.is_empty() {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "message body and recipients are required",
                    "team_run",
                    team_run_id,
                    None,
                ));
            }
            let allowed = team
                .member_ids
                .iter()
                .chain(std::iter::once(&team.host_agent_id))
                .collect::<std::collections::BTreeSet<_>>();
            if recipient_ids.iter().any(|id| !allowed.contains(id)) {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "every message recipient must belong to the exact Team",
                    "team_run",
                    team_run_id,
                    None,
                ));
            }
            let memberships = store.fabric_team_memberships(&auth.execution_space_id)?;
            let subscriptions = store.fabric_message_subscriptions(&auth.execution_space_id)?;
            for recipient_id in &recipient_ids {
                let matching = memberships
                    .iter()
                    .filter(|membership| {
                        membership.team_id == team.id
                            && membership.agent_member_id == *recipient_id
                            && membership.state
                                == harness_core::agentfirm_api::TeamMembershipStatus::Active
                    })
                    .collect::<Vec<_>>();
                if matching.len() != 1
                    || !subscriptions.iter().any(|subscription| {
                        subscription.subscriber_kind
                            == harness_core::agentfirm_api::MessageSubjectKind::AgentMember
                            && subscription.subscriber_ref == *recipient_id
                            && subscription.membership_ref.as_deref()
                                == Some(matching[0].id.as_str())
                            && subscription.status
                                == harness_core::agentfirm_api::MessageSubscriptionStatus::Active
                    })
                {
                    return Err(encoded_error(
                        "MESSAGE_ROUTE_UNAVAILABLE",
                        "recipient requires one active canonical TeamMembership and MessageSubscription",
                        "agent_identity",
                        recipient_id,
                        None,
                    ));
                }
            }
            let member_runs = store
                .trust_member_runs(&auth.execution_space_id)?
                .into_iter()
                .filter(|run| run.team_run_id == team_run_id)
                .collect::<Vec<_>>();
            let recipient_runtime_ids = recipient_ids
                .into_iter()
                .map(|identity_id| {
                    if identity_id == team.host_agent_id {
                        Ok("host".to_string())
                    } else {
                        let matching = member_runs
                            .iter()
                            .filter(|run| {
                                run.agent_member_id == identity_id
                                    && run.coordination_status == MemberCoordinationStatus::Active
                            })
                            .collect::<Vec<_>>();
                        match matching.as_slice() {
                            [run] => Ok(run.id.clone()),
                            _ => Err(encoded_error(
                                "AGENT_SESSION_AMBIGUOUS",
                                "message recipient requires exactly one active Team Member",
                                "agent_identity",
                                &identity_id,
                                None,
                            )),
                        }
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            let sender = TeamActorRef {
                kind: if actor_is_host {
                    TeamActorKind::Host
                } else {
                    TeamActorKind::AgentMember
                },
                id: if actor_is_host {
                    team.host_agent_id.clone()
                } else {
                    auth.actor.id.clone()
                },
                display_name: None,
                authn_source: Some("agentfirm_http_credential".into()),
            };
            let compatibility_id = format!(
                "role-message:{}",
                canonical_json_fingerprint(&json!({
                    "actor": &auth.actor,
                    "idempotency_key": &auth.idempotency_key,
                }))
            );
            let message = harness_core::TeamMessageProjection {
                id: compatibility_id.clone(),
                team_run_id: team_run_id.to_string(),
                work_id,
                source_plan_ref: None,
                sender: Some(sender.clone()),
                sender_runtime_id: if actor_is_host {
                    "host".into()
                } else {
                    actor_member_run
                        .as_ref()
                        .cloned()
                        .unwrap_or_else(|| auth.actor.id.clone())
                },
                recipients: recipient_runtime_ids
                    .iter()
                    .map(|id| harness_core::TeamRecipientRef {
                        kind: harness_core::TeamRecipientKind::ProviderRuntimeProjection,
                        id: id.clone(),
                    })
                    .collect(),
                recipient_runtime_ids,
                kind: match message_kind {
                    MessageKind::RequestDecision => harness_core::ProviderDispatchIntent::Control,
                    _ => harness_core::ProviderDispatchIntent::Message,
                },
                body: message_body,
                correlation_id,
                causation_id,
                response_intent: Some(if response_required {
                    harness_core::ProviderResponseIntent::ResponseRequired
                } else {
                    harness_core::ProviderResponseIntent::Informational
                }),
                evidence_refs,
                deliveries: Vec::new(),
                created_at: now_string(),
            };
            let canonical_id = format!("message:{compatibility_id}");
            let replayed = store
                .fabric_messages(&auth.execution_space_id)?
                .iter()
                .any(|message| message.id == canonical_id);
            let published =
                crate::publish_team_message(store, &sender, message).map_err(|error| {
                    encoded_error(
                        "RUNTIME_COMMAND_REJECTED",
                        error.to_string(),
                        "message",
                        &canonical_id,
                        None,
                    )
                })?;
            let canonical = store
                .fabric_messages(&auth.execution_space_id)?
                .into_iter()
                .find(|message| message.id == published.id)
                .ok_or_else(|| {
                    encoded_error(
                        "RUNTIME_COMMAND_RECOVERY_REQUIRED",
                        "NodeDaemon returned without a canonical Message",
                        "message",
                        &canonical_id,
                        None,
                    )
                })?;
            let event = store
                .canonical_operations_for_space(&auth.execution_space_id)?
                .into_iter()
                .filter(|operation| {
                    operation.event.aggregate_kind == "message"
                        && operation.event.aggregate_id == canonical.id
                })
                .max_by_key(|operation| operation.event.sequence)
                .ok_or_else(|| {
                    encoded_error(
                        "RUNTIME_COMMAND_RECOVERY_REQUIRED",
                        "canonical Message event is missing",
                        "message",
                        &canonical.id,
                        None,
                    )
                })?
                .event;
            Ok(RoleActionResult {
                ok: true,
                action_protocol_version: "agentfirm.role_actions.v1",
                projection: serde_json::to_value(canonical)?,
                event_id: event.id,
                resulting_version: event.resulting_version,
                store_sequence: event.store_sequence,
                replayed,
            })
        }
        CanonicalRoute::MemberRun {
            member_run_id,
            operation,
        } => {
            let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid MemberRun intent: {error}"),
                    "member_run",
                    member_run_id,
                    None,
                )
            })?;
            let (run, _) = require_member_or_host(store, &auth, member_run_id)?;
            let required_confirmation = match operation {
                "close" => Some("close_member_run"),
                "retire" => Some("retire_member_run"),
                _ => None,
            };
            if required_confirmation.is_some_and(|required| confirmed_action != Some(required)) {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    format!(
                        "server confirmation must exactly confirm {}",
                        required_confirmation.unwrap_or_default()
                    ),
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ));
            }
            if let Some(replay) = canonical_replay(store, &auth, "member_run", member_run_id)? {
                return Ok(replay);
            }
            if auth.expected_version != run.version {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    "MemberRun action requires its exact current revision",
                    "member_run",
                    member_run_id,
                    Some(run.version),
                ));
            }
            let command = match (operation, intent) {
                ("close", RoleActionIntent::CloseMemberRun) => {
                    crate::agentfirm_api::TrustCommand::CloseMemberRun {
                        member_run_id: member_run_id.into(),
                        updated_at: now_string(),
                    }
                }
                ("reopen", RoleActionIntent::ReopenMemberRun) => {
                    crate::agentfirm_api::TrustCommand::ReopenMemberRun {
                        member_run_id: member_run_id.into(),
                        updated_at: now_string(),
                    }
                }
                ("retire", RoleActionIntent::RetireMemberRun) => {
                    crate::agentfirm_api::TrustCommand::RetireMemberRun {
                        member_run_id: member_run_id.into(),
                        updated_at: now_string(),
                    }
                }
                ("resume-native-session", RoleActionIntent::ResumeNativeSession) => {
                    crate::agentfirm_api::TrustCommand::ResumeNativeSession {
                        member_run_id: member_run_id.into(),
                        updated_at: now_string(),
                    }
                }
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match MemberRun route",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    ))
                }
            };
            Ok(trust_result(crate::agentfirm_api::execute(
                store, auth, command,
            )?))
        }
        CanonicalRoute::Workspace {
            member_run_id,
            operation,
        } => {
            let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid Workspace intent: {error}"),
                    "member_run",
                    member_run_id,
                    None,
                )
            })?;
            let (run, _) = require_member_or_host(store, &auth, member_run_id)?;
            if operation == "provision" {
                let RoleActionIntent::ProvisionWorkspace {
                    project_binding_id,
                    work_id,
                    mode,
                    ownership,
                    canonical_root,
                    base_ref,
                } = intent
                else {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match Workspace provision",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    ));
                };
                if let Some(replay) = canonical_replay(
                    store,
                    &auth,
                    "workspace_binding",
                    &deterministic_id("workspace", &auth),
                )? {
                    return Ok(replay);
                }
                if auth.expected_version != run.version {
                    return Err(encoded_error(
                        "VERSION_CONFLICT",
                        "Workspace provision requires the exact MemberRun revision",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    ));
                }
                let canonical = std::fs::canonicalize(&canonical_root).map_err(|error| {
                    encoded_error(
                        "WORKSPACE_UNSAFE",
                        format!("workspace path is not canonical/readable: {error}"),
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    )
                })?;
                let (team_run, team) = team_for_run(store, &run.team_run_id)?;
                if project_binding_id != team_run.project_binding_id
                    || !store
                        .latest_node_project_registrations()?
                        .into_iter()
                        .any(|registration| {
                            registration.node_id == team.node_id
                                && registration.execution_space_id == auth.execution_space_id
                                && registration.project_binding_id == project_binding_id
                                && registration.status
                                    == harness_core::NodeProjectRegistrationStatus::Active
                        })
                {
                    return Err(encoded_error("WORKSPACE_UNSAFE", "workspace project binding is not active on the Team's exact Node and Execution Space", "member_run", member_run_id, Some(run.version)));
                }
                let execution_root = team_run.execution_root.as_deref().ok_or_else(|| {
                    encoded_error(
                        "WORKSPACE_UNSAFE",
                        "TeamRun has no server-observed execution root",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    )
                })?;
                let canonical_execution_root =
                    std::fs::canonicalize(execution_root).map_err(|error| {
                        encoded_error(
                            "WORKSPACE_UNSAFE",
                            format!("TeamRun execution root cannot be canonicalized: {error}"),
                            "member_run",
                            member_run_id,
                            Some(run.version),
                        )
                    })?;
                if !canonical.starts_with(&canonical_execution_root) {
                    return Err(encoded_error(
                        "WORKSPACE_UNSAFE",
                        "workspace escapes the TeamRun execution-root boundary",
                        "member_run",
                        member_run_id,
                        Some(run.version),
                    ));
                }
                let git_value = |args: &[&str]| {
                    std::process::Command::new("git")
                        .arg("-C")
                        .arg(&canonical)
                        .args(args)
                        .output()
                        .ok()
                        .filter(|output| output.status.success())
                        .and_then(|output| String::from_utf8(output.stdout).ok())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                };
                let git_common_dir = git_value(&["rev-parse", "--git-common-dir"])
                    .and_then(|value| {
                        let path = std::path::PathBuf::from(value);
                        std::fs::canonicalize(if path.is_absolute() {
                            path
                        } else {
                            canonical.join(path)
                        })
                        .ok()
                    })
                    .map(|path| path.to_string_lossy().into_owned());
                let git_head = git_value(&["rev-parse", "HEAD"]);
                let git_branch = git_value(&["branch", "--show-current"]);
                let mut binding = MemberWorkspaceBinding {
                    id: deterministic_id("workspace", &auth),
                    project_binding_id,
                    team_run_id: run.team_run_id.clone(),
                    member_run_id: member_run_id.into(),
                    work_id,
                    mode,
                    ownership,
                    canonical_root: canonical.to_string_lossy().into_owned(),
                    git_common_dir,
                    base_ref,
                    git_head,
                    git_branch,
                    dirty_fingerprint: None,
                    instruction_roots: Vec::new(),
                    skill_roots: Vec::new(),
                    lifecycle: WorkspaceLifecycle::Requested,
                    blocked_reason: None,
                    attached_member_generation: None,
                    version: 1,
                    created_by: auth.actor.clone(),
                    created_at: now_string(),
                    updated_at: now_string(),
                };
                let proof = observe_workspace_proof(&binding, run.runtime_generation)?;
                if proof.is_dirty {
                    binding.dirty_fingerprint = Some(canonical_json_fingerprint(&json!({
                        "canonical_root": &binding.canonical_root,
                        "git_head": &binding.git_head,
                        "observed_dirty": true,
                    })));
                }
                let role_action_key = auth.idempotency_key.clone();
                let binding_id = binding.id.clone();
                let mut create_auth = auth.clone();
                create_auth.idempotency_key = format!("{role_action_key}:workspace-create");
                create_auth.expected_version = 0;
                crate::agentfirm_api::execute(
                    store,
                    create_auth,
                    crate::agentfirm_api::TrustCommand::ProvisionWorkspace { binding },
                )?;
                let mut prepare_auth = auth.clone();
                prepare_auth.idempotency_key = format!("{role_action_key}:workspace-prepare");
                prepare_auth.expected_version = 1;
                crate::agentfirm_api::execute(
                    store,
                    prepare_auth,
                    crate::agentfirm_api::TrustCommand::TransitionWorkspace {
                        member_run_id: member_run_id.into(),
                        binding_id: binding_id.clone(),
                        next: WorkspaceLifecycle::Preparing,
                        proof: proof.clone(),
                        updated_at: now_string(),
                    },
                )?;
                auth.expected_version = 2;
                return Ok(trust_result(crate::agentfirm_api::execute(
                    store,
                    auth,
                    crate::agentfirm_api::TrustCommand::TransitionWorkspace {
                        member_run_id: member_run_id.into(),
                        binding_id,
                        next: WorkspaceLifecycle::Ready,
                        proof,
                        updated_at: now_string(),
                    },
                )?));
            }
            let binding = latest_workspace(store, &auth.execution_space_id, member_run_id)?;
            if let Some(replay) = canonical_replay(store, &auth, "workspace_binding", &binding.id)?
            {
                return Ok(replay);
            }
            if auth.expected_version != binding.version {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    "Workspace transition requires the exact binding revision",
                    "workspace_binding",
                    &binding.id,
                    Some(binding.version),
                ));
            }
            let next = match (operation, intent) {
                ("attach", RoleActionIntent::AttachWorkspace) => WorkspaceLifecycle::Attached,
                ("archive", RoleActionIntent::ArchiveWorkspace) => WorkspaceLifecycle::Archived,
                ("cleanup", RoleActionIntent::CleanupWorkspace) => WorkspaceLifecycle::Removed,
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match Workspace route",
                        "workspace_binding",
                        &binding.id,
                        Some(binding.version),
                    ))
                }
            };
            if matches!(next, WorkspaceLifecycle::Removed)
                && confirmed_action != Some("cleanup_workspace")
            {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    "server confirmation must exactly confirm cleanup_workspace",
                    "workspace_binding",
                    &binding.id,
                    Some(binding.version),
                ));
            }
            let proof = observe_workspace_proof(&binding, run.runtime_generation)?;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::TransitionWorkspace {
                    member_run_id: member_run_id.into(),
                    binding_id: binding.id,
                    next,
                    proof,
                    updated_at: now_string(),
                },
            )?))
        }
        CanonicalRoute::WorkRecord {
            team_id,
            work_id,
            operation,
        } => execute_work_record_action(
            store,
            auth,
            team_id,
            work_id,
            operation,
            body,
            confirmed_action,
        ),
        CanonicalRoute::Gate {
            requirement_id,
            operation,
        } => execute_gate_action(
            store,
            auth,
            requirement_id,
            operation,
            body,
            confirmed_action,
        ),
        CanonicalRoute::Waiver { waiver_id } => {
            execute_waiver_revoke(store, auth, waiver_id, body, confirmed_action)
        }
        CanonicalRoute::MessageDelivery {
            node_id,
            delivery_id,
        } => {
            let intent = serde_json::from_slice::<OperatorActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid MessageDelivery intent: {error}"),
                    "message_delivery",
                    delivery_id,
                    None,
                )
            })?;
            let OperatorActionIntent::ReconcileMessageDelivery {
                outcome,
                evidence_ref,
            } = intent
            else {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "semantic action does not match MessageDelivery route",
                    "message_delivery",
                    delivery_id,
                    None,
                ));
            };
            if auth.actor.kind != ActorKind::Service || auth.actor.id != node_id {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "Operator must be the exact Execution Node Service",
                    "execution_node",
                    node_id,
                    None,
                ));
            }
            if confirmed_action != Some("reconcile_message_delivery") {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    "server confirmation must exactly confirm reconcile_message_delivery",
                    "message_delivery",
                    delivery_id,
                    None,
                ));
            }
            let lease = store
                .latest_node_daemon_lease(node_id)?
                .filter(|lease| {
                    lease.status == NodeDaemonLeaseStatus::Active
                        && lease.expires_unix_ms > crate::current_unix_ms_u64()
                })
                .ok_or_else(|| {
                    encoded_error(
                        "NODE_DAEMON_GENERATION_FENCED",
                        "MessageDelivery reconcile requires the exact current NodeDaemon",
                        "execution_node",
                        node_id,
                        None,
                    )
                })?;
            let daemon_actor = ActorRef {
                kind: ActorKind::Service,
                id: lease.daemon_id.clone(),
            };
            let context = MutationContext {
                execution_space_id: auth.execution_space_id,
                authenticated_actor: daemon_actor,
                authority_actor: Some(auth.actor.clone()),
                command_name: "node_daemon.message_delivery.reconcile".into(),
                idempotency_key: format!(
                    "role-message-reconcile:{}:{}",
                    auth.actor.id, auth.idempotency_key
                ),
                expected_version: auth.expected_version,
                request_fingerprint: auth.request_fingerprint,
            };
            canonical_mutation_result(store.reconcile_canonical_message_delivery(
                &context,
                delivery_id,
                node_id,
                &lease.daemon_id,
                lease.generation,
                outcome,
                &evidence_ref,
                &now_string(),
            )?)
        }
        CanonicalRoute::RuntimeRecovery {
            node_id,
            command_id,
        } => {
            let intent = serde_json::from_slice::<OperatorActionIntent>(body).map_err(|error| {
                encoded_error(
                    "INVALID_STATE_TRANSITION",
                    format!("invalid RuntimeCommand recovery intent: {error}"),
                    "runtime_command",
                    command_id,
                    None,
                )
            })?;
            let OperatorActionIntent::ResolveRuntimeRecovery {
                resolution,
                evidence_ref,
            } = intent
            else {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "semantic action does not match RuntimeCommand recovery route",
                    "runtime_command",
                    command_id,
                    None,
                ));
            };
            if auth.actor.kind != ActorKind::Service || auth.actor.id != node_id {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "RuntimeCommand recovery requires the exact Execution Node Operator",
                    "execution_node",
                    node_id,
                    None,
                ));
            }
            if confirmed_action != Some("resolve_runtime_recovery") {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    "server confirmation must exactly confirm resolve_runtime_recovery",
                    "runtime_command",
                    command_id,
                    None,
                ));
            }
            let lease = store
                .latest_node_daemon_lease(node_id)?
                .filter(|lease| {
                    lease.status == NodeDaemonLeaseStatus::Active
                        && lease.expires_unix_ms > crate::current_unix_ms_u64()
                })
                .ok_or_else(|| {
                    encoded_error(
                        "NODE_DAEMON_GENERATION_FENCED",
                        "RuntimeCommand recovery requires the exact current NodeDaemon",
                        "execution_node",
                        node_id,
                        None,
                    )
                })?;
            let context = MutationContext {
                execution_space_id: auth.execution_space_id,
                authenticated_actor: ActorRef {
                    kind: ActorKind::Service,
                    id: lease.daemon_id.clone(),
                },
                authority_actor: Some(auth.actor.clone()),
                command_name: "node_daemon.runtime_command.resolve".into(),
                idempotency_key: format!(
                    "role-runtime-recovery:{}:{}",
                    auth.actor.id, auth.idempotency_key
                ),
                expected_version: auth.expected_version,
                request_fingerprint: auth.request_fingerprint,
            };
            canonical_mutation_result(store.resolve_runtime_command_recovery(
                &context,
                command_id,
                node_id,
                &lease.daemon_id,
                lease.generation,
                resolution,
                &evidence_ref,
                &now_string(),
            )?)
        }
        CanonicalRoute::Operator { node_id, operation } => {
            execute_operator_action(store, auth, node_id, operation, body, confirmed_action)
        }
    }
}

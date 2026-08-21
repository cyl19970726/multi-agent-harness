use super::*;


pub(super) fn local_node_id_path() -> CliResult<PathBuf> {
    Ok(project::firm_home().map_err(project_err)?.join("NODE_ID"))
}

pub(super) fn generated_node_uuid() -> CliResult<String> {
    let mut bytes = [0u8; 16];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
    ))
}

pub(super) fn read_local_node_id() -> CliResult<String> {
    let path = local_node_id_path()?;
    let id = fs::read_to_string(&path).map_err(|error| {
        CliError::Usage(format!(
            "local ExecutionNode is not initialized at {}: {error}; run `firm node init`",
            path.display()
        ))
    })?;
    let id = id.trim().to_string();
    if id.is_empty() {
        return Err(CliError::Usage(format!(
            "local ExecutionNode identity at {} is empty",
            path.display()
        )));
    }
    Ok(id)
}

pub(super) fn ensure_local_node_id() -> CliResult<String> {
    let path = local_node_id_path()?;
    if path.exists() {
        return read_local_node_id();
    }
    let id = generated_node_uuid()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(id.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            Ok(id)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => read_local_node_id(),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn node_command(store: &HarnessStore, resolved: &ResolvedStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "node init|list|show|drain|retire|project")?;
    match args[0].as_str() {
        "init" => {
            let id = ensure_local_node_id()?;
            if let Some(existing) = store
                .latest_execution_nodes()?
                .into_iter()
                .find(|node| node.id == id)
            {
                print_json(&existing)?;
            } else {
                let now = now_string();
                let node = ExecutionNode {
                    id,
                    display_name: value(args, "--display-name")
                        .unwrap_or_else(|| "local-node".to_string()),
                    status: ExecutionNodeStatus::Active,
                    created_at: now.clone(),
                    updated_at: now,
                };
                store.insert_execution_node(&node)?;
                print_json(&node)?;
            }
        }
        "list" => print_json(&store.latest_execution_nodes()?)?,
        "show" => {
            let id = match value(args, "--id") {
                Some(id) => id,
                None => read_local_node_id()?,
            };
            let node = store
                .latest_execution_nodes()?
                .into_iter()
                .find(|node| node.id == id)
                .ok_or_else(|| CliError::Usage(format!("ExecutionNode not found: {id}")))?;
            print_json(&node)?;
        }
        "drain" | "retire" => {
            let id = match value(args, "--id") {
                Some(id) => id,
                None => read_local_node_id()?,
            };
            let target = if args[0] == "drain" {
                ExecutionNodeStatus::Draining
            } else {
                ExecutionNodeStatus::Retired
            };
            let current = store
                .latest_execution_nodes()?
                .into_iter()
                .find(|node| node.id == id)
                .ok_or_else(|| CliError::Usage(format!("ExecutionNode not found: {id}")))?;
            let mut next = current.clone();
            next.status = target;
            next.updated_at = now_string();
            store.transition_execution_node(&current, &next)?;
            print_json(&next)?;
        }
        "project" => {
            require_subcommand(args, "node project register|unregister")?;
            let node_id = match value(args, "--node-id") {
                Some(id) => id,
                None => read_local_node_id()?,
            };
            let execution_space_id = value(args, "--execution-space-id")
                .or_else(|| {
                    resolved
                        .execution_space_context
                        .as_ref()
                        .map(|space| space.id.clone())
                })
                .ok_or_else(|| {
                    CliError::Usage(
                        "node project requires --execution-space-id or a selected --space"
                            .to_string(),
                    )
                })?;
            let project_binding_id = required(args, "--project-binding-id")?;
            match args[1].as_str() {
                "register" => {
                    let now = now_string();
                    let registration = NodeProjectRegistration {
                        node_id,
                        execution_space_id,
                        project_binding_id,
                        status: NodeProjectRegistrationStatus::Active,
                        created_at: now.clone(),
                        updated_at: now,
                    };
                    let selected_space_id = resolved
                        .execution_space_context
                        .as_ref()
                        .map(|space| space.id.as_str())
                        .ok_or_else(|| {
                            CliError::Usage(
                                "node project mutation requires an explicitly selected --space"
                                    .to_string(),
                            )
                        })?;
                    store.register_node_project(&registration, selected_space_id)?;
                    print_json(&registration)?;
                }
                "unregister" => {
                    let current = store
                        .latest_node_project_registrations()?
                        .into_iter()
                        .find(|registration| {
                            registration.node_id == node_id
                                && registration.execution_space_id == execution_space_id
                                && registration.project_binding_id == project_binding_id
                        })
                        .ok_or_else(|| {
                            CliError::Usage("NodeProjectRegistration not found".to_string())
                        })?;
                    let mut disabled = current.clone();
                    disabled.status = NodeProjectRegistrationStatus::Disabled;
                    disabled.updated_at = now_string();
                    let selected_space_id = resolved
                        .execution_space_context
                        .as_ref()
                        .map(|space| space.id.as_str())
                        .ok_or_else(|| {
                            CliError::Usage(
                                "node project mutation requires an explicitly selected --space"
                                    .to_string(),
                            )
                        })?;
                    store.register_node_project(&disabled, selected_space_id)?;
                    print_json(&disabled)?;
                }
                other => {
                    return Err(CliError::Usage(format!(
                        "unknown node project command: {other}"
                    )))
                }
            }
        }
        other => return Err(CliError::Usage(format!("unknown node command: {other}"))),
    }
    Ok(())
}

pub(super) fn team_command(store: &HarnessStore, resolved: &ResolvedStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "team create|list|show|rename|add-member|remove-member|activate-member|activate|deactivate|trash|restore|message",
    )?;
    let execution_space_id = resolved
        .execution_space_context
        .as_ref()
        .map(|space| space.id.clone())
        .ok_or_else(|| {
            CliError::Usage("Team mutation/read requires an explicitly selected --space".into())
        })?;
    let actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Human,
        id: value(args, "--actor-id").unwrap_or_else(|| "operator:cli".into()),
    };
    let context = |command_name: &str, idempotency_key: String, expected_version: u64| {
        harness_core::agentfirm_api::MutationContext {
            execution_space_id: execution_space_id.clone(),
            authenticated_actor: actor.clone(),
            authority_actor: None,
            command_name: command_name.into(),
            idempotency_key,
            expected_version,
            request_fingerprint: None,
        }
    };
    match args[0].as_str() {
        "create" => {
            let host_agent_member_id = value(args, "--host-agent-member-id")
                .or_else(|| value(args, "--host-agent-id"))
                .ok_or_else(|| {
                    CliError::Usage("missing required option --host-agent-member-id".into())
                })?;
            let mut member_ids = many(args, "--member");
            member_ids.retain(|member_id| member_id != &host_agent_member_id);
            let legacy_mission_id =
                value(args, "--legacy-mission-id").or_else(|| value(args, "--mission-id"));
            let timestamp = now_string();
            let team = AgentTeam {
                id: value(args, "--id").unwrap_or_else(|| generated_id("team")),
                name: required(args, "--name")?,
                description: required(args, "--description")?,
                node_id: match value(args, "--node-id") {
                    Some(id) => id,
                    None => read_local_node_id()?,
                },
                status: AgentTeamStatus::Active,
                revision: 1,
                legacy_mission_id: legacy_mission_id.clone(),
                trashed_at: None,
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
                mission_id: legacy_mission_id.unwrap_or_default(),
                host_agent_id: host_agent_member_id.clone(),
                member_ids: member_ids.clone(),
            };
            let memberships = initial_team_memberships(
                &team,
                &host_agent_member_id,
                &member_ids,
                &actor,
                &timestamp,
            );
            let created = store.create_agent_team(
                &context(
                    "team.create",
                    value(args, "--idempotency-key")
                        .unwrap_or_else(|| format!("team-create:{}", team.id)),
                    0,
                ),
                team,
                memberships,
            )?;
            print_json(&created.projection)?;
        }
        "list" => {
            let teams = latest_teams(store)?
                .into_values()
                .filter(|team| has_flag(args, "--all") || team.status == AgentTeamStatus::Active)
                .collect::<Vec<_>>();
            print_json(&teams)?
        }
        "show" => {
            let id = required(args, "--id")?;
            let team = latest_teams(store)?
                .remove(&id)
                .ok_or_else(|| CliError::Usage(format!("team not found: {id}")))?;
            print_json(&team)?;
        }
        "rename" => {
            let id = required(args, "--id")?;
            let mut team = latest_teams(store)?
                .remove(&id)
                .ok_or_else(|| CliError::Usage(format!("team not found: {id}")))?;
            team.name = required(args, "--name")?;
            if let Some(description) = value(args, "--description") {
                team.description = description;
            }
            let updated = store.update_agent_team_profile(
                &context(
                    "team.profile.update",
                    value(args, "--idempotency-key")
                        .unwrap_or_else(|| format!("team-profile:{}:{}", team.id, team.revision)),
                    team.revision,
                ),
                &team.id,
                &team.name,
                &team.description,
                &now_string(),
            )?;
            print_json(&updated.projection)?;
        }
        "add-member" => {
            let id = required(args, "--id")?;
            let member_id = required(args, "--member")?;
            let team = latest_teams(store)?
                .remove(&id)
                .ok_or_else(|| CliError::Usage(format!("team not found: {id}")))?;
            if !known_agent_member_ids(store)?.contains(&member_id) {
                return Err(CliError::Usage(format!(
                    "agent member not found: {member_id}"
                )));
            }
            let prior = store.fabric_team_memberships(&execution_space_id)?;
            let membership_generation = prior
                .iter()
                .filter(|membership| {
                    membership.team_id == team.id && membership.agent_member_id == member_id
                })
                .map(|membership| membership.membership_generation)
                .max()
                .unwrap_or(0)
                + 1;
            let timestamp = now_string();
            let membership_id =
                value(args, "--membership-id").unwrap_or_else(|| generated_id("membership"));
            let role = match value(args, "--role").as_deref().unwrap_or("member") {
                "host" => harness_core::agentfirm_api::TeamMembershipRole::Host,
                "member" => harness_core::agentfirm_api::TeamMembershipRole::Member,
                "observer" => harness_core::agentfirm_api::TeamMembershipRole::Observer,
                other => {
                    return Err(CliError::Usage(format!(
                        "invalid --role {other}; expected host|member|observer"
                    )))
                }
            };
            let membership = harness_core::agentfirm_api::TeamMembership {
                id: membership_id.clone(),
                team_id: team.id,
                agent_member_id: member_id.clone(),
                node_id: team.node_id,
                role,
                state: harness_core::agentfirm_api::TeamMembershipStatus::Active,
                membership_generation,
                default_subscription_refs: vec![
                    format!("direct:{}:{}", member_id, membership_id),
                    format!("team:{}:{}", id, membership_id),
                ],
                created_by: actor.clone(),
                revision: 1,
                joined_at: timestamp,
                left_at: None,
            };
            let joined = store.join_team_membership(
                &context(
                    "team.membership.join",
                    value(args, "--idempotency-key")
                        .unwrap_or_else(|| format!("team-membership-join:{membership_id}")),
                    0,
                ),
                membership,
            )?;
            print_json(&joined.projection)?;
        }
        "remove-member" => {
            let id = required(args, "--id")?;
            let member_id = required(args, "--member")?;
            let membership = store
                .fabric_team_memberships(&execution_space_id)?
                .into_iter()
                .find(|membership| {
                    membership.team_id == id
                        && membership.agent_member_id == member_id
                        && membership.state
                            == harness_core::agentfirm_api::TeamMembershipStatus::Active
                })
                .ok_or_else(|| CliError::Usage("active TeamMembership not found".into()))?;
            let left = store.leave_team_membership(
                &context(
                    "team.membership.leave",
                    value(args, "--idempotency-key")
                        .unwrap_or_else(|| format!("team-membership-leave:{}", membership.id)),
                    membership.revision,
                ),
                &membership.id,
                &now_string(),
            )?;
            print_json(&left.projection)?;
        }
        "activate-member" => {
            let id = required(args, "--id")?;
            let member_id = required(args, "--member")?;
            let membership = store
                .fabric_team_memberships(&execution_space_id)?
                .into_iter()
                .find(|membership| {
                    membership.team_id == id
                        && membership.agent_member_id == member_id
                        && membership.state
                            == harness_core::agentfirm_api::TeamMembershipStatus::Inactive
                })
                .ok_or_else(|| CliError::Usage("inactive TeamMembership not found".into()))?;
            let activated = store.activate_team_membership(
                &context(
                    "team.membership.activate",
                    value(args, "--idempotency-key")
                        .unwrap_or_else(|| format!("team-membership-activate:{}", membership.id)),
                    membership.revision,
                ),
                &membership.id,
                &now_string(),
            )?;
            print_json(&activated.projection)?;
        }
        "activate" | "deactivate" | "close" | "trash" | "archive" | "restore" => {
            let id = required(args, "--id")?;
            let team = latest_teams(store)?
                .remove(&id)
                .ok_or_else(|| CliError::Usage(format!("team not found: {id}")))?;
            let next_status = match args[0].as_str() {
                "activate" => AgentTeamStatus::Active,
                "deactivate" | "close" | "restore" => AgentTeamStatus::Inactive,
                "trash" | "archive" => AgentTeamStatus::Trashed,
                _ => unreachable!(),
            };
            let transitioned = store.transition_agent_team(
                &context(
                    "team.lifecycle.transition",
                    value(args, "--idempotency-key")
                        .unwrap_or_else(|| format!("team-lifecycle:{}:{}", team.id, team.revision)),
                    team.revision,
                ),
                &team.id,
                next_status,
                &now_string(),
            )?;
            print_json(&transitioned.projection)?;
        }
        "message" => return team_message_command(store, &execution_space_id, &args[1..]),
        other => return Err(CliError::Usage(format!("unknown team command: {other}"))),
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Ordinary peer-Team messaging (DOC-106): flat Team->Team and
// Team->TeamMembership Messages without WorkDelegation. Team-addressed
// Messages land in one shared Team Inbox delivery that wakes no Member until
// one exact membership generation claims it; direct TeamMembership targets
// are bound at admission. Cross-Node targets ride the remote fabric; a
// same-Space same-Node target is delivered by the local authoring Store.
// ---------------------------------------------------------------------------

pub(super) fn team_message_command(
    store: &HarnessStore,
    execution_space_id: &str,
    args: &[String],
) -> CliResult<()> {
    require_subcommand(
        args,
        "team message send --from-team <id> --from-member <agent-member-id> --to-team <id> [--to-member <agent-member-id> | --to-membership <membership-id>] --body <markdown> | team message inbox --team <id> [--all] | team message claim --team <id> --delivery-id <id> --membership-id <id>",
    )?;
    match args[0].as_str() {
        "send" => team_message_send(store, execution_space_id, &args[1..]),
        "inbox" => team_message_inbox(store, execution_space_id, &args[1..]),
        "claim" => team_message_claim(store, execution_space_id, &args[1..]),
        other => Err(CliError::Usage(format!(
            "unknown team message command: {other}; expected send|inbox|claim"
        ))),
    }
}

pub(super) fn team_message_send(
    store: &HarnessStore,
    execution_space_id: &str,
    args: &[String],
) -> CliResult<()> {
    use harness_core::agentfirm_api::{
        MessageAddressKind, MessageKind, MessageRecipientKind, MessageRecipientRef, ResponseIntent,
    };

    let firm_home = execution_space::firm_home().map_err(execution_space_err)?;
    let local_node_id = read_local_node_id()?;
    let source_team_id = required(args, "--from-team")?;
    let from_member = required(args, "--from-member")?;
    let body = required(args, "--body")?;
    if body.trim().is_empty() {
        return Err(CliError::Usage("--body must be non-empty Markdown".into()));
    }
    let target_team_id = required(args, "--to-team")?;
    if value(args, "--to-member").is_some() && value(args, "--to-membership").is_some() {
        return Err(CliError::Usage(
            "--to-member and --to-membership are mutually exclusive".into(),
        ));
    }
    // Remote route facts are all-or-nothing; a same-Space same-Node target is
    // authored and delivered locally with no fabric route.
    let remote_node = value(args, "--to-node");
    let remote_space = value(args, "--to-space");
    let remote_company = value(args, "--company");
    let remote_requested =
        remote_node.is_some() || remote_space.is_some() || remote_company.is_some();
    let target_store_for = |space_id: &str| -> CliResult<Option<HarnessStore>> {
        if space_id == execution_space_id {
            return Ok(Some(store.clone()));
        }
        Ok(execution_space::context_for_id(&firm_home, space_id)
            .map_err(execution_space_err)?
            .map(|space| HarnessStore::new(space.store_root)))
    };
    let recipient = if value(args, "--to-member").is_none()
        && value(args, "--to-membership").is_none()
    {
        MessageRecipientRef {
            kind: MessageRecipientKind::Team,
            id: target_team_id.clone(),
        }
    } else {
        // A direct target is resolved to its exact TeamMembership on the
        // target Node; the membership generation is never caller-invented.
        let member_id = if let Some(member) = value(args, "--to-member") {
            member
        } else {
            let membership_id = required(args, "--to-membership")?;
            let lookup_space = remote_space
                .clone()
                .unwrap_or_else(|| execution_space_id.to_string());
            let target_store = target_store_for(&lookup_space)?.ok_or_else(|| {
                CliError::Usage(format!(
                    "target Execution Space {lookup_space} is not registered on this Node; --to-membership requires local resolution"
                ))
            })?;
            let membership = target_store
                .fabric_team_memberships(&lookup_space)?
                .into_iter()
                .find(|membership| membership.id == membership_id)
                .ok_or_else(|| {
                    CliError::Usage(format!("target TeamMembership not found: {membership_id}"))
                })?;
            if membership.team_id != target_team_id {
                return Err(CliError::Usage(format!(
                    "TeamMembership {membership_id} does not belong to target Team {target_team_id}"
                )));
            }
            membership.agent_member_id
        };
        MessageRecipientRef {
            kind: MessageRecipientKind::AgentMember,
            id: member_id,
        }
    };
    let work_id = value(args, "--work-id");
    let idempotency_key =
        value(args, "--idempotency-key").unwrap_or_else(|| generated_id("team-message"));
    let now_unix_ms = current_unix_ms_u64();
    let expires_unix_ms = match value(args, "--expires-unix-ms") {
        Some(raw) => raw
            .parse::<u64>()
            .map_err(|_| CliError::Usage("--expires-unix-ms must be an unsigned integer".into()))?,
        None => now_unix_ms.saturating_add(5 * 60_000),
    };
    let remote_transfer = if remote_requested {
        let (Some(company_id), Some(target_node_id), Some(target_space_id)) =
            (remote_company, remote_node, remote_space)
        else {
            return Err(CliError::Usage(
                "remote peer-Team routing requires --company, --to-node, and --to-space together"
                    .into(),
            ));
        };
        if target_node_id == local_node_id {
            return Err(CliError::Usage(
                "remote route facts must name a distinct target Node; drop them for a local same-Node target"
                    .into(),
            ));
        }
        let target_team_revision = match target_store_for(&target_space_id)? {
            Some(target_store) => target_store
                .agent_teams(&target_space_id)?
                .into_iter()
                .find(|team| team.id == target_team_id)
                .ok_or_else(|| {
                    CliError::Usage(format!(
                        "target Team {target_team_id} is not in target Execution Space {target_space_id}"
                    ))
                })?
                .revision,
            None => required(args, "--to-team-revision")?
                .parse::<u64>()
                .map_err(|_| {
                    CliError::Usage("--to-team-revision must be an unsigned integer".into())
                })?,
        };
        let target_subscription_revision = match value(args, "--to-subscription-revision") {
            Some(raw) => Some(raw.parse::<u64>().map_err(|_| {
                CliError::Usage("--to-subscription-revision must be an unsigned integer".into())
            })?),
            None => None,
        };
        Some(fabric_runtime::QueueCollaborationMessageRequest {
            company_id,
            target_team_id: target_team_id.clone(),
            target_team_revision,
            target_node_id,
            target_execution_space_id: target_space_id,
            target_subscription_revision,
            expected_delegation_revision: 0,
            expires_unix_ms,
        })
    } else {
        None
    };
    let draft = harness_core::agentfirm_api::MessageDraft {
        address_kind: match recipient.kind {
            MessageRecipientKind::Team => MessageAddressKind::TeamChannel,
            _ => MessageAddressKind::DirectAgent,
        },
        target_ref: recipient.clone(),
        recipients: vec![recipient],
        team_id: Some(source_team_id.clone()),
        team_run_id: None,
        work_id,
        collaboration_scope: Some(harness_core::collaboration::CollaborationScope {
            source_team_id: source_team_id.clone(),
            target_team_id: target_team_id.clone(),
            delegation_id: None,
            expected_delegation_revision: None,
            source_work_ref: None,
            target_work_ref: None,
        }),
        kind: if value(args, "--causation-id").is_some() {
            MessageKind::Reply
        } else {
            MessageKind::Message
        },
        body,
        correlation_id: value(args, "--correlation-id")
            .unwrap_or_else(|| generated_id("correlation")),
        causation_id: value(args, "--causation-id"),
        response_intent: if has_flag(args, "--response-required") {
            ResponseIntent::ResponseRequired
        } else {
            ResponseIntent::Informational
        },
        evidence_refs: many(args, "--evidence-ref"),
        schema_version: 1,
    };
    let actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::AgentMember,
        id: from_member.clone(),
    };
    let resolved_peer = resolve_peer_team_message_admission_authority(
        store,
        &firm_home,
        execution_space_id,
        &local_node_id,
        &actor,
        &draft,
        remote_transfer.as_ref(),
    )
    .map_err(CliError::Usage)?;
    let lease = store
        .latest_node_daemon_lease(&local_node_id)?
        .filter(|lease| {
            lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| CliError::Usage("NODE_DAEMON_UNAVAILABLE".into()))?;
    let payload = serde_json::json!({
        "draft": draft,
        "remote_transfer": remote_transfer,
        "message_admission_authority":
            harness_core::collaboration::MessageAdmissionAuthority::PeerTeam(resolved_peer.authority.clone()),
        "delegation_authority": serde_json::Value::Null,
    });
    let command = harness_core::agentfirm_api::ControlCommandEnvelope {
        id: format!("runtime-command:{idempotency_key}"),
        execution_space_id: execution_space_id.to_string(),
        target_node_id: local_node_id.clone(),
        target_node_daemon_id: lease.daemon_id.clone(),
        target_node_daemon_generation: lease.generation,
        authenticated_actor: actor.clone(),
        command: harness_core::agentfirm_api::RuntimeCommandKind::AuthorMessage,
        required_capability: "message.author".into(),
        idempotency_key: idempotency_key.clone(),
        expected_version: 0,
        expires_unix_ms,
        binding: Default::default(),
        precondition: Default::default(),
        postcondition: runtime_command_postcondition_for(
            harness_core::agentfirm_api::RuntimeCommandKind::AuthorMessage,
        ),
        payload_fingerprint: harness_store::canonical_json_fingerprint(&payload),
        payload,
        // The replay fingerprint binds the immutable request, not a sampled
        // wall clock; real accepted/settled timestamps live on the durable
        // RuntimeCommand record.
        issued_at: format!("runtime-command:{idempotency_key}"),
    };
    let response =
        supervisor_daemon::runtime_command_via_socket(&firm_home, &local_node_id, &command)?;
    if response["ok"].as_bool() != Some(true) {
        let daemon_error = response["error"].as_str().unwrap_or("unknown error");
        // A resubmitted key arrives with a fresh expiry and therefore a new
        // envelope fingerprint. Read the original accepted Message back and
        // return it only when the recorded semantics are byte-identical to
        // this intent; genuine semantic drift stays a hard conflict.
        if daemon_error.contains("IDEMPOTENCY_KEY_REUSED") {
            let message_id = format!("message:{idempotency_key}");
            let original = store
                .fabric_messages(execution_space_id)?
                .into_iter()
                .find(|message| message.id == message_id);
            let draft_fingerprint = |message: &harness_core::agentfirm_api::Message| {
                harness_store::canonical_json_fingerprint(&serde_json::json!({
                    "sender_actor_ref": message.sender_actor_ref,
                    "sender_agent_member_id": message.sender_agent_member_id,
                    "address_kind": message.address_kind,
                    "target_ref": message.target_ref,
                    "recipients": message.recipients,
                    "team_id": message.team_id,
                    "work_id": message.work_id,
                    "collaboration_scope": message.collaboration_scope,
                    "kind": message.kind,
                    "body": message.body,
                    "correlation_id": message.correlation_id,
                    "causation_id": message.causation_id,
                    "response_intent": message.response_intent,
                    "evidence_refs": message.evidence_refs,
                }))
            };
            if let Some(original) = original {
                let intended = draft_fingerprint(&original);
                let draft_as_message_intent =
                    harness_store::canonical_json_fingerprint(&serde_json::json!({
                        "sender_actor_ref": actor,
                        "sender_agent_member_id": Some(actor.id.clone()),
                        "address_kind": draft.address_kind,
                        "target_ref": draft.target_ref,
                        "recipients": draft.recipients,
                        "team_id": draft.team_id,
                        "work_id": draft.work_id,
                        "collaboration_scope": draft.collaboration_scope,
                        "kind": draft.kind,
                        "body": draft.body,
                        "correlation_id": draft.correlation_id,
                        "causation_id": draft.causation_id,
                        "response_intent": draft.response_intent,
                        "evidence_refs": draft.evidence_refs,
                    }));
                if intended == draft_as_message_intent {
                    // Authoring replayed; the remote route is (re)queued
                    // idempotently so a prior crash between author and queue
                    // still reaches the target Node exactly once.
                    if resolved_peer.requires_remote_route {
                        let request =
                            remote_transfer.expect("remote route requires its request facts");
                        let queued = fabric_runtime::queue_collaboration_message(
                            &firm_home,
                            execution_space_id,
                            &local_node_id,
                            &actor,
                            &idempotency_key,
                            &original,
                            &request,
                            harness_core::collaboration::MessageAdmissionAuthority::PeerTeam(
                                resolved_peer.authority.clone(),
                            ),
                            current_unix_ms_u64(),
                        )
                        .map_err(|error| {
                            CliError::Usage(format!("REMOTE_ROUTE_FAILED: {}", error.message))
                        })?;
                        return print_json(&serde_json::json!({
                            "message": original,
                            "remote_transfer": queued,
                            "replayed": true,
                        }));
                    }
                    let deliveries = store
                        .fabric_message_deliveries(execution_space_id)?
                        .into_iter()
                        .filter(|delivery| delivery.message_id == original.id)
                        .collect::<Vec<_>>();
                    return print_json(&serde_json::json!({
                        "message": original,
                        "deliveries": deliveries,
                        "replayed": true,
                    }));
                }
            }
        }
        return Err(CliError::Usage(format!(
            "NodeDaemon rejected the peer-Team Message: {daemon_error}"
        )));
    }
    let message =
        serde_json::from_value::<harness_core::agentfirm_api::Message>(response["result"].clone())?;
    if resolved_peer.requires_remote_route {
        let request = remote_transfer.expect("remote route requires its request facts");
        let queued = fabric_runtime::queue_collaboration_message(
            &firm_home,
            execution_space_id,
            &local_node_id,
            &actor,
            &idempotency_key,
            &message,
            &request,
            harness_core::collaboration::MessageAdmissionAuthority::PeerTeam(
                resolved_peer.authority.clone(),
            ),
            current_unix_ms_u64(),
        )
        .map_err(|error| CliError::Usage(format!("REMOTE_ROUTE_FAILED: {}", error.message)))?;
        return print_json(&serde_json::json!({
            "message": message,
            "admission": resolved_peer.authority,
            "remote_transfer": queued,
        }));
    }
    let deliveries = store
        .fabric_message_deliveries(execution_space_id)?
        .into_iter()
        .filter(|delivery| delivery.message_id == message.id)
        .collect::<Vec<_>>();
    print_json(&serde_json::json!({
        "message": message,
        "admission": resolved_peer.authority,
        "deliveries": deliveries,
    }))
}

/// Shared Team Inbox read: Team-subject canonical deliveries joined with their
/// immutable Messages. This is a projection only; the delivery state machine
/// is never mutated by a read.
pub(crate) fn team_inbox_projection(
    store: &HarnessStore,
    execution_space_id: &str,
    team_id: &str,
    include_all: bool,
) -> CliResult<serde_json::Value> {
    let subscription_id = format!("team-inbox:{team_id}");
    let subscription = store
        .fabric_message_subscriptions(execution_space_id)?
        .into_iter()
        .find(|subscription| subscription.id == subscription_id)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "Team {team_id} has no durable Team Inbox subscription"
            ))
        })?;
    let messages = store
        .fabric_messages(execution_space_id)?
        .into_iter()
        .map(|message| (message.id.clone(), message))
        .collect::<BTreeMap<_, _>>();
    let mut deliveries = store
        .fabric_message_deliveries(execution_space_id)?
        .into_iter()
        .filter(|delivery| {
            delivery.subscription_id == subscription_id
                && delivery.recipient_kind == harness_core::agentfirm_api::MessageSubjectKind::Team
                && delivery.target_team_id.as_deref() == Some(team_id)
        })
        .collect::<Vec<_>>();
    deliveries.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let items = deliveries
        .into_iter()
        .filter(|delivery| {
            include_all
                || matches!(
                    delivery.status,
                    harness_core::agentfirm_api::CanonicalMessageDeliveryStatus::Queued
                )
        })
        .map(|delivery| {
            let message = messages.get(&delivery.message_id);
            serde_json::json!({
                "delivery_id": delivery.id,
                "delivery_version": delivery.version,
                "delivery_status": delivery.status,
                "attempt": delivery.attempt,
                "claim_id": delivery.claim_id,
                "claimed_node_daemon_generation": delivery.claimed_node_daemon_generation,
                "resolved_team_membership_id": delivery.resolved_team_membership_id,
                "recipient_agent_member_id": delivery.recipient_agent_member_id,
                "subscription_id": delivery.subscription_id,
                "subscription_revision": delivery.subscription_revision,
                "message_id": delivery.message_id,
                "message": message.map(|message| serde_json::json!({
                    "kind": message.kind,
                    "body": message.body,
                    "body_digest": message.body_digest,
                    "content_fingerprint": message.content_fingerprint,
                    "sender_actor_ref": message.sender_actor_ref,
                    "sender_agent_member_id": message.sender_agent_member_id,
                    "sender_session_id": message.sender_session_id,
                    "source_team_id": message.team_id,
                    "source_execution_space_id": message.source_execution_space_id,
                    "source_node_id": message.source_node_id,
                    "collaboration_scope": message.collaboration_scope,
                    "correlation_id": message.correlation_id,
                    "causation_id": message.causation_id,
                    "work_id": message.work_id,
                    "response_intent": message.response_intent,
                    "evidence_refs": message.evidence_refs,
                    "created_at": message.created_at,
                })),
                "created_at": delivery.created_at,
                "updated_at": delivery.updated_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "team_id": team_id,
        "subscription": subscription,
        "item_count": items.len(),
        "items": items,
    }))
}

pub(super) fn team_message_inbox(
    store: &HarnessStore,
    execution_space_id: &str,
    args: &[String],
) -> CliResult<()> {
    let team_id = required(args, "--team")?;
    let team = store
        .agent_teams(execution_space_id)?
        .into_iter()
        .find(|team| team.id == team_id)
        .ok_or_else(|| CliError::Usage(format!("team not found: {team_id}")))?;
    let inbox =
        team_inbox_projection(store, execution_space_id, &team.id, has_flag(args, "--all"))?;
    if has_flag(args, "--json") {
        print_json(&inbox)?;
    } else {
        for item in inbox["items"].as_array().into_iter().flatten() {
            let sender = item["message"]["sender_agent_member_id"]
                .as_str()
                .unwrap_or("unknown");
            let source_team = item["message"]["collaboration_scope"]["source_team_id"]
                .as_str()
                .or_else(|| item["message"]["source_team_id"].as_str())
                .unwrap_or("unknown");
            let first_line = item["message"]["body"]
                .as_str()
                .and_then(|body| body.lines().next())
                .unwrap_or_default();
            println!(
                "{}\t{}\tfrom={}@{}\t{}\t{}",
                item["delivery_id"].as_str().unwrap_or_default(),
                item["delivery_status"].as_str().unwrap_or("unknown"),
                sender,
                source_team,
                item["message"]["correlation_id"]
                    .as_str()
                    .unwrap_or_default(),
                first_line,
            );
        }
    }
    Ok(())
}

pub(super) fn team_message_claim(
    store: &HarnessStore,
    execution_space_id: &str,
    args: &[String],
) -> CliResult<()> {
    let team_id = required(args, "--team")?;
    let delivery_id = required(args, "--delivery-id")?;
    let membership_id = required(args, "--membership-id")?;
    let team = store
        .agent_teams(execution_space_id)?
        .into_iter()
        .find(|team| team.id == team_id)
        .ok_or_else(|| CliError::Usage(format!("team not found: {team_id}")))?;
    let membership = store
        .fabric_team_memberships(execution_space_id)?
        .into_iter()
        .find(|membership| {
            membership.id == membership_id
                && membership.team_id == team.id
                && membership.state == harness_core::agentfirm_api::TeamMembershipStatus::Active
        })
        .ok_or_else(|| {
            CliError::Usage(format!("active TeamMembership not found: {membership_id}"))
        })?;
    let lease = store
        .latest_node_daemon_lease(&team.node_id)?
        .filter(|lease| {
            lease.status == NodeDaemonLeaseStatus::Active
                && lease.expires_unix_ms > current_unix_ms_u64()
        })
        .ok_or_else(|| CliError::Usage("NODE_DAEMON_UNAVAILABLE".into()))?;
    let claim_id = value(args, "--claim-id").unwrap_or_else(|| generated_id("team-inbox-claim"));
    let claim = harness_core::agentfirm_api::TeamMessageDeliveryClaim {
        claim_id: claim_id.clone(),
        team_membership_id: membership.id.clone(),
        membership_generation: membership.membership_generation,
        node_daemon_generation: lease.generation,
        claim_expires_at: format!(
            "unix-ms:{}",
            current_unix_ms_u64().saturating_add(15 * 60_000)
        ),
    };
    let claimed = store.claim_team_message_delivery(
        &canonical_delivery_context(
            execution_space_id,
            &lease.daemon_id,
            "node_daemon.team_message.claim",
            claim_id,
            0,
        ),
        &delivery_id,
        &claim,
        &now_string(),
    )?;
    print_json(&claimed.projection)
}

// ---------------------------------------------------------------------------
// Mission + append-only Mission Log — current product-control surfaces.
// ---------------------------------------------------------------------------

pub(super) fn mission_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "mission list|show|log show (read-only legacy; writers retired by DOC-108)",
    )?;
    match args[0].as_str() {
        "log" => return mission_log_command(store, &args[1..]),
        command if retired_mission_write_command(command) => {
            return Err(retired_mission_write_error(command));
        }
        "list" => print_json(&store.latest_missions()?)?,
        "show" => {
            let id = required(args, "--id")?;
            let mission = store
                .latest_missions()?
                .into_iter()
                .find(|mission| mission.id == id)
                .ok_or_else(|| CliError::Usage(format!("mission not found: {id}")))?;
            print_json(&mission)?;
        }
        other => return Err(CliError::Usage(format!("unknown mission command: {other}"))),
    }
    Ok(())
}

/// `mission log show` — read-only legacy read of the append-only Mission Log
/// (ADR 0051). `append` was retired with the legacy CompanyOS cutover
/// (DOC-108): the log is historical provenance, never new current authority.
pub(super) fn mission_log_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "mission log show (append retired by DOC-108)")?;
    let json = has_flag(args, "--json");
    match args[0].as_str() {
        "append" => {
            return Err(retired_mission_write_error("log-append"));
        }
        "show" => {
            let mission_id = required(args, "--mission-id")?;
            let tail = value(args, "--tail")
                .map(|raw| {
                    raw.parse::<usize>().map_err(|_| {
                        CliError::Usage("--tail must be a positive integer".to_string())
                    })
                })
                .transpose()?;
            let entries = match tail {
                Some(n) => store.mission_log_tail(&mission_id, n)?,
                None => store.mission_log_entries(&mission_id)?,
            };
            if json {
                print_json(&entries)?;
            } else {
                println!("{}", format_mission_log_entries_text(&entries));
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown mission log command: {other}"
            )))
        }
    }
    Ok(())
}

/// Render Mission Log entries as plain text for a terminal reader — the
/// non-JSON `mission log show` output and the mandatory-reader tail
/// `team-run recover` prints before its recovery report. Entries are
/// rendered in the order given (oldest-of-the-slice first, matching
/// `HarnessStore::mission_log_tail`'s Unix-`tail` ordering); an empty slice
/// renders the explicit sentinel so a reader is never left wondering whether
/// the read failed or the Mission simply has no Log yet.
pub(super) fn format_mission_log_entries_text(entries: &[MissionLogEntry]) -> String {
    if entries.is_empty() {
        return "no mission log yet".to_string();
    }
    entries
        .iter()
        .map(|entry| {
            format!(
                "#{} [{}] {} @ {}\n{}",
                entry.revision,
                serde_snake_label(&entry.kind),
                entry.actor,
                entry.created_at,
                entry.body
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
pub(super) fn legacy_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "legacy wave")?;
    match args[0].as_str() {
        "wave" => legacy_wave_command(store, &args[1..]),
        other => Err(CliError::Usage(format!(
            "unknown legacy command: {other}; expected `legacy wave`"
        ))),
    }
}

pub(super) fn legacy_wave_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "legacy wave list|show|history (read-only ADR 0051 compatibility)",
    )?;
    match args[0].as_str() {
        "list" => {
            let mission_id = value(args, "--mission-id");
            let waves = store
                .latest_legacy_waves()?
                .into_iter()
                .filter(|wave| {
                    mission_id
                        .as_deref()
                        .is_none_or(|mission_id| wave.mission_id == mission_id)
                })
                .collect::<Vec<_>>();
            print_json(&waves)?;
        }
        "show" => print_json(&latest_legacy_wave(store, &required(args, "--id")?)?)?,
        "history" => {
            let id = required(args, "--id")?;
            let history = store
                .legacy_waves()?
                .into_iter()
                .filter(|wave| wave.id == id)
                .collect::<Vec<_>>();
            if history.is_empty() {
                return Err(CliError::Usage(format!("wave not found: {id}")));
            }
            print_json(&history)?;
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown legacy wave command: {other}; only list|show|history are readable"
            )))
        }
    }
    Ok(())
}

/// Wave write commands retired by the ADR 0051 Mission Log cutover. `wave
/// list`/`show`/`history` remain functional as historical reads; `create`,
/// `update`, `advance`, and `gate` no longer accept a new write on ANY
/// surface — CLI, HTTP (`/v1/waves`...), or MCP (`wave_create`...) — so
/// there is exactly one place (this function plus [`retired_wave_write_error`])
/// that states the retirement, matching the top-level `retired_command`
/// shim the old Goal stack used.
pub(crate) fn retired_wave_write_command(command: &str) -> bool {
    matches!(command, "create" | "update" | "advance" | "gate")
}

pub(crate) fn retired_wave_write_error(command: &str) -> CliError {
    CliError::Usage(format!(
        "`harness wave {command}` was retired with the Mission Log cutover (ADR 0051), and the Mission Log writers that absorbed it were themselves retired with the legacy CompanyOS cutover (DOC-108). Current coordination uses durable AgentTeam, Team-run Work, and identity-first Message delivery. Historical rows are read only through `harness legacy wave list|show|history` and `harness legacy-company-os export|verify`."
    ))
}

/// Mission/Mission Log write commands retired by the DOC-108 legacy
/// CompanyOS cutover. Mirrors the Wave shim: `mission list|show|log show`
/// remain functional as read-only legacy reads; `create`, `update-context`,
/// `close`, and `log append` no longer accept a new write on ANY surface —
/// CLI, HTTP (`POST /v1/missions`...), or MCP (`mission_create`...) — so
/// there is exactly one place (this function plus
/// [`retired_mission_write_command`]) that states the retirement.
pub(crate) fn retired_mission_write_command(command: &str) -> bool {
    matches!(
        command,
        "create" | "update-context" | "close" | "log-append"
    )
}

pub(crate) fn retired_mission_write_error(command: &str) -> CliError {
    CliError::Usage(format!(
        "`harness mission {command}` was retired with the legacy CompanyOS cutover (DOC-108): Mission is historical provenance, not current authority, and its writers are closed on every surface. Current coordination uses durable AgentTeam (`harness team`), Team-run Work (`harness team-run work`), and identity-first Message delivery. Historical Mission rows stay read-only through `harness mission list|show|log show` and `harness legacy-company-os export|verify`."
    ))
}

/// The whole `harness company` surface retired with the DOC-108 legacy
/// CompanyOS cutover: the Company Store registry, Docs, Organization,
/// Approval, Finance, and gateway sub-commands are no longer current
/// authority. Work responsibility is Team-scoped (`harness team-run work`)
/// with the read-only Global Work aggregate at `harness work list|show`;
/// historical Company data is export/verify-only through
/// `harness legacy-company-os export|verify`.
pub(crate) fn retired_company_error(command: &str) -> CliError {
    CliError::Usage(format!(
        "`harness company {command}` was retired with the legacy CompanyOS cutover (DOC-108): the Company Store registry and its Docs/Organization/Approval/Finance writers and reads are closed. Use `harness team`, `harness team-run work`, the read-only Global Work aggregate `harness work list|show`, and `harness legacy-company-os export|verify` for historical data."
    ))
}

/// Read one Legacy Wave by id. Its write commands (`create`/`update`/
/// `advance`/`gate`) retired with the ADR 0051 Mission Log cutover; this
/// remains only for `legacy wave show` and historical row inspection.
pub(super) fn latest_legacy_wave(store: &HarnessStore, id: &str) -> CliResult<LegacyWave> {
    store
        .latest_legacy_waves()?
        .into_iter()
        .find(|wave| wave.id == id)
        .ok_or_else(|| CliError::Usage(format!("wave not found: {id}")))
}

// ---------------------------------------------------------------------------
// Agent Team v0 — `harness team-run` command group
//
// A team run (AgentTeamRun) is one execution of an agent team against an
// objective; MemberRuns are its per-member session rows, TeamMessages the
// routed mail, and TeamRunEvents the folded per-run event log (seq is
// monotonically increasing per run, assigned by the writer). All rows journal
// to their own append-only JSONL with latest-wins projection, like every
// other harness object. The CLI arms and the HTTP routes
// (POST /v1/team-runs[...]) share the create/send helpers below so behaviour
// cannot diverge (same pattern as the WP-ii entity helpers). The `start` arm
// is the v0 orchestrator (see the "team-run start orchestration" block below);
// create/send only journal planning rows — a handoff/blocker message sent via
// `send` is only folded into the event log, the ProviderRuntimeProjection row is untouched.
// ---------------------------------------------------------------------------

/// Next event seq for a team run: max existing seq + 1 (1 when the run has no
/// events yet). Scans the run's folded event log.
pub(super) fn next_team_run_seq(store: &HarnessStore, team_run_id: &str) -> CliResult<u64> {
    let max_seq = store
        .current_team_run_events(team_run_id)?
        .into_iter()
        .map(|event| event.seq)
        .max()
        .unwrap_or(0);
    Ok(max_seq + 1)
}

/// Append one folded event to a team run's event log. The store allocates the
/// authoritative sequence under its global lock; the caller-provided value is
/// retained only as a source-compatible hint for existing call sites.
#[allow(clippy::too_many_arguments)]
pub(super) fn append_team_run_event(
    store: &HarnessStore,
    team_run_id: &str,
    _seq: u64,
    source_kind: TeamRunEventSourceKind,
    member_run_id: Option<String>,
    entity_type: &str,
    entity_id: &str,
    operation: &str,
    summary: &str,
) -> CliResult<TeamRunEvent> {
    let event = TeamRunEvent {
        id: generated_id("trev"),
        seq: 0,
        team_run_id: team_run_id.to_string(),
        source_kind,
        member_run_id,
        delegation_run_id: None,
        entity_type: entity_type.to_string(),
        entity_id: entity_id.to_string(),
        operation: operation.to_string(),
        summary: summary.to_string(),
        occurred_at: now_string(),
    };
    Ok(store.append_team_run_event_next(event)?)
}

/// Append a work-transition team-run event. Thin wrapper around
/// `append_team_run_event` that extracts the team-run id from the Work
/// struct.
pub(super) fn append_work_event(
    store: &HarnessStore,
    work: &Work,
    source_kind: TeamRunEventSourceKind,
    member_run_id: Option<String>,
    operation: &str,
    summary: &str,
) -> CliResult<TeamRunEvent> {
    append_team_run_event(
        store,
        &work.team_run_id,
        0,
        source_kind,
        member_run_id,
        "work",
        &work.id,
        operation,
        summary,
    )
}

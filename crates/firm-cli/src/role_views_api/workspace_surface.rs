use super::*;

/// Shared Team Inbox (DOC-106): a read-only projection over the durable
/// `team-inbox:` MessageSubscription and its Team-subject canonical
/// deliveries, joined with the immutable Messages. Delivery status, claim
/// binding, correlation, and author/Team provenance are carried for the
/// operator surface; no delivery is mutated by this read.
pub(crate) fn team_inbox_view(
    space_id: &str,
    store: &HarnessStore,
    team_id: &str,
    query: &Query,
    identity: Option<&ReadIdentity>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let team = facts.teams.iter().find(|team| team.id == team_id).ok_or((
        "404 Not Found",
        "TEAM_NOT_FOUND",
        team_id.to_string(),
    ))?;
    let exact_host_identity = identity.is_some_and(|identity| {
        (identity.actor.kind == ActorKind::AgentMember && identity.actor.id == team.host_agent_id)
            || identity
                .authority_actors
                .iter()
                .any(|actor| actor.kind == ActorKind::AgentMember && actor.id == team.host_agent_id)
    });
    let team_member_identity = identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::AgentMember
            && (identity.actor.id == team.host_agent_id
                || team.member_ids.contains(&identity.actor.id))
    }) || exact_host_identity;
    if !team_member_identity {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "TeamInbox requires a Team-scoped AgentMember identity".into(),
        ));
    }
    let inbox = crate::team_inbox_projection(store, space_id, team_id, true).map_err(|e| {
        (
            "500 Internal Server Error",
            "ROLE_VIEW_BUILD_FAILED",
            e.to_string(),
        )
    })?;
    let mut items = inbox["items"].as_array().cloned().unwrap_or_default();
    items.truncate(query.limit);
    let data = json!({
        "team": {
            "team_id": team.id,
            "display_name": team.name,
            "team_revision": facts.team_revisions.get(&team.id).copied().unwrap_or(0),
            "mission_id": team.mission_id,
            "host_agent_id": team.host_agent_id,
            "node_id": team.node_id,
            "status": enum_string(&team.status),
        },
        "subscription": inbox["subscription"],
        "items": items,
        "page": {
            "as_of_event_sequence": facts.sequence,
            "item_count": items.len(),
            "next_cursor": null,
        },
    });
    Ok(envelope("team_inbox", &facts, data, vec![], vec![]))
}

pub(crate) fn agent_workspace_view(
    space_id: &str,
    store: &HarnessStore,
    route_ref: &str,
    query: &Query,
    identity: Option<&ReadIdentity>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let route_member_run = facts.member_runs.iter().find(|run| run["id"] == route_ref);
    let route_run = facts
        .runs
        .iter()
        .find(|run| run.id == route_ref)
        .or_else(|| {
            route_member_run
                .and_then(|member_run| member_run["team_run_id"].as_str())
                .and_then(|id| facts.runs.iter().find(|run| run.id == id))
        });
    let resolved_team_id = route_run
        .map(|run| run.agent_team_id.as_str())
        .unwrap_or(route_ref);
    let team = facts
        .teams
        .iter()
        .find(|team| team.id == resolved_team_id)
        .ok_or(("404 Not Found", "TEAM_NOT_FOUND", route_ref.to_string()))?;
    let run = route_run.or_else(|| facts.latest_run(resolved_team_id));
    let run_id = run.map(|run| run.id.as_str());
    let host_member_binding = run
        .map(|run| {
            store.host_member_binding(&run.id).map_err(|error| {
                (
                    "409 Conflict",
                    "HOST_RUNTIME_BINDING_INVALID",
                    error.to_string(),
                )
            })
        })
        .transpose()?;
    let host_runtime_binding = run.and_then(|run| {
        store
            .host_runtime_binding(&run.id, crate::current_unix_ms_u64())
            .ok()
    });
    let selected_agent_id = query
        .values
        .get("agent_id")
        .and_then(|values| values.first())
        .map(String::as_str)
        .or_else(|| route_member_run.and_then(|run| run["agent_member_id"].as_str()))
        .unwrap_or(team.host_agent_id.as_str());
    let selected_is_host = selected_agent_id == team.host_agent_id;
    let selected_is_member = team
        .member_ids
        .iter()
        .any(|member_id| member_id == selected_agent_id);
    if !selected_is_host && !selected_is_member {
        return Err((
            "404 Not Found",
            "AGENT_NOT_IN_TEAM",
            format!(
                "AgentMember {selected_agent_id} is not part of Team {}",
                team.id
            ),
        ));
    }
    let exact_host_identity =
        identity.is_some_and(|identity| identity.has_agent_member(&team.host_agent_id));
    let exact_selected_identity =
        identity.is_some_and(|identity| identity.has_agent_member(selected_agent_id));
    if !identity.is_some_and(|identity| identity.may_read_team(team)) {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "AgentWorkspace requires exact Team membership or same-machine local Operator authority"
                .into(),
        ));
    }
    let projection_scope = "team_session_read";

    // Reuse the bounded TeamWorkspace summaries; no browser-side ledger joins
    // or second Work/Message model is introduced by AgentWorkspace.
    let team_envelope = team_view(
        space_id,
        store,
        route_ref,
        false,
        identity,
        query.company.as_deref(),
    )?;
    let team_data = &team_envelope["data"];
    let all_works = team_data["works"].as_array().cloned().unwrap_or_default();
    let all_messages = team_data["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let selected_recipient_ids = facts
        .member_runs
        .iter()
        .filter(|member_run| member_run["agent_member_id"] == selected_agent_id)
        .filter_map(|member_run| member_run["id"].as_str())
        .chain(std::iter::once(selected_agent_id))
        .collect::<BTreeSet<_>>();
    let messages = all_messages
        .into_iter()
        .filter(|message| {
            message["sender"]["id"]
                .as_str()
                .is_some_and(|id| selected_recipient_ids.contains(id))
                || message["recipients"].as_array().is_some_and(|recipients| {
                    recipients.iter().any(|recipient| {
                        recipient["id"]
                            .as_str()
                            .is_some_and(|id| selected_recipient_ids.contains(id))
                    })
                })
        })
        .collect::<Vec<_>>();
    let works = all_works
        .into_iter()
        .filter(|work| {
            selected_is_host
                || work["owner_actor_ref"]["id"] == selected_agent_id
                || work["eligible_member_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id == selected_agent_id))
        })
        .collect::<Vec<_>>();
    let selected_work_ids = works
        .iter()
        .filter_map(|work| work["work_id"].as_str())
        .collect::<BTreeSet<_>>();

    let mut roster = team_data["members"].as_array().cloned().unwrap_or_default();
    roster.retain(|member| member["agent_member_ref"]["id"] != team.host_agent_id);
    let host_member = facts
        .members
        .iter()
        .find(|member| member["id"] == team.host_agent_id);
    roster.insert(
        0,
        json!({
            "agent_member_ref":{"kind":"agent_member","id":team.host_agent_id},
            "display_name":host_member.and_then(|member|member["name"].as_str()).unwrap_or("Host Agent"),
            "role":host_member.and_then(|member|member["role"].as_str()).unwrap_or("Host"),
            "organization_status":host_member.and_then(|member|member["organization_status"].as_str()).unwrap_or("active"),
            "coordination_status":host_member_binding.as_ref().map(|binding|enum_string(&binding.member_run.coordination_status)),
            "provider":host_member_binding.as_ref().map(|binding|binding.runtime.provider.as_str()),
            "model":null,
            "native_session_health":match host_runtime_binding.as_ref(){Some(harness_application::HostRuntimeBinding::Managed(binding))=>enum_string(&binding.agent_session.lifecycle),Some(harness_application::HostRuntimeBinding::ExternalInteractive(_))=>"external".to_string(),None=>"unbound".to_string()},
            "host_session_mode":host_session_mode(host_member_binding.as_ref()),
            "current_member_run_ref":host_member_binding.as_ref().map(|binding|binding.member_run.id.as_str()),
            "runtime_state":host_member_binding.as_ref().map(|binding|enum_string(&binding.runtime.status)),
            "runtime_generation":host_member_binding.as_ref().map(|binding|binding.member_run.runtime_generation),
            "capacity":"unknown",
            "active_work_count":0,
            "queued_work_count":0,
            "review_work_count":0,
            "blocked_work_count":0,
            "latest_action":null,
            "is_host":true,
        }),
    );
    for member in &mut roster {
        if let Some(object) = member.as_object_mut() {
            object.entry("is_host").or_insert(json!(false));
            let is_selected = object
                .get("agent_member_ref")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                == Some(selected_agent_id);
            for key in [
                "provider",
                "model",
                "native_session_health",
                "current_member_run_ref",
                "runtime_generation",
                "latest_action",
            ] {
                object.remove(key);
            }
            if !is_selected {
                object.remove("runtime_state");
            }
        }
    }

    let mut member_runs = facts
        .member_runs
        .iter()
        .filter(|member_run| member_run["agent_member_id"] == selected_agent_id)
        .filter(|member_run| {
            member_run["team_run_id"].as_str().is_some_and(|candidate| {
                facts.runs.iter().any(|candidate_run| {
                    candidate_run.id == candidate && candidate_run.agent_team_id == team.id
                })
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    member_runs.sort_by(|left, right| {
        right["started_at"]
            .as_str()
            .cmp(&left["started_at"].as_str())
            .then_with(|| {
                right["runtime_generation"]
                    .as_u64()
                    .cmp(&left["runtime_generation"].as_u64())
            })
    });
    // Host is an AgentMember and resolves through the same MemberRun selector.
    // External interactive Hosts still have a detached user-driven MemberRun,
    // but no unverifiable native session is fabricated for it.
    let selected_member_run = if selected_is_host {
        host_member_binding.as_ref().and_then(|binding| {
            member_runs
                .iter()
                .find(|member_run| member_run["id"] == binding.member_run.id)
        })
    } else {
        run_id.and_then(|current_run_id| {
            member_runs
                .iter()
                .find(|member_run| member_run["team_run_id"] == current_run_id)
        })
    }
    .or_else(|| member_runs.first());
    // Provider-native Session reads are Team-scoped. The application layer has
    // The AgentSession binding is coordination provenance only. Provider-native
    // content is exposed below exclusively to the same-machine loopback
    // Operator; remote RoleView credentials never become transcript grants.
    let current_agent_sessions = facts
        .agent_sessions
        .iter()
        .filter(|session| session["execution_space_id"] == space_id)
        .filter(|session| session["agent_member_id"] == selected_agent_id)
        .filter(|session| session["lifecycle"] != "closed")
        .collect::<Vec<_>>();
    let current_agent_session = if selected_is_host {
        host_runtime_binding
            .as_ref()
            .and_then(|binding| match binding {
                harness_application::HostRuntimeBinding::Managed(binding) => facts
                    .agent_sessions
                    .iter()
                    .find(|session| session["id"] == binding.agent_session.id),
                harness_application::HostRuntimeBinding::ExternalInteractive(_) => None,
            })
    } else {
        match current_agent_sessions.as_slice() {
            [session] => Some(*session),
            _ => None,
        }
    };
    // The Team-scoped historical projection is decoded on demand. It is
    // independent from the volatile live overlay and never enters a ledger.
    let session_before_position = query
        .values
        .get("session_before")
        .and_then(|values| values.first())
        .and_then(|value| value.parse::<u64>().ok());
    let session_page_limit = query
        .values
        .get("session_limit")
        .and_then(|values| values.first())
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(crate::provider_event_api::DEFAULT_SESSION_PAGE_SIZE);
    let local_native_session_read = identity.is_some_and(ReadIdentity::may_read_native_session);
    let session_event_projection = Some(if local_native_session_read {
        let project_binding_id = store
            .provider_compatibility_scope()
            .map(|(project_id, _)| project_id)
            .unwrap_or_default();
        read_session_event_projection(
            store,
            &facts,
            SessionProjectionReadRequest {
                execution_space_id: space_id,
                project_id: project_binding_id,
                team_id: &team.id,
                selected_agent_id,
                before_position: session_before_position,
                page_limit: session_page_limit,
                run,
                selected_member_run,
            },
        )
    } else {
        unavailable_session_event_projection_code(
            "local_operator_required",
            "Provider-native Session history is available only from the same-machine Dashboard.",
        )
    });
    // Only an exact MemberRun selector plus its current canonical AgentSession
    // can receive the process-local live overlay. MemberRun and AgentSession
    // generations are independent fences; detached external Hosts stay null.
    let project_binding_id = store
        .provider_compatibility_scope()
        .map(|(project_id, _)| project_id)
        .unwrap_or_default();
    let live_provider_activity = local_native_session_read
        .then(|| {
            selected_member_run
                .and_then(|member_run| {
                    let typed_member = serde_json::from_value(member_run.clone()).ok()?;
                    crate::provider_event_api::exact_live_scope(
                        store,
                        space_id,
                        project_binding_id,
                        member_run["team_run_id"].as_str()?,
                        &typed_member,
                    )
                    .ok()
                })
                .as_ref()
                .and_then(crate::provider_event_api::live_snapshot)
        })
        .flatten();
    let persisted_session_projection = current_agent_session
        .and_then(|session| {
            read_persisted_session_projection(
                store,
                space_id,
                team,
                run,
                selected_agent_id,
                session,
                query,
                identity,
            )
        })
        .unwrap_or_else(|| {
            json!({
                "schema_version": "agentfirm.native_session_read.v1",
                "available": false,
                "reason_code": "exact_session_unavailable"
            })
        });
    let selected_member = facts
        .members
        .iter()
        .find(|member| member["id"] == selected_agent_id);
    let selected_roster = roster
        .iter()
        .find(|member| member["agent_member_ref"]["id"] == selected_agent_id);
    let selected_member_run_id = selected_member_run.and_then(|run| run["id"].as_str());
    let workspace_binding = selected_member_run_id
        .and_then(|member_run_id| current_workspace(&facts, member_run_id))
        .map(|workspace| record_summary("workspace_binding", workspace));
    let configuration = json!({
        "description":selected_member.and_then(|member|member["description"].as_str()),
        "prompt_ref":null,
        "prompt_projection":"not_modeled",
        "skill_refs":selected_member.and_then(|member|member["skill_refs"].as_array()).cloned().unwrap_or_default(),
        "capabilities":selected_member.and_then(|member|member["capabilities"].as_array()).cloned().unwrap_or_default(),
        "tool_refs":[],
        "tools_projection":"not_modeled_by_agent_member",
        "provider_profile_ref":selected_member.and_then(|member|member["provider_profile_ref"].as_str()),
        "model_preference":selected_member.and_then(|member|member["model_preference"].as_str()),
        "workspace_policy":selected_member.and_then(|member|member["workspace_policy"].as_str()),
        "permission_ceiling":selected_member.and_then(|member|member["permission_ceiling"].as_str()),
        "effective_permission_ceiling":current_agent_session.and_then(|session|session["effective_permission_ceiling"].as_str()),
        "resolved_workspace_cwd":current_agent_session.and_then(|session|session["workspace_cwd"].as_str()).or_else(||selected_member_run.and_then(|member|member["provider_cwd_hint"].as_str())),
        "forbidden_actions":[],
        "forbidden_actions_projection":"not_modeled",
        "workspace_binding":workspace_binding,
    });

    let authority_envelope = if exact_host_identity {
        team_view(
            space_id,
            store,
            route_ref,
            true,
            identity,
            query.company.as_deref(),
        )?
    } else if exact_selected_identity {
        if let Some(member_run_id) = selected_member_run_id {
            member_view(
                space_id,
                store,
                member_run_id,
                identity,
                query.company.as_deref(),
            )?
        } else {
            json!({"allowed_actions":[]})
        }
    } else {
        // A loopback Operator may inspect the Team-scoped read model, but it
        // never borrows an AgentMember's authenticated mutation authority.
        json!({"allowed_actions":[]})
    };
    let allowed_actions = authority_envelope["allowed_actions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|action| match action["target_ref"]["kind"].as_str() {
            Some("team_run") => true,
            // Host control authority and Team-scoped provider observation are
            // separate planes. Mutation actions still require exact authority.
            Some("member_run") => {
                selected_member_run_id.is_some_and(|id| action["target_ref"]["id"] == id)
            }
            Some("work") => action["target_ref"]["id"]
                .as_str()
                .is_some_and(|id| selected_work_ids.contains(id)),
            _ => false,
        })
        .collect::<Vec<_>>();

    let selected_runtime_status = current_agent_session
        .and_then(|session| session["lifecycle"].as_str().map(str::to_owned))
        .or_else(|| {
            selected_member_run
                .and_then(|member_run| member_run["runtime_status"].as_str().map(str::to_owned))
                .or_else(|| run.map(|run| enum_string(&run.status)))
        });
    let native_session_open_target = selected_member_run
        .and_then(|member_run| {
            serde_json::from_value::<crate::ProviderRuntimeProjection>(member_run.clone()).ok()
        })
        .and_then(|member_run| crate::native_session_open_target(&member_run).ok());
    let current_session = current_agent_session.map(|session| {
        json!({
            "agent_session_id":session["id"],
            "agent_session_generation":session["runtime_generation"],
            "lifecycle":session["lifecycle"],
            "runtime_residency":session["control_state"]["runtime_residency"],
            "activity":session["control_state"]["activity"],
            "provider":session["provider_kind"],
            "effective_permission_ceiling":session["effective_permission_ceiling"],
            "workspace_cwd":session["workspace_cwd"],
            "native_session_ref":session["native_session_ref"],
            "native_session_open_target":native_session_open_target,
        })
    });
    let selected = json!({
        "agent_member_ref":{"kind":"agent_member","id":selected_agent_id},
        "display_name":selected_member.and_then(|member|member["name"].as_str()).or_else(||selected_roster.and_then(|member|member["display_name"].as_str())).unwrap_or(if selected_is_host{"Host Agent"}else{"Agent"}),
        "role":selected_member.and_then(|member|member["role"].as_str()).or_else(||selected_roster.and_then(|member|member["role"].as_str())).unwrap_or(if selected_is_host{"Host"}else{"Agent"}),
        "organization_status":selected_member.and_then(|member|member["organization_status"].as_str()).unwrap_or("unknown"),
        "is_host":selected_is_host,
        "current_member_run_ref":selected_member_run_id,
        "provider":current_agent_session.and_then(|session|session["provider_kind"].as_str()).or_else(||selected_member_run.and_then(|run|run["provider"].as_str())),
        "execution_mode":selected_member_run.and_then(|run|run["execution_mode"].as_str()),
        "runtime_status":selected_runtime_status,
        "runtime_generation":selected_member_run.and_then(|run|run["runtime_generation"].as_u64()),
        "host_session_mode":if selected_is_host {Some(host_session_mode(host_member_binding.as_ref()))} else {None},
    });
    let unread_count = messages
        .iter()
        .filter(|message| {
            message["deliveries"].as_array().is_some_and(|deliveries| {
                deliveries.iter().any(|delivery| {
                    matches!(delivery["status"].as_str(), Some("queued" | "delivered"))
                })
            })
        })
        .count();
    let safe_team = json!({
        "team_id":team_data["team"]["team_id"],
        "display_name":team_data["team"]["display_name"],
        "team_revision":team_data["team"]["team_revision"],
        "mission_id":team_data["team"]["mission_id"],
        "host_agent_id":team_data["team"]["host_agent_id"],
        "viewer_role":team_data["team"]["viewer_role"],
        "status":team_data["team"]["status"],
        "latest_run_id":team_data["team"]["latest_run"]["id"],
    });
    let current_work_id = works
        .iter()
        .filter(|work| selected_is_host || work["owner_actor_ref"]["id"] == selected_agent_id)
        .find(|work| work["phase"] == "active")
        .or_else(|| {
            works
                .iter()
                .filter(|work| {
                    selected_is_host || work["owner_actor_ref"]["id"] == selected_agent_id
                })
                .find(|work| work["phase"] == "review")
        })
        .or_else(|| {
            works
                .iter()
                .filter(|work| {
                    selected_is_host || work["owner_actor_ref"]["id"] == selected_agent_id
                })
                .find(|work| work["phase"] == "open")
        })
        .and_then(|work| work["work_id"].as_str());
    let mut response = envelope(
        "agent_workspace",
        &facts,
        json!({
            "projection_scope":projection_scope,
            "team":safe_team,
            "selected_agent":selected,
            "roster":roster,
            "session_event_projection":session_event_projection,
            "persisted_session_projection":persisted_session_projection,
            "current_session":current_session,
            "live_provider_activity":live_provider_activity,
            "messages":messages,
            "works":works,
            "configuration":configuration,
            "context_summary":{
                "current_work_id":current_work_id,
                "message_count":messages.len(),
                "unread_count":unread_count,
                "last_activity_at":selected_member_run.and_then(|member_run|member_run["last_event_at"].as_str()),
                "authorization_count":allowed_actions.iter().filter(|action|action["disabled_reason"].is_null()).count(),
            },
        }),
        vec![],
        allowed_actions,
    );
    response["data"]
        .as_object_mut()
        .expect("AgentWorkspace data object")
        .remove("runtime_fabric");
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
fn read_persisted_session_projection(
    store: &HarnessStore,
    space_id: &str,
    team: &AgentTeam,
    run: Option<&AgentTeamRun>,
    selected_agent_id: &str,
    session_value: &Value,
    query: &Query,
    identity: Option<&ReadIdentity>,
) -> Option<Value> {
    let run = run?;
    let identity = identity?;
    let session: harness_core::agentfirm_api::AgentSession =
        serde_json::from_value(session_value.clone()).ok()?;
    let native = session.native_session_ref.as_ref()?;
    let lease = store.latest_node_daemon_lease(&team.node_id).ok()??;
    let mode_and_cursor = if let Some(after) = query
        .values
        .get("session_after")
        .and_then(|values| values.first())
        .and_then(|value| value.parse::<u64>().ok())
    {
        Some((
            crate::provider_event_api::PersistedSessionReadMode::After,
            after,
        ))
    } else {
        query
            .values
            .get("session_before")
            .and_then(|values| values.first())
            .and_then(|value| value.parse::<u64>().ok())
            .map(|before| {
                (
                    crate::provider_event_api::PersistedSessionReadMode::Older,
                    before,
                )
            })
    };
    let source_generation = query
        .values
        .get("session_source_generation")
        .and_then(|values| values.first())
        .cloned();
    let (mode, cursor) = match mode_and_cursor {
        Some((mode, value)) => {
            let source_generation = source_generation?;
            (
                mode,
                Some(crate::provider_event_api::PersistedSessionCursor {
                    source_generation,
                    ordering_key: harness_provider_events::PersistedOrderingKey {
                        kind: harness_provider_events::OrderingKeyKind::CompleteRowEndOffset,
                        value,
                    },
                }),
            )
        }
        None => (
            crate::provider_event_api::PersistedSessionReadMode::Snapshot,
            None,
        ),
    };
    let request = crate::provider_event_api::PersistedSessionReadRequest {
        execution_space_id: space_id.into(),
        project_binding_id: run.project_binding_id.clone(),
        team_id: team.id.clone(),
        team_run_id: run.id.clone(),
        agent_member_id: selected_agent_id.into(),
        agent_session_id: session.id.clone(),
        agent_session_generation: session.runtime_generation,
        native_session_fingerprint: crate::provider_event_api::native_session_fingerprint(native)
            .ok()?,
        node_id: team.node_id.clone(),
        node_daemon_id: lease.daemon_id,
        node_daemon_generation: lease.generation,
        mode,
        cursor,
        limit: query
            .values
            .get("session_limit")
            .and_then(|values| values.first())
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(crate::provider_event_api::DEFAULT_SESSION_PAGE_SIZE),
        viewer: crate::provider_event_api::PersistedSessionViewer {
            actor: identity.actor.clone(),
            authority_actors: identity.authority_actors.clone(),
            local_operator: identity.local_operator,
        },
    };
    let firm_home = crate::execution_space::firm_home().ok()?;
    Some(
        match crate::supervisor_daemon::native_session_read_via_socket(
            &firm_home,
            &team.node_id,
            &request,
        ) {
            Ok(response) => {
                let mut value = serde_json::to_value(response).ok()?;
                value["available"] = Value::Bool(true);
                value
            }
            Err(error) => json!({
                "schema_version": "agentfirm.native_session_read.v1",
                "available": false,
                "reason_code": "node_daemon_read_unavailable",
                "detail": error.to_string(),
            }),
        },
    )
}

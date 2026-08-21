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
    let exact_host_identity = identity.is_some_and(|identity| {
        (identity.actor.kind == ActorKind::AgentMember && identity.actor.id == team.host_agent_id)
            || identity
                .authority_actors
                .iter()
                .any(|actor| actor.kind == ActorKind::AgentMember && actor.id == team.host_agent_id)
    });
    let exact_selected_identity = identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::AgentMember && identity.actor.id == selected_agent_id
    });
    if !(exact_host_identity || exact_selected_identity) {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "AgentWorkspace requires the exact selected AgentMember or this Team's exact Host authority"
                .into(),
        ));
    }
    if selected_is_host && !exact_host_identity {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "Host Agent Session is visible only to this Team's exact Host authority".into(),
        ));
    }
    let projection_scope = if selected_is_host {
        "host_self_private"
    } else if exact_selected_identity {
        "member_self_private"
    } else {
        "host_member_public"
    };

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
    let mut messages = all_messages
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
    let mut works = all_works
        .into_iter()
        .filter(|work| {
            selected_is_host
                || work["owner_actor_ref"]["id"] == selected_agent_id
                || work["eligible_member_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id == selected_agent_id))
        })
        .collect::<Vec<_>>();
    let public_unread_count = messages
        .iter()
        .filter(|message| {
            message["deliveries"].as_array().is_some_and(|deliveries| {
                deliveries.iter().any(|delivery| {
                    matches!(delivery["status"].as_str(), Some("queued" | "delivered"))
                })
            })
        })
        .count();
    if projection_scope == "host_member_public" {
        // Coordination content and responsibility are public to the exact Host,
        // but delivery receipts, runtime bindings, and workspace bindings are
        // execution-private. Redact them before the RoleView leaves the server.
        for message in &mut messages {
            message["deliveries"] = json!([]);
        }
        for work in &mut works {
            work["current_member_run_ref"] = Value::Null;
            work["runtime_summary"] = json!({
                "state":"not_projected",
                "generation":null,
                "freshness":"unknown",
            });
            work["workspace_summary"] = json!({
                "binding_id":null,
                "lifecycle":"not_projected",
                "safety":"unknown",
            });
        }
    }
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
            "coordination_status":run.map(|run|enum_string(&run.status)),
            "provider":run.map(|run|run.host_surface.clone()),
            "model":null,
            "native_session_health":if run.and_then(|run|run.host_thread_id.as_ref()).is_some(){"available"}else{"unknown"},
            "host_session_mode":host_session_mode(run),
            "current_member_run_ref":null,
            "runtime_state":run.map(|run|enum_string(&run.status)),
            "runtime_generation":null,
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
            if !is_selected || projection_scope == "host_member_public" {
                object.remove("runtime_state");
            }
            if projection_scope == "host_member_public" {
                // The public Host-selected surface is responsibility and
                // coordination only. Provider-derived or Member-private live
                // state is structurally absent, including roster rollups.
                object.remove("coordination_status");
                object.insert("coordination_status".into(), Value::Null);
                object.insert("capacity".into(), json!("not_projected"));
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
    let selected_member_run = if selected_is_host {
        None
    } else {
        run_id
            .and_then(|current_run_id| {
                member_runs
                    .iter()
                    .find(|member_run| member_run["team_run_id"] == current_run_id)
            })
            .or_else(|| member_runs.first())
    };
    // Provider-private Session data is owner-bound, not merely Team-authorized.
    // The exact Host can read the Host Session. The exact Member can read that
    // Member's Session. Host authority selecting a Member receives only public
    // coordination/Work facts and never that Member's native Session internals.
    let may_read_private_session =
        (selected_is_host && exact_host_identity) || (!selected_is_host && exact_selected_identity);
    // The owner-only historical projection is decoded on demand. It is
    // independent from the volatile live overlay and never enters a ledger.
    let viewer_identity_id = identity
        .map(|identity| identity.actor.id.as_str())
        .unwrap_or_default();
    let session_event_projection = may_read_private_session.then(|| {
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
                viewer_identity_id,
                run,
                selected_member_run,
            },
        )
    });
    // Only an exact MemberRun selector plus its current canonical AgentSession
    // can receive the process-local live overlay. MemberRun and AgentSession
    // generations are independent fences; Host runs without a MemberRun stay null.
    let live_provider_activity = if may_read_private_session {
        let project_binding_id = store
            .provider_compatibility_scope()
            .map(|(project_id, _)| project_id)
            .unwrap_or_default();
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
    } else {
        None
    };
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
        "provider_profile_ref":if may_read_private_session {selected_member.and_then(|member|member["provider_profile_ref"].as_str())} else {None},
        "model_preference":if may_read_private_session {selected_member.and_then(|member|member["model_preference"].as_str())} else {None},
        "workspace_policy":if may_read_private_session {selected_member.and_then(|member|member["workspace_policy"].as_str())} else {None},
        "permission_ceiling":if may_read_private_session {selected_member.and_then(|member|member["permission_ceiling"].as_str())} else {None},
        "forbidden_actions":[],
        "forbidden_actions_projection":"not_modeled",
        "workspace_binding":if may_read_private_session {workspace_binding} else {None},
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
    } else if let Some(member_run_id) = selected_member_run_id {
        member_view(
            space_id,
            store,
            member_run_id,
            identity,
            query.company.as_deref(),
        )?
    } else {
        json!({"allowed_actions":[]})
    };
    let allowed_actions = authority_envelope["allowed_actions"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|action| match action["target_ref"]["kind"].as_str() {
            Some("team_run") => true,
            // Host control authority and provider-private observation are
            // separate planes. A Host-selected public projection may expose
            // exact, server-authorized MemberRun controls without exposing the
            // Member's Session, runtime facts, or workspace binding.
            Some("member_run") => {
                selected_member_run_id.is_some_and(|id| action["target_ref"]["id"] == id)
            }
            Some("work") => action["target_ref"]["id"]
                .as_str()
                .is_some_and(|id| selected_work_ids.contains(id)),
            _ => false,
        })
        .collect::<Vec<_>>();

    let selected_runtime_status = selected_member_run
        .and_then(|member_run| member_run["runtime_status"].as_str().map(str::to_owned))
        .or_else(|| run.map(|run| enum_string(&run.status)));
    let selected = json!({
        "agent_member_ref":{"kind":"agent_member","id":selected_agent_id},
        "display_name":selected_member.and_then(|member|member["name"].as_str()).or_else(||selected_roster.and_then(|member|member["display_name"].as_str())).unwrap_or(if selected_is_host{"Host Agent"}else{"Agent"}),
        "role":selected_member.and_then(|member|member["role"].as_str()).or_else(||selected_roster.and_then(|member|member["role"].as_str())).unwrap_or(if selected_is_host{"Host"}else{"Agent"}),
        "organization_status":selected_member.and_then(|member|member["organization_status"].as_str()).unwrap_or("unknown"),
        "is_host":selected_is_host,
        "current_member_run_ref":if may_read_private_session {selected_member_run_id} else {None},
        "provider":if may_read_private_session {selected_member_run.and_then(|run|run["provider"].as_str())} else {None},
        "execution_mode":if may_read_private_session {selected_member_run.and_then(|run|run["execution_mode"].as_str())} else {None},
        "runtime_status":if may_read_private_session {selected_runtime_status} else {None},
        "runtime_generation":if may_read_private_session {selected_member_run.and_then(|run|run["runtime_generation"].as_u64())} else {None},
        "host_session_mode":if selected_is_host {Some(host_session_mode(run))} else {None},
    });
    let unread_count = if projection_scope == "host_member_public" {
        public_unread_count
    } else {
        messages
            .iter()
            .filter(|message| {
                message["deliveries"].as_array().is_some_and(|deliveries| {
                    deliveries.iter().any(|delivery| {
                        matches!(delivery["status"].as_str(), Some("queued" | "delivered"))
                    })
                })
            })
            .count()
    };
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
    if projection_scope == "host_member_public" {
        let data = response["data"]
            .as_object_mut()
            .expect("AgentWorkspace data object");
        data.remove("session_event_projection");
        data.remove("live_provider_activity");
    }
    Ok(response)
}

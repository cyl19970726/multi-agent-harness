use super::*;

pub(crate) fn team_view(
    space_id: &str,
    store: &HarnessStore,
    team_id: &str,
    host: bool,
    identity: Option<&ReadIdentity>,
    company_id: Option<&str>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let route_run = facts.runs.iter().find(|run| run.id == team_id);
    let resolved_team_id = route_run
        .map(|run| run.agent_team_id.as_str())
        .unwrap_or(team_id);
    let team = facts
        .teams
        .iter()
        .find(|team| team.id == resolved_team_id)
        .ok_or(("404 Not Found", "TEAM_NOT_FOUND", team_id.to_string()))?;
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
    if (host && !exact_host_identity) || (!host && !team_member_identity) {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            if host {
                "HostConsole requires this Team's exact Host authority"
            } else {
                "TeamWorkspace requires a Team-scoped AgentMember identity"
            }
            .into(),
        ));
    }
    let run = route_run.or_else(|| facts.latest_run(resolved_team_id));
    let run_id = run.map(|r| r.id.as_str());
    let works = facts
        .works
        .iter()
        .filter(|w| {
            w.accountable_team_id.as_deref() == Some(resolved_team_id)
                || run_id == Some(w.team_run_id.as_str())
        })
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let work_graph = work_graph(&works);
    let team_work_ids = works
        .iter()
        .filter_map(|work| work["work_id"].as_str())
        .collect::<BTreeSet<_>>();
    let (activity, activity_truncated) = team_activity(&facts, &team_work_ids, run_id);
    // Team creation retains the Host identity for messaging, but that does
    // not fabricate an executing member. Show the Host in member capacity
    // only when this exact TeamRun has an explicit Host MemberRun.
    let host_has_member_run = run_id.is_some_and(|selected_run_id| {
        facts.member_runs.iter().any(|member_run| {
            member_run["team_run_id"] == selected_run_id
                && member_run["agent_member_id"] == team.host_agent_id
        })
    });
    let team_member_ids = team
        .member_ids
        .iter()
        .filter(|member_id| member_id.as_str() != team.host_agent_id || host_has_member_run)
        .collect::<BTreeSet<_>>();
    let members=facts.members.iter().filter(|m|m["id"].as_str().is_some_and(|id|team_member_ids.iter().any(|member|member.as_str()==id))).map(|member|{
        let member_id=member["id"].as_str().unwrap_or_default();
        let active=facts.member_runs.iter().filter(|r|r["agent_member_id"]==member_id&&run_id.is_some_and(|id|r["team_run_id"]==id)&&r["coordination_status"]=="active").collect::<Vec<_>>();
        let current=if active.len()==1 { Some(active[0]) } else { None };
        let assigned=works.iter().filter(|work|work["owner_actor_ref"]["id"]==member_id).collect::<Vec<_>>();
        let count_phase=|phase:&str|assigned.iter().filter(|work|work["phase"]==phase).count();
        let latest_action_summary=current.and_then(|run|run["native_session"]["native_session_id"].as_str()).and_then(|session_id|facts.runtime_commands.iter().filter(|command|command["target_session_id"]==session_id).max_by(|a,b|a["updated_at"].as_str().cmp(&b["updated_at"].as_str())).map(|command|record_summary("runtime_command",command)));
        // Adapter review state is a separate fact from runtime availability:
        // an idle member on an unreviewed provider tuple is *not* Ready. The
        // trust MemberRun carries only a profile ref, so the concrete tuple is
        // joined from the runtime-layer projection of the same run.
        let runtime_profile=current.and_then(|r|facts.provider_runtime_projections.iter().filter(|projection|projection["id"]==r["id"]).max_by_key(|projection|projection["runtime_generation"].as_u64().unwrap_or_default())).map(|projection|&projection["provider_profile"]);
        let provider_compatibility=runtime_profile.and_then(|profile|profile["compatibility_status"].as_str());
        let (provider_capability_admission,provider_capability_note)=provider_core_capability_admission(runtime_profile);
        json!({
            "agent_member_ref":{"kind":"agent_member","id":member_id},
            "display_name":member["name"],
            "role":member["role"],
            "organization_status":member["organization_status"],
            "coordination_status":current.map(|r|r["coordination_status"].clone()),
            "provider":current.and_then(|r|r["native_session"]["provider"].as_str()).or_else(||current.and_then(|r|r["provider_profile_snapshot"].as_str())).or_else(||member["provider_profile_ref"].as_str()),
            "model":member["model_preference"],
            "native_session_health":current.and_then(|r|r["native_session"]["availability"].as_str()),
            "current_member_run_ref":current.and_then(|r|r["id"].as_str()),
            "runtime_state":current.and_then(|r|r["runtime_status"].as_str()),
            "runtime_generation":current.and_then(|r|r["runtime_generation"].as_u64()),
            "capacity":match current.and_then(|r|r["runtime_status"].as_str()){Some("running")|Some("queued")=>"busy",Some("idle")|Some("waiting")=>"available",_=>"unknown"},
            "provider_compatibility":provider_compatibility,
            "provider_compatibility_note":runtime_profile.and_then(|profile|profile["compatibility_note"].as_str()),
            "provider_version":runtime_profile.and_then(|profile|profile["provider_version"].as_str()),
            "provider_capability_admission":provider_capability_admission,
            "provider_capability_note":provider_capability_note,
            "active_work_count":count_phase("active"),
            "queued_work_count":count_phase("open"),
            "review_work_count":count_phase("review"),
            "blocked_work_count":assigned.iter().filter(|work|work["condition"]=="blocked").count(),
            "latest_action":latest_action_summary,
        })
    }).collect::<Vec<_>>();
    let messages = facts
        .messages
        .iter()
        .filter(|m| run_id.is_some_and(|id| m["team_run_id"] == id))
        .map(|m| message_summary(&facts, m))
        .collect::<Vec<_>>();
    let pressure_summary = json!({
        "active_turns": members.iter().filter(|member| member["runtime_state"] == "running").count(),
        "ready_members": members.iter().filter(|member| member["capacity"] == "available" && member["provider_compatibility"] == "current" && member["provider_capability_admission"] == "active").count(),
        "total_members": members.len(),
        "ready_work": works.iter().filter(|work| work["readiness"]["state"] == "ready").count(),
        "review_work": works.iter().filter(|work| work["phase"] == "review").count(),
        "blocked_work": works.iter().filter(|work| work["condition"] == "blocked").count(),
    });
    let identity_attention = team_member_ids
        .iter()
        .filter(|member_id| {
            facts
                .member_runs
                .iter()
                .filter(|member_run| {
                    member_run["agent_member_id"] == member_id.as_str()
                        && run_id.is_some_and(|id| member_run["team_run_id"] == id)
                        && member_run["coordination_status"] == "active"
                })
                .count()
                > 1
        })
        .map(|member_id| {
            let observed_at = now();
            json!({"kind":"identity_conflict","severity":"critical","source_ref":{"kind":"agent_member","id":member_id},"reason_code":"multiple_active_member_runs","first_seen_at":observed_at,"last_seen_at":observed_at,"recommended_action":"Host must reconcile duplicate active MemberRuns before assigning or delivering Work"})
        })
        .collect::<Vec<_>>();
    let team_member_run_ids = facts
        .member_runs
        .iter()
        .filter(|run| run_id.is_some_and(|id| run["team_run_id"] == id))
        .filter_map(|run| run["id"].as_str())
        .collect::<BTreeSet<_>>();
    let belongs_to_team_work = |value: &Value| {
        value
            .get("work_id")
            .and_then(Value::as_str)
            .is_some_and(|id| team_work_ids.contains(id))
    };
    let raw_reports = records(&facts, |v| {
        belongs_to_team_work(v) && v.get("report_revision").is_some()
    });
    let raw_findings = records(&facts, |v| {
        belongs_to_team_work(v) && v.get("detail_markdown").is_some()
    });
    let raw_failures = records(&facts, |v| {
        belongs_to_team_work(v) && v.get("observed_failure").is_some()
    });
    let raw_requirements = records(&facts, |v| {
        belongs_to_team_work(v)
            && v.get("requirement_set_fingerprint").is_some()
            && facts.works.iter().any(|work| {
                v["work_id"] == work.id && v["work_revision"].as_u64() == Some(work.version)
            })
    });
    let matches_current_requirement = |candidate: &Value| {
        raw_requirements.iter().any(|requirement| {
            candidate["requirement_id"] == requirement["id"]
                && candidate["work_revision"] == requirement["work_revision"]
                && candidate["candidate_fingerprint"] == requirement["candidate_fingerprint"]
        })
    };
    let raw_evaluations = records(&facts, |v| {
        belongs_to_team_work(v)
            && v.get("verdict").is_some()
            && v.get("requirement_id").is_some()
            && matches_current_requirement(v)
            && raw_requirements.iter().any(|requirement| {
                v["requirement_id"] == requirement["id"]
                    && v["work_report_id"] == requirement["work_report_id"]
                    && v["config_fingerprint"] == requirement["config_fingerprint"]
                    && v["evaluator_fingerprint"] == requirement["evaluator_fingerprint"]
            })
    });
    let raw_waivers = records(&facts, |v| {
        belongs_to_team_work(v)
            && v.get("authority_actor").is_some()
            && v.get("requirement_id").is_some()
            && v["state"] == "active"
            && matches_current_requirement(v)
    });
    let raw_workspace_attention = team_member_run_ids
        .iter()
        .filter_map(|member_run_id| current_workspace(&facts, member_run_id))
        .filter(|value| {
            matches!(
                value["lifecycle"].as_str(),
                Some("dirty" | "conflicted" | "missing" | "cleanup_blocked")
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let raw_delegations = facts
        .side
        .iter()
        .filter(|v| {
            v.get("source_work_ref")
                .and_then(|reference| reference.get("work_id"))
                .and_then(Value::as_str)
                .is_some_and(|id| team_work_ids.contains(id))
                || v.get("target_work_ref")
                    .and_then(|reference| reference.get("work_id"))
                    .and_then(Value::as_str)
                    .is_some_and(|id| team_work_ids.contains(id))
        })
        .cloned()
        .collect::<Vec<_>>();
    let reports = record_summaries("work_report", raw_reports);
    let findings = record_summaries("work_finding", raw_findings);
    let failures = record_summaries("failure_analysis", raw_failures);
    let requirements = record_summaries("gate_requirement", raw_requirements.clone());
    let evaluations = record_summaries("gate_evaluation", raw_evaluations);
    let waivers = record_summaries("gate_waiver", raw_waivers.clone());
    let workspace_attention =
        record_summaries("workspace_binding", raw_workspace_attention.clone());
    let delegations = record_summaries("work_delegation", raw_delegations);
    let team_revision = facts.team_revisions.get(&team.id).copied().ok_or((
        "409 Conflict",
        "PROJECTION_CONFLICT",
        "selected Team has no durable revision".to_string(),
    ))?;
    let collaboration = collaboration_projection(company_id, &team.id, None);
    if !host {
        let latest_run = run.map(|run| json!({"id":run.id,"status":enum_string(&run.status),"previous_run_id":run.previous_run_id,"execution_node_id":run.execution_node_id,"project_binding_id":run.project_binding_id,"execution_root":run.execution_root,"created_at":run.created_at,"completed_at":run.completed_at}));
        let data = json!({"team":{"team_id":team.id,"display_name":team.name,"team_revision":team_revision,"mission_id":team.mission_id,"host_agent_id":team.host_agent_id,"viewer_role":if exact_host_identity{"host"}else{"member"},"node_id":team.node_id,"placement_generation":run.and_then(|run|facts.run_revisions.get(&run.id).copied()),"status":enum_string(&team.status),"latest_run":latest_run},"pressure_summary":pressure_summary,"works":works,"work_graph":work_graph,"members":members,"messages":messages,"activity":activity,"activity_truncated":activity_truncated,"reports":reports,"findings":findings,"failures":failures,"gate_requirements":requirements,"gate_evaluations":evaluations,"gate_waivers":waivers,"workspace_attention":workspace_attention,"delegation_provenance":delegations,"collaboration":collaboration,"page":{"as_of_event_sequence":facts.sequence,"item_count":works.len(),"next_cursor":null}});
        return Ok(envelope(
            "team_workspace",
            &facts,
            data,
            identity_attention,
            vec![],
        ));
    }
    let by_phase = |phase: &str| {
        works
            .iter()
            .filter(|w| w["phase"] == phase)
            .cloned()
            .collect::<Vec<_>>()
    };
    let host_authorized = exact_host_identity;
    let identity_conflicted = !identity_attention.is_empty();
    let disabled =
        (!host_authorized).then_some("authenticated actor is not this Team's exact Host");
    let message_disabled = disabled
        .map(str::to_string)
        .or_else(|| message_fabric_disabled(&facts, store, team));
    let mut actions = Vec::new();
    if let Some(run_id) = run_id {
        actions.push(action("create_work", "team_run", run_id, 0, disabled));
        actions.push(action(
            "send_message",
            "team_run",
            run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
        actions.push(action(
            "reply_message",
            "team_run",
            run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
    }
    for w in &works {
        let id = w["work_id"].as_str().unwrap_or_default();
        let Some(version) = w["work_revision"].as_u64() else {
            continue;
        };
        let phase = w["phase"].as_str().unwrap_or("unknown");
        let condition = w["condition"].as_str().unwrap_or("unknown");
        let assigned = !w["owner_actor_ref"].is_null();
        if phase == "open" && condition == "normal" && !assigned {
            actions.push(action("assign_work", "work", id, version, disabled));
        }
        if matches!(phase, "open" | "active") && assigned {
            actions.push(action("rebind_work", "work", id, version, disabled));
            actions.push(action("release_work", "work", id, version, disabled));
        }
        if phase == "review" && condition == "normal" {
            actions.push(action("request_changes", "work", id, version, disabled));
            if !w["latest_report_ref"].is_null() {
                actions.push(action(
                    "request_gate_evaluation",
                    "work",
                    id,
                    version,
                    disabled,
                ));
            }
            let gates = &w["gate_summary"];
            let gates_satisfied = gates["failed"].as_u64() == Some(0)
                && gates["pending"].as_u64() == Some(0)
                && gates["required"].as_u64()
                    == Some(
                        gates["passed"].as_u64().unwrap_or(0)
                            + gates["waived"].as_u64().unwrap_or(0),
                    );
            if !w["latest_report_ref"].is_null() && gates_satisfied {
                actions.push(action("accept_work", "work", id, version, disabled));
            }
        }
        if phase != "closed" {
            actions.push(action(
                "change_work_dependencies",
                "work",
                id,
                version,
                disabled,
            ));
            actions.push(action("cancel_work", "work", id, version, disabled));
        }
    }
    if let Some(run_id) = run_id {
        for member_run in facts
            .member_runs
            .iter()
            .filter(|value| value["team_run_id"] == run_id)
        {
            let Some(member_run_id) = member_run["id"].as_str() else {
                continue;
            };
            let Some(version) = member_run["version"].as_u64() else {
                continue;
            };
            match member_run["coordination_status"].as_str() {
                Some("active") => {
                    if member_run["runtime_status"] == "running" {
                        let interrupt_disabled = disabled.map(str::to_string).or_else(|| {
                            (!member_run_has_active_provider_capability(
                                &facts.provider_runtime_projections,
                                member_run,
                                "interrupt_current_cycle",
                            ))
                            .then(|| {
                                "the exact provider tuple has no active verified interrupt binding"
                                    .to_string()
                            })
                        });
                        actions.push(action(
                            "interrupt_member_run",
                            "member_run",
                            member_run_id,
                            version,
                            interrupt_disabled.as_deref(),
                        ));
                    }
                    actions.push(action(
                        "close_member_run",
                        "member_run",
                        member_run_id,
                        version,
                        disabled,
                    ));
                }
                Some("closed") => actions.push(action(
                    "reopen_member_run",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                )),
                _ => {}
            }
            if member_run["coordination_status"] != "retired" {
                actions.push(action(
                    "retire_member_run",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                ));
            }
            if member_run["coordination_status"] == "active"
                && matches!(
                    member_run["runtime_status"].as_str(),
                    Some("disconnected" | "failed" | "stopped")
                )
            {
                actions.push(action(
                    "resume_native_session",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                ));
            }
            let binding = facts
                .side
                .iter()
                .filter(|value| {
                    value["member_run_id"] == member_run_id && value.get("canonical_root").is_some()
                })
                .max_by_key(|value| value["version"].as_u64().unwrap_or(0));
            if let Some(binding) = binding {
                let binding_version = binding["version"].as_u64().unwrap_or(0);
                match binding["lifecycle"].as_str() {
                    Some("ready") => actions.push(action(
                        "attach_workspace",
                        "member_run",
                        member_run_id,
                        binding_version,
                        disabled,
                    )),
                    Some("attached" | "dirty" | "conflicted") => actions.push(action(
                        "archive_workspace",
                        "member_run",
                        member_run_id,
                        binding_version,
                        disabled,
                    )),
                    Some("cleanup_blocked") => actions.push(action(
                        "archive_workspace",
                        "member_run",
                        member_run_id,
                        binding_version,
                        disabled,
                    )),
                    Some("archived") => actions.push(action(
                        "cleanup_workspace",
                        "member_run",
                        member_run_id,
                        binding_version,
                        disabled,
                    )),
                    _ => {}
                }
            } else {
                actions.push(action(
                    "provision_workspace",
                    "member_run",
                    member_run_id,
                    version,
                    disabled,
                ));
            }
        }
    }
    for requirement in raw_requirements.iter() {
        let Some(requirement_id) = requirement["id"].as_str() else {
            continue;
        };
        let Some(version) = requirement["version"].as_u64() else {
            continue;
        };
        if identity.is_some_and(|identity| {
            requirement["evaluator_ref"]["kind"] == enum_string(&identity.actor.kind)
                && requirement["evaluator_ref"]["id"] == identity.actor.id
        }) {
            actions.push(action(
                "evaluate_gate",
                "gate_requirement",
                requirement_id,
                version,
                disabled,
            ));
        }
        if identity.is_some_and(|identity| !identity.authority_actors.is_empty()) {
            actions.push(action(
                "waive_gate",
                "gate_requirement",
                requirement_id,
                version,
                disabled,
            ));
        }
    }
    for waiver in raw_waivers
        .iter()
        .filter(|waiver| waiver["state"] == "active")
    {
        if let (Some(id), Some(version), Some(identity)) =
            (waiver["id"].as_str(), waiver["version"].as_u64(), identity)
        {
            let actor_matches = waiver["performed_by_actor"]["kind"]
                == enum_string(&identity.actor.kind)
                && waiver["performed_by_actor"]["id"] == identity.actor.id;
            let authority_matches = identity.authority_actors.iter().any(|authority| {
                waiver["authority_actor"]["kind"] == enum_string(&authority.kind)
                    && waiver["authority_actor"]["id"] == authority.id
            });
            if !actor_matches || !authority_matches {
                continue;
            }
            actions.push(action(
                "revoke_waiver",
                "gate_waiver",
                id,
                version,
                disabled,
            ));
        }
    }
    if identity_conflicted {
        actions.clear();
    }
    let mission = store
        .latest_missions()
        .map_err(|e| {
            (
                "500 Internal Server Error",
                "ROLE_VIEW_BUILD_FAILED",
                e.to_string(),
            )
        })?
        .into_iter()
        .find(|mission| mission.id == team.mission_id);
    let mission_log = store
        .mission_log_tail(&team.mission_id, 20)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e.to_string()))?
        .into_iter()
        .map(|entry| json!({"id":entry.id,"revision":entry.revision,"kind":enum_string(&entry.kind),"body":entry.body,"actor":entry.actor,"created_at":entry.created_at}))
        .collect::<Vec<_>>();
    let mission_context = mission.map(|mission| json!({"id":mission.id,"title":mission.title,"objective":mission.objective,"context":mission.context,"desired_outcome":mission.desired_outcome,"status":enum_string(&mission.status),"outcome_summary":mission.outcome_summary,"created_at":mission.created_at,"updated_at":mission.updated_at,"completed_at":mission.completed_at,"log":mission_log}));
    let supervisor = run.and_then(|run| store.latest_team_supervisor_lease(&run.id).ok().flatten()).map(|lease| {
        let current = enum_string(&lease.status) == "active" && lease.expires_unix_ms > SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;
        json!({"team_run_id":lease.team_run_id,"supervisor_id":lease.supervisor_id,"generation":lease.generation,"current":current,"heartbeat_unix_ms":lease.heartbeat_unix_ms,"expires_unix_ms":lease.expires_unix_ms,"owner_locator":lease.owner_locator,"node_daemon_generation":lease.node_daemon_generation,"status":enum_string(&lease.status)})
    });
    let host_reply_causations = facts
        .messages
        .iter()
        .filter(|message| {
            run_id.is_some_and(|id| message["team_run_id"] == id)
                && message["sender_actor_ref"]["id"] == team.host_agent_id
        })
        .filter_map(|message| message["causation_id"].as_str())
        .collect::<BTreeSet<_>>();
    let mut host_inbox = facts
        .messages
        .iter()
        .filter(|message| {
            run_id.is_some_and(|id| message["team_run_id"] == id)
                && message["sender_actor_ref"]["id"] != team.host_agent_id
                && message["response_intent"] == "response_required"
                && !message["id"]
                    .as_str()
                    .is_some_and(|id| host_reply_causations.contains(id))
                && (message["recipients"].as_array().is_some_and(|recipients| {
                    recipients
                        .iter()
                        .any(|recipient| recipient["id"] == team.host_agent_id)
                }) || message["target_ref"]["id"] == team.id)
        })
        .map(|message| message_summary(&facts, message))
        .collect::<Vec<_>>();
    host_inbox.sort_by(|left, right| {
        right["created_at"]
            .as_str()
            .cmp(&left["created_at"].as_str())
            .then_with(|| {
                right["message_id"]
                    .as_str()
                    .cmp(&left["message_id"].as_str())
            })
    });
    host_inbox.truncate(50);
    let team_message_ids = facts
        .messages
        .iter()
        .filter(|message| run_id.is_some_and(|id| message["team_run_id"] == id))
        .filter_map(|message| message["id"].as_str())
        .collect::<BTreeSet<_>>();
    let runtime_recovery = record_summaries(
        "runtime_command",
        facts
            .runtime_commands
            .iter()
            .filter(|command| {
                command["status"] == "recovery_required"
                    && command["source_record_id"].as_str().is_some_and(|id| {
                        team_work_ids.contains(id) || team_message_ids.contains(id)
                    })
            })
            .cloned()
            .collect(),
    );
    let host_attention_inbox =
        run.and_then(|run| store.host_attention_inbox_for_team_run(&run.id, false).ok());
    let host_runtime = run
        .map(|run| {
            let member = store.host_member_binding(&run.id).map_err(|error| {
                (
                    "409 Conflict",
                    "HOST_RUNTIME_BINDING_INVALID",
                    error.to_string(),
                )
            })?;
            let live = store
                .host_runtime_binding(&run.id, crate::current_unix_ms_u64())
                .ok();
            let managed = member.mode == HostControlMode::Managed;
        let queued_attentions = host_attention_inbox
            .as_ref()
            .map(|inbox| {
                inbox
                    .attentions
                    .iter()
                    .filter(|attention| attention.needs_host_action())
                    .count()
            })
            .unwrap_or(0);
        let last_inbox_read_at = host_attention_inbox.as_ref().and_then(|inbox| {
            inbox
                .attentions
                .iter()
                .filter(|attention| {
                    attention.status == harness_core::HostAttentionStatus::Acknowledged
                })
                .map(|attention| attention.updated_at.as_str())
                .max()
        });
        Ok(json!({
            "agent_member_id": member.host_agent_member_id,
            "member_run_id": member.member_run.id,
            "mode": if managed { "managed" } else { "external_interactive" },
            "delivery_guarantee": if managed { "daemon_managed" } else { "pull_only" },
            "runtime_residency": if managed { "managed_member_run" } else { "detached_user_driven" },
            "provider":member.runtime.provider,
            "runtime_generation":member.member_run.runtime_generation,
            "agent_session_id":live.as_ref().and_then(|binding| match binding {harness_application::HostRuntimeBinding::Managed(binding)=>Some(binding.agent_session.id.as_str()),harness_application::HostRuntimeBinding::ExternalInteractive(_)=>None}),
            "native_session_ref":live.as_ref().and_then(|binding| match binding {harness_application::HostRuntimeBinding::Managed(binding)=>binding.agent_session.native_session_ref.as_ref(),harness_application::HostRuntimeBinding::ExternalInteractive(_)=>None}),
            "effective_permission_ceiling":live.as_ref().and_then(|binding| match binding {harness_application::HostRuntimeBinding::Managed(binding)=>Some(&binding.agent_session.effective_permission_ceiling),harness_application::HostRuntimeBinding::ExternalInteractive(_)=>None}),
            "queued_actionable_items": queued_attentions,
            "last_inbox_read_at": last_inbox_read_at,
            "warning": if managed && live.is_none() {Some("Managed Host has no exact live AgentSession/Supervisor binding")} else if !managed {Some("External Host must read or wait for inbox updates")} else {None},
        }))
        })
        .transpose()?;
    Ok(envelope(
        "host_console",
        &facts,
        json!({"team_ref":team.id,"mission_ref":team.mission_id,"mission_context":mission_context,"team_supervisor":supervisor,"host_runtime":host_runtime,"host_inbox":host_inbox,"member_runtime":members,"runtime_recovery":runtime_recovery,"pressure_summary":pressure_summary,"all_works":works,"work_graph":work_graph,"work_queues":{"ready":works.iter().filter(|w|w["readiness"]["state"]=="ready").cloned().collect::<Vec<_>>(),"unassigned":works.iter().filter(|w|w["owner_actor_ref"].is_null()).cloned().collect::<Vec<_>>(),"blocked":works.iter().filter(|w|w["condition"]=="blocked").cloned().collect::<Vec<_>>(),"review":by_phase("review"),"integration":works.iter().filter(|w|w["module_refs"].as_array().is_some_and(|a|a.iter().any(|m|m=="integration-plan"))).cloned().collect::<Vec<_>>()},"member_capacity":members,"convergence_plans":[],"reusable_findings":findings,"workspace_conflicts":record_summaries("workspace_binding",raw_workspace_attention),"provider_capacity_attention":[{"state":"not_modeled","reason":"Provider account quota is not modeled in this RoleView."}],"deliveries_requiring_reconcile":record_summaries("work_delivery",facts.work_deliveries.iter().filter(|delivery|delivery_requires_team_reconcile(delivery,&team_work_ids)).cloned().collect()),"gate_attention":requirements,"daemon_summary":{"node_id":team.node_id,"lease_status":store.latest_node_daemon_lease(&team.node_id).ok().flatten().map(|lease|enum_string(&lease.status)),"generation":store.latest_node_daemon_lease(&team.node_id).ok().flatten().map(|lease|lease.generation)},"collaboration":collaboration}),
        identity_attention,
        actions,
    ))
}

pub(crate) fn unavailable_session_event_projection(reason: &str) -> Value {
    unavailable_session_event_projection_code("native_session_unavailable", reason)
}

fn unavailable_session_event_projection_code(code: &str, reason: &str) -> Value {
    json!({
        "schema_version":"agentfirm.provider_observation.v1",
        "agent_session_id":null,
        "agent_session_generation":null,
        "source_snapshot_fingerprint":null,
        "episodes":[],
        "truncated":false,
        "availability":"unavailable",
        "unavailable_reason_code":code,
        "disabled_reason":reason,
    })
}

pub(crate) fn normalized_provider(provider: &str) -> &str {
    match provider {
        "codex-app" | "codex_app" | "codex_app_server" => "codex",
        "kimi-code" | "kimi_code" | "kimi_acp" => "kimi",
        "claude-code" | "claude_code" | "claude_agent_sdk" => "claude",
        value => value,
    }
}

/// Resolve one current canonical AgentSession and return its server-owned
/// NativeSessionRef. MemberRun/TeamRun values are selectors only; they never
/// become a replacement source of provider identity or filesystem authority.
/// MemberRun.runtime_generation is intentionally not an AgentSession fence:
/// Team Close/Reopen may replace the adapter generation while the machine-owned
/// AgentSession and provider-native transcript remain continuous.
pub(crate) fn exact_agent_session_binding<'a>(
    agent_sessions: &'a [Value],
    execution_space_id: &str,
    agent_member_id: &str,
    native_session_id: &str,
    provider: Option<&str>,
) -> Result<(&'a Value, NativeSessionRef), &'static str> {
    if native_session_id.trim().is_empty() {
        return Err("The selected provider-native Session has no exact native id.");
    }
    let expected_provider = provider.map(normalized_provider);
    let current = agent_sessions
        .iter()
        .filter(|session| session["execution_space_id"] == execution_space_id)
        .filter(|session| session["agent_member_id"] == agent_member_id)
        .filter(|session| session["lifecycle"] != "closed")
        .filter_map(|session| {
            let native = serde_json::from_value::<NativeSessionRef>(
                session.get("native_session_ref")?.clone(),
            )
            .ok()?;
            (native.native_session_id == native_session_id
                && expected_provider.is_none_or(|expected| {
                    normalized_provider(&native.provider) == expected
                        && session["provider_kind"]
                            .as_str()
                            .is_some_and(|value| normalized_provider(value) == expected)
                }))
            .then_some((session, native))
        })
        .collect::<Vec<_>>();
    match current.as_slice() {
        [(session, native)] => Ok((*session, native.clone())),
        [] => Err("No current canonical AgentSession binds this provider-native Session."),
        _ => Err("Multiple current AgentSessions ambiguously bind this provider-native Session."),
    }
}

pub(crate) struct SessionProjectionReadRequest<'a> {
    pub(crate) execution_space_id: &'a str,
    pub(crate) project_id: &'a str,
    pub(crate) team_id: &'a str,
    pub(crate) selected_agent_id: &'a str,
    pub(crate) viewer_identity_id: &'a str,
    pub(crate) run: Option<&'a AgentTeamRun>,
    pub(crate) selected_member_run: Option<&'a Value>,
}

pub(crate) fn read_session_event_projection(
    _store: &HarnessStore,
    facts: &Facts,
    request: SessionProjectionReadRequest<'_>,
) -> Value {
    let selector = if let Some(member_run) = request.selected_member_run {
        let Some(native) = member_run
            .get("native_session")
            .filter(|value| !value.is_null())
        else {
            return unavailable_session_event_projection(
                "No provider-native Session is bound to this selected Agent run.",
            );
        };
        let Some(native_id) = native["native_session_id"].as_str() else {
            return unavailable_session_event_projection(
                "The selected provider-native Session has no exact native id.",
            );
        };
        (native_id, native["provider"].as_str())
    } else {
        let Some(run) = request.run else {
            return unavailable_session_event_projection(
                "No current TeamRun binds the selected Host Agent Session.",
            );
        };
        let Some(native_id) = run
            .host_thread_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
        else {
            return unavailable_session_event_projection(
                "No provider-native Session is bound to the selected Host run.",
            );
        };
        (native_id, Some(run.host_surface.as_str()))
    };
    if request
        .run
        .is_none_or(|run| run.project_binding_id != request.project_id)
    {
        return unavailable_session_event_projection(
            "The selected TeamRun belongs to another Project Binding.",
        );
    }
    let (session, native_session) = match exact_agent_session_binding(
        &facts.agent_sessions,
        request.execution_space_id,
        request.selected_agent_id,
        selector.0,
        selector.1,
    ) {
        Ok(binding) => binding,
        Err(reason) => return unavailable_session_event_projection(reason),
    };
    // Historical provider-native storage remains readable after Stop/Detach.
    // A current daemon lease is an execution-effect fence, not read authority.
    // The immutable AgentSession binding supplies exact provenance while the
    // owner-only RoleView and provider root checks remain mandatory.
    let Some(node_daemon_id) = session["node_daemon_id"]
        .as_str()
        .filter(|value| !value.trim().is_empty())
    else {
        return unavailable_session_event_projection_code(
            "agent_session_provenance_missing",
            "The canonical AgentSession has no recorded NodeDaemon provenance.",
        );
    };
    let Some(node_daemon_generation) = session["node_daemon_generation"]
        .as_u64()
        .filter(|generation| *generation > 0)
    else {
        return unavailable_session_event_projection_code(
            "agent_session_provenance_missing",
            "The canonical AgentSession has no recorded NodeDaemon generation.",
        );
    };
    crate::provider_event_api::read_historical_projection(
        crate::provider_event_api::HistoricalProjectionRequest {
            execution_space_id: request.execution_space_id,
            project_id: request.project_id,
            team_id: request.team_id,
            agent_member_id: request.selected_agent_id,
            agent_session_id: session["id"].as_str().unwrap_or_default(),
            agent_session_generation: session["runtime_generation"].as_u64().unwrap_or(0),
            node_daemon_id,
            node_daemon_generation,
            viewer_identity_id: request.viewer_identity_id,
            native_session: &native_session,
        },
    )
    .unwrap_or_else(|_| {
        unavailable_session_event_projection_code(
            "provider_native_read_failed",
            "The server could not verify and read the bound provider-native Session.",
        )
    })
}

/// How the selected TeamRun's Host provider session is owned. A
/// harness-managed Host has an exact NodeDaemon-owned AgentSession; an
/// external interactive Host only lends its own provider thread for
/// observation and must never be presented as a managed runtime; without a
/// bound thread the Host has no provider session at all.
pub(crate) fn host_session_mode(
    binding: Option<&harness_application::HostMemberBinding>,
) -> &'static str {
    match binding.map(|binding| binding.mode) {
        Some(HostControlMode::Managed) => "harness_managed",
        Some(HostControlMode::ExternalInteractive) => "external_interactive",
        None => "unbound",
    }
}

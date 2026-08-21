use super::*;

pub(crate) fn member_view(
    space_id: &str,
    store: &HarnessStore,
    member_run_id: &str,
    identity: Option<&ReadIdentity>,
    company_id: Option<&str>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let run = facts
        .member_runs
        .iter()
        .find(|r| r["id"] == member_run_id)
        .ok_or((
            "404 Not Found",
            "MEMBER_RUN_NOT_FOUND",
            member_run_id.to_string(),
        ))?;
    let member_id = run["agent_member_id"].as_str().unwrap_or_default();
    if !identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::AgentMember && identity.actor.id == member_id
    }) {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "MemberWorkbench is visible only to its authenticated AgentMember".into(),
        ));
    }
    let member = facts
        .members
        .iter()
        .find(|m| m["id"] == member_id)
        .cloned()
        .ok_or((
            "404 Not Found",
            "AGENT_MEMBER_NOT_FOUND",
            member_id.to_string(),
        ))?;
    let team_run_id = run["team_run_id"].as_str().unwrap_or_default();
    let active_generations = facts
        .member_runs
        .iter()
        .filter(|candidate| {
            candidate["agent_member_id"] == member_id
                && candidate["team_run_id"] == team_run_id
                && candidate["coordination_status"] == "active"
        })
        .count();
    if active_generations > 1 {
        return Err((
            "409 Conflict",
            "IDENTITY_CONFLICT",
            format!(
                "AgentMember {member_id} has {active_generations} active MemberRuns in TeamRun {team_run_id}"
            ),
        ));
    }
    let team = facts
        .runs
        .iter()
        .find(|r| r.id == team_run_id)
        .and_then(|r| facts.teams.iter().find(|t| t.id == r.agent_team_id))
        .ok_or(("404 Not Found", "TEAM_NOT_FOUND", team_run_id.to_string()))?;
    // DOC-106: Member responsibility follows the assignee TeamMembership, not
    // a MemberRun or runtime. Legacy rows still resolve through the mirrored
    // owner identity until responsibility migration binds their membership.
    let my_membership_ids = facts
        .team_memberships
        .iter()
        .filter(|membership| {
            membership["agent_member_id"].as_str() == Some(member_id)
                && membership["team_id"].as_str() == Some(team.id.as_str())
        })
        .filter_map(|membership| membership["id"].as_str())
        .collect::<BTreeSet<_>>();
    let in_team_scope = |work: &&Work| {
        work.accountable_team_id.as_deref() == Some(team.id.as_str())
            || work.team_run_id == team_run_id
    };
    let assigned_to_member = |work: &&Work| {
        work.assignee_membership_id
            .as_deref()
            .is_some_and(|id| my_membership_ids.contains(id))
            || work.owner_member_id.as_deref() == Some(member_id)
    };
    let team_work_ids = facts
        .works
        .iter()
        .filter(|work| in_team_scope(work))
        .map(|work| work.id.as_str())
        .collect::<BTreeSet<_>>();
    let my = facts
        .works
        .iter()
        .filter(|w| in_team_scope(w) && assigned_to_member(w))
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let member_work_ids = facts
        .works
        .iter()
        .filter(|work| in_team_scope(work) && assigned_to_member(work))
        .map(|work| work.id.clone())
        .collect::<BTreeSet<_>>();
    let collaboration = collaboration_projection(company_id, &team.id, Some(&member_work_ids));
    let pool = facts
        .works
        .iter()
        .filter(|w| {
            in_team_scope(w)
                && w.phase == WorkPhase::Open
                && w.condition == WorkCondition::Normal
                && (w.eligible_member_ids.is_empty()
                    || w.eligible_member_ids.iter().any(|id| id == member_id))
        })
        .map(|w| work_summary(&facts, team, w))
        .collect::<Vec<_>>();
    let queued = facts
        .message_deliveries
        .iter()
        .filter(|d| d["recipient_agent_member_id"] == member_id && d["status"] == "queued")
        .cloned()
        .collect::<Vec<_>>();
    let message_ids = queued
        .iter()
        .filter_map(|d| d["message_id"].as_str())
        .collect::<BTreeSet<_>>();
    let unread = facts
        .messages
        .iter()
        .filter(|m| m["id"].as_str().is_some_and(|id| message_ids.contains(id)))
        .map(|message| message_summary(&facts, message))
        .collect::<Vec<_>>();
    let workspace = current_workspace(&facts, member_run_id).cloned();
    let mut actions = Vec::new();
    let addressed_generation_is_current =
        run["coordination_status"] == "active" && active_generations == 1;
    let team_revision = facts.team_revisions.get(&team.id).copied().unwrap_or(0);
    if addressed_generation_is_current {
        let message_disabled = message_fabric_disabled(&facts, store, team);
        actions.push(action(
            "send_message",
            "team_run",
            team_run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
        actions.push(action(
            "reply_message",
            "team_run",
            team_run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
        actions.push(action(
            "request_decision",
            "team_run",
            team_run_id,
            team_revision,
            message_disabled.as_deref(),
        ));
    }
    for w in &my {
        if !addressed_generation_is_current {
            break;
        }
        let id = w["work_id"].as_str().unwrap_or_default();
        let Some(version) = w["work_revision"].as_u64() else {
            continue;
        };
        let phase = w["phase"].as_str().unwrap_or("unknown");
        let condition = w["condition"].as_str().unwrap_or("unknown");
        if phase == "open" && condition == "normal" {
            actions.push(action("start_work", "work", id, version, None));
        } else if phase == "active" && condition == "normal" {
            actions.push(action("block_work", "work", id, version, None));
            actions.push(action("submit_work", "work", id, version, None));
            if facts
                .works
                .iter()
                .find(|work| work.id == id)
                .is_some_and(|work| work.blocker_reason.is_some())
            {
                actions.push(action("revise_work", "work", id, version, None));
            }
            actions.push(action("write_report", "work", id, version, None));
            actions.push(action("write_finding", "work", id, version, None));
            actions.push(action("write_failure", "work", id, version, None));
        } else if phase == "active" && condition == "blocked" {
            actions.push(action("unblock_work", "work", id, version, None));
            actions.push(action("write_report", "work", id, version, None));
            actions.push(action("write_finding", "work", id, version, None));
            actions.push(action("write_failure", "work", id, version, None));
        }
    }
    for w in &pool {
        if !addressed_generation_is_current {
            break;
        }
        actions.push(action(
            "claim_work",
            "work",
            w["work_id"].as_str().unwrap_or_default(),
            w["work_revision"]
                .as_u64()
                .expect("Work summary carries a durable revision"),
            None,
        ));
    }
    for requirement in records(&facts, |value| {
        value.get("requirement_set_fingerprint").is_some()
            && value["evaluator_ref"]["kind"] == "agent_member"
            && value["evaluator_ref"]["id"] == member_id
    }) {
        if let (Some(id), Some(version)) =
            (requirement["id"].as_str(), requirement["version"].as_u64())
        {
            actions.push(action(
                "evaluate_gate",
                "gate_requirement",
                id,
                version,
                None,
            ));
        }
    }
    Ok(envelope(
        "member_workbench",
        &facts,
        json!({"agent_member":agent_member_summary(&member),"member_run":member_run_summary(run),"my_works":my,"eligible_ready_pool":pool,"unread_messages":unread,"queued_deliveries":record_summaries("message_delivery",queued),"workspace_binding":workspace.as_ref().map(|value|record_summary("workspace_binding",value)),"native_session_health":run["native_session"].get("availability").cloned().unwrap_or(json!("unknown")),"report_history":record_summaries("work_report",records(&facts,|v|v["authored_by"]["id"]==member_id&&v.get("report_revision").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"finding_history":record_summaries("work_finding",records(&facts,|v|v["reported_by"]["id"]==member_id&&v.get("detail_markdown").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"failure_history":record_summaries("failure_analysis",records(&facts,|v|v["reported_by"]["id"]==member_id&&v.get("observed_failure").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id)))),"gate_requirements":record_summaries("gate_requirement",records(&facts,|v|v.get("requirement_set_fingerprint").is_some()&&v["work_id"].as_str().is_some_and(|id|team_work_ids.contains(id))&&facts.works.iter().any(|work|v["work_id"]==work.id&&v["work_revision"].as_u64()==Some(work.version)))),"collaboration":collaboration}),
        vec![],
        actions,
    ))
}

pub(crate) fn operator_view(
    space_id: &str,
    store: &HarnessStore,
    node_id: &str,
    build_sha: &str,
    identity: Option<&ReadIdentity>,
    company_id: Option<&str>,
) -> ViewResult {
    let facts = Facts::read(space_id, store)
        .map_err(|e| ("500 Internal Server Error", "ROLE_VIEW_BUILD_FAILED", e))?;
    let node = store
        .latest_execution_nodes()
        .map_err(|e| {
            (
                "500 Internal Server Error",
                "ROLE_VIEW_BUILD_FAILED",
                e.to_string(),
            )
        })?
        .into_iter()
        .find(|n| n.id == node_id)
        .ok_or(("404 Not Found", "NODE_NOT_FOUND", node_id.to_string()))?;
    let operator_authorized = identity.is_some_and(|identity| {
        identity.actor.kind == ActorKind::Service && identity.actor.id == node_id
    });
    if !operator_authorized {
        return Err((
            "403 Forbidden",
            "NOT_AUTHORIZED",
            "OperatorView requires an exact machine-scoped Service authority".into(),
        ));
    }
    let lease = store.latest_node_daemon_lease(node_id).map_err(|e| {
        (
            "500 Internal Server Error",
            "ROLE_VIEW_BUILD_FAILED",
            e.to_string(),
        )
    })?;
    let node_revision = store
        .execution_nodes()
        .map_err(|e| {
            (
                "500 Internal Server Error",
                "ROLE_VIEW_BUILD_FAILED",
                e.to_string(),
            )
        })?
        .into_iter()
        .filter(|candidate| candidate.id == node_id)
        .count() as u64;
    let node_run_ids = facts
        .runs
        .iter()
        .filter(|run| run.execution_node_id == node_id)
        .map(|run| run.id.as_str())
        .collect::<BTreeSet<_>>();
    let node_member_run_ids = facts
        .member_runs
        .iter()
        .filter(|run| {
            run["team_run_id"]
                .as_str()
                .is_some_and(|id| node_run_ids.contains(id))
        })
        .filter_map(|run| run["id"].as_str())
        .collect::<BTreeSet<_>>();
    let message_backlog = facts
        .message_deliveries
        .iter()
        .filter(|delivery| delivery["target_node_id"] == node_id)
        .filter(|d| {
            matches!(
                d["status"].as_str(),
                Some("queued" | "claimed" | "failed" | "expired")
            )
        })
        .count();
    let work_backlog = facts
        .work_deliveries
        .iter()
        .filter(|delivery| {
            delivery["recipient_member_run_id"]
                .as_str()
                .is_some_and(|id| node_member_run_ids.contains(id))
        })
        .filter(|delivery| {
            matches!(
                delivery["status"].as_str(),
                Some("queued" | "claimed" | "failed" | "expired")
            )
        })
        .count();
    let backlog = message_backlog + work_backlog;
    let runtime_recovery = facts
        .runtime_commands
        .iter()
        .filter(|command| {
            command["target_node_id"] == node_id
                && command["status"] == "recovery_required"
                && command["effect_certainty"] == "unknown"
        })
        .map(|command| {
            let mut projected = command.clone();
            projected["summary"] = json!(format!(
                "command={} effect_certainty={} session={} generation={} failure={}",
                command["command"].as_str().unwrap_or("unknown"),
                command["effect_certainty"].as_str().unwrap_or("unknown"),
                command["target_session_id"].as_str().unwrap_or("none"),
                command["target_session_generation"].as_u64().unwrap_or(0),
                command["failure_code"].as_str().unwrap_or("unclassified"),
            ));
            projected
        })
        .collect::<Vec<_>>();
    let mut operator_actions = facts
        .work_deliveries
        .iter()
        .filter(|delivery| {
            delivery["status"] == "claimed"
                && delivery["recipient_member_run_id"]
                    .as_str()
                    .is_some_and(|id| node_member_run_ids.contains(id))
        })
        .filter_map(|delivery| {
            let delivery_id = delivery["id"].as_str()?;
            Some(action(
                "reconcile_delivery",
                "work_delivery",
                delivery_id,
                *facts
                    .canonical_versions
                    .get(&("work_delivery".into(), delivery_id.into()))?,
                None,
            ))
        })
        .collect::<Vec<_>>();
    for delivery in facts
        .message_deliveries
        .iter()
        .filter(|delivery| delivery["status"] == "claimed" && delivery["target_node_id"] == node_id)
    {
        if let (Some(id), Some(version)) = (delivery["id"].as_str(), delivery["version"].as_u64()) {
            operator_actions.push(action(
                "reconcile_message_delivery",
                "canonical_message_delivery",
                id,
                version,
                None,
            ));
        }
    }
    for command in &runtime_recovery {
        if let (Some(id), Some(version)) = (command["id"].as_str(), command["version"].as_u64()) {
            operator_actions.push(action(
                "resolve_runtime_recovery",
                "runtime_command",
                id,
                version,
                None,
            ));
        }
    }
    operator_actions.push(action(
        "diagnose",
        "execution_node",
        node_id,
        node_revision,
        None,
    ));
    let firm_home = crate::execution_space::firm_home().ok();
    let daemon_live = firm_home.as_ref().is_some_and(|home| {
        crate::supervisor_daemon::daemon_status_via_socket(home, node_id).is_some()
    });
    let local_machine_proven =
        crate::read_local_node_id().ok().as_deref() == Some(node_id) && firm_home.is_some();
    let mut daemon_action = action(
        if daemon_live {
            "stop_daemon"
        } else {
            "start_daemon"
        },
        "execution_node",
        node_id,
        node_revision,
        (!local_machine_proven)
            .then_some("this serve process cannot prove exact local Node lifecycle ownership"),
    );
    daemon_action["authority_generation"] =
        json!(lease.as_ref().map(|lease| lease.generation).unwrap_or(0));
    operator_actions.push(daemon_action);
    for (provider, execution_mode) in crate::role_actions_api::OPERATOR_PROVIDER_ADMISSION_TUPLES {
        let binding = crate::role_actions_api::provider_admission_action_binding(
            store,
            space_id,
            node_id,
            node_revision,
            provider,
            execution_mode,
        );
        let disabled_reason = (!local_machine_proven)
            .then_some("this serve process cannot prove exact local Node admission ownership")
            .map(str::to_string)
            .or_else(|| binding.disabled_reason.clone());
        let mut admission_action = action(
            "admit_provider",
            "execution_node",
            node_id,
            node_revision,
            disabled_reason.as_deref(),
        );
        admission_action["intent_binding"] =
            serde_json::to_value(binding).expect("provider admission action binding serializes");
        operator_actions.push(admission_action);
    }
    let remote_fabric = company_id.map(|company_id| {
        let result = (|| -> Result<Value, String> {
            let home = crate::execution_space::firm_home().map_err(|error| error.to_string())?;
            let layout = harness_store::remote_fabric_store::RemoteFabricStoreLayout::open(&home)
                .map_err(|error| error.to_string())?;
            let root = layout
                .node_local_root(company_id, node_id)
                .map_err(|error| error.to_string())?;
            if !root.exists() {
                return Ok(json!({
                    "company_id":company_id,
                    "node_id":node_id,
                    "state":"unavailable",
                    "reason":"no Node-local Remote Fabric journal exists",
                }));
            }
            let local = layout
                .open_node_local(company_id, node_id)
                .map_err(|error| error.to_string())?;
            let snapshot = local.snapshot().map_err(|error| error.to_string())?;
            let queued = snapshot
                .outboxes
                .values()
                .filter(|outbox| {
                    !matches!(
                        outbox.local_state,
                        harness_fabric::LocalOutboxState::Terminal
                    )
                })
                .count();
            let recovery_required = snapshot
                .inboxes
                .values()
                .filter(|inbox| inbox.state == harness_fabric::LocalInboxState::RecoveryRequired)
                .map(|inbox| inbox.operation_id.clone())
                .collect::<Vec<_>>();
            let now = crate::current_unix_ms_u64();
            let oldest_outbox_age_ms = snapshot
                .outboxes
                .values()
                .filter(|outbox| {
                    !matches!(
                        outbox.local_state,
                        harness_fabric::LocalOutboxState::Terminal
                    )
                })
                .filter_map(|outbox| outbox.operation.as_ref())
                .map(|operation| now.saturating_sub(operation.created_at_unix_ms))
                .max()
                .unwrap_or_default();
            let control_plane_diagnostics = layout
                .control_plane_root(company_id)
                .ok()
                .filter(|root| root.exists())
                .and_then(|_| layout.open_control_plane(company_id).ok())
                .and_then(|control_store| {
                    harness_fabric::diagnostics::inspect_fabric(&control_store, company_id, now)
                        .ok()
                });
            let control_plane_online = control_plane_diagnostics
                .as_ref()
                .map(|diagnostics| diagnostics.control_plane_online);
            let control_plane_metrics =
                control_plane_diagnostics.as_ref().and_then(|diagnostics| {
                    diagnostics
                        .nodes
                        .iter()
                        .find(|diagnostic| diagnostic.node_id == node_id)
                        .cloned()
                });
            let collaboration = layout
                .collaboration_root(company_id)
                .ok()
                .filter(|root| root.exists())
                .and_then(|root| {
                    HarnessStore::new(root)
                        .list_collaboration_delegations(
                            company_id,
                            &harness_store::CollaborationDelegationFilter {
                                source_team_id: None,
                                target_team_id: None,
                                node_id: Some(node_id.into()),
                                state: None,
                            },
                            None,
                            200,
                        )
                        .ok()
                        .map(|page| {
                            let attention = page
                                .items
                                .iter()
                                .filter(|delegation| {
                                    matches!(
                                        delegation.state,
                                        harness_core::collaboration::DelegationState::AwaitingTargetDecision
                                            | harness_core::collaboration::DelegationState::ProvisioningTargetWork
                                            | harness_core::collaboration::DelegationState::CancellationRequested
                                    )
                                })
                                .count();
                            json!({
                                "state":"observed",
                                "delegation_count":page.items.len(),
                                "attention_count":attention,
                                "as_of_store_sequence":page.as_of_store_sequence,
                            })
                        })
                })
                .unwrap_or_else(|| {
                    json!({
                        "state":"unavailable",
                        "reason":"Company collaboration projection is not present on this server",
                    })
                });
            let (state, reason) = match (control_plane_online, control_plane_metrics.as_ref()) {
                (Some(true), Some(_)) => ("observed", None),
                (Some(false), _) => (
                    "offline",
                    Some("Company Control Plane lease is offline or expired"),
                ),
                (Some(true), None) => (
                    "unknown",
                    Some("Control Plane has no projection for this Node"),
                ),
                (None, _) => (
                    "unknown",
                    Some(
                        "Control Plane metrics are unavailable; local journal is not health truth",
                    ),
                ),
            };
            Ok(json!({
                "company_id":company_id,
                "node_id":node_id,
                "state":state,
                "reason":reason,
                "gateway_session":snapshot.active_session,
                "outbox_depth":queued,
                "oldest_outbox_age_ms":oldest_outbox_age_ms,
                "inbox_depth":snapshot.inboxes.len(),
                "recovery_required":recovery_required,
                "control_plane_online":control_plane_online,
                "control_plane_metrics":control_plane_metrics,
                "collaboration":collaboration,
                "store_revision":snapshot.revision,
            }))
        })();
        result.unwrap_or_else(|error| {
            json!({
                "company_id":company_id,
                "node_id":node_id,
                "state":"unavailable",
                "reason":error,
            })
        })
    });
    Ok(envelope(
        "operator",
        &facts,
        json!({
            "node":{"node_id":node.id,"node_revision":node_revision,"daemon_generation":lease.as_ref().map(|l|l.generation),"status":enum_string(&node.status)},
            "build":{"build_sha":build_sha,"protocol_version":"agentfirm-member-trust/1","schema_version":SCHEMA_VERSION},
            "projects":record_summaries("node_project_registration",store.latest_node_project_registrations().unwrap_or_default().into_iter().filter(|p|p.node_id==node_id).filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "team_supervisors":record_summaries("team_supervisor_lease",store.team_runs().unwrap_or_default().into_iter().filter(|r|r.execution_node_id==node_id).filter_map(|r|store.latest_team_supervisor_lease(&r.id).ok().flatten()).filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "delivery_backlog":{"depth":backlog,"oldest_age_ms":null,"recovery_required":backlog>0},
            "runtime_recovery":record_summaries("runtime_command",runtime_recovery),
            "provider_admission":record_summaries("provider_compatibility_admission",store.latest_provider_compatibility_admissions().unwrap_or_default().into_iter().filter_map(|value|serde_json::to_value(value).ok()).collect()),
            "workspace_safety":record_summaries("workspace_binding",node_member_run_ids.iter().filter_map(|id|current_workspace(&facts,id)).cloned().collect()),
            "diagnostics":[{"kind":"daemon_lease","state":lease.as_ref().map(|l|enum_string(&l.status)).unwrap_or_else(||"unavailable".into())}],
            "remote_fabric":remote_fabric,
        }),
        vec![],
        operator_actions,
    ))
}

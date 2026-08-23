use super::*;

pub(super) fn dashboard_snapshot(store: &HarnessStore) -> CliResult<serde_json::Value> {
    let members = latest_members(store)?;
    let teams = latest_teams(store)?;
    let runtimes = latest_runtimes(store)?;
    let messages = latest_messages_in_append_order(store)?;
    let evidence = store.evidence()?;
    let provider_child_threads = store.provider_child_threads()?;
    let missions = store.latest_missions()?;
    let legacy_waves = store.latest_legacy_waves()?;
    // Unlike Mission, a MissionLogEntry is never revised in place — every
    // row is a permanent entry, so the whole-snapshot projection reads the raw
    // append-order ledger (`mission_log()`), not a latest-wins fold.
    let mission_log = store.mission_log()?;
    // Agent Team v0 ledger projections (append-only, latest-wins). The folded
    // event log is capped per run so a chatty run cannot bloat the snapshot.
    let team_runs = latest_team_runs_in_append_order(store)?;
    let member_runs = latest_member_runs_in_append_order(store)?;
    // Current inbox truth is projected only from the Wave 4C canonical
    // Message + per-recipient MessageDelivery plane. Retired
    // team_messages/provider_dispatch ledgers remain export-only history and
    // are never folded into a current Dashboard snapshot.
    let mut trust_scopes = BTreeSet::new();
    for member_run in &member_runs {
        if let Some(scope) = store.trust_member_run_scope(&member_run.id)? {
            trust_scopes.insert(scope);
        }
    }
    trust_scopes.extend(store.canonical_execution_space_ids()?);
    let mut agent_identities = Vec::new();
    let mut agent_sessions = Vec::new();
    let mut team_memberships = Vec::new();
    let mut work_execution_bindings = Vec::new();
    let mut work_deliveries = Vec::new();
    let mut canonical_messages = Vec::new();
    let mut canonical_message_deliveries = Vec::new();
    for execution_space_id in trust_scopes {
        agent_identities.extend(store.fabric_agent_identities(&execution_space_id)?);
        agent_sessions.extend(store.fabric_agent_sessions(&execution_space_id)?);
        team_memberships.extend(store.fabric_team_memberships(&execution_space_id)?);
        work_execution_bindings.extend(store.fabric_work_execution_bindings(&execution_space_id)?);
        work_deliveries.extend(store.current_work_deliveries(&execution_space_id)?);
        let space_messages = store.fabric_messages(&execution_space_id)?;
        let deliveries = store.fabric_message_deliveries(&execution_space_id)?;
        canonical_messages.extend(space_messages.iter().cloned());
        canonical_message_deliveries.extend(deliveries.iter().cloned());
    }
    let mut team_messages = Vec::new();
    for run in &team_runs {
        team_messages.extend(canonical_team_messages_for_run(store, &run.id)?);
    }
    team_messages.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let works = store.latest_works()?;
    let work_events = store.work_events()?;
    let work_delegations = store.latest_work_delegations()?;
    let work_delegation_events = store.work_delegation_events()?;
    let execution_nodes = store.latest_execution_nodes()?;
    let node_project_registrations = store.latest_node_project_registrations()?;
    let node_daemon_leases = store.latest_node_daemon_leases()?;
    let team_supervisor_leases = latest_team_supervisor_leases_in_append_order(store)?;
    let team_member_close_requests = latest_team_member_close_requests_in_append_order(store)?;
    // Old ledgers can contain v0 `thinking` rows. Keep the JSONL history
    // intact for migration/audit, but never project those rows into a new
    // snapshot: thinking is not product state or evidence.
    let member_actions = visible_member_actions_in_append_order(store)?;
    let delegation_runs = latest_delegation_runs_in_append_order(store)?;
    let team_run_events = recent_current_team_run_events_in_append_order(store, &team_runs, 500)?;
    let member_cards: Vec<_> = members
        .values()
        .map(|member| {
            let derived_team_ids = teams
                .values()
                .filter(|team| team.member_ids.iter().any(|id| id == &member.id))
                .map(|team| team.id.clone())
                .collect::<Vec<_>>();
            let runtime = member
                .provider_runtime_id
                .as_ref()
                .and_then(|runtime_id| runtimes.get(runtime_id));
            let inbox_count = messages
                .iter()
                .filter(|message| message.to_agent_id.as_ref() == Some(&member.id))
                .count();
            let queued_count = messages
                .iter()
                .filter(|message| message.to_agent_id.as_ref() == Some(&member.id))
                .filter(|message| message.delivery_status == RegistryDeliveryStatus::Queued)
                .count();
            let child_thread_count = provider_child_threads
                .iter()
                .filter(|thread| thread.agent_member_id == member.id)
                .count();
            serde_json::json!({
                "id": member.id,
                "name": member.name,
                "description": member.description,
                "role": member.role,
                "provider": member.provider,
                "status": member.status,
                "runtime_status": runtime.map(|runtime| &runtime.status),
                "runtime_id": runtime.map(|runtime| runtime.id.clone()),
                "runtime_pid": runtime.and_then(|runtime| runtime.pid),
                "runtime_alive": runtime.is_some_and(runtime_is_alive),
                "runtime_health": runtime.map(|runtime| runtime.health.clone()),
                "control_endpoint": member.control_endpoint.clone(),
                "native_session": member.native_session.clone(),
                "provider_thread_id": member.provider_thread_id.clone(),
                "provider_agent_path": member.provider_agent_path.clone(),
                "provider_agent_nickname": member.provider_agent_nickname.clone(),
                "provider_agent_role": member.provider_agent_role.clone(),
                "current_task_id": member.current_task_id,
                "current_proposal_id": member.current_proposal_id,
                "prompt_ref": member.prompt_ref,
                "skill_refs": member.skill_refs,
                // Config-tab + identity-rail data (Multica layout): these live on
                // the ProviderLaunchProfile but were not previously projected into the
                // snapshot. Additive — no schema change.
                "model": member.model,
                "profile": member.profile,
                "provider_config": member.provider_config,
                // Reverse membership is derived from the latest Mission-owned
                // Team definitions. ProviderLaunchProfile.team_ids is compatibility input,
                // never authoritative read-model state.
                "team_ids": derived_team_ids,
                "created_at": member.created_at,
                "last_seen_at": member.last_seen_at,
                "inbox_count": inbox_count,
                "queued_count": queued_count,
                "provider_child_thread_count": child_thread_count
            })
        })
        .collect();
    // DOC-108 pre-cutover tolerance note lives with the loop below; the
    // canonical id set is captured before `teams` is consumed so the
    // membership check covers every canonical Team, not only Active ones.
    let canonical_team_ids = teams.keys().cloned().collect::<BTreeSet<_>>();
    let mut team_values = teams
        .into_values()
        .filter(|team| team.status == AgentTeamStatus::Active)
        .map(|team| {
            // DEV-35 compatibility projection: the dashboard AgentTeam type
            // still requires mission_id / host_agent_id / member_ids. Derive
            // them from the durable TeamMembership authority — read-model
            // compat fields, never stored Team authority.
            let mut value = serde_json::to_value(&team).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(object) = value.as_object_mut() {
                let active_memberships = team_memberships
                    .iter()
                    .filter(|membership| {
                        membership.team_id == team.id
                            && membership.state
                                == harness_core::agentfirm_api::TeamMembershipStatus::Active
                    })
                    .collect::<Vec<_>>();
                let host_agent_id = active_memberships
                    .iter()
                    .find(|membership| {
                        membership.role == harness_core::agentfirm_api::TeamMembershipRole::Host
                    })
                    .map(|membership| membership.agent_member_id.clone())
                    .unwrap_or_default();
                let member_ids = active_memberships
                    .iter()
                    .filter(|membership| {
                        membership.role != harness_core::agentfirm_api::TeamMembershipRole::Host
                    })
                    .map(|membership| membership.agent_member_id.clone())
                    .collect::<Vec<_>>();
                object.insert(
                    "mission_id".to_string(),
                    team.legacy_mission_id.clone().unwrap_or_default().into(),
                );
                object.insert("host_agent_id".to_string(), host_agent_id.into());
                object.insert("member_ids".to_string(), member_ids.into());
            }
            value
        })
        .collect::<Vec<_>>();
    // DOC-108 pre-cutover tolerance: a TeamRun whose AgentTeam is absent from
    // the canonical projection but present in the retired legacy teams.jsonl
    // ledger is a migration fact. Render that Team as read-only legacy
    // context with an explicit integrity annotation instead of failing the
    // whole snapshot; a team id missing from both ledgers still fails closed
    // inside `canonical_team_messages_for_run`.
    let mut integrity_annotations = Vec::new();
    {
        let legacy_teams_by_id = legacy_team_definitions_by_id(store)?;
        let mut legacy_context_ids = BTreeSet::new();
        for run in &team_runs {
            if canonical_team_ids.contains(&run.agent_team_id) {
                continue;
            }
            if legacy_teams_by_id.contains_key(&run.agent_team_id) {
                legacy_context_ids.insert(run.agent_team_id.clone());
                integrity_annotations.push(serde_json::json!({
                    "kind": "pre_cutover_dangling_agent_team_ref",
                    "team_run_id": run.id,
                    "agent_team_id": run.agent_team_id,
                    "annotation": PRE_CUTOVER_DANGLING_TEAM_ANNOTATION,
                }));
            }
        }
        for id in legacy_context_ids {
            if let Some(legacy) = legacy_teams_by_id.get(&id) {
                let mut value = legacy.clone();
                if let Some(object) = value.as_object_mut() {
                    object.insert("legacy_context".to_string(), true.into());
                    object.insert("read_only".to_string(), true.into());
                    object.insert(
                        "integrity_annotation".to_string(),
                        PRE_CUTOVER_DANGLING_TEAM_ANNOTATION.into(),
                    );
                }
                team_values.push(value);
            }
        }
    }
    Ok(serde_json::json!({
        "generated_at": now_string(),
        "teams": team_values,
        "members": member_cards,
        "messages": messages,
        "evidence": evidence,
        "provider_child_threads": provider_child_threads,
        "missions": missions,
        "legacy_waves": legacy_waves,
        "mission_log": mission_log,
        "team_runs": team_runs,
        "member_runs": member_runs,
        "team_messages": team_messages,
        "agent_identities": agent_identities,
        "agent_sessions": agent_sessions,
        "team_memberships": team_memberships,
        "work_execution_bindings": work_execution_bindings,
        "canonical_messages": canonical_messages,
        "canonical_message_deliveries": canonical_message_deliveries,
        "works": works,
        "work_events": work_events,
        "work_deliveries": work_deliveries,
        "work_delegations": work_delegations,
        "work_delegation_events": work_delegation_events,
        "execution_nodes": execution_nodes,
        "node_project_registrations": node_project_registrations,
        "node_daemon_leases": node_daemon_leases,
        "team_supervisor_leases": team_supervisor_leases,
        "team_member_close_requests": team_member_close_requests,
        "member_actions": member_actions,
        "delegation_runs": delegation_runs,
        "team_run_events": team_run_events,
        "integrity_annotations": integrity_annotations
    }))
}

/// `GET /v1/meta` — server build/data provenance (issue #307, 2nd occurrence of
/// "panel shows something other than Store truth"). Every dashboard surface can
/// cross-check itself against this without reading server logs:
///   - `git_rev` / `built_at`: which commit and when this *server* binary was
///     built (embedded at compile time by `build.rs`, never shelled out here);
///   - `store_root`: which coordination store this response actually read;
///   - `latest_op_seq`: how far that store's Work operation log has advanced.
///
/// The store has no single field named "seq"; `work_operations.jsonl` is an
/// append-only per-store log (one row per create/assign/start/accept/...), so
/// its row count is a monotonic cursor over every WorkOperation the store has
/// recorded — the "newest event cursor" the store exposes (see
/// `HarnessStore::work_operations`). It only ever grows.
pub(super) fn dashboard_meta(store: &HarnessStore) -> CliResult<serde_json::Value> {
    let store_root = std::fs::canonicalize(store.root())
        .unwrap_or_else(|_| store.root().to_path_buf())
        .display()
        .to_string();
    let latest_op_seq = store.work_operations()?.len() as u64;
    let daemon_lease = store
        .latest_node_daemon_leases()?
        .into_iter()
        .filter(|lease| lease.status == NodeDaemonLeaseStatus::Active)
        .max_by_key(|lease| (lease.renewed_unix_ms, lease.generation));
    Ok(serde_json::json!({
        "git_rev": build_git_rev(),
        "built_at": build_built_at(),
        "store_root": store_root,
        "latest_op_seq": latest_op_seq,
        "server_version": env!("CARGO_PKG_VERSION"),
        "build_sha": build_git_rev(),
        "node_id": daemon_lease.as_ref().map(|lease| lease.node_id.as_str()),
        "daemon_generation": daemon_lease.as_ref().map(|lease| lease.generation),
        "protocol_version": agentfirm_api::MEMBER_TRUST_PROTOCOL_VERSION,
        "schema_version": "agentfirm.role_views.v1",
        "action_manifest_version": "agentfirm.role_actions.v1",
        "capability_auth": "x-agentfirm-token",
    }))
}

/// A bounded canonical projection for a Team deep link. It contains only
/// Harness coordination state belonging to the selected TeamRun plus its
/// Mission, Mission Log, Team, members, and Legacy Wave history. Provider-native transcript/activity is
/// deliberately absent and remains available only through native-session
/// projection routes.
pub(super) fn dashboard_team_run_snapshot(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<serde_json::Value> {
    let mut snapshot = dashboard_snapshot(store)?;
    let selected = snapshot["team_runs"]
        .as_array()
        .and_then(|runs| {
            runs.iter()
                .find(|run| run.get("id").and_then(|value| value.as_str()) == Some(team_run_id))
        })
        .cloned()
        .ok_or_else(|| CliError::Usage(format!("TeamRun not found: {team_run_id}")))?;
    let mission_id = selected
        .get("mission_id")
        .and_then(|value| value.as_str())
        .map(str::to_owned);
    let team_id = selected
        .get("agent_team_id")
        .and_then(|value| value.as_str())
        .map(str::to_owned);

    let agent_member_ids = snapshot["member_runs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| json_field_eq(row, "team_run_id", team_run_id))
        .filter_map(|row| {
            row.get("agent_member_id")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
        })
        .collect::<HashSet<_>>();

    retain_json_rows(&mut snapshot, "team_runs", |row| {
        json_field_eq(row, "id", team_run_id)
    });
    for key in [
        "member_runs",
        "team_messages",
        "works",
        "work_events",
        "work_deliveries",
        "team_supervisor_leases",
        "team_member_close_requests",
        "member_actions",
        "delegation_runs",
        "team_run_events",
    ] {
        retain_json_rows(&mut snapshot, key, |row| {
            json_field_eq(row, "team_run_id", team_run_id)
        });
    }
    retain_json_rows(&mut snapshot, "members", |row| {
        row.get("id")
            .and_then(|value| value.as_str())
            .is_some_and(|id| agent_member_ids.contains(id))
    });
    retain_json_rows(&mut snapshot, "teams", |row| {
        team_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "id", id))
    });
    retain_json_rows(&mut snapshot, "missions", |row| {
        mission_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "id", id))
    });
    retain_json_rows(&mut snapshot, "legacy_waves", |row| {
        mission_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "mission_id", id))
    });
    retain_json_rows(&mut snapshot, "mission_log", |row| {
        mission_id
            .as_deref()
            .is_some_and(|id| json_field_eq(row, "mission_id", id))
    });
    for key in ["messages", "events", "evidence", "provider_child_threads"] {
        retain_json_rows(&mut snapshot, key, |_| false);
    }
    if let Some(object) = snapshot.as_object_mut() {
        object.insert("company_os".to_string(), serde_json::json!({}));
    }
    Ok(snapshot)
}

pub(super) fn json_field_eq(row: &serde_json::Value, field: &str, expected: &str) -> bool {
    row.get(field).and_then(|value| value.as_str()) == Some(expected)
}

pub(super) fn retain_json_rows(
    snapshot: &mut serde_json::Value,
    key: &str,
    mut keep: impl FnMut(&serde_json::Value) -> bool,
) {
    if let Some(rows) = snapshot
        .get_mut(key)
        .and_then(serde_json::Value::as_array_mut)
    {
        rows.retain(|row| keep(row));
    }
}

pub(super) fn latest_member(
    store: &HarnessStore,
    member_id: &str,
) -> CliResult<ProviderLaunchProfile> {
    latest_members(store)?
        .remove(member_id)
        .ok_or_else(|| CliError::Usage(format!("agent member not found: {member_id}")))
}

pub(super) fn latest_message(store: &HarnessStore, message_id: &str) -> CliResult<RegistryMessage> {
    latest_messages(store)?
        .remove(message_id)
        .ok_or_else(|| CliError::Usage(format!("message not found: {message_id}")))
}

pub(super) fn latest_messages(
    store: &HarnessStore,
) -> CliResult<BTreeMap<String, RegistryMessage>> {
    let mut messages = BTreeMap::new();
    for message in store.messages()? {
        messages.insert(message.id.clone(), message);
    }
    Ok(messages)
}

pub(super) fn latest_runtime(
    store: &HarnessStore,
    runtime_id: &str,
) -> CliResult<Option<ProviderProcess>> {
    let mut runtimes = BTreeMap::new();
    for runtime in store.runtimes()? {
        runtimes.insert(runtime.id.clone(), runtime);
    }
    Ok(runtimes.remove(runtime_id))
}

pub(super) fn latest_team_runs_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<AgentTeamRun>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for run in store.team_runs()? {
        ids.retain(|id| id != &run.id);
        ids.push(run.id.clone());
        by_id.insert(run.id.clone(), run);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

pub(super) fn latest_member_runs_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<ProviderRuntimeProjection>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for run in store.member_runs()? {
        ids.retain(|id| id != &run.id);
        ids.push(run.id.clone());
        by_id.insert(run.id.clone(), run);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

pub(super) fn latest_team_supervisor_leases_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<TeamSupervisorLease>> {
    let mut ids = Vec::new();
    let mut by_team_run = BTreeMap::new();
    for lease in store.team_supervisor_leases()? {
        ids.retain(|id| id != &lease.team_run_id);
        ids.push(lease.team_run_id.clone());
        by_team_run.insert(lease.team_run_id.clone(), lease);
    }
    Ok(ids
        .into_iter()
        .filter_map(|id| by_team_run.remove(&id))
        .collect())
}

pub(super) fn latest_team_member_close_requests_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<TeamMemberCloseRequest>> {
    let mut ids = Vec::new();
    let mut by_member_run = BTreeMap::new();
    for request in store.team_member_close_requests()? {
        ids.retain(|id| id != &request.member_run_id);
        ids.push(request.member_run_id.clone());
        by_member_run.insert(request.member_run_id.clone(), request);
    }
    Ok(ids
        .into_iter()
        .filter_map(|id| by_member_run.remove(&id))
        .collect())
}

pub(super) fn latest_member_actions_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<MemberAction>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for action in store.member_actions()? {
        ids.retain(|id| id != &action.id);
        ids.push(action.id.clone());
        by_id.insert(action.id.clone(), action);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

/// Project the product-visible MemberAction view. Legacy v0 reasoning rows
/// remain in the append-only ledger but are never surfaced to a new operator
/// or MCP consumer as durable state.
pub(super) fn visible_member_actions_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<MemberAction>> {
    Ok(latest_member_actions_in_append_order(store)?
        .into_iter()
        .filter(|action| action.action_type != "thinking")
        .collect())
}

pub(super) fn latest_delegation_runs_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<DelegationRun>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for run in store.delegation_runs()? {
        ids.retain(|id| id != &run.id);
        ids.push(run.id.clone());
        by_id.insert(run.id.clone(), run);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

pub(super) fn current_team_run_events_in_append_order(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<Vec<TeamRunEvent>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for event in store.current_team_run_events(team_run_id)? {
        ids.retain(|id| id != &event.id);
        ids.push(event.id.clone());
        by_id.insert(event.id.clone(), event);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

/// Snapshot projection of the folded team-run event log: latest-wins by id,
/// then capped to the most recent `per_run_cap` events per team run (by seq)
/// so a chatty run cannot bloat every dashboard snapshot.
pub(super) fn recent_current_team_run_events_in_append_order(
    store: &HarnessStore,
    team_runs: &[AgentTeamRun],
    per_run_cap: usize,
) -> CliResult<Vec<TeamRunEvent>> {
    let mut events = Vec::new();
    for run in team_runs {
        events.extend(current_team_run_events_in_append_order(store, &run.id)?);
    }
    let mut seqs_by_run: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for event in &events {
        seqs_by_run
            .entry(event.team_run_id.clone())
            .or_default()
            .push(event.seq);
    }
    // The seq floor per run: the cap-th largest seq (0 when the run has fewer
    // than `per_run_cap` events, keeping all of them).
    let mut min_kept_seq = BTreeMap::new();
    for (run_id, mut seqs) in seqs_by_run {
        seqs.sort_unstable_by(|a, b| b.cmp(a));
        let floor = seqs
            .get(per_run_cap.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        min_kept_seq.insert(run_id, floor);
    }
    Ok(events
        .into_iter()
        .filter(|event| event.seq >= min_kept_seq.get(&event.team_run_id).copied().unwrap_or(0))
        .collect())
}

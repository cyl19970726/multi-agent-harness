use super::*;

/// GET /v1/host-attentions?team_run_id=<id> — reconciled latest HostAttention
/// rows for one TeamRun. The console reads these to show what needs Host
/// action; transport intake only, nothing here mutates Work.
pub(super) fn host_attentions_value(
    store: &HarnessStore,
    team_run_id: &str,
) -> CliResult<serde_json::Value> {
    if team_run_id.trim().is_empty() {
        return Err(CliError::Usage(
            "team_run_id query parameter is required".to_string(),
        ));
    }
    latest_team_run(store, team_run_id)?;
    let attentions = store
        .host_attentions()?
        .into_iter()
        .filter(|attention| attention.team_run_id == team_run_id)
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "attentions": attentions }))
}

/// POST /v1/host-attentions/{id}/ack — console acknowledgement of one
/// HostAttention. The console is a Host surface, so the endpoint resolves the
/// TeamRun's own host binding (binding unbound runs to the console first,
/// mirroring `team-run bind-host`) and walks the lifecycle as needed:
/// Actionable -> claim + complete + acknowledge; Claimed -> fail the stale
/// claim, then claim/complete/acknowledge; Delivered -> acknowledge;
/// Acknowledged -> idempotent. Never mutates Work.
pub(super) fn ack_host_attention_value(
    store: &HarnessStore,
    attention_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let _acknowledged_by = optional_json_string(body, "acknowledged_by")?
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "operator".to_string());

    fn find_attention(store: &HarnessStore, attention_id: &str) -> CliResult<HostAttention> {
        store
            .host_attentions()?
            .into_iter()
            .find(|attention| attention.id == attention_id)
            .ok_or_else(|| CliError::Usage(format!("host attention {attention_id} not found")))
    }

    let attention = find_attention(store, attention_id)?;
    let mut run = latest_team_run(store, &attention.team_run_id)?;

    // Claim/ack require an exact non-empty host binding. Console-created runs
    // may have none; bind them to the console surface exactly like the
    // `team-run bind-host` command does.
    if run.host_thread_id.is_none() {
        let mut next = run.clone();
        next.host_thread_id = Some("console".to_string());
        next.updated_at = now_string();
        store_conflict_as_usage(store.compare_and_append_team_run(&run, &next))?;
        append_team_run_event(
            store,
            &run.id,
            next_team_run_seq(store, &run.id)?,
            TeamRunEventSourceKind::Host,
            None,
            "host_binding",
            &run.id,
            "updated",
            &format!("Host binding set to {}:console", next.host_surface),
        )?;
        run = next;
    }
    let surface = run.host_surface.clone();
    let thread = run
        .host_thread_id
        .clone()
        .expect("host binding established above");

    let now = now_string();
    match attention.status {
        HostAttentionStatus::Acknowledged => {
            return Ok(serde_json::json!({ "attention": attention, "idempotent": true }));
        }
        HostAttentionStatus::Delivered => {
            let final_attention: HostAttention = store_conflict_as_usage(
                store.acknowledge_host_attention(attention_id, &surface, &thread, &now),
            )?;
            return Ok(serde_json::json!({ "attention": final_attention, "idempotent": false }));
        }
        HostAttentionStatus::Claimed => {
            if let Some(claim_id) = attention.claim_id.as_deref() {
                let _: HostAttention = store_conflict_as_usage(store.fail_host_attention_claim(
                    attention_id,
                    claim_id,
                    "console reclaim",
                    &now,
                ))?;
            }
        }
        HostAttentionStatus::Actionable => {}
        HostAttentionStatus::EscalationRequired => {
            // Already escalated — no further console ack needed.
            return Err(CliError::Usage(format!(
                "HostAttention {attention_id} has been escalated and requires human review"
            )));
        }
    }

    let claim_id = format!("console-{attention_id}");
    let claimed: HostAttentionClaimResult = store_conflict_as_usage(store.claim_host_attention(
        attention_id,
        &surface,
        &thread,
        &claim_id,
        &now,
    ))?;
    match claimed {
        HostAttentionClaimResult::Claimed(_) => {}
        HostAttentionClaimResult::NotActionable => {
            return Err(CliError::Usage(format!(
                "host attention {attention_id} is no longer actionable"
            )));
        }
    }
    let _: HostAttention = store_conflict_as_usage(store.complete_host_attention_claim(
        attention_id,
        &claim_id,
        "console-ack",
        &now,
    ))?;
    let final_attention: HostAttention = store_conflict_as_usage(
        store.acknowledge_host_attention(attention_id, &surface, &thread, &now),
    )?;
    Ok(serde_json::json!({ "attention": final_attention, "idempotent": false }))
}

/// Explicit successor-Supervisor reconciliation for a stale Work delivery
/// claim. Like the CLI path, this only requeues a claim from an older
/// generation and never guesses that a provider accepted the Work.
pub(crate) fn reconcile_team_work_delivery_value(
    store: &HarnessStore,
    team_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let delivery_id = required_json_string(body, "delivery_id")?;
    let supervisor_id = required_json_string(body, "supervisor_id")?;
    let supervisor_generation = body
        .get("supervisor_generation")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            CliError::Usage("supervisor_generation must be an unsigned integer".to_string())
        })?;
    let delivery = store_conflict_as_usage(store.reconcile_stale_work_delivery_claim(
        team_run_id,
        &delivery_id,
        &supervisor_id,
        supervisor_generation,
        current_unix_ms_u64(),
        &now_string(),
    ))?;
    Ok(serde_json::to_value(delivery)?)
}

pub(super) fn create_message_value(
    store: &HarnessStore,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let to_agent_id = json_string(body, "to_agent_id").or_else(|| json_string(body, "to"));
    let target = to_agent_id
        .as_deref()
        .map(|agent_id| latest_member(store, agent_id))
        .transpose()?;
    if let Some(member) = target.as_ref() {
        ensure_member_accepts_delivery(member)?;
    }
    let message = RegistryMessage {
        id: json_string(body, "id").unwrap_or_else(|| generated_id("msg")),
        task_id: json_string(body, "task_id").or_else(|| json_string(body, "task")),
        from_agent_id: required_json_string(body, "from_agent_id")
            .or_else(|_| required_json_string(body, "from"))?,
        to_agent_id,
        channel: json_string(body, "channel"),
        kind: parse_message_kind(json_string(body, "kind").as_deref().unwrap_or("message"))?,
        delivery_status: RegistryDeliveryStatus::Queued,
        content: required_json_string(body, "content")?,
        evidence_ids: json_string_array(body, "evidence_ids"),
        created_at: now_string(),
        delivery: None,
        sender_kind: match json_string(body, "sender_kind") {
            Some(value) => parse_sender_kind(&value)?,
            None => SenderKind::default(),
        },
    };
    store.append_message(&message)?;
    if let Some(member) = target.as_ref() {
        append_harness_runtime_control_fact(
            store,
            &member.id,
            member.provider_runtime_id.as_deref(),
            message.task_id.as_deref(),
            "message_queued",
            "RegistryMessage queued for Agent Member",
            None,
        )?;
    }
    Ok(serde_json::to_value(message)?)
}

// ---------------------------------------------------------------------------
// Create-entity side-effect helpers (WP-ii)
//
// These functions own the *persistence + event* logic for creating each core
// entity, so the CLI command arms and the HTTP create routes (POST /v1/teams,
// /agents) share one implementation. The CLI builds the struct from `--flag`
// args; the HTTP value-fns below build the same struct from a JSON body. Both
// then call these helpers, so behaviour cannot diverge.
// ---------------------------------------------------------------------------

pub(super) fn initial_team_memberships(
    team: &AgentTeam,
    host_agent_member_id: &str,
    member_ids: &[String],
    created_by: &harness_core::agentfirm_api::ActorRef,
    created_at: &str,
) -> Vec<harness_core::agentfirm_api::TeamMembership> {
    std::iter::once((
        host_agent_member_id.to_string(),
        harness_core::agentfirm_api::TeamMembershipRole::Host,
    ))
    .chain(member_ids.iter().cloned().map(|member_id| {
        (
            member_id,
            harness_core::agentfirm_api::TeamMembershipRole::Member,
        )
    }))
    .map(|(agent_member_id, role)| {
        let id = format!("membership:{}:{}", team.id, agent_member_id);
        harness_core::agentfirm_api::TeamMembership {
            id: id.clone(),
            team_id: team.id.clone(),
            agent_member_id: agent_member_id.clone(),
            node_id: team.node_id.clone(),
            role,
            state: harness_core::agentfirm_api::TeamMembershipStatus::Active,
            membership_generation: 1,
            default_subscription_refs: vec![
                format!("direct:{agent_member_id}:{id}"),
                format!("team:{}:{id}", team.id),
            ],
            created_by: created_by.clone(),
            revision: 1,
            joined_at: created_at.to_string(),
            left_at: None,
        }
    })
    .collect()
}

/// Persist a freshly-built vNext Team plus its authoritative initial
/// memberships in one canonical trust-ledger commit.
pub(super) fn persist_new_team(
    store: &HarnessStore,
    execution_space_id: &str,
    actor: &harness_core::agentfirm_api::ActorRef,
    team: AgentTeam,
) -> CliResult<AgentTeam> {
    let memberships = initial_team_memberships(
        &team,
        &team.host_agent_id,
        &team.member_ids,
        actor,
        &team.created_at,
    );
    let result = store.create_agent_team(
        &harness_core::agentfirm_api::MutationContext {
            execution_space_id: execution_space_id.to_string(),
            authenticated_actor: actor.clone(),
            authority_actor: None,
            command_name: "team.create".into(),
            idempotency_key: format!("team-create:{}", team.id),
            expected_version: 0,
            request_fingerprint: None,
        },
        team,
        memberships,
    )?;
    Ok(result.projection)
}

/// Persist a freshly-built Agent Member. Mirrors the `agent create` CLI arm.
pub(super) fn http_host_work_context(body: &serde_json::Value) -> CliResult<WorkCommandContext> {
    Ok(WorkCommandContext {
        event_id: json_string(body, "event_id").unwrap_or_else(|| generated_id("work-event")),
        performed_by_actor: TeamActorRef {
            kind: TeamActorKind::Operator,
            id: json_string(body, "actor_id").unwrap_or_else(|| "operator:dashboard".to_string()),
            display_name: json_string(body, "actor_name"),
            authn_source: Some("http_operator".to_string()),
        },
        authority_actor: Some(TeamActorRef {
            kind: TeamActorKind::Host,
            id: "host".to_string(),
            display_name: None,
            authn_source: Some("http_host_authority".to_string()),
        }),
        causation_ref: json_string(body, "caused_by_message_id").map(|id| WorkCausationRef {
            kind: "team_message".to_string(),
            id,
        }),
        idempotency_key: json_string(body, "idempotency_key")
            .unwrap_or_else(|| generated_id("work-command")),
        created_at: now_string(),
        duplicate_ok: json_bool(body, "duplicate_ok").unwrap_or(false),
    })
}

pub(super) fn required_json_work_version(body: &serde_json::Value) -> CliResult<u64> {
    json_u64(body, "expected_version").ok_or_else(|| {
        CliError::Usage("missing or invalid JSON field: expected_version".to_string())
    })
}

pub(super) fn require_reviewed_member_before_work_rebind(
    store: &HarnessStore,
    team_run_id: &str,
    member_run_id: &str,
) -> CliResult<()> {
    let mut member = latest_member_runs_in_append_order(store)?
        .into_iter()
        .find(|member| member.id == member_run_id)
        .ok_or_else(|| CliError::Usage(format!("member run not found: {member_run_id}")))?;
    if member.team_run_id != team_run_id {
        return Err(CliError::Usage(format!(
            "member run {member_run_id} does not belong to team run {team_run_id}"
        )));
    }
    if member.is_external_interactive() {
        return Ok(());
    }
    let expected = member.clone();
    let (profile, probe_error) = refreshed_team_member_provider_profile(&member)?;
    let resolution = resolve_provider_compatibility(store, &profile, probe_error.as_deref())?;
    let refusal = provider_compatibility_block_reason(
        &member,
        &profile,
        &resolution,
        "receive rebound durable Work",
    );
    if apply_refreshed_provider_profile(&mut member, profile) {
        store_conflict_as_usage(store.compare_and_append_member_run(&expected, &member))?;
    }
    refusal
        .map(|reason| Err(CliError::Usage(reason)))
        .unwrap_or(Ok(()))
}

pub(super) fn create_team_work_value(
    store: &HarnessStore,
    team_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let owner_member_run_id = optional_json_string(body, "owner_member_run_id")?;
    let claim_mode = optional_json_string(body, "claim_mode")?
        .map(|raw| parse_work_claim_mode(&raw))
        .transpose()?
        .unwrap_or(if owner_member_run_id.is_some() {
            WorkClaimMode::HostAssign
        } else {
            WorkClaimMode::TeamClaim
        });
    let context = http_host_work_context(body)?;
    let work = Work {
        id: json_string(body, "id").unwrap_or_else(|| generated_id("work")),
        team_run_id: team_run_id.to_string(),
        accountable_team_id: None,
        assignee_membership_id: None,
        created_by_member_id: None,
        parent_work_id: optional_json_string(body, "parent_work_id")?,
        title: required_json_string(body, "title")?,
        context_markdown: json_string(body, "context_markdown").unwrap_or_default(),
        completion_criteria_markdown: required_json_string(body, "completion_criteria_markdown")?,
        phase: WorkPhase::Open,
        condition: WorkCondition::Normal,
        resolution: None,
        owner_member_id: None,
        active_member_run_id: owner_member_run_id,
        claim_mode,
        eligible_member_ids: json_string_array(body, "eligible_member_ids"),
        prerequisite_work_ids: json_string_array(body, "prerequisite_work_ids"),
        priority: optional_json_string(body, "priority")?
            .map(|raw| parse_work_priority(&raw))
            .transpose()?
            .unwrap_or(WorkPriority::Normal),
        created_by_actor: context.performed_by_actor.clone(),
        result_summary: None,
        blocker_reason: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        github_links: Vec::new(),
        version: 0,
        created_at: String::new(),
        updated_at: String::new(),
    };
    Ok(serde_json::to_value(store.insert_work(work, context)?)?)
}

/// Create one cross-Team WorkDelegation and its target root Work atomically.
/// This is the shared JSON service used by HTTP and MCP; the CLI keeps the
/// same Store contract through `team-run work delegate`.
pub(crate) fn create_work_delegation_value(
    store: &HarnessStore,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let source_run_id = required_json_string(body, "source_team_run_id")?;
    let source_work_id = required_json_string(body, "source_work_id")?;
    let source_version = required_json_work_version(body)?;
    let target_team_id = required_json_string(body, "target_agent_team_id")?;
    let source = store
        .latest_works()?
        .into_iter()
        .find(|work| work.id == source_work_id && work.team_run_id == source_run_id)
        .ok_or_else(|| {
            CliError::Usage(format!("Work not found: {source_run_id}/{source_work_id}"))
        })?;
    if source.version != source_version {
        return Err(CliError::Usage(format!(
            "DELEGATION_STALE_SOURCE: Work {source_work_id} is version {}, expected {source_version}",
            source.version
        )));
    }
    let source_owner = source.owner_member_id.clone().ok_or_else(|| {
        CliError::Usage("DELEGATION_NOT_AUTHORIZED: source Work has no durable owner".to_string())
    })?;
    let target_runs = latest_team_runs_in_append_order(store)?
        .into_iter()
        .filter(|run| run.agent_team_id == target_team_id)
        .filter(|run| {
            !matches!(
                run.status,
                TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
            )
        })
        .collect::<Vec<_>>();
    if target_runs.len() != 1 {
        return Err(CliError::Usage(format!(
            "DELEGATION_TARGET_INVALID: target Team {target_team_id} must have exactly one active TeamRun, found {}",
            target_runs.len()
        )));
    }
    let target_run = &target_runs[0];
    let context = http_host_work_context(body)?;
    let request_hash = content_hash_hex16(&context.idempotency_key);
    let target_work_id = json_string(body, "target_work_id")
        .unwrap_or_else(|| format!("delegated-work-{request_hash}"));
    let now = context.created_at.clone();
    let target_work = Work {
        id: target_work_id.clone(),
        team_run_id: target_run.id.clone(),
        accountable_team_id: Some(target_team_id.clone()),
        assignee_membership_id: None,
        parent_work_id: None,
        title: required_json_string(body, "target_title")?,
        context_markdown: json_string(body, "target_context_markdown").unwrap_or_default(),
        completion_criteria_markdown: required_json_string(
            body,
            "target_completion_criteria_markdown",
        )?,
        phase: WorkPhase::Open,
        condition: WorkCondition::Normal,
        resolution: None,
        owner_member_id: None,
        active_member_run_id: None,
        claim_mode: WorkClaimMode::TeamClaim,
        eligible_member_ids: json_string_array(body, "target_eligible_member_ids"),
        prerequisite_work_ids: Vec::new(),
        priority: optional_json_string(body, "target_priority")?
            .map(|raw| parse_work_priority(&raw))
            .transpose()?
            .unwrap_or(source.priority),
        created_by_actor: context.performed_by_actor.clone(),
        created_by_member_id: None,
        result_summary: None,
        blocker_reason: None,
        artifact_refs: Vec::new(),
        check_refs: Vec::new(),
        github_links: Vec::new(),
        version: 0,
        created_at: String::new(),
        updated_at: String::new(),
    };
    let delegation = WorkDelegation {
        id: json_string(body, "delegation_id")
            .unwrap_or_else(|| format!("work-delegation-{request_hash}")),
        source_work_ref: WorkRef {
            team_run_id: source_run_id,
            work_id: source_work_id,
        },
        source_work_version: source_version,
        source_owner_member_id: source_owner,
        created_by_member_run_id: None,
        target_agent_team_id: target_team_id,
        target_work_ref: WorkRef {
            team_run_id: target_run.id.clone(),
            work_id: target_work_id,
        },
        delegated_by_actor: context.performed_by_actor.clone(),
        state: WorkDelegationState::Active,
        resolution_summary: None,
        blocker_reason: None,
        version: 1,
        created_at: now.clone(),
        updated_at: now,
    };
    let (delegation, target_work) =
        store.create_work_delegation_with_target_work(delegation, target_work, context)?;
    Ok(serde_json::json!({
        "delegation": delegation,
        "target_work": target_work,
    }))
}

pub(crate) fn cancel_work_delegation_value(
    store: &HarnessStore,
    delegation_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let delegation = store.cancel_work_delegation(
        delegation_id,
        required_json_work_version(body)?,
        &required_json_string(body, "reason")?,
        http_host_work_context(body)?,
    )?;
    Ok(serde_json::to_value(delegation)?)
}

pub(super) fn mutate_team_work_value(
    store: &HarnessStore,
    team_run_id: &str,
    work_id: &str,
    operation: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    if operation == "review" {
        return Err(CliError::Usage(
            "legacy Work review is retired; use canonical AgentFirm GateEvaluation and accept_work"
                .to_string(),
        ));
    }
    let current = store
        .latest_works()?
        .into_iter()
        .find(|work| work.id == work_id && work.team_run_id == team_run_id)
        .ok_or_else(|| CliError::Usage(format!("Work not found: {work_id}")))?;
    let expected_version = required_json_work_version(body)?;
    if current.version != expected_version {
        return Err(CliError::Usage(format!(
            "VERSION_CONFLICT: Work {work_id} is version {}, expected {expected_version}",
            current.version
        )));
    }
    let rebind_member_run_id = if operation == "rebind" {
        let member_run_id = required_json_string(body, "member_run_id")?;
        require_reviewed_member_before_work_rebind(store, team_run_id, &member_run_id)?;
        Some(member_run_id)
    } else {
        None
    };
    let context = http_host_work_context(body)?;
    let (work, event_op, event_summary) = match operation {
        "assign" => {
            let member_run_id = required_json_string(body, "member_run_id")?;
            let work = store.assign_work(work_id, expected_version, &member_run_id, context)?;
            (
                work,
                "assigned",
                format!("Work assigned to {member_run_id}"),
            )
        }
        "rebind" => {
            let target = rebind_member_run_id
                .as_deref()
                .expect("rebind target validated before mutation");
            let work = store.rebind_work(work_id, expected_version, target, context)?;
            (work, "rebound", format!("Work rebound to {target}"))
        }
        "block" => {
            let reason = required_json_string(body, "reason")?;
            let work = store.block_work_as_host(work_id, expected_version, &reason, context)?;
            (work, "blocked", format!("Work blocked by host: {reason}"))
        }
        "resume" => {
            let resolution = required_json_string(body, "resolution")?;
            let work =
                store.resume_work_as_host(work_id, expected_version, &resolution, context)?;
            (
                work,
                "resumed",
                format!("Work resumed by host: {resolution}"),
            )
        }
        "release" => {
            let work = store.release_work_as_host(work_id, expected_version, context)?;
            (work, "released", "Work released by host".to_string())
        }
        "request-changes" => {
            let reason = required_json_string(body, "reason")?;
            let work = store.request_work_changes(work_id, expected_version, &reason, context)?;
            (
                work,
                "changes_requested",
                format!("Changes requested: {reason}"),
            )
        }
        "accept" => {
            let work = store.accept_work(work_id, expected_version, context)?;
            let title = work.title.clone();
            (work, "accepted", format!("Work accepted: {title}"))
        }
        "cancel" => {
            let reason = required_json_string(body, "reason")?;
            let work = store.cancel_work(work_id, expected_version, &reason, context)?;
            (work, "cancelled", format!("Work cancelled: {reason}"))
        }
        other => {
            return Err(CliError::Usage(format!(
                "unsupported operator Work operation: {other}"
            )))
        }
    };
    append_work_event(
        store,
        &work,
        TeamRunEventSourceKind::Host,
        None,
        event_op,
        &event_summary,
    )?;
    Ok(serde_json::to_value(work)?)
}

pub(super) fn create_team_value(
    store: &HarnessStore,
    execution_space_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let host_agent_member_id = json_string(body, "host_agent_member_id")
        .or_else(|| json_string(body, "host_agent_id"))
        .ok_or_else(|| CliError::Usage("missing field host_agent_member_id".into()))?;
    let mut member_ids = json_string_array(body, "member");
    member_ids.retain(|member_id| member_id != &host_agent_member_id);
    let legacy_mission_id =
        json_string(body, "legacy_mission_id").or_else(|| json_string(body, "mission_id"));
    let timestamp = now_string();
    let team = AgentTeam {
        id: json_string(body, "id").unwrap_or_else(|| generated_id("team")),
        name: required_json_string(body, "name")?,
        description: required_json_string(body, "description")?,
        node_id: required_json_string(body, "node_id")?,
        status: AgentTeamStatus::Active,
        revision: 1,
        legacy_mission_id: legacy_mission_id.clone(),
        trashed_at: None,
        created_at: timestamp.clone(),
        updated_at: timestamp,
        mission_id: legacy_mission_id.unwrap_or_default(),
        host_agent_id: host_agent_member_id,
        member_ids,
    };
    let actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::Human,
        id: json_string(body, "actor_id").unwrap_or_else(|| "operator:http".into()),
    };
    Ok(serde_json::to_value(persist_new_team(
        store,
        execution_space_id,
        &actor,
        team,
    )?)?)
}

/// POST /v1/team-runs — create a team run from the JSON body (same semantics
/// as `team-run create`; the host surface defaults to "http").
pub(super) fn create_team_run_value(
    store: &HarnessStore,
    project_context: Option<&ProjectContext>,
    execution_space_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    if body.get("wave_index").is_some() || body.get("wave_id").is_some() {
        return Err(CliError::Usage(
            "JSON fields wave_id and wave_index are Legacy-only; supply agent_team_id and derive Mission through AgentTeam".to_string(),
        ));
    }
    let agent_team_id = optional_json_string(body, "agent_team_id")?;
    let member_values = body
        .get("members")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let mut members = Vec::new();
    for (member_index, member) in member_values.iter().enumerate() {
        let owned_paths = match member.get("owned_paths") {
            None => Vec::new(),
            Some(serde_json::Value::Array(paths)) => paths
                .iter()
                .enumerate()
                .map(|(path_index, path)| {
                    path.as_str().map(str::to_string).ok_or_else(|| {
                        CliError::Usage(format!(
                            "members[{member_index}].owned_paths[{path_index}] must be a string"
                        ))
                    })
                })
                .collect::<CliResult<Vec<_>>>()?,
            Some(_) => {
                return Err(CliError::Usage(format!(
                    "members[{member_index}].owned_paths must be an array"
                )));
            }
        };
        members.push(TeamMemberSpec {
            agent_member_id: required_json_string(member, "agent_member_id")?,
            name: required_json_string(member, "name")?,
            role: required_json_string(member, "role")?,
            provider: required_json_string(member, "provider")?,
            execution_mode: optional_json_string(member, "execution_mode")?,
            model: optional_json_string(member, "model")?,
            effort: optional_json_string(member, "effort")?,
            service_tier: optional_json_string(member, "service_tier")?,
            provider_cwd_hint: optional_json_string(member, "provider_cwd_hint")?,
            owned_paths,
            resume_native_session_id: optional_json_string(member, "resume_native_session_id")?,
            initial_work: optional_json_string(member, "initial_work")?,
        });
    }
    if members.is_empty() {
        if let Some(team_id) = agent_team_id.as_deref() {
            members = team_member_specs_from_definition(store, execution_space_id, team_id)?;
        }
    }
    let host_thread_id = optional_json_string(body, "host_thread_id")?;
    let requested_host_mode = optional_json_string(body, "host_runtime_mode")?;
    let team_id = agent_team_id.as_deref().ok_or_else(|| {
        CliError::Usage("agent_team_id is required for every AgentTeamRun".to_string())
    })?;
    let host_control_mode = configure_host_runtime_mode(
        store,
        execution_space_id,
        team_id,
        &mut members,
        requested_host_mode.as_deref(),
    )?;
    let budget_limit_usd = match body.get("budget_limit_usd") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(value.as_f64().ok_or_else(|| {
            CliError::Usage("JSON field budget_limit_usd must be a number or null".to_string())
        })?),
    };
    let host_surface =
        optional_json_string(body, "host_surface")?.unwrap_or_else(|| "http".to_string());
    let created = create_team_run(
        store,
        project_context,
        Some(execution_space_id),
        optional_json_string(body, "execution_root")?,
        &required_json_string(body, "objective")?,
        budget_limit_usd,
        &host_surface,
        host_thread_id,
        host_control_mode,
        optional_json_string(body, "previous_run_id")?,
        agent_team_id,
        None,
        None,
        &members,
    )?;
    Ok(created_team_run_json(&created))
}

/// POST /v1/team-runs/{id}/transition — attempt lifecycle. Body `{status}`; only
/// `reviewing → completed` and
/// `planning|waiting|reviewing → cancelled` are legal
/// (same logic as `team-run complete|cancel`, so CLI and UI cannot diverge).
pub(super) fn transition_team_run_value(
    store: &HarnessStore,
    team_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let target = parse_team_run_status(&required_json_string(body, "status")?)?;
    let run = transition_team_run(store, team_run_id, target)?;
    Ok(serde_json::to_value(run)?)
}

/// POST /v1/team-runs/{id}/messages — route a message inside the run (same
/// semantics as `team-run send`).
#[cfg(any())]
pub(super) fn send_team_message_value(
    store: &HarnessStore,
    team_run_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let recipient_runtime_ids = json_string_array(body, "recipient_runtime_ids");
    if recipient_runtime_ids.is_empty() {
        return Err(CliError::Usage(
            "missing JSON field: recipient_runtime_ids".to_string(),
        ));
    }
    // Bare Dashboard writes default to the Operator control plane, which is
    // response-required under the sender-aware default (ADR 0012: the
    // Dashboard is a control plane, so an Operator reply must wake an idle
    // member). An HTTP caller speaking FOR a member must say so explicitly
    // with `sender_kind`/`sender_id`; `sender_runtime_id` alone is a historical
    // projection field and does not carry provenance.
    let sender_kind = json_string(body, "sender_kind").unwrap_or_else(|| "operator".to_string());
    let sender_id = json_string(body, "sender_id").unwrap_or_else(|| {
        if sender_kind == "host" || sender_kind == "member_run" {
            json_string(body, "sender_runtime_id").unwrap_or_else(|| "operator".to_string())
        } else {
            "operator".to_string()
        }
    });
    let message = send_team_message_as_work(
        store,
        team_run_id,
        TeamActorRef {
            kind: parse_team_actor_kind(&sender_kind)?,
            id: sender_id,
            display_name: json_string(body, "sender_name"),
            authn_source: Some("http_request".to_string()),
        },
        recipient_runtime_ids,
        parse_team_message_kind(&required_json_string(body, "kind")?)?,
        &required_json_string(body, "body")?,
        json_string(body, "work_id"),
        json_string(body, "correlation_id"),
        json_string(body, "causation_id"),
        json_string(body, "source_plan_ref"),
        // Strict: a present-but-not-a-string `response_intent` is a caller
        // error, never a silent fall-through to the default.
        optional_json_string(body, "response_intent")?
            .map(|intent| parse_team_message_response_intent(&intent))
            .transpose()?,
    )?;
    Ok(serde_json::to_value(message)?)
}

/// POST /v1/agents — build an Agent Member from the JSON body and persist it.
/// Does NOT start a runtime: `--start` / runtime spawn stays a separate action.
pub(super) fn json_string(body: &serde_json::Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

pub(super) fn required_json_string(body: &serde_json::Value, key: &str) -> CliResult<String> {
    json_string(body, key).ok_or_else(|| CliError::Usage(format!("missing JSON field: {key}")))
}

pub(super) fn optional_json_string(
    body: &serde_json::Value,
    key: &str,
) -> CliResult<Option<String>> {
    match body.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|text| Some(text.to_string()))
            .ok_or_else(|| CliError::Usage(format!("JSON field {key} must be a string or null"))),
    }
}

pub(super) fn json_bool(body: &serde_json::Value, key: &str) -> Option<bool> {
    body.get(key).and_then(|value| value.as_bool())
}

pub(super) fn json_u64(body: &serde_json::Value, key: &str) -> Option<u64> {
    body.get(key).and_then(|value| value.as_u64())
}

pub(super) fn json_string_array(body: &serde_json::Value, key: &str) -> Vec<String> {
    body.get(key)
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn optional_json_string_array(
    body: &serde_json::Value,
    key: &str,
) -> CliResult<Vec<String>> {
    match body.get(key) {
        None => Ok(Vec::new()),
        Some(serde_json::Value::Array(values)) => values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                value.as_str().map(str::to_string).ok_or_else(|| {
                    CliError::Usage(format!("JSON field {key}[{index}] must be a string"))
                })
            })
            .collect(),
        Some(_) => Err(CliError::Usage(format!(
            "JSON field {key} must be an array"
        ))),
    }
}

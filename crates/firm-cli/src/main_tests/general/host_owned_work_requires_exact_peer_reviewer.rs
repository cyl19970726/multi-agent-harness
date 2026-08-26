use super::*;

fn create_assigned_review_work(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    owner: &ProviderRuntimeProjection,
    work_id: &str,
) -> Work {
    harness_application::WorkApplication::new(store)
        .create(harness_application::CreateWorkCommand {
            work_id: work_id.into(),
            team_run_id: created.team_run.id.clone(),
            accountable_team_id: created.team_run.agent_team_id.clone(),
            title: format!("Review {work_id}"),
            context_markdown: "Exercise exact Team review authority".into(),
            completion_criteria_markdown: "Submit evidence and receive independent review".into(),
            claim_mode: WorkClaimMode::HostAssign,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: WorkPriority::Normal,
            initial_member_run_id: Some(owner.id.clone()),
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            expected_version: 0,
            context: WorkCommandContext {
                event_id: format!("create-{work_id}"),
                performed_by_actor: created
                    .team_run
                    .host_actor
                    .clone()
                    .expect("exact Team Host"),
                authority_actor: None,
                causation_ref: None,
                idempotency_key: format!("create-{work_id}"),
                created_at: now_string(),
                duplicate_ok: false,
            },
        })
        .expect("create assigned review Work")
}

#[allow(clippy::too_many_arguments)]
fn role_action(
    store: &HarnessStore,
    lease: &TeamSupervisorLease,
    supervisor_valid: &AtomicBool,
    authority_gate: &Mutex<()>,
    member: &ProviderRuntimeProjection,
    token: &str,
    path: String,
    version: u64,
    key: &str,
    body: serde_json::Value,
    confirm: Option<&str>,
) -> CliResult<serde_json::Value> {
    dispatch_local_live_member_control(
        store,
        &lease.supervisor_id,
        lease.generation,
        supervisor_valid,
        authority_gate,
        LiveMemberControlRequest::RoleAction {
            team_run_id: member.team_run_id.clone(),
            member_run_id: member.id.clone(),
            capability_token: token.to_string(),
            path,
            expected_version: version,
            idempotency_key: key.into(),
            body,
            confirmed_action: confirm.map(str::to_string),
        },
    )
}

#[allow(clippy::too_many_arguments)]
fn advance_to_review(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    lease: &TeamSupervisorLease,
    supervisor_valid: &AtomicBool,
    authority_gate: &Mutex<()>,
    owner: &ProviderRuntimeProjection,
    token: &str,
    work: &Work,
) -> Work {
    let base = format!(
        "/v1/agentfirm/team-runs/{}/works/{}",
        created.team_run.id, work.id
    );
    role_action(
        store,
        lease,
        supervisor_valid,
        authority_gate,
        owner,
        token,
        format!("{base}/start"),
        work.version,
        &format!("start-{}", work.id),
        serde_json::json!({"action":"start_work"}),
        None,
    )
    .expect("owner starts Work");
    role_action(
        store,
        lease,
        supervisor_valid,
        authority_gate,
        owner,
        token,
        format!("{base}/submit"),
        work.version + 1,
        &format!("submit-{}", work.id),
        serde_json::json!({
            "action":"submit_work",
            "result_summary":format!("{} candidate", work.id),
            "candidate_revision":format!("revision:{}", work.id),
            "artifact_refs":[format!("artifact:{}", work.id)]
        }),
        None,
    )
    .expect("owner submits Work");
    store
        .latest_works()
        .expect("latest Works")
        .into_iter()
        .find(|candidate| candidate.id == work.id)
        .expect("submitted Work")
}

#[test]
fn host_owned_work_requires_exact_active_peer_while_member_work_remains_host_reviewed() {
    let (store, _root) = temp_store("host-owned-peer-review");
    let created = create_two_member_team_run(&store);
    let host = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "host")
        .expect("Host MemberRun")
        .clone();
    let reviewer = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "agent-builder-a")
        .expect("reviewer MemberRun")
        .clone();
    let worker = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "agent-builder-b")
        .expect("worker MemberRun")
        .clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-host-owned-peer-review",
            std::process::id(),
            "test://host-owned-peer-review",
            current_unix_ms_u64(),
            60_000,
        )
        .expect("acquire Supervisor lease");
    ensure_test_runtime_fabric(&store, &created, &lease);
    let ledger = TeamRunLedger::new(
        &store,
        &created.team_run.id,
        &lease.supervisor_id,
        lease.generation,
        Arc::new(AtomicBool::new(true)),
    );
    for member in [&host, &reviewer, &worker] {
        transition_provider_session_for_member(
            &ledger,
            member,
            harness_core::agentfirm_api::AgentSessionStatus::Active,
        )
        .expect("activate exact reviewer session");
    }
    let host_token = "a".repeat(64);
    let reviewer_token = "b".repeat(64);
    let worker_token = "c".repeat(64);
    let mut _live_controls = Vec::new();
    for (member, token) in [
        (&host, host_token.as_str()),
        (&reviewer, reviewer_token.as_str()),
        (&worker, worker_token.as_str()),
    ] {
        let capability = test_collaboration_capability(&store, &lease, member, token);
        _live_controls.push(register_live_member_control(member, &capability, 1));
    }
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());

    let host_work = create_assigned_review_work(&store, &created, &host, "host-owned-work");
    let host_review = advance_to_review(
        &store,
        &created,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &host,
        &host_token,
        &host_work,
    );
    let accept_path = format!(
        "/v1/agentfirm/teams/{}/works/{}/accept",
        created.team_run.agent_team_id, host_review.id
    );
    let reviewer_identity = crate::role_views_api::ReadIdentity {
        actor: harness_core::agentfirm_api::ActorRef {
            kind: harness_core::agentfirm_api::ActorKind::AgentMember,
            id: reviewer.agent_member_id.clone(),
        },
        authority_actors: Vec::new(),
        local_operator: false,
    };
    let reviewer_view = crate::role_views_api::member_view(
        &lease.execution_space_id,
        &store,
        &reviewer.id,
        Some(&reviewer_identity),
        None,
    )
    .expect("build exact reviewer MemberWorkbench");
    assert!(reviewer_view["data"]["reviewable_host_works"]
        .as_array()
        .is_some_and(|works| works.iter().any(|work| work["work_id"] == host_review.id)));
    assert!(reviewer_view["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions.iter().any(|action| {
            action["kind"] == "accept_work"
                && action["target_ref"]["id"] == host_review.id
                && action["disabled_reason"].is_null()
        })));
    let before_self_accept = durable_store_file_bytes(&store);
    let self_accept = role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &host,
        &host_token,
        accept_path.clone(),
        host_review.version,
        "host-self-accept",
        serde_json::json!({"action":"accept_work"}),
        Some("accept"),
    )
    .expect_err("Host owner cannot accept its own candidate");
    assert!(self_accept
        .to_string()
        .contains("accountable Work owner cannot accept its own candidate"));
    assert_eq!(
        durable_store_file_bytes(&store),
        before_self_accept,
        "rejected self-accept has zero durable effects"
    );
    role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &reviewer,
        &reviewer_token,
        accept_path,
        host_review.version,
        "peer-accept-host-work",
        serde_json::json!({"action":"accept_work"}),
        Some("accept"),
    )
    .expect("exact active Team peer accepts Host-owned Work");
    let accepted_host_work = store
        .latest_works()
        .expect("latest Works")
        .into_iter()
        .find(|candidate| candidate.id == host_review.id)
        .expect("accepted Host Work");
    assert_eq!(
        accepted_host_work.resolution,
        Some(WorkResolution::Accepted)
    );

    let member_work = create_assigned_review_work(&store, &created, &worker, "member-owned-work");
    let member_review = advance_to_review(
        &store,
        &created,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &worker,
        &worker_token,
        &member_work,
    );
    let member_accept_path = format!(
        "/v1/agentfirm/teams/{}/works/{}/accept",
        created.team_run.agent_team_id, member_review.id
    );
    let peer_member_accept = role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &reviewer,
        &reviewer_token,
        member_accept_path.clone(),
        member_review.version,
        "peer-cannot-accept-member-work",
        serde_json::json!({"action":"accept_work"}),
        Some("accept"),
    )
    .expect_err("ordinary Member Work remains Host-reviewed");
    assert!(peer_member_accept
        .to_string()
        .contains("UNAUTHORIZED_ACTOR"));
    role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &host,
        &host_token,
        member_accept_path,
        member_review.version,
        "host-accept-member-work",
        serde_json::json!({"action":"accept_work"}),
        Some("accept"),
    )
    .expect("exact Host acceptance of Member Work remains unchanged");

    let reviewer_actor = harness_core::agentfirm_api::ActorRef {
        kind: harness_core::agentfirm_api::ActorKind::AgentMember,
        id: reviewer.agent_member_id.clone(),
    };
    assert!(crate::agentfirm_api::work_review_authorized(
        &store,
        &lease.execution_space_id,
        &reviewer_actor,
        &created.team_run.agent_team_id,
        &accepted_host_work.id,
    )
    .expect("resolve active reviewer"));
    let reviewer_membership = store
        .fabric_team_memberships(&lease.execution_space_id)
        .expect("reviewer memberships")
        .into_iter()
        .find(|membership| membership.agent_member_id == reviewer.agent_member_id)
        .expect("reviewer active membership");
    store
        .leave_team_membership(
            &harness_core::agentfirm_api::MutationContext {
                execution_space_id: lease.execution_space_id.clone(),
                authenticated_actor: reviewer_actor.clone(),
                authority_actor: None,
                command_name: "membership.leave".into(),
                idempotency_key: "reviewer-membership-leave".into(),
                expected_version: reviewer_membership.revision,
                request_fingerprint: None,
            },
            &reviewer_membership.id,
            "unix-ms:reviewer-left",
        )
        .expect("deactivate reviewer membership");
    assert!(!crate::agentfirm_api::work_review_authorized(
        &store,
        &lease.execution_space_id,
        &reviewer_actor,
        &created.team_run.agent_team_id,
        &accepted_host_work.id,
    )
    .expect("resolve inactive reviewer"));
}

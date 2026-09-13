use super::*;

/// One authenticated entrance owns member Work writes. These cases pin the two
/// gaps that used to push callers off it: a Member could not create follow-up
/// Work through the Role Action (the route was Host-only, so the skill pointed
/// members at the unauthenticated local CLI), and a managed Host had no bound
/// `cancel` (the route existed but no command reached it).
#[allow(clippy::too_many_arguments)]
fn work_role_action(
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

fn latest_work(store: &HarnessStore, work_id: &str) -> Work {
    store
        .latest_works()
        .expect("latest Works")
        .into_iter()
        .find(|work| work.id == work_id)
        .unwrap_or_else(|| panic!("Work {work_id} exists"))
}

struct EntranceFixture {
    store: HarnessStore,
    _root: std::path::PathBuf,
    created: CreatedTeamRun,
    lease: TeamSupervisorLease,
    host: ProviderRuntimeProjection,
    member: ProviderRuntimeProjection,
    host_token: String,
    member_token: String,
    _live_controls: Vec<(
        ControlReceiver<MemberControlCommand>,
        LiveMemberControlRegistration,
    )>,
}

fn entrance_fixture(tag: &str) -> EntranceFixture {
    let (store, _root) = temp_store(tag);
    let created = create_two_member_team_run(&store);
    let host = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "host")
        .expect("Host MemberRun")
        .clone();
    let member = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "agent-builder-a")
        .expect("member MemberRun")
        .clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            &format!("supervisor-{tag}"),
            std::process::id(),
            &format!("test://{tag}"),
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
    for runtime in [&host, &member] {
        transition_provider_session_for_member(
            &ledger,
            runtime,
            harness_core::agentfirm_api::AgentSessionStatus::Active,
        )
        .expect("activate exact member session");
    }
    let host_token = "a".repeat(64);
    let member_token = "b".repeat(64);
    let mut live_controls = Vec::new();
    for (runtime, token) in [
        (&host, host_token.as_str()),
        (&member, member_token.as_str()),
    ] {
        let capability = test_collaboration_capability(&store, &lease, runtime, token);
        live_controls.push(register_live_member_control(runtime, &capability, 1));
    }
    EntranceFixture {
        store,
        _root,
        created,
        lease,
        host,
        member,
        host_token,
        member_token,
        _live_controls: live_controls,
    }
}

#[test]
fn an_active_member_creates_unassigned_follow_up_work_through_the_authenticated_entrance() {
    let fixture = entrance_fixture("member-work-create-entrance");
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());
    let create_path = format!(
        "/v1/agentfirm/team-runs/{}/works",
        fixture.created.team_run.id
    );

    work_role_action(
        &fixture.store,
        &fixture.lease,
        &supervisor_valid,
        &authority_gate,
        &fixture.member,
        &fixture.member_token,
        create_path.clone(),
        0,
        "member-creates-follow-up",
        serde_json::json!({
            "action":"create_work",
            "work_id":"member-follow-up",
            "title":"Follow-up inside my acceptance boundary",
            "completion_criteria_markdown":"the follow-up is observably done",
            "claim_mode":"team_claim"
        }),
        None,
    )
    .expect("an active Member creates follow-up Work through the one entrance");
    let created_work = latest_work(&fixture.store, "member-follow-up");
    assert_eq!(
        created_work.created_by_member_id.as_deref(),
        Some(fixture.member.agent_member_id.as_str()),
        "member-created Work carries the creator's own durable identity"
    );
    assert_eq!(
        created_work.owner_member_id, None,
        "a Member never creates responsibility"
    );
    assert_eq!(created_work.assignee_membership_id, None);
    assert_eq!(created_work.active_member_run_id, None);
    assert_eq!(created_work.phase, harness_core::WorkPhase::Open);

    // The closed intent has no owner/assignee field at all, so a body that
    // tries to carry responsibility is refused before any Store write.
    let before_owned_create = durable_store_file_bytes(&fixture.store);
    let owned = work_role_action(
        &fixture.store,
        &fixture.lease,
        &supervisor_valid,
        &authority_gate,
        &fixture.member,
        &fixture.member_token,
        create_path.clone(),
        0,
        "member-cannot-create-owned-work",
        serde_json::json!({
            "action":"create_work",
            "work_id":"member-owned-create",
            "title":"Work I assign to myself",
            "completion_criteria_markdown":"never reached",
            "owner_member_id":"agent-builder-a",
            "assignee_membership_id":"membership-agent-builder-a"
        }),
        None,
    )
    .expect_err("a Member cannot name an owner or assignee at creation");
    assert!(
        owned.to_string().contains("INVALID_STATE_TRANSITION"),
        "{owned}"
    );
    assert_eq!(
        durable_store_file_bytes(&fixture.store),
        before_owned_create,
        "a refused creation has zero durable effects"
    );

    // Host creation is unchanged and still carries no member provenance.
    work_role_action(
        &fixture.store,
        &fixture.lease,
        &supervisor_valid,
        &authority_gate,
        &fixture.host,
        &fixture.host_token,
        create_path,
        0,
        "host-creates-work",
        serde_json::json!({
            "action":"create_work",
            "work_id":"host-created",
            "title":"Host decomposition",
            "completion_criteria_markdown":"the slice is observably done"
        }),
        None,
    )
    .expect("Host creation through the same entrance is unchanged");
    let host_created = latest_work(&fixture.store, "host-created");
    assert_eq!(host_created.created_by_member_id, None);
    assert_eq!(host_created.owner_member_id, None);
}

#[test]
fn bound_work_cancel_is_host_only_and_needs_no_local_cli_fallback() {
    let fixture = entrance_fixture("member-work-cancel-entrance");
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());
    let create_path = format!(
        "/v1/agentfirm/team-runs/{}/works",
        fixture.created.team_run.id
    );
    work_role_action(
        &fixture.store,
        &fixture.lease,
        &supervisor_valid,
        &authority_gate,
        &fixture.host,
        &fixture.host_token,
        create_path,
        0,
        "cancel-fixture-create",
        serde_json::json!({
            "action":"create_work",
            "work_id":"cancel-target",
            "title":"Superseded slice",
            "completion_criteria_markdown":"never reached"
        }),
        None,
    )
    .expect("Host creates the Work to cancel");
    let target = latest_work(&fixture.store, "cancel-target");
    let cancel_path = format!(
        "/v1/agentfirm/team-runs/{}/works/cancel-target/cancel",
        fixture.created.team_run.id
    );

    let before_member_cancel = durable_store_file_bytes(&fixture.store);
    let member_cancel = work_role_action(
        &fixture.store,
        &fixture.lease,
        &supervisor_valid,
        &authority_gate,
        &fixture.member,
        &fixture.member_token,
        cancel_path.clone(),
        target.version,
        "member-cannot-cancel",
        serde_json::json!({"action":"cancel_work","reason":"not mine to cancel"}),
        Some("cancel"),
    )
    .expect_err("cancellation stays Host authority");
    assert!(
        member_cancel.to_string().contains("UNAUTHORIZED_ACTOR"),
        "{member_cancel}"
    );
    assert_eq!(
        durable_store_file_bytes(&fixture.store),
        before_member_cancel,
        "a refused cancellation has zero durable effects"
    );

    // The confirmation is a server-side gate, not a client convenience.
    let unconfirmed = work_role_action(
        &fixture.store,
        &fixture.lease,
        &supervisor_valid,
        &authority_gate,
        &fixture.host,
        &fixture.host_token,
        cancel_path.clone(),
        target.version,
        "host-cancel-unconfirmed",
        serde_json::json!({"action":"cancel_work","reason":"superseded"}),
        None,
    )
    .expect_err("cancel requires the exact server confirmation");
    assert!(
        unconfirmed.to_string().contains("CONFIRMATION_REQUIRED"),
        "{unconfirmed}"
    );

    work_role_action(
        &fixture.store,
        &fixture.lease,
        &supervisor_valid,
        &authority_gate,
        &fixture.host,
        &fixture.host_token,
        cancel_path,
        target.version,
        "host-cancels",
        serde_json::json!({"action":"cancel_work","reason":"superseded by the new slice"}),
        Some("cancel"),
    )
    .expect("the managed Host cancels through the bound entrance");
    let cancelled = latest_work(&fixture.store, "cancel-target");
    assert_eq!(
        cancelled.resolution,
        Some(harness_core::WorkResolution::Cancelled)
    );
}

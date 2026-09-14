use super::*;

/// Full 40-hex candidate revision used by this fixture's compliant
/// submit_work payloads (the #787 submit gate requires SHA + porcelain).
const SUBMIT_FIXTURE_SHA: &str = "3123456789abcdef0123456789abcdef01234567";

fn create_assigned_review_work(
    store: &HarnessStore,
    created: &CreatedTeamRun,
    lease: &TeamSupervisorLease,
    owner: &ProviderRuntimeProjection,
    work_id: &str,
) -> Work {
    let work = harness_application::WorkApplication::new(store)
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
        .expect("create review Work");
    assign_test_work_to_member(store, &lease.execution_space_id, created, owner, &work)
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
            "result_summary":format!("SHA {}: {} candidate.\ngit status --porcelain: empty", SUBMIT_FIXTURE_SHA, work.id),
            "candidate_revision":SUBMIT_FIXTURE_SHA,
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

fn receive_bound_work(
    ledger: &TeamRunLedger,
    owner: &ProviderRuntimeProjection,
    provider_receipt_id: &str,
) {
    let claimed = claim_canonical_work_for_member(ledger, owner)
        .expect("claim exact bound Work delivery")
        .expect("one exact bound Work delivery");
    ledger
        .complete_work_delivery(&claimed, provider_receipt_id)
        .expect("record exact provider receipt before semantic Result");
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

    let host_work = create_assigned_review_work(&store, &created, &lease, &host, "host-owned-work");
    bind_test_responsible_work_execution(&store, &lease, &host, &host_work);
    receive_bound_work(&ledger, &host, "provider-receipt:host-owned-work");
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
    for (kind, id) in [
        (harness_core::agentfirm_api::ActorKind::Human, "operator"),
        (
            harness_core::agentfirm_api::ActorKind::Service,
            "review-service",
        ),
    ] {
        let before_generic_accept = durable_store_file_bytes(&store);
        let error = crate::agentfirm_api::execute(
            &store,
            crate::agentfirm_api::AuthenticatedMutation {
                execution_space_id: lease.execution_space_id.clone(),
                actor: harness_core::agentfirm_api::ActorRef {
                    kind,
                    id: id.into(),
                },
                authorized_authority_actors: Vec::new(),
                idempotency_key: format!("generic-{id}-cannot-accept"),
                expected_version: host_review.version,
                request_fingerprint: None,
            },
            crate::agentfirm_api::TrustCommand::AcceptWork {
                team_id: created.team_run.agent_team_id.clone(),
                work_id: host_review.id.clone(),
                updated_at: now_string(),
            },
        )
        .expect_err("generic Trust HTTP/CLI actor cannot bypass Work reviewer authority");
        assert!(error.to_string().contains("UNAUTHORIZED_ACTOR"));
        assert_eq!(
            durable_store_file_bytes(&store),
            before_generic_accept,
            "generic Trust rejection has zero durable effects"
        );
    }
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

    let member_work =
        create_assigned_review_work(&store, &created, &lease, &worker, "member-owned-work");
    bind_test_responsible_work_execution(&store, &lease, &worker, &member_work);
    receive_bound_work(&ledger, &worker, "provider-receipt:member-owned-work");
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

/// Review authority is symmetric. Whoever may accept a Work may also return it
/// for changes: the exact Host for ordinary Member Work, and for Host-owned
/// Work either the Host or one exact active non-owner Team peer. Before this,
/// a peer could accept Host-owned Work but had to fall back to a Message to
/// ask for changes.
#[test]
fn host_owned_work_changes_may_be_requested_by_the_same_exact_peer_that_may_accept_it() {
    let (store, _root) = temp_store("host-owned-peer-request-changes");
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
            "supervisor-host-owned-peer-request-changes",
            std::process::id(),
            "test://host-owned-peer-request-changes",
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

    let host_work = create_assigned_review_work(&store, &created, &lease, &host, "host-owned-rc");
    bind_test_responsible_work_execution(&store, &lease, &host, &host_work);
    receive_bound_work(&ledger, &host, "provider-receipt:host-owned-rc");
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
    let request_changes_path = |work_id: &str| {
        format!(
            "/v1/agentfirm/teams/{}/works/{work_id}/request-changes",
            created.team_run.agent_team_id
        )
    };

    // The exact active non-owner peer returns Host-owned Work for changes.
    role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &reviewer,
        &reviewer_token,
        request_changes_path(&host_review.id),
        host_review.version,
        "peer-request-changes-host-work",
        serde_json::json!({"action":"request_changes","reason":"name the exact failing gate"}),
        None,
    )
    .expect("exact active Team peer requests changes on Host-owned Work");
    let returned = store
        .latest_works()
        .expect("latest Works")
        .into_iter()
        .find(|candidate| candidate.id == host_review.id)
        .expect("returned Host Work");
    assert_eq!(returned.phase, harness_core::WorkPhase::Open);
    assert_eq!(returned.condition, harness_core::WorkCondition::Normal);
    assert_eq!(returned.resolution, None);
    assert_eq!(
        returned.blocker_reason.as_deref(),
        Some("name the exact failing gate")
    );
    assert_eq!(returned.version, host_review.version + 1);
    assert_eq!(
        returned.owner_member_id.as_deref(),
        Some("host"),
        "peer review never moves responsibility"
    );

    // Ordinary Member Work stays Host-reviewed on both review verbs.
    let member_work = create_assigned_review_work(&store, &created, &lease, &worker, "member-rc");
    bind_test_responsible_work_execution(&store, &lease, &worker, &member_work);
    receive_bound_work(&ledger, &worker, "provider-receipt:member-rc");
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
    for (actor, token, key) in [
        (
            &reviewer,
            reviewer_token.as_str(),
            "peer-cannot-request-changes-member-work",
        ),
        (
            &worker,
            worker_token.as_str(),
            "owner-cannot-request-changes-on-itself",
        ),
    ] {
        let before = durable_store_file_bytes(&store);
        let refused = role_action(
            &store,
            &lease,
            &supervisor_valid,
            &authority_gate,
            actor,
            token,
            request_changes_path(&member_review.id),
            member_review.version,
            key,
            serde_json::json!({"action":"request_changes","reason":"peer opinion"}),
            None,
        )
        .expect_err("Member Work review authority stays with the exact Host");
        assert!(refused.to_string().contains("UNAUTHORIZED_ACTOR"), "{key}");
        assert_eq!(
            durable_store_file_bytes(&store),
            before,
            "{key} rejection has zero durable effects"
        );
    }
    role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &host,
        &host_token,
        request_changes_path(&member_review.id),
        member_review.version,
        "host-request-changes-member-work",
        serde_json::json!({"action":"request_changes","reason":"gate evidence missing"}),
        None,
    )
    .expect("exact Host request-changes on Member Work is unchanged");
}

/// #369: a structured GitHub link is itself the submitted evidence, so a
/// submission may carry one instead of a candidate revision. That capability
/// used to live only on the retired local `team-run work submit` verb; it now
/// belongs to the one authenticated entrance, which means the server — not the
/// caller — has to decide whether a caller-supplied link is evidence at all.
#[test]
fn a_submitted_github_link_is_preserved_and_a_self_inconsistent_link_mints_no_candidate() {
    let (store, _root) = temp_store("submitted-github-link");
    let created = create_two_member_team_run(&store);
    let worker = created
        .member_runs
        .iter()
        .find(|member| member.agent_member_id == "agent-builder-b")
        .expect("worker MemberRun")
        .clone();
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-submitted-github-link",
            std::process::id(),
            "test://submitted-github-link",
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
    transition_provider_session_for_member(
        &ledger,
        &worker,
        harness_core::agentfirm_api::AgentSessionStatus::Active,
    )
    .expect("activate the exact submitting session");
    let worker_token = "d".repeat(64);
    let capability = test_collaboration_capability(&store, &lease, &worker, &worker_token);
    let _live_control = register_live_member_control(&worker, &capability, 1);
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());

    let work = create_assigned_review_work(&store, &created, &lease, &worker, "linked-work");
    bind_test_responsible_work_execution(&store, &lease, &worker, &work);
    receive_bound_work(&ledger, &worker, "provider-receipt:linked-work");
    let base = format!(
        "/v1/agentfirm/team-runs/{}/works/{}",
        created.team_run.id, work.id
    );
    role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &worker,
        &worker_token,
        format!("{base}/start"),
        work.version,
        "start-linked-work",
        serde_json::json!({"action":"start_work"}),
        None,
    )
    .expect("owner starts the linked Work");

    // A link whose structured fields disagree with its own url is refused
    // before it can mint a synthetic candidate, and appends nothing.
    let before_spoofed = durable_store_file_bytes(&store);
    let spoofed = role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &worker,
        &worker_token,
        format!("{base}/submit"),
        work.version + 1,
        "submit-spoofed-link",
        serde_json::json!({
            "action":"submit_work",
            "result_summary":"submission carrying a spoofed link",
            "github_links":[{
                "kind":"pull_request",
                "owner":"example",
                "repo":"project",
                "number":17,
                "url":"https://github.com/attacker/elsewhere/pull/99",
                "status":"OPEN",
                "ci_status":"success",
                "ci_url":"https://github.com/example/project/actions/runs/17"
            }]
        }),
        None,
    )
    .expect_err("a link that does not describe its own url is not evidence");
    assert!(
        spoofed.to_string().contains("GITHUB_LINK_INCONSISTENT"),
        "{spoofed}"
    );
    assert_eq!(
        durable_store_file_bytes(&store),
        before_spoofed,
        "a refused submission has zero durable effects"
    );

    // The consistent link stands in for the candidate revision. The submitted
    // refs are exactly what the retired local verb produced: the caller's own
    // refs first, then the PR url on artifact_refs and the checks url on
    // check_refs.
    let pr_url = "https://github.com/example/project/pull/17";
    let ci_url = "https://github.com/example/project/actions/runs/17";
    let link = harness_core::GitHubLink {
        kind: harness_core::GitHubLinkKind::PullRequest,
        owner: "example".into(),
        repo: "project".into(),
        number: 17,
        url: pr_url.into(),
        status: Some("OPEN".into()),
        ci_status: Some("success".into()),
        ci_url: Some(ci_url.into()),
    };
    let mut artifact_refs = vec!["artifact:linked-work".to_string()];
    let mut check_refs = Vec::new();
    merge_github_link_refs(&link, &mut artifact_refs, &mut check_refs);
    assert_eq!(
        artifact_refs,
        vec!["artifact:linked-work".to_string(), pr_url.to_string()]
    );
    assert_eq!(check_refs, vec![ci_url.to_string()]);
    // Merging the same link twice never duplicates a ref.
    merge_github_link_refs(&link, &mut artifact_refs, &mut check_refs);
    assert_eq!(artifact_refs.len(), 2);
    assert_eq!(check_refs.len(), 1);

    role_action(
        &store,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &worker,
        &worker_token,
        format!("{base}/submit"),
        work.version + 1,
        "submit-linked-work",
        serde_json::json!({
            "action":"submit_work",
            "result_summary":"submission whose evidence is the linked pull request",
            "artifact_refs":artifact_refs,
            "check_refs":check_refs,
            "github_links":[link]
        }),
        None,
    )
    .expect("a structured GitHub link stands in for the candidate revision");

    let reports = store
        .trust_work_reports(&lease.execution_space_id)
        .expect("submitted WorkReports")
        .into_iter()
        .filter(|report| report.work_id == work.id)
        .collect::<Vec<_>>();
    let [report] = reports.as_slice() else {
        panic!("exactly one immutable Result for this revision: {reports:?}")
    };
    assert_eq!(
        report.artifact_refs,
        vec!["artifact:linked-work".to_string(), pr_url.to_string()]
    );
    assert_eq!(report.check_refs, vec![ci_url.to_string()]);
    assert_eq!(report.github_links.len(), 1);
    assert_eq!(report.github_links[0].url, pr_url);
    assert_eq!(report.github_links[0].number, 17);
    assert!(
        !report.report_only,
        "a linked submission is not report-only"
    );
    let derived = harness_store::canonical_work_candidate_revision(
        &report.summary,
        &report.artifact_refs,
        &report.check_refs,
        &report.github_links,
    );
    assert_eq!(
        report
            .candidate
            .as_ref()
            .map(|candidate| candidate.value.as_str()),
        Some(derived.as_str()),
        "the candidate is derived from the submitted link content"
    );
}

/// Attribution must follow the same predicate as authorization. `is_host` is
/// also true through `authorized_authority_actors`, so a peer credential that
/// merely carries the Host as an authority actor would have been recorded as a
/// Host-performed review — losing who actually reviewed, and suppressing the
/// Host's own WorkChangesRequested attention as self-authored.
#[test]
fn a_peer_credential_carrying_host_authority_is_still_recorded_as_the_peer() {
    let (store, _root) = temp_store("peer-attribution");
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
    let lease = store
        .acquire_test_supervisor_lease(
            &created.team_run.id,
            "supervisor-peer-attribution",
            std::process::id(),
            "test://peer-attribution",
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
    for member in [&host, &reviewer] {
        transition_provider_session_for_member(
            &ledger,
            member,
            harness_core::agentfirm_api::AgentSessionStatus::Active,
        )
        .expect("activate exact reviewer session");
    }
    let host_token = "e".repeat(64);
    let reviewer_token = "f".repeat(64);
    let mut _live_controls = Vec::new();
    for (member, token) in [
        (&host, host_token.as_str()),
        (&reviewer, reviewer_token.as_str()),
    ] {
        let capability = test_collaboration_capability(&store, &lease, member, token);
        _live_controls.push(register_live_member_control(member, &capability, 1));
    }
    let supervisor_valid = AtomicBool::new(true);
    let authority_gate = Mutex::new(());

    let work = create_assigned_review_work(&store, &created, &lease, &host, "attribution-work");
    bind_test_responsible_work_execution(&store, &lease, &host, &work);
    receive_bound_work(&ledger, &host, "provider-receipt:attribution-work");
    let review = advance_to_review(
        &store,
        &created,
        &lease,
        &supervisor_valid,
        &authority_gate,
        &host,
        &host_token,
        &work,
    );

    // The peer's own credential, carrying the Host as an authority actor.
    crate::role_actions_api::execute(
        &store,
        crate::agentfirm_api::AuthenticatedMutation {
            execution_space_id: lease.execution_space_id.clone(),
            actor: harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: reviewer.agent_member_id.clone(),
            },
            authorized_authority_actors: vec![harness_core::agentfirm_api::ActorRef {
                kind: harness_core::agentfirm_api::ActorKind::AgentMember,
                id: created_team_host_agent_id(&store, &created),
            }],
            idempotency_key: "peer-with-host-authority".into(),
            expected_version: review.version,
            request_fingerprint: None,
        },
        &format!(
            "/v1/agentfirm/teams/{}/works/{}/request-changes",
            created.team_run.agent_team_id, review.id
        ),
        serde_json::json!({"action":"request_changes","reason":"peer review with host authority attached"})
            .to_string()
            .as_bytes(),
        None,
    )
    .expect("the peer is authorized as the peer");

    let event = store
        .work_events()
        .expect("Work events")
        .into_iter()
        .rev()
        .find(|event| {
            event.work_id == review.id
                && event.kind == harness_core::WorkEventKind::ChangesRequested
        })
        .expect("committed ChangesRequested event");
    // The performer is the reviewing AgentMember and the MemberRun that
    // carried the review is evidence beside it. A Host authority actor still
    // must not rewrite either.
    assert_eq!(
        event.performed_by_actor.kind,
        harness_core::TeamActorKind::AgentMember,
        "an authority actor must not rewrite who performed the review"
    );
    assert_eq!(event.performed_by_actor.id, reviewer.agent_member_id);
    assert_eq!(
        event.executing_member_run_id(),
        Some(reviewer.id.as_str()),
        "the reviewing runtime generation stays recorded as evidence"
    );
    assert!(
        store
            .host_attentions()
            .expect("HostAttentions")
            .iter()
            .any(|attention| attention.work_id == review.id
                && attention.kind == harness_core::HostAttentionKind::WorkChangesRequested),
        "a peer-performed review still raises the Host's attention"
    );
}

fn created_team_host_agent_id(store: &HarnessStore, created: &CreatedTeamRun) -> String {
    store
        .latest_teams()
        .expect("latest teams")
        .remove(&created.team_run.agent_team_id)
        .expect("AgentTeam")
        .host_agent_id
}

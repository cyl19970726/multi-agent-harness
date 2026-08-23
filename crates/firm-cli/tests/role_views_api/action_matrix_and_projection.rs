use super::*;

pub(super) struct ActionMatrixContext<'a> {
    pub serve: &'a ServeHandle,
    pub store: &'a HarnessStore,
    pub space_id: &'a str,
    pub project_id: &'a str,
    pub run_id: &'a str,
    pub worker_id: &'a str,
    pub member_run_id: &'a str,
    pub action_route: &'a str,
    pub view_route: &'a str,
    pub team: &'a harness_core::AgentTeam,
    pub host_id: &'a str,
    pub node_id: &'a str,
    pub operator_route: &'a str,
    pub member_view_route: &'a str,
}

pub(super) fn assert_action_matrix_and_final_projections(context: ActionMatrixContext<'_>) {
    let ActionMatrixContext {
        serve,
        store,
        space_id,
        project_id,
        run_id,
        worker_id,
        member_run_id,
        action_route,
        view_route,
        team,
        host_id,
        node_id,
        operator_route,
        member_view_route,
    } = context;
    let trust_member_run = store
        .trust_member_runs(space_id)
        .expect("trust MemberRuns for final projection checks")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("canonical MemberRun for final projection checks");

    // Every mutable Work action exposed by the closed RoleAction matrix must
    // replay the original commit before consulting the now-advanced Work
    // revision. These sequences are deliberately table-driven so future
    // actions cannot silently regress to create-only idempotency coverage.
    let host_matrix_work = serde_json::json!({
        "action":"create_work",
        "work_id":"work-host-replay-matrix",
        "title":"Host replay matrix",
        "completion_criteria_markdown":"Every Host mutation replays exactly",
        "eligible_member_ids":[worker_id]
    });
    assert_exact_role_action_replay(
        serve,
        action_route,
        &host_matrix_work,
        &action_headers(TOKEN, "matrix-host-create", "0"),
        "host create",
    );
    let provider_run = store
        .member_runs()
        .expect("provider runtime projections")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("provider runtime for MemberRun");
    let mut failed_provider_run = provider_run.clone();
    failed_provider_run.status = MemberRunStatus::Failed;
    failed_provider_run.finished_at = Some("unix-ms:matrix-failed".into());
    store
        .compare_and_append_member_run(&provider_run, &failed_provider_run)
        .expect("record failed provider runtime generation");
    let mut successor_provider_run = provider_run.clone();
    successor_provider_run.runtime_generation += 1;
    successor_provider_run.status = MemberRunStatus::Idle;
    successor_provider_run.started_at = "unix-ms:matrix-successor".into();
    successor_provider_run.finished_at = None;
    let successor_run_id = successor_provider_run.id.clone();
    store
        .compare_and_advance_member_run_generation(&failed_provider_run, &successor_provider_run)
        .expect("append higher-generation replacement runtime");

    // Interrupt is a live-only semantic action, not a durable lifecycle
    // mutation. Prove malformed and stale HTTP requests fail at authorization
    // before provider control. Provider acknowledgement and the running-turn
    // precondition are exercised by provider-adapter journeys; synthesizing a
    // Running row here would let the independent NodeDaemon legitimately
    // settle it while this test compares store bytes.
    let interrupt_version = store
        .trust_member_runs(space_id)
        .expect("Interrupt canonical MemberRun")
        .into_iter()
        .find(|run| run.id == member_run_id)
        .expect("Interrupt MemberRun")
        .version
        .to_string();
    let interrupt_route =
        format!("/v1/agentfirm/member-runs/{member_run_id}/interrupt?project={project_id}");
    for (key, body) in [
        (
            "matrix-member-interrupt-wrong-action",
            serde_json::json!({"action":"close_member_run"}),
        ),
        (
            "matrix-member-interrupt-empty-reason",
            serde_json::json!({"action":"interrupt_member_run","reason":"   "}),
        ),
    ] {
        let before = ledger_digest(serve.fixture_store_root());
        let (status, rejected) = serve.post_json_with_headers(
            &interrupt_route,
            &body,
            &action_headers(TOKEN, key, &interrupt_version),
        );
        assert_eq!(status, 409, "invalid Interrupt intent: {rejected}");
        assert_eq!(
            rejected["error"]["code"], "INVALID_STATE_TRANSITION",
            "invalid Interrupt must fail at the closed semantic boundary"
        );
        assert_eq!(
            ledger_digest(serve.fixture_store_root()),
            before,
            "invalid Interrupt intent changed durable state"
        );
    }
    let stale_interrupt_version = u64::MAX.to_string();
    let before_stale_interrupt = ledger_digest(serve.fixture_store_root());
    let (status, rejected_stale_interrupt) = serve.post_json_with_headers(
        &interrupt_route,
        &serde_json::json!({
            "action":"interrupt_member_run",
            "reason":"stop exactly the current provider turn"
        }),
        &action_headers(
            TOKEN,
            "matrix-member-interrupt-stale",
            &stale_interrupt_version,
        ),
    );
    assert_eq!(
        status, 409,
        "stale Interrupt must fail before provider control: {rejected_stale_interrupt}"
    );
    assert_eq!(
        rejected_stale_interrupt["error"]["code"], "VERSION_CONFLICT",
        "valid semantic body must still bind the exact current MemberRun revision"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_stale_interrupt,
        "stale Interrupt changed durable state"
    );
    let host_steps = [
        (
            "assign",
            "assign_work",
            "1",
            "matrix-host-assign",
            serde_json::json!({"action":"assign_work","member_run_id":member_run_id}),
            None,
        ),
        (
            "release",
            "release_work",
            "2",
            "matrix-host-release",
            serde_json::json!({"action":"release_work"}),
            None,
        ),
        (
            "assign",
            "assign_work",
            "3",
            "matrix-host-reassign",
            serde_json::json!({"action":"assign_work","member_run_id":member_run_id}),
            None,
        ),
        (
            "rebind",
            "rebind_work",
            "4",
            "matrix-host-rebind",
            serde_json::json!({"action":"rebind_work","member_run_id":successor_run_id}),
            None,
        ),
        (
            "cancel",
            "cancel_work",
            "5",
            "matrix-host-cancel",
            serde_json::json!({"action":"cancel_work","reason":"matrix complete"}),
            Some("cancel"),
        ),
    ];
    for (route_suffix, label, version, key, body, confirmation) in host_steps {
        let route = format!("/v1/agentfirm/team-runs/{run_id}/works/work-host-replay-matrix/{route_suffix}?project={project_id}");
        let mut headers = vec![
            ("X-AgentFirm-Token", TOKEN),
            ("Idempotency-Key", key),
            ("If-Match", version),
        ];
        if let Some(confirmation) = confirmation {
            headers.push(("X-AgentFirm-Confirm", confirmation));
        }
        assert_exact_role_action_replay(serve, &route, &body, &headers, label);
    }
    let member_claim_work = serde_json::json!({
        "action":"create_work",
        "work_id":"work-member-claim-replay",
        "title":"Member claim replay",
        "completion_criteria_markdown":"Team claim replays exactly",
        "claim_mode":"team_claim",
        "eligible_member_ids":[worker_id]
    });
    assert_exact_role_action_replay(
        serve,
        action_route,
        &member_claim_work,
        &action_headers(TOKEN, "matrix-member-claim-create", "0"),
        "member-claim create",
    );
    assert_exact_role_action_replay(
    serve,
    &format!("/v1/agentfirm/team-runs/{run_id}/works/work-member-claim-replay/claim?project={project_id}"),
    &serde_json::json!({"action":"claim_work"}),
    &action_headers(MEMBER_TOKEN, "matrix-member-claim", "1"),
    "claim_work",
);
    for (route_suffix, label, version, key, body) in [
        (
            "block",
            "claimed block_work",
            "2",
            "matrix-claimed-block",
            serde_json::json!({"action":"block_work","reason":"claim-path blocker"}),
        ),
        (
            "resume",
            "claimed unblock_work",
            "3",
            "matrix-claimed-resume",
            serde_json::json!({"action":"unblock_work","resolution":"claim-path blocker resolved"}),
        ),
        (
            "submit",
            "claimed submit_work",
            "4",
            "matrix-claimed-submit",
            serde_json::json!({"action":"submit_work","result_summary":"claim path complete","candidate_revision":"3123456789abcdef0123456789abcdef01234567","check_refs":["check:claim-replay-matrix"]}),
        ),
    ] {
        let route = format!("/v1/agentfirm/team-runs/{run_id}/works/work-member-claim-replay/{route_suffix}?project={project_id}");
        assert_exact_role_action_replay(
            serve,
            &route,
            &body,
            &action_headers(MEMBER_TOKEN, key, version),
            label,
        );
    }

    let member_matrix_work = serde_json::json!({
        "action":"create_work",
        "work_id":"work-member-replay-matrix",
        "title":"Member replay matrix",
        "completion_criteria_markdown":"Every assigned Member mutation replays exactly",
        "eligible_member_ids":[worker_id]
    });
    assert_exact_role_action_replay(
        serve,
        action_route,
        &member_matrix_work,
        &action_headers(TOKEN, "matrix-member-create", "0"),
        "member-matrix create",
    );
    assert_exact_role_action_replay(
    serve,
    &format!("/v1/agentfirm/team-runs/{run_id}/works/work-member-replay-matrix/assign?project={project_id}"),
    &serde_json::json!({"action":"assign_work","member_run_id":member_run_id}),
    &action_headers(TOKEN, "matrix-member-assign", "1"),
    "member-matrix assign",
);
    let member_steps = [
        (
            "start",
            "start_work",
            "2",
            "matrix-member-start",
            serde_json::json!({"action":"start_work"}),
        ),
        (
            "block",
            "block_work",
            "3",
            "matrix-member-block",
            serde_json::json!({"action":"block_work","reason":"deterministic matrix blocker"}),
        ),
        (
            "resume",
            "unblock_work",
            "4",
            "matrix-member-resume",
            serde_json::json!({"action":"unblock_work","resolution":"matrix blocker resolved"}),
        ),
        (
            "submit",
            "submit_work",
            "5",
            "matrix-member-submit",
            serde_json::json!({"action":"submit_work","result_summary":"member replay matrix complete","candidate_revision":"2123456789abcdef0123456789abcdef01234567","check_refs":["check:member-replay-matrix"]}),
        ),
    ];
    for (route_suffix, label, version, key, body) in member_steps {
        let route = format!("/v1/agentfirm/team-runs/{run_id}/works/work-member-replay-matrix/{route_suffix}?project={project_id}");
        assert_exact_role_action_replay(
            serve,
            &route,
            &body,
            &action_headers(MEMBER_TOKEN, key, version),
            label,
        );
    }

    // An idle member on an unreviewed provider tuple keeps its runtime
    // availability fact but must not be counted Ready, and the adapter review
    // state rides along as its own fact with remediation metadata.
    let review_team_view_route =
        format!("/v1/views/team-workspace/{}?project={project_id}", team.id);
    let (status, current_team_view) =
        serve.get_json_with_headers(&review_team_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "current Team RoleView: {current_team_view}");
    let current_capability_admission = current_team_view["data"]["members"]
        .as_array()
        .and_then(|members| {
            members
                .iter()
                .find(|member| member["current_member_run_ref"] == successor_provider_run.id)
        })
        .map(|member| member["provider_capability_admission"].clone())
        .expect("current member capability admission");
    let mut review_run = successor_provider_run.clone();
    review_run.runtime_generation += 1;
    review_run.started_at = "unix-ms:matrix-review-required".into();
    let review_native_session = review_run
        .native_session
        .as_mut()
        .expect("review fixture keeps the discovered native session");
    review_native_session.availability = harness_core::NativeSessionAvailability::Available;
    review_native_session.supports_resume = true;
    review_native_session.last_verified_at = Some("unix-ms:matrix-native-verified".into());
    let mut review_profile = review_run
        .provider_profile
        .clone()
        .expect("provider profile snapshot");
    review_profile.provider_version = Some("0.146.0".into());
    review_profile.compatibility_status = ProviderCompatibilityStatus::ReviewRequired;
    review_profile.compatibility_note = Some(
        "Installed provider version has not been reviewed against this adapter contract; \
     regenerate protocol schemas and run provider acceptance before promotion."
            .into(),
    );
    review_run.provider_profile = Some(review_profile);
    store
        .compare_and_advance_member_run_generation(&successor_provider_run, &review_run)
        .expect("append review-required runtime generation");
    let (status, review_team_view) =
        serve.get_json_with_headers(&review_team_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(
        status, 200,
        "review-required Team RoleView: {review_team_view}"
    );
    let review_member = review_team_view["data"]["members"]
        .as_array()
        .and_then(|members| {
            members
                .iter()
                .find(|member| member["current_member_run_ref"] == review_run.id)
        })
        .cloned()
        .expect("review-required member row");
    assert_eq!(review_member["capacity"], "available");
    assert_eq!(
        review_member["provider_compatibility"], "review_required",
        "unreviewed exact tuple renders review_required as a separate fact"
    );
    assert_eq!(review_member["provider_version"], "0.146.0");
    assert_eq!(
        review_member["provider_capability_admission"], current_capability_admission,
        "changing only source/version review must not rewrite executable capability admission"
    );
    assert!(review_member["provider_compatibility_note"]
        .as_str()
        .is_some_and(|note| note.contains("run provider acceptance")));
    let ready_members = review_team_view["data"]["pressure_summary"]["ready_members"]
        .as_u64()
        .expect("ready_members");
    let review_members = review_team_view["data"]["members"]
        .as_array()
        .expect("members");
    let honestly_ready_members = review_members
        .iter()
        .filter(|member| {
            member["capacity"] == "available"
                && member["provider_compatibility"] == "current"
                && member["provider_capability_admission"] == "active"
        })
        .count() as u64;
    let review_blocked_available = review_members
        .iter()
        .filter(|member| {
            member["capacity"] == "available"
                && matches!(
                    member["provider_compatibility"].as_str(),
                    Some("review_required") | Some("incompatible") | Some("unavailable")
                )
        })
        .count() as u64;
    assert!(
        review_blocked_available >= 1,
        "the review-required member is present and still runtime-available"
    );
    assert_eq!(
        ready_members, honestly_ready_members,
        "Ready requires both a current exact provider tuple and active verified core capabilities"
    );
    // The public lifecycle surface keeps Close/Reopen and Resume distinct.
    // Closing an Active member advertises Reopen only; a caller cannot invoke
    // Resume against that Closed+Stopped projection as an alias for Reopen.
    let (status, lifecycle_before_close) =
        serve.get_json_with_headers(view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "lifecycle RoleView: {lifecycle_before_close}");
    let close_action = lifecycle_before_close["allowed_actions"]
        .as_array()
        .and_then(|actions| {
            actions.iter().find(|action| {
                action["kind"] == "close_member_run" && action["target_ref"]["id"] == member_run_id
            })
        })
        .expect("Active MemberRun close action");
    let lifecycle_route = |operation: &str| {
        format!("/v1/agentfirm/member-runs/{member_run_id}/{operation}?project={project_id}")
    };
    let stale_close_version = u64::MAX.to_string();
    let stale_close_headers = [
        ("X-AgentFirm-Token", TOKEN),
        ("Idempotency-Key", "matrix-member-close-stale"),
        ("If-Match", stale_close_version.as_str()),
        ("X-AgentFirm-Confirm", "close_member_run"),
    ];
    let before_stale_close = ledger_digest(serve.fixture_store_root());
    let (status, rejected_close) = serve.post_json_with_headers(
        &lifecycle_route("close"),
        &serde_json::json!({"action":"close_member_run"}),
        &stale_close_headers,
    );
    assert_eq!(
        status, 409,
        "a stale Close must fail before provider control: {rejected_close}"
    );
    assert_eq!(
        rejected_close["error"]["code"], "VERSION_CONFLICT",
        "Close remains exact-CAS bound"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_stale_close,
        "stale Close must have byte-zero durable side effects"
    );

    // The rest of this test covers RoleView lifecycle projection, not a live
    // provider control journey. Seed the closed state through the canonical
    // Store transition explicitly rather than pretending this HTTP fixture
    // closed a physical handle.
    let seeded_closed = store
        .transition_current_team_member_lifecycle(
            &MutationContext {
                execution_space_id: space_id.to_owned(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: team.host_agent_id.clone(),
                },
                authority_actor: None,
                command_name: "test_fixture.member_run.close".into(),
                idempotency_key: "seed-matrix-member-closed".into(),
                expected_version: close_action["required_version"]
                    .as_u64()
                    .expect("close action version"),
                request_fingerprint: None,
            },
            member_run_id,
            CurrentTeamMemberLifecycleTransition::Close,
            "unix-ms:matrix-member-closed-fixture",
        )
        .expect("seed canonical closed lifecycle fixture");
    assert_eq!(
        seeded_closed.runtime_projection.coordination_status,
        MemberCoordinationStatus::Closed
    );
    assert_eq!(
        seeded_closed.runtime_projection.status,
        MemberRunStatus::Stopped
    );
    let closed_version = seeded_closed.canonical.projection.version.to_string();
    let (status, lifecycle_closed) =
        serve.get_json_with_headers(view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "closed lifecycle RoleView: {lifecycle_closed}");
    let closed_actions = lifecycle_closed["allowed_actions"]
        .as_array()
        .expect("closed actions");
    assert!(closed_actions.iter().any(|action| {
        action["kind"] == "reopen_member_run" && action["target_ref"]["id"] == member_run_id
    }));
    assert!(!closed_actions.iter().any(|action| {
        action["kind"] == "resume_native_session" && action["target_ref"]["id"] == member_run_id
    }));
    let before_closed_resume = ledger_digest(serve.fixture_store_root());
    let (status, rejected_generic_closed_resume) = serve.post_json_with_headers(
        &format!("/v1/member-runs/{member_run_id}/resume-native-session?project={project_id}"),
        &serde_json::json!({
            "command":"resume_native_session",
            "member_run_id":member_run_id,
            "updated_at":"unix-ms:generic-closed-resume"
        }),
        &action_headers(
            TOKEN,
            "matrix-member-invalid-generic-closed-resume",
            &closed_version,
        ),
    );
    assert_eq!(
        status, 409,
        "generic HTTP Closed Resume must fail: {rejected_generic_closed_resume}"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_closed_resume,
        "generic HTTP Closed Resume must have byte-zero durable side effects"
    );
    let (status, rejected_closed_resume) = serve.post_json_with_headers(
        &lifecycle_route("resume-native-session"),
        &serde_json::json!({"action":"resume_native_session"}),
        &action_headers(
            TOKEN,
            "matrix-member-invalid-closed-resume",
            &closed_version,
        ),
    );
    assert_eq!(
        status, 409,
        "Closed Resume must fail: {rejected_closed_resume}"
    );
    assert_eq!(
        ledger_digest(serve.fixture_store_root()),
        before_closed_resume,
        "Closed Resume must have byte-zero durable side effects"
    );
    let reopened = assert_exact_role_action_replay(
        serve,
        &lifecycle_route("reopen"),
        &serde_json::json!({"action":"reopen_member_run"}),
        &action_headers(TOKEN, "matrix-member-reopen", &closed_version),
        "reopen Closed MemberRun",
    );
    assert_eq!(reopened["projection"]["coordination_status"], "active");
    assert_eq!(reopened["projection"]["runtime_status"], "queued");

    // Historical/corrupt stores can contain two active generations for one
    // AgentMember in the same TeamRun. Reads must never choose one
    // arbitrarily: MemberWorkbench fails closed and Host loses all mutations
    // until the identity conflict is reconciled.
    let mut duplicate_run = trust_member_run.clone();
    duplicate_run.id = "member-run-duplicate-active".into();
    if let Some(session) = duplicate_run.native_session.as_mut() {
        session.native_session_id = "duplicate-active-session".into();
    }
    store
        .legacy_import_create_trust_member_run_projection(
            &MutationContext {
                execution_space_id: space_id.to_owned(),
                authenticated_actor: ActorRef {
                    kind: ActorKind::AgentMember,
                    id: team.host_agent_id.clone(),
                },
                authority_actor: None,
                command_name: "member_run.create".into(),
                idempotency_key: "seed-duplicate-active-run".into(),
                expected_version: 0,
                request_fingerprint: None,
            },
            duplicate_run,
        )
        .expect("seed duplicate active MemberRun");
    let (status, duplicate_member) =
        serve.get_json_with_headers(member_view_route, &[("X-AgentFirm-Token", MEMBER_TOKEN)]);
    assert_eq!(status, 409, "duplicate MemberRun: {duplicate_member}");
    assert_eq!(duplicate_member["error"]["code"], "IDENTITY_CONFLICT");
    let (status, conflicted_host) =
        serve.get_json_with_headers(view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "conflicted Host RoleView: {conflicted_host}");
    assert_eq!(conflicted_host["allowed_actions"], serde_json::json!([]));
    assert!(conflicted_host["attention"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|item| item["reason_code"] == "multiple_active_member_runs")));

    let global_route = format!("/v1/views/global-work?project={project_id}");
    let mut response = None;
    for _ in 0..40 {
        let current = serve.get_json_with_headers(&global_route, &[("X-AgentFirm-Token", TOKEN)]);
        if current.0 != 503 || current.1["error"]["code"] != "SNAPSHOT_UNSTABLE" {
            response = Some(current);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let (status, global) = response.expect("Global Work projection must reach a stable snapshot");
    assert_eq!(status, 200, "Global Work RoleView: {global}");
    assert_eq!(global["view_kind"], "global_work");
    assert!(global["data"]["items"].as_array().is_some_and(|items| items
        .iter()
        .any(|work| work["work_id"] == "work-store-live-1")));
    let snapshot_vector = global["data"]["page"]["snapshot_vector"]
        .as_array()
        .expect("snapshot vector");
    assert_eq!(
        snapshot_vector.len(),
        2,
        "Global Work cursor must bind every space"
    );
    assert!(snapshot_vector
        .iter()
        .any(|point| point["execution_space_id"] == space_id));
    assert!(snapshot_vector
        .iter()
        .any(|point| point["execution_space_id"] == "role-action-empty-space"));
    let team_view_route = format!("/v1/views/team-workspace/{}?project={project_id}", team.id);
    let (status, team_view) =
        serve.get_json_with_headers(&team_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "Team RoleView: {team_view}");
    assert!(team_view["data"]["works"]
        .as_array()
        .is_some_and(|items| items
            .iter()
            .any(|work| work["work_id"] == "work-store-live-1")));
    assert_eq!(
        team_view["data"]["team"]["display_name"],
        "Role action team"
    );
    assert_eq!(team_view["data"]["team"]["host_agent_id"], host_id);
    assert_eq!(team_view["data"]["team"]["viewer_role"], "host");
    assert_eq!(team_view["data"]["team"]["latest_run"]["id"], run_id);
    let projected_work = team_view["data"]["works"]
        .as_array()
        .and_then(|works| {
            works
                .iter()
                .find(|work| work["work_id"] == "work-store-live-1")
        })
        .expect("projected Work");
    assert_eq!(projected_work["title"], "Close the local product loop");
    assert_eq!(projected_work["claim_mode"], "team_claim");
    assert!(projected_work["eligible_member_ids"].is_array());
    assert!(projected_work["artifact_refs"].is_array());
    assert!(projected_work["latest_event"].is_object());
    // DOC-106: every RoleView reads the one Work/WorkOperation authority, so
    // the same Work id carries the identical revision everywhere.
    let global_work = global["data"]["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|work| work["work_id"] == "work-store-live-1")
        })
        .expect("Global Work row");
    assert_eq!(
        global_work["work_revision"], projected_work["work_revision"],
        "Global and Team projections must carry the identical Work revision"
    );
    assert_eq!(
        global_work["accountable_team_id"],
        serde_json::json!(team.id)
    );
    assert_eq!(
        projected_work["accountable_team_id"],
        global_work["accountable_team_id"]
    );
    assert!(projected_work["assignee_ref"].is_object());
    assert!(team_view["data"]["members"]
        .as_array()
        .is_some_and(|members| members.iter().all(|member| {
            member["display_name"].is_string()
                && member["active_work_count"].is_u64()
                && member["queued_work_count"].is_u64()
                && member["review_work_count"].is_u64()
                && member["blocked_work_count"].is_u64()
        })));
    assert!(team_view["data"]["messages"]
        .as_array()
        .is_some_and(|messages| messages
            .iter()
            .all(|message| { message["body"].is_string() && message["deliveries"].is_array() })));
    assert!(team_view["data"]["activity"].is_array());
    assert!(team_view["data"]["activity_truncated"].is_boolean());
    assert_eq!(
        team_view["data"]["pressure_summary"]["total_members"].as_u64(),
        team_view["data"]["members"]
            .as_array()
            .map(|members| members.len() as u64)
    );
    assert!(team_view["data"]["pressure_summary"]["ready_work"].is_u64());
    let (status, operator) =
        serve.get_json_with_headers(operator_route, &[("X-AgentFirm-Token", OPERATOR_TOKEN)]);
    assert_eq!(status, 200, "Operator RoleView: {operator}");
    assert_eq!(operator["data"]["node"]["node_id"], node_id);
    assert!(operator["data"]["node"]["node_revision"]
        .as_u64()
        .is_some_and(|v| v >= 1));

    let (status, cross_team_denied) =
        serve.get_json_with_headers(member_view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(
        status, 403,
        "Host must not read MemberWorkbench: {cross_team_denied}"
    );
}

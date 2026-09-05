use super::*;

/// #787: a submission that names a candidate revision but omits the mandatory
/// Verbatim evidence is refused before any durable effect — and the Work
/// revision stays untouched, so the compliant submit right after still works.
pub(super) fn assert_submission_evidence_refusals(
    serve: &ServeHandle,
    store: &HarnessStore,
    run_id: &str,
    project_id: &str,
) {
    for (key, body, missing) in [
        (
            "submit-evidence-no-sha",
            serde_json::json!({
                "action":"submit_work",
                "result_summary":"Store-live loop complete, no evidence.",
                "candidate_revision":"0123456789abcdef0123456789abcdef01234567",
                "check_refs":["check:role-action-loop"]
            }),
            "WORK_SUBMISSION_EVIDENCE_MISSING",
        ),
        (
            "submit-evidence-short-sha",
            serde_json::json!({
                "action":"submit_work",
                "result_summary":"SHA 01234567 abbreviated.\ngit status --porcelain: empty",
                "candidate_revision":"01234567",
                "check_refs":["check:role-action-loop"]
            }),
            "WORK_SUBMISSION_EVIDENCE_MISSING",
        ),
    ] {
        let route = format!(
            "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/submit?project={project_id}"
        );
        let (status, refused) =
            serve.post_json_with_headers(&route, &body, &action_headers(MEMBER_TOKEN, key, "3"));
        assert_eq!(status, 409, "{key} must be refused: {refused}");
        assert!(
            serde_json::to_string(&refused)
                .expect("refusal body")
                .contains(missing),
            "{key} must name {missing}: {refused}"
        );
    }
    assert_eq!(
        store
            .latest_works()
            .expect("Works after refused submissions")
            .into_iter()
            .find(|work| work.id == "work-store-live-1")
            .expect("work-store-live-1")
            .version,
        3,
        "a refused submission must leave the Work revision untouched"
    );
}

/// DEV-214 (#830): a non-report-only submission without a candidate is
/// refused with REPORT_EVIDENCE_MISSING, and a report-only submission that
/// still names a candidate is refused with a typed usage error — both before
/// any durable effect, so work-store-live-1 stays at version 3.
pub(super) fn assert_report_only_refusals(serve: &ServeHandle, run_id: &str, project_id: &str) {
    for (key, body, missing) in [
        (
            "submit-no-candidate-no-report-only",
            serde_json::json!({
                "action":"submit_work",
                "result_summary":"No candidate revision and no report-only marker.",
                "check_refs":["check:role-action-loop"]
            }),
            "REPORT_EVIDENCE_MISSING",
        ),
        (
            "submit-report-only-with-candidate",
            serde_json::json!({
                "action":"submit_work",
                "result_summary":"Report-only but still names a candidate.",
                "candidate_revision":"0123456789abcdef0123456789abcdef01234567",
                "report_only":true,
                "check_refs":["check:role-action-loop"]
            }),
            "REPORT_ONLY_WITH_CANDIDATE",
        ),
    ] {
        let route = format!(
            "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-1/submit?project={project_id}"
        );
        let (status, refused) =
            serve.post_json_with_headers(&route, &body, &action_headers(MEMBER_TOKEN, key, "3"));
        assert_eq!(status, 409, "{key} must be refused: {refused}");
        assert!(
            serde_json::to_string(&refused)
                .expect("refusal body")
                .contains(missing),
            "{key} must name {missing}: {refused}"
        );
    }
}

/// DEV-214 (#830): a report-only submission succeeds through the real submit
/// path with candidate_revision null and report_only true on the record.
/// Runs on its own Work so the store-live loop's exact op counts are
/// untouched; call it after every op-count assertion has run.
/// DEV-214 (#830): a report-only submission succeeds through the real submit
/// path with candidate_revision null and report_only true on the record. It
/// runs on its own Work after the store-live Work was accepted (the worker
/// is idle then) and before the matrix section seeds its duplicate MemberRun.
pub(super) fn assert_report_only_submission_succeeds(
    serve: &ServeHandle,
    store: &HarnessStore,
    space_id: &str,
    run_id: &str,
    project_id: &str,
    team: &harness_core::AgentTeam,
    worker_id: &str,
) {
    let run = store
        .team_runs()
        .expect("TeamRuns for the report-only Work")
        .into_iter()
        .rfind(|run| run.id == run_id)
        .expect("exact TeamRun");
    let node_id = run.execution_node_id.as_str();
    let daemon = store
        .latest_node_daemon_lease(node_id)
        .expect("NodeDaemon lease for the report-only Work")
        .expect("live NodeDaemon lease");
    let member_run_id = store
        .trust_member_runs(space_id)
        .expect("member runs for the report-only Work")
        .into_iter()
        .find(|run| {
            run.agent_member_id == worker_id
                && run.team_run_id == run_id
                && run.coordination_status
                    == harness_core::agentfirm_api::MemberCoordinationStatus::Active
        })
        .expect("exact active worker MemberRun")
        .id;
    let worker_membership = store
        .fabric_team_memberships(space_id)
        .expect("TeamMemberships for the report-only Work")
        .into_iter()
        .find(|membership| membership.team_id == team.id && membership.agent_member_id == worker_id)
        .expect("exact worker TeamMembership");
    let worker_session = store
        .fabric_agent_sessions(space_id)
        .expect("AgentSessions for the report-only Work")
        .into_iter()
        .find(|session| session.id == "agent-session:role-view-owner:1")
        .expect("exact worker AgentSession");
    let action_route = format!("/v1/agentfirm/team-runs/{run_id}/works?project={project_id}");
    let (status, created) = serve.post_json_with_headers(
        &action_route,
        &serde_json::json!({
            "action":"create_work",
            "work_id":"work-store-live-report-only-1",
            "title":"Verify without producing a commit",
            "completion_criteria_markdown":"Report-only submission is accepted with candidate_revision null and report_only true",
            "claim_mode":"team_claim"
        }),
        &action_headers(TOKEN, "create-report-only-1", "0"),
    );
    assert_eq!(status, 200, "create report-only Work: {created}");
    let assign_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-report-only-1/assign?project={project_id}"
    );
    let (status, assigned) = serve.post_json_with_headers(
        &assign_route,
        &serde_json::json!({
            "action":"assign_work",
            "membership_id":worker_membership.id
        }),
        &action_headers(TOKEN, "assign-report-only-1", "1"),
    );
    assert_eq!(status, 200, "assign report-only Work: {assigned}");
    let assigned_work = store
        .latest_works()
        .expect("Works after report-only assignment")
        .into_iter()
        .find(|work| work.id == "work-store-live-report-only-1")
        .expect("assigned report-only Work");
    admit_provider_received_work_attempt(ProviderReceivedWorkAttemptInput {
        store,
        space_id,
        node_id,
        daemon: &daemon,
        member_run_id: &member_run_id,
        work: &assigned_work,
        team,
        membership: &worker_membership,
        worker_id,
        session: &worker_session,
        binding_generation: 1,
    });
    let start_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-report-only-1/start?project={project_id}"
    );
    let (status, started) = serve.post_json_with_headers(
        &start_route,
        &serde_json::json!({"action":"start_work"}),
        &action_headers(MEMBER_TOKEN, "start-report-only-1", "2"),
    );
    assert_eq!(status, 200, "start report-only Work: {started}");
    let submit_route = format!(
        "/v1/agentfirm/team-runs/{run_id}/works/work-store-live-report-only-1/submit?project={project_id}"
    );
    let (status, submitted) = serve.post_json_with_headers(
        &submit_route,
        &serde_json::json!({
            "action":"submit_work",
            "result_summary":"Verification complete; the Work produced no commit.",
            "report_only":true,
            "check_refs":["check:role-action-loop"]
        }),
        &action_headers(MEMBER_TOKEN, "submit-report-only-1", "3"),
    );
    assert_eq!(status, 200, "report-only submit: {submitted}");
    assert_eq!(submitted["projection"]["kind"], "result");
    assert_eq!(submitted["projection"]["work_revision"], 4);
    assert_eq!(
        submitted["projection"]["candidate"],
        serde_json::Value::Null,
        "a report-only submission stores no candidate revision: {submitted}"
    );
    assert_eq!(
        submitted["projection"]["report_only"], true,
        "the submission record carries the report_only marker: {submitted}"
    );
}

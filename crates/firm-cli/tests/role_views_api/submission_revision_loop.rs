use super::*;

/// The Work review loop, from the refusal that protects it to the accept that
/// closes it: a submission that names a candidate revision but omits its
/// mandatory Verbatim evidence is refused before any durable effect (#787),
/// the Host requests changes, the Member revises and resubmits, and the Host
/// accepts -- every transition CAS-bound and replay-exact.
///
/// Split out of `role_action_loop_is_authenticated_cas_bound_and_legacy_writers_are_gone`
/// (#949): it consumes the loop's state and returns none of it, so it reads as
/// its own scenario.
pub(super) struct SubmissionRevisionContext<'a> {
    pub serve: &'a ServeHandle,
    pub store: &'a HarnessStore,
    pub space_id: &'a str,
    pub project_id: &'a str,
    pub node_id: &'a str,
    pub run_id: &'a str,
    pub worker_id: &'a str,
    pub member_run_id: &'a str,
    pub team: &'a harness_core::AgentTeam,
    pub daemon: &'a harness_core::NodeDaemonLease,
    pub worker_membership: &'a harness_core::agentfirm_api::TeamMembership,
    pub worker_session: &'a AgentSession,
    pub view_route: &'a str,
    pub start_route: &'a str,
    pub first_attempt: &'a ProviderReceivedWorkAttempt,
    pub journal_before: usize,
}

pub(super) fn assert_submission_refusal_then_revision_and_accept(
    context: SubmissionRevisionContext<'_>,
) {
    let SubmissionRevisionContext {
        serve,
        store,
        space_id,
        project_id,
        node_id,
        run_id,
        worker_id,
        member_run_id,
        team,
        daemon,
        worker_membership,
        worker_session,
        view_route,
        start_route,
        first_attempt,
        journal_before,
    } = context;
    // #787: a submission that names a candidate revision but omits the
    // mandatory Verbatim evidence is refused before any durable effect.
    submission_evidence_refusal::assert_submission_evidence_refusals(
        serve, store, run_id, project_id,
    );
    submission_evidence_refusal::assert_report_only_refusals(serve, run_id, project_id);

    submission_evidence_refusal::assert_compliant_result_submission(
        serve,
        store,
        space_id,
        run_id,
        project_id,
        journal_before,
        first_attempt,
    );
    let (status, review_view) =
        serve.get_json_with_headers(view_route, &[("X-AgentFirm-Token", TOKEN)]);
    assert_eq!(status, 200, "review Host RoleView: {review_view}");
    assert!(review_view["allowed_actions"]
        .as_array()
        .is_some_and(|actions| actions
            .iter()
            .any(|action| action["kind"] == "accept_work" && action["required_version"] == 4)));
    let request_changes_route = format!(
        "/v1/agentfirm/teams/{}/works/work-store-live-1/request-changes?project={project_id}",
        team.id
    );
    let request_changes_headers = action_headers(TOKEN, "request-changes-store-live-1", "4");
    let request_changes_intent =
        serde_json::json!({"action":"request_changes","reason":"tighten exact replay evidence"});
    let (status, changes_requested) = serve.post_json_with_headers(
        &request_changes_route,
        &request_changes_intent,
        &request_changes_headers,
    );
    assert_eq!(status, 200, "request changes: {changes_requested}");
    assert_eq!(changes_requested["projection"]["version"], 5);
    assert_eq!(
        changes_requested["projection"]["phase"], "open",
        "Host changes return stable responsibility to canonical scheduling"
    );
    let (status, changes_replay) = serve.post_json_with_headers(
        &request_changes_route,
        &request_changes_intent,
        &request_changes_headers,
    );
    assert_eq!(status, 200, "request changes replay: {changes_replay}");
    assert_eq!(changes_replay["event_id"], changes_requested["event_id"]);
    assert_eq!(changes_replay["replayed"], true);
    let changes_requested_work = store
        .latest_works()
        .expect("Works after request changes")
        .into_iter()
        .find(|work| work.id == "work-store-live-1")
        .expect("Work awaiting revised Result");
    assert_eq!(changes_requested_work.version, 5);
    let revised_attempt = admit_provider_received_work_attempt(ProviderReceivedWorkAttemptInput {
        store,
        space_id,
        node_id,
        daemon,
        member_run_id,
        work: &changes_requested_work,
        team,
        membership: worker_membership,
        worker_id,
        session: worker_session,
        binding_generation: 2,
    });
    let revised_start_headers = action_headers(MEMBER_TOKEN, "start-store-live-1-revision", "5");
    let (status, revised_started) = serve.post_json_with_headers(
        start_route,
        &serde_json::json!({"action":"start_work"}),
        &revised_start_headers,
    );
    assert_eq!(
        status, 200,
        "member starts revised attempt: {revised_started}"
    );
    assert_eq!(revised_started["projection"]["version"], 6);
    assert_eq!(revised_started["projection"]["phase"], "active");
    let revise_route = format!(
        "/v1/agentfirm/teams/{}/works/work-store-live-1/revise?project={project_id}",
        team.id
    );
    let revise_headers = action_headers(MEMBER_TOKEN, "revise-store-live-1", "6");
    let revise_intent = serde_json::json!({"action":"revise_work","result_summary":"Revised Store-live loop","candidate_revision":"1123456789abcdef0123456789abcdef01234567","check_refs":["check:role-action-revise"]});
    let (status, revised) =
        serve.post_json_with_headers(&revise_route, &revise_intent, &revise_headers);
    assert_eq!(status, 200, "member revise: {revised}");
    assert_eq!(revised["projection"]["work_revision"], 7);
    assert_released_provider_received_attempt(store, space_id, &revised_attempt);
    let (status, revise_replay) =
        serve.post_json_with_headers(&revise_route, &revise_intent, &revise_headers);
    assert_eq!(status, 200, "member revise replay: {revise_replay}");
    assert_eq!(revise_replay["event_id"], revised["event_id"]);
    assert_eq!(revise_replay["replayed"], true);
    let accept_route = format!(
        "/v1/agentfirm/teams/{}/works/work-store-live-1/accept?project={project_id}",
        team.id
    );
    let canonical_before_accept = store
        .canonical_operations_for_space(&ExecutionSpaceId::new(space_id))
        .expect("before accept")
        .len();
    let no_confirm_headers = action_headers(TOKEN, "accept-no-confirm", "7");
    let (status, no_confirm) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &no_confirm_headers,
    );
    assert_eq!(status, 409, "missing confirmation: {no_confirm}");
    let member_accept_headers = [
        ("X-AgentFirm-Token", MEMBER_TOKEN),
        ("Idempotency-Key", "accept-member-spoof"),
        ("If-Match", "7"),
        ("X-AgentFirm-Confirm", "accept"),
    ];
    let (status, member_accept) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &member_accept_headers,
    );
    assert_eq!(status, 409, "Member authority spoof: {member_accept}");
    let stale_accept_headers = [
        ("X-AgentFirm-Token", TOKEN),
        ("Idempotency-Key", "accept-stale"),
        ("If-Match", "6"),
        ("X-AgentFirm-Confirm", "accept"),
    ];
    let (status, stale_accept) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &stale_accept_headers,
    );
    assert_eq!(status, 409, "stale accept: {stale_accept}");
    assert_eq!(
        store
            .canonical_operations_for_space(&ExecutionSpaceId::new(space_id))
            .expect("rejected accepts")
            .len(),
        canonical_before_accept,
        "rejected critical actions must have zero canonical side effects"
    );
    let accept_headers = [
        ("X-AgentFirm-Token", TOKEN),
        ("Idempotency-Key", "accept-store-live-1"),
        ("If-Match", "7"),
        ("X-AgentFirm-Confirm", "accept"),
    ];
    let (status, accepted) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &accept_headers,
    );
    assert_eq!(status, 200, "Host accept: {accepted}");
    assert_eq!(accepted["projection"]["phase"], "closed");
    assert_eq!(accepted["projection"]["resolution"], "accepted");
    let (status, accept_replay) = serve.post_json_with_headers(
        &accept_route,
        &serde_json::json!({"action":"accept_work"}),
        &accept_headers,
    );
    assert_eq!(status, 200, "accept replay: {accept_replay}");
    assert_eq!(accept_replay["event_id"], accepted["event_id"]);
    assert_eq!(accept_replay["replayed"], true);
}

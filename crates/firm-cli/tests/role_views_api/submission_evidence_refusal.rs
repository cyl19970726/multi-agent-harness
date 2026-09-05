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

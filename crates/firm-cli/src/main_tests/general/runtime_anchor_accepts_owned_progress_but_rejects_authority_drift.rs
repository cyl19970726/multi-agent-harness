use super::*;

#[test]
fn runtime_anchor_accepts_owned_progress_but_rejects_authority_drift() {
    let (store, root) = temp_store("runtime-anchor-progress");
    let created = create_two_member_team_run(&store);
    let anchor = created.member_runs[0].clone();
    let accepted = anchor.clone();
    let mut bound = accepted.clone();
    bound.native_session = Some(NativeSessionRef {
        provider: bound.provider.clone(),
        execution_mode: "codex_app_server".into(),
        native_session_id: "owned-session".into(),
        native_locator_kind: "codex_rollout".into(),
        provider_version: Some("test".into()),
        adapter_contract_version: "test".into(),
        availability: NativeSessionAvailability::Available,
        supports_resume: true,
        last_verified_at: Some("unix-ms:verified".into()),
        parent_native_session_id: None,
    });
    bound
        .provider_controls
        .model
        .mark_effective(Some("gpt-5.6-sol".into()), "provider receipt");
    bound.status = MemberRunStatus::Disconnected;
    bound.zero_output_streak = 2;
    bound.last_consumed_work_version = Some(7);
    assert!(member_runtime_progress_matches(
        &anchor, &accepted, &bound, true
    ));
    assert!(member_runtime_progress_matches(
        &anchor, &bound, &bound, false
    ));
    let mut refreshed_session = bound.clone();
    let refreshed = refreshed_session
        .native_session
        .as_mut()
        .expect("bound session");
    refreshed.provider_version = Some("new-observation".into());
    refreshed.availability = NativeSessionAvailability::Stale;
    refreshed.supports_resume = false;
    refreshed.last_verified_at = Some("unix-ms:later".into());
    refreshed.parent_native_session_id = Some("lineage-observation".into());
    assert!(member_runtime_progress_matches(
        &anchor,
        &bound,
        &refreshed_session,
        false
    ));

    let mut replaced_session = bound.clone();
    replaced_session
        .native_session
        .as_mut()
        .expect("bound session")
        .native_session_id = "foreign-replacement".into();
    assert!(!member_runtime_progress_matches(
        &anchor,
        &bound,
        &replaced_session,
        false
    ));
    for mutation in ["mode", "locator", "contract"] {
        let mut replaced = bound.clone();
        let session = replaced.native_session.as_mut().expect("bound session");
        match mutation {
            "mode" => session.execution_mode = "different-mode".into(),
            "locator" => session.native_locator_kind = "different-locator".into(),
            "contract" => session.adapter_contract_version = "different-contract".into(),
            _ => unreachable!(),
        }
        assert!(
            !member_runtime_progress_matches(&anchor, &bound, &replaced, false),
            "stable session authority mutation {mutation} must be rejected"
        );
    }
    let mut changed_request = bound.clone();
    changed_request.provider_controls.model.requested = Some("operator-change".into());
    assert!(!member_runtime_progress_matches(
        &anchor,
        &bound,
        &changed_request,
        false
    ));
    let mut changed_identity = bound.clone();
    changed_identity.role = "different-role".into();
    assert!(!member_runtime_progress_matches(
        &anchor,
        &bound,
        &changed_identity,
        false
    ));
    let mut changed_generation = bound.clone();
    changed_generation.runtime_generation += 1;
    assert!(!member_runtime_progress_matches(
        &anchor,
        &bound,
        &changed_generation,
        false
    ));
    std::fs::remove_dir_all(root).expect("cleanup");
}

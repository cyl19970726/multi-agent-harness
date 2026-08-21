use super::*;

#[test]
fn current_team_member_lifecycle_updates_both_projections_and_fences_foreign_space() {
    let harness = TestStore::new("combined-member-lifecycle");
    let host = human("host");
    let team_run = seed_team(&harness.store, "combined-member-lifecycle", &["member-a"]);
    create_member_and_run(
        &harness.store,
        &host,
        &team_run.id,
        "member-a",
        "runtime-member-a",
        true,
    );

    let legacy_before = harness.store.member_runs().expect("read runtime rows");
    let canonical_before = harness
        .store
        .trust_member_runs(SPACE)
        .expect("read canonical rows");
    let operations_before = harness
        .store
        .canonical_operations()
        .expect("read canonical operations");
    let mut foreign = context(host.clone(), "member_run.close", "foreign-close", 1);
    foreign.execution_space_id = "foreign-space".into();
    let error = harness
        .store
        .transition_current_team_member_lifecycle(
            &foreign,
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Close,
            "t2",
        )
        .expect_err("caller-selected foreign Execution Space must fail closed");
    assert!(error.to_string().contains("EXECUTION_SPACE_SCOPE_MISMATCH"));
    assert_eq!(harness.store.member_runs().unwrap(), legacy_before);
    assert_eq!(
        harness.store.trust_member_runs(SPACE).unwrap(),
        canonical_before
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_before
    );

    let close_context = context(host.clone(), "member_run.close", "close-current", 1);
    let closed = harness
        .store
        .transition_current_team_member_lifecycle(
            &close_context,
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Close,
            "t2",
        )
        .expect("close current Team Member");
    assert_eq!(
        closed.runtime_projection.coordination_status,
        firm_core::MemberCoordinationStatus::Closed
    );
    assert_eq!(closed.runtime_projection.status, MemberRunStatus::Stopped);
    assert_eq!(
        closed.canonical.projection.coordination_status,
        MemberCoordinationStatus::Closed
    );
    assert_eq!(
        closed.canonical.projection.runtime_status,
        MemberRuntimeStatus::Stopped
    );

    let legacy_after_close = harness.store.member_runs().unwrap();
    let operations_after_close = harness.store.canonical_operations().unwrap();
    let close_replay = harness
        .store
        .transition_current_team_member_lifecycle(
            &close_context,
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Close,
            "t2",
        )
        .expect("exact close retry replays");
    assert!(close_replay.canonical.replayed);
    assert_eq!(harness.store.member_runs().unwrap(), legacy_after_close);
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_after_close
    );

    let closed_resume_error = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(
                host.clone(),
                "member_run.resume_native_session",
                "resume-closed",
                2,
            ),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession,
            "t3",
        )
        .expect_err("ResumeNativeSession must not impersonate Reopen");
    assert_eq!(
        trust_code(closed_resume_error),
        TrustErrorCode::InvalidStateTransition
    );
    assert_eq!(harness.store.member_runs().unwrap(), legacy_after_close);
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_after_close
    );

    let stale_version_error = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(host.clone(), "member_run.reopen", "reopen-stale-version", 1),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Reopen,
            "t3",
        )
        .expect_err("stale canonical CAS must reject before either ledger changes");
    assert_eq!(
        trust_code(stale_version_error),
        TrustErrorCode::VersionConflict
    );
    assert_eq!(harness.store.member_runs().unwrap(), legacy_after_close);
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_after_close
    );

    let reopened = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(host.clone(), "member_run.reopen", "reopen-current", 2),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Reopen,
            "t3",
        )
        .expect("reopen current Team Member");
    assert_eq!(reopened.runtime_projection.runtime_generation, 2);
    assert_eq!(reopened.runtime_projection.status, MemberRunStatus::Queued);
    assert_eq!(reopened.canonical.projection.runtime_generation, 2);

    let mut disconnected = reopened.runtime_projection.clone();
    disconnected.status = MemberRunStatus::Disconnected;
    disconnected.last_event_at = Some("t4".into());
    harness
        .store
        .compare_and_append_member_run(&reopened.runtime_projection, &disconnected)
        .expect("record active provider transport loss");
    let resumed = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(
                host.clone(),
                "member_run.resume_native_session",
                "resume-current",
                4,
            ),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession,
            "t5",
        )
        .expect("resume the active Team Member native session");
    assert_eq!(resumed.runtime_projection.runtime_generation, 2);
    assert_eq!(resumed.runtime_projection.status, MemberRunStatus::Starting);
    assert_eq!(resumed.canonical.projection.runtime_generation, 2);
    assert_eq!(
        resumed.canonical.projection.runtime_status,
        MemberRuntimeStatus::Starting
    );
    assert_eq!(
        resumed.canonical.projection.native_session,
        resumed
            .runtime_projection
            .native_session
            .as_ref()
            .map(|session| serde_json::from_value(serde_json::to_value(session).unwrap()).unwrap())
    );
    let runtime_after_resume = harness.store.member_runs().unwrap();
    let operations_after_resume = harness.store.canonical_operations().unwrap();
    let resume_replay = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(
                host.clone(),
                "member_run.resume_native_session",
                "resume-current",
                4,
            ),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::ResumeNativeSession,
            "t5",
        )
        .expect("exact resume retry replays");
    assert!(resume_replay.canonical.replayed);
    assert_eq!(harness.store.member_runs().unwrap(), runtime_after_resume);
    assert_eq!(
        harness.store.canonical_operations().unwrap(),
        operations_after_resume
    );

    let retired = harness
        .store
        .transition_current_team_member_lifecycle(
            &context(host, "member_run.retire", "retire-current", 5),
            "runtime-member-a",
            CurrentTeamMemberLifecycleTransition::Retire,
            "t6",
        )
        .expect("retire current Team Member");
    assert_eq!(
        retired.runtime_projection.coordination_status,
        firm_core::MemberCoordinationStatus::Retired
    );
    assert_eq!(
        retired.canonical.projection.coordination_status,
        MemberCoordinationStatus::Retired
    );
    assert_eq!(
        retired.runtime_projection.finished_at.as_deref(),
        Some("t6")
    );
    assert_eq!(
        retired.canonical.projection.finished_at.as_deref(),
        Some("t6")
    );
}

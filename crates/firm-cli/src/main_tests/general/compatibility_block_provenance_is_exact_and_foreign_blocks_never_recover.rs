use super::*;

#[test]
fn compatibility_block_provenance_is_exact_and_foreign_blocks_never_recover() {
    let (store, root) = temp_store("compatibility-block-provenance");
    let created = create_two_member_team_run(&store);
    let mut member = created.member_runs[0].clone();
    member.status = MemberRunStatus::Blocked;
    let mut profile = team_member_provider_profile_for_mode("codex", Some("codex_app_server"));
    apply_provider_version(&mut profile, Some("9.9.9".into()));
    let exact = compatibility_block_action(&member, &profile, 1);
    store
        .append_member_action(&exact)
        .expect("append hostile exact-looking audit action");
    assert!(
        !compatibility_block_matches_current_tuple(&member, &profile),
        "a forged exact-looking MemberAction cannot authorize recovery"
    );
    member.provider_profile = Some(profile.clone());
    member.provider_compatibility_block_cause = Some(compatibility_test_cause(&member, &profile));
    member.provider_environment_observation = None;
    assert!(compatibility_block_matches_current_tuple(&member, &profile));

    let host_actor = created
        .team_run
        .host_actor
        .clone()
        .expect("exact fixture Host");
    assert_eq!(
        compatibility_recovery_status(&store, &member).expect("unbound recovery status"),
        MemberRunStatus::Idle,
        "no stable Work responsibility leaves compatibility recovery idle"
    );
    let stable_work = store
        .insert_work(
            harness_core::CurrentWorkDraft::new(
                "work-stable-responsibility".into(),
                created.team_run.id.clone(),
                created.team_run.agent_team_id.clone(),
                "stable Work".into(),
                "stable responsibility".into(),
                "member recovers for accountable Work".into(),
                WorkClaimMode::TeamClaim,
                WorkPriority::Normal,
                host_actor.clone(),
                "unix-ms:2".into(),
            )
            .into_work(),
            WorkCommandContext {
                event_id: "stable-work-create".into(),
                performed_by_actor: host_actor,
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "stable-work-create".into(),
                created_at: "unix-ms:2".into(),
                duplicate_ok: false,
            },
        )
        .expect("seed stable Work");
    let stable_work = assign_test_work_to_member(
        &store,
        "unit-test-space",
        &created,
        &created.member_runs[0],
        &stable_work,
    );
    assert!(stable_work.active_member_run_id.is_none());
    assert_eq!(
        compatibility_recovery_status(&store, &member).expect("stable recovery status"),
        MemberRunStatus::Queued,
        "stable AgentMember responsibility drives recovery"
    );
    let mut native = member.clone();
    native.native_session = Some(capacity_test_session());
    assert_eq!(
        compatibility_recovery_status(&store, &native).expect("native recovery status"),
        MemberRunStatus::Disconnected
    );

    let mut wrong_tuple = profile.clone();
    wrong_tuple.provider_version = Some("9.9.10".into());
    assert!(!compatibility_block_matches_current_tuple(
        &member,
        &wrong_tuple
    ));

    for (index, (action_type, summary)) in [
        ("operator_blocked", exact.summary.clone()),
        ("provider_capacity_blocked", exact.summary.clone()),
        ("provider_compatibility_blocked", "not parseable".into()),
    ]
    .into_iter()
    .enumerate()
    {
        let mut foreign = exact.clone();
        foreign.id = format!("hostile-action-{index}");
        foreign.seq = index as u64 + 2;
        foreign.action_type = action_type.into();
        foreign.summary = summary;
        store
            .append_member_action(&foreign)
            .expect("append hostile audit action");
        assert!(
            compatibility_block_matches_current_tuple(&member, &profile),
            "MemberAction prose is audit-only and cannot alter typed authority"
        );
    }
    std::fs::remove_dir_all(root).expect("cleanup");
}

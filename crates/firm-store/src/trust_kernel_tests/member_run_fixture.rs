use super::*;

pub(super) fn admit_fixture_member_run_for_session(
    store: &HarnessStore,
    team_run_id: &str,
    session: &AgentSession,
) -> String {
    let run_id = format!("runtime-{}-{team_run_id}", session.agent_member_id);
    let canonical = firm_core::agentfirm_api::MemberRun {
        id: run_id.clone(),
        agent_member_id: session.agent_member_id.clone(),
        team_run_id: team_run_id.into(),
        role_snapshot: "member".into(),
        provider_profile_snapshot: Some(session.provider_profile_ref.clone()),
        requested_controls: serde_json::json!({}),
        effective_controls: serde_json::json!({}),
        coordination_status: firm_core::agentfirm_api::MemberCoordinationStatus::Active,
        runtime_status: firm_core::agentfirm_api::MemberRuntimeStatus::Idle,
        runtime_generation: 1,
        workspace_binding_id: None,
        native_session: session.native_session_ref.clone(),
        version: 1,
        started_at: "t-member".into(),
        last_event_at: None,
        finished_at: None,
    };
    let legacy = ProviderRuntimeProjection {
        id: run_id.clone(),
        team_run_id: team_run_id.into(),
        slot_id: None,
        agent_member_id: session.agent_member_id.clone(),
        name: session.agent_member_id.clone(),
        role: "member".into(),
        provider: session.provider_kind.clone(),
        model: None,
        provider_controls: Default::default(),
        provider_profile: None,
        provider_capacity: None,
        provider_compatibility_block_cause: None,
        coordination_status: firm_core::MemberCoordinationStatus::Active,
        runtime_generation: 1,
        status: firm_core::MemberRunStatus::Idle,
        native_session: session.native_session_ref.as_ref().map(|native| {
            serde_json::from_value(serde_json::to_value(native).expect("serialize native session"))
                .expect("map native session")
        }),
        provider_cwd_hint: None,
        provider_environment_observation: None,
        owned_paths: Vec::new(),
        zero_output_streak: 0,
        last_consumed_work_version: None,
        started_at: "t-member".into(),
        last_event_at: None,
        finished_at: None,
    };
    let current = store
        .team_runs()
        .expect("TeamRuns")
        .into_iter()
        .rev()
        .find(|run| run.id == team_run_id)
        .expect("fixture TeamRun");
    let mut next = current.clone();
    next.member_run_ids.push(run_id.clone());
    next.updated_at = "t-member-admit".into();
    store
        .admit_member_run_with_canonical(
            &current,
            &next,
            &legacy,
            "space-test",
            &CanonicalMemberRunAdmission {
                context: context("host", "member_run.create", &format!("admit-{run_id}"), 0),
                run: canonical,
            },
        )
        .expect("admit exact fixture MemberRun");
    run_id
}

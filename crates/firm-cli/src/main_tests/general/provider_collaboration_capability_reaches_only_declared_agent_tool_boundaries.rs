fn capability(
    mechanism: harness_runtime_contract::CollaborationCapabilityMechanism,
) -> harness_runtime_contract::CollaborationCapabilityEnvelope {
    harness_runtime_contract::CollaborationCapabilityEnvelope::new(
        harness_runtime_contract::CollaborationCapabilitySecret::new("ef".repeat(32)).unwrap(),
        harness_runtime_contract::CollaborationCapabilityBinding {
            team_run_id: "team-run-capability".into(),
            member_run_id: "member-run-capability".into(),
            member_run_generation: 1,
            agent_session_id: "agent-session-capability".into(),
            agent_session_generation: 2,
            node_daemon_id: "daemon-capability".into(),
            node_daemon_generation: 3,
            supervisor_id: "supervisor-capability".into(),
            supervisor_generation: 4,
        },
        mechanism,
    )
    .unwrap()
}

fn assert_closed_agent_environment(
    environment: harness_runtime_contract::CollaborationCapabilityEnvironment,
) {
    let environment = environment.as_pairs();
    assert_eq!(environment.len(), 3);
    assert_eq!(
        environment[0],
        ("FIRM_TEAM_RUN_ID".into(), "team-run-capability".into())
    );
    assert_eq!(
        environment[1],
        ("FIRM_MEMBER_RUN_ID".into(), "member-run-capability".into())
    );
    assert_eq!(environment[2].0, "FIRM_MEMBER_ROLE_ACTION_TOKEN");
    assert_eq!(environment[2].1, "ef".repeat(32));
    assert!(environment.iter().all(|(name, _)| ![
        "GITHUB_TOKEN",
        "AWS_SECRET_ACCESS_KEY",
        "DATABASE_PASSWORD",
        "UNRELATED_API_KEY",
    ]
    .contains(&name.as_str())));
}

#[test]
fn provider_collaboration_capability_reaches_only_declared_agent_tool_boundaries() {
    assert_closed_agent_environment(
        harness_provider_codex::collaboration_agent_tool_environment(&capability(
            harness_provider_codex::COLLABORATION_CAPABILITY_MECHANISM,
        ))
        .unwrap(),
    );
    assert_closed_agent_environment(
        harness_provider_claude::collaboration_agent_tool_environment(&capability(
            harness_provider_claude::COLLABORATION_CAPABILITY_MECHANISM,
        ))
        .unwrap(),
    );
    assert_closed_agent_environment(
        harness_provider_kimi::collaboration_agent_tool_environment(&capability(
            harness_provider_kimi::COLLABORATION_CAPABILITY_MECHANISM,
        ))
        .unwrap(),
    );
    assert_closed_agent_environment(
        harness_provider_pi::collaboration_agent_tool_environment(&capability(
            harness_provider_pi::COLLABORATION_CAPABILITY_MECHANISM,
        ))
        .unwrap(),
    );
    assert_closed_agent_environment(
        harness_provider_deepseek::collaboration_agent_tool_environment(&capability(
            harness_provider_deepseek::COLLABORATION_CAPABILITY_MECHANISM,
        ))
        .unwrap(),
    );
}

use super::*;

#[test]
fn mission_log_keeps_one_mission_team_and_member_sessions_alive() {
    let home = TempHome::new("host-plan-mission-team");
    let project_id = init_project(&home, "host-plan");

    for (id, name, role, provider) in [
        ("agent-build", "PrimaryBuilder", "primary builder", "codex"),
        ("agent-review", "ReviewPartner", "reviewer", "kimi"),
        ("agent-repair", "RepairFixer", "repair specialist", "codex"),
    ] {
        let created = create_canonical_agent_member(
            &home,
            home.base(),
            &project_id,
            id,
            name,
            role,
            provider,
            &[("FIRM_COMPANY_OS_TOKEN", COMPANY_OS_TEST_TOKEN)],
        );
        assert!(
            created.status.success(),
            "canonical member create failed: {created:?}"
        );
    }
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "actor",
            "create-human",
            "--id",
            "human-owner",
            "--name",
            "Human Owner",
            "--responsibility",
            "Final company authority",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "actor",
            "create-agent",
            "--authority",
            "human-owner",
            "--id",
            "agent-review",
            "--agent-member",
            "agent-review",
            "--execution-space",
            &project_id,
            "--responsibility",
            "Same-id collision must remain unlinked",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "company",
            "org",
            "actor",
            "create-agent",
            "--authority",
            "human-owner",
            "--id",
            "agent-build",
            "--agent-member",
            "agent-build",
            "--execution-space",
            &project_id,
            "--responsibility",
            "Own persistent implementation work",
        ],
    );
    let mission = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-host-plan",
            "--title",
            "Ship host plan",
            "--objective",
            "Prove members can continue across plan revisions",
            "--context",
            "# Mission context\n\nKeep provider-native sessions.",
            "--json",
        ],
    );
    let node = run_json(&home, &project_id, &["node", "init"]);
    let node_id = node["id"].as_str().expect("node id").to_string();
    run_json(
        &home,
        &project_id,
        &[
            "node",
            "project",
            "register",
            "--node-id",
            &node_id,
            "--project-binding-id",
            &project_id,
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-platform",
            "--name",
            "Platform Team",
            "--description",
            "Long-lived Mission team",
            "--mission-id",
            "mission-host-plan",
            "--host-agent-id",
            "agent-build",
            "--node-id",
            &node_id,
            "--member",
            "agent-build",
            "--member",
            "agent-review",
        ],
    );
    assert!(mission["context"]
        .as_str()
        .is_some_and(|context| context.contains("provider-native")));
    // Host plan judgment is now a Mission Log entry, not a Wave (ADR 0051).
    let judgment_1 = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host-plan",
            "--kind",
            "judgment",
            "--body",
            "# Baseline\n\nTwo lanes start; review may carry forward.",
            "--json",
        ],
    );
    assert_eq!(judgment_1["kind"].as_str(), Some("judgment"));
    assert_eq!(judgment_1["revision"].as_u64(), Some(1));

    let created = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "create",
            "--objective",
            "Work across Host plan revisions",
            "--agent-team-id",
            "team-platform",
            "--resume-member",
            "PrimaryBuilder:codex-session-1",
            "--member-owned-path",
            "PrimaryBuilder:crates",
            "--member-owned-path",
            "PrimaryBuilder:apps",
            "--json",
        ],
    );
    assert_eq!(
        created["team_run"]["agent_team_id"].as_str(),
        Some("team-platform")
    );
    let persisted_team = run_json(
        &home,
        &project_id,
        &["team", "show", "--id", "team-platform"],
    );
    assert_eq!(
        persisted_team["legacy_mission_id"].as_str(),
        Some("mission-host-plan")
    );
    assert!(
        persisted_team.get("mission_id").is_none(),
        "Mission linkage is provenance, never vNext Team identity authority"
    );
    assert_eq!(
        created["member_runs"][0]["agent_member_id"].as_str(),
        Some("agent-build")
    );
    assert_eq!(
        created["member_runs"][0]["owned_paths"],
        serde_json::json!(["crates", "apps"]),
        "Mission-owned team identity survives member-owned-path overrides"
    );
    assert_eq!(
        created["member_runs"][1]["agent_member_id"].as_str(),
        Some("agent-review")
    );
    let snapshot = run_json(&home, &project_id, &["dashboard", "snapshot"]);
    // DEV-35 dashboard compatibility projection: the frontend AgentTeam type
    // still requires mission_id / host_agent_id / member_ids. The snapshot
    // derives them from the durable TeamMembership authority; stored Team
    // authority remains free of them (see the `team show` assertion above).
    let snapshot_teams = snapshot["teams"].as_array().expect("snapshot teams");
    let platform_team = snapshot_teams
        .iter()
        .find(|team| team["id"].as_str() == Some("team-platform"))
        .expect("team-platform present in dashboard snapshot teams");
    assert_eq!(
        platform_team["mission_id"].as_str(),
        Some("mission-host-plan"),
        "compat mission_id derives from legacy provenance"
    );
    assert_eq!(
        platform_team["host_agent_id"].as_str(),
        Some("agent-build"),
        "compat host_agent_id derives from the one active Host membership"
    );
    assert_eq!(
        platform_team["member_ids"],
        serde_json::json!(["agent-review"]),
        "compat member_ids are the active non-Host memberships"
    );
    let membership_projections = snapshot["company_os"]["agent_members"]
        .as_array()
        .expect("AgentMember membership projection");
    let projected_ids = membership_projections
        .iter()
        .filter_map(|member| member["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        projected_ids.is_superset(&std::collections::BTreeSet::from([
            "agent-build",
            "agent-review",
        ])),
        "Company projection must include the Team's canonical AgentMembers: {projected_ids:?}"
    );
    assert!(membership_projections.iter().all(|member| {
        member.get("member_run_id").is_none()
            && member.get("work_id").is_none()
            && member.get("native_session").is_none()
    }));
    assert!(created["team_run"]["wave_id"].is_null());
    let team_run_id = created["team_run"]["id"].as_str().unwrap();
    let builder_member_id = created["member_runs"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        created["member_runs"][0]["native_session"]["native_session_id"].as_str(),
        Some("codex-session-1")
    );

    let judgment_2 = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host-plan",
            "--kind",
            "judgment",
            "--body",
            "Baseline lane is ready; review continues",
            "--json",
        ],
    );
    assert_eq!(judgment_2["revision"].as_u64(), Some(2));
    let replan = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host-plan",
            "--kind",
            "replan",
            "--body",
            "# Repair if needed\n\nIntegrate completed work while review continues; carry ReviewPartner forward and add RepairFixer.",
            "--json",
        ],
    );
    assert_eq!(replan["revision"].as_u64(), Some(3));
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "add-member",
            "--id",
            "team-platform",
            "--member",
            "agent-repair",
        ],
    );
    let joined = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "add-member",
            "--id",
            team_run_id,
            "--member",
            "agent-repair:repair specialist:codex",
            "--initial-work",
            "Repair any issue found by the review lane",
        ],
    );
    assert!(joined["work"]["context_markdown"]
        .as_str()
        .is_some_and(|context| !context.contains("wave-plan")));
    assert_eq!(
        joined["team_run"]["member_run_ids"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    let repair_member_id = joined["member_run"]["id"].as_str().unwrap().to_string();
    let renamed = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "rename-member",
            "--id",
            team_run_id,
            "--member-run-id",
            &repair_member_id,
            "--name",
            "TargetedRepair",
        ],
    );
    assert_eq!(renamed["name"].as_str(), Some("TargetedRepair"));
    let deactivated = run_json(
        &home,
        &project_id,
        &[
            "team-run",
            "deactivate-member",
            "--id",
            team_run_id,
            "--member-run-id",
            &repair_member_id,
            "--reason",
            "No reproducible defect remained after review",
        ],
    );
    assert_eq!(deactivated["status"].as_str(), Some("stopped"));

    let status = run_json(
        &home,
        &project_id,
        &["team-run", "status", "--id", team_run_id, "--json"],
    );
    let builder = status["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["member_run"]["id"].as_str() == Some(&builder_member_id))
        .unwrap();
    assert_eq!(
        builder["member_run"]["native_session"]["native_session_id"].as_str(),
        Some("codex-session-1"),
        "Recording Mission Log judgment must not replace the ProviderRuntimeProjection or provider-native session"
    );

    // Explicit retry lineage cannot jump to another stable Team or Mission.
    run_json(
        &home,
        &project_id,
        &[
            "mission",
            "create",
            "--id",
            "mission-other",
            "--title",
            "Other Mission",
            "--objective",
            "Retry isolation fixture",
            "--json",
        ],
    );
    run_json(
        &home,
        &project_id,
        &[
            "team",
            "create",
            "--id",
            "team-other",
            "--name",
            "Other Team",
            "--description",
            "Retry isolation fixture",
            "--mission-id",
            "mission-other",
            "--host-agent-id",
            "agent-build",
            "--node-id",
            &node_id,
            "--member",
            "agent-build",
        ],
    );
    let cross_team_retry = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "invalid cross-team retry",
            "--agent-team-id",
            "team-other",
            "--previous",
            team_run_id,
        ],
    );
    assert!(!cross_team_retry.status.success());
    assert!(
        String::from_utf8_lossy(&cross_team_retry.stderr).contains("not for the same agent team")
    );

    // A seeded historical wave cannot revive the retired CLI message writer.
    // Mission/source-plan validation now belongs to canonical Message authoring.
    seed_historical_wave(&home, &project_id, "wave-other", "mission-other", 1, "host");
    let cross_mission_origin = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "send",
            "--id",
            team_run_id,
            "--from",
            "host",
            "--to",
            &builder_member_id,
            "--kind",
            "message",
            "--body",
            "invalid cross-Mission origin",
            "--origin-wave-id",
            "wave-other",
        ],
    );
    assert!(!cross_mission_origin.status.success());
    assert!(
        String::from_utf8_lossy(&cross_mission_origin.stderr).contains("RETIRED_WRITE_AUTHORITY")
    );
    // Mission is immutable Team metadata; callers cannot rebind a TeamRun by
    // supplying a different Mission at attempt creation.
    let cross_mission_retry = run_firm(
        &home,
        home.base(),
        &[
            "--project",
            &project_id,
            "team-run",
            "create",
            "--objective",
            "invalid cross-mission retry",
            "--agent-team-id",
            "team-platform",
            "--mission-id",
            "mission-other",
            "--previous",
            team_run_id,
        ],
    );
    assert!(!cross_mission_retry.status.success());
    assert!(String::from_utf8_lossy(&cross_mission_retry.stderr)
        .contains("mission_id and wave_id were removed"));

    let closeout = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "log",
            "append",
            "--mission-id",
            "mission-host-plan",
            "--kind",
            "closeout_evidence",
            "--body",
            "Host plan complete",
            "--json",
        ],
    );
    assert_eq!(closeout["revision"].as_u64(), Some(4));
    // Mission close no longer requires a Wave gate (ADR 0051): this Mission's
    // wave_ids is empty (wave create is retired, and the historical rows
    // above were seeded directly, not through insert_wave_and_update_mission),
    // yet close still succeeds on its own outcome.
    let closed_mission = run_json(
        &home,
        &project_id,
        &[
            "mission",
            "close",
            "--id",
            "mission-host-plan",
            "--outcome",
            "Mission completed while its Team history remains durable",
            "--json",
        ],
    );
    assert!(closed_mission.get("wave_ids").is_none());
    let team = run_json(
        &home,
        &project_id,
        &["team", "show", "--id", "team-platform"],
    );
    assert_eq!(team["status"].as_str(), Some("active"));
    let run = run_json(
        &home,
        &project_id,
        &["team-run", "status", "--id", team_run_id, "--json"],
    );
    assert_eq!(run["team_run"]["status"].as_str(), Some("planning"));
    let cancelled = run_json(
        &home,
        &project_id,
        &["team-run", "cancel", "--id", team_run_id, "--json"],
    );
    assert_eq!(
        cancelled["status"].as_str(),
        Some("cancelled"),
        "Mission closeout must not prevent its TeamRun from settling"
    );
}

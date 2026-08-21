use super::*;

#[test]
fn same_id_team_migration_fails_closed_on_alias_and_purge_records_no_delete_tombstone() {
    let (store, root) = fabric_store();
    for id in ["legacy-host", "legacy-member"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context("operator", "identity.migrate", &format!("identity-{id}"), 0),
                identity(id),
            )
            .unwrap();
    }
    let source = firm_core::agentfirm_api::LegacyAgentTeamProjection {
        id: "legacy-team".into(),
        name: "Legacy Team".into(),
        description: "explicit same-ID import".into(),
        mission_id: "legacy-mission".into(),
        host_agent_id: "legacy-host".into(),
        node_id: "11111111-1111-4111-8111-111111111111".into(),
        status: firm_core::agentfirm_api::LegacyAgentTeamStatus::Archived,
        member_ids: vec!["legacy-member".into()],
        created_at: "t1".into(),
        updated_at: "t2".into(),
    };
    let target = AgentTeam {
        id: source.id.clone(),
        name: source.name.clone(),
        description: source.description.clone(),
        node_id: source.node_id.clone(),
        status: AgentTeamStatus::Trashed,
        revision: 1,
        legacy_mission_id: Some(source.mission_id.clone()),
        trashed_at: Some(source.updated_at.clone()),
        mission_id: source.mission_id.clone(),
        host_agent_id: source.host_agent_id.clone(),
        member_ids: source.member_ids.clone(),
        created_at: source.created_at.clone(),
        updated_at: source.updated_at.clone(),
    };
    let migration_actor = actor("operator");
    let memberships = [
        ("legacy-host", TeamMembershipRole::Host),
        ("legacy-member", TeamMembershipRole::Member),
    ]
    .into_iter()
    .map(|(member_id, role)| TeamMembership {
        id: format!("membership:legacy-team:{member_id}"),
        team_id: "legacy-team".into(),
        agent_member_id: member_id.into(),
        node_id: source.node_id.clone(),
        role,
        state: TeamMembershipStatus::Inactive,
        membership_generation: 1,
        default_subscription_refs: Vec::new(),
        created_by: migration_actor.clone(),
        revision: 1,
        joined_at: "t1".into(),
        left_at: Some("t2".into()),
    })
    .collect::<Vec<_>>();
    let source_fingerprint = canonical_json_fingerprint(&serde_json::to_value(&source).unwrap());
    let mut bundle = AgentTeamMigrationBundle {
        source,
        target,
        memberships,
        identity_id_map: BTreeMap::from([
            ("legacy-host".into(), "legacy-host".into()),
            ("legacy-member".into(), "legacy-member".into()),
        ]),
        migration_id: "migration-legacy-team".into(),
        source_fingerprint,
    };
    let before_alias = store.canonical_operations().unwrap();
    bundle
        .identity_id_map
        .insert("legacy-host".into(), "legacy-member".into());
    store
        .migrate_legacy_agent_team_same_ids(
            &context("operator", "agent_team.migrate", "migration-hostile", 0),
            bundle.clone(),
        )
        .expect_err("identity aliasing fails closed");
    assert_eq!(store.canonical_operations().unwrap(), before_alias);
    bundle
        .identity_id_map
        .insert("legacy-host".into(), "legacy-host".into());
    let migrated = store
        .migrate_legacy_agent_team_same_ids(
            &context("operator", "agent_team.migrate", "migration-legacy-team", 0),
            bundle,
        )
        .unwrap();
    assert_eq!(migrated.projection.id, "legacy-team");
    assert_eq!(migrated.projection.status, AgentTeamStatus::Trashed);
    let rows_before_purge = store.canonical_operations().unwrap().len();
    let tombstone = store
        .record_agent_team_purge_tombstone(
            &context("operator", "agent_team.purge", "purge-legacy-team", 0),
            AgentTeamPurgeRequest {
                tombstone_id: "purge-legacy-team".into(),
                team_id: "legacy-team".into(),
                expected_team_revision: 1,
                approval_ref: "approval:purge".into(),
                export_manifest_ref: "export:legacy-team".into(),
                restore_window_closed_at: "t3".into(),
                requested_by: migration_actor,
                requested_at: "t4".into(),
            },
        )
        .unwrap();
    assert_eq!(tombstone.projection.team_id, "legacy-team");
    assert_eq!(
        store.canonical_operations().unwrap().len(),
        rows_before_purge + 1
    );
    assert!(store
        .agent_teams("space-test")
        .unwrap()
        .iter()
        .any(|team| team.id == "legacy-team"));
    assert_eq!(
        store
            .fabric_team_memberships("space-test")
            .unwrap()
            .iter()
            .filter(|membership| membership.team_id == "legacy-team")
            .count(),
        2
    );
    fs::remove_dir_all(root).unwrap();
}

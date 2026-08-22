use super::*;

#[test]
fn work_execution_binding_rechecks_authoritative_dependency_readiness() {
    let (store, _root) = fabric_store();
    for identity_id in ["builder", "host-a"] {
        store
            .migrate_legacy_agent_identity_same_id(
                &context(
                    "operator",
                    "identity.create",
                    &format!("identity-{identity_id}"),
                    0,
                ),
                identity(identity_id),
            )
            .unwrap();
    }
    let session = session("session-builder", "builder");
    store
        .create_agent_session(
            &service_context("session.create", "session-builder", 0),
            session.clone(),
        )
        .unwrap();
    append_runtime_team(&store, "team-a", "team-run-a");
    let membership = join_runtime_membership(
        &store,
        "membership-builder",
        "team-a",
        "builder",
        TeamMembershipRole::Member,
    );
    let prerequisite = insert_runtime_work(&store, "work-prerequisite", "team-a", "team-run-a");
    let dependent = insert_runtime_work(&store, "work-dependent", "team-a", "team-run-a");
    let dependent = store
        .replace_work_dependencies(
            &dependent.id,
            dependent.version,
            vec![prerequisite.id],
            firm_core::WorkCommandContext {
                event_id: "dependencies-dependent".into(),
                performed_by_actor: firm_core::TeamActorRef {
                    kind: firm_core::TeamActorKind::Host,
                    id: "host-a".into(),
                    display_name: None,
                    authn_source: Some("test".into()),
                },
                authority_actor: None,
                causation_ref: None,
                idempotency_key: "dependencies-dependent".into(),
                created_at: "t-dependencies".into(),
                duplicate_ok: false,
            },
        )
        .unwrap();

    let before = store.canonical_operations().unwrap();
    let error = store
        .bind_work_execution(
            &context("fixture-host", "work.bind", "binding-dependent", 0),
            WorkExecutionBinding {
                id: "binding-dependent".into(),
                work_id: dependent.id.clone(),
                work_revision: dependent.version,
                team_id: membership.team_id.clone(),
                team_membership_id: membership.id,
                agent_member_id: "builder".into(),
                agent_session_id: session.id,
                agent_session_generation: session.runtime_generation,
                delivery_id: "delivery-dependent".into(),
                binding_generation: 1,
                status: WorkExecutionBindingStatus::Active,
                version: 1,
                created_by: actor("fixture-host"),
                bound_at: "t-bind".into(),
                ended_at: None,
            },
        )
        .expect_err("pending prerequisite must reject exact execution binding");
    assert!(error.to_string().contains("WORK_NOT_READY"));
    assert_eq!(
        store.canonical_operations().unwrap(),
        before,
        "readiness rejection has zero canonical side effects"
    );
}

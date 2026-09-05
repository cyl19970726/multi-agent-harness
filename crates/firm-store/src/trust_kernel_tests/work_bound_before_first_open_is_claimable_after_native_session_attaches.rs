use super::work_responsibility_execution_admission_is_exact_and_idempotent::{
    admit_member_run, assign_responsibility, canonical_member_run, execution_binding,
};
use super::*;

/// GitHub #745 (DEV-231): a Work may be bound before the first provider Open
/// returns the native session id. When the id then attaches to the same exact
/// AgentSession generation, the stale-release judges the binding current
/// (`allow_native_session_attachment: true`) — so the claim must agree.
/// Before this fix the claim admitted the frozen binding with the strict
/// invocation rule, rejected it with MEMBER_RUN_GENERATION_FENCED on every
/// pass, and the pass failed the member run with no self-healing path.
#[test]
fn work_bound_before_first_open_is_claimable_after_native_session_attaches() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    store
        .migrate_legacy_agent_identity_same_id(
            &context("operator", "identity.create", "identity-pre-open", 0),
            identity("pre-open"),
        )
        .unwrap();
    let membership = join_runtime_membership(
        &store,
        "membership-pre-open",
        "team-admission",
        "pre-open",
        TeamMembershipRole::Member,
    );
    // The session exists but the provider has not opened yet: no native id.
    let target = session("session-pre-open", "pre-open");
    assert!(target.native_session_ref.is_none());
    store
        .create_agent_session(
            &service_context("session.create", "session-pre-open", 0),
            target.clone(),
        )
        .unwrap();
    admit_member_run(
        &store,
        canonical_member_run("member-run-pre-open", "pre-open", "run-admission"),
    );
    let mut runtime_binding = runtime_command_fixture(
        "runtime-pre-open",
        RuntimeCommandKind::StartCycle,
        &target,
        "start_cycle",
    )
    .0
    .binding;
    runtime_binding.target_member_run_id = Some("member-run-pre-open".into());
    runtime_binding.target_member_run_generation = Some(1);
    assert!(runtime_binding.native_session_ref.is_none());

    let work = assign_responsibility(&store, "work-pre-open", &membership.id);
    let binding = execution_binding(&work, &membership, &target, "binding-pre-open");
    store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-pre-open", 0),
            &runtime_binding,
            binding.clone(),
        )
        .expect("a Work is bound before the first provider Open");

    // The first Open attaches the native id to the same generation without
    // bumping the runtime or driver generation.
    let native = settled_native_session("thread-pre-open");
    let bound = store
        .bind_agent_session_native_session(
            &service_context("session.native.bind", "bind-native-pre-open", 1),
            &target.id,
            1,
            native.clone(),
        )
        .expect("the first Open attaches the native session");
    assert_eq!(bound.projection.runtime_generation, 1);
    assert_eq!(bound.projection.control_state.driver_generation, 1);

    // The stale-release keeps the binding current...
    let reconciliation = store
        .release_work_execution_binding_if_stale(
            &service_context(
                "node_daemon.work_execution_binding.release_if_stale",
                "stale-pre-open",
                binding.version,
            ),
            &binding.id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "t-stale",
        )
        .expect("stale reconciliation reads the binding");
    assert!(
        matches!(reconciliation, WorkExecutionBindingReconciliation::Current),
        "the attachment is durable progress, not stale authority: {reconciliation:?}"
    );

    // ...so the claim must admit the same binding under the same rule.
    let claimed = store
        .claim_work_for_provider(
            &service_context("work.claim", "claim-pre-open", 0),
            &binding.delivery_id,
            &target.node_id,
            &target.node_daemon_id,
            target.node_daemon_generation,
            "claim-pre-open",
            firm_core::agentfirm_api::RuntimeDispatchMode::QueueOnly,
            "t-claim",
        )
        .expect("a binding minted before the first Open is claimable once the native id attaches");
    assert_eq!(
        claimed.projection.source_record_id, binding.delivery_id,
        "the invocation carries exactly the pre-Open delivery"
    );
    assert_eq!(
        claimed.projection.binding.native_session_ref.as_ref(),
        Some(&native),
        "the recorded invocation carries the session the claim ran against"
    );
    let delivery = store
        .fabric_work_deliveries("space-test")
        .unwrap()
        .into_iter()
        .find(|candidate| candidate.id == binding.delivery_id)
        .expect("the claimed WorkDelivery");
    assert_eq!(delivery.status, WorkDeliveryStatus::Claimed);
    assert_eq!(delivery.claim_id.as_deref(), Some("claim-pre-open"));
    assert_eq!(
        store
            .fabric_work_execution_bindings("space-test")
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == binding.id)
            .unwrap()
            .status,
        WorkExecutionBindingStatus::Active,
        "no second binding, no release: the same delivery was claimed exactly once"
    );

    // Replacement is never tolerated: a binding frozen on a different native
    // id is fenced at bind time. (It can never reach the claim: bind is strict
    // and the session's native ref is write-once, so this negative sits on the
    // strict path by construction.)
    let other_work = assign_responsibility(&store, "work-pre-open-other", &membership.id);
    let mut other_runtime_binding = runtime_binding.clone();
    other_runtime_binding.native_session_ref = Some(settled_native_session("thread-other"));
    let mut other_binding =
        execution_binding(&other_work, &membership, &target, "binding-pre-open-other");
    other_binding.binding_generation = 1;
    other_binding.delivery_id = format!("work-delivery:{}:1", other_work.id);
    let fenced_bind = store
        .bind_responsible_work_execution(
            &service_context("work.bind", "binding-pre-open-other", 0),
            &other_runtime_binding,
            other_binding.clone(),
        )
        .expect_err("a binding naming another native session is not admitted");
    assert!(
        fenced_bind
            .to_string()
            .contains("MEMBER_RUN_GENERATION_FENCED"),
        "{fenced_bind}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

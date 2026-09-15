use super::work_responsibility_execution_admission_is_exact_and_idempotent::{
    admit_member_run, assign_responsibility, canonical_member_run, execution_binding,
};
use super::*;
use firm_core::agentfirm_api::{
    ActorKind, ActorRef, MutationContext, RuntimeCommandBinding, RuntimeCommandKind,
    WorkExecutionBinding,
};

fn daemon_context(
    daemon_id: &str,
    command: &str,
    key: &str,
    expected_version: u64,
) -> MutationContext {
    MutationContext {
        execution_space_id: "space-test".into(),
        authenticated_actor: ActorRef {
            kind: ActorKind::Service,
            id: daemon_id.into(),
        },
        authority_actor: None,
        command_name: command.into(),
        idempotency_key: key.into(),
        expected_version,
        request_fingerprint: None,
    }
}

fn runtime_binding_for(session: &AgentSession, member_run_id: &str) -> RuntimeCommandBinding {
    let mut binding = runtime_command_fixture(
        &format!("runtime-{}", session.id),
        RuntimeCommandKind::StartCycle,
        session,
        "start_cycle",
    )
    .0
    .binding;
    binding.target_member_run_id = Some(member_run_id.into());
    binding.target_member_run_generation = Some(1);
    binding
}

/// Raise the persisted binding generation to the ceiling the way a corrupt or
/// hand-edited ledger row would: the entrance itself only ever admits `max + 1`,
/// so this is the only way the counter can arrive there.
fn raise_persisted_binding_to_the_ceiling(root: &std::path::Path, binding_id: &str, work_id: &str) {
    let path = root.join("agentfirm_trust_operations.jsonl");
    let content = fs::read_to_string(&path).unwrap();
    let rows = content
        .lines()
        .map(|line| {
            let mut row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["operation"]["event"]["aggregate_kind"] == "work_execution_binding"
                && row["operation"]["event"]["aggregate_id"] == binding_id
            {
                for slot in ["resulting_projection", "event"] {
                    let target = &mut row["operation"][slot];
                    if slot == "event" {
                        target["payload"]["binding_generation"] = u64::MAX.into();
                        target["payload"]["delivery_id"] =
                            format!("work-delivery:{work_id}:{}", u64::MAX).into();
                    } else {
                        target["binding_generation"] = u64::MAX.into();
                        target["delivery_id"] =
                            format!("work-delivery:{work_id}:{}", u64::MAX).into();
                    }
                }
            }
            serde_json::to_string(&row).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, format!("{rows}\n")).unwrap();
}

/// `expected_binding_generation` is `max(existing) + 1`, and the entrance admits
/// a binding only when its generation equals that expectation. Computed with
/// `saturating_add(1)`, a ledger already sitting at `u64::MAX` expects `u64::MAX`
/// — so a second binding claiming the predecessor's own generation matched, and
/// with it the `work-delivery:{work}:{generation}` id it derives. `checked_add`
/// refuses the ceiling instead of minting a colliding successor.
#[test]
fn work_execution_binding_generation_fails_closed_at_the_ceiling() {
    let (store, root) = fabric_store();
    append_runtime_team(&store, "team-admission", "run-admission");
    seed_agent_member(
        &store,
        &context("operator", "identity.create", "identity-binding-ceiling", 0),
        "binding-ceiling",
    );
    let membership = join_runtime_membership(
        &store,
        "membership-binding-ceiling",
        "team-admission",
        "binding-ceiling",
        TeamMembershipRole::Member,
    );
    let session = session("session-binding-ceiling", "binding-ceiling");
    store
        .create_agent_session(
            &service_context("session.create", &session.id, 0),
            session.clone(),
        )
        .unwrap();
    let member_run_id = "member-run-binding-ceiling";
    admit_member_run(
        &store,
        canonical_member_run(member_run_id, "binding-ceiling", "run-admission"),
    );
    let work = assign_responsibility(&store, "work-binding-ceiling", &membership.id);

    let first = execution_binding(&work, &membership, &session, "binding-ceiling-1");
    store
        .bind_responsible_work_execution(
            &daemon_context("daemon-1", "work.bind", "binding-ceiling-1", 0),
            &runtime_binding_for(&session, member_run_id),
            first,
        )
        .expect("the ordinary first binding takes generation 1");

    raise_persisted_binding_to_the_ceiling(&root, "binding-ceiling-1", &work.id);
    assert_eq!(
        store
            .fabric_work_execution_bindings("space-test")
            .unwrap()
            .into_iter()
            .find(|binding| binding.id == "binding-ceiling-1")
            .expect("seeded binding is readable")
            .binding_generation,
        u64::MAX,
        "the ceiling row must be what the expectation is computed from"
    );

    let mut successor: WorkExecutionBinding =
        execution_binding(&work, &membership, &session, "binding-ceiling-2");
    successor.binding_generation = u64::MAX;
    successor.delivery_id = format!("work-delivery:{}:{}", work.id, u64::MAX);

    let before = store.canonical_operations().unwrap();
    let refusal = store
        .bind_responsible_work_execution(
            &daemon_context("daemon-1", "work.bind", "binding-ceiling-2", 0),
            &runtime_binding_for(&session, member_run_id),
            successor,
        )
        .expect_err("a successor binding must not reuse the predecessor's generation");
    assert!(
        refusal
            .to_string()
            .contains("WORK_EXECUTION_BINDING_GENERATION_EXHAUSTED"),
        "unexpected refusal: {refusal}"
    );
    assert_eq!(
        store.canonical_operations().unwrap(),
        before,
        "a refused binding must not commit a projection"
    );
    fs::remove_dir_all(root).unwrap();
}

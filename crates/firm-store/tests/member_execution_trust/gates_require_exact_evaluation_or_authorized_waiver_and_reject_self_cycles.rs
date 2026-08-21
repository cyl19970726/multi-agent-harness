use super::*;

#[test]
fn gates_require_exact_evaluation_or_authorized_waiver_and_reject_self_cycles() {
    let harness = TestStore::new("gates");
    let team_id = seed_team_work(&harness.store, "gates", "work-gate");
    let host = human("host");
    let critic = member_actor("critic");
    let mut cyclic = requirement("gate-cycle");
    cyclic.dependency_requirement_ids.push(cyclic.id.clone());
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_requirement(
                    &context(host.clone(), "gate.require", "cycle", 0),
                    &team_id,
                    cyclic,
                )
                .expect_err("self-cycle must fail")
        ),
        TrustErrorCode::GateDependencyCycle
    );
    let mut cycle_a = requirement("cycle-a");
    cycle_a.required = false;
    cycle_a.dependency_requirement_ids = vec!["cycle-b".into()];
    harness
        .store
        .create_trust_gate_requirement(
            &context(host.clone(), "gate.require", "cycle-a", 0),
            &team_id,
            cycle_a,
        )
        .expect("forward dependency may be declared");
    let mut cycle_b = requirement("cycle-b");
    cycle_b.required = false;
    cycle_b.dependency_requirement_ids = vec!["cycle-a".into()];
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_requirement(
                    &context(host.clone(), "gate.require", "cycle-b", 0),
                    &team_id,
                    cycle_b,
                )
                .expect_err("transitive cycle must fail")
        ),
        TrustErrorCode::GateDependencyCycle
    );

    harness
        .store
        .create_trust_gate_requirement(
            &context(host.clone(), "gate.require", "gate-a", 0),
            &team_id,
            requirement("gate-a"),
        )
        .expect("create exact requirement");
    assert_eq!(
        trust_code(
            harness
                .store
                .trust_gate_satisfied(SPACE, "work-gate", 1, "report-gate", "sha256:candidate",)
                .expect_err("required gate needs evaluation")
        ),
        TrustErrorCode::GateEvaluationRequired
    );
    let mut stale = evaluation("eval-stale", "gate-a", &critic);
    stale.candidate_fingerprint = "sha256:other".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_evaluation(
                    &context(critic.clone(), "gate.evaluate", "stale", 0),
                    stale,
                )
                .expect_err("stale candidate must fail")
        ),
        TrustErrorCode::GateRequirementStale
    );
    let before_wrong_evaluator = harness.store.canonical_operations().unwrap().len();
    let impostor = member_actor("worker");
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_evaluation(
                    &context(impostor.clone(), "gate.evaluate", "wrong-evaluator", 0),
                    evaluation("eval-impostor", "gate-a", &impostor),
                )
                .expect_err("wrong evaluator identity must fail")
        ),
        TrustErrorCode::UnauthorizedActor
    );
    assert_eq!(
        harness.store.canonical_operations().unwrap().len(),
        before_wrong_evaluator,
        "wrong evaluator rejection must have zero durable side effects"
    );
    harness
        .store
        .create_trust_gate_evaluation(
            &context(critic, "gate.evaluate", "exact", 0),
            evaluation("eval-exact", "gate-a", &member_actor("critic")),
        )
        .expect("exact evaluation");

    harness
        .store
        .create_trust_gate_requirement(
            &context(host.clone(), "gate.require", "gate-b", 0),
            &team_id,
            requirement("gate-b"),
        )
        .expect("create waiver requirement");
    let authority = human("release-manager");
    let waiver = GateWaiver {
        id: "waiver-b".into(),
        requirement_id: "gate-b".into(),
        work_id: "work-gate".into(),
        work_revision: 1,
        candidate_fingerprint: "sha256:candidate".into(),
        authority_actor: authority.clone(),
        performed_by_actor: host.clone(),
        reason: "documented emergency".into(),
        evidence_refs: vec!["evidence://waiver".into()],
        state: GateWaiverState::Active,
        version: 1,
        created_at: "t3".into(),
        revoked_at: None,
    };
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_waiver(
                    &context(host.clone(), "gate.waive", "unauthorized", 0),
                    waiver.clone(),
                )
                .expect_err("authority must be explicit")
        ),
        TrustErrorCode::GateWaiverUnauthorized
    );
    let mut authorized = context(host, "gate.waive", "authorized", 0);
    authorized.authority_actor = Some(authority);
    let mut missing_requirement = waiver.clone();
    missing_requirement.id = "waiver-missing".into();
    missing_requirement.requirement_id = "gate-missing".into();
    let mut missing_context = authorized.clone();
    missing_context.idempotency_key = "missing-requirement".into();
    assert_eq!(
        trust_code(
            harness
                .store
                .create_trust_gate_waiver(&missing_context, missing_requirement)
                .expect_err("waiver requirement must resolve")
        ),
        TrustErrorCode::GateRequirementStale
    );
    harness
        .store
        .create_trust_gate_waiver(&authorized, waiver)
        .expect("authorized waiver");
    harness
        .store
        .trust_gate_satisfied(SPACE, "work-gate", 1, "report-gate", "sha256:candidate")
        .expect("exact evaluation plus exact waiver satisfy all required gates");
}

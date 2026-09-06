use super::*;

#[test]
fn report_id_is_create_only_even_for_direct_store_callers() {
    let harness = TestStore::new("immutable-report");
    let team = seed_active_team_work(&harness.store, "immutable-report", "work-1");
    let worker = member_actor("worker");
    let report = report("progress-immutable", WorkReportKind::Progress, &worker);
    let ctx = context(worker.clone(), "report.create", "original", 0);
    let first = harness
        .store
        .create_trust_work_report(&ctx, &team, report.clone())
        .unwrap();
    let replay = harness
        .store
        .create_trust_work_report(&ctx, &team, report.clone())
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(first.event.id, replay.event.id);
    let before = harness.store.canonical_operations().unwrap();
    let mut changed = report;
    changed.summary = "rewritten content".into();
    let mut spoofed_replay = ctx.clone();
    spoofed_replay.request_fingerprint = Some(first.event.canonical_request_fingerprint.clone());
    assert!(
        harness
            .store
            .create_trust_work_report(&spoofed_replay, &team, changed.clone())
            .is_err(),
        "an optional transport fingerprint cannot hide changed Report content"
    );
    harness
        .store
        .create_trust_work_report(
            &context(worker, "report.create", "rewrite", 1),
            &team,
            changed,
        )
        .expect_err("an existing report ID cannot be rewritten with an aggregate CAS");
    assert_eq!(harness.store.canonical_operations().unwrap(), before);
}

fn submit_report_only(harness: &TestStore, team: &str) -> WorkReport {
    let worker = member_actor("worker");
    let mut result = report("report-only", WorkReportKind::Result, &worker);
    result.work_revision = 4;
    result.report_only = true;
    result.evidence_refs = vec!["check:reviewed-report".into()];
    harness
        .store
        .create_trust_work_report(
            &context(worker, "report.create", "submit-only", 0),
            team,
            result.clone(),
        )
        .unwrap();
    result
}

#[test]
fn report_only_accepts_atomic_submission_and_replays_original_snapshot() {
    let harness = TestStore::new("report-only-accept");
    let team = seed_active_team_work(&harness.store, "report-only-accept", "work-1");
    let report = submit_report_only(&harness, &team);
    let ctx = context(human("reviewer"), "work.accept", "accept-only", 4);
    let accepted = harness
        .store
        .accept_current_trust_work(&ctx, &team, "work-1", "t5")
        .unwrap();
    assert_eq!(
        accepted.projection.resolution,
        Some(firm_core::WorkResolution::Accepted)
    );
    assert_eq!(accepted.projection.version, 5);
    let operations = harness.store.canonical_operations_for_space(SPACE).unwrap();
    let acceptance = operations
        .iter()
        .find(|op| op.event.id == accepted.event.id)
        .unwrap();
    assert_eq!(acceptance.event.payload["work_report_id"], report.id);
    assert!(acceptance
        .immutable_side_records
        .contains(&serde_json::to_value(&report).unwrap()));
    let replay = harness
        .store
        .accept_current_trust_work(&ctx, &team, "work-1", "t5")
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.event.id, accepted.event.id);
    let mut stale = ctx.clone();
    stale.expected_version = 3;
    assert_eq!(
        trust_code(
            harness
                .store
                .accept_current_trust_work(&stale, &team, "work-1", "t5")
                .unwrap_err()
        ),
        TrustErrorCode::IdempotencyKeyReused
    );
    assert!(harness
        .store
        .accept_current_trust_work(&ctx, "foreign-team", "work-1", "t5")
        .is_err());
    assert!(harness
        .store
        .accept_current_trust_work(&ctx, &team, "foreign-work", "t5")
        .is_err());
    assert_eq!(
        harness.store.canonical_operations_for_space(SPACE).unwrap(),
        operations
    );
}

#[test]
fn report_only_cannot_bypass_its_candidate_requirement_but_ignores_another_report() {
    for (label, report_id, succeeds) in [
        ("own-gate", "report-only", false),
        ("other-gate", "older-report", true),
    ] {
        let harness = TestStore::new(label);
        let team = seed_active_team_work(&harness.store, label, "work-1");
        submit_report_only(&harness, &team);
        let mut gate = requirement("candidate-gate");
        gate.work_id = "work-1".into();
        gate.work_revision = 4;
        gate.work_report_id = report_id.into();
        harness
            .store
            .create_trust_gate_requirement(
                &context(human("host"), "gate.require", "gate", 0),
                &team,
                gate,
            )
            .unwrap();
        let before = harness.store.canonical_operations().unwrap();
        let result = harness.store.accept_current_trust_work(
            &context(human("reviewer"), "work.accept", "accept", 4),
            &team,
            "work-1",
            "t5",
        );
        assert_eq!(result.is_ok(), succeeds, "{label}: {result:?}");
        if !succeeds {
            assert_eq!(harness.store.canonical_operations().unwrap(), before);
        }
    }
}

// Model pre-fix stored histories without adding any production import/writer.
fn edit_history(harness: &TestStore, edit: impl FnOnce(&mut Vec<serde_json::Value>)) {
    let path = harness.root.join("agentfirm_trust_operations.jsonl");
    let mut rows = std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect::<Vec<_>>();
    edit(&mut rows);
    let bytes = rows
        .iter()
        .map(|row| serde_json::to_string(row).unwrap() + "\n")
        .collect::<String>();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn ambiguous_report_history_and_unbound_result_are_not_selected_as_latest() {
    for variant in [
        "rewritten-id",
        "second-result",
        "no-snapshot",
        "wrong-owner",
        "wrong-space",
    ] {
        let harness = TestStore::new(variant);
        let team = seed_active_team_work(&harness.store, variant, "work-1");
        submit_report_only(&harness, &team);
        edit_history(&harness, |rows| {
            let index = rows
                .iter()
                .position(|row| row["operation"]["event"]["aggregate_id"] == "report-only")
                .unwrap();
            if variant == "no-snapshot" {
                rows[index]["operation"]["immutable_side_records"] = serde_json::json!([]);
            } else if variant == "wrong-owner" {
                rows[index]["operation"]["resulting_projection"]["authored_by"]["id"] =
                    serde_json::json!("other-worker");
            } else if variant == "wrong-space" {
                rows[index]["execution_space_id"] = serde_json::json!("other-space");
            } else {
                let mut extra = rows[index].clone();
                extra["operation"]["event"]["id"] = serde_json::json!("extra-report-event");
                extra["operation"]["resulting_projection"]["summary"] =
                    serde_json::json!("replacement");
                extra["operation"]["resulting_projection"]["report_revision"] =
                    serde_json::json!(999);
                if variant == "second-result" {
                    extra["operation"]["event"]["aggregate_id"] = serde_json::json!("other-result");
                    extra["operation"]["resulting_projection"]["id"] =
                        serde_json::json!("other-result");
                } else {
                    extra["operation"]["event"]["resulting_version"] = serde_json::json!(2);
                }
                rows.push(extra);
            }
        });
        let before = std::fs::read(harness.root.join("agentfirm_trust_operations.jsonl")).unwrap();
        assert!(
            harness
                .store
                .accept_current_trust_work(
                    &context(human("reviewer"), "work.accept", "accept", 4),
                    &team,
                    "work-1",
                    "t5"
                )
                .is_err(),
            "{variant}"
        );
        assert_eq!(
            std::fs::read(harness.root.join("agentfirm_trust_operations.jsonl")).unwrap(),
            before
        );
    }
}

#[test]
fn integration_plan_report_only_submission_and_old_incompatible_binding_are_refused() {
    use firm_core::agentfirm_api::{WorkModuleBinding, WorkModuleId};
    for historical in [false, true] {
        let harness = TestStore::new("report-only-module");
        let team = seed_active_team_work(&harness.store, "report-only-module", "work-1");
        let config = serde_json::json!({
            "base_revision":"base", "target_revision":"target", "work_boundaries":[],
            "candidate_boundaries":[], "interfaces":[], "convergence_points":[],
            "merge_order":[], "conflict_owner":"host", "per_merge_checks":[],
            "combined_verification":[], "rollback_plan":"revert"
        });
        let binding = WorkModuleBinding {
            id: "integration-binding".into(),
            work_id: "work-1".into(),
            work_revision: 3,
            module_id: WorkModuleId::IntegrationPlan,
            module_version: firm_core::agentfirm_api::WorkModuleVersion::V1,
            config_fingerprint: canonical_json_fingerprint(&config),
            resolved_config: config,
            attached_by: human("host"),
            attached_at: "t3".into(),
            version: 1,
        };
        harness
            .store
            .bind_trust_work_module(
                &context(human("host"), "module.bind", "bind", 0),
                &team,
                binding,
            )
            .unwrap();
        if !historical {
            let worker = member_actor("worker");
            let mut result = report("report-only", WorkReportKind::Result, &worker);
            result.work_revision = 4;
            result.report_only = true;
            result.evidence_refs = vec!["check:report".into()];
            let before = harness.store.canonical_operations().unwrap();
            assert!(harness
                .store
                .create_trust_work_report(
                    &context(worker, "report.create", "submit", 0),
                    &team,
                    result
                )
                .is_err());
            assert_eq!(harness.store.canonical_operations().unwrap(), before);
        } else {
            // Retain a genuine canonical binding row, temporarily remove it to
            // produce a valid old report, then restore that inconsistent history.
            let mut binding_row = None;
            edit_history(&harness, |rows| {
                let index = rows
                    .iter()
                    .position(|row| {
                        row["operation"]["event"]["aggregate_id"] == "integration-binding"
                    })
                    .unwrap();
                binding_row = Some(rows.remove(index));
            });
            submit_report_only(&harness, &team);
            edit_history(&harness, |rows| rows.push(binding_row.unwrap()));
            assert!(harness
                .store
                .accept_current_trust_work(
                    &context(human("reviewer"), "work.accept", "accept", 4),
                    &team,
                    "work-1",
                    "t5"
                )
                .is_err());
        }
    }
}

#[test]
fn ordinary_candidate_acceptance_retains_verifier_payload() {
    let harness = TestStore::new("ordinary-candidate");
    let team = seed_active_team_work(&harness.store, "ordinary-candidate", "work-1");
    let worker = member_actor("worker");
    let mut result = report("candidate-report", WorkReportKind::Result, &worker);
    result.work_revision = 4;
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: "candidate-commit".into(),
    };
    let fingerprint = canonical_json_fingerprint(&serde_json::to_value(&candidate).unwrap());
    result.candidate = Some(candidate);
    result.candidate_fingerprint = Some(fingerprint.clone());
    result.evidence_refs = vec!["check:candidate".into()];
    harness
        .store
        .create_trust_work_report(
            &context(worker, "report.create", "submit", 0),
            &team,
            result,
        )
        .unwrap();
    let mut gate = requirement("candidate-check");
    gate.work_id = "work-1".into();
    gate.work_revision = 4;
    gate.work_report_id = "candidate-report".into();
    gate.candidate_fingerprint = fingerprint.clone();
    harness
        .store
        .create_trust_gate_requirement(
            &context(human("host"), "gate.require", "require", 0),
            &team,
            gate,
        )
        .unwrap();
    let before = harness.store.canonical_operations().unwrap();
    assert_eq!(
        trust_code(
            harness
                .store
                .accept_current_trust_work(
                    &context(human("reviewer"), "work.accept", "pending", 4),
                    &team,
                    "work-1",
                    "t5",
                )
                .unwrap_err()
        ),
        TrustErrorCode::GateEvaluationRequired
    );
    assert_eq!(harness.store.canonical_operations().unwrap(), before);
    let critic = member_actor("critic");
    let mut passed = evaluation("candidate-pass", "candidate-check", &critic);
    passed.work_id = "work-1".into();
    passed.work_revision = 4;
    passed.work_report_id = "candidate-report".into();
    passed.candidate_fingerprint = fingerprint.clone();
    harness
        .store
        .create_trust_gate_evaluation(&context(critic, "gate.evaluate", "evaluate", 0), passed)
        .unwrap();
    let accepted = harness
        .store
        .accept_current_trust_work(
            &context(human("reviewer"), "work.accept", "accept", 4),
            &team,
            "work-1",
            "t5",
        )
        .unwrap();
    assert_eq!(accepted.event.payload["candidate_fingerprint"], fingerprint);
    assert_eq!(accepted.event.payload["work_report_id"], "candidate-report");
}

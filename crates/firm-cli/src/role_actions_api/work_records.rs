use super::*;

pub(super) fn execute_work_record_action(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    team_id: &str,
    work_id: &str,
    operation: &str,
    body: &[u8],
    _confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid Work record intent: {error}"),
            "work",
            work_id,
            None,
        )
    })?;
    let team = store.latest_teams()?.remove(team_id).ok_or_else(|| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            "AgentTeam does not exist",
            "team",
            team_id,
            None,
        )
    })?;
    let current = current_canonical_work(store, &auth.execution_space_id, work_id)?;
    if current.accountable_team_id.as_deref() != Some(team_id) {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "Work does not belong to the addressed Team",
            "work",
            work_id,
            Some(current.version),
        ));
    }
    match operation {
        "request-changes" | "gate-requirements" => {
            require_host(&auth, &team.host_agent_id, "work", work_id)?;
        }
        "revise" | "reports" | "findings" | "failure-analyses" => {
            let _ = require_exact_work_member(store, &auth, &current)?;
        }
        _ => {}
    }
    let replay = match operation {
        "request-changes" => work_replay(
            store,
            &auth,
            work_id,
            harness_core::WorkEventKind::ChangesRequested,
        )?,
        "revise" | "reports" => canonical_replay(
            store,
            &auth,
            "work_report",
            &deterministic_id("work-report", &auth),
        )?,
        "findings" => canonical_replay(
            store,
            &auth,
            "work_finding",
            &deterministic_id("work-finding", &auth),
        )?,
        "failure-analyses" => canonical_replay(
            store,
            &auth,
            "failure_analysis",
            &deterministic_id("failure-analysis", &auth),
        )?,
        "gate-requirements" => canonical_replay(
            store,
            &auth,
            "gate_requirement",
            &deterministic_id("gate-requirement", &auth),
        )?,
        _ => None,
    };
    if let Some(replay) = replay {
        return Ok(replay);
    }
    if auth.expected_version != current.version {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "Work record action requires the exact current Work revision",
            "work",
            work_id,
            Some(current.version),
        ));
    }
    if operation == "request-changes" {
        let RoleActionIntent::RequestChanges { reason } = intent else {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match request-changes",
                "work",
                work_id,
                Some(current.version),
            ));
        };
        let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
        let before = store.work_operations()?.len();
        let work = store.request_work_changes(
            work_id,
            auth.expected_version,
            &reason,
            host_context(&auth, host_id, false),
        )?;
        return work_action_result(store, &auth, before, work);
    }
    if operation == "revise" {
        let RoleActionIntent::ReviseWork {
            result_summary,
            artifact_refs,
            check_refs,
            base_revision,
            candidate_revision,
        } = intent
        else {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match revise",
                "work",
                work_id,
                Some(current.version),
            ));
        };
        let _member_run = require_exact_work_member(store, &auth, &current)?;
        return create_result_report(
            store,
            auth,
            &team,
            &current,
            ResultReportInput {
                result_summary,
                artifact_refs,
                check_refs,
                base_revision,
                candidate_revision,
            },
        );
    }
    match (operation, intent) {
        (
            "reports",
            RoleActionIntent::WriteReport {
                summary,
                evidence_refs,
                recommended_next_action,
            },
        ) => {
            let _member_run = require_exact_work_member(store, &auth, &current)?;
            let report = WorkReport {
                id: deterministic_id("work-report", &auth),
                work_id: work_id.into(),
                work_revision: current.version,
                report_revision: canonical_report_count(store, &auth.execution_space_id, work_id)?
                    + 1,
                kind: WorkReportKind::Progress,
                authored_by: auth.actor.clone(),
                summary,
                base_revision: None,
                candidate: None,
                candidate_fingerprint: None,
                finding_refs: Vec::new(),
                failure_analysis_ref: None,
                artifact_refs: Vec::new(),
                check_refs: Vec::new(),
                evidence_refs,
                known_risks: Vec::new(),
                confidence: Some(Confidence::Medium),
                recommended_next_action,
                created_at: now_string(),
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::CreateWorkReport {
                    team_id: team_id.into(),
                    report,
                },
            )?))
        }
        (
            "findings",
            RoleActionIntent::WriteFinding {
                kind,
                summary,
                detail_markdown,
                evidence_refs,
                confidence,
            },
        ) => {
            let _member_run = require_exact_work_member(store, &auth, &current)?;
            let finding = WorkFinding {
                id: deterministic_id("work-finding", &auth),
                work_id: work_id.into(),
                work_revision: current.version,
                kind,
                summary,
                detail_markdown,
                affected_work_refs: Vec::new(),
                reusable_asset_refs: Vec::new(),
                invalidated_assumptions: Vec::new(),
                evidence_refs,
                confidence,
                reported_by: auth.actor.clone(),
                created_at: now_string(),
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::CreateWorkFinding {
                    team_id: team_id.into(),
                    finding,
                },
            )?))
        }
        (
            "failure-analyses",
            RoleActionIntent::WriteFailure {
                observed_failure,
                impact,
                primary_cause_status,
                primary_cause,
                retry_safety,
                recommended_host_decision,
                evidence_refs,
                confidence,
            },
        ) => {
            let member_run = require_exact_work_member(store, &auth, &current)?;
            let analysis = FailureAnalysis {
                id: deterministic_id("failure-analysis", &auth),
                work_id: work_id.into(),
                work_revision: current.version,
                member_run_id: Some(member_run),
                candidate: None,
                observed_failure,
                impact,
                primary_cause_status,
                primary_cause,
                contributing_causes: Vec::new(),
                attempts_already_made: Vec::new(),
                last_safe_checkpoint: None,
                retry_safety,
                side_effect_summary: None,
                recovery_options: Vec::new(),
                recommended_host_decision,
                evidence_refs,
                confidence,
                reported_by: auth.actor.clone(),
                created_at: now_string(),
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::CreateFailureAnalysis {
                    team_id: team_id.into(),
                    analysis,
                },
            )?))
        }
        (
            "gate-requirements",
            RoleActionIntent::RequestGateEvaluation {
                gate_type,
                gate_contract_version,
                evaluator_ref,
                evaluator_version,
                resolved_config,
                required,
            },
        ) => {
            require_host(&auth, &team.host_agent_id, "work", work_id)?;
            let report = store
                .canonical_operations_for_space(&auth.execution_space_id)?
                .into_iter()
                .filter(|op| op.event.aggregate_kind == "work_report")
                .filter_map(|op| serde_json::from_value::<WorkReport>(op.resulting_projection).ok())
                .filter(|report| {
                    report.work_id == work_id
                        && report.kind == WorkReportKind::Result
                        && report.work_revision == current.version
                })
                .max_by_key(|report| report.report_revision)
                .ok_or_else(|| {
                    encoded_error(
                        "REPORT_EVIDENCE_MISSING",
                        "Gate request requires the exact current result report",
                        "work",
                        work_id,
                        Some(current.version),
                    )
                })?;
            let candidate_fingerprint = report.candidate_fingerprint.clone().ok_or_else(|| {
                encoded_error(
                    "REPORT_EVIDENCE_MISSING",
                    "result report has no candidate fingerprint",
                    "work",
                    work_id,
                    Some(current.version),
                )
            })?;
            let evaluator_fingerprint = canonical_json_fingerprint(
                &json!({"actor":evaluator_ref,"version":evaluator_version}),
            );
            let config_fingerprint = canonical_json_fingerprint(&resolved_config);
            let requirement = GateRequirement {
                id: deterministic_id("gate-requirement", &auth),
                work_id: work_id.into(),
                work_revision: current.version,
                work_report_id: report.id,
                candidate_fingerprint,
                source: GateRequirementSource::Direct,
                source_binding_id: None,
                gate_type,
                gate_contract_version,
                evaluator_ref,
                evaluator_version,
                evaluator_fingerprint,
                resolved_config,
                config_fingerprint,
                required,
                dependency_requirement_ids: Vec::new(),
                requirement_set_fingerprint: String::new(),
                created_at: now_string(),
                version: 1,
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::CreateGateRequirement {
                    team_id: team_id.into(),
                    requirement,
                },
            )?))
        }
        _ => Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match Work record route",
            "work",
            work_id,
            Some(current.version),
        )),
    }
}

pub(super) struct ResultReportInput {
    result_summary: String,
    artifact_refs: Vec<String>,
    check_refs: Vec<String>,
    base_revision: Option<String>,
    candidate_revision: String,
}

pub(super) fn create_result_report(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    team: &harness_core::AgentTeam,
    current: &Work,
    input: ResultReportInput,
) -> Result<RoleActionResult, StoreError> {
    let ResultReportInput {
        result_summary,
        artifact_refs,
        check_refs,
        base_revision,
        candidate_revision,
    } = input;
    if candidate_revision.trim().is_empty() || artifact_refs.is_empty() && check_refs.is_empty() {
        return Err(encoded_error(
            "REPORT_EVIDENCE_MISSING",
            "result revision and at least one evidence ref are required",
            "work",
            &current.id,
            Some(current.version),
        ));
    }
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: candidate_revision,
    };
    let candidate_fingerprint = canonical_json_fingerprint(&serde_json::to_value(&candidate)?);
    let evidence_refs = artifact_refs
        .iter()
        .chain(check_refs.iter())
        .cloned()
        .collect();
    let report = WorkReport {
        id: deterministic_id("work-report", &auth),
        work_id: current.id.clone(),
        work_revision: current.version + 1,
        report_revision: canonical_report_count(store, &auth.execution_space_id, &current.id)? + 1,
        kind: WorkReportKind::Result,
        authored_by: auth.actor.clone(),
        summary: result_summary,
        base_revision,
        candidate: Some(candidate),
        candidate_fingerprint: Some(candidate_fingerprint),
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs,
        check_refs,
        evidence_refs,
        known_risks: Vec::new(),
        confidence: Some(Confidence::High),
        recommended_next_action: Some("host_review".into()),
        created_at: now_string(),
    };
    auth.expected_version = 0;
    Ok(trust_result(crate::agentfirm_api::execute(
        store,
        auth,
        crate::agentfirm_api::TrustCommand::CreateWorkReport {
            team_id: team.id.clone(),
            report,
        },
    )?))
}

pub(super) fn work_action_result(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    before: usize,
    work: Work,
) -> Result<RoleActionResult, StoreError> {
    let operations = store.work_operations()?;
    let operation = operations
        .iter()
        .rev()
        .find(|operation| {
            operation.work.id == work.id && operation.event.idempotency_key == auth.idempotency_key
        })
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "Work mutation committed without its operation",
                "work",
                &work.id,
                Some(work.version),
            )
        })?;
    Ok(RoleActionResult {
        ok: true,
        action_protocol_version: "agentfirm.role_actions.v1",
        projection: serde_json::to_value(&work)?,
        event_id: operation.event.id.clone(),
        resulting_version: work.version,
        store_sequence: operations.len() as u64,
        replayed: operations.len() == before,
    })
}

pub(super) fn execute_gate_action(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    requirement_id: &str,
    operation: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid Gate intent: {error}"),
            "gate_requirement",
            requirement_id,
            None,
        )
    })?;
    let requirement = store
        .canonical_operations_for_space(&auth.execution_space_id)?
        .into_iter()
        .filter(|operation| operation.event.aggregate_kind == "gate_requirement")
        .flat_map(|operation| {
            std::iter::once(operation.resulting_projection).chain(operation.immutable_side_records)
        })
        .filter_map(|value| serde_json::from_value::<GateRequirement>(value).ok())
        .filter(|item| item.id == requirement_id)
        .max_by_key(|item| item.version)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "GateRequirement does not exist",
                "gate_requirement",
                requirement_id,
                None,
            )
        })?;
    let replay_id = match operation {
        "evaluate" => {
            if auth.actor != requirement.evaluator_ref {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "only the frozen exact evaluator may evaluate this gate",
                    "gate_requirement",
                    requirement_id,
                    Some(requirement.version),
                ));
            }
            deterministic_id("gate-evaluation", &auth)
        }
        "waive" => {
            if confirmed_action != Some("waive_gate") {
                return Err(encoded_error(
                    "CONFIRMATION_REQUIRED",
                    "server confirmation must exactly confirm waive_gate",
                    "gate_requirement",
                    requirement_id,
                    Some(requirement.version),
                ));
            }
            if auth.authorized_authority_actors.is_empty() {
                return Err(encoded_error(
                    "UNAUTHORIZED_ACTOR",
                    "credential has no frozen waiver authority",
                    "gate_requirement",
                    requirement_id,
                    Some(requirement.version),
                ));
            }
            deterministic_id("gate-waiver", &auth)
        }
        _ => {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "unknown Gate operation",
                "gate_requirement",
                requirement_id,
                Some(requirement.version),
            ))
        }
    };
    if let Some(replay) = canonical_replay(
        store,
        &auth,
        if operation == "evaluate" {
            "gate_evaluation"
        } else {
            "gate_waiver"
        },
        &replay_id,
    )? {
        return Ok(replay);
    }
    if auth.expected_version != requirement.version {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "Gate action requires the exact current requirement revision",
            "gate_requirement",
            requirement_id,
            Some(requirement.version),
        ));
    }
    match (operation, intent) {
        (
            "evaluate",
            RoleActionIntent::EvaluateGate {
                verdict,
                summary,
                evidence_refs,
            },
        ) => {
            let mut dependency_ids = requirement.dependency_requirement_ids.clone();
            dependency_ids.sort();
            let evaluation = GateEvaluation {
                id: deterministic_id("gate-evaluation", &auth),
                requirement_id: requirement.id.clone(),
                work_id: requirement.work_id.clone(),
                work_revision: requirement.work_revision,
                work_report_id: requirement.work_report_id.clone(),
                candidate_fingerprint: requirement.candidate_fingerprint.clone(),
                config_fingerprint: requirement.config_fingerprint.clone(),
                evaluator_version: requirement.evaluator_version.clone(),
                evaluator_fingerprint: requirement.evaluator_fingerprint.clone(),
                dependency_fingerprint: canonical_json_fingerprint(&serde_json::to_value(
                    dependency_ids,
                )?),
                verdict,
                summary,
                evidence_refs,
                performed_by: auth.actor.clone(),
                evaluated_at: now_string(),
                version: 1,
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::EvaluateGate { evaluation },
            )?))
        }
        (
            "waive",
            RoleActionIntent::WaiveGate {
                reason,
                evidence_refs,
            },
        ) => {
            let authority_actor = auth
                .authorized_authority_actors
                .iter()
                .find(|authority| **authority == auth.actor)
                .cloned()
                .or_else(|| auth.authorized_authority_actors.first().cloned())
                .ok_or_else(|| {
                    encoded_error(
                        "UNAUTHORIZED_ACTOR",
                        "credential has no frozen waiver authority",
                        "gate_requirement",
                        requirement_id,
                        Some(requirement.version),
                    )
                })?;
            let waiver = GateWaiver {
                id: deterministic_id("gate-waiver", &auth),
                requirement_id: requirement.id,
                work_id: requirement.work_id,
                work_revision: requirement.work_revision,
                candidate_fingerprint: requirement.candidate_fingerprint,
                authority_actor,
                performed_by_actor: auth.actor.clone(),
                reason,
                evidence_refs,
                state: GateWaiverState::Active,
                version: 1,
                created_at: now_string(),
                revoked_at: None,
            };
            auth.expected_version = 0;
            Ok(trust_result(crate::agentfirm_api::execute(
                store,
                auth,
                crate::agentfirm_api::TrustCommand::WaiveGate { waiver },
            )?))
        }
        _ => Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match Gate route",
            "gate_requirement",
            requirement_id,
            Some(requirement.version),
        )),
    }
}

pub(super) fn execute_waiver_revoke(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    waiver_id: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid waiver intent: {error}"),
            "gate_waiver",
            waiver_id,
            None,
        )
    })?;
    if !matches!(intent, RoleActionIntent::RevokeWaiver) {
        return Err(encoded_error(
            "INVALID_STATE_TRANSITION",
            "semantic action does not match waiver revoke",
            "gate_waiver",
            waiver_id,
            None,
        ));
    }
    let waiver = store
        .trust_gate_waivers(&auth.execution_space_id)?
        .into_iter()
        .find(|item| item.id == waiver_id)
        .ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "GateWaiver does not exist",
                "gate_waiver",
                waiver_id,
                None,
            )
        })?;
    if confirmed_action != Some("revoke_waiver") {
        return Err(encoded_error(
            "CONFIRMATION_REQUIRED",
            "server confirmation must exactly confirm revoke_waiver",
            "gate_waiver",
            waiver_id,
            Some(waiver.version),
        ));
    }
    if waiver.performed_by_actor != auth.actor
        || !auth
            .authorized_authority_actors
            .contains(&waiver.authority_actor)
    {
        return Err(encoded_error(
            "UNAUTHORIZED_ACTOR",
            "only the exact waiver actor with its frozen authority may revoke",
            "gate_waiver",
            waiver_id,
            Some(waiver.version),
        ));
    }
    if let Some(replay) = canonical_replay(store, &auth, "gate_waiver", waiver_id)? {
        return Ok(replay);
    }
    if auth.expected_version != waiver.version {
        return Err(encoded_error(
            "VERSION_CONFLICT",
            "waiver revoke requires exact current revision",
            "gate_waiver",
            waiver_id,
            Some(waiver.version),
        ));
    }
    Ok(trust_result(crate::agentfirm_api::execute(
        store,
        auth,
        crate::agentfirm_api::TrustCommand::RevokeGateWaiver {
            waiver_id: waiver_id.into(),
            revoked_at: now_string(),
        },
    )?))
}

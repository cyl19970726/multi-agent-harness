use super::*;

impl HarnessStore {
    pub fn create_trust_work_report(
        &self,
        context: &MutationContext,
        team_id: &str,
        report: WorkReport,
    ) -> StoreResult<CanonicalMutationResult<WorkReport>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let source_work_revision = if report.kind == WorkReportKind::Result {
            report.work_revision.checked_sub(1).ok_or_else(|| {
                trust_error(
                    TrustErrorCode::WorkRevisionStale,
                    "result report must name the resulting non-zero Work revision",
                    "work_report",
                    &report.id,
                    None,
                )
            })?
        } else {
            report.work_revision
        };
        let current_work =
            self.trust_team_work_unlocked(team_id, &report.work_id, source_work_revision)?;
        self.require_exact_work_member_unlocked(
            &context.execution_space_id,
            &current_work,
            &context.authenticated_actor,
        )?;
        if report.authored_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkReport.authored_by must equal the authenticated actor",
                "work_report",
                &report.id,
                None,
            ));
        }
        if report.kind == WorkReportKind::Result
            && (report.candidate.is_none()
                || report
                    .candidate_fingerprint
                    .as_deref()
                    .unwrap_or("")
                    .is_empty()
                || report.evidence_refs.is_empty())
        {
            return Err(trust_error(
                TrustErrorCode::ReportEvidenceMissing,
                "result report requires exact CandidateRef, fingerprint and evidence",
                "work_report",
                &report.id,
                None,
            ));
        }
        if report.kind == WorkReportKind::Result
            && (current_work.phase != firm_core::WorkPhase::Active
                || current_work.condition != firm_core::WorkCondition::Normal
                || report.work_revision != current_work.version + 1)
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "result report may submit only normal active Work and must name the resulting Work revision",
                "work_report",
                &report.id,
                Some(current_work.version),
            ));
        }
        if let (Some(candidate), Some(fingerprint)) = (
            report.candidate.as_ref(),
            report.candidate_fingerprint.as_ref(),
        ) {
            let expected = canonical_json_fingerprint(&serde_json::to_value(candidate)?);
            if fingerprint != &expected {
                return Err(trust_error(
                    TrustErrorCode::ReportEvidenceMissing,
                    "candidate_fingerprint does not match canonical CandidateRef",
                    "work_report",
                    &report.id,
                    None,
                ));
            }
        }
        if report.kind == WorkReportKind::Failure && report.failure_analysis_ref.is_none() {
            return Err(trust_error(
                TrustErrorCode::FailureAnalysisMissing,
                "failure report requires FailureAnalysis",
                "work_report",
                &report.id,
                None,
            ));
        }
        if let Some(analysis_id) = report.failure_analysis_ref.as_deref() {
            let analysis = self
                .latest_trust_envelopes_unlocked(&context.execution_space_id, "failure_analysis")?
                .remove(analysis_id)
                .ok_or_else(|| {
                    trust_error(
                        TrustErrorCode::FailureAnalysisMissing,
                        "failure report references a missing FailureAnalysis",
                        "work_report",
                        &report.id,
                        None,
                    )
                })
                .and_then(|envelope| event_projection::<FailureAnalysis>(&envelope))?;
            if analysis.work_id != report.work_id || analysis.work_revision != report.work_revision
            {
                return Err(trust_error(
                    TrustErrorCode::FailureAnalysisMissing,
                    "FailureAnalysis does not match the report Work revision",
                    "work_report",
                    &report.id,
                    None,
                ));
            }
        }
        let mut resolved_requirements = Vec::new();
        if report.kind == WorkReportKind::Result {
            let candidate_fingerprint = report
                .candidate_fingerprint
                .as_ref()
                .expect("result validation requires candidate fingerprint");
            let bindings = self
                .latest_trust_envelopes_unlocked(
                    &context.execution_space_id,
                    "work_module_binding",
                )?
                .into_values()
                .map(|envelope| event_projection::<WorkModuleBinding>(&envelope))
                .collect::<StoreResult<Vec<_>>>()?;
            for binding in bindings.into_iter().filter(|binding| {
                binding.work_id == report.work_id
                    && binding.work_revision == source_work_revision
                    && binding.module_id == WorkModuleId::IntegrationPlan
                    && binding.module_version == 1
            }) {
                let definition = integration_plan_module_v1();
                for (index, template) in definition.default_gate_templates.iter().enumerate() {
                    let resolved_config = serde_json::json!({
                        "module_binding_id": binding.id,
                        "module_binding_version": binding.version,
                        "module_config_fingerprint": binding.config_fingerprint,
                        "template": template,
                    });
                    let evaluator_ref = firm_core::agentfirm_api::ActorRef {
                        kind: firm_core::agentfirm_api::ActorKind::Service,
                        id: definition.implementation_ref.clone(),
                    };
                    let evaluator_version = definition.module_version.to_string();
                    resolved_requirements.push(GateRequirement {
                        id: format!("gate:{}:{}:{index}", report.id, binding.id),
                        work_id: report.work_id.clone(),
                        work_revision: report.work_revision,
                        work_report_id: report.id.clone(),
                        candidate_fingerprint: candidate_fingerprint.clone(),
                        source: GateRequirementSource::Module,
                        source_binding_id: Some(binding.id.clone()),
                        gate_type: template
                            .get("gate_type")
                            .and_then(Value::as_str)
                            .unwrap_or("integration-plan-completeness")
                            .to_string(),
                        gate_contract_version: template
                            .get("gate_contract_version")
                            .and_then(Value::as_str)
                            .unwrap_or("1")
                            .to_string(),
                        evaluator_fingerprint: gate_evaluator_fingerprint(
                            &evaluator_ref,
                            &evaluator_version,
                        ),
                        evaluator_ref,
                        evaluator_version,
                        config_fingerprint: canonical_json_fingerprint(&resolved_config),
                        resolved_config,
                        required: template
                            .get("required")
                            .and_then(Value::as_bool)
                            .unwrap_or(true),
                        dependency_requirement_ids: Vec::new(),
                        requirement_set_fingerprint: String::new(),
                        created_at: report.created_at.clone(),
                        version: 1,
                    });
                }
            }
            let mut requirement_ids = resolved_requirements
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect::<Vec<_>>();
            requirement_ids.sort();
            let set_fingerprint =
                canonical_json_fingerprint(&serde_json::to_value(requirement_ids)?);
            for requirement in &mut resolved_requirements {
                requirement.requirement_set_fingerprint = set_fingerprint.clone();
            }
        }
        let mut side_records = resolved_requirements
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
        if report.kind == WorkReportKind::Result {
            let mut submitted_work = current_work;
            submitted_work.phase = firm_core::WorkPhase::Review;
            submitted_work.condition = firm_core::WorkCondition::Normal;
            submitted_work.version = report.work_revision;
            submitted_work.result_summary = Some(report.summary.clone());
            submitted_work.updated_at = report.created_at.clone();
            side_records.push(serde_json::to_value(submitted_work)?);
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_report",
            &report.id,
            "created",
            serde_json::to_value(&report)?,
            &report,
            side_records,
            Vec::new(),
        )
    }

    /// Latest immutable Work reports available to server-side application
    /// services. Callers must still bind the selected report to the current
    /// Work, Team, actor and placement before publishing it remotely.
    pub fn trust_work_reports(&self, execution_space_id: &str) -> StoreResult<Vec<WorkReport>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "work_report")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_work_findings(&self, execution_space_id: &str) -> StoreResult<Vec<WorkFinding>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "work_finding")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn trust_failure_analyses(
        &self,
        execution_space_id: &str,
    ) -> StoreResult<Vec<FailureAnalysis>> {
        self.latest_trust_envelopes_unlocked(execution_space_id, "failure_analysis")?
            .values()
            .map(event_projection)
            .collect()
    }

    pub fn create_trust_finding(
        &self,
        context: &MutationContext,
        team_id: &str,
        finding: WorkFinding,
    ) -> StoreResult<CanonicalMutationResult<WorkFinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let work =
            self.trust_team_work_unlocked(team_id, &finding.work_id, finding.work_revision)?;
        self.require_exact_work_member_unlocked(
            &context.execution_space_id,
            &work,
            &context.authenticated_actor,
        )?;
        if finding.reported_by != context.authenticated_actor {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "WorkFinding.reported_by must equal the authenticated actor",
                "work_finding",
                &finding.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_finding",
            &finding.id,
            "created",
            serde_json::to_value(&finding)?,
            &finding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_failure_analysis(
        &self,
        context: &MutationContext,
        team_id: &str,
        analysis: FailureAnalysis,
    ) -> StoreResult<CanonicalMutationResult<FailureAnalysis>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let work =
            self.trust_team_work_unlocked(team_id, &analysis.work_id, analysis.work_revision)?;
        let run = self.require_exact_work_member_unlocked(
            &context.execution_space_id,
            &work,
            &context.authenticated_actor,
        )?;
        if analysis.reported_by != context.authenticated_actor
            || analysis.member_run_id.as_deref() != Some(run.id.as_str())
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "FailureAnalysis must name the authenticated Work owner's exact active MemberRun",
                "failure_analysis",
                &analysis.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "failure_analysis",
            &analysis.id,
            "created",
            serde_json::to_value(&analysis)?,
            &analysis,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn bind_trust_work_module(
        &self,
        context: &MutationContext,
        team_id: &str,
        binding: WorkModuleBinding,
    ) -> StoreResult<CanonicalMutationResult<WorkModuleBinding>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        self.trust_team_work_unlocked(team_id, &binding.work_id, binding.work_revision)?;
        if binding.config_fingerprint != canonical_json_fingerprint(&binding.resolved_config) {
            return Err(trust_error(
                TrustErrorCode::ModuleConfigInvalid,
                "module config_fingerprint does not match resolved_config",
                "work_module_binding",
                &binding.id,
                None,
            ));
        }
        if binding.module_id != WorkModuleId::IntegrationPlan || binding.module_version != 1 {
            return Err(trust_error(
                TrustErrorCode::ModuleConfigInvalid,
                "unknown Work module id or version",
                "work_module_binding",
                &binding.id,
                None,
            ));
        }
        if !binding.resolved_config.is_object()
            || ![
                "base_revision",
                "target_revision",
                "work_boundaries",
                "candidate_boundaries",
                "interfaces",
                "convergence_points",
                "merge_order",
                "conflict_owner",
                "per_merge_checks",
                "combined_verification",
                "rollback_plan",
            ]
            .into_iter()
            .all(|key| binding.resolved_config.get(key).is_some())
        {
            return Err(trust_error(
                TrustErrorCode::ModuleConfigInvalid,
                "integration-plan@1 config is incomplete",
                "work_module_binding",
                &binding.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "work_module_binding",
            &binding.id,
            "attached",
            serde_json::to_value(&binding)?,
            &binding,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_gate_requirement(
        &self,
        context: &MutationContext,
        team_id: &str,
        mut requirement: GateRequirement,
    ) -> StoreResult<CanonicalMutationResult<GateRequirement>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        self.trust_team_work_unlocked(team_id, &requirement.work_id, requirement.work_revision)?;
        let expected_evaluator_fingerprint =
            gate_evaluator_fingerprint(&requirement.evaluator_ref, &requirement.evaluator_version);
        if requirement.evaluator_fingerprint != expected_evaluator_fingerprint {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "GateRequirement evaluator fingerprint does not match its frozen ActorRef/version",
                "gate_requirement",
                &requirement.id,
                None,
            ));
        }
        let existing = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .into_values()
            .collect::<Vec<_>>();
        if existing.iter().any(|item| item.id == requirement.id) {
            return Err(trust_error(
                TrustErrorCode::VersionConflict,
                "GateRequirement id already exists",
                "gate_requirement",
                &requirement.id,
                Some(1),
            ));
        }
        let mut graph = existing
            .iter()
            .map(|item| (item.id.clone(), item.dependency_requirement_ids.clone()))
            .collect::<BTreeMap<_, _>>();
        graph.insert(
            requirement.id.clone(),
            requirement.dependency_requirement_ids.clone(),
        );
        pub(super) fn reaches(
            graph: &BTreeMap<String, Vec<String>>,
            current: &str,
            target: &str,
            seen: &mut BTreeSet<String>,
        ) -> bool {
            if current == target {
                return true;
            }
            if !seen.insert(current.to_string()) {
                return false;
            }
            graph
                .get(current)
                .into_iter()
                .flatten()
                .any(|next| reaches(graph, next, target, seen))
        }
        if requirement
            .dependency_requirement_ids
            .iter()
            .any(|dependency| reaches(&graph, dependency, &requirement.id, &mut BTreeSet::new()))
        {
            return Err(trust_error(
                TrustErrorCode::GateDependencyCycle,
                "gate requirement introduces a dependency cycle",
                "gate_requirement",
                &requirement.id,
                None,
            ));
        }
        let mut same_set = existing
            .into_iter()
            .filter(|item| {
                item.work_id == requirement.work_id
                    && item.work_revision == requirement.work_revision
                    && item.work_report_id == requirement.work_report_id
                    && item.candidate_fingerprint == requirement.candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let mut required_ids = same_set
            .iter()
            .filter(|item| item.required)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        if requirement.required {
            required_ids.push(requirement.id.clone());
        }
        required_ids.sort();
        let set_fingerprint = canonical_json_fingerprint(&serde_json::to_value(required_ids)?);
        requirement.requirement_set_fingerprint = set_fingerprint.clone();
        for existing in &mut same_set {
            if existing.required {
                existing.requirement_set_fingerprint = set_fingerprint.clone();
                existing.version += 1;
            }
        }
        self.commit_trust_projection_unlocked(
            context,
            "gate_requirement",
            &requirement.id,
            "created",
            serde_json::to_value(&requirement)?,
            &requirement,
            same_set
                .into_iter()
                .filter(|item| item.required)
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()?,
            Vec::new(),
        )
    }

    pub fn create_trust_gate_evaluation(
        &self,
        context: &MutationContext,
        evaluation: GateEvaluation,
    ) -> StoreResult<CanonicalMutationResult<GateEvaluation>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let requirements = self.trust_gate_requirements_unlocked(&context.execution_space_id)?;
        let requirement = requirements
            .get(&evaluation.requirement_id)
            .cloned()
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "gate requirement not found",
                    "gate_evaluation",
                    &evaluation.id,
                    None,
                )
            })?;
        if context.authenticated_actor != requirement.evaluator_ref
            || evaluation.performed_by != context.authenticated_actor
        {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "authenticated evaluator must exactly match the frozen GateRequirement evaluator",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        let mut dependency_ids = requirement.dependency_requirement_ids.clone();
        dependency_ids.sort();
        let expected_dependency_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(dependency_ids)?);
        let prior_evaluations = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let waivers = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        if requirement.dependency_requirement_ids.iter().any(|id| {
            requirements.get(id).is_none_or(|dependency| {
                !gate_requirement_is_satisfied(
                    dependency,
                    &requirements,
                    &prior_evaluations,
                    &waivers,
                    &mut BTreeSet::new(),
                )
            })
        }) {
            return Err(trust_error(
                TrustErrorCode::GateEvaluationRequired,
                "gate dependencies must be satisfied before evaluation",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        if requirement.work_id != evaluation.work_id
            || requirement.work_revision != evaluation.work_revision
            || requirement.work_report_id != evaluation.work_report_id
            || requirement.candidate_fingerprint != evaluation.candidate_fingerprint
            || requirement.config_fingerprint != evaluation.config_fingerprint
            || requirement.evaluator_version != evaluation.evaluator_version
            || requirement.evaluator_fingerprint != evaluation.evaluator_fingerprint
            || evaluation.dependency_fingerprint != expected_dependency_fingerprint
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "evaluation does not exactly match the frozen requirement",
                "gate_evaluation",
                &evaluation.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "gate_evaluation",
            &evaluation.id,
            "evaluated",
            serde_json::to_value(&evaluation)?,
            &evaluation,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn create_trust_gate_waiver(
        &self,
        context: &MutationContext,
        waiver: GateWaiver,
    ) -> StoreResult<CanonicalMutationResult<GateWaiver>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        if waiver.state != GateWaiverState::Active
            || context.authority_actor.as_ref() != Some(&waiver.authority_actor)
            || context.authenticated_actor != waiver.performed_by_actor
        {
            return Err(trust_error(
                TrustErrorCode::GateWaiverUnauthorized,
                "waiver authority and authenticated actor must match the mutation context",
                "gate_waiver",
                &waiver.id,
                None,
            ));
        }
        let requirement = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .remove(&waiver.requirement_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "waiver references a missing gate requirement",
                    "gate_waiver",
                    &waiver.id,
                    None,
                )
            })?;
        if requirement.work_id != waiver.work_id
            || requirement.work_revision != waiver.work_revision
            || requirement.candidate_fingerprint != waiver.candidate_fingerprint
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "waiver does not exactly match the frozen requirement",
                "gate_waiver",
                &waiver.id,
                None,
            ));
        }
        self.commit_trust_projection_unlocked(
            context,
            "gate_waiver",
            &waiver.id,
            "created",
            serde_json::to_value(&waiver)?,
            &waiver,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn revoke_trust_gate_waiver(
        &self,
        context: &MutationContext,
        waiver_id: &str,
        revoked_at: &str,
    ) -> StoreResult<CanonicalMutationResult<GateWaiver>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut waiver = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .remove(waiver_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::GateRequirementStale,
                    "gate waiver not found",
                    "gate_waiver",
                    waiver_id,
                    None,
                )
            })
            .and_then(|envelope| event_projection::<GateWaiver>(&envelope))?;
        if waiver.state != GateWaiverState::Active
            || context.authority_actor.as_ref() != Some(&waiver.authority_actor)
            || context.authenticated_actor != waiver.performed_by_actor
        {
            return Err(trust_error(
                TrustErrorCode::GateWaiverUnauthorized,
                "only the exact authorized actor may revoke an active waiver",
                "gate_waiver",
                waiver_id,
                Some(waiver.version),
            ));
        }
        waiver.state = GateWaiverState::Revoked;
        waiver.version += 1;
        waiver.revoked_at = Some(revoked_at.to_string());
        self.commit_trust_projection_unlocked(
            context,
            "gate_waiver",
            waiver_id,
            "revoked",
            serde_json::json!({"revoked_at": revoked_at}),
            &waiver,
            Vec::new(),
            Vec::new(),
        )
    }

    pub fn trust_gate_satisfied(
        &self,
        execution_space_id: &str,
        work_id: &str,
        work_revision: u64,
        report_id: &str,
        candidate_fingerprint: &str,
    ) -> StoreResult<()> {
        let requirements = self
            .trust_gate_requirements_unlocked(execution_space_id)?
            .into_values()
            .filter(|requirement| {
                requirement.work_id == work_id
                    && requirement.work_revision == work_revision
                    && requirement.work_report_id == report_id
                    && requirement.candidate_fingerprint == candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let mut requirement_ids = requirements
            .iter()
            .filter(|requirement| requirement.required)
            .map(|requirement| requirement.id.clone())
            .collect::<Vec<_>>();
        requirement_ids.sort();
        let expected_set_fingerprint =
            canonical_json_fingerprint(&serde_json::to_value(requirement_ids)?);
        if requirements
            .iter()
            .filter(|requirement| requirement.required)
            .any(|requirement| requirement.requirement_set_fingerprint != expected_set_fingerprint)
        {
            return Err(trust_error(
                TrustErrorCode::GateRequirementStale,
                "gate requirement set fingerprint is stale",
                "work",
                work_id,
                Some(work_revision),
            ));
        }
        let bindings = self
            .latest_trust_envelopes_unlocked(execution_space_id, "work_module_binding")?
            .into_values()
            .map(|envelope| event_projection::<WorkModuleBinding>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        for requirement in &requirements {
            if requirement.source == GateRequirementSource::Module {
                let binding = requirement
                    .source_binding_id
                    .as_deref()
                    .and_then(|id| bindings.iter().find(|binding| binding.id == id))
                    .ok_or_else(|| {
                        trust_error(
                            TrustErrorCode::GateRequirementStale,
                            "module-derived gate lost its source binding",
                            "work",
                            work_id,
                            Some(work_revision),
                        )
                    })?;
                if binding.work_id != requirement.work_id
                    || binding.work_revision != requirement.work_revision
                    || binding.config_fingerprint
                        != requirement
                            .resolved_config
                            .get("module_config_fingerprint")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                    || binding.version
                        != requirement
                            .resolved_config
                            .get("module_binding_version")
                            .and_then(Value::as_u64)
                            .unwrap_or_default()
                {
                    return Err(trust_error(
                        TrustErrorCode::GateRequirementStale,
                        "module-derived gate no longer matches its frozen source binding",
                        "work",
                        work_id,
                        Some(work_revision),
                    ));
                }
            }
        }
        let evaluations = self
            .latest_trust_envelopes_unlocked(execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let waivers = self
            .latest_trust_envelopes_unlocked(execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?;
        let requirement_map = requirements
            .iter()
            .cloned()
            .map(|requirement| (requirement.id.clone(), requirement))
            .collect::<BTreeMap<_, _>>();
        for requirement in requirements
            .into_iter()
            .filter(|requirement| requirement.required)
        {
            if !gate_requirement_is_satisfied(
                &requirement,
                &requirement_map,
                &evaluations,
                &waivers,
                &mut BTreeSet::new(),
            ) {
                return Err(trust_error(
                    TrustErrorCode::GateEvaluationRequired,
                    "required gate has no exact valid evaluation or waiver",
                    "work",
                    work_id,
                    Some(work_revision),
                ));
            }
        }
        Ok(())
    }

    pub fn accept_trust_work(
        &self,
        context: &MutationContext,
        team_id: &str,
        work_id: &str,
        report_id: &str,
        candidate_fingerprint: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let request_payload = serde_json::json!({
            "team_id": team_id,
            "work_id": work_id,
            "work_report_id": report_id,
            "candidate_fingerprint": candidate_fingerprint,
            "updated_at": updated_at,
        });
        let request_fingerprint = canonical_json_fingerprint(&request_payload);
        if let Some(replay) =
            self.trust_operation_envelopes_unlocked()?
                .into_iter()
                .find(|envelope| {
                    envelope.execution_space_id == context.execution_space_id
                        && envelope.authenticated_actor_kind == context.authenticated_actor.kind
                        && envelope.authenticated_actor_id == context.authenticated_actor.id
                        && envelope.command_name == context.command_name
                        && envelope.operation.event.idempotency_key == context.idempotency_key
                })
        {
            if replay.operation.event.canonical_request_fingerprint != request_fingerprint
                || replay.operation.event.aggregate_kind != "work"
                || replay.operation.event.aggregate_id != work_id
            {
                return Err(trust_error(
                    TrustErrorCode::IdempotencyKeyReused,
                    "idempotency key was already used for a different Work acceptance",
                    "work",
                    work_id,
                    Some(replay.operation.event.resulting_version),
                ));
            }
            return Ok(CanonicalMutationResult {
                projection: event_projection(&replay)?,
                event: replay.operation.event,
                replayed: true,
            });
        }
        let current = self.trust_team_work_unlocked(team_id, work_id, context.expected_version)?;
        if current.phase != firm_core::WorkPhase::Review
            || current.condition != firm_core::WorkCondition::Normal
        {
            return Err(trust_error(
                TrustErrorCode::InvalidStateTransition,
                "Work must be in normal review before acceptance",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        if current.owner_member_id.as_deref() == Some(context.authenticated_actor.id.as_str()) {
            return Err(trust_error(
                TrustErrorCode::UnauthorizedActor,
                "the accountable Work owner cannot accept its own candidate",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        let report = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "work_report")?
            .remove(report_id)
            .ok_or_else(|| {
                trust_error(
                    TrustErrorCode::ReportEvidenceMissing,
                    "exact result WorkReport not found",
                    "work",
                    work_id,
                    Some(current.version),
                )
            })
            .and_then(|envelope| event_projection::<WorkReport>(&envelope))?;
        if report.kind != WorkReportKind::Result
            || report.work_id != current.id
            || report.work_revision != current.version
            || report.candidate.is_none()
            || report.candidate_fingerprint.as_deref() != Some(candidate_fingerprint)
            || report.evidence_refs.is_empty()
        {
            return Err(trust_error(
                TrustErrorCode::ReportEvidenceMissing,
                "acceptance requires the exact result Report, Candidate and evidence",
                "work",
                work_id,
                Some(current.version),
            ));
        }
        self.trust_gate_satisfied(
            &context.execution_space_id,
            work_id,
            current.version,
            report_id,
            candidate_fingerprint,
        )?;
        let requirements = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .into_values()
            .filter(|requirement| {
                requirement.work_id == work_id
                    && requirement.work_revision == current.version
                    && requirement.work_report_id == report_id
                    && requirement.candidate_fingerprint == candidate_fingerprint
            })
            .collect::<Vec<_>>();
        let requirement_ids = requirements
            .iter()
            .map(|requirement| requirement.id.as_str())
            .collect::<BTreeSet<_>>();
        let evaluations = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_evaluation")?
            .into_values()
            .map(|envelope| event_projection::<GateEvaluation>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .filter(|evaluation| requirement_ids.contains(evaluation.requirement_id.as_str()))
            .collect::<Vec<_>>();
        let waivers = self
            .latest_trust_envelopes_unlocked(&context.execution_space_id, "gate_waiver")?
            .into_values()
            .map(|envelope| event_projection::<GateWaiver>(&envelope))
            .collect::<StoreResult<Vec<_>>>()?
            .into_iter()
            .filter(|waiver| requirement_ids.contains(waiver.requirement_id.as_str()))
            .collect::<Vec<_>>();
        let mut next = current;
        next.phase = firm_core::WorkPhase::Closed;
        next.condition = firm_core::WorkCondition::Normal;
        next.resolution = Some(firm_core::WorkResolution::Accepted);
        next.result_summary = Some(report.summary.clone());
        next.version += 1;
        next.updated_at = updated_at.to_string();
        let actor_kind = match context.authenticated_actor.kind {
            ActorKind::Human => TeamActorKind::Operator,
            ActorKind::AgentMember => TeamActorKind::AgentMember,
            ActorKind::External => TeamActorKind::Operator,
            ActorKind::Service => TeamActorKind::Service,
        };
        let rollup_context = WorkCommandContext {
            event_id: format!("trust-accept:{}", context.idempotency_key),
            performed_by_actor: TeamActorRef {
                kind: actor_kind,
                id: context.authenticated_actor.id.clone(),
                display_name: None,
                authn_source: Some("agentfirm-trust-kernel".into()),
            },
            authority_actor: context
                .authority_actor
                .as_ref()
                .map(|authority| TeamActorRef {
                    kind: match authority.kind {
                        ActorKind::Human => TeamActorKind::Operator,
                        ActorKind::AgentMember => TeamActorKind::AgentMember,
                        ActorKind::External => TeamActorKind::Operator,
                        ActorKind::Service => TeamActorKind::Service,
                    },
                    id: authority.id.clone(),
                    display_name: None,
                    authn_source: Some("agentfirm-trust-kernel".into()),
                }),
            causation_ref: None,
            idempotency_key: context.idempotency_key.clone(),
            created_at: updated_at.to_string(),
            duplicate_ok: false,
        };
        let delegation_revisions =
            self.work_delegation_rollup_revisions_unlocked(&next, &rollup_context)?;
        let side_records = std::iter::once(serde_json::to_value(&report)?)
            .chain(
                requirements
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .chain(
                evaluations
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .chain(
                waivers
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .chain(
                delegation_revisions
                    .iter()
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()?,
            )
            .collect();
        self.commit_trust_work_acceptance_unlocked(context, request_payload, &next, side_records)
    }
}

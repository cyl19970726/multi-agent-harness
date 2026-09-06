use super::*;

impl HarnessStore {
    /// Compatibility entry for callers already holding an exact Candidate.
    pub fn accept_trust_work(
        &self,
        context: &MutationContext,
        team_id: &str,
        work_id: &str,
        report_id: &str,
        candidate_fingerprint: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        self.accept_work_submission(
            context,
            team_id,
            work_id,
            Some((report_id, candidate_fingerprint)),
            updated_at,
        )
    }

    /// Accept the unique immutable Result of this Work revision. The Store
    /// resolves it under the same lock as CAS and acceptance publication.
    pub fn accept_current_trust_work(
        &self,
        context: &MutationContext,
        team_id: &str,
        work_id: &str,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        self.accept_work_submission(context, team_id, work_id, None, updated_at)
    }

    fn accept_work_submission(
        &self,
        context: &MutationContext,
        team_id: &str,
        work_id: &str,
        exact_candidate: Option<(&str, &str)>,
        updated_at: &str,
    ) -> StoreResult<CanonicalMutationResult<Work>> {
        self.init()?;
        let _trust_lock = self.acquire_write_lock()?;
        let mut request_payload = serde_json::json!({
            "team_id": team_id, "work_id": work_id, "updated_at": updated_at,
        });
        if let Some((report_id, fingerprint)) = exact_candidate {
            request_payload["work_report_id"] = serde_json::json!(report_id);
            request_payload["candidate_fingerprint"] = serde_json::json!(fingerprint);
        }
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
                || replay.operation.event.expected_version != context.expected_version
                || replay.operation.event.transition != "accepted"
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
        let report = self.current_result_report_unlocked(&context.execution_space_id, &current)?;
        let report_id = report.id.as_str();
        if let Some((expected_report, expected_candidate)) = exact_candidate {
            if report_id != expected_report
                || report.candidate_fingerprint.as_deref() != Some(expected_candidate)
            {
                return Err(report_acceptance_error(
                    &current,
                    "acceptance does not match the current Result",
                ));
            }
        }
        let requirements = self
            .trust_gate_requirements_unlocked(&context.execution_space_id)?
            .into_values()
            .filter(|requirement| {
                requirement.work_id == work_id
                    && requirement.work_revision == report.work_revision
                    && requirement.work_report_id == report_id
            })
            .collect::<Vec<_>>();
        if report.report_only {
            let incompatible_module = self
                .latest_trust_envelopes_unlocked(
                    &context.execution_space_id,
                    "work_module_binding",
                )?
                .into_values()
                .map(|envelope| event_projection::<WorkModuleBinding>(&envelope))
                .collect::<StoreResult<Vec<_>>>()?
                .iter()
                .any(|binding| {
                    binding.work_id == work_id
                        && binding.work_revision == report.work_revision.saturating_sub(1)
                        && binding.module_id == WorkModuleId::IntegrationPlan
                });
            if report.candidate.is_some()
                || report.candidate_fingerprint.is_some()
                || incompatible_module
                || !requirements.is_empty()
            {
                return Err(report_acceptance_error(
                    &current,
                    "report-only Result is incompatible with Candidate or Module requirements",
                ));
            }
        } else {
            let fingerprint = report
                .candidate_fingerprint
                .as_deref()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    report_acceptance_error(&current, "Candidate Result has no fingerprint")
                })?;
            let candidate = report.candidate.as_ref().ok_or_else(|| {
                report_acceptance_error(&current, "Candidate Result has no Candidate")
            })?;
            if canonical_json_fingerprint(&serde_json::to_value(candidate)?) != fingerprint
                || requirements
                    .iter()
                    .any(|requirement| requirement.candidate_fingerprint != fingerprint)
            {
                return Err(report_acceptance_error(
                    &current,
                    "Candidate Result or requirement fingerprint does not match",
                ));
            }
            self.trust_gate_satisfied(
                &context.execution_space_id,
                work_id,
                report.work_revision,
                report_id,
                fingerprint,
            )?;
        }
        // This is an observation of the selected immutable submission, not a
        // new caller-supplied identity protocol. Replay compares the original request.
        request_payload["work_report_id"] = serde_json::json!(report_id);
        request_payload["candidate_fingerprint"] = serde_json::json!(report.candidate_fingerprint);
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
        let mut commit_context = context.clone();
        commit_context.request_fingerprint = Some(request_fingerprint);
        self.commit_trust_work_acceptance_unlocked(
            &commit_context,
            request_payload,
            &next,
            side_records,
        )
    }
    fn current_result_report_unlocked(
        &self,
        space: &str,
        current: &Work,
    ) -> StoreResult<WorkReport> {
        let envelopes = self.trust_operation_envelopes_unlocked()?;
        let reports = envelopes
            .iter()
            .filter(|row| {
                row.execution_space_id == space
                    && row.operation.event.aggregate_kind == "work_report"
            })
            .collect::<Vec<_>>();
        let mut selected = None;
        for envelope in &reports {
            let report = event_projection::<WorkReport>(envelope)?;
            if report.work_id != current.id
                || report.kind != WorkReportKind::Result
                || !self.result_revision_is_current_unlocked(&report, current)?
            {
                continue;
            }
            let event = &envelope.operation.event;
            let snapshots = envelope
                .operation
                .immutable_side_records
                .iter()
                .filter_map(|record| serde_json::from_value::<Work>(record.clone()).ok())
                .filter(|work| work.id == current.id)
                .collect::<Vec<_>>();
            if reports
                .iter()
                .filter(|row| row.operation.event.aggregate_id == report.id)
                .count()
                != 1
                || event.aggregate_id != report.id
                || event.expected_version != 0
                || event.resulting_version != 1
                || event.transition != "created"
                || snapshots.len() != 1
                || selected.is_some()
                || report.evidence_refs.is_empty()
            {
                return Err(report_acceptance_error(
                    current,
                    "Result source is ambiguous, mutable or missing evidence",
                ));
            }
            let snapshot = &snapshots[0];
            if snapshot.version != report.work_revision
                || snapshot.phase != firm_core::WorkPhase::Review
                || snapshot.condition != firm_core::WorkCondition::Normal
                || snapshot.accountable_team_id != current.accountable_team_id
                || snapshot.owner_member_id != current.owner_member_id
                || snapshot.owner_member_id.as_deref() != Some(report.authored_by.id.as_str())
                || event.performed_by_actor != report.authored_by
                || snapshot.result_summary.as_deref() != Some(report.summary.as_str())
            {
                return Err(report_acceptance_error(
                    current,
                    "Result is not bound to its atomic Work submission",
                ));
            }
            selected = Some(report);
        }
        selected.ok_or_else(|| {
            report_acceptance_error(current, "no unique current Result submission found")
        })
    }

    fn result_revision_is_current_unlocked(
        &self,
        report: &WorkReport,
        current: &Work,
    ) -> StoreResult<bool> {
        if report.work_revision == current.version {
            return Ok(true);
        }
        if report.work_revision > current.version {
            return Ok(false);
        }
        let mut updates = self
            .work_operations_unlocked()?
            .into_iter()
            .filter(|operation| {
                operation.work.id == current.id
                    && operation.event.resulting_version > report.work_revision
                    && operation.event.resulting_version <= current.version
            })
            .collect::<Vec<_>>();
        updates.sort_by_key(|operation| operation.event.resulting_version);
        Ok(
            updates.len() as u64 == current.version - report.work_revision
                && updates.iter().enumerate().all(|(offset, operation)| {
                    operation.event.expected_version == report.work_revision + offset as u64
                        && operation.event.resulting_version
                            == report.work_revision + offset as u64 + 1
                        && operation.event.kind == firm_core::WorkEventKind::Updated
                        && operation
                            .event
                            .payload
                            .get("reason")
                            .and_then(Value::as_str)
                            == Some("github_evidence_refresh")
                        && operation.work.phase == firm_core::WorkPhase::Review
                        && operation.work.condition == firm_core::WorkCondition::Normal
                }),
        )
    }
}

fn report_acceptance_error(work: &Work, message: &str) -> StoreError {
    trust_error(
        TrustErrorCode::ReportEvidenceMissing,
        message,
        "work",
        &work.id,
        Some(work.version),
    )
}

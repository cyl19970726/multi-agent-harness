//! Canonical application seam for Work mutations.
//!
//! Transport adapters bind identity and parse input. This service owns the
//! dispatch choice and returns the committed mutation metadata together with
//! the resulting Work projection.

use harness_application::{WorkAction, WorkActionKind, WorkApplication};
use harness_core::agentfirm_api::{
    ActorKind, CandidateKind, CandidateRef, Confidence, MemberCoordinationStatus, WorkReport,
    WorkReportKind,
};
use harness_core::Work;
use harness_store::{canonical_json_fingerprint, HarnessStore, StoreError};
use serde::Serialize;
use serde_json::Value;

use crate::agentfirm_api::{
    AuthenticatedMutation, TrustApplication, TrustCommand, TrustCommandResult,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalWorkActionKind {
    Lifecycle(WorkActionKind),
    SubmitResult,
    Report,
    Accept,
}

#[derive(Debug, Clone, Serialize)]
pub struct CanonicalWorkActionOutcome {
    pub kind: CanonicalWorkActionKind,
    /// Projection of the canonical aggregate committed by this command. A
    /// result submission commits a WorkReport; acceptance commits Work.
    pub projection: Value,
    /// Current Work after every side record/roll-up from the command.
    pub work: Work,
    pub event_id: String,
    pub store_sequence: u64,
    pub resulting_version: u64,
    pub replayed: bool,
}

#[derive(Debug, Clone)]
pub struct ResultSubmission {
    pub result_summary: String,
    pub artifact_refs: Vec<String>,
    pub check_refs: Vec<String>,
    pub github_links: Vec<harness_core::GitHubLink>,
    pub base_revision: Option<String>,
    pub candidate_revision: Option<String>,
}

pub enum CanonicalWorkCommand {
    Lifecycle {
        auth: Option<AuthenticatedMutation>,
        action: Box<WorkAction>,
    },
    SubmitResult {
        auth: AuthenticatedMutation,
        team_id: String,
        work_id: String,
        submission: ResultSubmission,
    },
    CreateReport {
        auth: AuthenticatedMutation,
        team_id: String,
        report: Box<WorkReport>,
    },
    Accept {
        auth: AuthenticatedMutation,
        team_id: String,
        work_id: String,
    },
}

pub fn execute(
    store: &HarnessStore,
    command: CanonicalWorkCommand,
) -> Result<CanonicalWorkActionOutcome, StoreError> {
    match command {
        CanonicalWorkCommand::Lifecycle {
            auth: Some(auth),
            action,
        } => execute_lifecycle(store, &auth, *action),
        CanonicalWorkCommand::Lifecycle { auth: None, action } => {
            execute_local_lifecycle(store, *action)
        }
        CanonicalWorkCommand::SubmitResult {
            auth,
            team_id,
            work_id,
            submission,
        } => submit_result(store, auth, &team_id, &work_id, submission),
        CanonicalWorkCommand::CreateReport {
            auth,
            team_id,
            report,
        } => create_report(store, auth, &team_id, *report),
        CanonicalWorkCommand::Accept {
            auth,
            team_id,
            work_id,
        } => accept(store, auth, &team_id, &work_id),
    }
}

fn execute_lifecycle(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    action: WorkAction,
) -> Result<CanonicalWorkActionOutcome, StoreError> {
    let before_work = store.work_operations()?.len();
    let before_canonical = store
        .canonical_operations_for_space(&auth.execution_space_id)?
        .len();
    let executed = WorkApplication::new(store).execute(action)?;
    let projection = serde_json::to_value(&executed.work)?;

    if let Some(operation) = store
        .canonical_operations_for_space(&auth.execution_space_id)?
        .into_iter()
        .find(|operation| operation.event.idempotency_key == auth.idempotency_key)
    {
        let authenticated_fingerprint_matches = match auth.request_fingerprint.as_deref() {
            None => true,
            Some(fingerprint) if operation.event.canonical_request_fingerprint == fingerprint => {
                true
            }
            Some(fingerprint) => store.work_events()?.into_iter().any(|event| {
                event.work_id == executed.work.id
                    && event.idempotency_key == auth.idempotency_key
                    && event.expected_version == auth.expected_version
                    && event.resulting_version == executed.work.version
                    && event
                        .causation_ref
                        .as_ref()
                        .filter(|reference| reference.kind == "agentfirm.role_action.v1")
                        .map(|reference| reference.id.as_str())
                        == Some(fingerprint)
            }),
        };
        if operation.event.aggregate_kind != "work"
            || operation.event.aggregate_id != executed.work.id
            || operation.event.performed_by_actor != auth.actor
            || operation.event.expected_version != auth.expected_version
            || operation.event.resulting_version != executed.work.version
            || operation.resulting_projection != projection
            || !authenticated_fingerprint_matches
        {
            return Err(StoreError::Conflict(
                "IDEMPOTENCY_KEY_REUSED: canonical Work outcome does not match the requested action"
                    .into(),
            ));
        }
        let current_len = store
            .canonical_operations_for_space(&auth.execution_space_id)?
            .len();
        return Ok(CanonicalWorkActionOutcome {
            kind: CanonicalWorkActionKind::Lifecycle(executed.kind),
            projection,
            work: executed.work,
            event_id: operation.event.id,
            store_sequence: operation.event.store_sequence,
            resulting_version: operation.event.resulting_version,
            replayed: current_len == before_canonical,
        });
    }

    let operations = store.work_operations()?;
    let operation = operations
        .iter()
        .rev()
        .find(|operation| {
            operation.work.id == executed.work.id
                && operation.event.idempotency_key == auth.idempotency_key
        })
        .ok_or_else(|| {
            StoreError::Conflict(
                "INVALID_STATE_TRANSITION: Work mutation committed without its operation".into(),
            )
        })?;
    if operation.event.expected_version != auth.expected_version
        || auth
            .request_fingerprint
            .as_deref()
            .is_some_and(|fingerprint| {
                operation
                    .event
                    .causation_ref
                    .as_ref()
                    .filter(|reference| reference.kind == "agentfirm.role_action.v1")
                    .map(|reference| reference.id.as_str())
                    != Some(fingerprint)
            })
    {
        return Err(conflict(
            "IDEMPOTENCY_KEY_REUSED",
            "Work operation does not match the authenticated request fingerprint or CAS",
        ));
    }
    Ok(CanonicalWorkActionOutcome {
        kind: CanonicalWorkActionKind::Lifecycle(executed.kind),
        projection,
        work: executed.work,
        event_id: operation.event.id.clone(),
        store_sequence: operations.len() as u64,
        resulting_version: operation.work.version,
        replayed: operations.len() == before_work,
    })
}

fn execute_local_lifecycle(
    store: &HarnessStore,
    action: WorkAction,
) -> Result<CanonicalWorkActionOutcome, StoreError> {
    let idempotency_key = action.context().idempotency_key.clone();
    let before = store.work_operations()?.len();
    let canonical_space_ids = store.canonical_execution_space_ids()?;
    let before_canonical = if canonical_space_ids.len() == 1 {
        store
            .canonical_operations_for_space(&canonical_space_ids[0])?
            .len()
    } else {
        0
    };
    let executed = WorkApplication::new(store).execute(action)?;
    let projection = serde_json::to_value(&executed.work)?;
    let canonical = if canonical_space_ids.len() == 1 {
        store.canonical_operations_for_space(&canonical_space_ids[0])?
    } else {
        Vec::new()
    };
    if let Some(operation) = canonical.iter().rev().find(|operation| {
        operation.event.aggregate_kind == "work"
            && operation.event.aggregate_id == executed.work.id
            && operation.event.resulting_version == executed.work.version
            && (operation.event.idempotency_key == idempotency_key
                || operation.resulting_projection == projection)
    }) {
        return Ok(CanonicalWorkActionOutcome {
            kind: CanonicalWorkActionKind::Lifecycle(executed.kind),
            projection,
            work: executed.work,
            event_id: operation.event.id.clone(),
            store_sequence: operation.event.store_sequence,
            resulting_version: operation.event.resulting_version,
            replayed: canonical.len() == before_canonical,
        });
    }
    let operations = store.work_operations()?;
    let operation = operations
        .iter()
        .rev()
        .find(|operation| {
            operation.work.id == executed.work.id
                && (operation.event.idempotency_key == idempotency_key
                    || operation.work.version == executed.work.version)
        })
        .ok_or_else(|| {
            StoreError::Conflict(
                "INVALID_STATE_TRANSITION: Work mutation committed without its operation".into(),
            )
        })?;
    Ok(CanonicalWorkActionOutcome {
        kind: CanonicalWorkActionKind::Lifecycle(executed.kind),
        projection,
        work: executed.work,
        event_id: operation.event.id.clone(),
        store_sequence: operations.len() as u64,
        resulting_version: operation.work.version,
        replayed: operations.len() == before,
    })
}

fn submit_result(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    team_id: &str,
    work_id: &str,
    submission: ResultSubmission,
) -> Result<CanonicalWorkActionOutcome, StoreError> {
    if let Some(operation) = store
        .canonical_operations_for_space(&auth.execution_space_id)?
        .into_iter()
        .find(|operation| operation.event.idempotency_key == auth.idempotency_key)
    {
        let report =
            serde_json::from_value::<WorkReport>(operation.resulting_projection).map_err(|_| {
                conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key is not a WorkReport submission",
                )
            })?;
        let expected_candidate_revision = submission
            .candidate_revision
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| {
                harness_store::canonical_work_candidate_revision(
                    &submission.result_summary,
                    &submission.artifact_refs,
                    &submission.check_refs,
                    &submission.github_links,
                )
            });
        if operation.event.aggregate_kind != "work_report"
            || report.work_id != work_id
            || report.summary != submission.result_summary
            || report.artifact_refs != submission.artifact_refs
            || report.check_refs != submission.check_refs
            || report.github_links != submission.github_links
            || report.base_revision != submission.base_revision
            || report
                .candidate
                .as_ref()
                .map(|candidate| candidate.value.as_str())
                != Some(expected_candidate_revision.as_str())
        {
            return Err(conflict(
                "IDEMPOTENCY_KEY_REUSED",
                "idempotency key is already bound to a different Work submission",
            ));
        }
        if operation.event.performed_by_actor != auth.actor {
            return Err(conflict(
                "IDEMPOTENCY_KEY_REUSED",
                "submission replay actor does not match the committed actor",
            ));
        }
        let work = current_work(store, &auth.execution_space_id, work_id)?;
        if work.accountable_team_id.as_deref() != Some(team_id)
            || work.owner_member_id.as_deref() != Some(auth.actor.id.as_str())
        {
            return Err(conflict(
                "IDEMPOTENCY_KEY_REUSED",
                "submission replay does not belong to the addressed Team",
            ));
        }
        return create_report(store, auth, team_id, report);
    }
    let current = current_work(store, &auth.execution_space_id, work_id)?;
    if current.accountable_team_id.as_deref() != Some(team_id) {
        return Err(conflict(
            "UNAUTHORIZED_ACTOR",
            "Work does not belong to the addressed Team",
        ));
    }
    if current.version != auth.expected_version {
        return Err(conflict(
            "VERSION_CONFLICT",
            "submission requires the exact current Work revision",
        ));
    }
    if auth.actor.kind != ActorKind::AgentMember
        || current.owner_member_id.as_deref() != Some(auth.actor.id.as_str())
        || !store
            .trust_member_runs(&auth.execution_space_id)?
            .into_iter()
            .any(|run| {
                run.agent_member_id == auth.actor.id
                    && run.coordination_status == MemberCoordinationStatus::Active
                    && current.active_member_run_id.as_deref() == Some(run.id.as_str())
            })
    {
        return Err(conflict(
            "UNAUTHORIZED_ACTOR",
            "submission requires the exact accountable AgentMember and active Work execution binding",
        ));
    }
    let candidate_revision = submission
        .candidate_revision
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            harness_store::canonical_work_candidate_revision(
                &submission.result_summary,
                &submission.artifact_refs,
                &submission.check_refs,
                &submission.github_links,
            )
        });
    let mut evidence_refs = submission
        .artifact_refs
        .iter()
        .chain(submission.check_refs.iter())
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .collect::<Vec<_>>();
    if evidence_refs.is_empty() {
        evidence_refs.push(format!("work-candidate:{candidate_revision}"));
    }
    let candidate = CandidateRef {
        kind: CandidateKind::GitCommit,
        value: candidate_revision,
    };
    let candidate_fingerprint = canonical_json_fingerprint(&serde_json::to_value(&candidate)?);
    let report = WorkReport {
        id: format!("work-report:{}", auth.idempotency_key),
        work_id: work_id.to_string(),
        work_revision: current.version + 1,
        report_revision: store
            .trust_work_reports(&auth.execution_space_id)?
            .into_iter()
            .filter(|report| report.work_id == work_id)
            .count() as u64
            + 1,
        kind: WorkReportKind::Result,
        authored_by: auth.actor.clone(),
        summary: submission.result_summary,
        base_revision: submission.base_revision,
        candidate: Some(candidate),
        candidate_fingerprint: Some(candidate_fingerprint),
        finding_refs: Vec::new(),
        failure_analysis_ref: None,
        artifact_refs: submission.artifact_refs,
        check_refs: submission.check_refs,
        github_links: submission.github_links,
        evidence_refs,
        known_risks: Vec::new(),
        confidence: Some(Confidence::High),
        recommended_next_action: Some("host_review".into()),
        created_at: crate::role_views_api::now(),
    };
    create_report(store, auth, team_id, report)
}

fn create_report(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    team_id: &str,
    report: WorkReport,
) -> Result<CanonicalWorkActionOutcome, StoreError> {
    let expected_work_revision = match report.kind {
        WorkReportKind::Result => auth
            .expected_version
            .checked_add(1)
            .ok_or_else(|| conflict("VERSION_CONFLICT", "Work report revision cannot overflow"))?,
        WorkReportKind::Progress | WorkReportKind::Failure => auth.expected_version,
    };
    if report.work_revision != expected_work_revision {
        return Err(conflict(
            "VERSION_CONFLICT",
            "WorkReport does not bind the exact addressed Work revision",
        ));
    }
    if let Some(operation) = store
        .canonical_operations_for_space(&auth.execution_space_id)?
        .into_iter()
        .find(|operation| operation.event.idempotency_key == auth.idempotency_key)
    {
        let committed = serde_json::from_value::<WorkReport>(
            operation.resulting_projection.clone(),
        )
        .map_err(|_| {
            conflict(
                "IDEMPOTENCY_KEY_REUSED",
                "idempotency key is not a WorkReport mutation",
            )
        })?;
        let fingerprint_matches = auth.request_fingerprint.as_deref().map_or_else(
            || {
                let mut comparable = report.clone();
                comparable.created_at = committed.created_at.clone();
                comparable.report_revision = committed.report_revision;
                comparable == committed
            },
            |fingerprint| operation.event.canonical_request_fingerprint == fingerprint,
        );
        if operation.event.aggregate_kind != "work_report"
            || operation.event.aggregate_id != report.id
            || operation.event.performed_by_actor != auth.actor
            || committed.work_id != report.work_id
            || committed.work_revision != report.work_revision
            || !fingerprint_matches
        {
            return Err(conflict(
                "IDEMPOTENCY_KEY_REUSED",
                "idempotency key is already bound to a different authenticated WorkReport request",
            ));
        }
        let work = current_work(store, &auth.execution_space_id, &report.work_id)?;
        if work.accountable_team_id.as_deref() != Some(team_id) {
            return Err(conflict(
                "IDEMPOTENCY_KEY_REUSED",
                "WorkReport replay does not belong to the addressed Team",
            ));
        }
        return Ok(CanonicalWorkActionOutcome {
            kind: if committed.kind == WorkReportKind::Result {
                CanonicalWorkActionKind::SubmitResult
            } else {
                CanonicalWorkActionKind::Report
            },
            projection: operation.resulting_projection,
            work,
            event_id: operation.event.id,
            store_sequence: operation.event.store_sequence,
            resulting_version: operation.event.resulting_version,
            replayed: true,
        });
    }
    let current = current_work(store, &auth.execution_space_id, &report.work_id)?;
    if current.accountable_team_id.as_deref() != Some(team_id) {
        return Err(conflict(
            "UNAUTHORIZED_ACTOR",
            "WorkReport does not belong to the addressed Team",
        ));
    }
    if current.version != auth.expected_version {
        return Err(conflict(
            "VERSION_CONFLICT",
            &format!(
                "WorkReport expected revision {}, current revision is {}",
                auth.expected_version, current.version
            ),
        ));
    }
    let work_id = report.work_id.clone();
    let kind = if report.kind == WorkReportKind::Result {
        CanonicalWorkActionKind::SubmitResult
    } else {
        CanonicalWorkActionKind::Report
    };
    auth.expected_version = 0;
    let result = TrustApplication::new(store).execute(
        auth.clone(),
        TrustCommand::CreateWorkReport {
            team_id: team_id.to_string(),
            report,
        },
    )?;
    outcome_from_trust(store, &auth.execution_space_id, &work_id, kind, result)
}

fn accept(
    store: &HarnessStore,
    auth: AuthenticatedMutation,
    team_id: &str,
    work_id: &str,
) -> Result<CanonicalWorkActionOutcome, StoreError> {
    if let Some(operation) = store
        .canonical_operations_for_space(&auth.execution_space_id)?
        .into_iter()
        .find(|operation| operation.event.idempotency_key == auth.idempotency_key)
    {
        let expected_resulting_version = auth.expected_version.checked_add(1).ok_or_else(|| {
            conflict(
                "VERSION_CONFLICT",
                "Work version cannot advance beyond the maximum revision",
            )
        })?;
        let accepted = serde_json::from_value::<Work>(operation.resulting_projection.clone())
            .map_err(|_| {
                conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key is not a Work acceptance",
                )
            })?;
        if operation.event.aggregate_kind != "work"
            || operation.event.aggregate_id != work_id
            || operation.event.performed_by_actor != auth.actor
            || accepted.accountable_team_id.as_deref() != Some(team_id)
            || accepted.version != expected_resulting_version
            || accepted.resolution != Some(harness_core::WorkResolution::Accepted)
        {
            return Err(conflict(
                "IDEMPOTENCY_KEY_REUSED",
                "idempotency key is already bound to a different Work action",
            ));
        }
        return Ok(CanonicalWorkActionOutcome {
            kind: CanonicalWorkActionKind::Accept,
            projection: operation.resulting_projection,
            work: accepted,
            event_id: operation.event.id,
            store_sequence: operation.event.store_sequence,
            resulting_version: operation.event.resulting_version,
            replayed: true,
        });
    }
    let current = current_work(store, &auth.execution_space_id, work_id)?;
    if current.accountable_team_id.as_deref() != Some(team_id)
        || current.version != auth.expected_version
    {
        return Err(conflict(
            "VERSION_CONFLICT",
            "accept requires the exact current Team-scoped Work revision",
        ));
    }
    let report = store
        .trust_work_reports(&auth.execution_space_id)?
        .into_iter()
        .filter(|report| {
            report.kind == WorkReportKind::Result
                && report.work_id == work_id
                && report.work_revision == current.version
        })
        .max_by_key(|report| report.report_revision)
        .ok_or_else(|| {
            conflict(
                "REPORT_EVIDENCE_MISSING",
                "accept requires the exact current result WorkReport",
            )
        })?;
    let candidate_fingerprint = report.candidate_fingerprint.clone().ok_or_else(|| {
        conflict(
            "REPORT_EVIDENCE_MISSING",
            "result WorkReport has no candidate fingerprint",
        )
    })?;
    let execution_space_id = auth.execution_space_id.clone();
    let result = TrustApplication::new(store).execute(
        auth,
        TrustCommand::AcceptWork {
            team_id: team_id.to_string(),
            work_id: work_id.to_string(),
            work_report_id: report.id,
            candidate_fingerprint,
            updated_at: crate::role_views_api::now(),
        },
    )?;
    outcome_from_trust(
        store,
        &execution_space_id,
        work_id,
        CanonicalWorkActionKind::Accept,
        result,
    )
}

fn outcome_from_trust(
    store: &HarnessStore,
    execution_space_id: &str,
    work_id: &str,
    kind: CanonicalWorkActionKind,
    result: TrustCommandResult,
) -> Result<CanonicalWorkActionOutcome, StoreError> {
    Ok(CanonicalWorkActionOutcome {
        kind,
        work: current_work(store, execution_space_id, work_id)?,
        projection: result.projection,
        event_id: result.event_id,
        store_sequence: result.store_sequence,
        resulting_version: result.resulting_version,
        replayed: result.replayed,
    })
}

pub fn current_work(
    store: &HarnessStore,
    execution_space_id: &str,
    work_id: &str,
) -> Result<Work, StoreError> {
    let canonical = store
        .canonical_operations_for_space(execution_space_id)?
        .into_iter()
        .flat_map(|operation| {
            std::iter::once(operation.resulting_projection).chain(operation.immutable_side_records)
        })
        .filter_map(|projection| serde_json::from_value::<Work>(projection).ok())
        .filter(|work| work.id == work_id)
        .max_by_key(|work| work.version);
    let compatibility = store
        .latest_works()?
        .into_iter()
        .find(|work| work.id == work_id);
    match (canonical, compatibility) {
        (Some(canonical), Some(compatibility)) if canonical.version == compatibility.version => {
            if canonical != compatibility {
                Err(conflict(
                    "IDENTITY_CONFLICT",
                    "canonical and compatibility Work projections conflict at one revision",
                ))
            } else {
                Ok(canonical)
            }
        }
        (Some(canonical), Some(compatibility)) => {
            Ok(if canonical.version > compatibility.version {
                canonical
            } else {
                compatibility
            })
        }
        (Some(work), None) | (None, Some(work)) => Ok(work),
        (None, None) => Err(conflict("INVALID_STATE_TRANSITION", "Work does not exist")),
    }
}

fn conflict(code: &str, message: &str) -> StoreError {
    StoreError::Conflict(format!("{code}: {message}"))
}

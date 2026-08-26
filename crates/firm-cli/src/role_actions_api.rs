//! Closed semantic action adapter for RoleView mutations.
//!
//! The browser may select an action and provide action-specific content, but
//! it never supplies actor identity, Host authority, CAS, idempotency, event
//! identity, or runtime identity. Those are bound from the authenticated
//! transport and the addressed Team/Work at this boundary.

use harness_application::{CreateWorkCommand, ReplaceWorkDependenciesCommand, WorkAction};
use harness_core::agentfirm_api::{
    ActorKind, ActorRef, CandidateKind, CandidateRef, Confidence, DeliveryReconcileOutcome,
    FailureAnalysis, GateEvaluation, GateRequirement, GateRequirementSource, GateVerdict,
    GateWaiver, GateWaiverState, MemberCoordinationStatus, MemberWorkspaceBinding, MutationContext,
    PrimaryCauseStatus, RetrySafety, RuntimeRecoveryResolution, WorkFinding, WorkFindingKind,
    WorkReport, WorkReportKind, WorkspaceLifecycle, WorkspaceMode, WorkspaceOwnership,
    WorkspaceSafetyProof,
};
use harness_core::{
    NodeDaemonLeaseStatus, TeamActorKind, TeamActorRef, Work, WorkCausationRef, WorkClaimMode,
    WorkCommandContext, WorkPriority,
};
use harness_store::{canonical_json_fingerprint, HarnessStore, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::agentfirm_api::AuthenticatedMutation;

mod canonical_actions;
mod member_close_adapter;
mod message_authoring_adapter;
mod operator_actions;
mod protocol;
mod runtime_recovery_adapter;
mod work_records;

use canonical_actions::*;
use message_authoring_adapter::*;
use operator_actions::*;
use protocol::*;
use work_records::*;

pub(crate) use member_close_adapter::authorize_member_close;
pub(crate) use protocol::{
    authorize_member_interrupt, provider_admission_action_binding,
    OPERATOR_PROVIDER_ADMISSION_TUPLES,
};
pub use protocol::{is_http_mutation_path, is_retired_legacy_write_path, RoleActionResult};

fn execute_work_action(
    store: &HarnessStore,
    auth: &AuthenticatedMutation,
    action: WorkAction,
) -> Result<crate::work_action_service::CanonicalWorkActionOutcome, StoreError> {
    crate::work_action_service::execute(
        store,
        crate::work_action_service::CanonicalWorkCommand::Lifecycle {
            auth: Some(auth.clone()),
            action: Box::new(action),
        },
    )
}

fn work_outcome_result(
    outcome: crate::work_action_service::CanonicalWorkActionOutcome,
) -> RoleActionResult {
    RoleActionResult {
        ok: true,
        action_protocol_version: "agentfirm.role_actions.v1",
        projection: outcome.projection,
        event_id: outcome.event_id,
        resulting_version: outcome.resulting_version,
        store_sequence: outcome.store_sequence,
        replayed: outcome.replayed,
    }
}

pub fn execute(
    store: &HarnessStore,
    mut auth: AuthenticatedMutation,
    path: &str,
    body: &[u8],
    confirmed_action: Option<&str>,
) -> Result<RoleActionResult, StoreError> {
    auth.request_fingerprint = Some(canonical_json_fingerprint(&json!({
        "protocol":"agentfirm.role_actions.v1",
        "execution_space_id":auth.execution_space_id,
        "actor":auth.actor,
        "authorized_authority_actors":auth.authorized_authority_actors,
        "path":path,
        "intent":serde_json::from_slice::<serde_json::Value>(body).unwrap_or(serde_json::Value::Null),
        "expected_version":auth.expected_version,
        "confirmation":confirmed_action,
        "project_scope":role_action_scope(store, &auth.execution_space_id, path),
    })));
    if let Some(route) = parse_canonical_route(path) {
        return execute_canonical_role_action(store, auth, route, body, confirmed_action);
    }
    if let Some((team_id, work_id)) = parse_dependencies_route(path) {
        let RoleActionIntent::ReplaceWorkDependencies {
            prerequisite_work_ids,
            reason,
        } = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                format!("invalid dependency replacement intent: {error}"),
                "work",
                work_id,
                None,
            )
        })?
        else {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match the dependency route",
                "work",
                work_id,
                None,
            ));
        };
        if reason.trim().is_empty() {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "dependency replacement requires a non-empty reason",
                "work",
                work_id,
                None,
            ));
        }
        let team = store.latest_teams()?.remove(team_id).ok_or_else(|| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                "AgentTeam does not exist",
                "team",
                team_id,
                None,
            )
        })?;
        let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
        let mut context = host_context(&auth, host_id, false);
        context.causation_ref = Some(WorkCausationRef {
            kind: "work_dependency_reason".into(),
            id: reason,
        });
        let outcome = execute_work_action(
            store,
            &auth,
            WorkAction::ReplaceDependencies(ReplaceWorkDependenciesCommand {
                accountable_team_id: team_id.to_string(),
                work_id: work_id.to_string(),
                expected_version: auth.expected_version,
                prerequisite_work_ids,
                context,
            }),
        )?;
        return Ok(work_outcome_result(outcome));
    }
    if let Some((team_id, work_id)) = parse_accept_route(path) {
        let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
            encoded_error(
                "INVALID_STATE_TRANSITION",
                format!("invalid closed role action intent: {error}"),
                "route",
                path,
                None,
            )
        })?;
        if !matches!(intent, RoleActionIntent::AcceptWork) {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match the exact route",
                "work",
                work_id,
                None,
            ));
        }
        if confirmed_action != Some("accept") {
            return Err(encoded_error(
                "CONFIRMATION_REQUIRED",
                "server confirmation header must exactly confirm accept",
                "work",
                work_id,
                None,
            ));
        }
        return Ok(work_outcome_result(crate::work_action_service::execute(
            store,
            crate::work_action_service::CanonicalWorkCommand::Accept {
                auth,
                team_id: team_id.to_string(),
                work_id: work_id.to_string(),
            },
        )?));
    }
    let route = parse_route(path).ok_or_else(|| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            "unknown AgentFirm role action route",
            "route",
            path,
            None,
        )
    })?;
    let intent = serde_json::from_slice::<RoleActionIntent>(body).map_err(|error| {
        encoded_error(
            "INVALID_STATE_TRANSITION",
            format!("invalid closed role action intent: {error}"),
            "route",
            path,
            None,
        )
    })?;
    let (_run, team) = team_for_run(store, route.team_run_id)?;
    if let (
        "submit",
        Some(work_id),
        RoleActionIntent::SubmitWork {
            result_summary,
            artifact_refs,
            check_refs,
            base_revision,
            candidate_revision,
        },
    ) = (route.operation, route.work_id, &intent)
    {
        let outcome = crate::work_action_service::execute(
            store,
            crate::work_action_service::CanonicalWorkCommand::SubmitResult {
                auth,
                team_id: team.id.clone(),
                work_id: work_id.to_string(),
                submission: crate::work_action_service::ResultSubmission {
                    result_summary: result_summary.clone(),
                    artifact_refs: artifact_refs.clone(),
                    check_refs: check_refs.clone(),
                    github_links: Vec::new(),
                    base_revision: base_revision.clone(),
                    candidate_revision: candidate_revision.clone(),
                },
            },
        )?;
        return Ok(work_outcome_result(outcome));
    }
    let outcome = match (route.operation, route.work_id, intent) {
        (
            "create",
            None,
            RoleActionIntent::CreateWork {
                work_id,
                title,
                context_markdown,
                completion_criteria_markdown,
                eligible_member_ids,
                prerequisite_work_ids,
                claim_mode,
                priority,
            },
        ) => {
            let host_id = require_host(&auth, &team.host_agent_id, "team_run", route.team_run_id)?;
            if auth.expected_version != 0 {
                return Err(encoded_error(
                    "VERSION_CONFLICT",
                    "Work create requires If-Match: 0",
                    "team_run",
                    route.team_run_id,
                    Some(0),
                ));
            }
            let context = host_context(&auth, host_id, false);
            execute_work_action(
                store,
                &auth,
                WorkAction::Create(CreateWorkCommand {
                    work_id,
                    team_run_id: route.team_run_id.to_string(),
                    accountable_team_id: team.id.clone(),
                    title,
                    context_markdown,
                    completion_criteria_markdown,
                    claim_mode,
                    eligible_member_ids,
                    prerequisite_work_ids,
                    priority,
                    initial_member_run_id: None,
                    artifact_refs: Vec::new(),
                    check_refs: Vec::new(),
                    github_links: Vec::new(),
                    expected_version: auth.expected_version,
                    context,
                }),
            )?
        }
        (operation, Some(work_id), intent) => {
            require_confirmed(operation, confirmed_action, work_id)?;
            if !matches!(
                operation,
                "assign" | "rebind" | "release" | "cancel" | "claim" | "start" | "block" | "resume"
            ) {
                return Err(encoded_error(
                    "INVALID_STATE_TRANSITION",
                    "unknown Work operation",
                    "work",
                    work_id,
                    None,
                ));
            }
            if matches!(operation, "assign" | "rebind" | "cancel") {
                require_host(&auth, &team.host_agent_id, "work", work_id)?;
            } else if operation != "release" || !is_host(&auth, &team.host_agent_id) {
                let _ = resolve_member_run(store, &auth, route.team_run_id)?;
            }
            let current = current_work(store, route.team_run_id, work_id)?;
            match (operation, intent) {
                (
                    "assign",
                    RoleActionIntent::AssignWork {
                        membership_id,
                        member_run_id,
                    },
                ) => {
                    let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
                    match (membership_id, member_run_id) {
                        (Some(membership_id), _) => execute_work_action(
                            store,
                            &auth,
                            WorkAction::AssignMembership {
                                work_id: work_id.to_string(),
                                expected_version: auth.expected_version,
                                membership_id,
                                execution_space_id: auth.execution_space_id.clone(),
                                context: host_context(&auth, host_id, false),
                            },
                        )?,
                        (None, Some(member_run_id)) => execute_work_action(
                            store,
                            &auth,
                            WorkAction::AssignRuntime {
                                work_id: work_id.to_string(),
                                expected_version: auth.expected_version,
                                member_run_id,
                                context: host_context(&auth, host_id, false),
                            },
                        )?,
                        (None, None) => {
                            return Err(encoded_error(
                                "INVALID_STATE_TRANSITION",
                                "assign_work requires membership_id (canonical TeamMembership responsibility) or the legacy member_run_id target",
                                "work",
                                work_id,
                                None,
                            ))
                        }
                    }
                }
                ("rebind", RoleActionIntent::RebindWork { member_run_id }) => {
                    let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
                    execute_work_action(
                        store,
                        &auth,
                        WorkAction::Rebind {
                            work_id: work_id.to_string(),
                            expected_version: auth.expected_version,
                            member_run_id,
                            context: host_context(&auth, host_id, false),
                        },
                    )?
                }
                ("release", RoleActionIntent::ReleaseWork)
                    if is_host(&auth, &team.host_agent_id) =>
                {
                    execute_work_action(
                        store,
                        &auth,
                        WorkAction::ReleaseHost {
                            work_id: work_id.to_string(),
                            expected_version: auth.expected_version,
                            context: host_context(&auth, &team.host_agent_id, false),
                        },
                    )?
                }
                ("release", RoleActionIntent::ReleaseWork) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    execute_work_action(
                        store,
                        &auth,
                        WorkAction::ReleaseMember {
                            work_id: work_id.to_string(),
                            expected_version: auth.expected_version,
                            member_run_id: member_run_id.clone(),
                            context: member_context(&auth, &member_run_id),
                        },
                    )?
                }
                ("cancel", RoleActionIntent::CancelWork { reason }) => {
                    let host_id = require_host(&auth, &team.host_agent_id, "work", work_id)?;
                    execute_work_action(
                        store,
                        &auth,
                        WorkAction::Cancel {
                            work_id: work_id.to_string(),
                            expected_version: auth.expected_version,
                            reason,
                            context: host_context(&auth, host_id, false),
                        },
                    )?
                }
                ("claim", RoleActionIntent::ClaimWork) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    execute_work_action(
                        store,
                        &auth,
                        WorkAction::Claim {
                            work_id: work_id.to_string(),
                            expected_version: auth.expected_version,
                            member_run_id: member_run_id.clone(),
                            context: member_context(&auth, &member_run_id),
                        },
                    )?
                }
                ("start", RoleActionIntent::StartWork) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    execute_work_action(
                        store,
                        &auth,
                        WorkAction::Start {
                            work_id: work_id.to_string(),
                            expected_version: auth.expected_version,
                            member_run_id: member_run_id.clone(),
                            context: member_context(&auth, &member_run_id),
                        },
                    )?
                }
                ("block", RoleActionIntent::BlockWork { reason }) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    execute_work_action(
                        store,
                        &auth,
                        WorkAction::BlockMember {
                            work_id: work_id.to_string(),
                            expected_version: auth.expected_version,
                            member_run_id: member_run_id.clone(),
                            reason,
                            context: member_context(&auth, &member_run_id),
                        },
                    )?
                }
                ("resume", RoleActionIntent::UnblockWork { resolution }) => {
                    let member_run_id = resolve_member_run(store, &auth, route.team_run_id)?;
                    execute_work_action(
                        store,
                        &auth,
                        WorkAction::ResumeMember {
                            work_id: work_id.to_string(),
                            expected_version: auth.expected_version,
                            member_run_id: member_run_id.clone(),
                            resolution,
                            context: member_context(&auth, &member_run_id),
                        },
                    )?
                }
                _ => {
                    return Err(encoded_error(
                        "INVALID_STATE_TRANSITION",
                        "semantic action does not match the exact route",
                        "work",
                        work_id,
                        Some(current.version),
                    ))
                }
            }
        }
        _ => {
            return Err(encoded_error(
                "INVALID_STATE_TRANSITION",
                "semantic action does not match the exact route",
                "route",
                path,
                None,
            ))
        }
    };
    Ok(work_outcome_result(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn operator_auth(key: &str, fingerprint: &str) -> AuthenticatedMutation {
        AuthenticatedMutation {
            execution_space_id: "space-test".into(),
            actor: ActorRef {
                kind: ActorKind::Service,
                id: "node-test".into(),
            },
            authorized_authority_actors: Vec::new(),
            idempotency_key: key.into(),
            expected_version: 1,
            request_fingerprint: Some(fingerprint.into()),
        }
    }

    fn operator_result(event_id: &str) -> RoleActionResult {
        RoleActionResult {
            ok: true,
            action_protocol_version: "agentfirm.role_actions.v1",
            projection: json!({"effect":"complete"}),
            event_id: event_id.into(),
            resulting_version: 1,
            store_sequence: 1,
            replayed: false,
        }
    }

    #[test]
    fn operator_journal_replays_completed_effect_and_fences_uncertain_restart() {
        let root = std::env::temp_dir().join(format!(
            "firm-operator-journal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let auth = operator_auth("completed", "fingerprint-completed");
        let first = execute_receipted_operator_action(&root, "node-test", &auth, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(operator_result("effect-1"))
        })
        .unwrap();
        assert!(!first.replayed);
        let replay = execute_receipted_operator_action(&root, "node-test", &auth, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(operator_result("must-not-run"))
        })
        .unwrap();
        assert!(replay.replayed);
        assert_eq!(replay.event_id, "effect-1");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        let changed_scope = operator_auth("completed", "fingerprint-changed-tuple-or-scope");
        let error = execute_receipted_operator_action(&root, "node-test", &changed_scope, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(operator_result("must-not-run-for-changed-scope"))
        })
        .expect_err("same key with a changed tuple or scope must fail closed");
        assert!(error.to_string().contains("fingerprint"), "{error}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let uncertain = operator_auth("uncertain", "fingerprint-uncertain");
        let (path, _, fingerprint) =
            operator_receipt_paths(&root, "node-test", &uncertain).unwrap();
        crate::execution_space::atomic_write_bytes(
            &path,
            &serde_json::to_vec(&OperatorActionReceipt {
                request_fingerprint: fingerprint,
                state: OperatorActionJournalState::InFlight,
                projection: None,
                event_id: None,
                resulting_version: None,
                store_sequence: None,
                recovery_detail: Some("fault after effect before receipt".into()),
            })
            .unwrap(),
        )
        .unwrap();
        let error = execute_receipted_operator_action(&root, "node-test", &uncertain, || {
            calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(operator_result("must-not-repeat"))
        })
        .expect_err("uncertain restart must require recovery");
        assert!(error.to_string().contains("RECOVERY_REQUIRED"), "{error}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);

        let torn = operator_auth("torn", "fingerprint-torn");
        let (torn_path, _, _) = operator_receipt_paths(&root, "node-test", &torn).unwrap();
        std::fs::write(&torn_path, b"{\"state\":").unwrap();
        let error = replay_receipted_operator_action(&root, "node-test", &torn)
            .expect_err("torn journal must require recovery");
        assert!(error.to_string().contains("RECOVERY_REQUIRED"), "{error}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn route_inventory_is_closed() {
        assert!(is_http_mutation_path("/v1/agentfirm/team-runs/run-1/works"));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/works/work-1/start"
        ));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/teams/team-1/works/work-1/accept"
        ));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/teams/team-1/works/work-1/dependencies"
        ));
        assert!(!is_http_mutation_path(
            "/v1/agentfirm/nodes/node-1/work-deliveries/delivery-1/reconcile"
        ));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/messages/send"
        ));
        assert!(is_http_mutation_path(
            "/v1/agentfirm/teams/team-1/works/work-1/request-changes"
        ));
        assert!(!is_http_mutation_path(
            "/v1/agentfirm/team-runs/run-1/works/work-1/browser-invented"
        ));
        for retired in [
            "/v1/team-runs/run-1/works",
            "/v1/team-runs/run-1/works/work-1/assign",
            "/v1/team-runs/run-1/works/work-1/review",
            "/v1/team-runs/run-1/works/work-1/accept",
            "/v1/work-delegations",
            "/v1/work-delegations/delegation-1",
            "/v1/work-delegations/delegation-1/accept",
        ] {
            assert!(is_retired_legacy_write_path(retired), "{retired}");
        }
        for canonical in [
            "/v1/agentfirm/team-runs/run-1/works",
            "/v1/agentfirm/team-runs/run-1/works/work-1/start",
            "/v1/agentfirm/teams/team-1/works/work-1/accept",
        ] {
            assert!(!is_retired_legacy_write_path(canonical), "{canonical}");
        }
    }
}

use super::{Work, WorkCondition, WorkPhase, WorkResolution};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Canonical payload for a versioned replacement of one Work's V1 hard
/// `depends_on` edges. Both lists are sorted so equivalent commands produce
/// identical event payloads and fingerprints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkDependenciesChangedPayload {
    pub previous_prerequisite_work_ids: Vec<String>,
    pub prerequisite_work_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkDependencyError {
    #[error("closed Work {work_id} is immutable")]
    TerminalWork { work_id: String },
    #[error("dependency {work_id} is repeated")]
    DuplicateDependency { work_id: String },
    #[error("Work {work_id} cannot depend on itself")]
    SelfDependency { work_id: String },
    #[error("prerequisite Work {work_id} does not exist")]
    MissingPrerequisite { work_id: String },
    #[error("prerequisite Work {work_id} is outside the accountable Team scope")]
    ScopeMismatch { work_id: String },
    #[error("dependency change would create a cycle: {path:?}")]
    Cycle { path: Vec<String> },
}

/// A stable explanation of why a Work cannot be newly claimed. Failed and
/// cancelled prerequisites are reconciliation reasons, not automatic
/// downstream terminal transitions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkReadinessReason {
    WorkNotOpen {
        phase: WorkPhase,
    },
    WorkConditionNotNormal {
        condition: WorkCondition,
    },
    PrerequisiteMissing {
        work_id: String,
    },
    PrerequisitePending {
        work_id: String,
        phase: WorkPhase,
        condition: WorkCondition,
    },
    PrerequisiteFailed {
        work_id: String,
    },
    PrerequisiteCancelled {
        work_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkReadiness {
    pub ready: bool,
    pub reasons: Vec<WorkReadinessReason>,
}

/// Validate and canonicalize a complete dependency-set replacement.
///
/// `all_works` is the authoritative scope snapshot read under the Store's
/// mutation fence. The kernel is deliberately pure: locking, CAS, event
/// append, and projection persistence remain Store responsibilities.
pub fn prepare_dependency_change(
    work: &Work,
    prerequisite_work_ids: Vec<String>,
    all_works: &[Work],
) -> Result<WorkDependenciesChangedPayload, WorkDependencyError> {
    if work.is_terminal() {
        return Err(WorkDependencyError::TerminalWork {
            work_id: work.id.clone(),
        });
    }

    let mut proposed = prerequisite_work_ids;
    let mut seen = BTreeSet::new();
    for id in &proposed {
        if !seen.insert(id.clone()) {
            return Err(WorkDependencyError::DuplicateDependency {
                work_id: id.clone(),
            });
        }
        if id == &work.id {
            return Err(WorkDependencyError::SelfDependency {
                work_id: work.id.clone(),
            });
        }
    }
    proposed.sort();

    let by_id: BTreeMap<&str, &Work> = all_works
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect();
    for id in &proposed {
        let prerequisite =
            by_id
                .get(id.as_str())
                .ok_or_else(|| WorkDependencyError::MissingPrerequisite {
                    work_id: id.clone(),
                })?;
        if work.accountable_team_id.is_none()
            || prerequisite.accountable_team_id != work.accountable_team_id
        {
            return Err(WorkDependencyError::ScopeMismatch {
                work_id: id.clone(),
            });
        }
    }

    for prerequisite_id in &proposed {
        let mut visiting = BTreeSet::new();
        let mut path = vec![work.id.clone()];
        if find_path_to_target(
            prerequisite_id,
            &work.id,
            work,
            &proposed,
            &by_id,
            &mut visiting,
            &mut path,
        ) {
            return Err(WorkDependencyError::Cycle { path });
        }
    }

    let mut previous = work.prerequisite_work_ids.clone();
    previous.sort();
    previous.dedup();
    Ok(WorkDependenciesChangedPayload {
        previous_prerequisite_work_ids: previous,
        prerequisite_work_ids: proposed,
    })
}

fn find_path_to_target(
    current_id: &str,
    target_id: &str,
    changed_work: &Work,
    proposed: &[String],
    by_id: &BTreeMap<&str, &Work>,
    visiting: &mut BTreeSet<String>,
    path: &mut Vec<String>,
) -> bool {
    path.push(current_id.to_owned());
    if current_id == target_id {
        return true;
    }
    if !visiting.insert(current_id.to_owned()) {
        path.pop();
        return false;
    }

    let dependencies = if current_id == changed_work.id {
        proposed
    } else {
        by_id
            .get(current_id)
            .map(|work| work.prerequisite_work_ids.as_slice())
            .unwrap_or_default()
    };
    let mut dependencies = dependencies.to_vec();
    dependencies.sort();
    for dependency_id in dependencies {
        if find_path_to_target(
            &dependency_id,
            target_id,
            changed_work,
            proposed,
            by_id,
            visiting,
            path,
        ) {
            return true;
        }
    }
    visiting.remove(current_id);
    path.pop();
    false
}

pub fn work_readiness(work: &Work, all_works: &[Work]) -> WorkReadiness {
    let mut reasons = Vec::new();
    if work.phase != WorkPhase::Open {
        reasons.push(WorkReadinessReason::WorkNotOpen { phase: work.phase });
    }
    if work.condition != WorkCondition::Normal {
        reasons.push(WorkReadinessReason::WorkConditionNotNormal {
            condition: work.condition,
        });
    }

    let by_id: BTreeMap<&str, &Work> = all_works
        .iter()
        .map(|candidate| (candidate.id.as_str(), candidate))
        .collect();
    let mut prerequisite_ids = work.prerequisite_work_ids.clone();
    prerequisite_ids.sort();
    prerequisite_ids.dedup();
    for id in prerequisite_ids {
        let Some(prerequisite) = by_id.get(id.as_str()) else {
            reasons.push(WorkReadinessReason::PrerequisiteMissing { work_id: id });
            continue;
        };
        match (prerequisite.phase, prerequisite.resolution) {
            (WorkPhase::Closed, Some(WorkResolution::Accepted)) => {}
            (WorkPhase::Closed, Some(WorkResolution::Failed)) => {
                reasons.push(WorkReadinessReason::PrerequisiteFailed { work_id: id });
            }
            (WorkPhase::Closed, Some(WorkResolution::Cancelled)) => {
                reasons.push(WorkReadinessReason::PrerequisiteCancelled { work_id: id });
            }
            _ => reasons.push(WorkReadinessReason::PrerequisitePending {
                work_id: id,
                phase: prerequisite.phase,
                condition: prerequisite.condition,
            }),
        }
    }
    reasons.sort();
    WorkReadiness {
        ready: reasons.is_empty(),
        reasons,
    }
}

/// Derive reverse edges. Successors are never writable authority.
pub fn derive_work_successor_ids(work_id: &str, all_works: &[Work]) -> Vec<String> {
    let mut successors: Vec<_> = all_works
        .iter()
        .filter(|work| {
            work.prerequisite_work_ids
                .iter()
                .any(|prerequisite_id| prerequisite_id == work_id)
        })
        .map(|work| work.id.clone())
        .collect();
    successors.sort();
    successors.dedup();
    successors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TeamActorKind, TeamActorRef, WorkClaimMode, WorkPriority};

    fn work(id: &str, prerequisites: &[&str]) -> Work {
        Work {
            id: id.into(),
            team_run_id: "run-1".into(),
            accountable_team_id: Some("team-1".into()),
            assignee_membership_id: None,
            legacy_parent_work_id: None,
            title: id.into(),
            context_markdown: String::new(),
            completion_criteria_markdown: "done".into(),
            phase: WorkPhase::Open,
            condition: WorkCondition::Normal,
            resolution: None,
            owner_member_id: None,
            active_member_run_id: None,
            claim_mode: WorkClaimMode::TeamClaim,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: prerequisites.iter().map(|id| (*id).into()).collect(),
            priority: WorkPriority::Normal,
            created_by_actor: TeamActorRef {
                kind: TeamActorKind::Host,
                id: "host-1".into(),
                display_name: None,
                authn_source: None,
            },
            created_by_member_id: None,
            result_summary: None,
            blocker_reason: None,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            version: 1,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        }
    }

    #[test]
    fn dependency_change_rejects_self_missing_scope_and_transitive_cycles() {
        let a = work("a", &[]);
        let b = work("b", &["a"]);
        let c = work("c", &["b"]);
        let graph = vec![a.clone(), b, c.clone()];

        assert_eq!(
            prepare_dependency_change(&a, vec!["a".into()], &graph),
            Err(WorkDependencyError::SelfDependency {
                work_id: "a".into()
            })
        );
        assert_eq!(
            prepare_dependency_change(&a, vec!["missing".into()], &graph),
            Err(WorkDependencyError::MissingPrerequisite {
                work_id: "missing".into()
            })
        );

        let mut other_team = c.clone();
        other_team.id = "other".into();
        other_team.accountable_team_id = Some("team-2".into());
        let mut scoped_graph = graph.clone();
        scoped_graph.push(other_team);
        assert_eq!(
            prepare_dependency_change(&a, vec!["other".into()], &scoped_graph),
            Err(WorkDependencyError::ScopeMismatch {
                work_id: "other".into()
            })
        );

        let error = prepare_dependency_change(&a, vec!["c".into()], &graph)
            .expect_err("a -> c closes a -> c -> b -> a");
        assert_eq!(
            error,
            WorkDependencyError::Cycle {
                path: vec!["a".into(), "c".into(), "b".into(), "a".into()]
            }
        );
    }

    #[test]
    fn fan_in_payload_and_fan_out_successors_are_canonical() {
        let a = work("a", &[]);
        let b = work("b", &[]);
        let dependent = work("dependent", &["a"]);
        let other = work("other", &["a"]);
        let graph = vec![other, b, dependent.clone(), a];

        let payload = prepare_dependency_change(&dependent, vec!["b".into(), "a".into()], &graph)
            .expect("valid fan-in");
        assert_eq!(payload.previous_prerequisite_work_ids, ["a"]);
        assert_eq!(payload.prerequisite_work_ids, ["a", "b"]);
        assert_eq!(
            derive_work_successor_ids("a", &graph),
            ["dependent", "other"]
        );
    }

    #[test]
    fn readiness_distinguishes_pending_failed_cancelled_and_missing() {
        let mut accepted = work("accepted", &[]);
        accepted.phase = WorkPhase::Closed;
        accepted.resolution = Some(WorkResolution::Accepted);
        let mut failed = work("failed", &[]);
        failed.phase = WorkPhase::Closed;
        failed.resolution = Some(WorkResolution::Failed);
        let mut cancelled = work("cancelled", &[]);
        cancelled.phase = WorkPhase::Closed;
        cancelled.resolution = Some(WorkResolution::Cancelled);
        let pending = work("pending", &[]);
        let target = work(
            "target",
            &["missing", "cancelled", "accepted", "pending", "failed"],
        );

        let readiness = work_readiness(&target, &[accepted, failed, cancelled, pending]);
        assert!(!readiness.ready);
        assert_eq!(readiness.reasons.len(), 4);
        assert!(readiness
            .reasons
            .contains(&WorkReadinessReason::PrerequisiteCancelled {
                work_id: "cancelled".into()
            }));
        assert!(readiness
            .reasons
            .contains(&WorkReadinessReason::PrerequisiteFailed {
                work_id: "failed".into()
            }));
        assert!(readiness
            .reasons
            .contains(&WorkReadinessReason::PrerequisiteMissing {
                work_id: "missing".into()
            }));
    }

    #[test]
    fn terminal_work_dependency_set_is_immutable() {
        let prerequisite = work("prerequisite", &[]);
        let mut terminal = work("terminal", &[]);
        terminal.phase = WorkPhase::Closed;
        terminal.resolution = Some(WorkResolution::Accepted);
        assert_eq!(
            prepare_dependency_change(&terminal, vec!["prerequisite".into()], &[prerequisite]),
            Err(WorkDependencyError::TerminalWork {
                work_id: "terminal".into()
            })
        );
    }
}

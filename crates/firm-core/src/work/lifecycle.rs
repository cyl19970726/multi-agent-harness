use super::{Work, WorkCondition, WorkEventKind, WorkPhase, WorkResolution};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum WorkLifecycleError {
    #[error("closed Work {work_id} is immutable")]
    TerminalWork { work_id: String },
    #[error("{operation:?} is not a valid Work transition")]
    InvalidTransition { operation: WorkEventKind },
}

/// Pure lifecycle check shared by every application adapter and Store write.
/// Dependency changes are permitted only while the Work is non-terminal; the
/// dependency kernel separately validates graph semantics.
pub fn validate_work_transition(
    current: &Work,
    next: &Work,
    operation: WorkEventKind,
) -> Result<(), WorkLifecycleError> {
    if current.is_terminal() {
        return Err(WorkLifecycleError::TerminalWork {
            work_id: current.id.clone(),
        });
    }

    let before = (current.phase, current.condition, current.resolution);
    let after = (next.phase, next.condition, next.resolution);
    let valid = match operation {
        WorkEventKind::Started => {
            before == (WorkPhase::Open, WorkCondition::Normal, None)
                && after == (WorkPhase::Active, WorkCondition::Normal, None)
        }
        WorkEventKind::Blocked => {
            before == (WorkPhase::Active, WorkCondition::Normal, None)
                && after == (WorkPhase::Active, WorkCondition::Blocked, None)
        }
        WorkEventKind::Resumed => {
            before == (WorkPhase::Active, WorkCondition::Blocked, None)
                && after == (WorkPhase::Active, WorkCondition::Normal, None)
        }
        WorkEventKind::Submitted => {
            before == (WorkPhase::Active, WorkCondition::Normal, None)
                && after == (WorkPhase::Review, WorkCondition::Normal, None)
        }
        WorkEventKind::ChangesRequested => {
            before == (WorkPhase::Review, WorkCondition::Normal, None)
                && after == (WorkPhase::Open, WorkCondition::Normal, None)
        }
        WorkEventKind::Accepted => {
            before == (WorkPhase::Review, WorkCondition::Normal, None)
                && after
                    == (
                        WorkPhase::Closed,
                        WorkCondition::Normal,
                        Some(WorkResolution::Accepted),
                    )
        }
        WorkEventKind::Cancelled => {
            after
                == (
                    WorkPhase::Closed,
                    WorkCondition::Normal,
                    Some(WorkResolution::Cancelled),
                )
        }
        WorkEventKind::Failed => {
            after
                == (
                    WorkPhase::Closed,
                    WorkCondition::Normal,
                    Some(WorkResolution::Failed),
                )
        }
        WorkEventKind::DependenciesChanged
        | WorkEventKind::Assigned
        | WorkEventKind::Claimed
        | WorkEventKind::Released
        | WorkEventKind::Updated
        | WorkEventKind::Rebound
        | WorkEventKind::ExecutionRetargeted => before == after,
        // A lost execution returns an open or started Work to the dispatchable
        // state; the responsibility fields are untouched by this rule.
        WorkEventKind::ExecutionRecovered => {
            matches!(before.0, WorkPhase::Open | WorkPhase::Active)
                && before.2.is_none()
                && after == (WorkPhase::Open, WorkCondition::Normal, None)
        }
        WorkEventKind::Created => false,
    };

    if valid {
        Ok(())
    } else {
        Err(WorkLifecycleError::InvalidTransition { operation })
    }
}

/// Closed Works are never reopened. This helper makes the terminal rule
/// explicit for callers that do not yet construct a full proposed projection.
pub fn ensure_work_mutable(work: &Work) -> Result<(), WorkLifecycleError> {
    if work.is_terminal() {
        Err(WorkLifecycleError::TerminalWork {
            work_id: work.id.clone(),
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TeamActorKind, TeamActorRef, WorkClaimMode, WorkPriority};

    fn work() -> Work {
        Work {
            id: "work-1".into(),
            team_run_id: "run-1".into(),
            accountable_team_id: Some("team-1".into()),
            assignee_membership_id: None,
            legacy_containment_ref: None,
            title: "Work".into(),
            context_markdown: String::new(),
            completion_criteria_markdown: "done".into(),
            phase: WorkPhase::Open,
            condition: WorkCondition::Normal,
            resolution: None,
            owner_member_id: None,
            active_member_run_id: None,
            claim_mode: WorkClaimMode::TeamClaim,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
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
    fn lifecycle_accepts_normal_path_and_rejects_reopen() {
        let open = work();
        let mut active = open.clone();
        active.phase = WorkPhase::Active;
        assert_eq!(
            validate_work_transition(&open, &active, WorkEventKind::Started),
            Ok(())
        );

        let mut review = active.clone();
        review.phase = WorkPhase::Review;
        assert_eq!(
            validate_work_transition(&active, &review, WorkEventKind::Submitted),
            Ok(())
        );

        let mut changes_requested = review.clone();
        changes_requested.phase = WorkPhase::Open;
        assert_eq!(
            validate_work_transition(&review, &changes_requested, WorkEventKind::ChangesRequested,),
            Ok(())
        );

        let mut closed = review.clone();
        closed.phase = WorkPhase::Closed;
        closed.resolution = Some(WorkResolution::Accepted);
        assert_eq!(
            validate_work_transition(&review, &closed, WorkEventKind::Accepted),
            Ok(())
        );
        assert_eq!(
            validate_work_transition(&closed, &open, WorkEventKind::Updated),
            Err(WorkLifecycleError::TerminalWork {
                work_id: "work-1".into()
            })
        );
    }

    #[test]
    fn failed_is_terminal_without_accepting_downstream_work() {
        let active = {
            let mut value = work();
            value.phase = WorkPhase::Active;
            value
        };
        let mut failed = active.clone();
        failed.phase = WorkPhase::Closed;
        failed.resolution = Some(WorkResolution::Failed);
        assert_eq!(
            validate_work_transition(&active, &failed, WorkEventKind::Failed),
            Ok(())
        );
        assert!(failed.is_terminal());
        assert!(!failed.is_accepted());
    }
}

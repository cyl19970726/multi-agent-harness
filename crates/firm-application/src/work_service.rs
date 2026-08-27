//! Application-owned Work use cases.
//!
//! Transport adapters authenticate actors and build [`WorkCommandContext`].
//! The Store owns atomic persistence and the Core owns lifecycle/DAG rules.
//! This module is the single composition seam; it deliberately contains no
//! second state machine.

use firm_core::{
    CurrentWorkDraft, GitHubLink, TeamActorRef, Work, WorkClaimMode, WorkCommandContext,
    WorkPriority,
};
use serde::Serialize;

/// Persistence port required by Work application use cases. Implementations
/// own locking, CAS, append-only operations and projection storage.
pub trait WorkPersistence {
    type Error;

    fn invalid_command(&self, message: String) -> Self::Error;
    fn insert_work(&self, work: Work, context: WorkCommandContext) -> Result<Work, Self::Error>;
    fn load_work(&self, work_id: &str) -> Result<Option<Work>, Self::Error>;
    fn replace_work_dependencies(
        &self,
        work_id: &str,
        expected_version: u64,
        prerequisite_work_ids: Vec<String>,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn assign_work_to_membership(
        &self,
        work_id: &str,
        expected_version: u64,
        membership_id: &str,
        execution_space_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn release_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn release_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn claim_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn start_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn block_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn block_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        reason: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn resume_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        resolution: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn resume_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        resolution: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn request_work_changes(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
    fn cancel_work(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> Result<Work, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateWorkCommand {
    pub work_id: String,
    pub team_run_id: String,
    pub accountable_team_id: String,
    pub title: String,
    pub context_markdown: String,
    pub completion_criteria_markdown: String,
    pub claim_mode: WorkClaimMode,
    pub eligible_member_ids: Vec<String>,
    pub prerequisite_work_ids: Vec<String>,
    pub priority: WorkPriority,
    pub artifact_refs: Vec<String>,
    pub check_refs: Vec<String>,
    pub github_links: Vec<GitHubLink>,
    pub expected_version: u64,
    pub context: WorkCommandContext,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceWorkDependenciesCommand {
    pub accountable_team_id: String,
    pub work_id: String,
    pub expected_version: u64,
    pub prerequisite_work_ids: Vec<String>,
    pub context: WorkCommandContext,
}

/// Transport-neutral canonical Work mutation selected by an adapter after it
/// has authenticated the actor and built the exact command context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkActionKind {
    Create,
    ReplaceDependencies,
    AssignMembership,
    ReleaseHost,
    ReleaseMember,
    Claim,
    Start,
    BlockHost,
    BlockMember,
    ResumeHost,
    ResumeMember,
    RequestChanges,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkAction {
    Create(CreateWorkCommand),
    ReplaceDependencies(ReplaceWorkDependenciesCommand),
    AssignMembership {
        work_id: String,
        expected_version: u64,
        membership_id: String,
        execution_space_id: String,
        context: WorkCommandContext,
    },
    ReleaseHost {
        work_id: String,
        expected_version: u64,
        context: WorkCommandContext,
    },
    ReleaseMember {
        work_id: String,
        expected_version: u64,
        member_run_id: String,
        context: WorkCommandContext,
    },
    Claim {
        work_id: String,
        expected_version: u64,
        member_run_id: String,
        context: WorkCommandContext,
    },
    Start {
        work_id: String,
        expected_version: u64,
        member_run_id: String,
        context: WorkCommandContext,
    },
    BlockHost {
        work_id: String,
        expected_version: u64,
        reason: String,
        context: WorkCommandContext,
    },
    BlockMember {
        work_id: String,
        expected_version: u64,
        member_run_id: String,
        reason: String,
        context: WorkCommandContext,
    },
    ResumeHost {
        work_id: String,
        expected_version: u64,
        resolution: String,
        context: WorkCommandContext,
    },
    ResumeMember {
        work_id: String,
        expected_version: u64,
        member_run_id: String,
        resolution: String,
        context: WorkCommandContext,
    },
    RequestChanges {
        work_id: String,
        expected_version: u64,
        reason: String,
        context: WorkCommandContext,
    },
    Cancel {
        work_id: String,
        expected_version: u64,
        reason: String,
        context: WorkCommandContext,
    },
}

impl WorkAction {
    pub fn kind(&self) -> WorkActionKind {
        match self {
            Self::Create(_) => WorkActionKind::Create,
            Self::ReplaceDependencies(_) => WorkActionKind::ReplaceDependencies,
            Self::AssignMembership { .. } => WorkActionKind::AssignMembership,
            Self::ReleaseHost { .. } => WorkActionKind::ReleaseHost,
            Self::ReleaseMember { .. } => WorkActionKind::ReleaseMember,
            Self::Claim { .. } => WorkActionKind::Claim,
            Self::Start { .. } => WorkActionKind::Start,
            Self::BlockHost { .. } => WorkActionKind::BlockHost,
            Self::BlockMember { .. } => WorkActionKind::BlockMember,
            Self::ResumeHost { .. } => WorkActionKind::ResumeHost,
            Self::ResumeMember { .. } => WorkActionKind::ResumeMember,
            Self::RequestChanges { .. } => WorkActionKind::RequestChanges,
            Self::Cancel { .. } => WorkActionKind::Cancel,
        }
    }

    pub fn context(&self) -> &WorkCommandContext {
        match self {
            Self::Create(command) => &command.context,
            Self::ReplaceDependencies(command) => &command.context,
            Self::AssignMembership { context, .. }
            | Self::ReleaseHost { context, .. }
            | Self::ReleaseMember { context, .. }
            | Self::Claim { context, .. }
            | Self::Start { context, .. }
            | Self::BlockHost { context, .. }
            | Self::BlockMember { context, .. }
            | Self::ResumeHost { context, .. }
            | Self::ResumeMember { context, .. }
            | Self::RequestChanges { context, .. }
            | Self::Cancel { context, .. } => context,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkActionOutcome {
    pub kind: WorkActionKind,
    pub work: Work,
}

/// Thin application composition over the authoritative Core + Store pair.
pub struct WorkApplication<'a, P: WorkPersistence + ?Sized> {
    port: &'a P,
}

impl<'a, P: WorkPersistence + ?Sized> WorkApplication<'a, P> {
    pub fn new(port: &'a P) -> Self {
        Self { port }
    }

    /// The one application dispatch consumed by CLI and HTTP adapters.
    pub fn execute(&self, action: WorkAction) -> Result<WorkActionOutcome, P::Error> {
        let kind = action.kind();
        let work = match action {
            WorkAction::Create(command) => self.create(command)?,
            WorkAction::ReplaceDependencies(command) => self.replace_dependencies(command)?,
            WorkAction::AssignMembership {
                work_id,
                expected_version,
                membership_id,
                execution_space_id,
                context,
            } => self.assign_membership(
                &work_id,
                expected_version,
                &membership_id,
                &execution_space_id,
                context,
            )?,
            WorkAction::ReleaseHost {
                work_id,
                expected_version,
                context,
            } => self.release_as_host(&work_id, expected_version, context)?,
            WorkAction::ReleaseMember {
                work_id,
                expected_version,
                member_run_id,
                context,
            } => self.release_as_member(&work_id, expected_version, &member_run_id, context)?,
            WorkAction::Claim {
                work_id,
                expected_version,
                member_run_id,
                context,
            } => self.claim(&work_id, expected_version, &member_run_id, context)?,
            WorkAction::Start {
                work_id,
                expected_version,
                member_run_id,
                context,
            } => self.start(&work_id, expected_version, &member_run_id, context)?,
            WorkAction::BlockHost {
                work_id,
                expected_version,
                reason,
                context,
            } => self.block_as_host(&work_id, expected_version, &reason, context)?,
            WorkAction::BlockMember {
                work_id,
                expected_version,
                member_run_id,
                reason,
                context,
            } => {
                self.block_as_member(&work_id, expected_version, &member_run_id, &reason, context)?
            }
            WorkAction::ResumeHost {
                work_id,
                expected_version,
                resolution,
                context,
            } => self.resume_as_host(&work_id, expected_version, &resolution, context)?,
            WorkAction::ResumeMember {
                work_id,
                expected_version,
                member_run_id,
                resolution,
                context,
            } => self.resume_as_member(
                &work_id,
                expected_version,
                &member_run_id,
                &resolution,
                context,
            )?,
            WorkAction::RequestChanges {
                work_id,
                expected_version,
                reason,
                context,
            } => self.request_changes(&work_id, expected_version, &reason, context)?,
            WorkAction::Cancel {
                work_id,
                expected_version,
                reason,
                context,
            } => self.cancel(&work_id, expected_version, &reason, context)?,
        };
        Ok(WorkActionOutcome { kind, work })
    }

    pub fn create(&self, command: CreateWorkCommand) -> Result<Work, P::Error> {
        if command.expected_version != 0 {
            return Err(self.port.invalid_command(
                "WORK_VERSION_CONFLICT: Work creation requires expected version 0".into(),
            ));
        }
        let mut draft = CurrentWorkDraft::new(
            command.work_id,
            command.team_run_id,
            command.accountable_team_id,
            command.title,
            command.context_markdown,
            command.completion_criteria_markdown,
            command.claim_mode,
            command.priority,
            command.context.performed_by_actor.clone(),
            command.context.created_at.clone(),
        );
        draft.eligible_member_ids = command.eligible_member_ids;
        draft.prerequisite_work_ids = command.prerequisite_work_ids;
        draft.artifact_refs = command.artifact_refs;
        draft.check_refs = command.check_refs;
        draft.github_links = command.github_links;
        self.port.insert_work(draft.into_work(), command.context)
    }

    pub fn replace_dependencies(
        &self,
        command: ReplaceWorkDependenciesCommand,
    ) -> Result<Work, P::Error> {
        let current = self.port.load_work(&command.work_id)?.ok_or_else(|| {
            self.port
                .invalid_command(format!("work not found: {}", command.work_id))
        })?;
        if current.accountable_team_id.as_deref() != Some(&command.accountable_team_id) {
            return Err(self.port.invalid_command(format!(
                "WORK_TEAM_SCOPE_MISMATCH: Work {} is not accountable to Team {}",
                command.work_id, command.accountable_team_id
            )));
        }
        self.port.replace_work_dependencies(
            &command.work_id,
            command.expected_version,
            command.prerequisite_work_ids,
            command.context,
        )
    }

    pub fn assign_membership(
        &self,
        work_id: &str,
        expected_version: u64,
        membership_id: &str,
        execution_space_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port.assign_work_to_membership(
            work_id,
            expected_version,
            membership_id,
            execution_space_id,
            context,
        )
    }

    pub fn release_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .release_work_as_host(work_id, expected_version, context)
    }

    pub fn release_as_member(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .release_work(work_id, expected_version, member_run_id, context)
    }

    pub fn claim(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .claim_work(work_id, expected_version, member_run_id, context)
    }

    pub fn start(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .start_work(work_id, expected_version, member_run_id, context)
    }

    pub fn block_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .block_work_as_host(work_id, expected_version, reason, context)
    }

    pub fn block_as_member(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        reason: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .block_work(work_id, expected_version, member_run_id, reason, context)
    }

    pub fn resume_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        resolution: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .resume_work_as_host(work_id, expected_version, resolution, context)
    }

    pub fn resume_as_member(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        resolution: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port.resume_work(
            work_id,
            expected_version,
            member_run_id,
            resolution,
            context,
        )
    }

    pub fn request_changes(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .request_work_changes(work_id, expected_version, reason, context)
    }

    pub fn cancel(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .cancel_work(work_id, expected_version, reason, context)
    }
}

/// A stable Host actor constructor for non-HTTP adapters. Authentication is
/// still the caller's responsibility; this merely avoids transport-specific
/// Work command shapes.
pub fn host_actor(id: impl Into<String>, authn_source: impl Into<String>) -> TeamActorRef {
    TeamActorRef {
        kind: firm_core::TeamActorKind::Host,
        id: id.into(),
        display_name: None,
        authn_source: Some(authn_source.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> WorkCommandContext {
        WorkCommandContext {
            event_id: "event-1".into(),
            performed_by_actor: host_actor("host-1", "test"),
            authority_actor: None,
            causation_ref: None,
            idempotency_key: "key-1".into(),
            created_at: "2026-08-25T00:00:00Z".into(),
            duplicate_ok: false,
        }
    }

    #[test]
    fn every_typed_work_action_has_one_stable_outcome_kind() {
        let create = || CreateWorkCommand {
            work_id: "work-1".into(),
            team_run_id: "run-1".into(),
            accountable_team_id: "team-1".into(),
            title: "Work".into(),
            context_markdown: String::new(),
            completion_criteria_markdown: "done".into(),
            claim_mode: WorkClaimMode::TeamClaim,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: WorkPriority::Normal,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            github_links: Vec::new(),
            expected_version: 0,
            context: context(),
        };
        let actions = vec![
            WorkAction::Create(create()),
            WorkAction::ReplaceDependencies(ReplaceWorkDependenciesCommand {
                accountable_team_id: "team-1".into(),
                work_id: "work-1".into(),
                expected_version: 1,
                prerequisite_work_ids: Vec::new(),
                context: context(),
            }),
            WorkAction::AssignMembership {
                work_id: "work-1".into(),
                expected_version: 1,
                membership_id: "membership-1".into(),
                execution_space_id: "space-1".into(),
                context: context(),
            },
            WorkAction::ReleaseHost {
                work_id: "work-1".into(),
                expected_version: 1,
                context: context(),
            },
            WorkAction::ReleaseMember {
                work_id: "work-1".into(),
                expected_version: 1,
                member_run_id: "member-run-1".into(),
                context: context(),
            },
            WorkAction::Claim {
                work_id: "work-1".into(),
                expected_version: 1,
                member_run_id: "member-run-1".into(),
                context: context(),
            },
            WorkAction::Start {
                work_id: "work-1".into(),
                expected_version: 1,
                member_run_id: "member-run-1".into(),
                context: context(),
            },
            WorkAction::BlockHost {
                work_id: "work-1".into(),
                expected_version: 1,
                reason: "blocked".into(),
                context: context(),
            },
            WorkAction::BlockMember {
                work_id: "work-1".into(),
                expected_version: 1,
                member_run_id: "member-run-1".into(),
                reason: "blocked".into(),
                context: context(),
            },
            WorkAction::ResumeHost {
                work_id: "work-1".into(),
                expected_version: 1,
                resolution: "fixed".into(),
                context: context(),
            },
            WorkAction::ResumeMember {
                work_id: "work-1".into(),
                expected_version: 1,
                member_run_id: "member-run-1".into(),
                resolution: "fixed".into(),
                context: context(),
            },
            WorkAction::RequestChanges {
                work_id: "work-1".into(),
                expected_version: 1,
                reason: "revise".into(),
                context: context(),
            },
            WorkAction::Cancel {
                work_id: "work-1".into(),
                expected_version: 1,
                reason: "cancel".into(),
                context: context(),
            },
        ];
        let kinds = actions.iter().map(WorkAction::kind).collect::<Vec<_>>();
        assert_eq!(
            kinds,
            vec![
                WorkActionKind::Create,
                WorkActionKind::ReplaceDependencies,
                WorkActionKind::AssignMembership,
                WorkActionKind::ReleaseHost,
                WorkActionKind::ReleaseMember,
                WorkActionKind::Claim,
                WorkActionKind::Start,
                WorkActionKind::BlockHost,
                WorkActionKind::BlockMember,
                WorkActionKind::ResumeHost,
                WorkActionKind::ResumeMember,
                WorkActionKind::RequestChanges,
                WorkActionKind::Cancel,
            ]
        );
    }
}

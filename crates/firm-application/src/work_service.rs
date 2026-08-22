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
    fn assign_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
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
    fn rebind_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
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
    fn submit_work(&self, command: SubmitWorkCommand) -> Result<Work, Self::Error>;
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
    pub initial_member_run_id: Option<String>,
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

/// Thin application composition over the authoritative Core + Store pair.
pub struct WorkApplication<'a, P: WorkPersistence + ?Sized> {
    port: &'a P,
}

impl<'a, P: WorkPersistence + ?Sized> WorkApplication<'a, P> {
    pub fn new(port: &'a P) -> Self {
        Self { port }
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
        draft.active_member_run_id = command.initial_member_run_id;
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

    pub fn assign_runtime(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .assign_work(work_id, expected_version, member_run_id, context)
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

    pub fn rebind(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> Result<Work, P::Error> {
        self.port
            .rebind_work(work_id, expected_version, member_run_id, context)
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

    pub fn submit(&self, command: SubmitWorkCommand) -> Result<Work, P::Error> {
        self.port.submit_work(command)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitWorkCommand {
    pub work_id: String,
    pub expected_version: u64,
    pub member_run_id: String,
    pub result_summary: String,
    pub artifact_refs: Vec<String>,
    pub check_refs: Vec<String>,
    pub github_links: Vec<GitHubLink>,
    pub base_revision: Option<String>,
    pub candidate_revision: Option<String>,
    pub context: WorkCommandContext,
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

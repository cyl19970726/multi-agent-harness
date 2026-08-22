use super::*;
use firm_application::{SubmitWorkCommand, WorkPersistence};

impl WorkPersistence for HarnessStore {
    type Error = StoreError;

    fn invalid_command(&self, message: String) -> Self::Error {
        StoreError::Conflict(message)
    }

    fn insert_work(&self, work: Work, context: WorkCommandContext) -> StoreResult<Work> {
        HarnessStore::insert_work(self, work, context)
    }

    fn load_work(&self, work_id: &str) -> StoreResult<Option<Work>> {
        Ok(self
            .latest_works()?
            .into_iter()
            .find(|work| work.id == work_id))
    }

    fn replace_work_dependencies(
        &self,
        work_id: &str,
        expected_version: u64,
        prerequisite_work_ids: Vec<String>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::replace_work_dependencies(
            self,
            work_id,
            expected_version,
            prerequisite_work_ids,
            context,
        )
    }

    fn assign_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::assign_work(self, work_id, expected_version, member_run_id, context)
    }

    fn assign_work_to_membership(
        &self,
        work_id: &str,
        expected_version: u64,
        membership_id: &str,
        execution_space_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::assign_work_to_membership(
            self,
            work_id,
            expected_version,
            membership_id,
            execution_space_id,
            context,
        )
    }

    fn rebind_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::rebind_work(self, work_id, expected_version, member_run_id, context)
    }

    fn release_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::release_work_as_host(self, work_id, expected_version, context)
    }

    fn release_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::release_work(self, work_id, expected_version, member_run_id, context)
    }

    fn claim_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::claim_work(self, work_id, expected_version, member_run_id, context)
    }

    fn start_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::start_work(self, work_id, expected_version, member_run_id, context)
    }

    fn block_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::block_work_as_host(self, work_id, expected_version, reason, context)
    }

    fn block_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::block_work(
            self,
            work_id,
            expected_version,
            member_run_id,
            reason,
            context,
        )
    }

    fn resume_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        resolution: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::resume_work_as_host(self, work_id, expected_version, resolution, context)
    }

    fn resume_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        resolution: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::resume_work(
            self,
            work_id,
            expected_version,
            member_run_id,
            resolution,
            context,
        )
    }

    fn submit_work(&self, command: SubmitWorkCommand) -> StoreResult<Work> {
        HarnessStore::submit_work_with_revision_and_links(
            self,
            &command.work_id,
            command.expected_version,
            &command.member_run_id,
            &command.result_summary,
            command.artifact_refs,
            command.check_refs,
            command.github_links,
            command.base_revision,
            command.candidate_revision,
            command.context,
        )
    }

    fn request_work_changes(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::request_work_changes(self, work_id, expected_version, reason, context)
    }

    fn cancel_work(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        HarnessStore::cancel_work(self, work_id, expected_version, reason, context)
    }
}

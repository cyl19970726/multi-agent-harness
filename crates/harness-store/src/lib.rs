use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::{
    AgentEvent, AgentMember, AgentRuntime, AgentTeam, AgentTeamRun, Decision, DelegationRun,
    Evidence, Gap, Goal, GoalCase, GoalDesign, GoalEvaluation, GoalOrchestrationRun, MemberAction,
    MemberRun, Message, MessageDelivery, MessageDeliveryStatus, MessageTerminalSource, Proposal,
    ProviderChildThread, ProviderSession, ProviderSessionStatus, Review, Task, TeamMessage,
    TeamRunEvent, Vision, WorkflowArtifactManifest, WorkflowPatch, WorkflowRun, WorkflowStep,
};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("timed out waiting for store write lock {0}")]
    LockTimeout(String),
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageDeliveryClaimResult {
    Claimed(Box<Message>),
    NotQueued,
    BlockedBySession(String),
}

#[derive(Debug, Clone)]
pub struct HarnessStore {
    root: PathBuf,
}

impl HarnessStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn init(&self) -> StoreResult<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(self.root.join("provider-sessions"))?;
        fs::create_dir_all(self.root.join("prompts"))?;
        fs::create_dir_all(self.root.join("runtimes"))?;
        Ok(())
    }

    pub fn append_goal(&self, value: &Goal) -> StoreResult<()> {
        self.append_jsonl("goals.jsonl", value)
    }

    pub fn append_member(&self, value: &AgentMember) -> StoreResult<()> {
        self.append_jsonl("members.jsonl", value)
    }

    pub fn append_team(&self, value: &AgentTeam) -> StoreResult<()> {
        self.append_jsonl("teams.jsonl", value)
    }

    pub fn append_runtime(&self, value: &AgentRuntime) -> StoreResult<()> {
        self.append_jsonl("agent_runtimes.jsonl", value)
    }

    pub fn append_event(&self, value: &AgentEvent) -> StoreResult<()> {
        self.append_jsonl("agent_events.jsonl", value)
    }

    pub fn append_proposal(&self, value: &Proposal) -> StoreResult<()> {
        self.append_jsonl("proposals.jsonl", value)
    }

    pub fn append_task(&self, value: &Task) -> StoreResult<()> {
        self.append_jsonl("tasks.jsonl", value)
    }

    pub fn append_message(&self, value: &Message) -> StoreResult<()> {
        self.append_jsonl("messages.jsonl", value)
    }

    pub fn append_evidence(&self, value: &Evidence) -> StoreResult<()> {
        self.append_jsonl("evidence.jsonl", value)
    }

    pub fn append_decision(&self, value: &Decision) -> StoreResult<()> {
        self.append_jsonl("decisions.jsonl", value)
    }

    pub fn append_review(&self, value: &Review) -> StoreResult<()> {
        self.append_jsonl("reviews.jsonl", value)
    }

    pub fn append_gap(&self, value: &Gap) -> StoreResult<()> {
        self.append_jsonl("gaps.jsonl", value)
    }

    pub fn append_goal_design(&self, value: &GoalDesign) -> StoreResult<()> {
        self.append_jsonl("goal_designs.jsonl", value)
    }

    pub fn append_goal_evaluation(&self, value: &GoalEvaluation) -> StoreResult<()> {
        self.append_jsonl("goal_evaluations.jsonl", value)
    }

    pub fn append_goal_case(&self, value: &GoalCase) -> StoreResult<()> {
        self.append_jsonl("goal_cases.jsonl", value)
    }

    pub fn append_vision(&self, value: &Vision) -> StoreResult<()> {
        self.append_jsonl("visions.jsonl", value)
    }

    pub fn append_provider_session(&self, value: &ProviderSession) -> StoreResult<()> {
        self.append_jsonl("provider_sessions.jsonl", value)
    }

    pub fn append_provider_child_thread(&self, value: &ProviderChildThread) -> StoreResult<()> {
        self.append_jsonl("provider_child_threads.jsonl", value)
    }

    pub fn append_workflow_run(&self, value: &WorkflowRun) -> StoreResult<()> {
        self.append_jsonl("workflow_runs.jsonl", value)
    }

    pub fn append_workflow_step(&self, value: &WorkflowStep) -> StoreResult<()> {
        self.append_jsonl("workflow_steps.jsonl", value)
    }

    pub fn append_workflow_patch(&self, value: &WorkflowPatch) -> StoreResult<()> {
        self.append_jsonl("workflow_patches.jsonl", value)
    }

    pub fn append_workflow_artifact_manifest(
        &self,
        value: &WorkflowArtifactManifest,
    ) -> StoreResult<()> {
        self.append_jsonl("workflow_artifact_manifests.jsonl", value)
    }

    pub fn append_goal_orchestration_run(&self, value: &GoalOrchestrationRun) -> StoreResult<()> {
        self.append_jsonl("goal_orchestration_runs.jsonl", value)
    }

    pub fn append_team_run(&self, value: &AgentTeamRun) -> StoreResult<()> {
        self.append_jsonl("team_runs.jsonl", value)
    }

    pub fn append_member_run(&self, value: &MemberRun) -> StoreResult<()> {
        self.append_jsonl("member_runs.jsonl", value)
    }

    pub fn append_team_message(&self, value: &TeamMessage) -> StoreResult<()> {
        self.append_jsonl("team_messages.jsonl", value)
    }

    pub fn append_member_action(&self, value: &MemberAction) -> StoreResult<()> {
        self.append_jsonl("member_actions.jsonl", value)
    }

    pub fn append_delegation_run(&self, value: &DelegationRun) -> StoreResult<()> {
        self.append_jsonl("delegation_runs.jsonl", value)
    }

    pub fn append_team_run_event(&self, value: &TeamRunEvent) -> StoreResult<()> {
        self.append_jsonl("team_run_events.jsonl", value)
    }

    pub fn claim_queued_message_delivery(
        &self,
        agent_member_id: &str,
        message_id: &str,
        delivery: MessageDelivery,
        mut provider_session: ProviderSession,
    ) -> StoreResult<MessageDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;

        let latest_sessions = latest_by_id(
            self.read_jsonl::<ProviderSession>("provider_sessions.jsonl")?,
            |session| session.id.clone(),
        );
        if let Some(session) = latest_sessions.into_values().find(|session| {
            session.agent_member_id == agent_member_id && session_blocks_delivery(session)
        }) {
            return Ok(MessageDeliveryClaimResult::BlockedBySession(session.id));
        }

        let latest_messages =
            latest_by_id(self.read_jsonl::<Message>("messages.jsonl")?, |message| {
                message.id.clone()
            });
        let Some(mut message) = latest_messages.get(message_id).cloned() else {
            return Ok(MessageDeliveryClaimResult::NotQueued);
        };
        if message.to_agent_id.as_deref() != Some(agent_member_id)
            || message.delivery_status != MessageDeliveryStatus::Queued
        {
            return Ok(MessageDeliveryClaimResult::NotQueued);
        }

        provider_session.agent_member_id = agent_member_id.to_string();
        provider_session.task_id = message.task_id.clone();
        provider_session.status = ProviderSessionStatus::Running;
        self.append_jsonl_unlocked("provider_sessions.jsonl", &provider_session)?;

        message.delivery_status = MessageDeliveryStatus::Acknowledged;
        message.delivery = Some(delivery);
        self.append_jsonl_unlocked("messages.jsonl", &message)?;

        Ok(MessageDeliveryClaimResult::Claimed(Box::new(message)))
    }

    pub fn goals(&self) -> StoreResult<Vec<Goal>> {
        self.read_jsonl("goals.jsonl")
    }

    pub fn members(&self) -> StoreResult<Vec<AgentMember>> {
        self.read_jsonl("members.jsonl")
    }

    pub fn teams(&self) -> StoreResult<Vec<AgentTeam>> {
        self.read_jsonl("teams.jsonl")
    }

    pub fn runtimes(&self) -> StoreResult<Vec<AgentRuntime>> {
        self.read_jsonl("agent_runtimes.jsonl")
    }

    pub fn events(&self) -> StoreResult<Vec<AgentEvent>> {
        self.read_jsonl("agent_events.jsonl")
    }

    pub fn proposals(&self) -> StoreResult<Vec<Proposal>> {
        self.read_jsonl("proposals.jsonl")
    }

    pub fn tasks(&self) -> StoreResult<Vec<Task>> {
        self.read_jsonl("tasks.jsonl")
    }

    pub fn messages(&self) -> StoreResult<Vec<Message>> {
        self.read_jsonl("messages.jsonl")
    }

    pub fn evidence(&self) -> StoreResult<Vec<Evidence>> {
        self.read_jsonl("evidence.jsonl")
    }

    pub fn decisions(&self) -> StoreResult<Vec<Decision>> {
        self.read_jsonl("decisions.jsonl")
    }

    pub fn reviews(&self) -> StoreResult<Vec<Review>> {
        self.read_jsonl("reviews.jsonl")
    }

    pub fn gaps(&self) -> StoreResult<Vec<Gap>> {
        self.read_jsonl("gaps.jsonl")
    }

    pub fn goal_designs(&self) -> StoreResult<Vec<GoalDesign>> {
        self.read_jsonl("goal_designs.jsonl")
    }

    pub fn goal_evaluations(&self) -> StoreResult<Vec<GoalEvaluation>> {
        self.read_jsonl("goal_evaluations.jsonl")
    }

    pub fn goal_cases(&self) -> StoreResult<Vec<GoalCase>> {
        self.read_jsonl("goal_cases.jsonl")
    }

    pub fn visions(&self) -> StoreResult<Vec<Vision>> {
        self.read_jsonl("visions.jsonl")
    }

    pub fn provider_sessions(&self) -> StoreResult<Vec<ProviderSession>> {
        self.read_jsonl("provider_sessions.jsonl")
    }

    pub fn provider_child_threads(&self) -> StoreResult<Vec<ProviderChildThread>> {
        self.read_jsonl("provider_child_threads.jsonl")
    }

    pub fn workflow_runs(&self) -> StoreResult<Vec<WorkflowRun>> {
        self.read_jsonl("workflow_runs.jsonl")
    }

    pub fn workflow_steps(&self) -> StoreResult<Vec<WorkflowStep>> {
        self.read_jsonl("workflow_steps.jsonl")
    }

    pub fn workflow_patches(&self) -> StoreResult<Vec<WorkflowPatch>> {
        self.read_jsonl("workflow_patches.jsonl")
    }

    pub fn workflow_artifact_manifests(&self) -> StoreResult<Vec<WorkflowArtifactManifest>> {
        self.read_jsonl("workflow_artifact_manifests.jsonl")
    }

    pub fn goal_orchestration_runs(&self) -> StoreResult<Vec<GoalOrchestrationRun>> {
        self.read_jsonl("goal_orchestration_runs.jsonl")
    }

    pub fn team_runs(&self) -> StoreResult<Vec<AgentTeamRun>> {
        self.read_jsonl("team_runs.jsonl")
    }

    pub fn member_runs(&self) -> StoreResult<Vec<MemberRun>> {
        self.read_jsonl("member_runs.jsonl")
    }

    pub fn team_messages(&self) -> StoreResult<Vec<TeamMessage>> {
        self.read_jsonl("team_messages.jsonl")
    }

    pub fn member_actions(&self) -> StoreResult<Vec<MemberAction>> {
        self.read_jsonl("member_actions.jsonl")
    }

    pub fn delegation_runs(&self) -> StoreResult<Vec<DelegationRun>> {
        self.read_jsonl("delegation_runs.jsonl")
    }

    pub fn team_run_events(&self) -> StoreResult<Vec<TeamRunEvent>> {
        self.read_jsonl("team_run_events.jsonl")
    }

    fn append_jsonl<T: Serialize>(&self, file_name: &str, value: &T) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.append_jsonl_unlocked(file_name, value)
    }

    fn append_jsonl_unlocked<T: Serialize>(&self, file_name: &str, value: &T) -> StoreResult<()> {
        let mut row = Vec::new();
        serde_json::to_writer(&mut row, value)?;
        row.push(b'\n');

        let path = self.root.join(file_name);
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(&row)?;
        file.flush()?;
        // Durability: fsync the row to stable storage before returning. Without
        // this a crash immediately after a claim append (the Running session row
        // + the Acknowledged message row in `claim_queued_message_delivery`) can
        // lose those rows from the OS page cache; latest-wins projection would
        // then revert the message to Queued and double-deliver it. `flush()`
        // only drains the userspace buffer, not the kernel cache, so we must
        // `sync_all`. Always called under the global flock, so write ordering
        // across files is preserved.
        file.sync_all()?;
        Ok(())
    }

    fn acquire_write_lock(&self) -> StoreResult<StoreWriteLock> {
        let lock_path = self.root.join(".store.lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match lock_file_exclusive(&file) {
                Ok(()) => return Ok(StoreWriteLock { file }),
                Err(error) if would_block_lock(&error) => {
                    if Instant::now() >= deadline {
                        return Err(StoreError::LockTimeout(lock_path.display().to_string()));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => return Err(StoreError::Io(error)),
            }
        }
    }

    fn read_jsonl<T: DeserializeOwned>(&self, file_name: &str) -> StoreResult<Vec<T>> {
        let path = self.root.join(file_name);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(path)?;
        let mut values = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            values.push(serde_json::from_str(&line)?);
        }
        Ok(values)
    }
}

fn latest_by_id<T>(
    values: Vec<T>,
    mut id: impl FnMut(&T) -> String,
) -> std::collections::BTreeMap<String, T> {
    let mut latest = std::collections::BTreeMap::new();
    for value in values {
        latest.insert(id(&value), value);
    }
    latest
}

fn session_blocks_delivery(session: &ProviderSession) -> bool {
    session.status == ProviderSessionStatus::Queued
        || session.status == ProviderSessionStatus::Running
        || (session.status == ProviderSessionStatus::Stale
            && session.terminal_source != Some(MessageTerminalSource::Failed))
}

fn lock_file_exclusive(file: &File) -> std::io::Result<()> {
    let result = unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) };
    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn unlock_file(file: &File) {
    let _ = unsafe { flock(file.as_raw_fd(), LOCK_UN) };
}

fn would_block_lock(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(11) | Some(35))
        || error.kind() == std::io::ErrorKind::WouldBlock
}

struct StoreWriteLock {
    file: File,
}

impl Drop for StoreWriteLock {
    fn drop(&mut self) {
        unlock_file(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Barrier};
    use std::time::{SystemTime, UNIX_EPOCH};

    use harness_core::{
        DelegationMode, DelegationStatus, Goal, GoalStage, GoalStatus, MemberActionStatus,
        MemberRunStatus, MessageKind, SenderKind, TeamDeliveryPolicy, TeamDeliveryStatus,
        TeamMessageDelivery, TeamMessageKind, TeamRunEventSourceKind, TeamRunStatus,
    };

    use super::*;

    #[test]
    fn append_and_read_goal_jsonl() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        let goal = Goal {
            phases: Vec::new(),
            knowledge: Vec::new(),
            design_synthesis_at: None,
            id: "goal-1".into(),
            title: "Self-host".into(),
            owner_agent_id: "leader-1".into(),
            status: GoalStatus::Active,
            priority: "p0".into(),
            created_at: "2026-05-26T00:00:00Z".into(),
            updated_at: "2026-05-26T00:00:00Z".into(),
            vision_id: None,
            goal_design_id: None,
            closed_by_decision_id: None,
            git_metadata: None,
            stage: GoalStage::default(),
            description_md: None,
            design_md: None,
            acceptance_md: None,
            explorations: Vec::new(),
            skill_refs: Vec::new(),
            stage_changed_at: None,
        };

        store.append_goal(&goal).expect("append goal");
        assert_eq!(store.goals().expect("read goals"), vec![goal]);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn concurrent_appends_write_complete_jsonl_rows() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-concurrent-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = Arc::new(HarnessStore::new(&root));
        let worker_count = 8;
        let appends_per_worker = 25;
        let barrier = Arc::new(Barrier::new(worker_count));
        let mut handles = Vec::new();

        for worker in 0..worker_count {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                for index in 0..appends_per_worker {
                    let goal = Goal {
                        phases: Vec::new(),
                        knowledge: Vec::new(),
                        design_synthesis_at: None,
                        id: format!("goal-{worker}-{index}"),
                        title: "Concurrent".into(),
                        owner_agent_id: "leader-1".into(),
                        status: GoalStatus::Active,
                        priority: "p1".into(),
                        created_at: "2026-05-26T00:00:00Z".into(),
                        updated_at: "2026-05-26T00:00:00Z".into(),
                        vision_id: None,
                        goal_design_id: None,
                        closed_by_decision_id: None,
                        git_metadata: None,
                        stage: GoalStage::default(),
                        description_md: None,
                        design_md: None,
                        acceptance_md: None,
                        explorations: Vec::new(),
                        skill_refs: Vec::new(),
                        stage_changed_at: None,
                    };
                    store.append_goal(&goal).expect("append goal");
                }
            }));
        }

        for handle in handles {
            handle.join().expect("worker thread");
        }

        let goals = store.goals().expect("read goals");
        assert_eq!(goals.len(), worker_count * appends_per_worker);
        let ids = goals
            .iter()
            .map(|goal| goal.id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), worker_count * appends_per_worker);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_uses_unlocked_existing_lock_file() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-stale-lock-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("init store");
        std::fs::write(root.join(".store.lock"), "left by interrupted writer\n")
            .expect("write existing lock file");
        let goal = Goal {
            phases: Vec::new(),
            knowledge: Vec::new(),
            design_synthesis_at: None,
            id: "goal-stale-lock".into(),
            title: "Stale lock".into(),
            owner_agent_id: "leader-1".into(),
            status: GoalStatus::Active,
            priority: "p1".into(),
            created_at: "2026-05-26T00:00:00Z".into(),
            updated_at: "2026-05-26T00:00:00Z".into(),
            vision_id: None,
            goal_design_id: None,
            closed_by_decision_id: None,
            git_metadata: None,
            stage: GoalStage::default(),
            description_md: None,
            design_md: None,
            acceptance_md: None,
            explorations: Vec::new(),
            skill_refs: Vec::new(),
            stage_changed_at: None,
        };

        store
            .append_goal(&goal)
            .expect("append with unlocked lock file");
        assert_eq!(store.goals().expect("read goals"), vec![goal]);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn claim_queued_message_is_atomic_and_blocks_second_claim() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-claim-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store
            .append_message(&test_message("message-1", "agent-1"))
            .expect("append message 1");
        store
            .append_message(&test_message("message-2", "agent-1"))
            .expect("append message 2");

        let claim = store
            .claim_queued_message_delivery(
                "agent-1",
                "message-1",
                test_delivery("delivery-1"),
                test_provider_session("delivery-1", "agent-1"),
            )
            .expect("claim message");
        assert!(matches!(claim, MessageDeliveryClaimResult::Claimed(_)));

        let latest_message = store
            .messages()
            .expect("messages")
            .into_iter()
            .rev()
            .find(|message| message.id == "message-1")
            .expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            MessageDeliveryStatus::Acknowledged
        );
        assert_eq!(
            latest_message
                .delivery
                .and_then(|delivery| delivery.provider_session_id),
            Some("delivery-1".into())
        );

        let second_claim = store
            .claim_queued_message_delivery(
                "agent-1",
                "message-2",
                test_delivery("delivery-2"),
                test_provider_session("delivery-2", "agent-1"),
            )
            .expect("second claim");
        assert_eq!(
            second_claim,
            MessageDeliveryClaimResult::BlockedBySession("delivery-1".into())
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    /// Durability: a claim writes the Acknowledged message row and the Running
    /// provider-session row, fsyncs them, and a *separate* store handle opened
    /// against the same root (no shared in-memory state, mirroring a process
    /// restart after a crash) reads them back. This guards the double-delivery
    /// regression: if the Acknowledged row were lost, latest-wins would revert
    /// the message to Queued and it would be claimable again.
    #[test]
    fn claim_appends_survive_reopen() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-durability-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store
            .append_message(&test_message("message-d", "agent-d"))
            .expect("append message");

        let claim = store
            .claim_queued_message_delivery(
                "agent-d",
                "message-d",
                test_delivery("delivery-d"),
                test_provider_session("delivery-d", "agent-d"),
            )
            .expect("claim message");
        assert!(matches!(claim, MessageDeliveryClaimResult::Claimed(_)));

        // Reopen with a fresh handle: only on-disk (fsynced) state is visible.
        let reopened = HarnessStore::new(&root);

        let message = reopened
            .messages()
            .expect("read messages")
            .into_iter()
            .rev()
            .find(|message| message.id == "message-d")
            .expect("acknowledged message row survives reopen");
        assert_eq!(
            message.delivery_status,
            MessageDeliveryStatus::Acknowledged,
            "acknowledged status must survive a restart so the message is not re-delivered"
        );

        let session = reopened
            .provider_sessions()
            .expect("read provider sessions")
            .into_iter()
            .rev()
            .find(|session| session.id == "delivery-d")
            .expect("running provider-session row survives reopen");
        assert_eq!(session.status, ProviderSessionStatus::Running);

        // The reopened store must refuse to re-claim: because both the
        // Acknowledged message row and the Running provider-session row survived
        // the fsync, the re-claim is rejected (the Running session for this
        // agent blocks delivery; were both rows lost it would return Claimed and
        // double-deliver). Either rejection variant proves no double-delivery.
        let reclaim = reopened
            .claim_queued_message_delivery(
                "agent-d",
                "message-d",
                test_delivery("delivery-d2"),
                test_provider_session("delivery-d2", "agent-d"),
            )
            .expect("reclaim attempt");
        assert!(
            !matches!(reclaim, MessageDeliveryClaimResult::Claimed(_)),
            "fsynced claim state must prevent a second delivery, got {reclaim:?}"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn team_test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "harness-store-team-test-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ))
    }

    fn append_sparse_row(root: &Path, file_name: &str, row: &str) {
        let mut file = OpenOptions::new()
            .append(true)
            .open(root.join(file_name))
            .expect("open jsonl for sparse row");
        writeln!(file, "{row}").expect("write sparse row");
        file.sync_all().expect("sync sparse row");
    }

    #[test]
    fn append_and_read_team_run_jsonl() {
        let root = team_test_root("team-run");
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: "tr-1".into(),
            definition_id: Some("td-1".into()),
            previous_run_id: Some("tr-0".into()),
            host_surface: "codex-app".into(),
            host_thread_id: Some("thread-1".into()),
            objective: "Ship the feature".into(),
            status: TeamRunStatus::Running,
            wave_index: 2,
            member_run_ids: vec!["mr-1".into()],
            task_ids: vec!["task-1".into()],
            budget_limit_usd: Some(12.5),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:2".into(),
            completed_at: None,
        };

        store.append_team_run(&run).expect("append team run");
        // A sparse row omitting every optional field must read back with defaults.
        append_sparse_row(
            &root,
            "team_runs.jsonl",
            r#"{"id":"tr-sparse","host_surface":"kimi-cli","objective":"obj","status":"planning","created_at":"unix-ms:3","updated_at":"unix-ms:3"}"#,
        );

        let runs = store.team_runs().expect("read team runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], run);
        let sparse = &runs[1];
        assert_eq!(sparse.id, "tr-sparse");
        assert_eq!(sparse.wave_index, 1);
        assert!(sparse.definition_id.is_none());
        assert!(sparse.previous_run_id.is_none());
        assert!(sparse.host_thread_id.is_none());
        assert!(sparse.member_run_ids.is_empty());
        assert!(sparse.task_ids.is_empty());
        assert!(sparse.budget_limit_usd.is_none());
        assert!(sparse.completed_at.is_none());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_member_run_jsonl() {
        let root = team_test_root("member-run");
        let store = HarnessStore::new(&root);
        let member_run = MemberRun {
            id: "mr-1".into(),
            team_run_id: "tr-1".into(),
            slot_id: Some("slot-1".into()),
            name: "worker-1".into(),
            role: "worker".into(),
            provider: "kimi".into(),
            model: Some("kimi-k2".into()),
            status: MemberRunStatus::Running,
            provider_session_id: Some("ps-1".into()),
            acp_session_id: Some("acp-1".into()),
            current_task_id: Some("task-1".into()),
            worktree_ref: Some("wt-1".into()),
            owned_paths: vec!["src/".into()],
            started_at: "unix-ms:1".into(),
            last_event_at: Some("unix-ms:2".into()),
            finished_at: None,
        };

        store
            .append_member_run(&member_run)
            .expect("append member run");
        append_sparse_row(
            &root,
            "member_runs.jsonl",
            r#"{"id":"mr-sparse","team_run_id":"tr-1","name":"w","role":"worker","provider":"codex","status":"idle","started_at":"unix-ms:3"}"#,
        );

        let runs = store.member_runs().expect("read member runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], member_run);
        let sparse = &runs[1];
        assert_eq!(sparse.id, "mr-sparse");
        assert_eq!(sparse.status, MemberRunStatus::Idle);
        assert!(sparse.slot_id.is_none());
        assert!(sparse.model.is_none());
        assert!(sparse.provider_session_id.is_none());
        assert!(sparse.acp_session_id.is_none());
        assert!(sparse.current_task_id.is_none());
        assert!(sparse.worktree_ref.is_none());
        assert!(sparse.owned_paths.is_empty());
        assert!(sparse.last_event_at.is_none());
        assert!(sparse.finished_at.is_none());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_team_message_jsonl() {
        let root = team_test_root("team-message");
        let store = HarnessStore::new(&root);
        let message = TeamMessage {
            id: "tm-1".into(),
            team_run_id: "tr-1".into(),
            task_id: Some("task-1".into()),
            from_member_id: "host".into(),
            to_member_ids: vec!["mr-1".into()],
            kind: TeamMessageKind::Assignment,
            body: "Take task-1".into(),
            correlation_id: "corr-1".into(),
            causation_id: None,
            evidence_refs: vec!["ev-1".into()],
            deliveries: vec![TeamMessageDelivery {
                member_id: "mr-1".into(),
                policy: TeamDeliveryPolicy::Inject,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:1".into(),
        };

        store
            .append_team_message(&message)
            .expect("append team message");
        append_sparse_row(
            &root,
            "team_messages.jsonl",
            r#"{"id":"tm-sparse","team_run_id":"tr-1","from_member_id":"host","kind":"broadcast","body":"hi","correlation_id":"corr-2","created_at":"unix-ms:3"}"#,
        );

        let messages = store.team_messages().expect("read team messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], message);
        let sparse = &messages[1];
        assert_eq!(sparse.id, "tm-sparse");
        assert_eq!(sparse.kind, TeamMessageKind::Broadcast);
        assert!(sparse.task_id.is_none());
        assert!(sparse.to_member_ids.is_empty());
        assert!(sparse.causation_id.is_none());
        assert!(sparse.evidence_refs.is_empty());
        assert!(sparse.deliveries.is_empty());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_member_action_jsonl() {
        let root = team_test_root("member-action");
        let store = HarnessStore::new(&root);
        let action = MemberAction {
            id: "ma-1".into(),
            seq: 7,
            team_run_id: "tr-1".into(),
            member_run_id: "mr-1".into(),
            task_id: Some("task-1".into()),
            action_type: "tool_completed".into(),
            status: MemberActionStatus::Succeeded,
            title: "cargo test".into(),
            summary: "all green".into(),
            evidence_refs: vec!["ev-1".into()],
            started_at: "unix-ms:1".into(),
            completed_at: Some("unix-ms:2".into()),
        };

        store
            .append_member_action(&action)
            .expect("append member action");
        append_sparse_row(
            &root,
            "member_actions.jsonl",
            r#"{"id":"ma-sparse","seq":8,"team_run_id":"tr-1","member_run_id":"mr-1","action_type":"blocked","status":"started","title":"t","summary":"s","started_at":"unix-ms:3"}"#,
        );

        let actions = store.member_actions().expect("read member actions");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], action);
        let sparse = &actions[1];
        assert_eq!(sparse.id, "ma-sparse");
        assert_eq!(sparse.seq, 8);
        assert!(sparse.task_id.is_none());
        assert!(sparse.evidence_refs.is_empty());
        assert!(sparse.completed_at.is_none());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_delegation_run_jsonl() {
        let root = team_test_root("delegation-run");
        let store = HarnessStore::new(&root);
        let delegation = DelegationRun {
            id: "dr-1".into(),
            team_run_id: "tr-1".into(),
            parent_member_run_id: "mr-1".into(),
            parent_task_id: Some("task-1".into()),
            mode: DelegationMode::HarnessWorker,
            provider: "claude".into(),
            provider_child_thread_id: None,
            workflow_run_id: Some("wfr-1".into()),
            objective: "Research X".into(),
            status: DelegationStatus::Running,
            evidence_ids: vec!["ev-1".into()],
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:2".into(),
        };

        store
            .append_delegation_run(&delegation)
            .expect("append delegation run");
        append_sparse_row(
            &root,
            "delegation_runs.jsonl",
            r#"{"id":"dr-sparse","team_run_id":"tr-1","parent_member_run_id":"mr-1","mode":"provider_native","provider":"codex","objective":"obj","status":"planned","created_at":"unix-ms:3","updated_at":"unix-ms:3"}"#,
        );

        let delegations = store.delegation_runs().expect("read delegation runs");
        assert_eq!(delegations.len(), 2);
        assert_eq!(delegations[0], delegation);
        let sparse = &delegations[1];
        assert_eq!(sparse.id, "dr-sparse");
        assert_eq!(sparse.mode, DelegationMode::ProviderNative);
        assert_eq!(sparse.status, DelegationStatus::Planned);
        assert!(sparse.parent_task_id.is_none());
        assert!(sparse.provider_child_thread_id.is_none());
        assert!(sparse.workflow_run_id.is_none());
        assert!(sparse.evidence_ids.is_empty());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn append_and_read_team_run_event_jsonl() {
        let root = team_test_root("team-run-event");
        let store = HarnessStore::new(&root);
        let event = TeamRunEvent {
            id: "tre-1".into(),
            seq: 3,
            team_run_id: "tr-1".into(),
            source_kind: TeamRunEventSourceKind::Member,
            member_run_id: Some("mr-1".into()),
            delegation_run_id: None,
            entity_type: "action".into(),
            entity_id: "ma-1".into(),
            operation: "completed".into(),
            summary: "tool completed".into(),
            occurred_at: "unix-ms:1".into(),
        };

        store
            .append_team_run_event(&event)
            .expect("append team run event");
        append_sparse_row(
            &root,
            "team_run_events.jsonl",
            r#"{"id":"tre-sparse","seq":4,"team_run_id":"tr-1","source_kind":"host","entity_type":"team_run","entity_id":"tr-1","operation":"created","summary":"run started","occurred_at":"unix-ms:3"}"#,
        );

        let events = store.team_run_events().expect("read team run events");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], event);
        let sparse = &events[1];
        assert_eq!(sparse.id, "tre-sparse");
        assert_eq!(sparse.source_kind, TeamRunEventSourceKind::Host);
        assert!(sparse.member_run_id.is_none());
        assert!(sparse.delegation_run_id.is_none());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn test_message(id: &str, agent_id: &str) -> Message {
        Message {
            id: id.into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some(agent_id.into()),
            channel: Some("assignment".into()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Do the task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
            sender_kind: SenderKind::Agent,
        }
    }

    fn test_delivery(provider_session_id: &str) -> MessageDelivery {
        MessageDelivery {
            provider_session_id: Some(provider_session_id.into()),
            provider_request_id: None,
            provider_thread_id: None,
            provider_turn_id: None,
            terminal_source: None,
            delivered_at: None,
            last_error: None,
        }
    }

    fn test_provider_session(id: &str, agent_id: &str) -> ProviderSession {
        ProviderSession {
            id: id.into(),
            provider: "codex".into(),
            agent_member_id: agent_id.into(),
            task_id: Some("task-1".into()),
            workspace_ref: None,
            provider_thread_id: None,
            provider_turn_id: None,
            terminal_source: None,
            status: ProviderSessionStatus::Running,
            command: "harness".into(),
            args: Vec::new(),
            prompt_ref: None,
            prompt_summary: None,
            provider_session_ref: None,
            stdout_ref: None,
            jsonl_ref: None,
            transcript_ref: None,
            last_message_ref: None,
            exit_code: None,
            started_at: "unix-ms:1".into(),
            ended_at: None,
            evidence_ids: Vec::new(),
        }
    }
}

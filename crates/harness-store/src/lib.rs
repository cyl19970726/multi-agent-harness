use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use harness_core::{
    AgentEvent, AgentMember, AgentRuntime, AgentTeam, Decision, Evidence, Goal, Message, Proposal,
    ProviderChildThread, ProviderSession, Task,
};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type StoreResult<T> = Result<T, StoreError>;

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

    pub fn append_provider_session(&self, value: &ProviderSession) -> StoreResult<()> {
        self.append_jsonl("provider_sessions.jsonl", value)
    }

    pub fn append_provider_child_thread(&self, value: &ProviderChildThread) -> StoreResult<()> {
        self.append_jsonl("provider_child_threads.jsonl", value)
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

    pub fn provider_sessions(&self) -> StoreResult<Vec<ProviderSession>> {
        self.read_jsonl("provider_sessions.jsonl")
    }

    pub fn provider_child_threads(&self) -> StoreResult<Vec<ProviderChildThread>> {
        self.read_jsonl("provider_child_threads.jsonl")
    }

    fn append_jsonl<T: Serialize>(&self, file_name: &str, value: &T) -> StoreResult<()> {
        self.init()?;
        let path = self.root.join(file_name);
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        serde_json::to_writer(&mut file, value)?;
        file.write_all(b"\n")?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use harness_core::{Goal, GoalStatus};

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
            id: "goal-1".into(),
            title: "Self-host".into(),
            objective: "Manage this repo through harness objects".into(),
            owner_agent_id: "leader-1".into(),
            status: GoalStatus::Active,
            success_criteria: vec!["Goal is persisted".into()],
            priority: "p0".into(),
            created_at: "2026-05-26T00:00:00Z".into(),
            updated_at: "2026-05-26T00:00:00Z".into(),
        };

        store.append_goal(&goal).expect("append goal");
        assert_eq!(store.goals().expect("read goals"), vec![goal]);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }
}

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::{
    AgentEvent, AgentMember, AgentRuntime, AgentTeam, AgentTeamRun, Decision, DelegationRun,
    Evidence, Gap, MemberAction, MemberRun, Message, MessageDelivery, MessageDeliveryStatus,
    MessageTerminalSource, Mission, MissionStatus, PendingInteraction, Proposal,
    ProviderChildThread, ProviderSession, ProviderSessionStatus, Review, TeamMessage, TeamRunEvent,
    TeamRunStatus, Vision, Wave, WaveExecutorKind, WaveGateStatus, WaveStatus,
    WorkflowArtifactManifest, WorkflowPatch, WorkflowRun, WorkflowStep,
};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

mod company_os;
pub use company_os::{
    ActionAuditReservation, ActionCommandClaimResult, CompanyActor, FinancialRecord,
};

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
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid company os record: {0}")]
    CompanyOsValidation(String),
    #[error("company os reference not found: {0}")]
    CompanyOsMissingReference(String),
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

    pub fn append_mission(&self, value: &Mission) -> StoreResult<()> {
        self.append_jsonl("missions.jsonl", value)
    }

    /// Insert a new native Mission under the store lock. Unlike the generic
    /// append method this rejects a concurrently-created duplicate id.
    pub fn insert_mission(&self, value: &Mission) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let missions = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        });
        if missions.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "mission already exists: {}",
                value.id
            )));
        }
        self.append_jsonl_unlocked("missions.jsonl", value)
    }

    pub fn append_wave(&self, value: &Wave) -> StoreResult<()> {
        self.append_jsonl("waves.jsonl", value)
    }

    /// Atomically allocate/validate one Wave index, append the Wave, and update
    /// its Mission's ordered membership. This prevents concurrent creates from
    /// duplicating an index or losing one `wave_ids` update.
    pub fn insert_wave_and_update_mission(
        &self,
        mut wave: Wave,
        requested_index: Option<u32>,
        mission_updated_at: &str,
    ) -> StoreResult<Wave> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut missions = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        });
        let mut mission = missions.remove(&wave.mission_id).ok_or_else(|| {
            StoreError::Conflict(format!("native mission not found: {}", wave.mission_id))
        })?;
        if matches!(
            mission.status,
            MissionStatus::Completed | MissionStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "mission {} is {:?} and cannot accept another Wave",
                mission.id, mission.status
            )));
        }
        let waves = latest_by_id(self.read_jsonl::<Wave>("waves.jsonl")?, |row| {
            row.id.clone()
        })
        .into_values()
        .collect::<Vec<_>>();
        if waves.iter().any(|existing| existing.id == wave.id) {
            return Err(StoreError::Conflict(format!(
                "wave already exists: {}",
                wave.id
            )));
        }
        wave.index = match requested_index {
            Some(index) => index,
            None => waves
                .iter()
                .filter(|existing| existing.mission_id == wave.mission_id)
                .map(|existing| existing.index)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "wave index space is exhausted for mission {}",
                        wave.mission_id
                    ))
                })?,
        };
        if wave.index == 0 {
            return Err(StoreError::Conflict(
                "wave index must be at least 1".to_string(),
            ));
        }
        if waves
            .iter()
            .any(|existing| existing.mission_id == wave.mission_id && existing.index == wave.index)
        {
            return Err(StoreError::Conflict(format!(
                "wave index {} already exists for mission {}",
                wave.index, wave.mission_id
            )));
        }

        let mut ordered = waves
            .iter()
            .filter(|existing| existing.mission_id == wave.mission_id)
            .map(|existing| (existing.index, existing.id.clone()))
            .collect::<Vec<_>>();
        ordered.push((wave.index, wave.id.clone()));
        ordered.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        mission.wave_ids = ordered.into_iter().map(|(_, id)| id).collect();
        mission.updated_at = mission_updated_at.to_string();

        self.append_jsonl_unlocked("waves.jsonl", &wave)?;
        self.append_jsonl_unlocked("missions.jsonl", &mission)?;
        Ok(wave)
    }

    /// Atomically close one Mission after every ordered Wave has an accepted,
    /// completed gate. The Wave set is checked under the same store lock as
    /// the Mission CAS so a concurrent Wave create cannot race closeout.
    pub fn compare_and_close_mission(&self, expected: &Mission, next: &Mission) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("mission not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "mission {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        if current.wave_ids.is_empty() {
            return Err(StoreError::Conflict(format!(
                "mission {} has no Waves to close",
                current.id
            )));
        }
        let waves = latest_by_id(self.read_jsonl::<Wave>("waves.jsonl")?, |wave| {
            wave.id.clone()
        });
        let mut actual_wave_ids = waves
            .values()
            .filter(|wave| wave.mission_id == current.id)
            .map(|wave| (wave.index, wave.id.clone()))
            .collect::<Vec<_>>();
        actual_wave_ids.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let actual_wave_ids = actual_wave_ids
            .into_iter()
            .map(|(_, id)| id)
            .collect::<Vec<_>>();
        if actual_wave_ids != current.wave_ids {
            return Err(StoreError::Conflict(format!(
                "mission {} Wave membership changed or is inconsistent; retry closeout",
                current.id
            )));
        }
        for wave_id in &current.wave_ids {
            let wave = waves.get(wave_id).ok_or_else(|| {
                StoreError::Conflict(format!(
                    "mission {} references missing Wave {wave_id}",
                    current.id
                ))
            })?;
            if wave.mission_id != current.id
                || wave.status != WaveStatus::Completed
                || wave.gate_status != WaveGateStatus::Accepted
            {
                return Err(StoreError::Conflict(format!(
                    "mission {} cannot close: Wave {} is status {:?} with gate {:?}",
                    current.id, wave.id, wave.status, wave.gate_status
                )));
            }
        }
        if next.id != current.id
            || next.status != MissionStatus::Completed
            || next.outcome_summary.as_deref().is_none_or(str::is_empty)
            || next.completed_by.as_deref().is_none_or(str::is_empty)
            || next.completed_at.as_deref().is_none_or(str::is_empty)
        {
            return Err(StoreError::Conflict(
                "mission closeout must preserve identity and record completed status, outcome, actor, and timestamp"
                    .to_string(),
            ));
        }
        self.append_jsonl_unlocked("missions.jsonl", next)
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

    pub fn append_team_run(&self, value: &AgentTeamRun) -> StoreResult<()> {
        self.append_jsonl("team_runs.jsonl", value)
    }

    /// Atomically append a newly-created TeamRun and register it as an attempt
    /// of its native Wave. New writes are either fully unlinked compatibility
    /// runs or carry both Mission and Wave ids; Mission-only rows are rejected.
    pub fn insert_team_run_and_register_attempt(
        &self,
        value: &AgentTeamRun,
        wave_updated_at: &str,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let runs = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        });
        if runs.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "team run already exists: {}",
                value.id
            )));
        }

        match (value.mission_id.as_deref(), value.wave_id.as_deref()) {
            (None, None) => self.append_jsonl_unlocked("team_runs.jsonl", value),
            (Some(_), None) | (None, Some(_)) => Err(StoreError::Conflict(
                "new TeamRun must be either unlinked or linked to both Mission and Wave"
                    .to_string(),
            )),
            (Some(mission_id), Some(wave_id)) => {
                let mission =
                    latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
                        mission.id.clone()
                    })
                    .remove(mission_id)
                    .ok_or_else(|| {
                        StoreError::Conflict(format!("mission not found: {mission_id}"))
                    })?;
                if matches!(
                    mission.status,
                    MissionStatus::Completed | MissionStatus::Cancelled
                ) {
                    return Err(StoreError::Conflict(format!(
                        "mission {mission_id} is {:?} and cannot accept a TeamRun attempt",
                        mission.status
                    )));
                }
                let mut waves = latest_by_id(self.read_jsonl::<Wave>("waves.jsonl")?, |wave| {
                    wave.id.clone()
                });
                let mut wave = waves
                    .remove(wave_id)
                    .ok_or_else(|| StoreError::Conflict(format!("wave not found: {wave_id}")))?;
                if wave.mission_id != mission_id {
                    return Err(StoreError::Conflict(format!(
                        "wave {wave_id} belongs to mission {}, not {mission_id}",
                        wave.mission_id
                    )));
                }
                if wave.executor_kind != WaveExecutorKind::AgentTeam {
                    return Err(StoreError::Conflict(format!(
                        "wave {wave_id} is not an agent_team Wave"
                    )));
                }
                if !matches!(
                    wave.status,
                    WaveStatus::Planned | WaveStatus::Running | WaveStatus::Waiting
                ) {
                    return Err(StoreError::Conflict(format!(
                        "wave {wave_id} is terminal and cannot accept another attempt"
                    )));
                }
                let attempts = wave
                    .executor_run_ids
                    .iter()
                    .filter_map(|id| runs.get(id))
                    .collect::<Vec<_>>();
                if let Some(active) = attempts.iter().find(|run| {
                    matches!(
                        run.status,
                        TeamRunStatus::Planning
                            | TeamRunStatus::Running
                            | TeamRunStatus::Waiting
                            | TeamRunStatus::Reviewing
                    )
                }) {
                    return Err(StoreError::Conflict(format!(
                        "wave {wave_id} already has active attempt {} in status {:?}",
                        active.id, active.status
                    )));
                }
                if let Some(last_attempt_id) = wave.executor_run_ids.last() {
                    if value.previous_run_id.as_deref() != Some(last_attempt_id.as_str()) {
                        return Err(StoreError::Conflict(format!(
                            "retry for wave {wave_id} must set previous_run_id to latest attempt {last_attempt_id}"
                        )));
                    }
                }
                if let Some(previous_id) = value.previous_run_id.as_deref() {
                    let previous = runs.get(previous_id).ok_or_else(|| {
                        StoreError::Conflict(format!("previous team run not found: {previous_id}"))
                    })?;
                    if previous.mission_id.as_deref() != Some(mission_id)
                        || previous.wave_id.as_deref() != Some(wave_id)
                    {
                        return Err(StoreError::Conflict(format!(
                            "previous run {previous_id} is not an attempt of mission {mission_id} wave {wave_id}"
                        )));
                    }
                }
                self.append_jsonl_unlocked("team_runs.jsonl", value)?;
                if !wave.executor_run_ids.contains(&value.id) {
                    wave.executor_run_ids.push(value.id.clone());
                }
                wave.updated_at = wave_updated_at.to_string();
                self.append_jsonl_unlocked("waves.jsonl", &wave)
            }
        }
    }

    /// Compare-and-append one Wave row. Used by lifecycle/gate updates so a
    /// concurrent attempt registration or gate cannot be silently overwritten.
    pub fn compare_and_append_wave(&self, expected: &Wave, next: &Wave) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<Wave>("waves.jsonl")?, |wave| {
            wave.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("wave not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "wave {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        let mut missions = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        });
        let mut mission = missions.remove(&next.mission_id).ok_or_else(|| {
            StoreError::Conflict(format!("native mission not found: {}", next.mission_id))
        })?;
        if matches!(
            mission.status,
            MissionStatus::Completed | MissionStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "mission {} is {:?} and its Waves are immutable",
                mission.id, mission.status
            )));
        }
        mission.status = match next.gate_status {
            WaveGateStatus::Blocked => MissionStatus::Blocked,
            WaveGateStatus::Accepted | WaveGateStatus::Revise | WaveGateStatus::Pending => {
                MissionStatus::Running
            }
        };
        mission.updated_at = next.updated_at.clone();
        self.append_jsonl_unlocked("waves.jsonl", next)?;
        self.append_jsonl_unlocked("missions.jsonl", &mission)
    }

    pub fn append_member_run(&self, value: &MemberRun) -> StoreResult<()> {
        self.append_jsonl("member_runs.jsonl", value)
    }

    pub fn append_team_message(&self, value: &TeamMessage) -> StoreResult<()> {
        self.append_jsonl("team_messages.jsonl", value)
    }

    /// Append a manually-authored TeamMessage under the global lock. Assignment
    /// correlations are unique within a TeamRun even under concurrent sends.
    pub fn append_team_message_checked(&self, value: &TeamMessage) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let messages = latest_by_id(
            self.read_jsonl::<TeamMessage>("team_messages.jsonl")?,
            |message| message.id.clone(),
        );
        if messages.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "team message already exists: {}",
                value.id
            )));
        }
        if value.kind == harness_core::TeamMessageKind::Assignment
            && messages.values().any(|message| {
                message.team_run_id == value.team_run_id
                    && message.kind == harness_core::TeamMessageKind::Assignment
                    && message.correlation_id == value.correlation_id
            })
        {
            return Err(StoreError::Conflict(format!(
                "correlation_id `{}` already identifies an assignment in team run {}",
                value.correlation_id, value.team_run_id
            )));
        }
        self.append_jsonl_unlocked("team_messages.jsonl", value)
    }

    pub fn append_member_action(&self, value: &MemberAction) -> StoreResult<()> {
        self.append_jsonl("member_actions.jsonl", value)
    }

    pub fn append_pending_interaction(&self, value: &PendingInteraction) -> StoreResult<()> {
        self.append_jsonl("pending_interactions.jsonl", value)
    }

    pub fn append_delegation_run(&self, value: &DelegationRun) -> StoreResult<()> {
        self.append_jsonl("delegation_runs.jsonl", value)
    }

    pub fn append_team_run_event(&self, value: &TeamRunEvent) -> StoreResult<()> {
        self.append_jsonl("team_run_events.jsonl", value)
    }

    /// Allocate and append the next per-TeamRun event sequence under one store
    /// lock so concurrent HTTP/MCP/provider writers cannot duplicate `seq`.
    pub fn append_team_run_event_next(&self, mut value: TeamRunEvent) -> StoreResult<TeamRunEvent> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        value.seq = self
            .read_jsonl::<TeamRunEvent>("team_run_events.jsonl")?
            .into_iter()
            .filter(|event| event.team_run_id == value.team_run_id)
            .map(|event| event.seq)
            .max()
            .unwrap_or(0)
            + 1;
        self.append_jsonl_unlocked("team_run_events.jsonl", &value)?;
        Ok(value)
    }

    /// Compare-and-append a TeamRun lifecycle row and synchronize its linked
    /// Wave status under the same lock. This prevents two start/transition
    /// processes from resurrecting or overwriting one attempt.
    pub fn compare_and_append_team_run_with_wave_status(
        &self,
        expected: &AgentTeamRun,
        next: &AgentTeamRun,
        linked_wave_status: WaveStatus,
        wave_updated_at: &str,
    ) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let current = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(&expected.id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {}", expected.id)))?;
        if current != *expected {
            return Err(StoreError::Conflict(format!(
                "team run {} changed concurrently or is no longer startable",
                expected.id
            )));
        }

        let linked_wave = match (next.mission_id.as_deref(), next.wave_id.as_deref()) {
            (None, None) => None,
            (Some(mission_id), Some(wave_id)) => {
                let mission =
                    latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
                        mission.id.clone()
                    })
                    .remove(mission_id)
                    .ok_or_else(|| {
                        StoreError::Conflict(format!("mission not found: {mission_id}"))
                    })?;
                if matches!(
                    mission.status,
                    MissionStatus::Completed | MissionStatus::Cancelled
                ) {
                    return Err(StoreError::Conflict(format!(
                        "mission {mission_id} is {:?} and cannot transition TeamRun {}",
                        mission.status, next.id
                    )));
                }
                let mut wave = latest_by_id(self.read_jsonl::<Wave>("waves.jsonl")?, |wave| {
                    wave.id.clone()
                })
                .remove(wave_id)
                .ok_or_else(|| StoreError::Conflict(format!("wave not found: {wave_id}")))?;
                if wave.mission_id != mission_id || !wave.executor_run_ids.contains(&next.id) {
                    return Err(StoreError::Conflict(format!(
                        "team run {} is not registered to mission {mission_id} wave {wave_id}",
                        next.id
                    )));
                }
                if wave.status == WaveStatus::Completed || wave.accepted_run_id.is_some() {
                    return Err(StoreError::Conflict(format!(
                        "wave {wave_id} is already accepted"
                    )));
                }
                wave.status = linked_wave_status;
                wave.updated_at = wave_updated_at.to_string();
                Some(wave)
            }
            _ => {
                return Err(StoreError::Conflict(
                    "TeamRun lifecycle has incomplete Mission/Wave linkage".to_string(),
                ));
            }
        };

        let linked_mission = if next.mission_id.is_some()
            && matches!(
                linked_wave_status,
                WaveStatus::Running | WaveStatus::Waiting
            ) {
            let mission_id = next.mission_id.as_deref().unwrap_or_default();
            let mut mission =
                latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
                    mission.id.clone()
                })
                .remove(mission_id)
                .ok_or_else(|| StoreError::Conflict(format!("mission not found: {mission_id}")))?;
            mission.status = MissionStatus::Running;
            mission.updated_at = wave_updated_at.to_string();
            Some(mission)
        } else {
            None
        };

        self.append_jsonl_unlocked("team_runs.jsonl", next)?;
        if let Some(wave) = linked_wave {
            self.append_jsonl_unlocked("waves.jsonl", &wave)?;
        }
        if let Some(mission) = linked_mission {
            self.append_jsonl_unlocked("missions.jsonl", &mission)?;
        }
        Ok(())
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

    /// Raw append-only Mission ledger rows, in append order.
    pub fn missions(&self) -> StoreResult<Vec<Mission>> {
        self.read_jsonl("missions.jsonl")
    }

    /// Latest-row-wins Mission projection, ordered by id for deterministic
    /// dashboard/API consumers.
    pub fn latest_missions(&self) -> StoreResult<Vec<Mission>> {
        Ok(latest_by_id(self.missions()?, |mission| mission.id.clone())
            .into_values()
            .collect())
    }

    /// Raw append-only Wave ledger rows, in append order.
    pub fn waves(&self) -> StoreResult<Vec<Wave>> {
        self.read_jsonl("waves.jsonl")
    }

    /// Latest-row-wins Wave projection, ordered by Mission then Wave index for
    /// deterministic product reads. The id is a final tie-breaker for corrupt
    /// legacy rows; native authoring rejects duplicate Mission/index pairs.
    pub fn latest_waves(&self) -> StoreResult<Vec<Wave>> {
        let mut waves = latest_by_id(self.waves()?, |wave| wave.id.clone())
            .into_values()
            .collect::<Vec<_>>();
        waves.sort_by(|left, right| {
            left.mission_id
                .cmp(&right.mission_id)
                .then(left.index.cmp(&right.index))
                .then(left.id.cmp(&right.id))
        });
        Ok(waves)
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

    pub fn pending_interactions(&self) -> StoreResult<Vec<PendingInteraction>> {
        self.read_jsonl("pending_interactions.jsonl")
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
        DelegationMode, DelegationStatus, MemberActionStatus, MemberRunStatus, MessageKind,
        Mission, MissionStatus, SenderKind, TeamDeliveryPolicy, TeamDeliveryStatus,
        TeamMessageDelivery, TeamMessageKind, TeamRunEventSourceKind, TeamRunStatus, Wave,
        WaveExecutorKind, WaveGateStatus, WaveStatus,
    };

    use super::*;

    #[test]
    fn mission_and_wave_ledgers_keep_history_and_project_latest_rows() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-mission-wave-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        let mission = Mission {
            id: "mission-1".into(),
            title: "Ship Mission/Wave".into(),
            objective: "Add the migration foundation".into(),
            desired_outcome: Some("A compatible, durable contract".into()),
            status: MissionStatus::Planned,
            wave_ids: vec!["wave-1".into()],
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        let mut updated_mission = mission.clone();
        updated_mission.status = MissionStatus::Running;
        updated_mission.updated_at = "unix-ms:2".into();
        store.append_mission(&mission).expect("append mission");
        store
            .append_mission(&updated_mission)
            .expect("append updated mission");

        let wave = Wave {
            id: "wave-1".into(),
            mission_id: "mission-1".into(),
            index: 1,
            title: "Contract".into(),
            objective: "Define the additive contract".into(),
            exit_criteria: Some("Schema and store rows validate".into()),
            status: WaveStatus::Running,
            executor_kind: WaveExecutorKind::AgentTeam,
            executor_run_ids: vec!["team-run-1".into()],
            accepted_run_id: None,
            plan_note: None,
            outcome_summary: None,
            artifact_refs: vec!["schemas/mission.schema.json".into()],
            gate_status: WaveGateStatus::Pending,
            gate_note: None,
            accepted_by: None,
            accepted_at: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        };
        let mut accepted_wave = wave.clone();
        accepted_wave.status = WaveStatus::Completed;
        accepted_wave.accepted_run_id = Some("team-run-1".into());
        accepted_wave.gate_status = WaveGateStatus::Accepted;
        accepted_wave.accepted_by = Some("host".into());
        accepted_wave.accepted_at = Some("unix-ms:2".into());
        accepted_wave.updated_at = "unix-ms:2".into();
        store.append_wave(&wave).expect("append wave");
        store
            .append_wave(&accepted_wave)
            .expect("append accepted wave");

        assert_eq!(store.missions().expect("raw missions").len(), 2);
        assert_eq!(store.waves().expect("raw waves").len(), 2);
        assert_eq!(
            store.latest_missions().expect("latest missions"),
            vec![updated_mission]
        );
        assert_eq!(
            store.latest_waves().expect("latest waves"),
            vec![accepted_wave]
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn native_wave_attempt_and_event_updates_are_concurrency_safe() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-native-concurrency-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = Arc::new(HarnessStore::new(&root));
        store
            .insert_mission(&Mission {
                id: "mission-concurrent".into(),
                title: "Concurrent Mission".into(),
                objective: "Keep native joins lossless".into(),
                desired_outcome: None,
                status: MissionStatus::Planned,
                wave_ids: Vec::new(),
                outcome_summary: None,
                completed_by: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            })
            .expect("insert mission");

        let wave_barrier = Arc::new(Barrier::new(2));
        let wave_handles = ["wave-a", "wave-b"].map(|id| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&wave_barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.insert_wave_and_update_mission(
                    Wave {
                        id: id.into(),
                        mission_id: "mission-concurrent".into(),
                        index: 0,
                        title: id.into(),
                        objective: "one ordered wave".into(),
                        exit_criteria: None,
                        status: WaveStatus::Planned,
                        executor_kind: WaveExecutorKind::AgentTeam,
                        executor_run_ids: Vec::new(),
                        accepted_run_id: None,
                        plan_note: None,
                        outcome_summary: None,
                        artifact_refs: Vec::new(),
                        gate_status: WaveGateStatus::Pending,
                        gate_note: None,
                        accepted_by: None,
                        accepted_at: None,
                        created_at: "unix-ms:2".into(),
                        updated_at: "unix-ms:2".into(),
                    },
                    None,
                    "unix-ms:2",
                )
            })
        });
        for handle in wave_handles {
            handle.join().expect("wave thread").expect("insert wave");
        }
        let waves = store.latest_waves().expect("latest waves");
        assert_eq!(
            waves.iter().map(|wave| wave.index).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let mission = store.latest_missions().expect("latest missions").remove(0);
        assert_eq!(
            mission.wave_ids,
            vec![waves[0].id.clone(), waves[1].id.clone()]
        );

        let mut max_index_wave = waves[0].clone();
        max_index_wave.id = "wave-max-index".into();
        max_index_wave.index = u32::MAX;
        max_index_wave.executor_run_ids.clear();
        store
            .insert_wave_and_update_mission(max_index_wave.clone(), Some(u32::MAX), "unix-ms:2")
            .expect("insert maximum explicit wave index");
        let mut overflow_wave = max_index_wave;
        overflow_wave.id = "wave-overflow".into();
        let error = store
            .insert_wave_and_update_mission(overflow_wave, None, "unix-ms:2")
            .expect_err("implicit wave index must not overflow");
        assert!(
            error.to_string().contains("index space is exhausted"),
            "error: {error}"
        );

        let wave_id = waves[0].id.clone();
        let run_barrier = Arc::new(Barrier::new(2));
        let run_handles = ["team-run-a", "team-run-b"].map(|id| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&run_barrier);
            let wave_id = wave_id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.insert_team_run_and_register_attempt(
                    &AgentTeamRun {
                        id: id.into(),
                        definition_id: None,
                        previous_run_id: None,
                        mission_id: Some("mission-concurrent".into()),
                        wave_id: Some(wave_id),
                        host_surface: "test".into(),
                        host_thread_id: None,
                        objective: "attempt".into(),
                        status: TeamRunStatus::Planning,
                        member_run_ids: vec![format!("member-{id}")],
                        budget_limit_usd: None,
                        created_at: "unix-ms:3".into(),
                        updated_at: "unix-ms:3".into(),
                        completed_at: None,
                    },
                    "unix-ms:3",
                )
            })
        });
        let run_results = run_handles
            .into_iter()
            .map(|handle| handle.join().expect("run thread"))
            .collect::<Vec<_>>();
        assert_eq!(
            run_results.iter().filter(|result| result.is_ok()).count(),
            1
        );
        assert_eq!(
            run_results.iter().filter(|result| result.is_err()).count(),
            1
        );
        let wave = store
            .latest_waves()
            .expect("latest waves")
            .into_iter()
            .find(|wave| wave.id == wave_id)
            .expect("attempt wave");
        assert_eq!(wave.executor_run_ids.len(), 1);
        let event_run_id = wave.executor_run_ids[0].clone();

        let event_barrier = Arc::new(Barrier::new(8));
        let event_handles = (0..8)
            .map(|index| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&event_barrier);
                let event_run_id = event_run_id.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append_team_run_event_next(TeamRunEvent {
                        id: format!("event-{index}"),
                        seq: 0,
                        team_run_id: event_run_id,
                        source_kind: TeamRunEventSourceKind::Host,
                        member_run_id: None,
                        delegation_run_id: None,
                        entity_type: "message".into(),
                        entity_id: format!("message-{index}"),
                        operation: "created".into(),
                        summary: "concurrent".into(),
                        occurred_at: "unix-ms:4".into(),
                    })
                })
            })
            .collect::<Vec<_>>();
        for handle in event_handles {
            handle.join().expect("event thread").expect("append event");
        }
        let mut seqs = store
            .team_run_events()
            .expect("events")
            .into_iter()
            .map(|event| event.seq)
            .collect::<Vec<_>>();
        seqs.sort_unstable();
        assert_eq!(seqs, (1..=8).collect::<Vec<_>>());

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
                    let mission = Mission {
                        id: format!("mission-{worker}-{index}"),
                        title: "Concurrent".into(),
                        objective: "Exercise concurrent append integrity".into(),
                        desired_outcome: None,
                        status: MissionStatus::Running,
                        wave_ids: Vec::new(),
                        outcome_summary: None,
                        completed_by: None,
                        created_at: "2026-05-26T00:00:00Z".into(),
                        updated_at: "2026-05-26T00:00:00Z".into(),
                        completed_at: None,
                    };
                    store.append_mission(&mission).expect("append mission");
                }
            }));
        }

        for handle in handles {
            handle.join().expect("worker thread");
        }

        let missions = store.missions().expect("read missions");
        assert_eq!(missions.len(), worker_count * appends_per_worker);
        let ids = missions
            .iter()
            .map(|mission| mission.id.clone())
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
        let mission = Mission {
            id: "mission-stale-lock".into(),
            title: "Stale lock".into(),
            objective: "Verify an unlocked existing lock file is reusable".into(),
            desired_outcome: None,
            status: MissionStatus::Running,
            wave_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "2026-05-26T00:00:00Z".into(),
            updated_at: "2026-05-26T00:00:00Z".into(),
            completed_at: None,
        };

        store
            .append_mission(&mission)
            .expect("append with unlocked lock file");
        assert_eq!(store.missions().expect("read missions"), vec![mission]);

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
            mission_id: Some("mission-1".into()),
            wave_id: Some("wave-2".into()),
            host_surface: "codex-app".into(),
            host_thread_id: Some("thread-1".into()),
            objective: "Ship the feature".into(),
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-1".into()],
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
        assert!(sparse.definition_id.is_none());
        assert!(sparse.previous_run_id.is_none());
        assert!(sparse.mission_id.is_none());
        assert!(sparse.wave_id.is_none());
        assert!(sparse.host_thread_id.is_none());
        assert!(sparse.member_run_ids.is_empty());
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
            provider_profile: None,
            status: MemberRunStatus::Running,
            provider_session_id: Some("ps-1".into()),
            acp_session_id: Some("acp-1".into()),
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
            provider_call_id: Some("tool-1".into()),
            action_type: "tool_completed".into(),
            status: MemberActionStatus::Succeeded,
            provider_status: Some("completed".into()),
            semantic_status: Some("succeeded".into()),
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
            kind: MessageKind::Assignment,
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

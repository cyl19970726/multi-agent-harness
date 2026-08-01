use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::{
    AgentEvent, AgentMember, AgentMessageRoute, AgentRuntime, AgentTeam, AgentTeamRun, Decision,
    DelegationRun, Evidence, Gap, MemberAction, MemberRun, Message, MessageDelivery,
    MessageDeliveryStatus, MessageTerminalSource, Mission, MissionStatus, PendingInteraction,
    Proposal, ProviderChildThread, ProviderExecutionStatus, Review, TeamDeliveryPolicy,
    TeamDeliveryStatus, TeamMemberCloseRequest, TeamMemberCloseStatus, TeamMessage,
    TeamMessageKind, TeamRunEvent, TeamRunStatus, TeamSupervisorLease, TeamSupervisorLeaseStatus,
    Vision, Wave, WaveExecutorKind, WaveGateStatus, WaveStatus, WorkflowArtifactManifest,
    WorkflowPatch, WorkflowRun, WorkflowStep,
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
    BlockedByDelivery(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamMessageDeliveryClaimResult {
    Claimed(Box<TeamMessage>),
    NotQueued,
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
        fs::create_dir_all(self.root.join("prompts"))?;
        fs::create_dir_all(self.root.join("runtimes"))?;
        Ok(())
    }

    pub fn append_mission(&self, value: &Mission) -> StoreResult<()> {
        self.append_jsonl("missions.jsonl", value)
    }

    /// Compare-and-append one Mission revision. Used for context and AgentTeam
    /// relation edits so concurrent Host decisions cannot overwrite each
    /// other.
    pub fn compare_and_append_mission(
        &self,
        expected: &Mission,
        next: &Mission,
    ) -> StoreResult<()> {
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
        if next.id != current.id
            || next.created_at != current.created_at
            || next.wave_ids != current.wave_ids
        {
            return Err(StoreError::Conflict(
                "mission revision must preserve identity, creation time, and Wave membership"
                    .to_string(),
            ));
        }
        let teams = latest_by_id(self.read_jsonl::<AgentTeam>("teams.jsonl")?, |team| {
            team.id.clone()
        });
        for team_id in &next.agent_team_ids {
            if !teams.contains_key(team_id) {
                return Err(StoreError::Conflict(format!(
                    "agent team not found: {team_id}"
                )));
            }
        }
        self.append_jsonl_unlocked("missions.jsonl", next)
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

    /// Atomically promote one stable Agent Inbox message into a concrete
    /// MemberRun mailbox. The source Message remains durable identity-level
    /// truth; its latest status records that the router accepted it.
    pub fn route_agent_message_to_team(
        &self,
        route: &AgentMessageRoute,
        team_message: &TeamMessage,
    ) -> StoreResult<AgentMessageRoute> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;

        if let Some(existing) = latest_by_id(
            self.read_jsonl::<AgentMessageRoute>("agent_message_routes.jsonl")?,
            |route| route.agent_message_id.clone(),
        )
        .remove(&route.agent_message_id)
        {
            return Ok(existing);
        }
        let mut source = latest_by_id(self.read_jsonl::<Message>("messages.jsonl")?, |message| {
            message.id.clone()
        })
        .remove(&route.agent_message_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "Agent Inbox message not found: {}",
                route.agent_message_id
            ))
        })?;
        if source.to_agent_id.as_deref() != Some(route.agent_member_id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "message {} is not addressed to Agent {}",
                source.id, route.agent_member_id
            )));
        }
        if source.delivery_status != MessageDeliveryStatus::Queued {
            return Err(StoreError::Conflict(format!(
                "message {} is not queued for routing",
                source.id
            )));
        }
        let member = latest_by_id(
            self.read_jsonl::<MemberRun>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&route.member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("MemberRun not found: {}", route.member_run_id))
        })?;
        if member.team_run_id != route.team_run_id
            || member.agent_member_id.as_deref() != Some(route.agent_member_id.as_str())
        {
            return Err(StoreError::Conflict(format!(
                "MemberRun {} is not the Agent {} runtime in TeamRun {}",
                route.member_run_id, route.agent_member_id, route.team_run_id
            )));
        }
        if team_message.id != route.team_message_id
            || team_message.team_run_id != route.team_run_id
            || !team_message
                .to_member_ids
                .iter()
                .any(|id| id == &route.member_run_id)
        {
            return Err(StoreError::Conflict(
                "Agent Inbox route and TeamMessage target do not match".to_string(),
            ));
        }
        let team_messages = latest_by_id(
            self.read_jsonl::<TeamMessage>("team_messages.jsonl")?,
            |message| message.id.clone(),
        );
        if team_messages.contains_key(&team_message.id) {
            return Err(StoreError::Conflict(format!(
                "team message already exists: {}",
                team_message.id
            )));
        }
        if team_message.kind == harness_core::TeamMessageKind::Assignment
            && team_messages.values().any(|message| {
                message.team_run_id == team_message.team_run_id
                    && message.kind == harness_core::TeamMessageKind::Assignment
                    && message.correlation_id == team_message.correlation_id
            })
        {
            return Err(StoreError::Conflict(format!(
                "correlation_id `{}` already identifies an assignment in team run {}",
                team_message.correlation_id, team_message.team_run_id
            )));
        }

        source.delivery_status = MessageDeliveryStatus::Acknowledged;
        self.append_jsonl_unlocked("team_messages.jsonl", team_message)?;
        self.append_jsonl_unlocked("messages.jsonl", &source)?;
        self.append_jsonl_unlocked("agent_message_routes.jsonl", route)?;
        Ok(route.clone())
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

    /// Compare-and-append one TeamRun revision.
    ///
    /// Host binding is mutable coordination metadata, but changing it must not
    /// silently overwrite a concurrent lifecycle/member update. Keep the
    /// identity, execution scope, and creation time stable while allowing the
    /// caller to revise addressability fields and `updated_at`.
    pub fn compare_and_append_team_run(
        &self,
        expected: &AgentTeamRun,
        next: &AgentTeamRun,
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
                "team run {} changed concurrently; retry the operation",
                expected.id
            )));
        }
        if next.id != current.id
            || next.created_at != current.created_at
            || next.mission_id != current.mission_id
            || next.wave_id != current.wave_id
            || next.agent_team_id != current.agent_team_id
            || next.definition_id != current.definition_id
            || next.previous_run_id != current.previous_run_id
            || next.execution_root != current.execution_root
            || next.member_run_ids != current.member_run_ids
            || next.status != current.status
            || next.objective != current.objective
            || next.budget_limit_usd != current.budget_limit_usd
            || next.completed_at != current.completed_at
        {
            return Err(StoreError::Conflict(
                "Host binding revision must preserve TeamRun identity, scope, members, lifecycle, and objective"
                    .to_string(),
            ));
        }
        self.append_jsonl_unlocked("team_runs.jsonl", next)
    }

    /// Atomically append a newly-created TeamRun. Mission-scoped runs are the
    /// primary path and intentionally have no Wave id. Rows with both ids are
    /// retained only for legacy direct-Wave executor compatibility.
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
            (None, Some(_)) => Err(StoreError::Conflict(
                "a TeamRun with wave_id must also name that Wave's Mission".to_string(),
            )),
            (Some(mission_id), None) => {
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
                        "mission {mission_id} is {:?} and cannot start another TeamRun",
                        mission.status
                    )));
                }
                if let Some(team_id) = value.agent_team_id.as_deref() {
                    if !mission.agent_team_ids.iter().any(|id| id == team_id) {
                        return Err(StoreError::Conflict(format!(
                            "agent team {team_id} is not linked to mission {mission_id}"
                        )));
                    }
                }
                self.append_jsonl_unlocked("team_runs.jsonl", value)
            }
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
        if value.kind == TeamMessageKind::Handoff {
            let existing_handoffs = messages
                .values()
                .filter(|message| {
                    message.team_run_id == value.team_run_id
                        && message.from_member_id == value.from_member_id
                        && message.kind == TeamMessageKind::Handoff
                        && message.correlation_id == value.correlation_id
                })
                .collect::<Vec<_>>();
            let duplicate_trigger = value.causation_id.as_deref().is_some_and(|causation_id| {
                existing_handoffs
                    .iter()
                    .any(|message| message.causation_id.as_deref() == Some(causation_id))
            });
            let duplicate_inject_continuation =
                value.causation_id.as_deref().is_some_and(|causation_id| {
                    messages.get(causation_id).is_some_and(|control| {
                        control.team_run_id == value.team_run_id
                            && control.kind == TeamMessageKind::Control
                            && control.correlation_id == value.correlation_id
                            && control.deliveries.iter().any(|delivery| {
                                delivery.member_id == value.from_member_id
                                    && delivery.policy == TeamDeliveryPolicy::Inject
                                    && matches!(
                                        delivery.status,
                                        TeamDeliveryStatus::Delivered
                                            | TeamDeliveryStatus::Acknowledged
                                    )
                            })
                            && control
                                .causation_id
                                .as_deref()
                                .is_some_and(|control_cause_id| {
                                    existing_handoffs
                                        .iter()
                                        .any(|message| message.id == control_cause_id)
                                })
                    })
                });
            if duplicate_trigger || duplicate_inject_continuation {
                return Err(StoreError::Conflict(format!(
                    "MemberRun {} already handed off correlation `{}` for this provider turn",
                    value.from_member_id, value.correlation_id
                )));
            }
        }
        // A pending inbound delivery that explicitly requires a response
        // fences a same-correlation Handoff as stale. Informational or
        // acknowledgement-only mail does not start rounds, so it must not
        // fence either — otherwise a Handoff would deadlock behind mail that
        // is intentionally never driven on its own (ADR 0046 §4).
        if value.kind == harness_core::TeamMessageKind::Handoff
            && messages.values().any(|message| {
                message.team_run_id == value.team_run_id
                    && message.correlation_id == value.correlation_id
                    && message.from_member_id != value.from_member_id
                    && message.requires_response()
                    && message.deliveries.iter().any(|delivery| {
                        delivery.member_id == value.from_member_id
                            && matches!(
                                delivery.status,
                                TeamDeliveryStatus::Queued | TeamDeliveryStatus::Claimed
                            )
                    })
            })
        {
            return Err(StoreError::Conflict(format!(
                "MemberRun {} cannot hand off correlation `{}` while newer inbound mail is queued or claimed",
                value.from_member_id, value.correlation_id
            )));
        }
        self.append_jsonl_unlocked("team_messages.jsonl", value)
    }

    /// Acquire the one durable Supervisor lease for a TeamRun. An active,
    /// unexpired lease held by another Supervisor rejects the attach before any
    /// provider side effect. Reacquisition after expiry increments generation.
    #[allow(clippy::too_many_arguments)]
    pub fn acquire_team_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        owner_process_id: u32,
        owner_locator: &str,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let run_exists = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .contains_key(team_run_id);
        if !run_exists {
            return Err(StoreError::Conflict(format!(
                "team run not found: {team_run_id}"
            )));
        }
        let current = self.latest_lease_for_run_unlocked(team_run_id)?;
        if let Some(current) = current.as_ref() {
            if current.status == TeamSupervisorLeaseStatus::Active
                && current.expires_unix_ms > now_unix_ms
                && current.supervisor_id != supervisor_id
            {
                return Err(StoreError::Conflict(format!(
                    "team run {team_run_id} is supervised by {} generation {} until unix-ms:{}",
                    current.supervisor_id, current.generation, current.expires_unix_ms
                )));
            }
            if current.status == TeamSupervisorLeaseStatus::Active
                && current.expires_unix_ms > now_unix_ms
                && current.supervisor_id == supervisor_id
            {
                return Ok(current.clone());
            }
        }
        let generation = current
            .as_ref()
            .map(|lease| lease.generation.saturating_add(1))
            .unwrap_or(1);
        let lease = TeamSupervisorLease {
            team_run_id: team_run_id.to_string(),
            supervisor_id: supervisor_id.to_string(),
            generation,
            owner_process_id,
            owner_locator: owner_locator.to_string(),
            status: TeamSupervisorLeaseStatus::Active,
            acquired_unix_ms: now_unix_ms,
            heartbeat_unix_ms: now_unix_ms,
            expires_unix_ms: now_unix_ms.saturating_add(ttl_ms.max(1)),
            released_unix_ms: None,
        };
        // Acquisition is rare (one per Supervisor generation) while heartbeats
        // are ~1/s, so this is where compaction belongs.
        self.compact_supervisor_leases_unlocked()?;
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn renew_team_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        generation: u64,
        now_unix_ms: u64,
        ttl_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "team run {team_run_id} has no Supervisor lease to renew"
                ))
            })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "Supervisor lease for team run {team_run_id} is no longer owned by {supervisor_id} generation {generation}"
            )));
        }
        lease.heartbeat_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms.saturating_add(ttl_ms.max(1));
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    pub fn release_team_supervisor_lease(
        &self,
        team_run_id: &str,
        supervisor_id: &str,
        generation: u64,
        now_unix_ms: u64,
    ) -> StoreResult<TeamSupervisorLease> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no Supervisor lease to release"
            ))
        })?;
        if lease.supervisor_id != supervisor_id || lease.generation != generation {
            return Err(StoreError::Conflict(format!(
                "Supervisor lease for team run {team_run_id} belongs to {} generation {}, not {supervisor_id} generation {generation}",
                lease.supervisor_id, lease.generation
            )));
        }
        if lease.status == TeamSupervisorLeaseStatus::Released {
            return Ok(lease);
        }
        lease.status = TeamSupervisorLeaseStatus::Released;
        lease.heartbeat_unix_ms = now_unix_ms;
        lease.expires_unix_ms = now_unix_ms;
        lease.released_unix_ms = Some(now_unix_ms);
        self.append_jsonl_unlocked("team_supervisor_leases.jsonl", &lease)?;
        Ok(lease)
    }

    /// Persist a Host Close before touching the process-local provider handle.
    /// Repeated requests while one is pending are idempotent.
    pub fn latch_team_member_close(
        &self,
        value: &TeamMemberCloseRequest,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let member = latest_by_id(
            self.read_jsonl::<MemberRun>("member_runs.jsonl")?,
            |member| member.id.clone(),
        )
        .remove(&value.member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!("MemberRun not found: {}", value.member_run_id))
        })?;
        if member.team_run_id != value.team_run_id {
            return Err(StoreError::Conflict(format!(
                "MemberRun {} belongs to {}, not {}",
                value.member_run_id, member.team_run_id, value.team_run_id
            )));
        }
        if let Some(current) = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(&value.member_run_id)
        {
            if current.status == TeamMemberCloseStatus::Pending {
                return Ok(current);
            }
        }
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", value)?;
        Ok(value.clone())
    }

    /// Mark one durable Close as applied after the MemberRun is stopped.
    pub fn complete_team_member_close(
        &self,
        team_run_id: &str,
        member_run_id: &str,
        request_id: &str,
        applied_at: &str,
    ) -> StoreResult<TeamMemberCloseRequest> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut request = latest_by_id(
            self.read_jsonl::<TeamMemberCloseRequest>("team_member_close_requests.jsonl")?,
            |request| request.member_run_id.clone(),
        )
        .remove(member_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "MemberRun {member_run_id} has no durable Close request"
            ))
        })?;
        if request.team_run_id != team_run_id || request.id != request_id {
            return Err(StoreError::Conflict(format!(
                "Close request {request_id} does not own MemberRun {member_run_id} in TeamRun {team_run_id}"
            )));
        }
        if request.status == TeamMemberCloseStatus::Applied {
            return Ok(request);
        }
        request.status = TeamMemberCloseStatus::Applied;
        request.applied_at = Some(applied_at.to_string());
        self.append_jsonl_unlocked("team_member_close_requests.jsonl", &request)?;
        Ok(request)
    }

    /// Claim one queued TeamMessage delivery under the same durable lock used
    /// for the Supervisor lease. A claim must be completed with a real provider
    /// receipt or explicitly reconciled; it is never auto-requeued on expiry.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        now_unix_ms: u64,
        claim_ttl_ms: u64,
        updated_at: &str,
    ) -> StoreResult<TeamMessageDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut message = match latest_by_id(
            self.read_jsonl::<TeamMessage>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        {
            Some(message) if message.team_run_id == team_run_id => message,
            _ => return Ok(TeamMessageDeliveryClaimResult::NotQueued),
        };
        let Some(delivery) = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
        else {
            return Ok(TeamMessageDeliveryClaimResult::NotQueued);
        };
        if delivery.status != TeamDeliveryStatus::Queued {
            return Ok(TeamMessageDeliveryClaimResult::NotQueued);
        }
        delivery.status = TeamDeliveryStatus::Claimed;
        delivery.attempt = delivery.attempt.saturating_add(1);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_by_supervisor_id = Some(supervisor_id.to_string());
        delivery.claimed_generation = Some(supervisor_generation);
        delivery.claimed_unix_ms = Some(now_unix_ms);
        delivery.claim_expires_unix_ms = Some(now_unix_ms.saturating_add(claim_ttl_ms.max(1)));
        delivery.provider_receipt_id = None;
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(TeamMessageDeliveryClaimResult::Claimed(Box::new(message)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_team_message_delivery_claim(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<TeamMessage> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id)
        .ok_or_else(|| {
            StoreError::Conflict(format!(
                "team run {team_run_id} has no active Supervisor lease"
            ))
        })?;
        if lease.status != TeamSupervisorLeaseStatus::Active
            || lease.supervisor_id != supervisor_id
            || lease.generation != supervisor_generation
            || lease.expires_unix_ms <= now_unix_ms
        {
            return Err(StoreError::Conflict(format!(
                "team run {team_run_id} Supervisor lease is not owned by {supervisor_id} generation {supervisor_generation}"
            )));
        }
        let mut message = latest_by_id(
            self.read_jsonl::<TeamMessage>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;
        if delivery.status == TeamDeliveryStatus::Delivered
            && delivery.claim_id.as_deref() == Some(claim_id)
        {
            return Ok(message);
        }
        if delivery.status != TeamDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
            || delivery.claimed_by_supervisor_id.as_deref() != Some(supervisor_id)
            || delivery.claimed_generation != Some(supervisor_generation)
        {
            return Err(StoreError::Conflict(format!(
                "delivery claim {claim_id} no longer owns message {message_id} for {member_run_id}"
            )));
        }
        delivery.status = TeamDeliveryStatus::Delivered;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
    }

    /// Atomically acknowledge one already-delivered TeamMessage recipient.
    ///
    /// ACK does not require a live Supervisor because the Host or operator may
    /// read and acknowledge mail while the provider runtime is idle or down.
    /// It does require a real delivered receipt and never advances a queued or
    /// uncertain claim.
    pub fn acknowledge_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        updated_at: &str,
    ) -> StoreResult<TeamMessage> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut message = latest_by_id(
            self.read_jsonl::<TeamMessage>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;
        match delivery.status {
            TeamDeliveryStatus::Acknowledged => return Ok(message),
            TeamDeliveryStatus::Delivered => {}
            TeamDeliveryStatus::Queued | TeamDeliveryStatus::Claimed => {
                return Err(StoreError::Conflict(format!(
                    "message {message_id} has not been delivered to {member_run_id}"
                )));
            }
            TeamDeliveryStatus::Failed | TeamDeliveryStatus::Expired => {
                return Err(StoreError::Conflict(format!(
                    "message {message_id} delivery to {member_run_id} cannot be acknowledged from {:?}",
                    delivery.status
                )));
            }
        }
        delivery.status = TeamDeliveryStatus::Acknowledged;
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
    }

    /// Resolve a claimed delivery after a crash. `provider_accepted=true`
    /// records a reviewed native receipt; false explicitly returns it to the
    /// queue. No automatic timeout path calls this method.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_team_message_delivery_claim(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        claim_id: &str,
        provider_accepted: bool,
        provider_receipt_id: Option<&str>,
        updated_at: &str,
    ) -> StoreResult<TeamMessage> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut message = latest_by_id(
            self.read_jsonl::<TeamMessage>("team_messages.jsonl")?,
            |message| message.id.clone(),
        )
        .remove(message_id)
        .ok_or_else(|| StoreError::Conflict(format!("team message not found: {message_id}")))?;
        if message.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "message {message_id} belongs to {}, not {team_run_id}",
                message.team_run_id
            )));
        }
        let delivery = message
            .deliveries
            .iter_mut()
            .find(|delivery| delivery.member_id == member_run_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "message {message_id} has no delivery for {member_run_id}"
                ))
            })?;
        if delivery.status != TeamDeliveryStatus::Claimed
            || delivery.claim_id.as_deref() != Some(claim_id)
        {
            return Err(StoreError::Conflict(format!(
                "message {message_id} does not have active claim {claim_id} for {member_run_id}"
            )));
        }
        if provider_accepted {
            let receipt = provider_receipt_id.ok_or_else(|| {
                StoreError::Conflict(
                    "provider-accepted reconciliation requires a native receipt id".to_string(),
                )
            })?;
            delivery.status = TeamDeliveryStatus::Delivered;
            delivery.provider_receipt_id = Some(receipt.to_string());
        } else {
            delivery.status = TeamDeliveryStatus::Queued;
            delivery.claim_id = None;
            delivery.claimed_by_supervisor_id = None;
            delivery.claimed_generation = None;
            delivery.claimed_unix_ms = None;
            delivery.claim_expires_unix_ms = None;
            delivery.provider_receipt_id = None;
        }
        delivery.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("team_messages.jsonl", &message)?;
        Ok(message)
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
            (Some(mission_id), None) => {
                latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
                    mission.id.clone()
                })
                .remove(mission_id)
                .ok_or_else(|| StoreError::Conflict(format!("mission not found: {mission_id}")))?;
                // Mission closeout does not own or stop this run. A linked
                // long-lived Team must still be able to complete, fail, or be
                // cancelled after the Mission records its own outcome.
                None
            }
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
            (None, Some(_)) => {
                return Err(StoreError::Conflict(
                    "TeamRun lifecycle has a wave_id without mission_id".to_string(),
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
    ) -> StoreResult<MessageDeliveryClaimResult> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;

        let latest_messages =
            latest_by_id(self.read_jsonl::<Message>("messages.jsonl")?, |message| {
                message.id.clone()
            });
        if let Some(active) = latest_messages.values().find(|message| {
            message.to_agent_id.as_deref() == Some(agent_member_id)
                && message
                    .delivery
                    .as_ref()
                    .is_some_and(delivery_blocks_another_claim)
        }) {
            let delivery_id = active
                .delivery
                .as_ref()
                .and_then(|delivery| delivery.delivery_id.clone())
                .unwrap_or_else(|| active.id.clone());
            return Ok(MessageDeliveryClaimResult::BlockedByDelivery(delivery_id));
        }
        let Some(mut message) = latest_messages.get(message_id).cloned() else {
            return Ok(MessageDeliveryClaimResult::NotQueued);
        };
        if message.to_agent_id.as_deref() != Some(agent_member_id)
            || message.delivery_status != MessageDeliveryStatus::Queued
        {
            return Ok(MessageDeliveryClaimResult::NotQueued);
        }

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

    pub fn agent_message_routes(&self) -> StoreResult<Vec<AgentMessageRoute>> {
        self.read_jsonl("agent_message_routes.jsonl")
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

    pub fn team_supervisor_leases(&self) -> StoreResult<Vec<TeamSupervisorLease>> {
        self.read_jsonl("team_supervisor_leases.jsonl")
    }

    pub fn latest_team_supervisor_lease(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<TeamSupervisorLease>> {
        Ok(latest_by_id(self.team_supervisor_leases()?, |lease| {
            lease.team_run_id.clone()
        })
        .remove(team_run_id))
    }

    pub fn team_member_close_requests(&self) -> StoreResult<Vec<TeamMemberCloseRequest>> {
        self.read_jsonl("team_member_close_requests.jsonl")
    }

    pub fn latest_team_member_close_request(
        &self,
        member_run_id: &str,
    ) -> StoreResult<Option<TeamMemberCloseRequest>> {
        Ok(latest_by_id(self.team_member_close_requests()?, |request| {
            request.member_run_id.clone()
        })
        .remove(member_run_id))
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

    /// Read only the trailing `window` bytes of a JSONL file, dropping the first
    /// (possibly partial) line unless the window covers the whole file.
    ///
    /// Only valid for latest-wins projections keyed by a field, where the answer
    /// is the LAST matching row. Callers must fall back to `read_jsonl` when the
    /// key is absent from the tail — absence in the window proves nothing.
    ///
    /// Motivation: Supervisor lease heartbeats append ~1 row/s per live run and
    /// every renewal re-parsed the entire file under the global write lock,
    /// making heartbeat cost O(N) and cumulative cost O(N²). Measured on
    /// `star-harness-dogfood`: 71,524 rows / 23 MB, 15,101 renewals in 4.77 h
    /// for one run, with observed renewal drift (p50 1135 ms against a 1000 ms
    /// sleep) already showing the parse cost.
    fn read_jsonl_tail<T: DeserializeOwned>(
        &self,
        file_name: &str,
        window: u64,
    ) -> StoreResult<Vec<T>> {
        let path = self.root.join(file_name);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let start = len.saturating_sub(window);
        // Whether the byte before `start` is a newline decides if `start`
        // already sits on a row boundary. Discarding unconditionally would drop
        // a COMPLETE row whenever the window happens to land there, which costs
        // a needless full-scan fallback (and would silently lose a row for any
        // future caller that does not have one).
        let starts_on_boundary = if start == 0 {
            true
        } else {
            file.seek(SeekFrom::Start(start - 1))?;
            let mut prev = [0u8; 1];
            std::io::Read::read_exact(&mut file, &mut prev)?;
            prev[0] == b'\n'
        };
        file.seek(SeekFrom::Start(start))?;
        let mut values = Vec::new();
        let mut lines = BufReader::new(file).lines();
        if !starts_on_boundary {
            // Discard the torn first line inside the window.
            let _ = lines.next();
        }
        for line in lines {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            values.push(serde_json::from_str(&line)?);
        }
        Ok(values)
    }

    /// Latest lease for one run: tail window first, full scan only on miss.
    fn latest_lease_for_run_unlocked(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<TeamSupervisorLease>> {
        const TAIL_WINDOW_BYTES: u64 = 256 * 1024;
        let tail = self.read_jsonl_tail::<TeamSupervisorLease>(
            "team_supervisor_leases.jsonl",
            TAIL_WINDOW_BYTES,
        )?;
        // rfind, not filter().next_back(): latest-wins means the LAST matching
        // row in the window, and rfind scans from the back so it stops at the
        // first hit instead of walking the whole window.
        if let Some(found) = tail
            .into_iter()
            .rfind(|lease| lease.team_run_id == team_run_id)
        {
            return Ok(Some(found));
        }
        Ok(latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id))
    }

    /// Collapse the lease file to one row per run (latest wins).
    ///
    /// Called on acquisition, which is rare (one per Supervisor generation),
    /// while heartbeats are frequent. Bounds the file at ~#runs rows so the
    /// tail window above always hits and the file stops growing without bound.
    /// Generation fencing is unaffected: the retained row is exactly the row a
    /// full-scan latest-wins projection would have produced.
    fn compact_supervisor_leases_unlocked(&self) -> StoreResult<()> {
        let path = self.root.join("team_supervisor_leases.jsonl");
        if !path.exists() {
            return Ok(());
        }
        let all = self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?;
        let latest = latest_by_id(all, |lease| lease.team_run_id.clone());
        let temp = self.root.join("team_supervisor_leases.jsonl.compact");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp)?;
            for lease in latest.values() {
                let mut row = Vec::new();
                serde_json::to_writer(&mut row, lease)?;
                row.push(b'\n');
                file.write_all(&row)?;
            }
            file.flush()?;
            file.sync_all()?;
        }
        fs::rename(&temp, &path)?;
        // fsync the PARENT DIRECTORY, not just the temp inode. POSIX allows a
        // crash to recover either the old or the new directory entry after a
        // rename; only syncing the directory makes the replacement durable.
        // Without it a system crash can resurrect the pre-compaction file and
        // with it an already-issued generation, violating the monotonic
        // higher-generation contract in ADR 0044.
        if let Ok(dir) = File::open(&self.root) {
            let _ = dir.sync_all();
        }
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

        // A writer holds the store flock, but ordinary projections deliberately
        // do not: Dashboard/API reads must remain concurrent with one another.
        // `write_all` still may expose a short prefix before the trailing
        // newline becomes visible, so take a byte snapshot and retry only that
        // unmistakably incomplete final-row state. A complete snapshot is
        // immutable in memory even if another append starts immediately after.
        // The bounded retry preserves honest corruption reporting for a file
        // that remains truncated instead of silently dropping its final row.
        const INCOMPLETE_ROW_RETRY: Duration = Duration::from_secs(1);
        const INCOMPLETE_ROW_POLL: Duration = Duration::from_millis(5);
        let deadline = Instant::now() + INCOMPLETE_ROW_RETRY;
        let snapshot = loop {
            let bytes = fs::read(&path)?;
            if bytes.is_empty() || bytes.ends_with(b"\n") || Instant::now() >= deadline {
                break bytes;
            }
            thread::sleep(INCOMPLETE_ROW_POLL);
        };

        let mut values = Vec::new();
        for line in snapshot.split(|byte| *byte == b'\n') {
            if line.iter().all(|byte| byte.is_ascii_whitespace()) {
                continue;
            }
            values.push(serde_json::from_slice(line)?);
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

fn delivery_blocks_another_claim(delivery: &MessageDelivery) -> bool {
    matches!(
        delivery.execution_status,
        Some(ProviderExecutionStatus::Queued | ProviderExecutionStatus::Running)
    ) || (delivery.execution_status == Some(ProviderExecutionStatus::Stale)
        && delivery.terminal_source != Some(MessageTerminalSource::Failed))
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
        DelegationMode, DelegationStatus, MemberActionStatus, MemberRunStatus,
        MemberWorkspaceSnapshot, MessageKind, Mission, MissionStatus, SenderKind,
        TeamDeliveryPolicy, TeamDeliveryStatus, TeamMessageDelivery, TeamMessageKind,
        TeamMessageResponseIntent, TeamRunEventSourceKind, TeamRunStatus, Wave, WaveExecutorKind,
        WaveGateStatus, WaveStatus,
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
            context: String::new(),
            desired_outcome: Some("A compatible, durable contract".into()),
            status: MissionStatus::Planned,
            wave_ids: vec!["wave-1".into()],
            agent_team_ids: Vec::new(),
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
            context: String::new(),
            revision: 1,
            updated_by: Some("host".into()),
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
                context: String::new(),
                desired_outcome: None,
                status: MissionStatus::Planned,
                wave_ids: Vec::new(),
                agent_team_ids: Vec::new(),
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
                        context: String::new(),
                        revision: 1,
                        updated_by: Some("host".into()),
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
                        agent_team_id: None,
                        previous_run_id: None,
                        mission_id: Some("mission-concurrent".into()),
                        wave_id: Some(wave_id),
                        project_binding_id: Some("project-concurrent".into()),
                        host_surface: "test".into(),
                        host_thread_id: None,
                        host_actor: None,
                        host_control_mode: Default::default(),
                        objective: "attempt".into(),
                        execution_root: Some("/projects/concurrent".into()),
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
                        context: String::new(),
                        desired_outcome: None,
                        status: MissionStatus::Running,
                        wave_ids: Vec::new(),
                        agent_team_ids: Vec::new(),
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
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Running,
            wave_ids: Vec::new(),
            agent_team_ids: Vec::new(),
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
            .claim_queued_message_delivery("agent-1", "message-1", test_delivery("delivery-1"))
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
                .and_then(|delivery| delivery.delivery_id),
            Some("delivery-1".into())
        );

        let second_claim = store
            .claim_queued_message_delivery("agent-1", "message-2", test_delivery("delivery-2"))
            .expect("second claim");
        assert_eq!(
            second_claim,
            MessageDeliveryClaimResult::BlockedByDelivery("delivery-1".into())
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    /// Durability: a claim writes and fsyncs the Acknowledged message row with
    /// its Running delivery attempt, and a *separate* store handle opened
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
            .claim_queued_message_delivery("agent-d", "message-d", test_delivery("delivery-d"))
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

        let delivery = message.delivery.expect("delivery attempt survives reopen");
        assert_eq!(delivery.delivery_id.as_deref(), Some("delivery-d"));
        assert_eq!(
            delivery.execution_status,
            Some(ProviderExecutionStatus::Running)
        );

        // The reopened store must refuse to re-claim: because both the
        // Acknowledged message row and its Running delivery attempt survived
        // the fsync, the re-claim is rejected (the active attempt for this
        // agent blocks delivery; were the row lost it would return Claimed and
        // double-deliver). Either rejection variant proves no double-delivery.
        let reclaim = reopened
            .claim_queued_message_delivery("agent-d", "message-d", test_delivery("delivery-d2"))
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

    #[test]
    fn jsonl_read_retries_a_concurrently_incomplete_final_row() {
        let root = team_test_root("concurrent-partial-row");
        let store = HarnessStore::new(&root);
        store.init().expect("initialize store");
        let path = root.join("concurrent.jsonl");
        let (partial_ready_tx, partial_ready_rx) = std::sync::mpsc::channel();

        let writer_store = store.clone();
        let writer = std::thread::spawn(move || {
            let _lock = writer_store.acquire_write_lock().expect("writer lock");
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .expect("open concurrent ledger");
            file.write_all(br#"{"id":"row-1""#)
                .expect("write partial row");
            file.flush().expect("flush partial row");
            partial_ready_tx.send(()).expect("announce partial row");
            std::thread::sleep(Duration::from_millis(30));
            file.write_all(b",\"value\":1}\n")
                .expect("finish concurrent row");
            file.sync_all().expect("sync concurrent row");
        });

        partial_ready_rx.recv().expect("wait for partial row");
        let rows = store
            .read_jsonl::<serde_json::Value>("concurrent.jsonl")
            .expect("reader waits for the complete final row");
        writer.join().expect("writer completes");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["id"], "row-1");
        assert_eq!(rows[0]["value"], 1);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn seed_lease_run(store: &HarnessStore, id: &str) {
        store
            .append_team_run(&AgentTeamRun {
                id: id.into(),
                definition_id: None,
                agent_team_id: None,
                previous_run_id: None,
                mission_id: None,
                wave_id: None,
                project_binding_id: None,
                host_surface: "codex-app".into(),
                host_thread_id: None,
                host_actor: None,
                host_control_mode: Default::default(),
                objective: "lease test".into(),
                execution_root: None,
                status: TeamRunStatus::Running,
                member_run_ids: Vec::new(),
                budget_limit_usd: None,
                created_at: "unix-ms:1".into(),
                updated_at: "unix-ms:1".into(),
                completed_at: None,
            })
            .expect("seed run");
    }

    /// The tail-window fast path must not change which lease a reader sees,
    /// even when the target row sits far in front of the window.
    #[test]
    fn supervisor_lease_tail_read_agrees_with_full_scan() {
        let root = team_test_root("lease-tail");
        let store = HarnessStore::new(&root);
        seed_lease_run(&store, "run-a");
        seed_lease_run(&store, "run-b");
        store
            .acquire_team_supervisor_lease("run-a", "sup-a", 1, "a", 1_000, 15_000)
            .expect("acquire a");
        store
            .acquire_team_supervisor_lease("run-b", "sup-b", 2, "b", 1_000, 15_000)
            .expect("acquire b");
        // Push run-a's latest row well outside the 256 KiB tail window.
        for tick in 0..4_000u64 {
            store
                .renew_team_supervisor_lease("run-b", "sup-b", 1, 2_000 + tick, 15_000)
                .expect("renew b");
        }

        let tail = store
            .latest_lease_for_run_unlocked("run-a")
            .expect("tail read")
            .expect("run-a lease present");
        let full = latest_by_id(
            store
                .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
                .expect("full scan"),
            |lease| lease.team_run_id.clone(),
        )
        .remove("run-a")
        .expect("run-a in full scan");
        assert_eq!(tail.supervisor_id, full.supervisor_id);
        assert_eq!(tail.generation, full.generation);
        assert_eq!(tail.expires_unix_ms, full.expires_unix_ms);
    }

    /// The tail window may land exactly on a row boundary. Discarding the first
    /// line unconditionally would drop a COMPLETE row; reviewer-reported.
    #[test]
    fn supervisor_lease_tail_keeps_a_row_when_window_lands_on_a_boundary() {
        let root = team_test_root("lease-boundary");
        let store = HarnessStore::new(&root);
        seed_lease_run(&store, "run-a");
        store
            .acquire_team_supervisor_lease("run-a", "sup-a", 1, "a", 1_000, 15_000)
            .expect("acquire");
        for tick in 0..20u64 {
            store
                .renew_team_supervisor_lease("run-a", "sup-a", 1, 1_001 + tick, 15_000)
                .expect("renew");
        }
        let path = root.join("team_supervisor_leases.jsonl");
        let bytes = std::fs::read(&path).expect("read lease file");
        let total = bytes.len() as u64;
        // Start the window exactly at the first byte of the LAST row, i.e. one
        // past the second-to-last newline. The file ends with a newline, so the
        // last newline is the row terminator, not the row start.
        let last_terminator = bytes
            .iter()
            .rposition(|&b| b == b'\n')
            .expect("trailing newline");
        let row_start = bytes[..last_terminator]
            .iter()
            .rposition(|&b| b == b'\n')
            .expect("a previous row") as u64
            + 1;
        let window = total - row_start;
        let rows = store
            .read_jsonl_tail::<TeamSupervisorLease>("team_supervisor_leases.jsonl", window)
            .expect("tail read");
        assert_eq!(
            rows.len(),
            1,
            "a window landing on a row boundary must keep that row, got {}",
            rows.len()
        );
    }

    /// Compaction on acquire bounds the file at one row per run and must keep
    /// generation fencing intact.
    #[test]
    fn supervisor_lease_acquire_compacts_and_keeps_fencing() {
        let root = team_test_root("lease-compact");
        let store = HarnessStore::new(&root);
        seed_lease_run(&store, "run-a");
        store
            .acquire_team_supervisor_lease("run-a", "sup-1", 1, "a", 1_000, 10)
            .expect("acquire gen 1");
        for tick in 0..500u64 {
            store
                .renew_team_supervisor_lease("run-a", "sup-1", 1, 1_001 + tick, 10)
                .expect("renew");
        }
        let before = store
            .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
            .expect("read")
            .len();
        assert!(before > 500, "history should be long before compaction");

        // The lease has expired, so a different Supervisor takes generation 2.
        let gen2 = store
            .acquire_team_supervisor_lease("run-a", "sup-2", 3, "b", 900_000, 15_000)
            .expect("acquire gen 2");
        assert_eq!(gen2.generation, 2);

        // Compaction runs before the new row is appended, so one run yields the
        // collapsed prior row plus the freshly acquired lease. The invariant is
        // that the file is bounded by run count rather than by heartbeat count.
        let after = store
            .read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")
            .expect("read")
            .len();
        assert_eq!(
            after, 2,
            "compaction must bound the file at ~one row per run, got {after} (was {before})"
        );

        // The fenced-out generation must still be rejected after compaction.
        assert!(
            store
                .renew_team_supervisor_lease("run-a", "sup-1", 1, 900_001, 15_000)
                .is_err(),
            "stale generation must not renew"
        );
        let live = store
            .latest_lease_for_run_unlocked("run-a")
            .expect("tail")
            .expect("present");
        assert_eq!(live.supervisor_id, "sup-2");
        assert_eq!(live.generation, 2);
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
            agent_team_id: Some("td-1".into()),
            previous_run_id: Some("tr-0".into()),
            mission_id: Some("mission-1".into()),
            wave_id: Some("wave-2".into()),
            project_binding_id: Some("project-example".into()),
            host_surface: "codex-app".into(),
            host_thread_id: Some("thread-1".into()),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "Ship the feature".into(),
            execution_root: Some("/projects/example/worktrees/feature".into()),
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
        assert!(sparse.project_binding_id.is_none());
        assert!(sparse.host_thread_id.is_none());
        assert!(sparse.execution_root.is_none());
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
            agent_member_id: Some("agent-worker-1".into()),
            name: "worker-1".into(),
            role: "worker".into(),
            provider: "kimi".into(),
            model: Some("kimi-k2".into()),
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Running,
            native_session: None,
            worktree_ref: Some("/projects/example/worktrees/worker-1".into()),
            workspace_snapshot: Some(MemberWorkspaceSnapshot {
                cwd: "/projects/example/worktrees/worker-1".into(),
                project_binding_id: Some("project-example".into()),
                resolution_source: Some("member_worktree".into()),
                git_head: Some("0123456789abcdef".into()),
                git_branch: Some("feature/worker-1".into()),
                instruction_roots: vec!["/projects/example".into()],
                skill_roots: vec!["/projects/example/.agents/skills".into()],
            }),
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
        assert!(sparse.coordination_is_active());
        assert_eq!(sparse.runtime_generation, 1);
        assert!(sparse.slot_id.is_none());
        assert!(sparse.agent_member_id.is_none());
        assert!(sparse.model.is_none());
        assert!(sparse.worktree_ref.is_none());
        assert!(sparse.workspace_snapshot.is_none());
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
            origin_wave_id: Some("wave-2".into()),
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-1".into()],
            kind: TeamMessageKind::Assignment,
            body: "Take task-1".into(),
            correlation_id: "corr-1".into(),
            causation_id: None,
            response_intent: None,
            evidence_refs: vec!["ev-1".into()],
            deliveries: vec![TeamMessageDelivery {
                member_id: "mr-1".into(),
                policy: TeamDeliveryPolicy::Inject,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("test-receipt".into()),
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
    fn handoff_is_fenced_until_newer_same_correlation_mail_reaches_provider() {
        let root = team_test_root("handoff-mail-fence");
        let store = HarnessStore::new(&root);
        let correction = TeamMessage {
            id: "tm-correction".into(),
            team_run_id: "tr-fence".into(),
            origin_wave_id: None,
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-kimi".into()],
            kind: TeamMessageKind::Message,
            body: "Use the corrected requirement".into(),
            correlation_id: "corr-fence".into(),
            causation_id: Some("tm-assignment".into()),
            // Explicit response intent: this correction must reach the
            // provider before any Handoff, so it fences (ADR 0046 §4).
            response_intent: Some(TeamMessageResponseIntent::ResponseRequired),
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: "mr-kimi".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                updated_at: "unix-ms:1".into(),
            }],
            created_at: "unix-ms:1".into(),
        };
        store
            .append_team_message_checked(&correction)
            .expect("append correction");
        let handoff = TeamMessage {
            id: "tm-handoff".into(),
            team_run_id: "tr-fence".into(),
            origin_wave_id: None,
            sender: None,
            from_member_id: "mr-kimi".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["host".into()],
            kind: TeamMessageKind::Handoff,
            body: "done".into(),
            correlation_id: "corr-fence".into(),
            causation_id: Some("tm-assignment".into()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: "host".into(),
                policy: TeamDeliveryPolicy::ManualAck,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("harness-control-plane".into()),
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        let queued_error = store
            .append_team_message_checked(&handoff)
            .expect_err("queued correction must fence stale handoff");
        assert!(queued_error.to_string().contains("queued or claimed"));

        let mut claimed = correction.clone();
        claimed.deliveries[0].status = TeamDeliveryStatus::Claimed;
        claimed.deliveries[0].claim_id = Some("claim-1".into());
        store
            .append_team_message(&claimed)
            .expect("persist claim projection");
        let claimed_error = store
            .append_team_message_checked(&handoff)
            .expect_err("uncertain claimed correction must also fence handoff");
        assert!(claimed_error.to_string().contains("queued or claimed"));

        let mut delivered = claimed;
        delivered.deliveries[0].status = TeamDeliveryStatus::Delivered;
        delivered.deliveries[0].attempt = 1;
        delivered.deliveries[0].provider_receipt_id = Some("kimi-session:turn-2".into());
        store
            .append_team_message(&delivered)
            .expect("persist provider receipt");
        store
            .append_team_message_checked(&handoff)
            .expect("handoff is valid after provider receipt");

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn informational_mail_neither_fences_handoff_nor_requires_response() {
        let root = team_test_root("handoff-informational-fence");
        let store = HarnessStore::new(&root);
        // Acknowledgement-only peer mail: kind `message` with no explicit
        // intent is informational by default (ADR 0046 §4).
        let ack_only = TeamMessage {
            id: "tm-ack".into(),
            team_run_id: "tr-info".into(),
            origin_wave_id: None,
            sender: None,
            from_member_id: "mr-peer".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-kimi".into()],
            kind: TeamMessageKind::Message,
            body: "ACK: noted, no reply needed".into(),
            correlation_id: "corr-info".into(),
            causation_id: Some("tm-assignment".into()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: "mr-kimi".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                updated_at: "unix-ms:1".into(),
            }],
            created_at: "unix-ms:1".into(),
        };
        assert!(!ack_only.requires_response());
        store
            .append_team_message_checked(&ack_only)
            .expect("append informational mail");
        let handoff = TeamMessage {
            id: "tm-handoff".into(),
            team_run_id: "tr-info".into(),
            origin_wave_id: None,
            sender: None,
            from_member_id: "mr-kimi".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["host".into()],
            kind: TeamMessageKind::Handoff,
            body: "done".into(),
            correlation_id: "corr-info".into(),
            causation_id: Some("tm-assignment".into()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: "host".into(),
                policy: TeamDeliveryPolicy::ManualAck,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("harness-control-plane".into()),
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        // Informational mail never starts a provider round on its own, so it
        // must not fence a Handoff either — otherwise the Handoff would
        // deadlock behind mail that is intentionally never driven.
        store
            .append_team_message_checked(&handoff)
            .expect("informational mail must not fence handoff");

        // The same pending delivery with explicit response intent fences.
        let question = TeamMessage {
            id: "tm-question".into(),
            correlation_id: "corr-info-q".into(),
            causation_id: None,
            response_intent: Some(TeamMessageResponseIntent::ResponseRequired),
            created_at: "unix-ms:3".into(),
            ..ack_only.clone()
        };
        assert!(question.requires_response());
        store
            .append_team_message_checked(&question)
            .expect("append response-required question");
        let fenced = TeamMessage {
            id: "tm-handoff-q".into(),
            correlation_id: "corr-info-q".into(),
            causation_id: Some("tm-assignment-q".into()),
            created_at: "unix-ms:4".into(),
            ..handoff.clone()
        };
        let error = store
            .append_team_message_checked(&fenced)
            .expect_err("response-required question must fence stale handoff");
        assert!(error.to_string().contains("queued or claimed"));

        // Safety regression guard: a Host mid-round correction is ordinary
        // `message` mail with no explicit intent, but it is sender-aware
        // response-required, so it MUST still fence a same-correlation Handoff
        // — otherwise a member could hand off work that never absorbed the
        // correction.
        let host_correction = TeamMessage {
            id: "tm-host-correction".into(),
            from_member_id: "host".into(),
            correlation_id: "corr-info-host".into(),
            causation_id: None,
            response_intent: None,
            body: "Revise: drop the extra scope before handing off".into(),
            created_at: "unix-ms:5".into(),
            ..ack_only.clone()
        };
        assert!(
            host_correction.requires_response(),
            "Host ordinary mail defaults to response_required"
        );
        store
            .append_team_message_checked(&host_correction)
            .expect("append host correction");
        let stale = TeamMessage {
            id: "tm-handoff-host".into(),
            correlation_id: "corr-info-host".into(),
            causation_id: Some("tm-assignment-host".into()),
            created_at: "unix-ms:6".into(),
            ..handoff.clone()
        };
        let error = store
            .append_team_message_checked(&stale)
            .expect_err("pending Host correction must fence stale handoff");
        assert!(error.to_string().contains("queued or claimed"));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn concurrent_same_turn_handoffs_allow_exactly_one_append() {
        let root = team_test_root("same-turn-handoff");
        let store = Arc::new(HarnessStore::new(&root));
        let assignment = TeamMessage {
            id: "tm-assignment".into(),
            team_run_id: "tr-converge".into(),
            origin_wave_id: None,
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-codex".into()],
            kind: TeamMessageKind::Assignment,
            body: "Own the convergence fix".into(),
            correlation_id: "corr-converge".into(),
            causation_id: None,
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: "mr-codex".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("codex-turn-1".into()),
                updated_at: "unix-ms:1".into(),
            }],
            created_at: "unix-ms:1".into(),
        };
        store
            .append_team_message_checked(&assignment)
            .expect("append Assignment");
        let handoff = TeamMessage {
            id: "tm-handoff-a".into(),
            team_run_id: assignment.team_run_id.clone(),
            origin_wave_id: None,
            sender: None,
            from_member_id: "mr-codex".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["host".into()],
            kind: TeamMessageKind::Handoff,
            body: "## RESULT\ndone".into(),
            correlation_id: assignment.correlation_id.clone(),
            causation_id: Some(assignment.id.clone()),
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: "host".into(),
                policy: TeamDeliveryPolicy::ManualAck,
                status: TeamDeliveryStatus::Delivered,
                attempt: 1,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: Some("harness-control-plane".into()),
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        let barrier = Arc::new(Barrier::new(2));
        let handles = ["tm-handoff-a", "tm-handoff-b"]
            .into_iter()
            .map(|id| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                let mut candidate = handoff.clone();
                candidate.id = id.into();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.append_team_message_checked(&candidate)
                })
            })
            .collect::<Vec<_>>();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("handoff writer"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let conflict = results
            .iter()
            .find_map(|result| result.as_ref().err())
            .expect("one same-turn conflict");
        assert!(conflict.to_string().contains("already handed off"));
        assert_eq!(
            store
                .team_messages()
                .expect("messages")
                .into_iter()
                .filter(|message| message.kind == TeamMessageKind::Handoff)
                .count(),
            1
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn durable_supervisor_lease_and_message_claim_are_cross_process_safe() {
        let root = team_test_root("supervisor-claim");
        let store = Arc::new(HarnessStore::new(&root));
        let run = AgentTeamRun {
            id: "tr-claim".into(),
            definition_id: None,
            agent_team_id: None,
            previous_run_id: None,
            mission_id: None,
            wave_id: None,
            project_binding_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: Some("thread-claim".into()),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "claim exactly once".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-claim".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("append run");

        let first = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-a", 101, "test:a", 100, 1_000)
            .expect("first Supervisor");
        assert_eq!(first.generation, 1);
        let conflict = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-b", 202, "test:b", 101, 1_000)
            .expect_err("second active Supervisor must be rejected");
        assert!(conflict.to_string().contains("supervisor-a"));
        let second = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-b", 202, "test:b", 1_101, 1_000)
            .expect("expired lease may be replaced");
        assert_eq!(second.generation, 2);

        let message = TeamMessage {
            id: "tm-claim".into(),
            team_run_id: run.id.clone(),
            origin_wave_id: None,
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-claim".into()],
            kind: TeamMessageKind::Assignment,
            body: "only once".into(),
            correlation_id: "corr-claim".into(),
            causation_id: None,
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: "mr-claim".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        store
            .append_team_message_checked(&message)
            .expect("append queued message");
        let early_ack = store
            .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:2")
            .expect_err("queued delivery cannot be acknowledged");
        assert!(early_ack.to_string().contains("has not been delivered"));

        let barrier = Arc::new(Barrier::new(2));
        let handles = ["claim-a", "claim-b"].map(|claim_id| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            let run_id = run.id.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .claim_team_message_delivery(
                        &run_id,
                        "tm-claim",
                        "mr-claim",
                        "supervisor-b",
                        2,
                        claim_id,
                        1_102,
                        1_000,
                        "unix-ms:3",
                    )
                    .expect("claim call")
            })
        });
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("claim thread"))
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, TeamMessageDeliveryClaimResult::Claimed(_)))
                .count(),
            1
        );
        let claimed = results
            .into_iter()
            .find_map(|result| match result {
                TeamMessageDeliveryClaimResult::Claimed(message) => Some(*message),
                TeamMessageDeliveryClaimResult::NotQueued => None,
            })
            .expect("one claim");
        let claim_id = claimed.deliveries[0].claim_id.clone().expect("claim id");
        let stale_completion = store
            .complete_team_message_delivery_claim(
                &run.id,
                &message.id,
                "mr-claim",
                "supervisor-a",
                1,
                &claim_id,
                "native-turn-stale",
                1_103,
                "unix-ms:4",
            )
            .expect_err("a stale Supervisor generation cannot complete another lease's claim");
        assert!(stale_completion
            .to_string()
            .contains("Supervisor lease is not owned"));
        let delivered = store
            .complete_team_message_delivery_claim(
                &run.id,
                &message.id,
                "mr-claim",
                "supervisor-b",
                2,
                &claim_id,
                "native-turn-1",
                1_103,
                "unix-ms:4",
            )
            .expect("complete claim");
        assert_eq!(
            delivered.deliveries[0].status,
            TeamDeliveryStatus::Delivered
        );
        assert_eq!(
            delivered.deliveries[0].provider_receipt_id.as_deref(),
            Some("native-turn-1")
        );
        let acknowledged = store
            .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:5")
            .expect("acknowledge delivered message");
        assert_eq!(
            acknowledged.deliveries[0].status,
            TeamDeliveryStatus::Acknowledged
        );
        let acknowledged_again = store
            .acknowledge_team_message_delivery(&run.id, &message.id, "mr-claim", "unix-ms:6")
            .expect("ACK is idempotent");
        assert_eq!(
            acknowledged_again.deliveries[0].status,
            TeamDeliveryStatus::Acknowledged
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn member_close_request_survives_store_reopen_and_is_idempotent() {
        let root = team_test_root("durable-member-close");
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: "tr-close".into(),
            definition_id: None,
            agent_team_id: None,
            previous_run_id: None,
            mission_id: None,
            wave_id: None,
            project_binding_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: Some("thread-close".into()),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "close once".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-close".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        let member = MemberRun {
            id: "mr-close".into(),
            team_run_id: run.id.clone(),
            slot_id: None,
            agent_member_id: None,
            name: "Builder".into(),
            role: "builder".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Running,
            native_session: None,
            worktree_ref: None,
            workspace_snapshot: None,
            owned_paths: Vec::new(),
            started_at: "unix-ms:1".into(),
            last_event_at: None,
            finished_at: None,
        };
        store.append_team_run(&run).expect("append run");
        store.append_member_run(&member).expect("append member");

        let request = TeamMemberCloseRequest {
            id: "close-1".into(),
            team_run_id: run.id.clone(),
            member_run_id: member.id.clone(),
            requested_by: "host".into(),
            reason: "accepted".into(),
            status: TeamMemberCloseStatus::Pending,
            requested_at: "unix-ms:2".into(),
            applied_at: None,
        };
        let latched = store
            .latch_team_member_close(&request)
            .expect("latch Close");
        let repeated = store
            .latch_team_member_close(&TeamMemberCloseRequest {
                id: "close-duplicate".into(),
                ..request.clone()
            })
            .expect("repeat Close");
        assert_eq!(latched.id, repeated.id);

        let reopened = HarnessStore::new(&root);
        let pending = reopened
            .latest_team_member_close_request(&member.id)
            .expect("read Close after reopen")
            .expect("durable Close");
        assert_eq!(pending.status, TeamMemberCloseStatus::Pending);
        let applied = reopened
            .complete_team_member_close(&run.id, &member.id, &pending.id, "unix-ms:3")
            .expect("apply Close");
        assert_eq!(applied.status, TeamMemberCloseStatus::Applied);
        assert_eq!(applied.applied_at.as_deref(), Some("unix-ms:3"));
        let applied_again = reopened
            .complete_team_member_close(&run.id, &member.id, &pending.id, "unix-ms:4")
            .expect("Close apply is idempotent");
        assert_eq!(applied_again.applied_at.as_deref(), Some("unix-ms:3"));

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

    fn test_delivery(delivery_id: &str) -> MessageDelivery {
        MessageDelivery {
            delivery_id: Some(delivery_id.into()),
            execution_status: Some(ProviderExecutionStatus::Running),
            native_session: None,
            started_at: Some("unix-ms:1".into()),
            provider_request_id: None,
            provider_thread_id: None,
            provider_turn_id: None,
            terminal_source: None,
            delivered_at: None,
            last_error: None,
        }
    }
}

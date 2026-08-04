use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::{
    validate_agent_team_topology, AgentEvent, AgentMember, AgentMessageRoute, AgentRuntime,
    AgentTeam, AgentTeamRun, Decision, DelegationRun, Evidence, Gap, MemberAction, MemberRun,
    Message, MessageDelivery, MessageDeliveryStatus, MessageTerminalSource, Mission, MissionStatus,
    PendingInteraction, Proposal, ProviderChildThread, ProviderExecutionStatus, Review,
    TeamDeliveryPolicy, TeamDeliveryStatus, TeamMemberCloseRequest, TeamMemberCloseStatus,
    TeamMessage, TeamMessageKind, TeamRunEvent, TeamRunStatus, TeamSupervisorLease,
    TeamSupervisorLeaseStatus, Vision, Wave, WaveExecutorKind, WaveGateStatus, WaveStatus, Work,
    WorkClaimMode, WorkCommandContext, WorkDelivery, WorkDeliveryStatus, WorkDeliveryUpdate,
    WorkEvent, WorkEventKind, WorkOperation, WorkStatus, WorkflowArtifactManifest, WorkflowPatch,
    WorkflowRun, WorkflowStep,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkDeliveryClaimResult {
    Claimed(Box<WorkDelivery>),
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

    /// Insert a new AgentTeam under the store lock. Rejects a
    /// concurrently-created duplicate id and enforces the recursive topology
    /// invariants (ADR 0051) against the latest projection plus the candidate
    /// before appending. Member-existence checks stay with the caller; this
    /// guard owns graph integrity only.
    pub fn insert_agent_team(&self, value: &AgentTeam) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut teams = latest_by_id(self.read_jsonl::<AgentTeam>("teams.jsonl")?, |team| {
            team.id.clone()
        });
        if teams.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "agent team already exists: {}",
                value.id
            )));
        }
        teams.insert(value.id.clone(), value.clone());
        validate_agent_team_topology(&teams)
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.append_jsonl_unlocked("teams.jsonl", value)
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

    /// Insert a Work and its authoritative creation event/outbox as one
    /// crash-atomic JSONL row. Work commands intentionally refuse a legacy
    /// Assignment-message store so one Execution Space never has two ownership
    /// authorities.
    pub fn insert_work(&self, mut work: Work, context: WorkCommandContext) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            &work.id,
            WorkEventKind::Created,
        )? {
            return Ok(existing.work);
        }
        self.ensure_work_event_id_available_unlocked(&context.event_id)?;
        let team_run = self.require_team_run_unlocked(&work.team_run_id)?;
        if matches!(
            team_run.status,
            TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "team run {} is {:?} and cannot accept new Work",
                team_run.id, team_run.status
            )));
        }
        if self.latest_works_unlocked()?.contains_key(work.id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "work already exists: {}",
                work.id
            )));
        }
        if work.title.trim().is_empty() || work.completion_criteria_markdown.trim().is_empty() {
            return Err(StoreError::Conflict(
                "work title and completion criteria are required".to_string(),
            ));
        }
        work.version = 1;
        work.status = WorkStatus::Open;
        work.created_at = context.created_at.clone();
        work.updated_at = context.created_at.clone();
        if let Some(member_run_id) = work.active_member_run_id.as_deref() {
            let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
            self.ensure_member_can_receive_work_unlocked(&member)?;
            let stable_identity = stable_member_identity(&member);
            if work
                .owner_member_id
                .as_deref()
                .is_some_and(|owner| owner != stable_identity)
            {
                return Err(StoreError::Conflict(
                    "owner_member_id does not match active MemberRun stable identity".to_string(),
                ));
            }
            work.owner_member_id = Some(stable_identity);
        }
        match context.performed_by_actor.kind {
            harness_core::TeamActorKind::MemberRun => {
                let member = self.require_member_run_unlocked(
                    &context.performed_by_actor.id,
                    &work.team_run_id,
                )?;
                if !member.coordination_is_active() {
                    return Err(StoreError::Conflict(
                        "only an active MemberRun may create Work".to_string(),
                    ));
                }
                let own_identity = stable_member_identity(&member);
                if work
                    .owner_member_id
                    .as_deref()
                    .is_some_and(|owner| owner != own_identity)
                    || work
                        .active_member_run_id
                        .as_deref()
                        .is_some_and(|owner| owner != member.id)
                {
                    return Err(StoreError::Conflict(
                        "an ordinary Member may create only self-owned or unassigned Work"
                            .to_string(),
                    ));
                }
            }
            _ => require_host_actor(&context.performed_by_actor)?,
        }
        self.validate_work_relations_unlocked(&work)?;
        let deliveries =
            self.initial_work_deliveries_unlocked(&work, &context.event_id, &context.created_at)?;
        let operation = WorkOperation {
            event: WorkEvent {
                id: context.event_id,
                team_run_id: work.team_run_id.clone(),
                work_id: work.id.clone(),
                sequence: 1,
                kind: WorkEventKind::Created,
                expected_version: 0,
                resulting_version: 1,
                performed_by_actor: context.performed_by_actor,
                authority_actor: context.authority_actor,
                causation_ref: context.causation_ref,
                idempotency_key: context.idempotency_key,
                payload: serde_json::Value::Null,
                created_at: context.created_at,
            },
            work: work.clone(),
            deliveries,
            delivery_updates: Vec::new(),
        };
        self.append_jsonl_unlocked("work_operations.jsonl", &operation)?;
        Ok(work)
    }

    pub fn assign_work(
        &self,
        work_id: &str,
        expected_version: u64,
        owner_member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Assigned,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.is_terminal()
            || current.status != WorkStatus::Open
            || current.owner_member_id.is_some()
            || current.active_member_run_id.is_some()
        {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must be open to assign"
            )));
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let member = self.require_member_run_unlocked(owner_member_run_id, &current.team_run_id)?;
        self.ensure_member_can_receive_work_unlocked(&member)?;
        let owner_id = stable_member_identity(&member);
        let mut next = current.clone();
        next.owner_member_id = Some(owner_id);
        next.active_member_run_id = Some(member.id.clone());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Assigned, context)
    }

    /// Rebind non-terminal Work to a replacement runtime generation of the
    /// same stable member identity. This is the sole safe Host primitive after
    /// a runtime dies: the version bump fences the old runtime, the Rebound
    /// event records both bindings, and a fresh WorkDelivery targets the new
    /// MemberRun.
    ///
    /// A still-claimed delivery is an uncertain handoff and must first be
    /// completed, failed by its current lease owner, or reconciled by a
    /// successor. Provider-received/acknowledged deliveries remain immutable
    /// evidence and do not prevent a new-version rebind.
    pub fn rebind_work(
        &self,
        work_id: &str,
        expected_version: u64,
        new_member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Rebound,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is terminal and cannot be rebound"
            )));
        }
        let old_member_run_id = current.active_member_run_id.clone().ok_or_else(|| {
            StoreError::Conflict(format!("work {work_id} has no runtime binding to replace"))
        })?;
        let owner_member_id = current.owner_member_id.clone().ok_or_else(|| {
            StoreError::Conflict(format!("work {work_id} has no stable owner identity"))
        })?;
        if old_member_run_id == new_member_run_id {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already bound to MemberRun {new_member_run_id}"
            )));
        }
        let previous =
            self.require_member_run_unlocked(&old_member_run_id, &current.team_run_id)?;
        if previous.coordination_is_active()
            && !matches!(
                previous.status,
                harness_core::MemberRunStatus::Completed
                    | harness_core::MemberRunStatus::Failed
                    | harness_core::MemberRunStatus::Stopped
            )
        {
            return Err(StoreError::Conflict(format!(
                "OLD_RUNTIME_ACTIVE: MemberRun {old_member_run_id} must be closed or terminal before Work rebind"
            )));
        }
        if self
            .latest_work_deliveries_unlocked()?
            .values()
            .any(|delivery| {
                delivery.work_id == work_id && delivery.status == WorkDeliveryStatus::Claimed
            })
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work has a claimed delivery".to_string(),
            ));
        }
        let replacement =
            self.require_member_run_unlocked(new_member_run_id, &current.team_run_id)?;
        self.ensure_member_can_receive_work_unlocked(&replacement)?;
        let replacement_identity = stable_member_identity(&replacement);
        if replacement_identity != owner_member_id {
            return Err(StoreError::Conflict(format!(
                "OWNER_MISMATCH: replacement MemberRun {new_member_run_id} belongs to {replacement_identity}, expected {owner_member_id}"
            )));
        }

        let mut next = current.clone();
        next.active_member_run_id = Some(replacement.id.clone());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Rebound,
            context,
            serde_json::json!({
                "previous_member_run_id": old_member_run_id,
                "replacement_member_run_id": new_member_run_id,
                "owner_member_id": owner_member_id,
            }),
        )
    }

    pub fn claim_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Claimed,
        )? {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.status != WorkStatus::Open
            || current.owner_member_id.is_some()
            || current.claim_mode != WorkClaimMode::TeamClaim
        {
            return Err(StoreError::Conflict(format!(
                "CLAIM_LOST: work {work_id} is not an unowned team-claim Work"
            )));
        }
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if !matches!(
            member.status,
            harness_core::MemberRunStatus::Idle | harness_core::MemberRunStatus::Running
        ) || !member.coordination_is_active()
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: MemberRun {member_run_id} is not available and active"
            )));
        }
        let owner_id = stable_member_identity(&member);
        if !current.eligible_member_ids.is_empty()
            && !current.eligible_member_ids.iter().any(|id| id == &owner_id)
        {
            return Err(StoreError::Conflict(format!(
                "member {owner_id} is not eligible to claim work {work_id}"
            )));
        }
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        if !current.is_claim_ready(works.iter()) {
            return Err(StoreError::Conflict(format!("work {work_id} is not ready")));
        }
        if works.iter().any(|work| {
            work.team_run_id == current.team_run_id
                && work.status == WorkStatus::InProgress
                && work.active_member_run_id.as_deref() == Some(member_run_id)
        }) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: MemberRun {member_run_id} already has active Work"
            )));
        }
        let mut next = current.clone();
        next.owner_member_id = Some(owner_id);
        next.active_member_run_id = Some(member.id.clone());
        next.status = WorkStatus::InProgress;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Claimed, context)
    }

    pub fn start_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Started,
        )? {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.status != WorkStatus::Open
            || current.active_member_run_id.as_deref() != Some(member_run_id)
        {
            return Err(StoreError::Conflict(format!(
                "MemberRun {member_run_id} does not own open work {work_id}"
            )));
        }
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if !matches!(
            member.status,
            harness_core::MemberRunStatus::Idle | harness_core::MemberRunStatus::Running
        ) || !member.coordination_is_active()
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: MemberRun {member_run_id} is not available and active"
            )));
        }
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        if !current.is_claim_ready(works.iter()) {
            return Err(StoreError::Conflict(format!("work {work_id} is not ready")));
        }
        if works.iter().any(|work| {
            work.team_run_id == current.team_run_id
                && work.status == WorkStatus::InProgress
                && work.active_member_run_id.as_deref() == Some(member_run_id)
        }) {
            return Err(StoreError::Conflict(format!(
                "MEMBER_BUSY: MemberRun {member_run_id} already has active Work"
            )));
        }
        let mut next = current.clone();
        next.status = WorkStatus::InProgress;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Started, context)
    }

    pub fn block_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict("BLOCKER_REASON_REQUIRED".to_string()));
        }
        self.transition_owned_work(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Blocked,
            WorkStatus::InProgress,
            WorkStatus::Blocked,
            |work| work.blocker_reason = Some(reason.to_string()),
        )
    }

    pub fn block_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict("BLOCKER_REASON_REQUIRED".to_string()));
        }
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Blocked,
            WorkStatus::InProgress,
            WorkStatus::Blocked,
            serde_json::Value::Null,
            |work| work.blocker_reason = Some(reason.to_string()),
        )
    }

    pub fn resume_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        resolution: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if resolution.trim().is_empty() {
            return Err(StoreError::Conflict(
                "blocker resolution is required".to_string(),
            ));
        }
        self.transition_owned_work_with_payload(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Resumed,
            WorkStatus::Blocked,
            WorkStatus::InProgress,
            serde_json::json!({ "resolution": resolution }),
            |work| work.blocker_reason = None,
        )
    }

    pub fn resume_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        resolution: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if resolution.trim().is_empty() {
            return Err(StoreError::Conflict(
                "blocker resolution is required".to_string(),
            ));
        }
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Resumed,
            WorkStatus::Blocked,
            WorkStatus::InProgress,
            serde_json::json!({ "resolution": resolution }),
            |work| work.blocker_reason = None,
        )
    }

    pub fn release_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.release_work_with_authority(work_id, expected_version, Some(member_run_id), context)
    }

    pub fn release_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.release_work_with_authority(work_id, expected_version, None, context)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn submit_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        result_summary: &str,
        artifact_refs: Vec<String>,
        check_refs: Vec<String>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if result_summary.trim().is_empty() {
            return Err(StoreError::Conflict("RESULT_REQUIRED".to_string()));
        }
        self.transition_owned_work(
            work_id,
            expected_version,
            member_run_id,
            context,
            WorkEventKind::Submitted,
            WorkStatus::InProgress,
            WorkStatus::Review,
            |work| {
                work.result_summary = Some(result_summary.to_string());
                work.artifact_refs = artifact_refs;
                work.check_refs = check_refs;
                work.blocker_reason = None;
            },
        )
    }

    pub fn accept_work(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.accept_work_with_summary(work_id, expected_version, None, context)
    }

    pub fn accept_work_with_summary(
        &self,
        work_id: &str,
        expected_version: u64,
        summary: Option<&str>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if summary.is_some_and(|value| value.trim().is_empty()) {
            return Err(StoreError::Conflict(
                "acceptance summary must not be empty when provided".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Accepted,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.status != WorkStatus::Review {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must await Host acceptance"
            )));
        }
        let mut next = current.clone();
        next.status = WorkStatus::Done;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        let payload = summary
            .map(|summary| serde_json::json!({ "summary": summary }))
            .unwrap_or(serde_json::Value::Null);
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Accepted,
            context,
            payload,
        )
    }

    pub fn request_work_changes(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "changes-requested reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::ChangesRequested,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.status != WorkStatus::Review {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must await Host acceptance"
            )));
        }
        let mut next = current.clone();
        next.status = WorkStatus::InProgress;
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(
            current,
            next,
            WorkEventKind::ChangesRequested,
            context,
        )
    }

    pub fn cancel_work(
        &self,
        work_id: &str,
        expected_version: u64,
        reason: &str,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "cancellation reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Cancelled,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already terminal"
            )));
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let mut next = current.clone();
        next.status = WorkStatus::Cancelled;
        next.blocker_reason = Some(reason.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Cancelled, context)
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_owned_work(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
        kind: WorkEventKind,
        required_status: WorkStatus,
        resulting_status: WorkStatus,
        mutate: impl FnOnce(&mut Work),
    ) -> StoreResult<Work> {
        self.transition_owned_work_with_payload(
            work_id,
            expected_version,
            member_run_id,
            context,
            kind,
            required_status,
            resulting_status,
            serde_json::Value::Null,
            mutate,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_owned_work_with_payload(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        context: WorkCommandContext,
        kind: WorkEventKind,
        required_status: WorkStatus,
        resulting_status: WorkStatus,
        payload: serde_json::Value,
        mutate: impl FnOnce(&mut Work),
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) =
            self.idempotent_work_operation_unlocked(&context.idempotency_key, work_id, kind)?
        {
            return Ok(existing.work);
        }
        require_member_actor(&context.performed_by_actor, member_run_id)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.status != required_status
            || current.active_member_run_id.as_deref() != Some(member_run_id)
        {
            return Err(StoreError::Conflict(format!(
                "MemberRun {member_run_id} does not own active work {work_id} in required state"
            )));
        }
        // A Closed or Retired MemberRun no longer mutates its owned Work:
        // unfinished Work moves only via Host reassign/cancel or after an
        // explicit Reopen (docs/product/agent-team-works.md). This aligns
        // member-side transitions with insert/claim/start/receive, which
        // already require active coordination.
        let member = self.require_member_run_unlocked(member_run_id, &current.team_run_id)?;
        if !member.coordination_is_active() {
            return Err(StoreError::Conflict(format!(
                "MEMBER_UNAVAILABLE: MemberRun {member_run_id} coordination is {:?}; Reopen before mutating owned Work",
                member.coordination_status
            )));
        }
        let mut next = current.clone();
        mutate(&mut next);
        next.status = resulting_status;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(current, next, kind, context, payload)
    }

    #[allow(clippy::too_many_arguments)]
    fn transition_work_as_host(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
        kind: WorkEventKind,
        required_status: WorkStatus,
        resulting_status: WorkStatus,
        payload: serde_json::Value,
        mutate: impl FnOnce(&mut Work),
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) =
            self.idempotent_work_operation_unlocked(&context.idempotency_key, work_id, kind)?
        {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.status != required_status {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is not in required state"
            )));
        }
        if current.active_member_run_id.is_none() || current.owner_member_id.is_none() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} has no owner to retain"
            )));
        }
        let mut next = current.clone();
        mutate(&mut next);
        next.status = resulting_status;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(current, next, kind, context, payload)
    }

    fn release_work_with_authority(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: Option<&str>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Released,
        )? {
            return Ok(existing.work);
        }
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.status != WorkStatus::Open {
            return Err(StoreError::Conflict(format!(
                "work {work_id} must be open to release"
            )));
        }
        if current.active_member_run_id.is_none() || current.owner_member_id.is_none() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is already unassigned"
            )));
        }
        match member_run_id {
            Some(member_run_id) => {
                require_member_actor(&context.performed_by_actor, member_run_id)?;
                if current.active_member_run_id.as_deref() != Some(member_run_id) {
                    return Err(StoreError::Conflict(format!(
                        "MemberRun {member_run_id} does not own open work {work_id}"
                    )));
                }
            }
            None => require_host_actor(&context.performed_by_actor)?,
        }
        self.ensure_deliveries_reassignable_unlocked(&current)?;
        let mut next = current.clone();
        next.owner_member_id = None;
        next.active_member_run_id = None;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_unlocked(current, next, WorkEventKind::Released, context)
    }

    fn append_work_transition_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            kind,
            context,
            serde_json::Value::Null,
        )
    }

    fn append_work_transition_with_payload_unlocked(
        &self,
        current: Work,
        next: Work,
        kind: WorkEventKind,
        context: WorkCommandContext,
        payload: serde_json::Value,
    ) -> StoreResult<Work> {
        self.ensure_work_event_id_available_unlocked(&context.event_id)?;
        let sequence = self
            .work_operations_unlocked()?
            .iter()
            .filter(|operation| operation.work.id == current.id)
            .count() as u64
            + 1;
        let deliveries = if matches!(
            kind,
            WorkEventKind::Assigned
                | WorkEventKind::ChangesRequested
                | WorkEventKind::Resumed
                | WorkEventKind::Rebound
        ) {
            self.initial_work_deliveries_unlocked(&next, &context.event_id, &context.created_at)?
        } else {
            Vec::new()
        };
        let mut next_delivery_update_sequence =
            self.next_work_delivery_update_sequence_unlocked()?;
        let delivery_updates = self
            .latest_work_deliveries_unlocked()?
            .into_values()
            .filter(|delivery| {
                delivery.work_id == current.id
                    && delivery.status == WorkDeliveryStatus::Queued
                    && delivery.work_version < next.version
            })
            .map(|delivery| {
                let update_sequence = next_delivery_update_sequence;
                next_delivery_update_sequence = next_delivery_update_sequence.saturating_add(1);
                WorkDeliveryUpdate {
                    delivery_id: delivery.id,
                    update_sequence,
                    status: WorkDeliveryStatus::Invalidated,
                    attempt: delivery.attempt,
                    claim_id: delivery.claim_id,
                    claimed_by_supervisor_id: delivery.claimed_by_supervisor_id,
                    claimed_generation: delivery.claimed_generation,
                    provider_receipt_id: delivery.provider_receipt_id,
                    failure_reason: delivery.failure_reason,
                    updated_at: context.created_at.clone(),
                }
            })
            .collect();
        let operation = WorkOperation {
            event: WorkEvent {
                id: context.event_id,
                team_run_id: next.team_run_id.clone(),
                work_id: next.id.clone(),
                sequence,
                kind,
                expected_version: current.version,
                resulting_version: next.version,
                performed_by_actor: context.performed_by_actor,
                authority_actor: context.authority_actor,
                causation_ref: context.causation_ref,
                idempotency_key: context.idempotency_key,
                payload,
                created_at: context.created_at,
            },
            work: next.clone(),
            deliveries,
            delivery_updates,
        };
        self.append_jsonl_unlocked("work_operations.jsonl", &operation)?;
        Ok(next)
    }

    fn ensure_work_store_compatible_unlocked(&self) -> StoreResult<()> {
        // `assignment` is no longer a TeamMessageKind, so a legacy row fails
        // deserialization before any Work mutation can be accepted. We do not
        // migrate or reinterpret that history: use a fresh Execution Space.
        let _ = self.read_jsonl::<TeamMessage>("team_messages.jsonl")?;
        Ok(())
    }

    fn require_team_run_unlocked(&self, team_run_id: &str) -> StoreResult<AgentTeamRun> {
        latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        })
        .remove(team_run_id)
        .ok_or_else(|| StoreError::Conflict(format!("team run not found: {team_run_id}")))
    }

    fn require_member_run_unlocked(
        &self,
        member_run_id: &str,
        team_run_id: &str,
    ) -> StoreResult<MemberRun> {
        let member = latest_by_id(self.read_jsonl::<MemberRun>("member_runs.jsonl")?, |row| {
            row.id.clone()
        })
        .remove(member_run_id)
        .ok_or_else(|| StoreError::Conflict(format!("member run not found: {member_run_id}")))?;
        if member.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "MemberRun {member_run_id} does not belong to TeamRun {team_run_id}"
            )));
        }
        Ok(member)
    }

    fn validate_work_relations_unlocked(&self, work: &Work) -> StoreResult<()> {
        let works = self.latest_works_unlocked()?;
        for prerequisite_id in &work.prerequisite_work_ids {
            let prerequisite = works.get(prerequisite_id).ok_or_else(|| {
                StoreError::Conflict(format!("prerequisite work not found: {prerequisite_id}"))
            })?;
            if prerequisite.team_run_id != work.team_run_id || prerequisite.id == work.id {
                return Err(StoreError::Conflict(
                    "prerequisites must be distinct Works in the same TeamRun".to_string(),
                ));
            }
        }
        if let Some(parent_id) = work.parent_work_id.as_deref() {
            let parent = works.get(parent_id).ok_or_else(|| {
                StoreError::Conflict(format!("parent work not found: {parent_id}"))
            })?;
            if parent.team_run_id != work.team_run_id || parent.id == work.id {
                return Err(StoreError::Conflict(
                    "parent_work_id must reference a distinct Work in the same TeamRun".to_string(),
                ));
            }
        }
        if let Some(member_run_id) = work.active_member_run_id.as_deref() {
            let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
            self.ensure_member_can_receive_work_unlocked(&member)?;
            if work.owner_member_id.as_deref() != Some(stable_member_identity(&member).as_str()) {
                return Err(StoreError::Conflict(
                    "owner_member_id does not match active MemberRun stable identity".to_string(),
                ));
            }
        } else if work.owner_member_id.is_some() {
            return Err(StoreError::Conflict(
                "owned Work requires an active_member_run_id binding".to_string(),
            ));
        }
        Ok(())
    }

    fn ensure_member_can_receive_work_unlocked(&self, member: &MemberRun) -> StoreResult<()> {
        if !member.coordination_is_active()
            || matches!(
                member.status,
                harness_core::MemberRunStatus::Stopped | harness_core::MemberRunStatus::Failed
            )
        {
            return Err(StoreError::Conflict(format!(
                "MEMBER_UNAVAILABLE: MemberRun {} cannot receive Work while {:?}/{:?}",
                member.id, member.coordination_status, member.status
            )));
        }
        Ok(())
    }

    fn initial_work_deliveries_unlocked(
        &self,
        work: &Work,
        event_id: &str,
        updated_at: &str,
    ) -> StoreResult<Vec<WorkDelivery>> {
        let Some(member_run_id) = work.active_member_run_id.as_deref() else {
            return Ok(Vec::new());
        };
        let member = self.require_member_run_unlocked(member_run_id, &work.team_run_id)?;
        self.ensure_member_can_receive_work_unlocked(&member)?;
        Ok(vec![WorkDelivery {
            id: format!("work-delivery-{event_id}-{member_run_id}"),
            work_event_id: event_id.to_string(),
            team_run_id: work.team_run_id.clone(),
            work_id: work.id.clone(),
            work_version: work.version,
            recipient_member_run_id: member_run_id.to_string(),
            status: WorkDeliveryStatus::Queued,
            attempt: 0,
            claim_id: None,
            claimed_by_supervisor_id: None,
            claimed_generation: None,
            provider_receipt_id: None,
            failure_reason: None,
            updated_at: updated_at.to_string(),
        }])
    }

    fn current_work_unlocked(&self, work_id: &str, expected_version: u64) -> StoreResult<Work> {
        let current = self
            .latest_works_unlocked()?
            .remove(work_id)
            .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        if current.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "VERSION_CONFLICT: work {work_id} is version {}, expected {expected_version}",
                current.version
            )));
        }
        Ok(current)
    }

    fn ensure_deliveries_reassignable_unlocked(&self, work: &Work) -> StoreResult<()> {
        if self
            .latest_work_deliveries_unlocked()?
            .values()
            .any(|delivery| {
                delivery.work_id == work.id
                    && delivery.work_version == work.version
                    && work.active_member_run_id.as_deref()
                        == Some(delivery.recipient_member_run_id.as_str())
                    && matches!(
                        delivery.status,
                        WorkDeliveryStatus::Claimed | WorkDeliveryStatus::ProviderReceived
                    )
            })
        {
            return Err(StoreError::Conflict(
                "RECONCILIATION_REQUIRED: Work delivery was already accepted".to_string(),
            ));
        }
        Ok(())
    }

    /// Return an exact idempotent retry, while rejecting accidental reuse of
    /// the same key for a different Work or command. A bare key is not enough
    /// to identify an operation safely: without this fingerprint a retry of
    /// `start(work-a)` could silently return the result of `cancel(work-b)`.
    fn idempotent_work_operation_unlocked(
        &self,
        idempotency_key: &str,
        work_id: &str,
        kind: WorkEventKind,
    ) -> StoreResult<Option<WorkOperation>> {
        let existing = self
            .work_operations_unlocked()?
            .into_iter()
            .find(|operation| operation.event.idempotency_key == idempotency_key);
        let Some(existing) = existing else {
            return Ok(None);
        };
        if existing.event.work_id != work_id || existing.event.kind != kind {
            return Err(StoreError::Conflict(format!(
                "IDEMPOTENCY_CONFLICT: key {idempotency_key} already belongs to {:?} on Work {}",
                existing.event.kind, existing.event.work_id
            )));
        }
        Ok(Some(existing))
    }

    fn work_operations_unlocked(&self) -> StoreResult<Vec<WorkOperation>> {
        self.read_jsonl("work_operations.jsonl")
    }

    fn ensure_work_event_id_available_unlocked(&self, event_id: &str) -> StoreResult<()> {
        if self
            .work_operations_unlocked()?
            .iter()
            .any(|operation| operation.event.id == event_id)
        {
            return Err(StoreError::Conflict(format!(
                "WORK_EVENT_ID_CONFLICT: event id {event_id} is already in use"
            )));
        }
        Ok(())
    }

    fn next_work_delivery_update_sequence_unlocked(&self) -> StoreResult<u64> {
        let embedded_max = self
            .work_operations_unlocked()?
            .into_iter()
            .flat_map(|operation| operation.delivery_updates)
            .map(|update| update.update_sequence)
            .max()
            .unwrap_or(0);
        let standalone_max = self
            .read_jsonl::<WorkDeliveryUpdate>("work_delivery_updates.jsonl")?
            .into_iter()
            .map(|update| update.update_sequence)
            .max()
            .unwrap_or(0);
        Ok(embedded_max.max(standalone_max).saturating_add(1))
    }

    fn latest_works_unlocked(&self) -> StoreResult<std::collections::BTreeMap<String, Work>> {
        Ok(latest_by_id(self.work_operations_unlocked()?, |operation| {
            operation.work.id.clone()
        })
        .into_iter()
        .map(|(id, operation)| (id, operation.work))
        .collect())
    }

    fn latest_work_deliveries_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, WorkDelivery>> {
        let mut deliveries = std::collections::BTreeMap::new();
        let mut legacy_updates = Vec::new();
        let mut sequenced_updates = Vec::new();
        let mut legacy_order = 0_u64;
        for operation in self.work_operations_unlocked()? {
            for delivery in operation.deliveries {
                deliveries.insert(delivery.id.clone(), delivery);
            }
            for update in operation.delivery_updates {
                if update.update_sequence == 0 {
                    legacy_updates.push((update.updated_at.clone(), legacy_order, update));
                    legacy_order = legacy_order.saturating_add(1);
                } else {
                    sequenced_updates.push(update);
                }
            }
        }
        for update in self.read_jsonl::<WorkDeliveryUpdate>("work_delivery_updates.jsonl")? {
            if update.update_sequence == 0 {
                legacy_updates.push((update.updated_at.clone(), legacy_order, update));
                legacy_order = legacy_order.saturating_add(1);
            } else {
                sequenced_updates.push(update);
            }
        }
        // Rows written before update_sequence existed remain readable. Their
        // best available ordering evidence is timestamp plus stable file-scan
        // order. All new writes are then folded by the Store-assigned sequence,
        // independent of caller clocks or which JSONL file carries the update.
        legacy_updates.sort_by(|left, right| {
            compare_store_timestamps(&left.0, &right.0).then(left.1.cmp(&right.1))
        });
        sequenced_updates.sort_by_key(|update| update.update_sequence);
        for update in legacy_updates
            .into_iter()
            .map(|(_, _, update)| update)
            .chain(sequenced_updates)
        {
            if let Some(delivery) = deliveries.get_mut(&update.delivery_id) {
                apply_work_delivery_update(delivery, update);
            }
        }
        Ok(deliveries)
    }

    pub fn append_team_message(&self, value: &TeamMessage) -> StoreResult<()> {
        self.append_jsonl("team_messages.jsonl", value)
    }

    /// Append a manually-authored TeamMessage under the global lock.
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
        if let Some(work_id) = value.work_id.as_deref() {
            let work = self
                .latest_works_unlocked()?
                .remove(work_id)
                .ok_or_else(|| StoreError::Conflict(format!("Work not found: {work_id}")))?;
            if work.team_run_id != value.team_run_id {
                return Err(StoreError::Conflict(format!(
                    "Work {work_id} belongs to TeamRun {}, not {}",
                    work.team_run_id, value.team_run_id
                )));
            }
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
    /// processes from resurrecting or overwriting one attempt. A completion
    /// also checks the authoritative Work projection while holding this same
    /// lock, so a concurrent Work create cannot slip between the guard and the
    /// TeamRun CAS.
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
        if next.status == TeamRunStatus::Completed {
            let unfinished = self
                .latest_works_unlocked()?
                .into_values()
                .filter(|work| work.team_run_id == next.id && !work.is_terminal())
                .collect::<Vec<_>>();
            if !unfinished.is_empty() {
                let detail = unfinished
                    .iter()
                    .map(|work| {
                        let status = serde_json::to_string(&work.status)
                            .unwrap_or_else(|_| format!("{:?}", work.status));
                        format!(
                            "{} ({}, version {})",
                            work.id,
                            status.trim_matches('"'),
                            work.version
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(StoreError::Conflict(format!(
                    "team run {} cannot complete while Works remain non-terminal: {detail}; accept or cancel every Work first",
                    next.id
                )));
            }
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

    /// Latest-row-wins AgentTeam projection keyed by team id. This is the
    /// input for recursive topology validation and queries (ADR 0051).
    pub fn latest_teams(&self) -> StoreResult<std::collections::BTreeMap<String, AgentTeam>> {
        Ok(latest_by_id(self.teams()?, |team| team.id.clone()))
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

    pub fn work_operations(&self) -> StoreResult<Vec<WorkOperation>> {
        self.work_operations_unlocked()
    }

    pub fn latest_works(&self) -> StoreResult<Vec<Work>> {
        Ok(self.latest_works_unlocked()?.into_values().collect())
    }

    pub fn work_events(&self) -> StoreResult<Vec<WorkEvent>> {
        Ok(self
            .work_operations_unlocked()?
            .into_iter()
            .map(|operation| operation.event)
            .collect())
    }

    pub fn latest_work_deliveries(&self) -> StoreResult<Vec<WorkDelivery>> {
        Ok(self
            .latest_work_deliveries_unlocked()?
            .into_values()
            .collect())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn claim_work_delivery(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<WorkDeliveryClaimResult> {
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
        let mut deliveries = self.latest_work_deliveries_unlocked()?;
        let Some(mut delivery) = deliveries.remove(delivery_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        if delivery.team_run_id != team_run_id
            || delivery.recipient_member_run_id != member_run_id
            || !matches!(
                delivery.status,
                WorkDeliveryStatus::Queued | WorkDeliveryStatus::Failed
            )
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        let works = self.latest_works_unlocked()?;
        let Some(work) = works.get(&delivery.work_id) else {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        };
        // A queued row is only actionable for the newest Work revision and
        // current runtime binding. `Open` is deliberately not required:
        // revisions created by resume/change-request/rebind can be delivered
        // while the Work is in progress, blocked, or under review.
        if work.team_run_id != team_run_id
            || work.version != delivery.work_version
            || work.active_member_run_id.as_deref() != Some(member_run_id)
            || work.is_terminal()
            || !work.prerequisites_satisfied(works.values())
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        // A provider receipt is published as soon as the native runtime
        // accepts a Work prompt. The member may not have executed `work start`
        // yet, so the Work can still be `open` during this hand-off window.
        // Treat that receipted (or still-claimed) Work as occupying the single
        // member execution slot, in addition to explicitly active lifecycle
        // states. A later revision of the *same* Work remains deliverable for
        // resume/change-request; only a different Work is fenced.
        if works.values().any(|other| {
            other.id != work.id
                && other.team_run_id == team_run_id
                && other.active_member_run_id.as_deref() == Some(member_run_id)
                && (matches!(other.status, WorkStatus::InProgress | WorkStatus::Blocked)
                    || (other.status == WorkStatus::Open
                        && deliveries.values().any(|existing| {
                            existing.work_id == other.id
                                && existing.recipient_member_run_id == member_run_id
                                && matches!(
                                    existing.status,
                                    WorkDeliveryStatus::Claimed
                                        | WorkDeliveryStatus::ProviderReceived
                                )
                        })))
        }) {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        let member = self.require_member_run_unlocked(member_run_id, team_run_id)?;
        if self
            .ensure_member_can_receive_work_unlocked(&member)
            .is_err()
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        delivery.status = WorkDeliveryStatus::Claimed;
        delivery.attempt = delivery.attempt.saturating_add(1);
        delivery.claim_id = Some(claim_id.to_string());
        delivery.claimed_by_supervisor_id = Some(supervisor_id.to_string());
        delivery.claimed_generation = Some(supervisor_generation);
        delivery.provider_receipt_id = None;
        delivery.failure_reason = None;
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &WorkDeliveryUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(WorkDeliveryClaimResult::Claimed(Box::new(delivery)))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        provider_receipt_id: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<WorkDelivery> {
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
                "team run {team_run_id} Supervisor lease is not current"
            )));
        }
        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("WorkDelivery not found: {delivery_id}"))
            })?;
        let owns_claim = delivery.team_run_id == team_run_id
            && delivery.recipient_member_run_id == member_run_id
            && delivery.claim_id.as_deref() == Some(claim_id)
            && delivery.claimed_by_supervisor_id.as_deref() == Some(supervisor_id)
            && delivery.claimed_generation == Some(supervisor_generation);
        if delivery.status == WorkDeliveryStatus::ProviderReceived && owns_claim {
            if delivery.provider_receipt_id.as_deref() != Some(provider_receipt_id) {
                return Err(StoreError::Conflict(format!(
                    "WorkDelivery claim {claim_id} was already completed with a different provider receipt"
                )));
            }
            return Ok(delivery);
        }
        if !owns_claim
            || delivery.recipient_member_run_id != member_run_id
            || delivery.status != WorkDeliveryStatus::Claimed
        {
            return Err(StoreError::Conflict(format!(
                "WorkDelivery claim {claim_id} no longer owns {delivery_id}"
            )));
        }
        delivery.status = WorkDeliveryStatus::ProviderReceived;
        delivery.provider_receipt_id = Some(provider_receipt_id.to_string());
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &WorkDeliveryUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: delivery.provider_receipt_id.clone(),
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(delivery)
    }

    /// Fail the currently-owned WorkDelivery claim. Only the Supervisor that
    /// owns the current, unexpired TeamRun lease and the exact durable claim
    /// may write this terminal delivery outcome. The failure reason is control
    /// evidence, not a copy of provider output.
    #[allow(clippy::too_many_arguments)]
    pub fn fail_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        claim_id: &str,
        reason: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<WorkDelivery> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "WorkDelivery failure reason is required".to_string(),
            ));
        }
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
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
                "team run {team_run_id} Supervisor lease is not current"
            )));
        }

        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("WorkDelivery not found: {delivery_id}"))
            })?;
        let owns_claim = delivery.team_run_id == team_run_id
            && delivery.recipient_member_run_id == member_run_id
            && delivery.claim_id.as_deref() == Some(claim_id)
            && delivery.claimed_by_supervisor_id.as_deref() == Some(supervisor_id)
            && delivery.claimed_generation == Some(supervisor_generation);
        if delivery.status == WorkDeliveryStatus::Failed && owns_claim {
            if delivery.failure_reason.as_deref() != Some(reason) {
                return Err(StoreError::Conflict(format!(
                    "WorkDelivery claim {claim_id} was already failed with a different reason"
                )));
            }
            return Ok(delivery);
        }
        if delivery.status != WorkDeliveryStatus::Claimed || !owns_claim {
            return Err(StoreError::Conflict(format!(
                "WorkDelivery claim {claim_id} no longer owns {delivery_id}"
            )));
        }

        delivery.status = WorkDeliveryStatus::Failed;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = Some(reason.to_string());
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &WorkDeliveryUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: delivery.claim_id.clone(),
                claimed_by_supervisor_id: delivery.claimed_by_supervisor_id.clone(),
                claimed_generation: delivery.claimed_generation,
                provider_receipt_id: None,
                failure_reason: delivery.failure_reason.clone(),
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(delivery)
    }

    /// Requeue a WorkDelivery claim abandoned by an older Supervisor
    /// generation. This is intentionally explicit: an expired lease alone is
    /// not proof that the provider did not receive the Work.
    ///
    /// Only the current, unexpired successor lease may reconcile. A claim with
    /// a provider receipt, or a delivery already marked provider-received or
    /// acknowledged, is never rolled back.
    #[allow(clippy::too_many_arguments)]
    pub fn reconcile_stale_work_delivery_claim(
        &self,
        team_run_id: &str,
        delivery_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<WorkDelivery> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let lease = self
            .latest_lease_for_run_unlocked(team_run_id)?
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

        let mut delivery = self
            .latest_work_deliveries_unlocked()?
            .remove(delivery_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!("WorkDelivery not found: {delivery_id}"))
            })?;
        if delivery.team_run_id != team_run_id {
            return Err(StoreError::Conflict(format!(
                "WorkDelivery {delivery_id} belongs to {}, not {team_run_id}",
                delivery.team_run_id
            )));
        }
        if delivery.status == WorkDeliveryStatus::Queued
            && delivery.claim_id.is_none()
            && delivery.claimed_by_supervisor_id.is_none()
            && delivery.claimed_generation.is_none()
            && delivery.provider_receipt_id.is_none()
        {
            return Ok(delivery);
        }
        if delivery.status != WorkDeliveryStatus::Claimed {
            return Err(StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: WorkDelivery {delivery_id} is {:?} and cannot be requeued",
                delivery.status
            )));
        }
        if delivery.provider_receipt_id.is_some() {
            return Err(StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: WorkDelivery {delivery_id} has a provider receipt"
            )));
        }
        let claimed_generation = delivery.claimed_generation.ok_or_else(|| {
            StoreError::Conflict(format!(
                "RECONCILIATION_REQUIRED: WorkDelivery {delivery_id} has no claimed generation"
            ))
        })?;
        if claimed_generation >= supervisor_generation {
            return Err(StoreError::Conflict(format!(
                "WorkDelivery {delivery_id} is not a stale claim from a predecessor Supervisor generation"
            )));
        }

        delivery.status = WorkDeliveryStatus::Queued;
        delivery.claim_id = None;
        delivery.claimed_by_supervisor_id = None;
        delivery.claimed_generation = None;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = None;
        delivery.updated_at = updated_at.to_string();
        let update_sequence = self.next_work_delivery_update_sequence_unlocked()?;
        self.append_jsonl_unlocked(
            "work_delivery_updates.jsonl",
            &WorkDeliveryUpdate {
                delivery_id: delivery.id.clone(),
                update_sequence,
                status: delivery.status,
                attempt: delivery.attempt,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: delivery.updated_at.clone(),
            },
        )?;
        Ok(delivery)
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

fn stable_member_identity(member: &MemberRun) -> String {
    member
        .agent_member_id
        .clone()
        .or_else(|| member.slot_id.clone())
        .unwrap_or_else(|| member.id.clone())
}

fn compare_store_timestamps(left: &str, right: &str) -> std::cmp::Ordering {
    match (
        left.strip_prefix("unix-ms:")
            .and_then(|value| value.parse::<u128>().ok()),
        right
            .strip_prefix("unix-ms:")
            .and_then(|value| value.parse::<u128>().ok()),
    ) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left.cmp(right),
    }
}

fn apply_work_delivery_update(delivery: &mut WorkDelivery, update: WorkDeliveryUpdate) {
    delivery.status = update.status;
    delivery.attempt = update.attempt;
    delivery.claim_id = update.claim_id;
    delivery.claimed_by_supervisor_id = update.claimed_by_supervisor_id;
    delivery.claimed_generation = update.claimed_generation;
    delivery.provider_receipt_id = update.provider_receipt_id;
    delivery.failure_reason = update.failure_reason;
    delivery.updated_at = update.updated_at;
}

fn require_host_actor(actor: &harness_core::TeamActorRef) -> StoreResult<()> {
    if matches!(
        actor.kind,
        harness_core::TeamActorKind::Host
            | harness_core::TeamActorKind::Operator
            | harness_core::TeamActorKind::Service
    ) {
        Ok(())
    } else {
        Err(StoreError::Conflict(
            "Host authority is required for this Work command".to_string(),
        ))
    }
}

fn require_member_actor(
    actor: &harness_core::TeamActorRef,
    member_run_id: &str,
) -> StoreResult<()> {
    if actor.kind == harness_core::TeamActorKind::MemberRun && actor.id == member_run_id {
        Ok(())
    } else {
        Err(StoreError::Conflict(format!(
            "trusted MemberRun actor {member_run_id} is required"
        )))
    }
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
            .create(true)
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
            work_id: None,
            origin_wave_id: Some("wave-2".into()),
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-1".into()],
            kind: TeamMessageKind::Message,
            body: "Please review task-1".into(),
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
            work_id: None,
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
            work_id: None,
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
            work_id: None,
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
            work_id: None,
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
            work_id: None,
            origin_wave_id: None,
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-codex".into()],
            kind: TeamMessageKind::Message,
            body: "Review the convergence fix".into(),
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
            .expect("append conversation anchor");
        let handoff = TeamMessage {
            id: "tm-handoff-a".into(),
            team_run_id: assignment.team_run_id.clone(),
            work_id: None,
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
            work_id: None,
            origin_wave_id: None,
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-claim".into()],
            kind: TeamMessageKind::Message,
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

    fn work_test_fixture(
        name: &str,
    ) -> (PathBuf, HarnessStore, AgentTeamRun, MemberRun, MemberRun) {
        let root = team_test_root(name);
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: format!("tr-{name}"),
            definition_id: None,
            agent_team_id: None,
            previous_run_id: None,
            mission_id: None,
            wave_id: None,
            project_binding_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: Some(format!("host-{name}")),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "prove Works".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec![format!("mr-{name}-a"), format!("mr-{name}-b")],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        let member = |suffix: &str| MemberRun {
            id: format!("mr-{name}-{suffix}"),
            team_run_id: run.id.clone(),
            slot_id: Some(format!("slot-{suffix}")),
            agent_member_id: Some(format!("agent-{suffix}")),
            name: format!("Member {suffix}"),
            role: "builder".into(),
            provider: "codex".into(),
            model: None,
            provider_controls: Default::default(),
            provider_profile: None,
            provider_capacity: None,
            coordination_status: Default::default(),
            runtime_generation: 1,
            status: MemberRunStatus::Idle,
            native_session: None,
            worktree_ref: None,
            workspace_snapshot: None,
            owned_paths: Vec::new(),
            started_at: "unix-ms:1".into(),
            last_event_at: None,
            finished_at: None,
        };
        let member_a = member("a");
        let member_b = member("b");
        store.append_team_run(&run).expect("append team run");
        store.append_member_run(&member_a).expect("append member a");
        store.append_member_run(&member_b).expect("append member b");
        (root, store, run, member_a, member_b)
    }

    fn host_work_context(id: &str, key: &str, at: &str) -> WorkCommandContext {
        WorkCommandContext {
            event_id: id.into(),
            performed_by_actor: harness_core::TeamActorRef {
                kind: harness_core::TeamActorKind::Host,
                id: "host".into(),
                display_name: Some("Host".into()),
                authn_source: Some("test".into()),
            },
            authority_actor: None,
            causation_ref: None,
            idempotency_key: key.into(),
            created_at: at.into(),
        }
    }

    fn member_work_context(
        member_run_id: &str,
        id: &str,
        key: &str,
        at: &str,
    ) -> WorkCommandContext {
        WorkCommandContext {
            event_id: id.into(),
            performed_by_actor: harness_core::TeamActorRef {
                kind: harness_core::TeamActorKind::MemberRun,
                id: member_run_id.into(),
                display_name: None,
                authn_source: Some("bound-runtime:test".into()),
            },
            authority_actor: None,
            causation_ref: None,
            idempotency_key: key.into(),
            created_at: at.into(),
        }
    }

    fn unassigned_test_work(run_id: &str, id: &str) -> Work {
        Work {
            id: id.into(),
            team_run_id: run_id.into(),
            parent_work_id: None,
            source_work_item_ref: None,
            title: "Implement Work core".into(),
            context_markdown: "Build the smallest correct slice.".into(),
            completion_criteria_markdown: "Tests pass and state is reconstructable.".into(),
            status: WorkStatus::Open,
            owner_member_id: None,
            active_member_run_id: None,
            claim_mode: WorkClaimMode::TeamClaim,
            eligible_member_ids: Vec::new(),
            prerequisite_work_ids: Vec::new(),
            priority: harness_core::WorkPriority::High,
            created_by_actor: host_work_context("ignored", "ignored", "unix-ms:1")
                .performed_by_actor,
            result_summary: None,
            blocker_reason: None,
            artifact_refs: Vec::new(),
            check_refs: Vec::new(),
            version: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn completed_team_run(run: &AgentTeamRun, at: &str) -> AgentTeamRun {
        let mut completed = run.clone();
        completed.status = TeamRunStatus::Completed;
        completed.updated_at = at.into();
        completed.completed_at = Some(at.into());
        completed
    }

    #[test]
    fn team_run_completion_guard_is_store_authoritative() {
        let (root, store, run, _, _) = work_test_fixture("completion-guard");
        store
            .insert_work(
                unassigned_test_work(&run.id, "work-open"),
                host_work_context("we-open", "create-open", "unix-ms:2"),
            )
            .expect("create open Work");

        let error = store
            .compare_and_append_team_run_with_wave_status(
                &run,
                &completed_team_run(&run, "unix-ms:3"),
                WaveStatus::Waiting,
                "unix-ms:3",
            )
            .expect_err("Store must reject completion while Work is non-terminal");
        assert!(
            error
                .to_string()
                .contains("Works remain non-terminal: work-open (open, version 1)"),
            "completion guard should identify the authoritative unfinished Work: {error}"
        );
        assert_eq!(
            store
                .team_runs()
                .expect("read TeamRuns")
                .into_iter()
                .rev()
                .find(|candidate| candidate.id == run.id)
                .expect("TeamRun remains present")
                .status,
            TeamRunStatus::Running,
            "a rejected completion must not append a terminal TeamRun row"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn team_run_completion_and_work_create_serialize_without_invalid_state() {
        for iteration in 0..16 {
            let (root, store, run, _, _) =
                work_test_fixture(&format!("completion-create-race-{iteration}"));
            let barrier = Arc::new(Barrier::new(3));

            let completion_store = store.clone();
            let completion_run = run.clone();
            let completion_barrier = Arc::clone(&barrier);
            let completion = std::thread::spawn(move || {
                completion_barrier.wait();
                completion_store.compare_and_append_team_run_with_wave_status(
                    &completion_run,
                    &completed_team_run(&completion_run, "unix-ms:3"),
                    WaveStatus::Waiting,
                    "unix-ms:3",
                )
            });

            let work_store = store.clone();
            let work_run_id = run.id.clone();
            let work_barrier = Arc::clone(&barrier);
            let create = std::thread::spawn(move || {
                work_barrier.wait();
                work_store.insert_work(
                    unassigned_test_work(&work_run_id, "work-racing"),
                    host_work_context("we-racing", "create-racing", "unix-ms:2"),
                )
            });

            barrier.wait();
            let completion_result = completion.join().expect("completion thread");
            let create_result = create.join().expect("Work create thread");
            assert_ne!(
                completion_result.is_ok(),
                create_result.is_ok(),
                "the write lock must serialize the race so exactly one operation succeeds"
            );

            let latest_run = store
                .team_runs()
                .expect("read TeamRuns")
                .into_iter()
                .rev()
                .find(|candidate| candidate.id == run.id)
                .expect("TeamRun remains present");
            let has_nonterminal_work = store
                .latest_works()
                .expect("read Works")
                .into_iter()
                .any(|work| work.team_run_id == run.id && !work.is_terminal());
            assert!(
                latest_run.status != TeamRunStatus::Completed || !has_nonterminal_work,
                "completed TeamRun plus non-terminal Work is forbidden regardless of race winner"
            );

            std::fs::remove_dir_all(root).expect("remove temp store");
        }
    }

    #[test]
    fn work_lifecycle_is_event_authoritative_and_requires_host_acceptance() {
        let (root, store, run, member, _) = work_test_fixture("work-lifecycle");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-1"),
                host_work_context("we-1", "create-1", "unix-ms:2"),
            )
            .expect("create Work");
        assert_eq!(work.version, 1);
        assert_eq!(work.status, WorkStatus::Open);

        let claimed = store
            .claim_work(
                &work.id,
                1,
                &member.id,
                member_work_context(&member.id, "we-2", "claim-1", "unix-ms:3"),
            )
            .expect("claim Work");
        assert_eq!(claimed.status, WorkStatus::InProgress);
        assert_eq!(claimed.owner_member_id.as_deref(), Some("agent-a"));

        let submitted = store
            .submit_work(
                &work.id,
                2,
                &member.id,
                "Implemented and checked",
                vec!["artifact://patch".into()],
                vec!["check://unit".into()],
                member_work_context(&member.id, "we-3", "submit-1", "unix-ms:4"),
            )
            .expect("submit Work");
        assert_eq!(submitted.status, WorkStatus::Review);

        let accepted = store
            .accept_work_with_summary(
                &work.id,
                3,
                Some("Host accepted the checked implementation"),
                host_work_context("we-4", "accept-1", "unix-ms:5"),
            )
            .expect("Host accepts Work");
        assert_eq!(accepted.status, WorkStatus::Done);
        assert_eq!(store.work_events().expect("events").len(), 4);
        assert_eq!(
            store
                .work_events()
                .expect("events")
                .into_iter()
                .find(|event| event.id == "we-4")
                .expect("accept event")
                .payload["summary"],
            "Host accepted the checked implementation"
        );
        assert_eq!(store.latest_works().expect("works"), vec![accepted]);
        assert!(
            store
                .latest_work_deliveries()
                .expect("deliveries")
                .is_empty(),
            "a member self-claim is already runtime possession and must not create a loopback WorkDelivery"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn blocked_work_can_be_resumed_by_owner_or_host_with_a_recorded_resolution() {
        let (root, store, run, member, _) = work_test_fixture("work-resume");
        let mut assigned = unassigned_test_work(&run.id, "work-resume-owner");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-resume-1", "create-resume-1", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let started = store
            .start_work(
                &assigned.id,
                assigned.version,
                &member.id,
                member_work_context(&member.id, "we-resume-2", "start-resume-1", "unix-ms:3"),
            )
            .expect("start Work");
        let blocked = store
            .block_work(
                &started.id,
                started.version,
                &member.id,
                "dependency unavailable",
                member_work_context(&member.id, "we-resume-3", "block-resume-1", "unix-ms:4"),
            )
            .expect("owner blocks Work");
        let empty = store
            .resume_work(
                &blocked.id,
                blocked.version,
                &member.id,
                "  ",
                member_work_context(&member.id, "ignored", "empty-resolution", "unix-ms:5"),
            )
            .expect_err("resume requires a resolution");
        assert!(empty.to_string().contains("resolution is required"));
        let resumed = store
            .resume_work(
                &blocked.id,
                blocked.version,
                &member.id,
                "dependency restored",
                member_work_context(&member.id, "we-resume-4", "resume-owner", "unix-ms:5"),
            )
            .expect("owner resumes Work");
        assert_eq!(resumed.status, WorkStatus::InProgress);
        assert!(resumed.blocker_reason.is_none());
        let resumed_event = store
            .work_events()
            .expect("events")
            .into_iter()
            .find(|event| event.id == "we-resume-4")
            .expect("resumed event");
        assert_eq!(resumed_event.kind, WorkEventKind::Resumed);
        assert_eq!(resumed_event.payload["resolution"], "dependency restored");
        assert!(store
            .latest_work_deliveries()
            .expect("deliveries")
            .iter()
            .any(|delivery| {
                delivery.work_id == resumed.id
                    && delivery.work_version == resumed.version
                    && delivery.status == WorkDeliveryStatus::Queued
            }));

        let blocked_by_host = store
            .block_work_as_host(
                &resumed.id,
                resumed.version,
                "Host paused integration",
                host_work_context("we-resume-5", "block-host", "unix-ms:6"),
            )
            .expect("Host blocks Work");
        let resumed_by_host = store
            .resume_work_as_host(
                &blocked_by_host.id,
                blocked_by_host.version,
                "integration boundary cleared",
                host_work_context("we-resume-6", "resume-host", "unix-ms:7"),
            )
            .expect("Host resumes Work");
        assert_eq!(resumed_by_host.status, WorkStatus::InProgress);
        assert_eq!(resumed_by_host.active_member_run_id, Some(member.id));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn release_clears_safe_open_ownership_and_rejects_an_in_flight_delivery() {
        let (root, store, run, member, _) = work_test_fixture("work-release");
        let mut assigned = unassigned_test_work(&run.id, "work-release-safe");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-release-1", "create-release-1", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let released = store
            .release_work(
                &assigned.id,
                assigned.version,
                &member.id,
                member_work_context(&member.id, "we-release-2", "release-owner", "unix-ms:3"),
            )
            .expect("owner releases queued Work");
        assert_eq!(released.status, WorkStatus::Open);
        assert!(released.owner_member_id.is_none());
        assert!(released.active_member_run_id.is_none());
        assert!(store
            .latest_work_deliveries()
            .expect("deliveries")
            .iter()
            .any(|delivery| {
                delivery.work_id == released.id
                    && delivery.status == WorkDeliveryStatus::Invalidated
            }));

        let mut in_flight = unassigned_test_work(&run.id, "work-release-in-flight");
        in_flight.active_member_run_id = Some(member.id.clone());
        in_flight.claim_mode = WorkClaimMode::HostAssign;
        let in_flight = store
            .insert_work(
                in_flight,
                host_work_context("we-release-3", "create-release-2", "unix-ms:4"),
            )
            .expect("create second assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == in_flight.id)
            .expect("queued delivery");
        let lease = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-1", 11, "test:release", 100, 100)
            .expect("lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-release",
                101,
                "unix-ms:5",
            )
            .expect("claim delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
        };
        let error = store
            .release_work_as_host(
                &in_flight.id,
                in_flight.version,
                host_work_context("we-release-4", "release-host", "unix-ms:6"),
            )
            .expect_err("in-flight Work cannot be released");
        assert!(error.to_string().contains("RECONCILIATION_REQUIRED"));

        let _received = store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "native-receipt-release",
                102,
                "unix-ms:7",
            )
            .expect("record provider receipt");
        let received_error = store
            .release_work_as_host(
                &in_flight.id,
                in_flight.version,
                host_work_context("we-release-5", "release-received", "unix-ms:8"),
            )
            .expect_err("provider-received Work cannot be released");
        assert!(received_error
            .to_string()
            .contains("RECONCILIATION_REQUIRED"));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn historical_provider_receipt_does_not_lock_later_work_revisions() {
        let (root, store, run, member, peer) = work_test_fixture("historical-receipt");
        let mut assigned = unassigned_test_work(&run.id, "work-historical-receipt");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-history-1", "history-create", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == assigned.id)
            .expect("initial delivery");
        let lease = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-history", 3, "test", 100, 100)
            .expect("lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-history",
                101,
                "unix-ms:3",
            )
            .expect("claim delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
        };
        store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "native-receipt-history",
                102,
                "unix-ms:4",
            )
            .expect("provider receives revision 1");

        let mut failed_previous = member.clone();
        failed_previous.status = MemberRunStatus::Failed;
        failed_previous.finished_at = Some("unix-ms:5".into());
        store
            .append_member_run(&failed_previous)
            .expect("record runtime failure");
        let mut replacement = member.clone();
        replacement.id = "member-history-generation-2".into();
        replacement.runtime_generation += 1;
        replacement.status = MemberRunStatus::Idle;
        replacement.started_at = "unix-ms:6".into();
        replacement.finished_at = None;
        store
            .append_member_run(&replacement)
            .expect("append replacement runtime");

        let rebound = store
            .rebind_work(
                &assigned.id,
                assigned.version,
                &replacement.id,
                host_work_context("we-history-2", "history-rebind", "unix-ms:7"),
            )
            .expect("rebind advances Work beyond historical receipt");
        let released = store
            .release_work_as_host(
                &rebound.id,
                rebound.version,
                host_work_context("we-history-3", "history-release", "unix-ms:8"),
            )
            .expect("historical receipt must not block release of newer revision");
        let reassigned = store
            .assign_work(
                &released.id,
                released.version,
                &peer.id,
                host_work_context("we-history-4", "history-assign", "unix-ms:9"),
            )
            .expect("historical receipt must not block later assignment");
        assert_eq!(
            reassigned.active_member_run_id.as_deref(),
            Some(peer.id.as_str())
        );
        assert!(store
            .latest_work_deliveries()
            .expect("deliveries")
            .iter()
            .any(|candidate| {
                candidate.id == delivery.id
                    && candidate.status == WorkDeliveryStatus::ProviderReceived
                    && candidate.provider_receipt_id.as_deref() == Some("native-receipt-history")
            }));
        let reassigned_delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|candidate| {
                candidate.work_id == reassigned.id
                    && candidate.work_version == reassigned.version
                    && candidate.recipient_member_run_id == peer.id
            })
            .expect("reassigned delivery");
        let reassigned_claim = match store
            .claim_work_delivery(
                &run.id,
                &reassigned_delivery.id,
                &peer.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-reassigned-history",
                103,
                "unix-ms:10",
            )
            .expect("claim reassigned delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("reassigned delivery must be claimed"),
        };
        store
            .complete_work_delivery_claim(
                &run.id,
                &reassigned_delivery.id,
                &peer.id,
                &lease.supervisor_id,
                lease.generation,
                reassigned_claim.claim_id.as_deref().expect("claim id"),
                "native-receipt-reassigned",
                104,
                "unix-ms:11",
            )
            .expect("provider receives reassigned revision");
        let started = store
            .start_work(
                &reassigned.id,
                reassigned.version,
                &peer.id,
                member_work_context(&peer.id, "we-history-5", "history-start", "unix-ms:12"),
            )
            .expect("member advances beyond its provider receipt");
        let cancelled = store
            .cancel_work(
                &started.id,
                started.version,
                "Host no longer needs this Work",
                host_work_context("we-history-6", "history-cancel", "unix-ms:13"),
            )
            .expect("historical receipts must not block cancellation");
        assert_eq!(cancelled.status, WorkStatus::Cancelled);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn delivery_projection_folds_cross_file_updates_by_store_sequence() {
        let (root, store, run, member, _) = work_test_fixture("delivery-fold-sequence");
        let mut assigned = unassigned_test_work(&run.id, "work-fold-sequence");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-fold-1", "fold-create", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == assigned.id)
            .expect("initial delivery");
        let first = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-fold-1", 4, "test", 100, 10)
            .expect("first lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &first.supervisor_id,
                first.generation,
                "claim-fold",
                101,
                // Caller timestamps are deliberately non-monotonic. The
                // Store sequence, not this string, is authoritative.
                "unix-ms:999",
            )
            .expect("claim delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
        };
        assert_eq!(claimed.status, WorkDeliveryStatus::Claimed);
        let successor = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-fold-2", 5, "test", 111, 100)
            .expect("successor lease");
        store
            .reconcile_stale_work_delivery_claim(
                &run.id,
                &delivery.id,
                &successor.supervisor_id,
                successor.generation,
                112,
                "unix-ms:998",
            )
            .expect("standalone update requeues delivery");
        let released = store
            .release_work_as_host(
                &assigned.id,
                assigned.version,
                host_work_context("we-fold-2", "fold-release", "unix-ms:1"),
            )
            .expect("embedded update invalidates the later-requeued delivery");
        assert_eq!(released.version, 2);
        let projected = store
            .latest_work_deliveries()
            .expect("project deliveries")
            .into_iter()
            .find(|candidate| candidate.id == delivery.id)
            .expect("delivery remains as evidence");
        assert_eq!(projected.status, WorkDeliveryStatus::Invalidated);
        let standalone_updates = store
            .read_jsonl::<WorkDeliveryUpdate>("work_delivery_updates.jsonl")
            .expect("standalone updates");
        let embedded_updates = store
            .work_operations()
            .expect("operations")
            .into_iter()
            .flat_map(|operation| operation.delivery_updates)
            .collect::<Vec<_>>();
        assert!(standalone_updates
            .iter()
            .all(|update| update.update_sequence > 0));
        assert!(embedded_updates
            .iter()
            .all(|update| update.update_sequence > 0));
        assert!(
            embedded_updates
                .iter()
                .map(|update| update.update_sequence)
                .max()
                .expect("embedded sequence")
                > standalone_updates
                    .iter()
                    .map(|update| update.update_sequence)
                    .max()
                    .expect("standalone sequence")
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_event_id_reuse_is_rejected_before_delivery_identity_can_collide() {
        let (root, store, run, member, _) = work_test_fixture("event-id-uniqueness");
        let mut first = unassigned_test_work(&run.id, "work-event-id-first");
        first.active_member_run_id = Some(member.id.clone());
        first.claim_mode = WorkClaimMode::HostAssign;
        store
            .insert_work(
                first,
                host_work_context("same-work-event", "event-first", "unix-ms:2"),
            )
            .expect("first event and delivery");
        let mut second = unassigned_test_work(&run.id, "work-event-id-second");
        second.active_member_run_id = Some(member.id.clone());
        second.claim_mode = WorkClaimMode::HostAssign;
        let error = store
            .insert_work(
                second,
                host_work_context("same-work-event", "event-second", "unix-ms:3"),
            )
            .expect_err("caller event id reuse must be rejected");
        assert!(error.to_string().contains("WORK_EVENT_ID_CONFLICT"));
        assert_eq!(store.work_operations().expect("operations").len(), 1);
        assert_eq!(store.latest_work_deliveries().expect("deliveries").len(), 1);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn successor_supervisor_reconciles_a_stale_work_delivery_claim_before_reclaim() {
        let (root, store, run, member, _) = work_test_fixture("work-delivery-reconcile");
        let mut assigned = unassigned_test_work(&run.id, "work-reconcile");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        store
            .insert_work(
                assigned,
                host_work_context("we-reconcile-1", "create-reconcile", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == "work-reconcile")
            .expect("queued delivery");
        let first = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-1", 11, "test:first", 100, 10)
            .expect("first lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &first.supervisor_id,
                first.generation,
                "claim-generation-1",
                101,
                "unix-ms:3",
            )
            .expect("first claim")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be claimed"),
        };
        assert_eq!(claimed.attempt, 1);

        let second = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-2", 22, "test:successor", 111, 100)
            .expect("successor lease");
        assert_eq!(second.generation, 2);
        let requeued = store
            .reconcile_stale_work_delivery_claim(
                &run.id,
                &delivery.id,
                &second.supervisor_id,
                second.generation,
                112,
                "unix-ms:4",
            )
            .expect("successor reconciles stale claim");
        assert_eq!(requeued.status, WorkDeliveryStatus::Queued);
        assert_eq!(requeued.attempt, 1);
        assert!(requeued.claim_id.is_none());

        let reclaimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &second.supervisor_id,
                second.generation,
                "claim-generation-2",
                113,
                "unix-ms:5",
            )
            .expect("successor reclaims delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must be reclaimable"),
        };
        assert_eq!(reclaimed.attempt, 2);
        assert_eq!(reclaimed.claimed_generation, Some(second.generation));
        let received = store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &second.supervisor_id,
                second.generation,
                reclaimed.claim_id.as_deref().expect("second claim id"),
                "native-receipt-reconcile",
                114,
                "unix-ms:6",
            )
            .expect("record provider receipt");
        assert_eq!(received.status, WorkDeliveryStatus::ProviderReceived);
        assert_eq!(
            store
                .complete_work_delivery_claim(
                    &run.id,
                    &delivery.id,
                    &member.id,
                    &second.supervisor_id,
                    second.generation,
                    reclaimed.claim_id.as_deref().expect("second claim id"),
                    "native-receipt-reconcile",
                    115,
                    "unix-ms:6-retry",
                )
                .expect("same provider receipt retry is idempotent"),
            received
        );
        let different_receipt = store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &second.supervisor_id,
                second.generation,
                reclaimed.claim_id.as_deref().expect("second claim id"),
                "different-native-receipt",
                116,
                "unix-ms:6-retry-2",
            )
            .expect_err("a retry cannot rewrite receipt identity");
        assert!(different_receipt
            .to_string()
            .contains("different provider receipt"));
        let third = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-3", 33, "test:third", 212, 100)
            .expect("third lease");
        let uncertain = store
            .reconcile_stale_work_delivery_claim(
                &run.id,
                &delivery.id,
                &third.supervisor_id,
                third.generation,
                213,
                "unix-ms:7",
            )
            .expect_err("provider-received delivery is never rolled back");
        assert!(uncertain.to_string().contains("cannot be requeued"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_delivery_waits_for_prerequisites_and_current_lease_can_fail_its_claim() {
        let (root, store, run, member_a, member_b) = work_test_fixture("work-delivery-ready");
        let prerequisite = store
            .insert_work(
                unassigned_test_work(&run.id, "work-prerequisite"),
                host_work_context("we-ready-1", "ready-create-prereq", "unix-ms:2"),
            )
            .expect("create prerequisite");
        let claimed_prerequisite = store
            .claim_work(
                &prerequisite.id,
                prerequisite.version,
                &member_b.id,
                member_work_context(
                    &member_b.id,
                    "we-ready-2",
                    "ready-claim-prereq",
                    "unix-ms:3",
                ),
            )
            .expect("claim prerequisite");

        let mut dependent = unassigned_test_work(&run.id, "work-dependent");
        dependent.claim_mode = WorkClaimMode::HostAssign;
        dependent.active_member_run_id = Some(member_a.id.clone());
        dependent.prerequisite_work_ids = vec![prerequisite.id.clone()];
        let dependent = store
            .insert_work(
                dependent,
                host_work_context("we-ready-3", "ready-create-dependent", "unix-ms:4"),
            )
            .expect("create dependent");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == dependent.id)
            .expect("dependent delivery");
        let lease = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-ready", 7, "test", 100, 100)
            .expect("lease");
        assert_eq!(
            store
                .claim_work_delivery(
                    &run.id,
                    &delivery.id,
                    &member_a.id,
                    &lease.supervisor_id,
                    lease.generation,
                    "claim-before-ready",
                    101,
                    "unix-ms:5",
                )
                .expect("not ready is not an error"),
            WorkDeliveryClaimResult::NotQueued
        );

        let submitted = store
            .submit_work(
                &prerequisite.id,
                claimed_prerequisite.version,
                &member_b.id,
                "prerequisite complete",
                Vec::new(),
                vec!["check://ready".into()],
                member_work_context(
                    &member_b.id,
                    "we-ready-4",
                    "ready-submit-prereq",
                    "unix-ms:6",
                ),
            )
            .expect("submit prerequisite");
        store
            .accept_work(
                &submitted.id,
                submitted.version,
                host_work_context("we-ready-5", "ready-accept-prereq", "unix-ms:7"),
            )
            .expect("accept prerequisite");

        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member_a.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-after-ready",
                102,
                "unix-ms:8",
            )
            .expect("claim ready delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("delivery must now be claimable"),
        };
        let failed = store
            .fail_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member_a.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "provider transport exited before receipt",
                103,
                "unix-ms:9",
            )
            .expect("current lease fails claim");
        assert_eq!(failed.status, WorkDeliveryStatus::Failed);
        assert_eq!(
            failed.failure_reason.as_deref(),
            Some("provider transport exited before receipt")
        );
        assert_eq!(
            store
                .fail_work_delivery_claim(
                    &run.id,
                    &delivery.id,
                    &member_a.id,
                    &lease.supervisor_id,
                    lease.generation,
                    claimed.claim_id.as_deref().expect("claim id"),
                    "provider transport exited before receipt",
                    104,
                    "unix-ms:10",
                )
                .expect("same failure retry is idempotent"),
            failed
        );
        let retried = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member_a.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-after-transport-failure",
                105,
                "unix-ms:11",
            )
            .expect("failed pre-receipt delivery remains retryable")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => {
                panic!("failed pre-receipt delivery must be retryable")
            }
        };
        assert_eq!(retried.status, WorkDeliveryStatus::Claimed);
        assert_eq!(retried.attempt, 2);
        assert_eq!(
            retried.claim_id.as_deref(),
            Some("claim-after-transport-failure")
        );
        assert!(retried.failure_reason.is_none());
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn host_rebind_fences_old_runtime_and_preserves_provider_receipt_evidence() {
        let (root, store, run, member, peer) = work_test_fixture("work-rebind-runtime");
        let mut assigned = unassigned_test_work(&run.id, "work-rebind");
        assigned.claim_mode = WorkClaimMode::HostAssign;
        assigned.active_member_run_id = Some(member.id.clone());
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-rebind-1", "rebind-create", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|delivery| delivery.work_id == assigned.id)
            .expect("initial delivery");
        let lease = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-rebind", 9, "test", 100, 100)
            .expect("lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-rebind",
                101,
                "unix-ms:3",
            )
            .expect("claim initial delivery")
        {
            WorkDeliveryClaimResult::Claimed(delivery) => delivery,
            WorkDeliveryClaimResult::NotQueued => panic!("initial delivery must be queued"),
        };
        store
            .complete_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "provider-receipt-before-crash",
                102,
                "unix-ms:4",
            )
            .expect("provider receipt");
        let started = store
            .start_work(
                &assigned.id,
                assigned.version,
                &member.id,
                member_work_context(&member.id, "we-rebind-2", "rebind-start", "unix-ms:5"),
            )
            .expect("start before runtime crash");

        let mut failed_previous = member.clone();
        failed_previous.status = MemberRunStatus::Failed;
        failed_previous.finished_at = Some("unix-ms:6".into());
        store
            .append_member_run(&failed_previous)
            .expect("record previous runtime failure");

        let mut replacement = member.clone();
        replacement.id = "member-a-generation-2".into();
        replacement.runtime_generation = member.runtime_generation + 1;
        replacement.status = MemberRunStatus::Idle;
        replacement.started_at = "unix-ms:7".into();
        replacement.finished_at = None;
        store
            .append_member_run(&replacement)
            .expect("append replacement runtime");
        let owner_mismatch = store
            .rebind_work(
                &started.id,
                started.version,
                &peer.id,
                host_work_context("ignored", "rebind-peer", "unix-ms:8"),
            )
            .expect_err("Host cannot change stable owner through rebind");
        assert!(owner_mismatch.to_string().contains("OWNER_MISMATCH"));
        let rebound = store
            .rebind_work(
                &started.id,
                started.version,
                &replacement.id,
                host_work_context("we-rebind-3", "rebind-runtime", "unix-ms:9"),
            )
            .expect("Host rebinds stable owner to replacement runtime");
        assert_eq!(rebound.status, WorkStatus::InProgress);
        assert_eq!(rebound.owner_member_id, started.owner_member_id);
        assert_eq!(
            rebound.active_member_run_id.as_deref(),
            Some(replacement.id.as_str())
        );
        let deliveries = store.latest_work_deliveries().expect("deliveries");
        assert!(deliveries.iter().any(|candidate| {
            candidate.id == delivery.id
                && candidate.status == WorkDeliveryStatus::ProviderReceived
                && candidate.provider_receipt_id.as_deref() == Some("provider-receipt-before-crash")
        }));
        let replacement_delivery = deliveries
            .iter()
            .find(|candidate| {
                candidate.work_id == rebound.id
                    && candidate.work_version == rebound.version
                    && candidate.recipient_member_run_id == replacement.id
            })
            .expect("fresh delivery for replacement");
        assert!(matches!(
            store
                .claim_work_delivery(
                    &run.id,
                    &replacement_delivery.id,
                    &replacement.id,
                    &lease.supervisor_id,
                    lease.generation,
                    "claim-replacement",
                    103,
                    "unix-ms:11",
                )
                .expect("in-progress revision is deliverable"),
            WorkDeliveryClaimResult::Claimed(_)
        ));
        let fenced = store
            .submit_work(
                &started.id,
                started.version,
                &member.id,
                "stale runtime result",
                Vec::new(),
                Vec::new(),
                member_work_context(&member.id, "ignored", "stale-submit", "unix-ms:12"),
            )
            .expect_err("old runtime version is fenced");
        assert!(fenced.to_string().contains("VERSION_CONFLICT"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn unavailable_members_and_idempotency_key_reuse_are_rejected() {
        let (root, store, run, member, _) = work_test_fixture("work-command-guards");
        let first = store
            .insert_work(
                unassigned_test_work(&run.id, "work-idempotent-a"),
                host_work_context("we-guard-1", "shared-key", "unix-ms:2"),
            )
            .expect("first command");
        let other_work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-idempotent-b"),
                host_work_context("ignored", "shared-key", "unix-ms:3"),
            )
            .expect_err("same key cannot identify a different Work");
        assert!(other_work.to_string().contains("IDEMPOTENCY_CONFLICT"));
        let other_command = store
            .assign_work(
                &first.id,
                first.version,
                &member.id,
                host_work_context("ignored", "shared-key", "unix-ms:4"),
            )
            .expect_err("same key cannot identify a different command");
        assert!(other_command.to_string().contains("IDEMPOTENCY_CONFLICT"));

        let mut failed_member = member.clone();
        failed_member.status = MemberRunStatus::Failed;
        failed_member.finished_at = Some("unix-ms:5".into());
        store
            .append_member_run(&failed_member)
            .expect("record failed member");
        let mut assigned_to_failed = unassigned_test_work(&run.id, "work-failed-member");
        assigned_to_failed.claim_mode = WorkClaimMode::HostAssign;
        assigned_to_failed.active_member_run_id = Some(failed_member.id.clone());
        let failed = store
            .insert_work(
                assigned_to_failed,
                host_work_context("we-guard-2", "create-failed", "unix-ms:6"),
            )
            .expect_err("failed member cannot receive owned Work");
        assert!(failed.to_string().contains("MEMBER_UNAVAILABLE"));

        let mut stopped_member = failed_member;
        stopped_member.status = MemberRunStatus::Stopped;
        store
            .append_member_run(&stopped_member)
            .expect("record stopped member");
        let stopped_work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-assign-stopped"),
                host_work_context("we-guard-3", "create-for-stopped", "unix-ms:7"),
            )
            .expect("create unassigned Work");
        let stopped = store
            .assign_work(
                &stopped_work.id,
                stopped_work.version,
                &stopped_member.id,
                host_work_context("we-guard-4", "assign-stopped", "unix-ms:8"),
            )
            .expect_err("stopped member cannot be assigned");
        assert!(stopped.to_string().contains("MEMBER_UNAVAILABLE"));

        let mut closed_member = stopped_member;
        closed_member.status = MemberRunStatus::Idle;
        closed_member.coordination_status = harness_core::MemberCoordinationStatus::Closed;
        store
            .append_member_run(&closed_member)
            .expect("record closed coordination");
        let unassigned = store
            .insert_work(
                unassigned_test_work(&run.id, "work-assign-closed"),
                host_work_context("we-guard-5", "create-unassigned", "unix-ms:9"),
            )
            .expect("create unassigned Work");
        let closed = store
            .assign_work(
                &unassigned.id,
                unassigned.version,
                &closed_member.id,
                host_work_context("we-guard-6", "assign-closed", "unix-ms:10"),
            )
            .expect_err("closed member cannot be assigned");
        assert!(closed.to_string().contains("MEMBER_UNAVAILABLE"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn closed_member_cannot_mutate_owned_work_until_reopen() {
        let (root, store, run, member, _) = work_test_fixture("closed-member-owned-work");
        let created = store
            .insert_work(
                unassigned_test_work(&run.id, "work-owned-closed"),
                host_work_context("we-closed-1", "create-owned", "unix-ms:2"),
            )
            .expect("create Work");
        let assigned = store
            .assign_work(
                &created.id,
                created.version,
                &member.id,
                host_work_context("we-closed-2", "assign-owned", "unix-ms:3"),
            )
            .expect("assign Work");
        let started = store
            .start_work(
                &assigned.id,
                assigned.version,
                &member.id,
                member_work_context(&member.id, "we-closed-3", "start-owned", "unix-ms:4"),
            )
            .expect("start Work");

        // Close lands mid-execution: coordination flips Closed while the Work
        // stays owned and InProgress.
        let mut closed_member = member.clone();
        closed_member.coordination_status = harness_core::MemberCoordinationStatus::Closed;
        closed_member.status = MemberRunStatus::Stopped;
        store
            .append_member_run(&closed_member)
            .expect("record closed coordination");

        let blocked = store
            .block_work(
                &started.id,
                started.version,
                &member.id,
                "still blocked",
                member_work_context(&member.id, "we-closed-4", "block-owned", "unix-ms:5"),
            )
            .expect_err("closed member cannot block owned Work");
        assert!(
            blocked.to_string().contains("MEMBER_UNAVAILABLE"),
            "unexpected error: {blocked}"
        );
        let submitted = store
            .submit_work(
                &started.id,
                started.version,
                &member.id,
                "result from a closed runtime",
                Vec::new(),
                Vec::new(),
                member_work_context(&member.id, "we-closed-5", "submit-owned", "unix-ms:6"),
            )
            .expect_err("closed member cannot submit owned Work");
        assert!(
            submitted.to_string().contains("MEMBER_UNAVAILABLE"),
            "unexpected error: {submitted}"
        );
        // The Work projection is untouched by both rejections.
        let current = store
            .latest_works()
            .expect("latest works")
            .into_iter()
            .find(|work| work.id == started.id)
            .expect("owned Work");
        assert_eq!(current.status, WorkStatus::InProgress);
        assert_eq!(current.version, started.version);

        // Reopen (coordination Active, next runtime generation) restores the
        // member-side transition path for the same durable Work.
        let mut reopened_member = closed_member.clone();
        reopened_member.coordination_status = harness_core::MemberCoordinationStatus::Active;
        reopened_member.status = MemberRunStatus::Idle;
        reopened_member.runtime_generation += 1;
        store
            .append_member_run(&reopened_member)
            .expect("record reopened member");
        let submitted = store
            .submit_work(
                &started.id,
                started.version,
                &member.id,
                "result after reopen",
                Vec::new(),
                Vec::new(),
                member_work_context(&member.id, "we-closed-6", "submit-reopened", "unix-ms:7"),
            )
            .expect("reopened member submits owned Work");
        assert_eq!(submitted.status, WorkStatus::Review);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn concurrent_work_claim_has_exactly_one_winner_and_idempotent_retry() {
        let (root, store, run, member_a, member_b) = work_test_fixture("work-claim-race");
        store
            .insert_work(
                unassigned_test_work(&run.id, "work-race"),
                host_work_context("we-race-1", "create-race", "unix-ms:2"),
            )
            .expect("create Work");
        let store = Arc::new(store);
        let barrier = Arc::new(Barrier::new(3));
        let handles = [member_a, member_b]
            .into_iter()
            .enumerate()
            .map(|(index, member)| {
                let store = Arc::clone(&store);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    store.claim_work(
                        "work-race",
                        1,
                        &member.id,
                        member_work_context(
                            &member.id,
                            &format!("we-race-{}", index + 2),
                            &format!("claim-race-{index}"),
                            "unix-ms:3",
                        ),
                    )
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("claim thread"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        let winner = results.into_iter().find_map(Result::ok).expect("winner");
        let retry_member = winner
            .active_member_run_id
            .as_deref()
            .expect("active member");
        let retried = store
            .claim_work(
                "work-race",
                1,
                retry_member,
                member_work_context(
                    retry_member,
                    "ignored",
                    if retry_member.ends_with("-a") {
                        "claim-race-0"
                    } else {
                        "claim-race-1"
                    },
                    "unix-ms:4",
                ),
            )
            .expect("idempotent retry");
        assert_eq!(retried, winner);
        assert!(
            store
                .latest_work_deliveries()
                .expect("deliveries")
                .is_empty(),
            "the winning Member already possesses self-claimed Work in its bound runtime"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn member_created_work_is_limited_to_self_or_unassigned() {
        let (root, store, run, member_a, member_b) = work_test_fixture("member-work-authority");

        let mut peer_owned = unassigned_test_work(&run.id, "work-peer-owned");
        peer_owned.active_member_run_id = Some(member_b.id.clone());
        peer_owned.claim_mode = WorkClaimMode::HostAssign;
        let error = store
            .insert_work(
                peer_owned,
                member_work_context(
                    &member_a.id,
                    "we-member-peer",
                    "member-create-peer",
                    "unix-ms:2",
                ),
            )
            .expect_err("ordinary Member must not assign peer-owned Work");
        assert!(
            error
                .to_string()
                .contains("only self-owned or unassigned Work"),
            "error: {error}"
        );

        let mut self_owned = unassigned_test_work(&run.id, "work-self-owned");
        self_owned.active_member_run_id = Some(member_a.id.clone());
        self_owned.claim_mode = WorkClaimMode::HostAssign;
        let self_owned = store
            .insert_work(
                self_owned,
                member_work_context(
                    &member_a.id,
                    "we-member-self",
                    "member-create-self",
                    "unix-ms:3",
                ),
            )
            .expect("Member creates self-owned Work");
        assert_eq!(
            self_owned.active_member_run_id.as_deref(),
            Some(member_a.id.as_str())
        );
        assert_eq!(self_owned.owner_member_id.as_deref(), Some("agent-a"));

        let unassigned = store
            .insert_work(
                unassigned_test_work(&run.id, "work-unassigned-child"),
                member_work_context(
                    &member_a.id,
                    "we-member-open",
                    "member-create-open",
                    "unix-ms:4",
                ),
            )
            .expect("Member creates unassigned Work");
        assert!(unassigned.owner_member_id.is_none());
        assert!(unassigned.active_member_run_id.is_none());

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn team_message_work_link_must_resolve_inside_the_same_team_run() {
        let (root, store, run, member, _) = work_test_fixture("message-work-link");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-discussed"),
                host_work_context("we-discussed", "create-discussed", "unix-ms:2"),
            )
            .expect("create discussed Work");
        let message = TeamMessage {
            id: "tm-work-discussion".into(),
            team_run_id: run.id.clone(),
            work_id: Some(work.id.clone()),
            origin_wave_id: None,
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec![member.id.clone()],
            kind: TeamMessageKind::Message,
            body: "Clarify the evidence for this Work.".into(),
            correlation_id: "corr-work-discussion".into(),
            causation_id: None,
            response_intent: Some(TeamMessageResponseIntent::ResponseRequired),
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: member.id.clone(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                updated_at: "unix-ms:3".into(),
            }],
            created_at: "unix-ms:3".into(),
        };
        store
            .append_team_message_checked(&message)
            .expect("same-TeamRun Work discussion");

        let mut foreign = message;
        foreign.id = "tm-cross-run-work".into();
        foreign.team_run_id = "another-team-run".into();
        let error = store
            .append_team_message_checked(&foreign)
            .expect_err("cross-TeamRun Work link must be rejected");
        assert!(
            error.to_string().contains("belongs to TeamRun"),
            "error: {error}"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn legacy_assignment_message_is_not_readable_by_works_store() {
        let (root, store, run, _, _) = work_test_fixture("legacy-work-store");
        append_sparse_row(
            &root,
            "team_messages.jsonl",
            &format!(
                r#"{{"id":"legacy-assignment","team_run_id":"{}","from_member_id":"host","kind":"assignment","body":"legacy","correlation_id":"legacy","created_at":"unix-ms:1"}}"#,
                run.id
            ),
        );
        let error = store
            .insert_work(
                unassigned_test_work(&run.id, "work-rejected"),
                host_work_context("we-rejected", "create-rejected", "unix-ms:2"),
            )
            .expect_err("legacy store must be rejected");
        assert!(error.to_string().contains("assignment"));
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

    fn test_agent_team(
        id: &str,
        member_ids: &[&str],
        parent_team_id: Option<&str>,
        host_member_id: Option<&str>,
    ) -> AgentTeam {
        AgentTeam {
            id: id.into(),
            name: format!("{id} name"),
            description: format!("{id} description"),
            owner_agent_id: "host".into(),
            status: harness_core::AgentTeamStatus::Active,
            member_ids: member_ids.iter().map(|id| id.to_string()).collect(),
            parent_team_id: parent_team_id.map(str::to_string),
            host_member_id: host_member_id.map(str::to_string),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        }
    }

    fn temp_store(label: &str) -> (PathBuf, HarnessStore) {
        let root = std::env::temp_dir().join(format!(
            "harness-store-{label}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_nanos()
        ));
        let store = HarnessStore::new(&root);
        (root, store)
    }

    #[test]
    fn insert_agent_team_persists_and_folds_latest_projection() {
        let (root, store) = temp_store("team-topology-insert");
        let root_team = test_agent_team("root", &["lead", "cto"], None, Some("lead"));
        let child = test_agent_team("child", &["worker"], Some("root"), Some("cto"));
        store.insert_agent_team(&root_team).expect("insert root");
        store.insert_agent_team(&child).expect("insert child");

        let teams = store.latest_teams().expect("latest teams");
        assert_eq!(teams.len(), 2);
        assert_eq!(
            harness_core::team_subtree_ids(&teams, "root"),
            vec!["root".to_string(), "child".to_string()]
        );
        assert_eq!(
            harness_core::child_team_ids(&teams, "root"),
            vec!["child".to_string()]
        );
        assert_eq!(
            harness_core::team_ancestor_ids(&teams, "child"),
            vec!["root".to_string()]
        );

        // A later revision of the same id folds into one latest row.
        let mut renamed = root_team.clone();
        renamed.name = "Root Renamed".into();
        renamed.updated_at = "unix-ms:2".into();
        store.append_team(&renamed).expect("append rename revision");
        let teams = store.latest_teams().expect("latest teams after rename");
        assert_eq!(teams.len(), 2);
        assert_eq!(teams["root"].name, "Root Renamed");
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn insert_agent_team_rejects_duplicate_id_under_lock() {
        let (root, store) = temp_store("team-topology-duplicate");
        let team = test_agent_team("root", &[], None, None);
        store.insert_agent_team(&team).expect("insert first");
        let error = store
            .insert_agent_team(&team)
            .expect_err("duplicate id must be rejected");
        assert!(matches!(error, StoreError::Conflict(_)));
        assert!(error.to_string().contains("already exists"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn insert_agent_team_enforces_topology_invariants() {
        let (root, store) = temp_store("team-topology-guard");
        store
            .insert_agent_team(&test_agent_team(
                "root",
                &["lead", "cto"],
                None,
                Some("lead"),
            ))
            .expect("insert root");

        // Unknown parent.
        let error = store
            .insert_agent_team(&test_agent_team(
                "orphan",
                &[],
                Some("missing"),
                Some("cto"),
            ))
            .expect_err("unknown parent must be rejected");
        assert!(error.to_string().contains("missing parent AgentTeam"));

        // Non-root without a durable host.
        let error = store
            .insert_agent_team(&test_agent_team("hostless", &[], Some("root"), None))
            .expect_err("non-root without host must be rejected");
        assert!(error.to_string().contains("host_member_id"));

        // Host that is not a direct member of the parent team.
        let error = store
            .insert_agent_team(&test_agent_team(
                "stranger-hosted",
                &[],
                Some("root"),
                Some("outsider"),
            ))
            .expect_err("host outside parent membership must be rejected");
        assert!(error.to_string().contains("not a direct member"));

        // One member hosting a second team.
        let error = store
            .insert_agent_team(&test_agent_team(
                "second-child",
                &[],
                Some("root"),
                Some("lead"),
            ))
            .expect_err("member hosting two teams must be rejected");
        assert!(error.to_string().contains("more than one AgentTeam"));

        // A valid child still inserts after all rejections, proving the failed
        // candidates were never appended.
        store
            .insert_agent_team(&test_agent_team(
                "child",
                &["worker"],
                Some("root"),
                Some("cto"),
            ))
            .expect("valid child inserts");
        assert_eq!(store.latest_teams().expect("latest teams").len(), 2);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn insert_agent_team_rejects_cycle_candidates() {
        let (root, store) = temp_store("team-topology-cycle");
        store
            .insert_agent_team(&test_agent_team("root", &["lead"], None, Some("lead")))
            .expect("insert root");
        // Self-parent candidate with its own distinct host: parent resolution,
        // host presence, direct-host, and host-uniqueness all pass, so the
        // acyclic invariant is the one that must reject it.
        let error = store
            .insert_agent_team(&test_agent_team(
                "loop",
                &["loopy"],
                Some("loop"),
                Some("loopy"),
            ))
            .expect_err("self-parent must be rejected");
        assert!(
            error.to_string().contains("cycle"),
            "unexpected error: {error}"
        );
        assert_eq!(store.latest_teams().expect("latest teams").len(), 1);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }
}

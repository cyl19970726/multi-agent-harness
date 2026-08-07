use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use harness_core::{
    validate_agent_team_topology, validate_work_cutover_with_fences, AgentEvent, AgentMember,
    AgentMemberStatus, AgentMessageRoute, AgentRuntime, AgentTeam, AgentTeamRun, Decision,
    DelegationRun, DurableAgentMember, Evidence, Gap, GitHubLink, HostAttention,
    HostAttentionInbox, HostAttentionKind, HostAttentionStatus, MemberAction, MemberRun, Message,
    MessageDelivery, MessageDeliveryStatus, MessageTerminalSource, Mission, MissionLogEntry,
    MissionStatus, PendingInteraction, Proposal, ProviderChildThread, ProviderExecutionStatus,
    Review, TeamDeliveryPolicy, TeamDeliveryStatus, TeamMemberCloseRequest, TeamMemberCloseStatus,
    TeamMessage, TeamMessageKind, TeamRunEvent, TeamRunStatus, TeamSupervisorLease,
    TeamSupervisorLeaseStatus, Validate, Vision, Wave, WaveExecutorKind, WaveGateStatus,
    WaveStatus, Work, WorkClaimMode, WorkCommandContext, WorkCutoverFence, WorkCutoverReport,
    WorkDelivery, WorkDeliveryStatus, WorkDeliveryUpdate, WorkEvent, WorkEventKind, WorkItem,
    WorkItemStatus, WorkOperation, WorkStatus, WorkflowArtifactManifest, WorkflowPatch,
    WorkflowRun, WorkflowStep,
};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

mod company_os;
pub mod docs_v2;
pub use company_os::{
    ActionAuditReservation, ActionCommandClaimResult, CompanyActor, FinancialRecord,
};

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
}

const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;
const COMPANY_WORK_ITEMS_LEDGER: &str = "company_os_work_items.jsonl";
const WORK_CUTOVER_FENCES_LEDGER: &str = "company_os_work_cutover_fences.jsonl";

/// Normalize surface identifiers into their canonical form.
/// All surface comparisons and storage MUST route through this.
/// Aliases: kimi|kimi-cli|kimi-code → kimi; codex|codex-app|codex-app-server → codex;
/// claude|claude-code → claude. Unknown surfaces pass through unchanged.
pub fn canonical_surface(surface: &str) -> &str {
    match surface {
        "kimi" | "kimi-cli" | "kimi-code" => "kimi",
        "codex" | "codex-app" | "codex-app-server" => "codex",
        "claude" | "claude-code" => "claude",
        other => other,
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostAttentionClaimResult {
    Claimed(Box<HostAttention>),
    NotActionable,
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

    /// Append one [`MissionLogEntry`] under the store lock, atomically
    /// allocating its monotonic `revision` the same way
    /// `insert_wave_and_update_mission` allocates a Wave index: read the
    /// current max for this `mission_id`, then `+ 1` (starting at 1). This
    /// is the Mission Log's ONLY write operation (ADR 0051) — there is no
    /// update or delete, so unlike Wave there is no compare-and-append
    /// variant to race against.
    pub fn append_mission_log_entry(
        &self,
        mut entry: MissionLogEntry,
    ) -> StoreResult<MissionLogEntry> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        if entry.body.trim().is_empty() {
            return Err(StoreError::Conflict(
                "mission log entry body must not be empty".to_string(),
            ));
        }
        if entry.actor.trim().is_empty() {
            return Err(StoreError::Conflict(
                "mission log entry actor must not be empty".to_string(),
            ));
        }
        let missions = latest_by_id(self.read_jsonl::<Mission>("missions.jsonl")?, |mission| {
            mission.id.clone()
        });
        if !missions.contains_key(&entry.mission_id) {
            return Err(StoreError::Conflict(format!(
                "mission not found: {}",
                entry.mission_id
            )));
        }
        let existing = self.read_jsonl::<MissionLogEntry>("mission_log.jsonl")?;
        if existing.iter().any(|row| row.id == entry.id) {
            return Err(StoreError::Conflict(format!(
                "mission log entry already exists: {}",
                entry.id
            )));
        }
        entry.revision = existing
            .iter()
            .filter(|row| row.mission_id == entry.mission_id)
            .map(|row| row.revision)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "mission log revision space is exhausted for mission {}",
                    entry.mission_id
                ))
            })?;
        self.append_jsonl_unlocked("mission_log.jsonl", &entry)?;
        Ok(entry)
    }

    /// Atomically close one Mission. Prior to ADR 0051 this required every
    /// ordered Wave to have an accepted, completed gate; Wave write commands
    /// (including the gate) are now retired, so a native post-cutover
    /// Mission always has empty `wave_ids` and closes on its own outcome —
    /// the Host records `kind = closeout_evidence` in the Mission Log
    /// beforehand by convention, not as a store-enforced precondition (ADR
    /// 0051 "Mission closeout evidence becomes a ... Log entry instead of a
    /// separate Wave-outcome convention"). A legacy Mission that already
    /// accumulated `wave_ids` before the cutover keeps the original
    /// Wave-gate requirement so its in-flight contract does not change
    /// underneath it; no NEW Mission can reach that branch since Wave create
    /// no longer populates membership. The Wave set is still checked under
    /// the same store lock as the Mission CAS so a concurrent Wave create
    /// (of a legacy, still-populated Mission) cannot race closeout.
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
        if !current.wave_ids.is_empty() {
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
    /// invariants (ADR 0052) against the latest projection plus the candidate
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

    /// Insert one slim, durable Organization identity under the store lock.
    /// Provider/runtime/session state belongs to MemberRun and native sessions,
    /// never to this ledger (ADR 0052).
    pub fn insert_durable_member(&self, value: &DurableAgentMember) -> StoreResult<()> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let members = latest_by_id(
            self.read_jsonl::<DurableAgentMember>("durable_agent_members.jsonl")?,
            |member| member.id.clone(),
        );
        if members.contains_key(&value.id) {
            return Err(StoreError::Conflict(format!(
                "durable AgentMember already exists: {}",
                value.id
            )));
        }
        self.append_jsonl_unlocked("durable_agent_members.jsonl", value)
    }

    /// Explicitly converge one row from the legacy runtime-heavy AgentMember
    /// registry into the durable identity ledger. Existing identical results
    /// are idempotent; divergent re-projections are refused.
    pub fn converge_registry_member(
        &self,
        value: &DurableAgentMember,
    ) -> StoreResult<DurableAgentMember> {
        value
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let registry = latest_by_id(self.read_jsonl::<AgentMember>("members.jsonl")?, |member| {
            member.id.clone()
        });
        let source = registry.get(&value.id).ok_or_else(|| {
            StoreError::Conflict(format!("compatibility AgentMember not found: {}", value.id))
        })?;
        let expected_status = match source.status {
            AgentMemberStatus::Retired => harness_core::DurableAgentMemberStatus::Retired,
            AgentMemberStatus::Paused
            | AgentMemberStatus::Stale
            | AgentMemberStatus::Closed
            | AgentMemberStatus::Closing => harness_core::DurableAgentMemberStatus::Paused,
            _ => harness_core::DurableAgentMemberStatus::Active,
        };
        let expected_profile = source
            .profile
            .clone()
            .or_else(|| Some(source.provider.clone()));
        if value.name != source.name
            || value.description != source.description
            || value.role != source.role
            || value.provider_profile != expected_profile
            || value.model != source.model
            || value.workspace_policy != source.workspace_policy
            || value.status != expected_status
            || value.created_at != source.created_at
            || value.updated_at != source.created_at
        {
            return Err(StoreError::Conflict(format!(
                "durable AgentMember {} does not match its deterministic compatibility projection",
                value.id
            )));
        }
        let durable = latest_by_id(
            self.read_jsonl::<DurableAgentMember>("durable_agent_members.jsonl")?,
            |member| member.id.clone(),
        );
        if let Some(existing) = durable.get(&value.id) {
            if existing == value {
                return Ok(existing.clone());
            }
            return Err(StoreError::Conflict(format!(
                "durable AgentMember {} already exists with different identity fields",
                value.id
            )));
        }
        self.append_jsonl_unlocked("durable_agent_members.jsonl", value)?;
        Ok(value.clone())
    }

    /// Bootstrap the durable Lead for an existing root Team and converge the
    /// compatibility `owner_agent_id` alias to the same identity. The operation
    /// refuses non-root Teams, conflicting owners, and divergent duplicate
    /// identities; it never manufactures a second Host authority.
    pub fn bootstrap_root_lead_member(
        &self,
        root_team_id: &str,
        member: &DurableAgentMember,
    ) -> StoreResult<AgentTeam> {
        member
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut teams = latest_by_id(self.read_jsonl::<AgentTeam>("teams.jsonl")?, |team| {
            team.id.clone()
        });
        let mut root = teams.remove(root_team_id).ok_or_else(|| {
            StoreError::Conflict(format!("root AgentTeam not found: {root_team_id}"))
        })?;
        if root.parent_team_id.is_some() {
            return Err(StoreError::Conflict(format!(
                "AgentTeam {root_team_id} is not a root Team"
            )));
        }
        if root.owner_agent_id != "host" && root.owner_agent_id != member.id {
            return Err(StoreError::Conflict(format!(
                "AgentTeam {root_team_id} has conflicting compatibility owner {}",
                root.owner_agent_id
            )));
        }
        if root
            .host_member_id
            .as_deref()
            .is_some_and(|id| id != member.id)
        {
            return Err(StoreError::Conflict(format!(
                "AgentTeam {root_team_id} already has a different durable Host"
            )));
        }

        let durable = latest_by_id(
            self.read_jsonl::<DurableAgentMember>("durable_agent_members.jsonl")?,
            |row| row.id.clone(),
        );
        let should_append_member = match durable.get(&member.id) {
            Some(existing) if existing == member => false,
            Some(_) => {
                return Err(StoreError::Conflict(format!(
                    "durable AgentMember {} already exists with different identity fields",
                    member.id
                )))
            }
            None => true,
        };

        let team_already_converged = root.owner_agent_id == member.id
            && root.host_member_id.as_deref() == Some(member.id.as_str())
            && root.member_ids.iter().any(|id| id == &member.id)
            && root.updated_at == member.updated_at;
        root.owner_agent_id = member.id.clone();
        root.host_member_id = Some(member.id.clone());
        if !root.member_ids.iter().any(|id| id == &member.id) {
            root.member_ids.push(member.id.clone());
        }
        root.updated_at = member.updated_at.clone();
        teams.insert(root.id.clone(), root.clone());
        validate_agent_team_topology(&teams)
            .map_err(|error| StoreError::Conflict(error.to_string()))?;

        if should_append_member {
            self.append_jsonl_unlocked("durable_agent_members.jsonl", member)?;
        }
        if !team_already_converged {
            self.append_jsonl_unlocked("teams.jsonl", &root)?;
        }
        Ok(root)
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

    /// Idempotently append one durable Host-attention fact.
    ///
    /// Runtime integration must derive `attention.id` from the causal event
    /// (for example `host-attention-<work-event-id>`). Replaying the same event
    /// returns the latest delivery/intake projection instead of resetting it
    /// to `actionable` or fabricating a TeamMessage.
    pub fn ensure_host_attention(&self, attention: &HostAttention) -> StoreResult<HostAttention> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_host_attention_unlocked(attention)
    }

    /// Repair the only intentional two-ledger crash boundary: a WorkOperation
    /// may be fsynced immediately before its derived HostAttention row. The
    /// deterministic attention id makes this replay safe and lets Host reads or
    /// an explicit startup reconciliation materialize exactly the missing row.
    pub fn reconcile_work_host_attentions(&self) -> StoreResult<Vec<HostAttention>> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()
    }

    /// Latest-wins Host-attention projection across all TeamRuns.
    pub fn host_attentions(&self) -> StoreResult<Vec<HostAttention>> {
        self.reconcile_work_host_attentions()?;
        Ok(self
            .latest_host_attentions_unlocked()?
            .into_values()
            .collect())
    }

    /// Read one TeamRun's Host-attention projection, including an explicit
    /// warning when no exact native Host task is bound.
    pub fn host_attention_inbox_for_team_run(
        &self,
        team_run_id: &str,
        include_all: bool,
    ) -> StoreResult<HostAttentionInbox> {
        self.reconcile_work_host_attentions()?;
        self.host_attention_inbox_for_team_run_unreconciled(team_run_id, include_all)
    }

    /// Aggregate only attentions owned by the exact provider-native Host task.
    /// Unbound TeamRuns and other tasks are excluded by construction.
    pub fn host_attention_inboxes_for_native_thread(
        &self,
        host_surface: &str,
        host_thread_id: &str,
        include_all: bool,
    ) -> StoreResult<Vec<HostAttentionInbox>> {
        if host_surface.trim().is_empty() || host_thread_id.trim().is_empty() {
            return Err(StoreError::Conflict(
                "Host surface and native thread id must not be empty".to_string(),
            ));
        }
        self.reconcile_work_host_attentions()?;
        let runs = latest_by_id(self.read_jsonl::<AgentTeamRun>("team_runs.jsonl")?, |run| {
            run.id.clone()
        });
        let mut inboxes = Vec::new();
        for run in runs.into_values().filter(|run| {
            canonical_surface(&run.host_surface) == canonical_surface(host_surface)
                && run.host_thread_id.as_deref() == Some(host_thread_id)
        }) {
            let inbox =
                self.host_attention_inbox_for_team_run_unreconciled(&run.id, include_all)?;
            if include_all || !inbox.attentions.is_empty() {
                inboxes.push(inbox);
            }
        }
        Ok(inboxes)
    }

    /// Fence one delivery attempt to the TeamRun's current exact Host binding.
    /// A claimed or delivered row cannot be claimed again, which prevents a
    /// managed idle wake and a safe-boundary hook from both starting delivery.
    pub fn claim_host_attention(
        &self,
        attention_id: &str,
        host_surface: &str,
        host_thread_id: &str,
        claim_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttentionClaimResult> {
        require_non_empty_store(attention_id, "Host attention id")?;
        require_non_empty_store(host_surface, "Host surface")?;
        require_non_empty_store(host_thread_id, "Host thread id")?;
        require_non_empty_store(claim_id, "Host attention claim id")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.reconcile_work_host_attentions_unlocked()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        self.require_exact_host_binding_unlocked(
            &attention.team_run_id,
            host_surface,
            host_thread_id,
        )?;
        if attention.status == HostAttentionStatus::Claimed
            && attention.claim_id.as_deref() == Some(claim_id)
            && attention.claimed_host_surface.as_deref() == Some(host_surface)
            && attention.claimed_host_thread_id.as_deref() == Some(host_thread_id)
        {
            return Ok(HostAttentionClaimResult::Claimed(Box::new(attention)));
        }
        if attention.status != HostAttentionStatus::Actionable {
            return Ok(HostAttentionClaimResult::NotActionable);
        }
        attention.status = HostAttentionStatus::Claimed;
        attention.attempt = attention.attempt.saturating_add(1);
        attention.claim_id = Some(claim_id.to_string());
        attention.claimed_host_surface = Some(host_surface.to_string());
        attention.claimed_host_thread_id = Some(host_thread_id.to_string());
        attention.provider_receipt_id = None;
        attention.last_failure_reason = None;
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(HostAttentionClaimResult::Claimed(Box::new(attention)))
    }

    /// Record provider-native delivery receipt for the currently-owned claim.
    pub fn complete_host_attention_claim(
        &self,
        attention_id: &str,
        claim_id: &str,
        provider_receipt_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(provider_receipt_id, "Host attention provider receipt")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        if attention.status == HostAttentionStatus::Delivered
            && attention.claim_id.as_deref() == Some(claim_id)
            && attention.provider_receipt_id.as_deref() == Some(provider_receipt_id)
        {
            return Ok(attention);
        }
        if attention.status != HostAttentionStatus::Claimed
            || attention.claim_id.as_deref() != Some(claim_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention claim {claim_id} no longer owns {attention_id}"
            )));
        }
        let surface = attention.claimed_host_surface.clone().ok_or_else(|| {
            StoreError::Conflict("claimed HostAttention has no Host surface".to_string())
        })?;
        let thread_id = attention.claimed_host_thread_id.clone().ok_or_else(|| {
            StoreError::Conflict("claimed HostAttention has no Host thread id".to_string())
        })?;
        self.require_exact_host_binding_unlocked(&attention.team_run_id, &surface, &thread_id)?;
        attention.status = HostAttentionStatus::Delivered;
        attention.provider_receipt_id = Some(provider_receipt_id.to_string());
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// Return an uncertain/failed claim to the actionable state for retry.
    pub fn fail_host_attention_claim(
        &self,
        attention_id: &str,
        claim_id: &str,
        reason: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(reason, "Host attention failure reason")?;
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        if attention.status != HostAttentionStatus::Claimed
            || attention.claim_id.as_deref() != Some(claim_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention claim {claim_id} no longer owns {attention_id}"
            )));
        }
        attention.status = HostAttentionStatus::Actionable;
        attention.claim_id = None;
        attention.claimed_host_surface = None;
        attention.claimed_host_thread_id = None;
        attention.provider_receipt_id = None;
        attention.last_failure_reason = Some(reason.to_string());
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
    }

    /// ACK transport intake from the exact currently-bound Host task. This is
    /// intentionally independent of Work accept/request-changes commands.
    pub fn acknowledge_host_attention(
        &self,
        attention_id: &str,
        host_surface: &str,
        host_thread_id: &str,
        updated_at: &str,
    ) -> StoreResult<HostAttention> {
        require_non_empty_store(updated_at, "Host attention updated_at")?;
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        let mut attention = self.require_host_attention_unlocked(attention_id)?;
        self.require_exact_host_binding_unlocked(
            &attention.team_run_id,
            host_surface,
            host_thread_id,
        )?;
        if attention.status == HostAttentionStatus::Acknowledged {
            return Ok(attention);
        }
        if attention.status != HostAttentionStatus::Delivered
            || attention
                .claimed_host_surface
                .as_deref()
                .map(canonical_surface)
                != Some(canonical_surface(host_surface))
            || attention.claimed_host_thread_id.as_deref() != Some(host_thread_id)
        {
            return Err(StoreError::Conflict(format!(
                "HostAttention {attention_id} has not been delivered to this exact Host task"
            )));
        }
        attention.status = HostAttentionStatus::Acknowledged;
        attention.updated_at = updated_at.to_string();
        self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
        Ok(attention)
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
        let run_team_id = durable_team_id(&team_run);
        match (work.team_id.as_deref(), run_team_id) {
            (Some(work_team_id), Some(run_team_id)) if work_team_id != run_team_id => {
                return Err(StoreError::Conflict(format!(
                    "TEAM_SCOPE_MISMATCH: Work names AgentTeam {work_team_id}, but TeamRun {} belongs to {run_team_id}",
                    team_run.id
                )));
            }
            (Some(_), Some(_)) if work.source_work_item_ref.is_some() => {
                return Err(StoreError::Conflict(
                    "SOURCE_WORK_ITEM_REQUIRES_EXPLICIT_CUTOVER: create the compatibility Work first, retire its Company WorkItem authority, then run Work promote"
                        .to_string(),
                ));
            }
            (None, Some(run_team_id)) if work.source_work_item_ref.is_none() => {
                work.team_id = Some(run_team_id.to_string())
            }
            // A source-linked Work stays in readable TeamRun compatibility
            // scope until the explicit promotion command validates the
            // independently selected Company Store.
            (None, Some(_)) => {}
            (Some(_), None) => {
                return Err(StoreError::Conflict(format!(
                    "TEAM_SCOPE_UNAVAILABLE: TeamRun {} has no durable AgentTeam identity",
                    team_run.id
                )));
            }
            _ => {}
        }
        if self.latest_works_unlocked()?.contains_key(work.id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "work already exists: {}",
                work.id
            )));
        }
        if !context.duplicate_ok {
            let normalized = normalize_work_title(&work.title);
            for existing in self.latest_works_unlocked()?.values() {
                if existing.team_run_id == work.team_run_id
                    && !existing.is_terminal()
                    && normalize_work_title(&existing.title) == normalized
                {
                    return Err(StoreError::Conflict(format!(
                        "DUPLICATE_TITLE: a non-terminal Work ({}) with title \"{}\" already exists in team run {}; pass --duplicate-ok to skip this guard",
                        existing.id, existing.title, work.team_run_id
                    )));
                }
            }
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
        work.created_by_actor = context.performed_by_actor.clone();
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
                    .created_by_member_id
                    .as_deref()
                    .is_some_and(|creator| creator != own_identity)
                {
                    return Err(StoreError::Conflict(
                        "created_by_member_id does not match creator MemberRun stable identity"
                            .to_string(),
                    ));
                }
                work.created_by_member_id = Some(own_identity.clone());
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
            _ => {
                require_host_actor(&context.performed_by_actor)?;
                if work.created_by_member_id.is_some() {
                    return Err(StoreError::Conflict(
                        "only a MemberRun actor may set created_by_member_id".to_string(),
                    ));
                }
            }
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
        self.append_work_operation_unlocked(&operation)?;
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
        let (previous, replacement) = if old_member_run_id == new_member_run_id {
            let revisions = self
                .read_jsonl::<MemberRun>("member_runs.jsonl")?
                .into_iter()
                .filter(|member| {
                    member.id == old_member_run_id && member.team_run_id == current.team_run_id
                })
                .collect::<Vec<_>>();
            let replacement = revisions.last().cloned().ok_or_else(|| {
                StoreError::Conflict(format!("member run not found: {new_member_run_id}"))
            })?;
            if compare_store_timestamps(&replacement.started_at, &current.updated_at)
                != std::cmp::Ordering::Greater
            {
                return Err(StoreError::Conflict(format!(
                    "WORK_ALREADY_BOUND: MemberRun {new_member_run_id} generation {} does not postdate Work version {}",
                    replacement.runtime_generation, current.version
                )));
            }
            let previous = revisions
                .iter()
                .rev()
                .skip(1)
                .find(|member| member.runtime_generation < replacement.runtime_generation)
                .cloned()
                .ok_or_else(|| {
                    StoreError::Conflict(format!(
                        "WORK_ALREADY_BOUND: MemberRun {new_member_run_id} has no higher replacement runtime generation"
                    ))
                })?;
            (previous, replacement)
        } else {
            (
                self.require_member_run_unlocked(&old_member_run_id, &current.team_run_id)?,
                self.require_member_run_unlocked(new_member_run_id, &current.team_run_id)?,
            )
        };
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
                "previous_runtime_generation": previous.runtime_generation,
                "replacement_runtime_generation": replacement.runtime_generation,
                "owner_member_id": owner_member_id,
            }),
        )
    }

    /// Append an explicit full-projection repair after a stale mixed-version
    /// writer omitted immutable additive provenance. Raw sparse operations
    /// remain untouched; the recovered reducer state becomes a new `Updated`
    /// WorkOperation at the next version without changing lifecycle, owner, or
    /// runtime binding.
    pub fn reconcile_work_projection_provenance(
        &self,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Updated,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let raw_current = latest_by_id(self.work_operations_unlocked()?, |operation| {
            operation.work.id.clone()
        })
        .remove(work_id)
        .ok_or_else(|| StoreError::Conflict(format!("work not found: {work_id}")))?;
        if raw_current.work.version != expected_version {
            return Err(StoreError::Conflict(format!(
                "VERSION_CONFLICT: work {work_id} is at version {}, expected {expected_version}",
                raw_current.work.version
            )));
        }
        let current = self.current_work_unlocked(work_id, expected_version)?;
        let mut recovered_fields = Vec::new();
        if raw_current.work.team_id.is_none() && current.team_id.is_some() {
            recovered_fields.push("team_id");
        }
        if raw_current.work.created_by_member_id.is_none() && current.created_by_member_id.is_some()
        {
            recovered_fields.push("created_by_member_id");
        }
        if recovered_fields.is_empty() {
            return Err(StoreError::Conflict(format!(
                "WORK_PROJECTION_PROVENANCE_CURRENT: Work {work_id} has no recoverable sparse provenance"
            )));
        }

        let mut next = current.clone();
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Updated,
            context,
            serde_json::json!({
                "reason": "mixed_version_projection_recovery",
                "recovered_fields": recovered_fields,
                "source_event_id": raw_current.event.id,
            }),
        )
    }

    /// Explicitly promote a compatibility TeamRun-scoped Work to the durable
    /// AgentTeam named by its current execution attempt. Source-linked Work
    /// refuses promotion while the Company WorkItem remains live, preventing
    /// two mutable owner/status authorities from surviving cutover.
    pub fn promote_work_to_team_scope(
        &self,
        company_store: &HarnessStore,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.promote_work_to_team_scope_inner(
            company_store,
            work_id,
            expected_version,
            context,
            || {},
            || Ok(()),
        )
    }

    fn promote_work_to_team_scope_inner<BeforeFence, AfterFence>(
        &self,
        company_store: &HarnessStore,
        work_id: &str,
        expected_version: u64,
        context: WorkCommandContext,
        before_fence: BeforeFence,
        after_fence: AfterFence,
    ) -> StoreResult<Work>
    where
        BeforeFence: FnOnce(),
        AfterFence: FnOnce() -> StoreResult<()>,
    {
        self.init()?;
        company_store.init()?;
        let (_first_lock, _second_lock) = self.acquire_joint_write_locks(company_store)?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::TeamScopePromoted,
        )? {
            if let Some(fence) = self.prepare_work_cutover_fence_unlocked(
                company_store,
                &existing.work,
                &existing.event,
            )? {
                company_store.append_jsonl_unlocked(WORK_CUTOVER_FENCES_LEDGER, &fence)?;
            }
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.team_id.is_some() {
            if current.source_work_item_ref.is_some() {
                let promotion = self
                    .work_operations_unlocked()?
                    .into_iter()
                    .rev()
                    .find(|operation| {
                        operation.work.id == current.id
                            && operation.event.kind == WorkEventKind::TeamScopePromoted
                    })
                    .ok_or_else(|| {
                        StoreError::Conflict(format!(
                            "CUTOVER_PROVENANCE_MISSING: Team-scoped Work {work_id} has no promotion event"
                        ))
                    })?;
                if let Some(fence) = self.prepare_work_cutover_fence_unlocked(
                    company_store,
                    &current,
                    &promotion.event,
                )? {
                    company_store.append_jsonl_unlocked(WORK_CUTOVER_FENCES_LEDGER, &fence)?;
                    return Ok(current);
                }
            }
            return Err(StoreError::Conflict(format!(
                "WORK_ALREADY_TEAM_SCOPED: Work {work_id} already belongs to AgentTeam {}",
                current.team_id.as_deref().unwrap_or_default()
            )));
        }
        let run = self.require_team_run_unlocked(&current.team_run_id)?;
        let team_id = durable_team_id(&run).ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_SCOPE_UNAVAILABLE: TeamRun {} has no durable AgentTeam identity",
                run.id
            ))
        })?;
        let mut next = current.clone();
        next.team_id = Some(team_id.to_string());
        next.version += 1;
        next.updated_at = context.created_at.clone();
        let intended_event = WorkEvent {
            id: context.event_id.clone(),
            team_run_id: next.team_run_id.clone(),
            work_id: next.id.clone(),
            sequence: 0,
            kind: WorkEventKind::TeamScopePromoted,
            expected_version: current.version,
            resulting_version: next.version,
            performed_by_actor: context.performed_by_actor.clone(),
            authority_actor: context.authority_actor.clone(),
            causation_ref: context.causation_ref.clone(),
            idempotency_key: context.idempotency_key.clone(),
            payload: serde_json::Value::Null,
            created_at: context.created_at.clone(),
        };
        // Refuse a deterministic execution-ledger collision before the
        // one-way Company fence is persisted. Once the fence exists, only
        // crash/I/O/recoverable projection failures may interrupt completion.
        self.ensure_work_event_id_available_unlocked(&intended_event.id)?;
        let fence =
            self.prepare_work_cutover_fence_unlocked(company_store, &next, &intended_event)?;
        before_fence();
        if let Some(fence) = fence {
            company_store.append_jsonl_unlocked(WORK_CUTOVER_FENCES_LEDGER, &fence)?;
        }
        // This failure boundary models a process crash after the durable
        // Company refusal marker but before the Execution Store operation.
        // Production passes a no-op; deterministic tests stop here and prove
        // that restart/retry is safe and idempotent.
        after_fence()?;
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::TeamScopePromoted,
            context,
            serde_json::json!({ "team_id": team_id }),
        )
    }

    fn prepare_work_cutover_fence_unlocked(
        &self,
        company_store: &HarnessStore,
        work: &Work,
        promotion_event: &WorkEvent,
    ) -> StoreResult<Option<WorkCutoverFence>> {
        let Some(source_id) = work.source_work_item_ref.as_deref() else {
            return Ok(None);
        };
        let team_id = work.team_id.as_deref().ok_or_else(|| {
            StoreError::Conflict(format!(
                "TEAM_SCOPE_UNAVAILABLE: Work {} has no durable AgentTeam identity",
                work.id
            ))
        })?;
        let source = company_store
            .latest_work_items()?
            .into_iter()
            .find(|item| item.id == source_id)
            .ok_or_else(|| {
                StoreError::Conflict(format!(
                    "COMPANY_WORK_ITEM_MISSING: source WorkItem {source_id} does not exist"
                ))
            })?;
        if !work_item_is_retired(source.status) {
            return Err(StoreError::Conflict(format!(
                "ACTIVE_COMPANY_WORK_ITEM_CONFLICT: WorkItem {source_id} is {:?}; archive, cancel, complete, or return it to draft before Team-scope promotion",
                source.status
            )));
        }
        if self.latest_works_unlocked()?.values().any(|other| {
            other.id != work.id
                && other.source_work_item_ref.as_deref() == Some(source_id)
                && other.team_id.is_some()
        }) {
            return Err(StoreError::Conflict(format!(
                "DUPLICATE_COMPANY_WORK_ITEM_LINK: WorkItem {source_id} already has a persistent Team Work"
            )));
        }

        let candidate = WorkCutoverFence {
            company_work_item_id: source_id.to_string(),
            work_id: work.id.clone(),
            team_id: team_id.to_string(),
            promotion_event_id: promotion_event.id.clone(),
            expected_work_version: promotion_event.expected_version,
            company_work_item_status: source.status,
            company_work_item_updated_at: source.updated_at.clone(),
            company_work_item_snapshot: serde_json::to_value(&source)?,
            idempotency_key: promotion_event.idempotency_key.clone(),
            created_at: promotion_event.created_at.clone(),
        };
        let existing = company_store
            .work_cutover_fences_unlocked()?
            .into_iter()
            .filter(|fence| fence.company_work_item_id == source_id)
            .collect::<Vec<_>>();
        match existing.as_slice() {
            [] => Ok(Some(candidate)),
            [fence]
                if fence.work_id == candidate.work_id
                    && fence.team_id == candidate.team_id
                    && fence.company_work_item_status == candidate.company_work_item_status
                    && fence.company_work_item_updated_at
                        == candidate.company_work_item_updated_at
                    && fence.company_work_item_snapshot
                        == candidate.company_work_item_snapshot
                    && fence.expected_work_version <= candidate.expected_work_version =>
            {
                Ok(None)
            }
            _ => Err(StoreError::Conflict(format!(
                "COMPANY_WORK_ITEM_CUTOVER_FENCE_CONFLICT: WorkItem {source_id} is already fenced for another promotion"
            ))),
        }
    }

    /// Move a persistent Work onto a successor execution attempt of the same
    /// AgentTeam. Stable ownership, creator provenance, source relations, and
    /// Work identity remain unchanged; only the execution binding moves.
    pub fn retarget_work_execution(
        &self,
        work_id: &str,
        expected_version: u64,
        successor_team_run_id: &str,
        successor_member_run_id: Option<&str>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::ExecutionRetargeted,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.is_terminal() {
            return Err(StoreError::Conflict(format!(
                "work {work_id} is terminal and cannot be retargeted"
            )));
        }
        self.reconcile_work_host_attentions_unlocked()?;
        if self
            .latest_host_attentions_unlocked()?
            .values()
            .any(|attention| {
                attention.work_id == current.id
                    && attention.team_run_id == current.team_run_id
                    && attention.needs_host_action()
            })
        {
            return Err(StoreError::Conflict(format!(
                "HOST_ATTENTION_PENDING: Work {work_id} has unresolved attention owned by TeamRun {}; the exact Host must ACK intake before execution retarget",
                current.team_run_id
            )));
        }
        let team_id = current.team_id.clone().ok_or_else(|| {
            StoreError::Conflict(format!(
                "WORK_NOT_TEAM_SCOPED: promote Work {work_id} before retargeting execution"
            ))
        })?;
        if current.team_run_id == successor_team_run_id {
            return Err(StoreError::Conflict(format!(
                "Work {work_id} already targets TeamRun {successor_team_run_id}"
            )));
        }
        let successor = self.require_team_run_unlocked(successor_team_run_id)?;
        if matches!(
            successor.status,
            TeamRunStatus::Completed | TeamRunStatus::Failed | TeamRunStatus::Cancelled
        ) {
            return Err(StoreError::Conflict(format!(
                "successor TeamRun {} is {:?} and cannot execute Work",
                successor.id, successor.status
            )));
        }
        if durable_team_id(&successor) != Some(team_id.as_str()) {
            return Err(StoreError::Conflict(format!(
                "TEAM_SCOPE_MISMATCH: successor TeamRun {} does not belong to AgentTeam {team_id}",
                successor.id
            )));
        }
        if let Some(previous_member_run_id) = current.active_member_run_id.as_deref() {
            let previous =
                self.require_member_run_unlocked(previous_member_run_id, &current.team_run_id)?;
            if previous.coordination_is_active()
                && !matches!(
                    previous.status,
                    harness_core::MemberRunStatus::Completed
                        | harness_core::MemberRunStatus::Failed
                        | harness_core::MemberRunStatus::Stopped
                )
            {
                return Err(StoreError::Conflict(format!(
                    "OLD_RUNTIME_ACTIVE: MemberRun {previous_member_run_id} must be closed or terminal before execution retarget"
                )));
            }
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

        let new_binding = match (current.owner_member_id.as_deref(), successor_member_run_id) {
            (None, None) => None,
            (None, Some(_)) => {
                return Err(StoreError::Conflict(
                    "unassigned Work cannot gain an execution binding during retarget".to_string(),
                ));
            }
            (Some(_), None) => {
                return Err(StoreError::Conflict(
                    "owned Work requires --successor-member-run-id during retarget".to_string(),
                ));
            }
            (Some(owner_id), Some(member_run_id)) => {
                let member =
                    self.require_member_run_unlocked(member_run_id, successor_team_run_id)?;
                self.ensure_member_can_receive_work_unlocked(&member)?;
                let successor_identity = stable_member_identity(&member);
                if successor_identity != owner_id {
                    return Err(StoreError::Conflict(format!(
                        "OWNER_MISMATCH: successor MemberRun {member_run_id} belongs to {successor_identity}, expected {owner_id}"
                    )));
                }
                Some(member.id)
            }
        };

        let previous_team_run_id = current.team_run_id.clone();
        let previous_member_run_id = current.active_member_run_id.clone();
        let mut next = current.clone();
        next.team_run_id = successor_team_run_id.to_string();
        next.active_member_run_id = new_binding.clone();
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::ExecutionRetargeted,
            context,
            serde_json::json!({
                "team_id": team_id,
                "previous_team_run_id": previous_team_run_id,
                "successor_team_run_id": successor_team_run_id,
                "previous_member_run_id": previous_member_run_id,
                "successor_member_run_id": new_binding,
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
        self.submit_work_with_links(
            work_id,
            expected_version,
            member_run_id,
            result_summary,
            artifact_refs,
            check_refs,
            Vec::new(),
            context,
        )
    }

    /// [`submit_work`] plus an explicit GitHub issue/PR linkage snapshot
    /// (issue #369). The base method keeps its historical signature; links are
    /// merged into any links already attached at create time.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_work_with_links(
        &self,
        work_id: &str,
        expected_version: u64,
        member_run_id: &str,
        result_summary: &str,
        artifact_refs: Vec<String>,
        check_refs: Vec<String>,
        github_links: Vec<GitHubLink>,
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
                // Merge rather than replace: a Work created with
                // `--github-issue` keeps that link when a `--github-pr` is
                // attached at submit time.
                for link in github_links {
                    if !work.github_links.contains(&link) {
                        work.github_links.push(link);
                    }
                }
                work.blocker_reason = None;
            },
        )
    }

    /// Refresh the GitHub linkage snapshot on a Work without touching its
    /// lifecycle (issue #369 Phase 2, daemon CI poll). Host/Service actor
    /// only. When the links are unchanged the current Work is returned without
    /// appending a `Updated` operation, so a steady-state poll never churns
    /// versions.
    pub fn update_work_github_links(
        &self,
        work_id: &str,
        expected_version: u64,
        github_links: Vec<GitHubLink>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.ensure_work_store_compatible_unlocked()?;
        if let Some(existing) = self.idempotent_work_operation_unlocked(
            &context.idempotency_key,
            work_id,
            WorkEventKind::Updated,
        )? {
            return Ok(existing.work);
        }
        require_host_actor(&context.performed_by_actor)?;
        let current = self.current_work_unlocked(work_id, expected_version)?;
        if current.github_links == github_links {
            return Ok(current);
        }
        let mut next = current.clone();
        next.github_links = github_links;
        next.version += 1;
        next.updated_at = context.created_at.clone();
        self.append_work_transition_with_payload_unlocked(
            current,
            next,
            WorkEventKind::Updated,
            context,
            serde_json::json!({ "reason": "github_ci_poll" }),
        )
    }

    /// Host-side auto-submit when the daemon observes a linked pull request
    /// reach `MERGED` (issue #369 Phase 2). The Work must be `in_progress` and
    /// carry a `pull_request` link with `status == "MERGED"`; the fresh link
    /// snapshot is stored with the transition. Host acceptance still moves the
    /// Work from `review` to `done`; this only automates the submission step.
    pub fn submit_work_on_pr_merge(
        &self,
        work_id: &str,
        expected_version: u64,
        result_summary: &str,
        github_links: Vec<GitHubLink>,
        context: WorkCommandContext,
    ) -> StoreResult<Work> {
        if result_summary.trim().is_empty() {
            return Err(StoreError::Conflict("RESULT_REQUIRED".to_string()));
        }
        if !github_links.iter().any(|link| {
            link.kind == harness_core::GitHubLinkKind::PullRequest
                && link.status.as_deref() == Some("MERGED")
        }) {
            return Err(StoreError::Conflict(
                "PR_MERGE_REQUIRED: auto-submit requires a pull_request link with status MERGED"
                    .to_string(),
            ));
        }
        self.transition_work_as_host(
            work_id,
            expected_version,
            context,
            WorkEventKind::Submitted,
            WorkStatus::InProgress,
            WorkStatus::Review,
            serde_json::json!({ "reason": "github_pr_merge_observed" }),
            |work| {
                work.result_summary = Some(result_summary.to_string());
                // The fresh observed snapshot replaces the stored one; any
                // issue links attached at create time are carried forward.
                let mut merged = Vec::new();
                for link in github_links {
                    if !merged.contains(&link) {
                        merged.push(link);
                    }
                }
                for link in &work.github_links {
                    if !merged.contains(link) {
                        merged.push(link.clone());
                    }
                }
                work.github_links = merged;
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
        let prereq_event_id = context.event_id.clone();
        let prereq_created_at = context.created_at.clone();
        let deliveries = if matches!(
            kind,
            WorkEventKind::Assigned
                | WorkEventKind::ChangesRequested
                | WorkEventKind::Resumed
                | WorkEventKind::Rebound
                | WorkEventKind::TeamScopePromoted
                | WorkEventKind::ExecutionRetargeted
                | WorkEventKind::Accepted
                | WorkEventKind::Cancelled
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
        self.append_work_operation_unlocked(&operation)?;
        // When a work is accepted (Done), notify works that depend on it
        // as a prerequisite: create deliveries for their owner members.
        if kind == WorkEventKind::Accepted {
            let team_run_id = &next.team_run_id;
            let prerequisite_id = &next.id;
            let all_works = self.latest_works_unlocked()?;
            for dependent_work in all_works.values() {
                if dependent_work.team_run_id == *team_run_id
                    && dependent_work
                        .prerequisite_work_ids
                        .iter()
                        .any(|pid| pid == prerequisite_id)
                    && !dependent_work.is_terminal()
                {
                    if let Some(owner_member_id) = dependent_work.active_member_run_id.as_deref() {
                        if let Ok(member) =
                            self.require_member_run_unlocked(owner_member_id, team_run_id)
                        {
                            if self
                                .ensure_member_can_receive_work_unlocked(&member)
                                .is_ok()
                            {
                                let dep_delivery = WorkDelivery {
                                    id: format!(
                                        "work-delivery-prereq-{}-{}",
                                        prereq_event_id, dependent_work.id
                                    ),
                                    work_event_id: prereq_event_id.clone(),
                                    team_run_id: team_run_id.clone(),
                                    work_id: dependent_work.id.clone(),
                                    work_version: dependent_work.version,
                                    recipient_member_run_id: owner_member_id.to_string(),
                                    status: WorkDeliveryStatus::Queued,
                                    attempt: 0,
                                    claim_id: None,
                                    claimed_by_supervisor_id: None,
                                    claimed_generation: None,
                                    provider_receipt_id: None,
                                    failure_reason: None,
                                    updated_at: prereq_created_at.clone(),
                                };
                                self.append_jsonl_unlocked("work_deliveries.jsonl", &dep_delivery)?;
                                // Also ensure HostAttention for prerequisite completion
                                let prereq_attention = HostAttention {
                                    id: format!("host-attention-prereq-{}", dep_delivery.id),
                                    team_run_id: team_run_id.clone(),
                                    kind: HostAttentionKind::WorkPrerequisiteCompleted,
                                    work_id: dependent_work.id.clone(),
                                    work_version: dependent_work.version,
                                    source_event_ref: prereq_event_id.clone(),
                                    member_run_id: Some(owner_member_id.to_string()),
                                    status: HostAttentionStatus::Actionable,
                                    attempt: 0,
                                    claim_id: None,
                                    claimed_host_surface: None,
                                    claimed_host_thread_id: None,
                                    provider_receipt_id: None,
                                    last_failure_reason: None,
                                    created_at: prereq_created_at.clone(),
                                    updated_at: prereq_created_at.clone(),
                                };
                                prereq_attention
                                    .validate()
                                    .map_err(|error| StoreError::Conflict(error.to_string()))?;
                                self.append_jsonl_unlocked(
                                    "host_attentions.jsonl",
                                    &prereq_attention,
                                )?;
                            }
                        }
                    }
                }
            }
        }
        self.ensure_host_attention_for_work_operation_unlocked(&operation)?;
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

    fn ensure_host_attention_unlocked(
        &self,
        attention: &HostAttention,
    ) -> StoreResult<HostAttention> {
        attention
            .validate()
            .map_err(|error| StoreError::Conflict(error.to_string()))?;
        if attention.status != HostAttentionStatus::Actionable
            || attention.attempt != 0
            || attention.claim_id.is_some()
            || attention.claimed_host_surface.is_some()
            || attention.claimed_host_thread_id.is_some()
            || attention.provider_receipt_id.is_some()
        {
            return Err(StoreError::Conflict(
                "new HostAttention must be actionable and unclaimed".to_string(),
            ));
        }

        let mut attentions = self.latest_host_attentions_unlocked()?;
        if let Some(existing) = attentions.remove(&attention.id) {
            if Self::same_host_attention_fact(&existing, attention) {
                return Ok(existing);
            }
            return Err(StoreError::Conflict(format!(
                "HostAttention id {} already names a different causal fact",
                attention.id
            )));
        }

        self.require_team_run_unlocked(&attention.team_run_id)?;
        let source_operation = self
            .work_operations_unlocked()?
            .into_iter()
            .find(|operation| operation.event.id == attention.source_event_ref);
        if let Some(operation) = source_operation {
            if operation.event.team_run_id != attention.team_run_id
                || operation.event.work_id != attention.work_id
                || operation.event.resulting_version != attention.work_version
            {
                return Err(StoreError::Conflict(format!(
                    "HostAttention {} does not match source WorkEvent {}",
                    attention.id, attention.source_event_ref
                )));
            }
        } else {
            // Member-runtime attention can be caused by a TeamRun/provider
            // event rather than a WorkEvent. Validate that its current Work
            // subject still resolves inside the named TeamRun.
            let work = self
                .latest_works_unlocked()?
                .remove(&attention.work_id)
                .ok_or_else(|| {
                    StoreError::Conflict(format!("work not found: {}", attention.work_id))
                })?;
            if work.team_run_id != attention.team_run_id {
                return Err(StoreError::Conflict(format!(
                    "Work {} does not belong to TeamRun {}",
                    attention.work_id, attention.team_run_id
                )));
            }
            if work.version < attention.work_version {
                return Err(StoreError::Conflict(format!(
                    "HostAttention references future Work version {} > {}",
                    attention.work_version, work.version
                )));
            }
        }
        if let Some(member_run_id) = attention.member_run_id.as_deref() {
            self.require_member_run_unlocked(member_run_id, &attention.team_run_id)?;
        }

        self.append_jsonl_unlocked("host_attentions.jsonl", attention)?;
        Ok(attention.clone())
    }

    fn same_host_attention_fact(left: &HostAttention, right: &HostAttention) -> bool {
        left.team_run_id == right.team_run_id
            && left.kind == right.kind
            && left.work_id == right.work_id
            && left.work_version == right.work_version
            && left.source_event_ref == right.source_event_ref
            && left.member_run_id == right.member_run_id
            && left.created_at == right.created_at
    }

    fn host_attention_for_work_operation(operation: &WorkOperation) -> Option<HostAttention> {
        let kind = match operation.event.kind {
            WorkEventKind::Submitted => HostAttentionKind::WorkReviewRequested,
            WorkEventKind::Blocked => HostAttentionKind::WorkBlocked,
            WorkEventKind::Accepted => HostAttentionKind::WorkAccepted,
            WorkEventKind::ChangesRequested => HostAttentionKind::WorkChangesRequested,
            WorkEventKind::Cancelled => HostAttentionKind::WorkCancelled,
            _ => return None,
        };
        Some(HostAttention {
            id: format!("host-attention-{}", operation.event.id),
            team_run_id: operation.event.team_run_id.clone(),
            kind,
            work_id: operation.event.work_id.clone(),
            work_version: operation.event.resulting_version,
            source_event_ref: operation.event.id.clone(),
            member_run_id: operation.work.active_member_run_id.clone(),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: operation.event.created_at.clone(),
            updated_at: operation.event.created_at.clone(),
        })
    }

    fn ensure_host_attention_for_work_operation_unlocked(
        &self,
        operation: &WorkOperation,
    ) -> StoreResult<Option<HostAttention>> {
        Self::host_attention_for_work_operation(operation)
            .map(|attention| self.ensure_host_attention_unlocked(&attention))
            .transpose()
    }

    fn reconcile_work_host_attentions_unlocked(&self) -> StoreResult<Vec<HostAttention>> {
        let operations = self.work_operations_unlocked()?;
        let mut projected = self.latest_host_attentions_unlocked()?;
        let mut reconciled = Vec::new();
        for operation in &operations {
            let Some(attention) = Self::host_attention_for_work_operation(operation) else {
                continue;
            };
            if let Some(existing) = projected.get(&attention.id) {
                if !Self::same_host_attention_fact(existing, &attention) {
                    return Err(StoreError::Conflict(format!(
                        "HostAttention id {} already names a different causal fact",
                        attention.id
                    )));
                }
                reconciled.push(existing.clone());
                continue;
            }
            attention
                .validate()
                .map_err(|error| StoreError::Conflict(error.to_string()))?;
            self.require_team_run_unlocked(&attention.team_run_id)?;
            if let Some(member_run_id) = attention.member_run_id.as_deref() {
                self.require_member_run_unlocked(member_run_id, &attention.team_run_id)?;
            }
            self.append_jsonl_unlocked("host_attentions.jsonl", &attention)?;
            projected.insert(attention.id.clone(), attention.clone());
            reconciled.push(attention);
        }
        Ok(reconciled)
    }

    fn host_attention_inbox_for_team_run_unreconciled(
        &self,
        team_run_id: &str,
        include_all: bool,
    ) -> StoreResult<HostAttentionInbox> {
        let run = self.require_team_run_unlocked(team_run_id)?;
        let attentions = self
            .latest_host_attentions_unlocked()?
            .into_values()
            .filter(|attention| attention.team_run_id == team_run_id)
            .filter(|attention| include_all || attention.needs_host_action())
            .collect::<Vec<_>>();
        let warning = if run.host_thread_id.is_none() && !attentions.is_empty() {
            Some(format!(
                "UNBOUND_HOST: TeamRun {} has actionable Host attention but no exact native Host task; bind host_surface + host_thread_id before delivery",
                run.id
            ))
        } else {
            None
        };
        Ok(HostAttentionInbox {
            team_run_id: run.id,
            host_surface: run.host_surface,
            host_thread_id: run.host_thread_id,
            warning,
            attentions,
        })
    }

    fn latest_host_attentions_unlocked(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, HostAttention>> {
        Ok(latest_by_id(
            self.read_jsonl::<HostAttention>("host_attentions.jsonl")?,
            |attention| attention.id.clone(),
        ))
    }

    fn require_host_attention_unlocked(&self, attention_id: &str) -> StoreResult<HostAttention> {
        self.latest_host_attentions_unlocked()?
            .remove(attention_id)
            .ok_or_else(|| StoreError::Conflict(format!("HostAttention not found: {attention_id}")))
    }

    fn require_exact_host_binding_unlocked(
        &self,
        team_run_id: &str,
        host_surface: &str,
        host_thread_id: &str,
    ) -> StoreResult<AgentTeamRun> {
        require_non_empty_store(host_surface, "Host surface")?;
        require_non_empty_store(host_thread_id, "Host thread id")?;
        let run = self.require_team_run_unlocked(team_run_id)?;
        if canonical_surface(&run.host_surface) != canonical_surface(host_surface)
            || run.host_thread_id.as_deref() != Some(host_thread_id)
        {
            return Err(StoreError::Conflict(format!(
                "HOST_BINDING_MISMATCH: TeamRun {team_run_id} is not bound to {host_surface}/{host_thread_id}"
            )));
        }
        Ok(run)
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
            if !works_share_scope(prerequisite, work) || prerequisite.id == work.id {
                return Err(StoreError::Conflict(
                    "prerequisites must be distinct Works in the same durable Team scope"
                        .to_string(),
                ));
            }
        }
        if let Some(parent_id) = work.parent_work_id.as_deref() {
            let parent = works.get(parent_id).ok_or_else(|| {
                StoreError::Conflict(format!("parent work not found: {parent_id}"))
            })?;
            if !works_share_scope(parent, work) || parent.id == work.id {
                return Err(StoreError::Conflict(
                    "parent_work_id must reference a distinct Work in the same durable Team scope"
                        .to_string(),
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
        if self
            .ensure_member_can_receive_work_unlocked(&member)
            .is_err()
        {
            return Ok(Vec::new());
        }
        // Skip loopback deliveries for terminal work: the owning member
        // already knows their work is Done/Cancelled — self-notification is
        // redundant. Non-terminal events (Created, Assigned, ChangesRequested,
        // Resumed, Rebound) genuinely need delivery even to the owner.
        if work.is_terminal() {
            if let Some(ref owner_id) = work.owner_member_id {
                if owner_id == &stable_member_identity(&member) {
                    return Ok(Vec::new());
                }
            }
        }
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
            .work_operations_with_recovered_provenance_unlocked()?
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
        // If the original process crashed after fsyncing the WorkOperation but
        // before its derived HostAttention row, the ordinary idempotent retry
        // repairs that gap before returning the already-applied Work result.
        self.ensure_host_attention_for_work_operation_unlocked(&existing)?;
        Ok(Some(existing))
    }

    fn work_operations_unlocked(&self) -> StoreResult<Vec<WorkOperation>> {
        self.read_jsonl("work_operations.jsonl")
    }

    /// Fold immutable additive provenance through every WorkOperation.
    ///
    /// Mixed-version writers may deserialize a newer complete projection,
    /// discard unknown fields, and append a later row without `team_id` or
    /// `created_by_member_id`. Once either fact has been established, no Work
    /// command is allowed to remove or change it. Reads therefore recover a
    /// missing later value from ordered WorkOperation ledger history, while a
    /// conflicting non-null value remains corruption and is refused.
    fn work_operations_with_recovered_provenance_unlocked(
        &self,
    ) -> StoreResult<Vec<WorkOperation>> {
        let mut team_ids = std::collections::BTreeMap::<String, String>::new();
        let mut creator_ids = std::collections::BTreeMap::<String, String>::new();
        let mut recovered = Vec::new();
        for mut operation in self.work_operations_unlocked()? {
            let work_id = operation.work.id.clone();
            match (team_ids.get(&work_id), operation.work.team_id.as_deref()) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return Err(StoreError::Conflict(format!(
                        "WORK_PROJECTION_PROVENANCE_CONFLICT: Work {work_id} changed team_id from {expected} to {actual} in event {}",
                        operation.event.id
                    )));
                }
                (Some(expected), None) => operation.work.team_id = Some(expected.clone()),
                (None, Some(actual)) => {
                    team_ids.insert(work_id.clone(), actual.to_string());
                }
                _ => {}
            }
            match (
                creator_ids.get(&work_id),
                operation.work.created_by_member_id.as_deref(),
            ) {
                (Some(expected), Some(actual)) if expected != actual => {
                    return Err(StoreError::Conflict(format!(
                        "WORK_PROJECTION_PROVENANCE_CONFLICT: Work {work_id} changed created_by_member_id from {expected} to {actual} in event {}",
                        operation.event.id
                    )));
                }
                (Some(expected), None) => {
                    operation.work.created_by_member_id = Some(expected.clone())
                }
                (None, Some(actual)) => {
                    creator_ids.insert(work_id, actual.to_string());
                }
                _ => {}
            }
            recovered.push(operation);
        }
        Ok(recovered)
    }

    /// Current-version writers must emit a complete projection. This guard is
    /// the refusal half of mixed-schema compatibility; the recovery fold above
    /// is the lossless-preservation half for sparse rows already appended by a
    /// stale binary.
    fn append_work_operation_unlocked(&self, operation: &WorkOperation) -> StoreResult<()> {
        if let Some(current) = self
            .latest_works_unlocked()?
            .remove(operation.work.id.as_str())
        {
            if current.team_id.is_some() && operation.work.team_id != current.team_id {
                return Err(StoreError::Conflict(format!(
                    "WORK_PROJECTION_PROVENANCE_REGRESSION: Work {} event {} would drop or change team_id",
                    operation.work.id, operation.event.id
                )));
            }
            if current.created_by_member_id.is_some()
                && operation.work.created_by_member_id != current.created_by_member_id
            {
                return Err(StoreError::Conflict(format!(
                    "WORK_PROJECTION_PROVENANCE_REGRESSION: Work {} event {} would drop or change created_by_member_id",
                    operation.work.id, operation.event.id
                )));
            }
        }
        self.append_jsonl_unlocked("work_operations.jsonl", operation)
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
        Ok(latest_by_id(
            self.work_operations_with_recovered_provenance_unlocked()?,
            |operation| operation.work.id.clone(),
        )
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

    /// Fail a TeamMessage delivery that can never be completed because the
    /// target member has stopped / failed / been retired.
    ///
    /// Transitions from `Queued` (pre-bind failure) or `Claimed` (transport
    /// disconnect) to `Failed`. A delivery already at `Failed` with the same
    /// reason is idempotent.
    #[allow(clippy::too_many_arguments)]
    pub fn fail_team_message_delivery(
        &self,
        team_run_id: &str,
        message_id: &str,
        member_run_id: &str,
        supervisor_id: &str,
        supervisor_generation: u64,
        reason: &str,
        now_unix_ms: u64,
        updated_at: &str,
    ) -> StoreResult<TeamMessage> {
        if reason.trim().is_empty() {
            return Err(StoreError::Conflict(
                "TeamMessage delivery failure reason is required".to_string(),
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

        // Idempotent: already failed with same reason.
        if delivery.status == TeamDeliveryStatus::Failed {
            if delivery
                .failure_reason
                .as_deref()
                .is_some_and(|existing| existing == reason)
            {
                return Ok(message);
            }
            return Err(StoreError::Conflict(format!(
                "message {message_id} delivery for {member_run_id} was already failed with a different reason"
            )));
        }

        // Allowed transitions: Queued→Failed (pre-bind), Claimed→Failed
        // (post-bind / transport disconnect).
        match delivery.status {
            TeamDeliveryStatus::Queued => {}
            TeamDeliveryStatus::Claimed => {
                // Only the owning Supervisor generation may fail its own claim.
                if delivery.claimed_by_supervisor_id.as_deref() != Some(supervisor_id)
                    || delivery.claimed_generation != Some(supervisor_generation)
                {
                    return Err(StoreError::Conflict(format!(
                        "message {message_id} delivery for {member_run_id} was claimed by a different Supervisor generation"
                    )));
                }
            }
            _ => {
                return Err(StoreError::Conflict(format!(
                    "message {message_id} delivery for {member_run_id} is already {:?}",
                    delivery.status
                )));
            }
        }

        delivery.status = TeamDeliveryStatus::Failed;
        delivery.claim_id = None;
        delivery.claimed_by_supervisor_id = None;
        delivery.claimed_generation = None;
        delivery.claimed_unix_ms = None;
        delivery.claim_expires_unix_ms = None;
        delivery.provider_receipt_id = None;
        delivery.failure_reason = Some(reason.to_string());
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

    /// Raw append-only Mission Log rows across every Mission, in append
    /// order. Prefer [`Self::mission_log_entries`] when scoping to one
    /// Mission; this is here for parity with `waves()`/`missions()`.
    pub fn mission_log(&self) -> StoreResult<Vec<MissionLogEntry>> {
        self.read_jsonl("mission_log.jsonl")
    }

    /// Every [`MissionLogEntry`] for one Mission, ordered by `revision`
    /// ascending. There is no latest-wins collapse: unlike Wave/Mission the
    /// Log has no mutable identity, every row is a permanent entry.
    pub fn mission_log_entries(&self, mission_id: &str) -> StoreResult<Vec<MissionLogEntry>> {
        let mut entries = self
            .mission_log()?
            .into_iter()
            .filter(|entry| entry.mission_id == mission_id)
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.revision);
        Ok(entries)
    }

    /// The last `n` [`MissionLogEntry`] rows for one Mission, oldest-first
    /// within the returned slice (Unix `tail` ordering) so a reader sees them
    /// in the order they were written. Returns fewer than `n` rows if the
    /// Mission has fewer entries, and an empty Vec if it has none yet.
    pub fn mission_log_tail(
        &self,
        mission_id: &str,
        n: usize,
    ) -> StoreResult<Vec<MissionLogEntry>> {
        let entries = self.mission_log_entries(mission_id)?;
        let start = entries.len().saturating_sub(n);
        Ok(entries[start..].to_vec())
    }

    pub fn members(&self) -> StoreResult<Vec<AgentMember>> {
        self.read_jsonl("members.jsonl")
    }

    pub fn durable_members(&self) -> StoreResult<Vec<DurableAgentMember>> {
        self.read_jsonl("durable_agent_members.jsonl")
    }

    /// Latest-row-wins durable AgentMember projection, ordered by id.
    pub fn latest_durable_members(
        &self,
    ) -> StoreResult<std::collections::BTreeMap<String, DurableAgentMember>> {
        Ok(latest_by_id(self.durable_members()?, |member| {
            member.id.clone()
        }))
    }

    pub fn teams(&self) -> StoreResult<Vec<AgentTeam>> {
        self.read_jsonl("teams.jsonl")
    }

    /// Latest-row-wins AgentTeam projection keyed by team id. This is the
    /// input for recursive topology validation and queries (ADR 0052).
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

    /// Read-only ADR 0052 cutover gate across the independently selected
    /// Execution Space and Company Store. It does not migrate or dual-write
    /// either side; callers decide whether a reported snapshot may advance.
    pub fn work_cutover_report(
        &self,
        company_store: &HarnessStore,
    ) -> StoreResult<WorkCutoverReport> {
        self.init()?;
        company_store.init()?;
        let (_first_lock, _second_lock) = self.acquire_joint_write_locks(company_store)?;
        let works = self
            .latest_works_unlocked()?
            .into_values()
            .collect::<Vec<_>>();
        let team_runs = latest_by_id(self.read_jsonl("team_runs.jsonl")?, |run: &AgentTeamRun| {
            run.id.clone()
        })
        .into_values()
        .collect::<Vec<_>>();
        let company_work_items = company_store.latest_work_items()?;
        let fences = company_store.work_cutover_fences_unlocked()?;
        let work_events = self
            .work_operations_unlocked()?
            .into_iter()
            .map(|operation| operation.event)
            .collect::<Vec<_>>();
        Ok(validate_work_cutover_with_fences(
            &works,
            &team_runs,
            &company_work_items,
            &fences,
            &work_events,
        ))
    }

    pub fn work_cutover_fences(&self) -> StoreResult<Vec<WorkCutoverFence>> {
        self.work_cutover_fences_unlocked()
    }

    fn work_cutover_fences_unlocked(&self) -> StoreResult<Vec<WorkCutoverFence>> {
        self.read_jsonl(WORK_CUTOVER_FENCES_LEDGER)
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

    /// Claim a queued WorkDelivery for a terminal work notification.
    ///
    /// Like [`claim_work_delivery`] but permits terminal (Accepted /
    /// Cancelled) works, skips the prerequisite-satisfied check, and does not
    /// fence on another active work occupying the member slot. A terminal-work
    /// notification is informational (the supervisor turns it into a
    /// TeamMessage), not an execution assignment.
    #[allow(clippy::too_many_arguments)]
    pub fn claim_work_notification(
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
        // Terminal works are allowed; the supervisor will turn this delivery
        // into a TeamMessage, not a work-assignment prompt.
        if work.team_run_id != team_run_id
            || work.version != delivery.work_version
            || work.active_member_run_id.as_deref() != Some(member_run_id)
            || !work.is_terminal()
        {
            return Ok(WorkDeliveryClaimResult::NotQueued);
        }
        // No slot-occupancy fence: a terminal-work notification never blocks
        // an active execution assignment.
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
        self.ensure_host_attention_unlocked(&HostAttention {
            id: format!("host-attention-wd-{}-failed", delivery.id),
            team_run_id: delivery.team_run_id.clone(),
            kind: HostAttentionKind::WorkDeliveryFailed,
            work_id: delivery.work_id.clone(),
            work_version: delivery.work_version,
            source_event_ref: format!("wd-update:{}", update_sequence),
            member_run_id: Some(delivery.recipient_member_run_id.clone()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: delivery.updated_at.clone(),
            updated_at: delivery.updated_at.clone(),
        })?;
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
        if file_name == COMPANY_WORK_ITEMS_LEDGER {
            let work_item = serde_json::from_slice::<WorkItem>(&row)?;
            if self
                .work_cutover_fences_unlocked()?
                .iter()
                .any(|fence| fence.company_work_item_id == work_item.id)
            {
                return Err(StoreError::Conflict(format!(
                    "COMPANY_WORK_ITEM_CUTOVER_FENCED: WorkItem {} is immutable after Team Work authority promotion; Approval and Finance ledgers remain independent",
                    work_item.id
                )));
            }
        }
        row.push(b'\n');

        let path = self.root.join(file_name);
        let creates_ledger = !path.exists();
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
        if creates_ledger {
            // The first fence/operation append creates a directory entry.
            // Syncing only the inode is insufficient across a system crash;
            // persist that new name before reporting the append durable.
            File::open(&self.root)?.sync_all()?;
        }
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

    /// Acquire two store locks in canonical path order. This closes the
    /// independently-read Company/Execution TOCTOU without claiming that two
    /// JSONL appends are one physical transaction. Equal canonical roots share
    /// one lock, which also keeps single-root test and compatibility stores
    /// deadlock-free.
    fn acquire_joint_write_locks(
        &self,
        other: &HarnessStore,
    ) -> StoreResult<(StoreWriteLock, Option<StoreWriteLock>)> {
        let self_root = fs::canonicalize(&self.root)?;
        let other_root = fs::canonicalize(&other.root)?;
        if self_root == other_root {
            return Ok((self.acquire_write_lock()?, None));
        }
        if self_root < other_root {
            let first = self.acquire_write_lock()?;
            let second = other.acquire_write_lock()?;
            Ok((first, Some(second)))
        } else {
            let first = other.acquire_write_lock()?;
            let second = self.acquire_write_lock()?;
            Ok((first, Some(second)))
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

/// Normalize a Work title for duplicate detection: trim, lowercase, collapse
/// whitespace. Two titles that differ only in casing or spacing are treated as
/// the same logical Work within a team run.
fn normalize_work_title(title: &str) -> String {
    let trimmed = title.trim().to_lowercase();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    words.join(" ")
}

fn stable_member_identity(member: &MemberRun) -> String {
    member
        .agent_member_id
        .clone()
        .or_else(|| member.slot_id.clone())
        .unwrap_or_else(|| member.id.clone())
}

fn durable_team_id(run: &AgentTeamRun) -> Option<&str> {
    run.agent_team_id
        .as_deref()
        .or(run.definition_id.as_deref())
}

fn works_share_scope(left: &Work, right: &Work) -> bool {
    match (left.team_id.as_deref(), right.team_id.as_deref()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => left.team_run_id == right.team_run_id,
        _ => false,
    }
}

fn work_item_is_retired(status: WorkItemStatus) -> bool {
    matches!(
        status,
        WorkItemStatus::Draft
            | WorkItemStatus::Completed
            | WorkItemStatus::Cancelled
            | WorkItemStatus::Archived
    )
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

fn require_non_empty_store(value: &str, label: &str) -> StoreResult<()> {
    if value.trim().is_empty() {
        Err(StoreError::Conflict(format!("{label} must not be empty")))
    } else {
        Ok(())
    }
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
    use std::sync::{mpsc, Arc, Barrier};
    use std::time::{SystemTime, UNIX_EPOCH};

    use harness_core::{
        DelegationMode, DelegationStatus, HostAttentionKind, MemberActionStatus, MemberRunStatus,
        MemberWorkspaceSnapshot, MessageKind, Mission, MissionLogEntry, MissionLogEntryKind,
        MissionStatus, SenderKind, TeamActorKind, TeamActorRef, TeamDeliveryPolicy,
        TeamDeliveryStatus, TeamMessageDelivery, TeamMessageKind, TeamMessageResponseIntent,
        TeamRunEventSourceKind, TeamRunStatus, Wave, WaveExecutorKind, WaveGateStatus, WaveStatus,
        WorkPriority,
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

    /// ADR 0051 changed `compare_and_close_mission` to skip the Wave-gate
    /// check entirely for a Mission whose `wave_ids` is empty (the only
    /// shape a NEW Mission can have now, since Wave create is retired). This
    /// proves the OTHER branch is untouched: a Mission that already
    /// accumulated `wave_ids` before the cutover still requires every one
    /// of them to be an accepted, completed Wave -- its in-flight contract
    /// does not silently change underneath it.
    #[test]
    fn mission_close_with_legacy_wave_ids_still_requires_accepted_gate() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-legacy-mission-close-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        let mission = Mission {
            id: "mission-legacy".into(),
            title: "Pre-cutover Mission".into(),
            objective: "Already has Wave membership from before ADR 0051".into(),
            context: String::new(),
            desired_outcome: None,
            status: MissionStatus::Running,
            wave_ids: vec!["wave-legacy".into()],
            agent_team_ids: Vec::new(),
            outcome_summary: None,
            completed_by: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_mission(&mission).expect("append mission");
        let pending_wave = Wave {
            id: "wave-legacy".into(),
            mission_id: "mission-legacy".into(),
            index: 1,
            title: "Legacy Wave".into(),
            objective: "Not yet accepted".into(),
            context: String::new(),
            revision: 1,
            updated_by: Some("host".into()),
            exit_criteria: None,
            status: WaveStatus::Running,
            executor_kind: WaveExecutorKind::Host,
            executor_run_ids: Vec::new(),
            accepted_run_id: None,
            plan_note: None,
            outcome_summary: None,
            artifact_refs: Vec::new(),
            gate_status: WaveGateStatus::Pending,
            gate_note: None,
            accepted_by: None,
            accepted_at: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
        };
        store.append_wave(&pending_wave).expect("append wave");

        let mut closed = mission.clone();
        closed.status = MissionStatus::Completed;
        closed.outcome_summary = Some("done".into());
        closed.completed_by = Some("host".into());
        closed.completed_at = Some("unix-ms:2".into());
        closed.updated_at = "unix-ms:2".into();
        let error = store
            .compare_and_close_mission(&mission, &closed)
            .expect_err("a pending legacy Wave must still block closeout");
        assert!(
            error.to_string().contains("cannot close: Wave"),
            "error: {error}"
        );

        let mut accepted_wave = pending_wave.clone();
        accepted_wave.status = WaveStatus::Completed;
        accepted_wave.gate_status = WaveGateStatus::Accepted;
        accepted_wave.accepted_by = Some("host".into());
        accepted_wave.accepted_at = Some("unix-ms:2".into());
        accepted_wave.updated_at = "unix-ms:2".into();
        store
            .compare_and_append_wave(&pending_wave, &accepted_wave)
            .expect("accept the legacy wave");

        // compare_and_append_wave folds the gate outcome into Mission.status
        // as a side effect (line ~754 above), so the CAS baseline for close
        // must be the freshly stored row, not the pre-gate local `mission`.
        let after_gate = store
            .latest_missions()
            .expect("latest missions")
            .into_iter()
            .find(|row| row.id == "mission-legacy")
            .expect("mission row after gate acceptance");
        let mut closed_after_gate = after_gate.clone();
        closed_after_gate.status = MissionStatus::Completed;
        closed_after_gate.outcome_summary = Some("done".into());
        closed_after_gate.completed_by = Some("host".into());
        closed_after_gate.completed_at = Some("unix-ms:3".into());
        closed_after_gate.updated_at = "unix-ms:3".into();
        store
            .compare_and_close_mission(&after_gate, &closed_after_gate)
            .expect("an accepted legacy Wave allows closeout, same as before ADR 0051");
        assert_eq!(
            store
                .latest_missions()
                .expect("latest missions")
                .into_iter()
                .find(|row| row.id == "mission-legacy")
                .expect("closed mission row")
                .status,
            MissionStatus::Completed
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

    /// A minimal native Mission for Mission Log tests below.
    fn mission_log_test_mission(id: &str) -> Mission {
        Mission {
            id: id.into(),
            title: "Ship the Mission Log cutover".into(),
            objective: "Prove append-only Mission Log semantics".into(),
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
        }
    }

    #[test]
    fn mission_log_entries_round_trip_with_ordered_revisions_and_tail() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-mission-log-round-trip-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store
            .insert_mission(&mission_log_test_mission("mission-log-1"))
            .expect("insert mission");

        let kinds = [
            MissionLogEntryKind::Judgment,
            MissionLogEntryKind::Replan,
            MissionLogEntryKind::Recovery,
            MissionLogEntryKind::CloseoutEvidence,
        ];
        for (index, kind) in kinds.iter().enumerate() {
            let appended = store
                .append_mission_log_entry(MissionLogEntry {
                    id: format!("entry-{index}"),
                    mission_id: "mission-log-1".into(),
                    revision: 0, // store-assigned; must be overwritten below
                    kind: *kind,
                    body: format!("entry body {index}"),
                    actor: "host".into(),
                    created_at: format!("unix-ms:{index}"),
                })
                .unwrap_or_else(|error| panic!("append entry {index}: {error}"));
            // Store-assigned, monotonic starting at 1 -- the CLI's placeholder
            // `revision: 0` is never trusted back.
            assert_eq!(appended.revision, (index + 1) as u32);
        }

        // A second Mission's entries never leak into the first Mission's
        // ledger, exactly like Wave's per-mission index scoping.
        store
            .insert_mission(&mission_log_test_mission("mission-log-2"))
            .expect("insert other mission");
        store
            .append_mission_log_entry(MissionLogEntry {
                id: "entry-other-mission".into(),
                mission_id: "mission-log-2".into(),
                revision: 0,
                kind: MissionLogEntryKind::Judgment,
                body: "unrelated mission's judgment".into(),
                actor: "host".into(),
                created_at: "unix-ms:9".into(),
            })
            .expect("append other-mission entry");

        let entries = store
            .mission_log_entries("mission-log-1")
            .expect("mission log entries");
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.revision)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(entries[0].kind, MissionLogEntryKind::Judgment);
        assert_eq!(entries[3].kind, MissionLogEntryKind::CloseoutEvidence);

        // tail(2) returns the last two, oldest-of-the-tail first (Unix `tail`
        // ordering), never the unrelated Mission's row.
        let tail = store
            .mission_log_tail("mission-log-1", 2)
            .expect("mission log tail");
        assert_eq!(
            tail.iter().map(|entry| entry.revision).collect::<Vec<_>>(),
            vec![3, 4]
        );

        // tail(n) larger than the ledger returns every row, not an error.
        let full_tail = store
            .mission_log_tail("mission-log-1", 100)
            .expect("mission log tail overshoot");
        assert_eq!(full_tail.len(), 4);

        // A Mission with no entries yet has an empty tail, not an error --
        // the CLI/skill treat this as "no mission log yet", not a failure.
        store
            .insert_mission(&mission_log_test_mission("mission-log-empty"))
            .expect("insert empty mission");
        assert_eq!(
            store
                .mission_log_tail("mission-log-empty", 3)
                .expect("empty tail"),
            Vec::new()
        );

        // The raw cross-mission ledger sees every row in append order.
        assert_eq!(store.mission_log().expect("raw mission log").len(), 5);

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn mission_log_entry_rejects_empty_body_empty_actor_and_missing_mission() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-mission-log-validation-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = HarnessStore::new(&root);
        store
            .insert_mission(&mission_log_test_mission("mission-log-validate"))
            .expect("insert mission");

        let base = MissionLogEntry {
            id: "entry-invalid".into(),
            mission_id: "mission-log-validate".into(),
            revision: 0,
            kind: MissionLogEntryKind::Judgment,
            body: "   ".into(),
            actor: "host".into(),
            created_at: "unix-ms:1".into(),
        };
        let empty_body_error = store
            .append_mission_log_entry(base.clone())
            .expect_err("whitespace-only body must be rejected");
        assert!(
            empty_body_error
                .to_string()
                .contains("body must not be empty"),
            "error: {empty_body_error}"
        );

        let mut empty_actor = base.clone();
        empty_actor.body = "a real judgment".into();
        empty_actor.actor = "  ".into();
        let empty_actor_error = store
            .append_mission_log_entry(empty_actor)
            .expect_err("whitespace-only actor must be rejected");
        assert!(
            empty_actor_error
                .to_string()
                .contains("actor must not be empty"),
            "error: {empty_actor_error}"
        );

        let mut missing_mission = base.clone();
        missing_mission.body = "a real judgment".into();
        missing_mission.mission_id = "mission-log-does-not-exist".into();
        let missing_mission_error = store
            .append_mission_log_entry(missing_mission)
            .expect_err("unknown mission must be rejected");
        assert!(
            missing_mission_error
                .to_string()
                .contains("mission not found"),
            "error: {missing_mission_error}"
        );

        // No invalid attempt above left a row behind.
        assert_eq!(
            store
                .mission_log_entries("mission-log-validate")
                .expect("mission log entries")
                .len(),
            0
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn mission_log_entry_revision_is_monotonic_under_concurrent_append() {
        let root = std::env::temp_dir().join(format!(
            "harness-store-mission-log-concurrency-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock")
                .as_millis()
        ));
        let store = Arc::new(HarnessStore::new(&root));
        store
            .insert_mission(&mission_log_test_mission("mission-log-concurrent"))
            .expect("insert mission");

        let barrier = Arc::new(Barrier::new(4));
        let handles = ["a", "b", "c", "d"].map(|tag| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store.append_mission_log_entry(MissionLogEntry {
                    id: format!("entry-concurrent-{tag}"),
                    mission_id: "mission-log-concurrent".into(),
                    revision: 0,
                    kind: MissionLogEntryKind::Judgment,
                    body: format!("concurrent judgment {tag}"),
                    actor: "host".into(),
                    created_at: "unix-ms:1".into(),
                })
            })
        });
        let mut revisions = Vec::new();
        for handle in handles {
            revisions.push(
                handle
                    .join()
                    .expect("append thread")
                    .expect("append entry")
                    .revision,
            );
        }
        revisions.sort_unstable();
        // Four concurrent appends against the same Mission never collide or
        // skip: the store lock serializes the max-plus-one allocation exactly
        // like insert_wave_and_update_mission's index allocation.
        assert_eq!(revisions, vec![1, 2, 3, 4]);
        let stored_revisions = store
            .mission_log_entries("mission-log-concurrent")
            .expect("mission log entries")
            .into_iter()
            .map(|entry| entry.revision)
            .collect::<Vec<_>>();
        assert_eq!(stored_revisions, vec![1, 2, 3, 4]);

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

    fn seed_host_attention_fixture(
        store: &HarnessStore,
        run_id: &str,
        host_thread_id: Option<&str>,
    ) -> (AgentTeamRun, MemberRun, Work) {
        let run = AgentTeamRun {
            id: run_id.into(),
            definition_id: None,
            agent_team_id: None,
            previous_run_id: None,
            mission_id: None,
            wave_id: None,
            project_binding_id: None,
            host_surface: "codex-app".into(),
            host_thread_id: host_thread_id.map(str::to_string),
            host_actor: None,
            host_control_mode: Default::default(),
            objective: "prove exact Host attention".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec![format!("member-{run_id}")],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("seed TeamRun");
        let member = MemberRun {
            id: format!("member-{run_id}"),
            team_run_id: run_id.into(),
            slot_id: None,
            agent_member_id: None,
            name: "builder".into(),
            role: "builder".into(),
            provider: "kimi".into(),
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
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };
        store.append_member_run(&member).expect("seed MemberRun");
        let work = store
            .insert_work(
                Work {
                    id: format!("work-{run_id}"),
                    team_run_id: run_id.into(),
                    team_id: None,
                    parent_work_id: None,
                    source_work_item_ref: None,
                    title: "deliver exact Host attention".into(),
                    context_markdown: String::new(),
                    completion_criteria_markdown: "Host receives exact durable attention".into(),
                    status: WorkStatus::Open,
                    owner_member_id: None,
                    active_member_run_id: Some(member.id.clone()),
                    claim_mode: WorkClaimMode::HostAssign,
                    eligible_member_ids: Vec::new(),
                    prerequisite_work_ids: Vec::new(),
                    priority: WorkPriority::Normal,
                    created_by_member_id: None,
                    created_by_actor: TeamActorRef {
                        kind: TeamActorKind::Host,
                        id: "host".into(),
                        display_name: None,
                        authn_source: None,
                    },
                    result_summary: None,
                    blocker_reason: None,
                    artifact_refs: Vec::new(),
                    check_refs: Vec::new(),
                    github_links: Vec::new(),
                    version: 0,
                    created_at: String::new(),
                    updated_at: String::new(),
                },
                WorkCommandContext {
                    event_id: format!("work-event-{run_id}"),
                    performed_by_actor: TeamActorRef {
                        kind: TeamActorKind::Host,
                        id: "host".into(),
                        display_name: None,
                        authn_source: None,
                    },
                    authority_actor: None,
                    causation_ref: None,
                    idempotency_key: format!("create-work-{run_id}"),
                    created_at: "unix-ms:2".into(),
                    duplicate_ok: false,
                },
            )
            .expect("seed Work");
        (run, member, work)
    }

    #[test]
    fn host_attention_is_durable_exact_bound_and_semantically_separate() {
        let root = team_test_root("host-attention");
        let store = HarnessStore::new(&root);
        let (run, member, work) = seed_host_attention_fixture(&store, "run-a", None);
        let attention = HostAttention {
            id: "host-attention-work-review-a".into(),
            team_run_id: run.id.clone(),
            kind: HostAttentionKind::WorkReviewRequested,
            work_id: work.id.clone(),
            work_version: work.version,
            source_event_ref: "work-event-review-a".into(),
            member_run_id: Some(member.id.clone()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: "unix-ms:3".into(),
            updated_at: "unix-ms:3".into(),
        };
        store
            .ensure_host_attention(&attention)
            .expect("append attention");
        assert!(
            store.team_messages().expect("messages").is_empty(),
            "Work state attention must not fabricate TeamMessage conversation"
        );
        let unbound = store
            .host_attention_inbox_for_team_run(&run.id, false)
            .expect("unbound projection");
        assert_eq!(unbound.attentions.len(), 1);
        assert!(unbound.warning.as_deref().is_some_and(|warning| {
            warning.contains("UNBOUND_HOST") && warning.contains(&run.id)
        }));
        assert!(store
            .host_attention_inboxes_for_native_thread("codex-app", "other-task", false)
            .expect("other task")
            .is_empty());

        let mut bound = run.clone();
        bound.host_thread_id = Some("codex-task-a".into());
        bound.updated_at = "unix-ms:4".into();
        store
            .compare_and_append_team_run(&run, &bound)
            .expect("bind exact Host task");
        let exact = store
            .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false)
            .expect("exact Host inbox");
        assert_eq!(exact.len(), 1);
        assert_eq!(exact[0].attentions[0].id, attention.id);
        assert!(store
            .host_attention_inboxes_for_native_thread("codex-app", "codex-task-b", false)
            .expect("other exact task")
            .is_empty());

        let claimed = store
            .claim_host_attention(
                &attention.id,
                "codex-app",
                "codex-task-a",
                "claim-a",
                "unix-ms:5",
            )
            .expect("claim attention");
        assert!(matches!(claimed, HostAttentionClaimResult::Claimed(_)));
        assert!(matches!(
            store
                .claim_host_attention(
                    &attention.id,
                    "codex-app",
                    "codex-task-a",
                    "claim-a",
                    "unix-ms:5",
                )
                .expect("idempotent claim"),
            HostAttentionClaimResult::Claimed(_)
        ));
        assert!(store
            .claim_host_attention(
                &attention.id,
                "codex-app",
                "codex-task-a",
                "claim-b",
                "unix-ms:5",
            )
            .is_ok_and(|result| result == HostAttentionClaimResult::NotActionable));

        let delivered = store
            .complete_host_attention_claim(
                &attention.id,
                "claim-a",
                "codex-turn-start-1",
                "unix-ms:6",
            )
            .expect("record provider receipt");
        assert_eq!(delivered.status, HostAttentionStatus::Delivered);
        assert!(delivered.needs_host_action());
        assert_eq!(
            store
                .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false,)
                .expect("delivered still actionable")[0]
                .attentions
                .len(),
            1
        );

        let acknowledged = store
            .acknowledge_host_attention(&attention.id, "codex-app", "codex-task-a", "unix-ms:7")
            .expect("Host intake ACK");
        assert_eq!(acknowledged.status, HostAttentionStatus::Acknowledged);
        assert!(store
            .host_attention_inboxes_for_native_thread("codex-app", "codex-task-a", false)
            .expect("actionable inbox after ACK")
            .is_empty());
        assert_eq!(
            store.latest_works().expect("Work remains")[0].status,
            WorkStatus::Open,
            "attention ACK must not accept or request changes on Work"
        );
        assert_eq!(
            store
                .ensure_host_attention(&attention)
                .expect("causal replay remains idempotent")
                .status,
            HostAttentionStatus::Acknowledged,
            "replaying projection must not reset Host intake"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn submitted_and_blocked_work_reconcile_exactly_one_host_attention_each() {
        let root = team_test_root("work-host-attention-reconciliation");
        let store = HarnessStore::new(&root);
        let (_review_run, review_member, review_work) =
            seed_host_attention_fixture(&store, "review-run", Some("review-host-task"));
        let started_review = store
            .start_work(
                &review_work.id,
                review_work.version,
                &review_member.id,
                member_work_context(
                    &review_member.id,
                    "work-event-review-started",
                    "work-command-review-started",
                    "unix-ms:3",
                ),
            )
            .expect("start review Work");
        let submitted = store
            .submit_work(
                &started_review.id,
                started_review.version,
                &review_member.id,
                "ready for exact Host review",
                Vec::new(),
                vec!["cargo:test".into()],
                member_work_context(
                    &review_member.id,
                    "work-event-review-submitted",
                    "work-command-review-submitted",
                    "unix-ms:4",
                ),
            )
            .expect("submit Work");

        let (_blocked_run, blocked_member, blocked_work) =
            seed_host_attention_fixture(&store, "blocked-run", Some("blocked-host-task"));
        let started_blocked = store
            .start_work(
                &blocked_work.id,
                blocked_work.version,
                &blocked_member.id,
                member_work_context(
                    &blocked_member.id,
                    "work-event-blocked-started",
                    "work-command-blocked-started",
                    "unix-ms:5",
                ),
            )
            .expect("start blocked Work");
        let blocked = store
            .block_work(
                &started_blocked.id,
                started_blocked.version,
                &blocked_member.id,
                "needs Host decision",
                member_work_context(
                    &blocked_member.id,
                    "work-event-blocked",
                    "work-command-blocked",
                    "unix-ms:6",
                ),
            )
            .expect("block Work");

        let attentions = store.host_attentions().expect("derived Host attentions");
        assert_eq!(attentions.len(), 2);
        let review_attention = attentions
            .iter()
            .find(|attention| attention.work_id == submitted.id)
            .expect("review attention");
        assert_eq!(
            review_attention.id,
            "host-attention-work-event-review-submitted"
        );
        assert_eq!(
            review_attention.kind,
            HostAttentionKind::WorkReviewRequested
        );
        assert_eq!(review_attention.work_version, submitted.version);
        let blocked_attention = attentions
            .iter()
            .find(|attention| attention.work_id == blocked.id)
            .expect("blocked attention");
        assert_eq!(blocked_attention.id, "host-attention-work-event-blocked");
        assert_eq!(blocked_attention.kind, HostAttentionKind::WorkBlocked);
        assert_eq!(blocked_attention.work_version, blocked.version);
        assert!(
            store.team_messages().expect("TeamMessages").is_empty(),
            "Work-state attention must not fabricate conversation"
        );

        // Simulate the process dying after work_operations.jsonl was fsynced
        // but before host_attentions.jsonl reached disk.
        std::fs::remove_file(root.join("host_attentions.jsonl"))
            .expect("remove derived ledger to simulate crash gap");
        let reconciled = store
            .reconcile_work_host_attentions()
            .expect("repair crash gap from WorkEvent truth");
        assert_eq!(reconciled.len(), 2);
        let repaired_bytes = std::fs::read(root.join("host_attentions.jsonl"))
            .expect("repaired Host-attention ledger");
        assert_eq!(
            repaired_bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .count(),
            2
        );
        store
            .reconcile_work_host_attentions()
            .expect("idempotent second reconciliation");
        assert_eq!(
            std::fs::read(root.join("host_attentions.jsonl")).expect("stable ledger"),
            repaired_bytes,
            "reconciliation must not append duplicates"
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
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
            zero_output_streak: 0,
            last_consumed_work_version: None,
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
                failure_reason: None,
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
                failure_reason: None,
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
                failure_reason: None,
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
                failure_reason: None,
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
                failure_reason: None,
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
                failure_reason: None,
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
                failure_reason: None,
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
                failure_reason: None,
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

    /// When a member fails before binding (pre-bind), queued TeamMessage deliveries
    /// transition to Failed so they do not stay permanently actionable in the inbox.
    #[test]
    fn fail_queued_delivery_clears_pre_bind_mail_and_is_idempotent() {
        let root = team_test_root("pre-bind-mail-fail");
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: "tr-fail-mail".into(),
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
            objective: "fail orphaned mail".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-orphan".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("append run");

        let lease = store
            .acquire_team_supervisor_lease(
                &run.id,
                "supervisor-pre-bind",
                300,
                "test:pre-bind",
                100,
                5_000,
            )
            .expect("acquire Supervisor lease");

        let message = TeamMessage {
            id: "tm-orphan".into(),
            team_run_id: run.id.clone(),
            work_id: None,
            origin_wave_id: None,
            sender: None,
            from_member_id: "host".into(),
            recipients: Vec::new(),
            to_member_ids: vec!["mr-orphan".into()],
            kind: TeamMessageKind::Message,
            body: "orphaned work assignment".into(),
            correlation_id: "corr-orphan".into(),
            causation_id: None,
            response_intent: None,
            evidence_refs: Vec::new(),
            deliveries: vec![TeamMessageDelivery {
                member_id: "mr-orphan".into(),
                policy: TeamDeliveryPolicy::Queue,
                status: TeamDeliveryStatus::Queued,
                attempt: 0,
                claim_id: None,
                claimed_by_supervisor_id: None,
                claimed_generation: None,
                claimed_unix_ms: None,
                claim_expires_unix_ms: None,
                provider_receipt_id: None,
                failure_reason: None,
                updated_at: "unix-ms:2".into(),
            }],
            created_at: "unix-ms:2".into(),
        };
        store
            .append_team_message_checked(&message)
            .expect("append queued message");

        // Pre-bind failure: member never bound, delivery is still Queued.
        let msgs = store.team_messages().expect("read messages");
        let queued = msgs
            .iter()
            .find(|m| m.id == "tm-orphan")
            .expect("tm-orphan present");
        assert_eq!(
            queued.deliveries[0].status,
            TeamDeliveryStatus::Queued,
            "starts queued"
        );

        // Fail the delivery.
        let failed = store
            .fail_team_message_delivery(
                &run.id,
                &message.id,
                "mr-orphan",
                &lease.supervisor_id,
                lease.generation,
                "pre-bind member terminated",
                200,
                "unix-ms:3",
            )
            .expect("fail queued delivery");

        assert_eq!(failed.deliveries[0].status, TeamDeliveryStatus::Failed);
        assert_eq!(
            failed.deliveries[0].failure_reason.as_deref(),
            Some("pre-bind member terminated")
        );
        assert!(failed.deliveries[0].claim_id.is_none());
        assert!(failed.deliveries[0].provider_receipt_id.is_none());

        // Idempotent: same reason succeeds.
        let again = store
            .fail_team_message_delivery(
                &run.id,
                &message.id,
                "mr-orphan",
                &lease.supervisor_id,
                lease.generation,
                "pre-bind member terminated",
                201,
                "unix-ms:4",
            )
            .expect("idempotent fail with same reason");

        assert_eq!(again.deliveries[0].status, TeamDeliveryStatus::Failed);

        // Different reason is rejected.
        let conflict = store
            .fail_team_message_delivery(
                &run.id,
                &message.id,
                "mr-orphan",
                &lease.supervisor_id,
                lease.generation,
                "different reason",
                202,
                "unix-ms:5",
            )
            .expect_err("different failure reason must be rejected");
        assert!(conflict.to_string().contains("different reason"));

        // Message survives store reopen.
        drop(store);
        let reopened = HarnessStore::new(&root);
        let msgs_after = reopened.team_messages().expect("read after reopen");
        let reloaded = latest_by_id(msgs_after, |m| m.id.clone())
            .remove("tm-orphan")
            .expect("tm-orphan survived reopen");
        assert_eq!(reloaded.deliveries[0].status, TeamDeliveryStatus::Failed);
        assert_eq!(
            reloaded.deliveries[0].failure_reason.as_deref(),
            Some("pre-bind member terminated")
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
            zero_output_streak: 0,
            last_consumed_work_version: None,
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
            zero_output_streak: 0,
            last_consumed_work_version: None,
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
            duplicate_ok: false,
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
            duplicate_ok: false,
        }
    }

    fn unassigned_test_work(run_id: &str, id: &str) -> Work {
        Work {
            id: id.into(),
            team_run_id: run_id.into(),
            team_id: None,
            created_by_member_id: None,
            parent_work_id: None,
            source_work_item_ref: None,
            title: format!("Implement Work core — {id}"),
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
            github_links: Vec::new(),
            version: 0,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn test_company_work_item(id: &str, status: WorkItemStatus, updated_at: &str) -> WorkItem {
        let status = serde_json::to_value(status).expect("serialize WorkItem status");
        serde_json::from_value(serde_json::json!({
            "id": id,
            "title": "Compatibility WorkItem",
            "objective": "cut over",
            "status": status,
            "source_document_ref": "document-1",
            "source_record_refs": [],
            "result_document_ref": null,
            "result_record_refs": [],
            "submitted_by": {"actor_type": "human", "actor_id": "human-1"},
            "requested_by": null,
            "accountable_owner": {"actor_type": "human", "actor_id": "human-1"},
            "assignees": [],
            "contributors": [],
            "reviewer": null,
            "approver": null,
            "execution_mode": "agent_team",
            "execution_refs": [],
            "approval_refs": [],
            "evidence_refs": [],
            "artifact_refs": [],
            "outcome_summary": null,
            "due_at": null,
            "priority": null,
            "risk_level": null,
            "created_at": "unix-ms:1",
            "updated_at": updated_at,
            "completed_at": null
        }))
        .expect("WorkItem")
    }

    fn source_linked_work_fixture(
        name: &str,
    ) -> (PathBuf, HarnessStore, PathBuf, HarnessStore, WorkItem, Work) {
        let (root, store, run, _, _) = work_test_fixture(name);
        let company_root = team_test_root(&format!("{name}-company"));
        let company_store = HarnessStore::new(&company_root);
        company_store.init().expect("init Company Store");
        let source = test_company_work_item(
            &format!("company-work-{name}"),
            WorkItemStatus::Archived,
            "unix-ms:2",
        );
        company_store
            .append_jsonl(COMPANY_WORK_ITEMS_LEDGER, &source)
            .expect("seed retired WorkItem");

        let mut linked_run = run.clone();
        linked_run.agent_team_id = Some(format!("agent-team-{name}"));
        linked_run.updated_at = "unix-ms:2".into();
        store
            .append_team_run(&linked_run)
            .expect("link durable AgentTeam");
        let mut work = unassigned_test_work(&run.id, &format!("work-{name}"));
        work.source_work_item_ref = Some(source.id.clone());
        let work = store
            .insert_work(
                work,
                host_work_context(
                    &format!("event-create-{name}"),
                    &format!("command-create-{name}"),
                    "unix-ms:3",
                ),
            )
            .expect("insert compatibility Work");
        (root, store, company_root, company_store, source, work)
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
    fn rebind_redelivers_same_member_run_id_at_a_higher_runtime_generation() {
        let (root, store, run, member, _) = work_test_fixture("same-id-generation-rebind");
        let mut linked_run = run.clone();
        linked_run.agent_team_id = Some("agent-team-same-id-rebind".into());
        linked_run.updated_at = "unix-ms:2".into();
        store
            .append_team_run(&linked_run)
            .expect("link durable AgentTeam");
        let mut assigned = unassigned_test_work(&run.id, "work-same-id-rebind");
        assigned.claim_mode = WorkClaimMode::HostAssign;
        assigned.owner_member_id = member.agent_member_id.clone();
        assigned.active_member_run_id = Some(member.id.clone());
        let created = store
            .insert_work(
                assigned,
                member_work_context(
                    &member.id,
                    "event-create-same-id-rebind",
                    "command-create-same-id-rebind",
                    "unix-ms:3",
                ),
            )
            .expect("create assigned Work");

        let mut failed = member.clone();
        failed.status = MemberRunStatus::Failed;
        failed.finished_at = Some("unix-ms:4".into());
        store
            .append_member_run(&failed)
            .expect("record failed generation");
        let mut replacement = member.clone();
        replacement.runtime_generation += 1;
        replacement.status = MemberRunStatus::Idle;
        replacement.started_at = "unix-ms:5".into();
        replacement.finished_at = None;
        store
            .append_member_run(&replacement)
            .expect("append same-id replacement generation");

        let rebound = store
            .rebind_work(
                &created.id,
                created.version,
                &replacement.id,
                host_work_context(
                    "event-rebind-same-id-generation",
                    "command-rebind-same-id-generation",
                    "unix-ms:6",
                ),
            )
            .expect("higher same-id generation must fence and redeliver Work");
        assert_eq!(rebound.active_member_run_id, created.active_member_run_id);
        assert_eq!(rebound.team_id, created.team_id);
        assert_eq!(rebound.created_by_member_id, created.created_by_member_id);
        let operation = store
            .work_operations()
            .unwrap()
            .into_iter()
            .find(|operation| operation.event.kind == WorkEventKind::Rebound)
            .expect("Rebound operation");
        assert_eq!(operation.event.payload["previous_runtime_generation"], 1);
        assert_eq!(operation.event.payload["replacement_runtime_generation"], 2);
        assert!(store
            .latest_work_deliveries()
            .unwrap()
            .iter()
            .any(|delivery| {
                delivery.work_id == rebound.id
                    && delivery.work_version == rebound.version
                    && delivery.recipient_member_run_id == replacement.id
                    && delivery.status == WorkDeliveryStatus::Queued
            }));
        assert!(store
            .rebind_work(
                &rebound.id,
                rebound.version,
                &replacement.id,
                host_work_context(
                    "event-repeat-same-id-generation",
                    "command-repeat-same-id-generation",
                    "unix-ms:7",
                ),
            )
            .expect_err("same runtime generation cannot rebound twice")
            .to_string()
            .contains("WORK_ALREADY_BOUND"));

        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn sparse_mixed_version_rebound_recovers_and_repersists_work_provenance() {
        let (root, store, run, member, _) = work_test_fixture("sparse-rebound-provenance");
        let mut linked_run = run.clone();
        linked_run.agent_team_id = Some("agent-team-sparse-rebound".into());
        linked_run.updated_at = "unix-ms:2".into();
        store
            .append_team_run(&linked_run)
            .expect("link durable AgentTeam");

        let mut assigned = unassigned_test_work(&run.id, "work-sparse-rebound");
        assigned.claim_mode = WorkClaimMode::HostAssign;
        assigned.owner_member_id = member.agent_member_id.clone();
        assigned.active_member_run_id = Some(member.id.clone());
        let created = store
            .insert_work(
                assigned,
                member_work_context(
                    &member.id,
                    "event-create-sparse-rebound",
                    "command-create-sparse-rebound",
                    "unix-ms:3",
                ),
            )
            .expect("Member creates Team-scoped Work");
        assert_eq!(
            created.team_id.as_deref(),
            Some("agent-team-sparse-rebound")
        );
        assert_eq!(created.created_by_member_id, member.agent_member_id);

        let mut replacement = member.clone();
        replacement.id = "member-sparse-rebound-generation-2".into();
        replacement.runtime_generation += 1;
        replacement.started_at = "unix-ms:4".into();
        store
            .append_member_run(&replacement)
            .expect("append replacement runtime");

        let rebound_context = host_work_context(
            "event-sparse-mixed-writer-rebound",
            "command-sparse-mixed-writer-rebound",
            "unix-ms:5",
        );
        let mut sparse_work = created.clone();
        sparse_work.active_member_run_id = Some(replacement.id.clone());
        sparse_work.team_id = None;
        sparse_work.created_by_member_id = None;
        sparse_work.version += 1;
        sparse_work.updated_at = rebound_context.created_at.clone();
        let sparse_operation = WorkOperation {
            event: WorkEvent {
                id: rebound_context.event_id,
                team_run_id: sparse_work.team_run_id.clone(),
                work_id: sparse_work.id.clone(),
                sequence: 2,
                kind: WorkEventKind::Rebound,
                expected_version: created.version,
                resulting_version: sparse_work.version,
                performed_by_actor: rebound_context.performed_by_actor,
                authority_actor: rebound_context.authority_actor,
                causation_ref: rebound_context.causation_ref,
                idempotency_key: rebound_context.idempotency_key,
                payload: serde_json::json!({
                    "previous_member_run_id": member.id.clone(),
                    "replacement_member_run_id": replacement.id.clone(),
                }),
                created_at: rebound_context.created_at,
            },
            work: sparse_work,
            deliveries: Vec::new(),
            delivery_updates: Vec::new(),
        };
        let refused = store
            .append_work_operation_unlocked(&sparse_operation)
            .expect_err("current writer must refuse provenance regression");
        assert!(refused
            .to_string()
            .contains("WORK_PROJECTION_PROVENANCE_REGRESSION"));

        // Model the already-observed stale HTTP writer: it omitted both keys
        // entirely, bypassing code this newer binary did not yet contain.
        let mut sparse_json = serde_json::to_value(&sparse_operation).expect("operation JSON");
        let sparse_projection = sparse_json["work"]
            .as_object_mut()
            .expect("Work projection object");
        sparse_projection.remove("team_id");
        sparse_projection.remove("created_by_member_id");
        store
            .append_jsonl("work_operations.jsonl", &sparse_json)
            .expect("simulate stale mixed-version append");
        let raw = store.work_operations().expect("raw WorkOperations");
        assert!(raw.last().expect("sparse rebound").work.team_id.is_none());
        assert!(raw
            .last()
            .expect("sparse rebound")
            .work
            .created_by_member_id
            .is_none());

        let recovered = store.latest_works().expect("recovered Works").remove(0);
        assert_eq!(recovered.team_id, created.team_id);
        assert_eq!(recovered.created_by_member_id, created.created_by_member_id);
        let repair_context = host_work_context(
            "event-reconcile-sparse-rebound",
            "command-reconcile-sparse-rebound",
            "unix-ms:6",
        );
        let repaired = store
            .reconcile_work_projection_provenance(
                &recovered.id,
                recovered.version,
                repair_context.clone(),
            )
            .expect("explicit reconciliation re-persists recovered provenance");
        assert_eq!(repaired.status, WorkStatus::Open);
        assert_eq!(
            repaired.active_member_run_id.as_deref(),
            Some(replacement.id.as_str())
        );
        assert_eq!(repaired.team_id, created.team_id);
        assert_eq!(repaired.created_by_member_id, created.created_by_member_id);
        assert_eq!(
            store
                .reconcile_work_projection_provenance(
                    &recovered.id,
                    recovered.version,
                    repair_context,
                )
                .expect("repair retry is idempotent"),
            repaired
        );
        let raw = store.work_operations().expect("repaired WorkOperations");
        assert_eq!(raw.last().expect("repair operation").work, repaired);
        assert_eq!(raw.last().unwrap().event.kind, WorkEventKind::Updated);

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
                failure_reason: None,
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

    // ── duplicate-title guard ──────────────────────────────────────────

    fn work_with_title(run_id: &str, id: &str, title: &str) -> Work {
        let mut work = unassigned_test_work(run_id, id);
        work.title = title.to_string();
        work
    }

    #[test]
    fn duplicate_title_guard_refuses_non_terminal_match() {
        let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-guard");
        let ctx1 = host_work_context("dup-ctx-1", "create-first", "unix-ms:3");
        store
            .insert_work(
                work_with_title(&run.id, "work-audit-1", "Audit Company Docs"),
                ctx1,
            )
            .expect("create first Work");

        let ctx2 = host_work_context("dup-ctx-2", "create-dup", "unix-ms:4");
        let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
        let error = store
            .insert_work(dup, ctx2)
            .expect_err("duplicate title must fail");
        assert!(
            error.to_string().contains("DUPLICATE_TITLE"),
            "error: {error}"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn duplicate_title_guard_allows_when_flag_is_duplicate_ok() {
        let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-flag");
        let ctx1 = host_work_context("dup-ctx-flag-1", "create-first", "unix-ms:3");
        store
            .insert_work(
                work_with_title(&run.id, "work-audit-1", "Audit Company Docs"),
                ctx1,
            )
            .expect("create first Work");

        let mut ctx2 = host_work_context("dup-ctx-flag-2", "create-dup-ok", "unix-ms:4");
        ctx2.duplicate_ok = true;
        let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
        let created = store
            .insert_work(dup, ctx2)
            .expect("duplicate-ok must allow same-title Work");
        assert_eq!(created.title, "Audit Company Docs");
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn duplicate_title_guard_allows_when_existing_is_done() {
        let (root, store, run, member_a, _member_b) = work_test_fixture("dup-title-done");
        let ctx1 = host_work_context("dup-ctx-done-1", "create-first", "unix-ms:3");
        let mut work = work_with_title(&run.id, "work-audit-1", "Audit Company Docs");
        work.claim_mode = WorkClaimMode::HostAssign;
        work.active_member_run_id = Some(member_a.id.clone());
        work.owner_member_id = member_a.agent_member_id.clone();
        let first = store.insert_work(work, ctx1).expect("create first Work");

        // Start → Submit → Accept to make the work Done.
        let first = store
            .start_work(
                &first.id,
                first.version,
                &member_a.id,
                member_work_context(&member_a.id, "start", "start-key", "unix-ms:4"),
            )
            .expect("start");
        let first = store
            .submit_work(
                &first.id,
                first.version,
                &member_a.id,
                "All tests pass.",
                Vec::new(),
                Vec::new(),
                member_work_context(&member_a.id, "submit", "submit-key", "unix-ms:5"),
            )
            .expect("submit");
        store
            .accept_work(
                &first.id,
                first.version,
                host_work_context("accept", "accept-key", "unix-ms:6"),
            )
            .expect("accept first Work");

        let ctx2 = host_work_context("dup-ctx-done-2", "create-after-done", "unix-ms:7");
        let dup = work_with_title(&run.id, "work-audit-2", "Audit Company Docs");
        store
            .insert_work(dup, ctx2)
            .expect("terminal existing Work must not block new same-title");
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn duplicate_title_guard_normalizes_casing_and_spacing() {
        let (root, store, run, _member, _assigned_work) = work_test_fixture("dup-title-normalize");
        let ctx1 = host_work_context("dup-norm-1", "create-first", "unix-ms:3");
        store
            .insert_work(
                work_with_title(&run.id, "work-norm-1", "audit company docs"),
                ctx1,
            )
            .expect("create first Work");

        let ctx2 = host_work_context("dup-norm-2", "create-dup-norm", "unix-ms:4");
        let dup = work_with_title(&run.id, "work-norm-2", "AUDIT   COMPANY   DOCS");
        let error = store
            .insert_work(dup, ctx2)
            .expect_err("different casing/spacing must still be detected");
        assert!(
            error.to_string().contains("DUPLICATE_TITLE"),
            "error: {error}"
        );
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

    fn test_durable_member(id: &str) -> DurableAgentMember {
        DurableAgentMember {
            id: id.into(),
            name: id.into(),
            description: format!("Durable identity for {id}"),
            role: "lead".into(),
            provider_profile: Some("kimi/qwen3.8-max".into()),
            model: Some("qwen/qwen3.8-max".into()),
            workspace_policy: Some("project_binding".into()),
            project_binding_id: Some("project-harness".into()),
            business_access_ceiling_refs: vec!["company_os.read".into()],
            status: harness_core::DurableAgentMemberStatus::Active,
            created_by_member_id: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:2".into(),
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

    #[test]
    fn persistent_work_promotes_and_retargets_without_losing_provenance() {
        let (root, store, run, member_a, _) = work_test_fixture("team-scope-retarget");
        let company_root = team_test_root("team-scope-retarget-company");
        let company_store = HarnessStore::new(&company_root);
        company_store.init().expect("init company store");
        let archived_source: harness_core::WorkItem = serde_json::from_value(serde_json::json!({
            "id": "company-work-persistent",
            "title": "Compatibility WorkItem",
            "objective": "cut over",
            "status": "archived",
            "source_document_ref": "document-1",
            "source_record_refs": [],
            "result_document_ref": null,
            "result_record_refs": [],
            "submitted_by": {"actor_type": "human", "actor_id": "human-1"},
            "requested_by": null,
            "accountable_owner": {"actor_type": "human", "actor_id": "human-1"},
            "assignees": [],
            "contributors": [],
            "reviewer": null,
            "approver": null,
            "execution_mode": "agent_team",
            "execution_refs": [],
            "approval_refs": [],
            "evidence_refs": [],
            "artifact_refs": [],
            "outcome_summary": null,
            "due_at": null,
            "priority": null,
            "risk_level": null,
            "created_at": "unix-ms:1",
            "updated_at": "unix-ms:1",
            "completed_at": null
        }))
        .expect("archived Company WorkItem");
        company_store
            .append_jsonl("company_os_work_items.jsonl", &archived_source)
            .expect("append archived source");

        let mut linked_run = run.clone();
        linked_run.agent_team_id = Some("agent-team-persistent".into());
        linked_run.updated_at = "unix-ms:2".into();
        store
            .append_team_run(&linked_run)
            .expect("link durable team before compatibility Work creation");

        let mut initial = unassigned_test_work(&run.id, "persistent-work");
        initial.claim_mode = WorkClaimMode::HostAssign;
        initial.owner_member_id = member_a.agent_member_id.clone();
        initial.active_member_run_id = Some(member_a.id.clone());
        initial.source_work_item_ref = Some(archived_source.id.clone());
        let created = store
            .insert_work(
                initial,
                member_work_context(
                    &member_a.id,
                    "event-create-persistent",
                    "command-create-persistent",
                    "unix-ms:2",
                ),
            )
            .expect("insert legacy Work");
        assert!(created.team_id.is_none());
        assert_eq!(created.created_by_member_id, member_a.agent_member_id);

        let promoted = store
            .promote_work_to_team_scope(
                &company_store,
                &created.id,
                created.version,
                host_work_context(
                    "event-promote-persistent",
                    "command-promote-persistent",
                    "unix-ms:4",
                ),
            )
            .expect("promote Work");
        assert_eq!(promoted.team_id.as_deref(), Some("agent-team-persistent"));
        assert_eq!(promoted.owner_member_id, created.owner_member_id);
        assert_eq!(promoted.created_by_member_id, created.created_by_member_id);
        assert_eq!(promoted.source_work_item_ref, created.source_work_item_ref);
        assert!(
            store
                .work_cutover_report(&company_store)
                .expect("cutover report")
                .valid
        );

        let mut closed_member = member_a.clone();
        closed_member.coordination_status = harness_core::MemberCoordinationStatus::Closed;
        closed_member.status = harness_core::MemberRunStatus::Stopped;
        closed_member.finished_at = Some("unix-ms:5".into());
        store
            .append_member_run(&closed_member)
            .expect("close old runtime");
        let old_run_attention = HostAttention {
            id: "host-attention-old-runtime-stopped".into(),
            team_run_id: linked_run.id.clone(),
            kind: HostAttentionKind::MemberStoppedWithOwnedReadyWork,
            work_id: promoted.id.clone(),
            work_version: promoted.version,
            source_event_ref: "member-runtime-stopped-old".into(),
            member_run_id: Some(closed_member.id.clone()),
            status: HostAttentionStatus::Actionable,
            attempt: 0,
            claim_id: None,
            claimed_host_surface: None,
            claimed_host_thread_id: None,
            provider_receipt_id: None,
            last_failure_reason: None,
            created_at: "unix-ms:5".into(),
            updated_at: "unix-ms:5".into(),
        };
        store
            .ensure_host_attention(&old_run_attention)
            .expect("record old execution attention before retarget");

        let mut successor_run = linked_run.clone();
        successor_run.id = "tr-team-scope-retarget-successor".into();
        successor_run.previous_run_id = Some(linked_run.id.clone());
        successor_run.member_run_ids = vec!["mr-team-scope-retarget-successor-a".into()];
        successor_run.created_at = "unix-ms:6".into();
        successor_run.updated_at = "unix-ms:6".into();
        store
            .append_team_run(&successor_run)
            .expect("append successor run");
        let mut successor_member = member_a.clone();
        successor_member.id = successor_run.member_run_ids[0].clone();
        successor_member.team_run_id = successor_run.id.clone();
        successor_member.coordination_status = harness_core::MemberCoordinationStatus::Active;
        successor_member.status = harness_core::MemberRunStatus::Idle;
        successor_member.finished_at = None;
        store
            .append_member_run(&successor_member)
            .expect("append successor member");

        let pending_error = store
            .retarget_work_execution(
                &promoted.id,
                promoted.version,
                &successor_run.id,
                Some(&successor_member.id),
                host_work_context(
                    "event-retarget-persistent-pending",
                    "command-retarget-persistent-pending",
                    "unix-ms:7",
                ),
            )
            .expect_err("unresolved old-Host attention must fence retarget");
        assert!(pending_error.to_string().contains("HOST_ATTENTION_PENDING"));
        assert!(matches!(
            store
                .claim_host_attention(
                    &old_run_attention.id,
                    &linked_run.host_surface,
                    linked_run
                        .host_thread_id
                        .as_deref()
                        .expect("bound old Host"),
                    "claim-old-runtime-attention",
                    "unix-ms:7",
                )
                .expect("claim old Host attention"),
            HostAttentionClaimResult::Claimed(_)
        ));
        store
            .complete_host_attention_claim(
                &old_run_attention.id,
                "claim-old-runtime-attention",
                "old-host-provider-receipt",
                "unix-ms:8",
            )
            .expect("deliver to exact old Host");
        store
            .acknowledge_host_attention(
                &old_run_attention.id,
                &linked_run.host_surface,
                linked_run
                    .host_thread_id
                    .as_deref()
                    .expect("bound old Host"),
                "unix-ms:9",
            )
            .expect("old Host ACK before retarget");

        let retargeted = store
            .retarget_work_execution(
                &promoted.id,
                promoted.version,
                &successor_run.id,
                Some(&successor_member.id),
                host_work_context(
                    "event-retarget-persistent",
                    "command-retarget-persistent",
                    "unix-ms:10",
                ),
            )
            .expect("retarget Work");
        assert_eq!(retargeted.team_run_id, successor_run.id);
        assert_eq!(retargeted.team_id, promoted.team_id);
        assert_eq!(retargeted.owner_member_id, promoted.owner_member_id);
        assert_eq!(
            retargeted.created_by_member_id,
            promoted.created_by_member_id
        );
        assert_eq!(
            retargeted.active_member_run_id,
            Some(successor_member.id.clone())
        );

        let events = store.work_events().expect("events");
        assert!(events
            .iter()
            .any(|event| event.kind == WorkEventKind::TeamScopePromoted));
        assert!(events
            .iter()
            .any(|event| event.kind == WorkEventKind::ExecutionRetargeted));
        let deliveries = store.latest_work_deliveries().expect("deliveries");
        assert!(deliveries.iter().any(|delivery| {
            delivery.work_version == retargeted.version
                && delivery.recipient_member_run_id == successor_member.id
                && delivery.status == WorkDeliveryStatus::Queued
        }));
        assert!(
            deliveries
                .iter()
                .filter(|delivery| {
                    delivery.work_id == retargeted.id
                        && delivery.work_version < retargeted.version
                        && delivery.status == WorkDeliveryStatus::Invalidated
                })
                .count()
                >= 2
        );

        std::fs::remove_dir_all(root).expect("remove temp store");
        std::fs::remove_dir_all(company_root).expect("remove company temp store");
    }

    #[test]
    fn concurrent_company_revision_cannot_cross_the_promotion_fence_window() {
        let (root, store, company_root, company_store, source, work) =
            source_linked_work_fixture("cutover-concurrency");
        let context = host_work_context(
            "event-promote-cutover-concurrency",
            "command-promote-cutover-concurrency",
            "unix-ms:4",
        );
        let (entered_tx, entered_rx) = mpsc::channel();
        let release = Arc::new(Barrier::new(2));
        let promoter_release = Arc::clone(&release);
        let promotion_store = store.clone();
        let promotion_company_store = company_store.clone();
        let work_id = work.id.clone();
        let promotion = std::thread::spawn(move || {
            promotion_store.promote_work_to_team_scope_inner(
                &promotion_company_store,
                &work_id,
                work.version,
                context,
                || {
                    entered_tx
                        .send(())
                        .expect("signal validated cross-store snapshot");
                    promoter_release.wait();
                },
                || Ok(()),
            )
        });
        entered_rx
            .recv()
            .expect("promotion reached the pre-fence boundary");

        let mut revived = source.clone();
        revived.status = WorkItemStatus::InProgress;
        revived.updated_at = "unix-ms:5".into();
        let writer_store = company_store.clone();
        let (writer_started_tx, writer_started_rx) = mpsc::channel();
        let (writer_done_tx, writer_done_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            writer_started_tx.send(()).expect("signal writer start");
            let result = writer_store.append_jsonl(COMPANY_WORK_ITEMS_LEDGER, &revived);
            writer_done_tx.send(result).expect("send writer result");
        });
        writer_started_rx.recv().expect("writer started");
        assert!(matches!(
            writer_done_rx.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        release.wait();
        let promoted = promotion
            .join()
            .expect("promotion thread")
            .expect("promotion succeeds");
        let writer_error = writer_done_rx
            .recv()
            .expect("writer completed after fence")
            .expect_err("post-fence WorkItem revision must be refused");
        writer.join().expect("writer thread");

        assert!(promoted.is_team_scoped());
        assert!(writer_error
            .to_string()
            .contains("COMPANY_WORK_ITEM_CUTOVER_FENCED"));
        assert_eq!(
            company_store
                .latest_work_items()
                .expect("Company WorkItems")[0]
                .status,
            WorkItemStatus::Archived
        );
        assert!(
            store
                .work_cutover_report(&company_store)
                .expect("consistent cutover report")
                .valid
        );

        std::fs::remove_dir_all(root).expect("remove Execution Store");
        std::fs::remove_dir_all(company_root).expect("remove Company Store");
    }

    #[test]
    fn deterministic_execution_collision_is_refused_before_company_fencing() {
        let (root, store, company_root, company_store, source, work) =
            source_linked_work_fixture("cutover-preflight");
        let collision = store
            .promote_work_to_team_scope(
                &company_store,
                &work.id,
                work.version,
                host_work_context(
                    "event-create-cutover-preflight",
                    "command-promote-cutover-preflight",
                    "unix-ms:4",
                ),
            )
            .expect_err("event collision must be rejected before the one-way fence");
        assert!(collision.to_string().contains("WORK_EVENT_ID_CONFLICT"));
        assert!(company_store.work_cutover_fences().unwrap().is_empty());

        let mut still_company_authority = source;
        still_company_authority.status = WorkItemStatus::InProgress;
        still_company_authority.updated_at = "unix-ms:5".into();
        company_store
            .append_jsonl(COMPANY_WORK_ITEMS_LEDGER, &still_company_authority)
            .expect("failed preflight must leave Company authority writable");
        assert!(!store.latest_works().unwrap()[0].is_team_scoped());

        std::fs::remove_dir_all(root).expect("remove Execution Store");
        std::fs::remove_dir_all(company_root).expect("remove Company Store");
    }

    #[test]
    fn crash_after_company_fence_restarts_and_retries_exactly_once() {
        let (root, store, company_root, company_store, source, work) =
            source_linked_work_fixture("cutover-crash-retry");
        let context = host_work_context(
            "event-promote-cutover-crash-retry",
            "command-promote-cutover-crash-retry",
            "unix-ms:4",
        );
        let injected = store
            .promote_work_to_team_scope_inner(
                &company_store,
                &work.id,
                work.version,
                context.clone(),
                || {},
                || {
                    Err(StoreError::Conflict(
                        "INJECTED_CRASH_AFTER_COMPANY_FENCE".into(),
                    ))
                },
            )
            .expect_err("injected crash boundary");
        assert!(injected
            .to_string()
            .contains("INJECTED_CRASH_AFTER_COMPANY_FENCE"));
        assert_eq!(company_store.work_cutover_fences().unwrap().len(), 1);
        assert!(!store.latest_works().unwrap()[0].is_team_scoped());
        let mut forbidden_revision = source.clone();
        forbidden_revision.status = WorkItemStatus::InProgress;
        forbidden_revision.updated_at = "unix-ms:5".into();
        assert!(company_store
            .append_jsonl(COMPANY_WORK_ITEMS_LEDGER, &forbidden_revision)
            .expect_err("fence survives failed promotion")
            .to_string()
            .contains("COMPANY_WORK_ITEM_CUTOVER_FENCED"));
        assert!(
            !store
                .work_cutover_report(&company_store)
                .expect("pending-fence report")
                .valid
        );

        let reopened_store = HarnessStore::new(&root);
        let reopened_company_store = HarnessStore::new(&company_root);
        let promoted = reopened_store
            .promote_work_to_team_scope(
                &reopened_company_store,
                &work.id,
                work.version,
                context.clone(),
            )
            .expect("restart completes fenced promotion");
        let repeated = reopened_store
            .promote_work_to_team_scope(&reopened_company_store, &work.id, work.version, context)
            .expect("same retry is idempotent");
        assert_eq!(repeated, promoted);
        assert_eq!(
            reopened_company_store.work_cutover_fences().unwrap().len(),
            1
        );
        assert_eq!(
            reopened_store
                .work_events()
                .unwrap()
                .iter()
                .filter(|event| event.kind == WorkEventKind::TeamScopePromoted)
                .count(),
            1
        );
        assert!(
            reopened_store
                .work_cutover_report(&reopened_company_store)
                .expect("completed cutover report")
                .valid
        );

        std::fs::remove_dir_all(root).expect("remove Execution Store");
        std::fs::remove_dir_all(company_root).expect("remove Company Store");
    }

    #[test]
    fn crash_gap_allows_compatibility_work_to_advance_then_refreshes_promotion() {
        let (root, store, company_root, company_store, _, work) =
            source_linked_work_fixture("cutover-crash-advance");
        let original_context = host_work_context(
            "event-promote-cutover-crash-advance-original",
            "command-promote-cutover-crash-advance-original",
            "unix-ms:4",
        );
        store
            .promote_work_to_team_scope_inner(
                &company_store,
                &work.id,
                work.version,
                original_context.clone(),
                || {},
                || Err(StoreError::Conflict("INJECTED_CRASH_GAP".into())),
            )
            .expect_err("stop after durable Company fence");
        let member = store.member_runs().unwrap().remove(0);
        let advanced = store
            .assign_work(
                &work.id,
                work.version,
                &member.id,
                host_work_context(
                    "event-assign-during-cutover-crash-gap",
                    "command-assign-during-cutover-crash-gap",
                    "unix-ms:5",
                ),
            )
            .expect("TeamRun-scoped compatibility Work remains mutable");
        assert!(advanced.team_id.is_none());
        assert!(store
            .promote_work_to_team_scope(&company_store, &work.id, work.version, original_context,)
            .expect_err("stale expected version must still be refused")
            .to_string()
            .contains("VERSION_CONFLICT"));

        let promoted = store
            .promote_work_to_team_scope(
                &company_store,
                &advanced.id,
                advanced.version,
                host_work_context(
                    "event-promote-cutover-crash-advance-refreshed",
                    "command-promote-cutover-crash-advance-refreshed",
                    "unix-ms:6",
                ),
            )
            .expect("refreshed retry completes original fence intent");
        assert!(promoted.is_team_scoped());
        assert_eq!(company_store.work_cutover_fences().unwrap().len(), 1);
        let promotion = store
            .work_events()
            .unwrap()
            .into_iter()
            .find(|event| event.kind == WorkEventKind::TeamScopePromoted)
            .expect("completion event");
        assert_eq!(promotion.expected_version, advanced.version);
        assert!(
            store
                .work_cutover_report(&company_store)
                .expect("advanced-gap cutover report")
                .valid
        );

        std::fs::remove_dir_all(root).expect("remove Execution Store");
        std::fs::remove_dir_all(company_root).expect("remove Company Store");
    }

    #[test]
    fn migration_verification_detects_and_repairs_a_missing_legacy_fence() {
        let (root, store, company_root, company_store, _, work) =
            source_linked_work_fixture("cutover-fence-migration");
        let promoted = store
            .promote_work_to_team_scope(
                &company_store,
                &work.id,
                work.version,
                host_work_context(
                    "event-promote-cutover-fence-migration",
                    "command-promote-cutover-fence-migration",
                    "unix-ms:4",
                ),
            )
            .expect("initial promotion");
        std::fs::remove_file(company_root.join(WORK_CUTOVER_FENCES_LEDGER))
            .expect("simulate a pre-fence migrated store");
        let report = store
            .work_cutover_report(&company_store)
            .expect("legacy migration report");
        assert!(!report.valid);
        assert!(report.issues.iter().any(|issue| {
            issue.kind == harness_core::WorkCutoverIssueKind::MissingCompanyWorkItemFence
        }));

        let unchanged = HarnessStore::new(&root)
            .promote_work_to_team_scope(
                &HarnessStore::new(&company_root),
                &promoted.id,
                promoted.version,
                host_work_context(
                    "event-repair-cutover-fence-migration",
                    "command-repair-cutover-fence-migration",
                    "unix-ms:5",
                ),
            )
            .expect("migration repair installs fence from existing promotion provenance");
        assert_eq!(unchanged, promoted);
        assert_eq!(store.work_events().unwrap().len(), 2);
        assert_eq!(company_store.work_cutover_fences().unwrap().len(), 1);
        assert!(
            store
                .work_cutover_report(&company_store)
                .expect("repaired migration report")
                .valid
        );

        std::fs::remove_dir_all(root).expect("remove Execution Store");
        std::fs::remove_dir_all(company_root).expect("remove Company Store");
    }

    #[test]
    fn durable_member_insert_and_registry_convergence_refuse_ambiguous_identity() {
        let (root, store) = temp_store("durable-agent-member");
        let durable = test_durable_member("lead");
        store
            .insert_durable_member(&durable)
            .expect("insert durable member");
        assert_eq!(
            store.latest_durable_members().expect("durable projection")["lead"],
            durable
        );
        let duplicate = store
            .insert_durable_member(&durable)
            .expect_err("duplicate durable id must be refused");
        assert!(duplicate.to_string().contains("already exists"));

        let missing_registry = store
            .converge_registry_member(&test_durable_member("compat-missing"))
            .expect_err("convergence requires a compatibility source row");
        assert!(missing_registry
            .to_string()
            .contains("compatibility AgentMember not found"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    // ── Lane B: upstream event push — Work lifecycle → Host attention ──

    #[test]
    fn work_submit_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-submit-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-submit-ha-1"),
                host_work_context("we-submit-1", "create-submit-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-submit-2", "claim-submit-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let _submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec!["check://y".into()],
                member_work_context(&member.id, "we-submit-3", "submit-submit-ha", "unix-ms:4"),
            )
            .expect("submit Work");
        let attentions = store.host_attentions().expect("host attentions");
        let review = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested);
        assert!(
            review.is_some(),
            "bound run must emit WorkReviewRequested on submit"
        );
        assert_eq!(review.unwrap().team_run_id, run.id);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    fn test_github_link(status: &str, ci_status: Option<&str>) -> harness_core::GitHubLink {
        harness_core::GitHubLink {
            kind: harness_core::GitHubLinkKind::PullRequest,
            owner: "cyl19970726".into(),
            repo: "multi-agent-harness".into(),
            number: 365,
            url: "https://github.com/cyl19970726/multi-agent-harness/pull/365".into(),
            status: Some(status.into()),
            ci_status: ci_status.map(str::to_string),
            ci_url: Some(
                "https://github.com/cyl19970726/multi-agent-harness/actions/runs/1".into(),
            ),
        }
    }

    #[test]
    fn update_work_github_links_refreshes_snapshot_without_churn() {
        let (root, store, run, _member, _) = work_test_fixture("github-update");
        let created = store
            .insert_work(
                unassigned_test_work(&run.id, "github-update-1"),
                host_work_context("we-gu-1", "create-github-update", "unix-ms:2"),
            )
            .expect("create Work");
        assert!(created.github_links.is_empty());

        let refreshed = store
            .update_work_github_links(
                &created.id,
                created.version,
                vec![test_github_link("MERGED", Some("success"))],
                host_work_context("we-gu-2", "poll-github-update-1", "unix-ms:3"),
            )
            .expect("refresh snapshot");
        assert_eq!(refreshed.version, created.version + 1);
        assert_eq!(refreshed.github_links.len(), 1);
        assert_eq!(
            refreshed.github_links[0].ci_status.as_deref(),
            Some("success")
        );

        // Steady-state poll with identical links must not churn versions.
        let unchanged = store
            .update_work_github_links(
                &created.id,
                refreshed.version,
                vec![test_github_link("MERGED", Some("success"))],
                host_work_context("we-gu-3", "poll-github-update-2", "unix-ms:4"),
            )
            .expect("steady-state poll is a no-op");
        assert_eq!(unchanged.version, refreshed.version);

        // A changed CI outcome appends one more Updated operation.
        let re_polled = store
            .update_work_github_links(
                &created.id,
                unchanged.version,
                vec![test_github_link("MERGED", Some("failure"))],
                host_work_context("we-gu-4", "poll-github-update-3", "unix-ms:5"),
            )
            .expect("changed CI refreshes");
        assert_eq!(re_polled.version, unchanged.version + 1);
        assert_eq!(
            re_polled.github_links[0].ci_status.as_deref(),
            Some("failure")
        );

        // Stale expected version is rejected.
        let stale = store.update_work_github_links(
            &created.id,
            created.version,
            vec![test_github_link("MERGED", Some("success"))],
            host_work_context("we-gu-5", "poll-github-update-4", "unix-ms:6"),
        );
        assert!(
            stale.is_err() && stale.unwrap_err().to_string().contains("VERSION_CONFLICT"),
            "stale poll must conflict"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn submit_work_on_pr_merge_transitions_in_progress_work_to_review() {
        let (root, store, run, member, _) = work_test_fixture("github-merge-submit");
        let created = store
            .insert_work(
                unassigned_test_work(&run.id, "github-merge-submit-1"),
                host_work_context("we-ms-1", "create-github-merge", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &created.id,
                created.version,
                &member.id,
                member_work_context(&member.id, "we-ms-2", "claim-github-merge", "unix-ms:3"),
            )
            .expect("claim Work");
        assert_eq!(claimed.status, WorkStatus::InProgress);

        // Refuses when no MERGED pull_request link is present.
        let not_merged = store.submit_work_on_pr_merge(
            &claimed.id,
            claimed.version,
            "auto-submit",
            vec![test_github_link("OPEN", Some("success"))],
            host_work_context("we-ms-3", "submit-merge-reject", "unix-ms:4"),
        );
        assert!(
            not_merged.is_err()
                && not_merged
                    .unwrap_err()
                    .to_string()
                    .contains("PR_MERGE_REQUIRED"),
            "auto-submit without a MERGED link must be refused"
        );

        // Observed merge transitions InProgress -> Review with the fresh
        // snapshot stored.
        let submitted = store
            .submit_work_on_pr_merge(
                &claimed.id,
                claimed.version,
                "auto-submitted by GitHub merge observation",
                vec![test_github_link("MERGED", Some("success"))],
                host_work_context("we-ms-4", "submit-merge-ok", "unix-ms:5"),
            )
            .expect("auto-submit on merge");
        assert_eq!(submitted.status, WorkStatus::Review);
        assert_eq!(
            submitted.result_summary.as_deref(),
            Some("auto-submitted by GitHub merge observation")
        );
        assert_eq!(submitted.github_links[0].status.as_deref(), Some("MERGED"));
        assert_eq!(
            submitted.github_links[0].ci_status.as_deref(),
            Some("success")
        );

        // A review Work is not auto-submittable again.
        let re_submit = store.submit_work_on_pr_merge(
            &submitted.id,
            submitted.version,
            "again",
            vec![test_github_link("MERGED", Some("success"))],
            host_work_context("we-ms-5", "submit-merge-again", "unix-ms:6"),
        );
        assert!(
            re_submit.is_err()
                && re_submit
                    .unwrap_err()
                    .to_string()
                    .contains("required state"),
            "review Work must not be auto-submitted twice"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_block_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-block-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-block-ha-1"),
                host_work_context("we-block-1", "create-block-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-block-2", "claim-block-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let _blocked = store
            .block_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "dependency missing",
                member_work_context(&member.id, "we-block-3", "block-block-ha", "unix-ms:4"),
            )
            .expect("block Work");
        let attentions = store.host_attentions().expect("host attentions");
        let blocked = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkBlocked);
        assert!(
            blocked.is_some(),
            "bound run must emit WorkBlocked on block"
        );
        assert_eq!(blocked.unwrap().team_run_id, run.id);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_accept_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-accept-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-accept-ha-1"),
                host_work_context("we-accept-1", "create-accept-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-accept-2", "claim-accept-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://z".into()],
                vec![],
                member_work_context(&member.id, "we-accept-3", "submit-accept-ha", "unix-ms:4"),
            )
            .expect("submit Work");
        let _accepted = store
            .accept_work_with_summary(
                &submitted.id,
                submitted.version,
                Some("Host accepted"),
                host_work_context("we-accept-4", "accept-accept-ha", "unix-ms:5"),
            )
            .expect("accept Work");
        let attentions = store.host_attentions().expect("host attentions");
        let accepted = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkAccepted);
        assert!(
            accepted.is_some(),
            "bound run must emit WorkAccepted on accept"
        );
        assert_eq!(accepted.unwrap().team_run_id, run.id);
        // WorkReviewRequested should still be present from the earlier submit
        let review = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested);
        assert!(
            review.is_some(),
            "WorkReviewRequested must persist after accept"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_changes_requested_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-cr-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-cr-ha-1"),
                host_work_context("we-cr-1", "create-cr-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-cr-2", "claim-cr-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec![],
                member_work_context(&member.id, "we-cr-3", "submit-cr-ha", "unix-ms:4"),
            )
            .expect("submit Work");
        let _changes = store
            .request_work_changes(
                &submitted.id,
                submitted.version,
                "needs more tests",
                host_work_context("we-cr-4", "request-changes-cr-ha", "unix-ms:5"),
            )
            .expect("request changes");
        let attentions = store.host_attentions().expect("host attentions");
        let cr = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkChangesRequested);
        assert!(
            cr.is_some(),
            "bound run must emit WorkChangesRequested on request changes"
        );
        assert_eq!(cr.unwrap().team_run_id, run.id);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_cancel_emits_host_attention_for_bound_run() {
        let (root, store, run, member, _) = work_test_fixture("work-cancel-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-cancel-ha-1"),
                host_work_context("we-cancel-1", "create-cancel-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-cancel-2", "claim-cancel-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let _cancelled = store
            .cancel_work(
                &claimed.id,
                claimed.version,
                "no longer needed",
                host_work_context("we-cancel-3", "cancel-cancel-ha", "unix-ms:4"),
            )
            .expect("cancel Work");
        let attentions = store.host_attentions().expect("host attentions");
        let cancelled = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkCancelled);
        assert!(
            cancelled.is_some(),
            "bound run must emit WorkCancelled on cancel"
        );
        assert_eq!(cancelled.unwrap().team_run_id, run.id);
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn host_attention_dedup_ignores_duplicate_event() {
        let (root, store, run, member, _) = work_test_fixture("work-dedup-ha");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-dedup-ha-1"),
                host_work_context("we-dedup-1", "create-dedup-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-dedup-2", "claim-dedup-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let ctx = member_work_context(&member.id, "we-dedup-3", "submit-dedup-ha", "unix-ms:4");
        let _submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec![],
                ctx.clone(),
            )
            .expect("first submit");
        // Second submit with same idempotency key should be a no-op (dedup).
        let _again = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec![],
                ctx,
            )
            .expect("idempotent second submit");
        let attentions = store.host_attentions().expect("host attentions");
        let review_count = attentions
            .iter()
            .filter(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested)
            .count();
        assert_eq!(
            review_count, 1,
            "dedup must emit exactly one WorkReviewRequested"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_transitions_dont_fail_for_unbound_run() {
        let root = team_test_root("work-unbound-ha");
        let store = HarnessStore::new(&root);
        let run = AgentTeamRun {
            id: "tr-work-unbound-ha".into(),
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
            objective: "prove unbound graceful".into(),
            execution_root: None,
            status: TeamRunStatus::Running,
            member_run_ids: vec!["mr-work-unbound-ha".into()],
            budget_limit_usd: None,
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            completed_at: None,
        };
        store.append_team_run(&run).expect("append unbound run");
        let member = MemberRun {
            id: "mr-work-unbound-ha".into(),
            team_run_id: run.id.clone(),
            slot_id: Some("slot-unbound".into()),
            agent_member_id: Some("agent-unbound".into()),
            name: "Member Unbound".into(),
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
            zero_output_streak: 0,
            last_consumed_work_version: None,
        };
        store.append_member_run(&member).expect("append member");
        let work = store
            .insert_work(
                unassigned_test_work(&run.id, "work-unbound-ha-1"),
                host_work_context("we-ub-1", "create-ub-ha", "unix-ms:2"),
            )
            .expect("create Work");
        let claimed = store
            .claim_work(
                &work.id,
                work.version,
                &member.id,
                member_work_context(&member.id, "we-ub-2", "claim-ub-ha", "unix-ms:3"),
            )
            .expect("claim Work");
        let _submitted = store
            .submit_work(
                &claimed.id,
                claimed.version,
                &member.id,
                "done",
                vec!["artifact://x".into()],
                vec![],
                member_work_context(&member.id, "we-ub-3", "submit-ub-ha", "unix-ms:4"),
            )
            .expect("submit Work with unbound run");
        let attentions = store.host_attentions().expect("host attentions");
        // HostAttention is still emitted at the store level even for unbound runs;
        // the runtime delivery layer gates on binding, not the store.
        let review = attentions
            .iter()
            .find(|a| a.work_id == work.id && a.kind == HostAttentionKind::WorkReviewRequested);
        assert!(
            review.is_some(),
            "WorkReviewRequested must still be emitted for unbound runs"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn work_delivery_failure_emits_host_attention() {
        let (root, store, run, member, _) = work_test_fixture("work-wdf-ha");
        let mut assigned = unassigned_test_work(&run.id, "work-wdf-ha-1");
        assigned.active_member_run_id = Some(member.id.clone());
        assigned.claim_mode = WorkClaimMode::HostAssign;
        let assigned = store
            .insert_work(
                assigned,
                host_work_context("we-wdf-1", "create-wdf-ha", "unix-ms:2"),
            )
            .expect("create assigned Work");
        let delivery = store
            .latest_work_deliveries()
            .expect("deliveries")
            .into_iter()
            .find(|d| d.work_id == assigned.id)
            .expect("delivery");
        let lease = store
            .acquire_team_supervisor_lease(&run.id, "supervisor-wdf", 7, "test", 100, 100)
            .expect("lease");
        let claimed = match store
            .claim_work_delivery(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                "claim-wdf",
                100,
                "unix-ms:3",
            )
            .expect("claim")
        {
            WorkDeliveryClaimResult::Claimed(d) => d,
            _ => panic!("delivery must be claimed"),
        };
        let failed = store
            .fail_work_delivery_claim(
                &run.id,
                &delivery.id,
                &member.id,
                &lease.supervisor_id,
                lease.generation,
                claimed.claim_id.as_deref().expect("claim id"),
                "provider crash",
                101,
                "unix-ms:4",
            )
            .expect("fail delivery");
        assert_eq!(failed.status, WorkDeliveryStatus::Failed);
        let attentions = store.host_attentions().expect("host attentions");
        let wdf = attentions
            .iter()
            .find(|a| a.work_id == assigned.id && a.kind == HostAttentionKind::WorkDeliveryFailed);
        assert!(
            wdf.is_some(),
            "must emit WorkDeliveryFailed for failed delivery claim"
        );
        std::fs::remove_dir_all(root).expect("remove temp store");
    }

    #[test]
    fn root_lead_bootstrap_writes_one_identity_and_one_host_authority() {
        let (root, store) = temp_store("root-lead-bootstrap");
        store
            .insert_agent_team(&test_agent_team("root", &["worker"], None, None))
            .expect("insert compatibility root");
        let lead = test_durable_member("lead");
        let team = store
            .bootstrap_root_lead_member("root", &lead)
            .expect("bootstrap root Lead");
        assert_eq!(team.owner_agent_id, "lead");
        assert_eq!(team.host_member_id.as_deref(), Some("lead"));
        assert!(team.member_ids.iter().any(|id| id == "lead"));
        assert_eq!(store.latest_durable_members().unwrap().len(), 1);

        // Same exact bootstrap is idempotent; a different identity is refused.
        let same = store
            .bootstrap_root_lead_member("root", &lead)
            .expect("repeat bootstrap");
        assert_eq!(same, team);
        assert_eq!(store.latest_durable_members().unwrap().len(), 1);
        assert_eq!(store.teams().unwrap().len(), 2);
        let conflict = store
            .bootstrap_root_lead_member("root", &test_durable_member("other-lead"))
            .expect_err("second root Host must be refused");
        assert!(conflict.to_string().contains("conflicting"));
        std::fs::remove_dir_all(root).expect("remove temp store");
    }
}

//! Finite application boundary for machine coordination. No provider runtime types.
use crate::{
    daemon_error::DaemonResult,
    daemon_protocol::{
        NativeSessionWakeUpdate, PersistedSessionReadRequest, PersistedSessionReadResponse,
    },
};
use harness_core::{
    agentfirm_api::{
        AgentSession, AgentSessionStatus, NativeSessionRef, PermissionCeiling, RuntimeDispatchMode,
    },
    ExecutionSpace, MemberAction, MemberActionStatus, TeamRunStatus,
};
use harness_store::HarnessStore;
use serde::Serialize;
use std::{
    path::Path,
    sync::{atomic::AtomicBool, Arc, Mutex},
};
pub type NativeSessionWakeSink = Arc<dyn Fn(NativeSessionWakeUpdate) + Send + Sync>;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderPermissionMapping {
    pub provider: String,
    pub requested: PermissionCeiling,
    pub effective: PermissionCeiling,
    pub native_sandbox: String,
    pub native_approval: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NodeSessionCapabilities {
    pub start: bool,
    pub resume: bool,
    pub cancel_turn: bool,
    pub stop: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeamRunDriveOutcome {
    /// The TeamRun left `Running`, or its canonical TeamRun/MemberRun/Work/
    /// Message/RuntimeCommand state changed under this generation. A later
    /// adoption would start from a different canonical state.
    Progressed { team_run_status: TeamRunStatus },
    /// The Supervisor returned with the TeamRun still `Running` and not one
    /// canonical row changed. Re-adopting this exact `canonical_state` can
    /// only repeat this outcome, so the daemon holds adoption until the state
    /// changes or an explicit recovery/start intent arrives.
    NoProgress {
        canonical_state: String,
        detail: String,
    },
}

/// Exactly one owner; consuming drive retains the original registration's Drop path.
pub trait PreparedRun: Send {
    fn drive(
        self: Box<Self>,
        space: ExecutionSpace,
        max_concurrency: usize,
        input_acceptance_secs: u64,
        live_sink: NativeSessionWakeSink,
        serving_status: Arc<Mutex<String>>,
    ) -> DaemonResult<TeamRunDriveOutcome>;
}
pub struct PreparedDaemonRun {
    pub handle: Box<dyn PreparedRun>,
    pub project_binding_id: String,
    pub daemon_generation: u64,
    pub supervisor_id: String,
    pub supervisor_generation: u64,
    pub heartbeat_valid: Arc<AtomicBool>,
}
pub trait NodeSessionHandle: Send {
    fn provider(&self) -> &'static str;
    fn native_session_id(&self) -> &str;
}
pub struct OpenedSession {
    pub runtime: Box<dyn NodeSessionHandle>,
    pub native_session_ref: NativeSessionRef,
    pub permission_mapping: ProviderPermissionMapping,
}
#[allow(clippy::too_many_arguments)]
pub trait DaemonApplicationPort: Send + Sync {
    /// Existing immutable Message transfer digest, composed by the application.
    fn message_body_digest(&self, body: &str) -> String;
    fn prepare_team_run(
        &self,
        store: &HarnessStore,
        run_id: &str,
        space: &ExecutionSpace,
        node_id: &str,
        daemon_id: &str,
        instance_id: &str,
        max_concurrency: usize,
    ) -> DaemonResult<PreparedDaemonRun>;
    fn open_node_session(
        &self,
        session: &AgentSession,
        cwd: &Path,
        display_name: &str,
    ) -> Result<OpenedSession, String>;
    fn node_session_capabilities(&self, provider: &str) -> Option<NodeSessionCapabilities>;
    fn map_permission(
        &self,
        provider: &str,
        permission: PermissionCeiling,
    ) -> Result<ProviderPermissionMapping, String>;
    fn effective_delivery_mode(
        &self,
        provider: &str,
        requested: RuntimeDispatchMode,
        lifecycle: AgentSessionStatus,
        busy: bool,
    ) -> Result<RuntimeDispatchMode, String>;
    fn read_native_session(
        &self,
        home: &Path,
        node_id: &str,
        daemon_id: &str,
        generation: u64,
        instance_id: Option<&str>,
        request: &PersistedSessionReadRequest,
    ) -> DaemonResult<PersistedSessionReadResponse>;
    fn ensure_team_message_fabric(
        &self,
        store: &HarnessStore,
        run_id: &str,
        space_id: &str,
        daemon_id: &str,
        generation: u64,
    ) -> DaemonResult<()>;
    fn append_recovery_action(
        &self,
        store: &HarnessStore,
        run_id: &str,
        member_id: &str,
        action: &str,
        status: MemberActionStatus,
        summary: &str,
        detail: &str,
    ) -> DaemonResult<MemberAction>;
    fn append_recovery_action_with_evidence(
        &self,
        store: &HarnessStore,
        run_id: &str,
        member_id: &str,
        action: &str,
        status: MemberActionStatus,
        summary: &str,
        detail: &str,
        provider_status: Option<String>,
        evidence: &[String],
    ) -> DaemonResult<MemberAction>;

    fn list_execution_spaces(&self, home: &Path) -> Result<Vec<ExecutionSpace>, String>;
    fn execution_space(&self, home: &Path, id: &str) -> Result<Option<ExecutionSpace>, String>;
    fn canonical_run_state(
        &self,
        store: &HarnessStore,
        space_id: Option<&str>,
        run_id: &str,
    ) -> DaemonResult<String>;
    fn post_native_wake(
        &self,
        endpoint: &crate::daemon_protocol::NativeSessionWakeEndpoint,
        space_id: &str,
        update: &NativeSessionWakeUpdate,
    ) -> Result<(), crate::daemon_protocol::NativeSessionWakePostError>;
    fn daemon_log_path(&self, home: &Path, node_id: &str) -> std::path::PathBuf;
}

//! Shared wire types, independent of native readers and provider adapters.

use harness_core::agentfirm_api::ActorRef;
use harness_provider_events::{PersistedOrderingKey, ProviderNativeEventRecord};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PersistedSessionReadMode {
    Snapshot,
    Older,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedSessionCursor {
    pub source_generation: String,
    pub ordering_key: PersistedOrderingKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedSessionViewer {
    pub actor: ActorRef,
    #[serde(default)]
    pub authority_actors: Vec<ActorRef>,
    /// Valid only on the machine-local AF_UNIX control path. Remote fabric
    /// callers must present an exact AgentMember or Team Host identity.
    #[serde(default)]
    pub local_operator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedSessionReadRequest {
    pub execution_space_id: String,
    pub project_binding_id: String,
    pub team_id: String,
    pub team_run_id: String,
    pub agent_member_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    pub native_session_fingerprint: String,
    pub node_id: String,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub mode: PersistedSessionReadMode,
    #[serde(default)]
    pub cursor: Option<PersistedSessionCursor>,
    pub limit: usize,
    pub viewer: PersistedSessionViewer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedSessionReadResponse {
    pub schema_version: String,
    pub native_source_ref: String,
    pub source_generation: String,
    pub snapshot_watermark: Option<PersistedOrderingKey>,
    pub records: Vec<ProviderNativeEventRecord>,
    pub has_more: bool,
    pub next_before: Option<PersistedSessionCursor>,
    pub incomplete_tail: bool,
    pub source_reset: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum NativeSessionWakeUpdate {
    MayHaveAdvanced {
        team_run_id: String,
        agent_member_id: String,
        member_run_id: String,
        member_run_generation: u64,
    },
    TurnTerminal {
        team_run_id: String,
        agent_member_id: String,
        member_run_id: String,
        member_run_generation: u64,
    },
}

#[derive(Clone)]
pub(crate) struct NativeSessionWakeEndpoint {
    pub(crate) authority: String,
    pub(crate) token: String,
    pub(crate) serve_instance_id: String,
}

#[derive(Debug)]
pub(crate) enum NativeSessionWakePostError {
    Unavailable(std::io::Error),
    Rejected(String),
}

impl NativeSessionWakePostError {
    pub(crate) fn clears_registered_endpoint(&self) -> bool {
        matches!(self, Self::Unavailable(_))
    }
}

impl std::fmt::Display for NativeSessionWakePostError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(error) => write!(formatter, "serve callback unavailable: {error}"),
            Self::Rejected(status) => {
                write!(formatter, "serve rejected exact live scope: {status}")
            }
        }
    }
}

impl From<std::io::Error> for NativeSessionWakePostError {
    fn from(error: std::io::Error) -> Self {
        Self::Unavailable(error)
    }
}

pub(crate) const AT_CAPACITY_REFUSAL: &str = "NodeDaemon at capacity";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_read_viewer_defaults_and_wake_wire_shape_remain_closed() {
        let viewer = serde_json::json!({"actor": {"kind": "agent_member", "id": "member"}});
        let decoded: PersistedSessionViewer = serde_json::from_value(viewer.clone()).unwrap();
        assert!(!decoded.local_operator);
        assert!(decoded.authority_actors.is_empty());
        let mut extra = viewer;
        extra["unreviewed_authority"] = true.into();
        assert!(serde_json::from_value::<PersistedSessionViewer>(extra).is_err());
        let wake = serde_json::json!({"reason": "turn_terminal", "team_run_id": "run", "agent_member_id": "member", "member_run_id": "lane", "member_run_generation": 3});
        let decoded: NativeSessionWakeUpdate = serde_json::from_value(wake.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), wake);
        let mut extra = wake;
        extra["session_generation"] = 4.into();
        assert!(serde_json::from_value::<NativeSessionWakeUpdate>(extra).is_err());
    }
}

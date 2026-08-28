use serde::{Deserialize, Serialize};

pub const PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION: &str =
    "agentfirm.provider_native_event_record.v2";
pub const PROVIDER_EVENT_ADAPTER_VERSION: &str = "agentfirm.provider_event_adapter.v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Codex,
    Claude,
    Kimi,
    Pi,
    DeepseekHarness,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Kimi => "kimi",
            Self::Pi => "pi",
            Self::DeepseekHarness => "deepseek_harness",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticKind {
    SessionMetadata,
    UserInput,
    Reasoning,
    AssistantResponse,
    ToolCallRequested,
    ToolCallStarted,
    ToolCallCompleted,
    ToolCallFailed,
    CommandEvent,
    FileEvent,
    ArtifactCreated,
    UsageReported,
    InteractionRequired,
    InteractionResolved,
    RuntimeStarted,
    RuntimeReady,
    RuntimeStopped,
    TransportInterrupted,
    TurnCompleted,
    TurnFailed,
    TurnCancelled,
    CommandRecoveryRequired,
    MalformedOrIncomplete,
    UnclassifiedNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePhase {
    Requested,
    Started,
    Progress,
    Terminal,
    Recovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Partial,
    Complete,
    Incomplete,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectCertainty {
    None,
    NotApplied,
    Applied,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FragmentVisibility {
    TeamSession,
    TeamPublic,
    OperatorOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FragmentPayload {
    Native {
        #[serde(default)]
        event_type: Option<String>,
    },
    SessionMetadata {
        #[serde(default)]
        native_session_id: Option<String>,
    },
    UserInput {
        text: String,
    },
    AssistantResponse {
        text: String,
    },
    Reasoning {
        text: String,
    },
    Tool {
        tool_name: String,
        #[serde(default)]
        call_id: Option<String>,
        #[serde(default)]
        display_detail: Option<String>,
    },
    Command {
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        status: Option<String>,
    },
    File {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        action: Option<String>,
    },
    Artifact {
        display_name: String,
        #[serde(default)]
        media_type: Option<String>,
        #[serde(default)]
        content_digest: Option<String>,
    },
    Usage {
        #[serde(default)]
        input_tokens: Option<u64>,
        #[serde(default)]
        output_tokens: Option<u64>,
        #[serde(default)]
        total_tokens: Option<u64>,
    },
    Interaction {
        reason_code: String,
        prompt: String,
    },
    Runtime {
        state: String,
    },
    Transport {
        reason_code: String,
    },
    Turn {
        outcome: String,
        #[serde(default)]
        display_summary: Option<String>,
    },
    Recovery {
        reason_code: String,
    },
    Malformed {
        reason_code: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderEventFragment {
    pub fragment_id: String,
    pub fragment_index: u32,
    pub semantic_kind: SemanticKind,
    pub lifecycle_phase: LifecyclePhase,
    pub completeness: Completeness,
    pub effect_certainty: EffectCertainty,
    pub visibility: FragmentVisibility,
    pub payload: FragmentPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderNativeEventRecord {
    pub schema_version: String,
    pub record_id: String,
    pub provider: ProviderKind,
    pub adapter_version: String,
    /// Opaque, scoped provider-source locator. Never a provider filesystem path
    /// and never a Harness Evidence reference.
    pub native_source_ref: String,
    #[serde(alias = "agent_identity_id")]
    pub agent_member_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub provider_event_id: Option<String>,
    pub ordering_position: u64,
    #[serde(default)]
    pub causal_parent_id: Option<String>,
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub runtime_command_id: Option<String>,
    #[serde(default)]
    pub occurred_at: Option<String>,
    pub observed_at: String,
    /// Exact provider-native row, preserved response-locally for the local
    /// Session viewer. Harness never writes this value to a durable Store.
    pub native_event: serde_json::Value,
    /// Fingerprint of the provider-native content used for response-local
    /// dedupe. It is not a Harness Evidence reference.
    pub source_content_fingerprint: String,
    /// Ordered semantic projections of this exact native record. One native
    /// row may contain several independently useful fragments (for example a
    /// Claude response with reasoning, text and tool use). The raw record is
    /// retained exactly once above and is never copied into the fragments.
    pub fragments: Vec<ProviderEventFragment>,
}

impl ProviderNativeEventRecord {
    pub fn is_team_public_allowlisted(fragment: &ProviderEventFragment) -> bool {
        fragment.visibility == FragmentVisibility::TeamPublic
            && matches!(
                fragment.semantic_kind,
                SemanticKind::InteractionRequired
                    | SemanticKind::InteractionResolved
                    | SemanticKind::RuntimeStarted
                    | SemanticKind::RuntimeReady
                    | SemanticKind::RuntimeStopped
                    | SemanticKind::TransportInterrupted
                    | SemanticKind::CommandRecoveryRequired
            )
    }

    pub fn validate(&self) -> Result<(), ObservationValidationError> {
        if self.schema_version != PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION
            || self.adapter_version != PROVIDER_EVENT_ADAPTER_VERSION
        {
            return Err(ObservationValidationError::UnsupportedVersion);
        }
        if self.ordering_position == 0 {
            return Err(ObservationValidationError::InvalidOrdering);
        }
        if self.agent_session_generation == 0
            || self.node_daemon_generation == 0
            || !self.native_source_ref.starts_with("provider-source:")
            || !self.source_content_fingerprint.starts_with("sha256:")
            || self.source_content_fingerprint.len() != 71
        {
            return Err(ObservationValidationError::InvalidAuthorityOrSource);
        }
        if self.fragments.is_empty()
            || self
                .fragments
                .iter()
                .enumerate()
                .any(|(index, fragment)| fragment.fragment_index as usize != index)
        {
            return Err(ObservationValidationError::InvalidFragments);
        }
        for fragment in &self.fragments {
            if fragment.visibility == FragmentVisibility::TeamPublic
                && !Self::is_team_public_allowlisted(fragment)
            {
                return Err(ObservationValidationError::PrivateSemanticKind);
            }
            if self.runtime_command_id.is_none()
                && fragment.effect_certainty != EffectCertainty::None
            {
                return Err(ObservationValidationError::UnboundEffect);
            }
            if fragment.semantic_kind == SemanticKind::CommandRecoveryRequired
                && (self.runtime_command_id.is_none()
                    || fragment.effect_certainty != EffectCertainty::Unknown
                    || fragment.completeness != Completeness::RecoveryRequired)
            {
                return Err(ObservationValidationError::InvalidRecovery);
            }
            let payload_matches = matches!(
                (fragment.semantic_kind, &fragment.payload),
                (
                    SemanticKind::UnclassifiedNative,
                    FragmentPayload::Native { .. }
                ) | (
                    SemanticKind::SessionMetadata,
                    FragmentPayload::SessionMetadata { .. }
                ) | (SemanticKind::UserInput, FragmentPayload::UserInput { .. })
                    | (
                        SemanticKind::AssistantResponse,
                        FragmentPayload::AssistantResponse { .. }
                    )
                    | (SemanticKind::Reasoning, FragmentPayload::Reasoning { .. })
                    | (
                        SemanticKind::ToolCallRequested
                            | SemanticKind::ToolCallStarted
                            | SemanticKind::ToolCallCompleted
                            | SemanticKind::ToolCallFailed,
                        FragmentPayload::Tool { .. }
                    )
                    | (SemanticKind::CommandEvent, FragmentPayload::Command { .. })
                    | (SemanticKind::FileEvent, FragmentPayload::File { .. })
                    | (
                        SemanticKind::ArtifactCreated,
                        FragmentPayload::Artifact { .. }
                    )
                    | (SemanticKind::UsageReported, FragmentPayload::Usage { .. })
                    | (
                        SemanticKind::InteractionRequired | SemanticKind::InteractionResolved,
                        FragmentPayload::Interaction { .. }
                    )
                    | (
                        SemanticKind::RuntimeStarted
                            | SemanticKind::RuntimeReady
                            | SemanticKind::RuntimeStopped,
                        FragmentPayload::Runtime { .. }
                    )
                    | (
                        SemanticKind::TransportInterrupted,
                        FragmentPayload::Transport { .. }
                    )
                    | (
                        SemanticKind::TurnCompleted
                            | SemanticKind::TurnFailed
                            | SemanticKind::TurnCancelled,
                        FragmentPayload::Turn { .. }
                    )
                    | (
                        SemanticKind::CommandRecoveryRequired,
                        FragmentPayload::Recovery { .. }
                    )
                    | (
                        SemanticKind::MalformedOrIncomplete,
                        FragmentPayload::Malformed { .. }
                    )
            );
            if !payload_matches {
                return Err(ObservationValidationError::PayloadMismatch);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ObservationValidationError {
    #[error("observation schema or adapter version is unsupported")]
    UnsupportedVersion,
    #[error("observation has an impossible ordering position")]
    InvalidOrdering,
    #[error("private semantic content cannot enter the Team projection")]
    PrivateSemanticKind,
    #[error("effect certainty requires an exact RuntimeCommand binding")]
    UnboundEffect,
    #[error("recovery observations require an exact command and unknown effect")]
    InvalidRecovery,
    #[error("semantic kind and payload variant do not match")]
    PayloadMismatch,
    #[error("authority generation or provider-source provenance is invalid")]
    InvalidAuthorityOrSource,
    #[error("event record must contain ordered semantic fragments")]
    InvalidFragments,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionEventProjection {
    pub schema_version: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    /// Fingerprint of this response's bounded, on-demand projection. It is not
    /// a cursor, history id, replay token, or evidence reference.
    pub source_snapshot_fingerprint: String,
    pub episodes: Vec<super::SessionEpisode>,
    pub truncated: bool,
    /// Typed read state for the UI. An available projection may legitimately
    /// contain zero episodes; absence is never inferred from an empty array.
    pub availability: SessionProjectionAvailability,
    #[serde(default)]
    pub unavailable_reason_code: Option<String>,
    #[serde(default)]
    pub disabled_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionProjectionAvailability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamRuntimeActivity {
    pub record_id: String,
    pub fragment_id: String,
    #[serde(alias = "agent_identity_id")]
    pub agent_member_id: String,
    pub semantic_kind: SemanticKind,
    pub lifecycle_phase: LifecyclePhase,
    pub completeness: Completeness,
    pub effect_certainty: EffectCertainty,
    #[serde(default)]
    pub occurred_at: Option<String>,
    pub payload: FragmentPayload,
}

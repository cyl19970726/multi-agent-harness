use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    Completeness, EffectCertainty, LifecyclePhase, ObservationPayload, ObservationVisibility,
    ProviderKind, ProviderObservation, SemanticKind, PROVIDER_EVENT_ADAPTER_VERSION,
    PROVIDER_OBSERVATION_SCHEMA_VERSION,
};

const MAX_TEXT_CHARS: usize = 4_000;
const MAX_DISPLAY_DETAIL_CHARS: usize = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterFidelity {
    Structured,
    Summary,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterManifest {
    pub provider: ProviderKind,
    pub adapter_version: String,
    pub native_families: Vec<String>,
    pub fidelity: AdapterFidelity,
    pub stable_native_identity: bool,
    pub streaming: bool,
    pub terminal_events: bool,
    pub tool_events: bool,
    pub artifact_events: bool,
    pub usage_events: bool,
    pub interaction_events: bool,
    pub cancellation_events: bool,
    pub supported_semantic_kinds: Vec<SemanticKind>,
    pub redaction_policy: String,
}

pub fn adapter_manifest(provider: ProviderKind) -> AdapterManifest {
    let (families, stable_native_identity, supported) = match provider {
        ProviderKind::Codex => (
            &["event_msg", "response_item", "turn_notification"][..],
            true,
            &[
                SemanticKind::AuthoredResponse,
                SemanticKind::ToolCallRequested,
                SemanticKind::ToolCallStarted,
                SemanticKind::ToolCallCompleted,
                SemanticKind::UsageReported,
                SemanticKind::TransportInterrupted,
                SemanticKind::TurnCompleted,
                SemanticKind::TurnFailed,
                SemanticKind::TurnCancelled,
            ][..],
        ),
        ProviderKind::Claude => (
            &["message", "stream_event", "result"][..],
            false,
            &[
                SemanticKind::AuthoredResponse,
                SemanticKind::ToolCallRequested,
                SemanticKind::ToolCallCompleted,
                SemanticKind::UsageReported,
                SemanticKind::TransportInterrupted,
                SemanticKind::TurnCompleted,
                SemanticKind::TurnCancelled,
            ][..],
        ),
        ProviderKind::Kimi => (
            &["turn", "context.append_loop_event", "acp_notification"][..],
            false,
            &[
                SemanticKind::AuthoredResponse,
                SemanticKind::ToolCallRequested,
                SemanticKind::ToolCallCompleted,
                SemanticKind::ArtifactCreated,
                SemanticKind::TransportInterrupted,
                SemanticKind::TurnCompleted,
                SemanticKind::TurnCancelled,
            ][..],
        ),
        ProviderKind::Pi => (
            &["rpc_event", "session_message"][..],
            false,
            &[
                SemanticKind::AuthoredResponse,
                SemanticKind::ToolCallStarted,
                SemanticKind::ToolCallCompleted,
                SemanticKind::ToolCallFailed,
                SemanticKind::ArtifactCreated,
                SemanticKind::InteractionRequired,
                SemanticKind::TransportInterrupted,
                SemanticKind::TurnCompleted,
            ][..],
        ),
    };
    manifest(provider, families, stable_native_identity, supported)
}

fn manifest(
    provider: ProviderKind,
    families: &[&str],
    stable_native_identity: bool,
    supported: &[SemanticKind],
) -> AdapterManifest {
    let has = |kind| supported.contains(&kind);
    AdapterManifest {
        provider,
        adapter_version: PROVIDER_EVENT_ADAPTER_VERSION.into(),
        native_families: families.iter().map(|item| (*item).into()).collect(),
        fidelity: AdapterFidelity::Structured,
        stable_native_identity,
        streaming: true,
        terminal_events: supported.iter().any(|kind| {
            matches!(
                kind,
                SemanticKind::TurnCompleted
                    | SemanticKind::TurnFailed
                    | SemanticKind::TurnCancelled
            )
        }),
        tool_events: supported.iter().any(|kind| {
            matches!(
                kind,
                SemanticKind::ToolCallRequested
                    | SemanticKind::ToolCallStarted
                    | SemanticKind::ToolCallCompleted
                    | SemanticKind::ToolCallFailed
            )
        }),
        artifact_events: has(SemanticKind::ArtifactCreated),
        usage_events: has(SemanticKind::UsageReported),
        interaction_events: supported.iter().any(|kind| {
            matches!(
                kind,
                SemanticKind::InteractionRequired | SemanticKind::InteractionResolved
            )
        }),
        cancellation_events: has(SemanticKind::TurnCancelled),
        supported_semantic_kinds: supported.to_vec(),
        redaction_policy: "drop_secrets_paths_raw_tool_io_and_private_reasoning".into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeContext {
    pub provider: ProviderKind,
    pub native_source_ref: String,
    pub agent_identity_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub provider_thread_id: Option<String>,
    pub runtime_command_id: Option<String>,
    pub observed_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeEvent {
    pub native_event_id: Option<String>,
    pub provider_turn_id: Option<String>,
    pub ordering_position: u64,
    pub occurred_at: Option<String>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeOutcome {
    Observation(Box<ProviderObservation>),
    DroppedPrivate,
    Unsupported,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("decode context is incomplete")]
    InvalidContext,
    #[error("native event has no ordering position")]
    MissingOrdering,
    #[error("native event is malformed: {0}")]
    Malformed(&'static str),
    #[error("provider event attempted to select server authority")]
    AuthorityInjection,
    #[error("provider event violates the canonical semantic contract")]
    InvalidSemantic,
}

pub fn decode_native_json_line(
    context: &DecodeContext,
    native_event_id: Option<String>,
    provider_turn_id: Option<String>,
    ordering_position: u64,
    occurred_at: Option<String>,
    line: &str,
) -> Result<DecodeOutcome, DecodeError> {
    match serde_json::from_str(line) {
        Ok(raw) => decode_native_event(
            context,
            NativeEvent {
                native_event_id,
                provider_turn_id,
                ordering_position,
                occurred_at,
                raw,
            },
        ),
        Err(_) => {
            if ordering_position == 0 {
                return Err(DecodeError::MissingOrdering);
            }
            Ok(DecodeOutcome::Observation(Box::new(malformed_observation(
                context,
                native_event_id,
                provider_turn_id,
                ordering_position,
                occurred_at,
                line,
            ))))
        }
    }
}

pub fn decode_native_event(
    context: &DecodeContext,
    event: NativeEvent,
) -> Result<DecodeOutcome, DecodeError> {
    validate_context(context, &event)?;
    reject_authority_injection(&event.raw)?;
    let decoded = if let Some(common) = decode_common(&event)? {
        Some(common)
    } else {
        match context.provider {
            ProviderKind::Codex => decode_codex(&event),
            ProviderKind::Claude => decode_claude(&event),
            ProviderKind::Kimi => decode_kimi(&event),
            ProviderKind::Pi => decode_pi(&event),
        }?
    };
    let Some(decoded) = decoded else {
        return Ok(DecodeOutcome::Unsupported);
    };
    if decoded.private_reasoning {
        return Ok(DecodeOutcome::DroppedPrivate);
    }
    let source_fingerprint = fingerprint(&event.raw);
    let native_identity = event.native_event_id.clone().unwrap_or_else(|| {
        format!(
            "fingerprint:{}:{}",
            event.ordering_position,
            &source_fingerprint["sha256:".len()..]
        )
    });
    let observation_id = format!(
        "{}:{}:{}",
        context.provider.as_str(),
        context.agent_session_id,
        native_identity
    );
    let observation = ProviderObservation {
        schema_version: PROVIDER_OBSERVATION_SCHEMA_VERSION.into(),
        observation_id,
        provider: context.provider,
        adapter_version: PROVIDER_EVENT_ADAPTER_VERSION.into(),
        native_source_ref: context.native_source_ref.clone(),
        agent_identity_id: context.agent_identity_id.clone(),
        agent_session_id: context.agent_session_id.clone(),
        agent_session_generation: context.agent_session_generation,
        node_daemon_id: context.node_daemon_id.clone(),
        node_daemon_generation: context.node_daemon_generation,
        provider_thread_id: context.provider_thread_id.clone(),
        provider_turn_id: event
            .provider_turn_id
            .or_else(|| native_turn_id(&event.raw)),
        provider_event_id: event.native_event_id,
        ordering_position: event.ordering_position,
        causal_parent_id: decoded.causal_parent_id,
        correlation_id: decoded.correlation_id,
        runtime_command_id: context.runtime_command_id.clone(),
        occurred_at: event.occurred_at,
        observed_at: context.observed_at.clone(),
        semantic_kind: decoded.kind,
        lifecycle_phase: decoded.phase,
        completeness: decoded.completeness,
        effect_certainty: decoded.effect_certainty,
        visibility: decoded.visibility,
        validated_references: vec![],
        redacted: decoded.redacted,
        truncated: decoded.truncated,
        source_evidence_fingerprint: source_fingerprint,
        payload: decoded.payload,
    };
    observation
        .validate()
        .map_err(|_| DecodeError::InvalidSemantic)?;
    Ok(DecodeOutcome::Observation(Box::new(observation)))
}

fn decode_common(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    let row_type = string(&event.raw, "/type")?;
    Ok(match row_type {
        "runtime_started" => Some(public_runtime(
            ObservationPayload::Runtime {
                state: "started".into(),
            },
            SemanticKind::RuntimeStarted,
            LifecyclePhase::Started,
            Completeness::Partial,
        )),
        "runtime_ready" => Some(public_runtime(
            ObservationPayload::Runtime {
                state: "ready".into(),
            },
            SemanticKind::RuntimeReady,
            LifecyclePhase::Progress,
            Completeness::Complete,
        )),
        "runtime_stopped" => Some(public_runtime(
            ObservationPayload::Runtime {
                state: "stopped".into(),
            },
            SemanticKind::RuntimeStopped,
            LifecyclePhase::Terminal,
            Completeness::Complete,
        )),
        "interaction_required" => Some(interaction(&event.raw)),
        "interaction_resolved" => Some(public_runtime(
            ObservationPayload::Interaction {
                reason_code: safe_label(
                    event
                        .raw
                        .get("reasonCode")
                        .and_then(|value| value.as_str())
                        .unwrap_or("provider_interaction_resolved"),
                ),
                prompt: "Provider interaction was resolved".into(),
            },
            SemanticKind::InteractionResolved,
            LifecyclePhase::Terminal,
            Completeness::Complete,
        )),
        "command_recovery_required" => Some(Decoded {
            kind: SemanticKind::CommandRecoveryRequired,
            phase: LifecyclePhase::Recovery,
            completeness: Completeness::RecoveryRequired,
            effect_certainty: EffectCertainty::Unknown,
            visibility: ObservationVisibility::TeamPublic,
            payload: ObservationPayload::Recovery {
                reason_code: "native_effect_unknown".into(),
            },
            causal_parent_id: None,
            correlation_id: None,
            redacted: true,
            truncated: false,
            private_reasoning: false,
        }),
        _ => None,
    })
}

fn malformed_observation(
    context: &DecodeContext,
    native_event_id: Option<String>,
    provider_turn_id: Option<String>,
    ordering_position: u64,
    occurred_at: Option<String>,
    line: &str,
) -> ProviderObservation {
    let source_fingerprint = format!("sha256:{:x}", Sha256::digest(line.as_bytes()));
    let native_identity = native_event_id.clone().unwrap_or_else(|| {
        format!(
            "malformed:{ordering_position}:{}",
            &source_fingerprint["sha256:".len()..]
        )
    });
    ProviderObservation {
        schema_version: PROVIDER_OBSERVATION_SCHEMA_VERSION.into(),
        observation_id: format!(
            "{}:{}:{native_identity}",
            context.provider.as_str(),
            context.agent_session_id
        ),
        provider: context.provider,
        adapter_version: PROVIDER_EVENT_ADAPTER_VERSION.into(),
        native_source_ref: context.native_source_ref.clone(),
        agent_identity_id: context.agent_identity_id.clone(),
        agent_session_id: context.agent_session_id.clone(),
        agent_session_generation: context.agent_session_generation,
        node_daemon_id: context.node_daemon_id.clone(),
        node_daemon_generation: context.node_daemon_generation,
        provider_thread_id: context.provider_thread_id.clone(),
        provider_turn_id,
        provider_event_id: native_event_id,
        ordering_position,
        causal_parent_id: None,
        correlation_id: None,
        runtime_command_id: context.runtime_command_id.clone(),
        occurred_at,
        observed_at: context.observed_at.clone(),
        semantic_kind: SemanticKind::MalformedOrIncomplete,
        lifecycle_phase: LifecyclePhase::Recovery,
        completeness: Completeness::Incomplete,
        effect_certainty: EffectCertainty::None,
        visibility: ObservationVisibility::OperatorOnly,
        validated_references: vec![],
        redacted: true,
        truncated: false,
        source_evidence_fingerprint: source_fingerprint,
        payload: ObservationPayload::Malformed {
            reason_code: "native_json_malformed".into(),
        },
    }
}

fn validate_context(context: &DecodeContext, event: &NativeEvent) -> Result<(), DecodeError> {
    if !context.native_source_ref.starts_with("evidence:")
        || context.agent_identity_id.trim().is_empty()
        || context.agent_session_id.trim().is_empty()
        || context.node_daemon_id.trim().is_empty()
        || context.observed_at.trim().is_empty()
        || context.agent_session_generation == 0
        || context.node_daemon_generation == 0
    {
        return Err(DecodeError::InvalidContext);
    }
    if event.ordering_position == 0 {
        return Err(DecodeError::MissingOrdering);
    }
    Ok(())
}

fn reject_authority_injection(raw: &serde_json::Value) -> Result<(), DecodeError> {
    const FORBIDDEN: &[&str] = &[
        "agent_identity_id",
        "agent_session_id",
        "agent_session_generation",
        "node_daemon_id",
        "node_daemon_generation",
        "runtime_command_id",
        "visibility",
        "permission_ceiling",
        "validated_references",
    ];
    if raw
        .as_object()
        .is_some_and(|object| FORBIDDEN.iter().any(|field| object.contains_key(*field)))
    {
        Err(DecodeError::AuthorityInjection)
    } else {
        Ok(())
    }
}

struct Decoded {
    kind: SemanticKind,
    phase: LifecyclePhase,
    completeness: Completeness,
    effect_certainty: EffectCertainty,
    visibility: ObservationVisibility,
    payload: ObservationPayload,
    causal_parent_id: Option<String>,
    correlation_id: Option<String>,
    redacted: bool,
    truncated: bool,
    private_reasoning: bool,
}

fn private(payload: ObservationPayload, kind: SemanticKind, phase: LifecyclePhase) -> Decoded {
    Decoded {
        kind,
        phase,
        completeness: if phase == LifecyclePhase::Terminal {
            Completeness::Complete
        } else {
            Completeness::Partial
        },
        effect_certainty: EffectCertainty::None,
        visibility: ObservationVisibility::SessionOwnerPrivate,
        payload,
        causal_parent_id: None,
        correlation_id: None,
        redacted: false,
        truncated: false,
        private_reasoning: false,
    }
}

fn public_runtime(
    payload: ObservationPayload,
    kind: SemanticKind,
    phase: LifecyclePhase,
    completeness: Completeness,
) -> Decoded {
    Decoded {
        kind,
        phase,
        completeness,
        effect_certainty: EffectCertainty::None,
        visibility: ObservationVisibility::TeamPublic,
        payload,
        causal_parent_id: None,
        correlation_id: None,
        redacted: false,
        truncated: false,
        private_reasoning: false,
    }
}

fn decode_codex(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    let row_type = string(&event.raw, "/type")?;
    let payload_type = event
        .raw
        .pointer("/payload/type")
        .and_then(|value| value.as_str());
    let item_type = event
        .raw
        .pointer("/item/type")
        .and_then(|value| value.as_str());
    match (row_type, payload_type.or(item_type)) {
        ("event_msg", Some("agent_reasoning" | "reasoning")) => Ok(Some(reasoning_drop())),
        ("event_msg", Some("agent_message")) => {
            Ok(Some(authored(string(&event.raw, "/payload/message")?)))
        }
        ("event_msg", Some("task_complete")) => Ok(Some(turn("completed"))),
        ("event_msg", Some("token_count")) => Ok(Some(codex_usage(&event.raw))),
        ("response_item", Some("reasoning")) => Ok(Some(reasoning_drop())),
        ("response_item", Some("message"))
            if event
                .raw
                .pointer("/payload/role")
                .and_then(|value| value.as_str())
                == Some("assistant") =>
        {
            Ok(text_from_parts(
                event
                    .raw
                    .pointer("/payload/content")
                    .unwrap_or(&serde_json::Value::Null),
            )
            .map(|text| authored(&text)))
        }
        ("response_item", Some("function_call" | "custom_tool_call")) => Ok(Some(tool(
            if payload_type == Some("custom_tool_call") {
                SemanticKind::ToolCallStarted
            } else {
                SemanticKind::ToolCallRequested
            },
            if payload_type == Some("custom_tool_call") {
                LifecyclePhase::Started
            } else {
                LifecyclePhase::Requested
            },
            event.raw.pointer("/payload/name").and_then(|v| v.as_str()),
            event
                .raw
                .pointer("/payload/call_id")
                .and_then(|v| v.as_str()),
        ))),
        ("response_item", Some("function_call_output" | "custom_tool_call_output")) => {
            Ok(Some(tool(
                SemanticKind::ToolCallCompleted,
                LifecyclePhase::Terminal,
                Some("tool"),
                event
                    .raw
                    .pointer("/payload/call_id")
                    .and_then(|v| v.as_str()),
            )))
        }
        ("turn/completed", _) | ("turn_completed", _) => Ok(Some(turn("completed"))),
        ("turn/failed", _) | ("turn_failed", _) => Ok(Some(turn("failed"))),
        ("turn/cancelled", _) | ("turn_cancelled", _) => Ok(Some(turn("cancelled"))),
        ("transport_interrupted", _) => Ok(Some(transport("provider_transport_interrupted"))),
        _ => Ok(None),
    }
}

fn codex_usage(raw: &serde_json::Value) -> Decoded {
    let usage = raw.pointer("/payload/info/total_token_usage");
    private(
        ObservationPayload::Usage {
            input_tokens: usage
                .and_then(|value| value.get("input_tokens"))
                .and_then(|value| value.as_u64()),
            output_tokens: usage
                .and_then(|value| value.get("output_tokens"))
                .and_then(|value| value.as_u64()),
            total_tokens: usage
                .and_then(|value| value.get("total_tokens"))
                .and_then(|value| value.as_u64()),
        },
        SemanticKind::UsageReported,
        LifecyclePhase::Progress,
    )
}

fn native_turn_id(raw: &serde_json::Value) -> Option<String> {
    raw.pointer("/payload/turn_id")
        .or_else(|| raw.pointer("/turn_id"))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

fn decode_claude(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    let row_type = string(&event.raw, "/type")?;
    match row_type {
        "assistant" => {
            let content = event
                .raw
                .pointer("/message/content")
                .ok_or(DecodeError::Malformed("assistant content"))?;
            decode_claude_content(content)
        }
        "stream_event" => match event.raw.get("event").and_then(|v| v.as_str()) {
            Some("text_delta") => Ok(event
                .raw
                .pointer("/delta/text")
                .and_then(|v| v.as_str())
                .map(authored)),
            Some("message_stop") => Ok(Some(turn("completed"))),
            Some("message_start" | "content_block_start" | "content_block_stop") => Ok(None),
            _ => Ok(None),
        },
        "result" => Ok(Some(usage(&event.raw))),
        "transport_interrupted" => Ok(Some(transport("provider_transport_interrupted"))),
        "cancelled" => Ok(Some(turn("cancelled"))),
        _ => Ok(None),
    }
}

fn decode_claude_content(content: &serde_json::Value) -> Result<Option<Decoded>, DecodeError> {
    if let Some(text) = content.as_str() {
        return Ok(Some(authored(text)));
    }
    let parts = content
        .as_array()
        .ok_or(DecodeError::Malformed("assistant content array"))?;
    for part in parts {
        match part.get("type").and_then(|v| v.as_str()) {
            Some("thinking") => return Ok(Some(reasoning_drop())),
            Some("text") => return Ok(part.get("text").and_then(|v| v.as_str()).map(authored)),
            Some("tool_use") => {
                return Ok(Some(tool(
                    SemanticKind::ToolCallRequested,
                    LifecyclePhase::Requested,
                    part.get("name").and_then(|v| v.as_str()),
                    part.get("id").and_then(|v| v.as_str()),
                )))
            }
            Some("tool_result") => {
                return Ok(Some(tool(
                    SemanticKind::ToolCallCompleted,
                    LifecyclePhase::Terminal,
                    Some("tool"),
                    part.get("tool_use_id").and_then(|v| v.as_str()),
                )))
            }
            _ => {}
        }
    }
    Ok(None)
}

fn decode_kimi(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    let row_type = string(&event.raw, "/type")?;
    match row_type {
        "context.append_loop_event" => {
            let event_type = string(&event.raw, "/event/type")?;
            match event_type {
                "content.part" => match event
                    .raw
                    .pointer("/event/part/type")
                    .and_then(|v| v.as_str())
                {
                    Some("think" | "thinking") => Ok(Some(reasoning_drop())),
                    Some("text") => Ok(event
                        .raw
                        .pointer("/event/part/text")
                        .and_then(|v| v.as_str())
                        .map(authored)),
                    _ => Ok(None),
                },
                "tool.call" => Ok(Some(tool(
                    SemanticKind::ToolCallRequested,
                    LifecyclePhase::Requested,
                    event.raw.pointer("/event/name").and_then(|v| v.as_str()),
                    event.raw.pointer("/event/id").and_then(|v| v.as_str()),
                ))),
                "tool.result" => Ok(Some(tool(
                    SemanticKind::ToolCallCompleted,
                    LifecyclePhase::Terminal,
                    Some("tool"),
                    event.raw.pointer("/event/id").and_then(|v| v.as_str()),
                ))),
                "artifact.created" => Ok(Some(artifact(&event.raw, "/event"))),
                _ => Ok(None),
            }
        }
        "turn.end" | "turn_end" => Ok(Some(turn("completed"))),
        "turn.cancelled" => Ok(Some(turn("cancelled"))),
        "transport_interrupted" => Ok(Some(transport("provider_transport_interrupted"))),
        _ => Ok(None),
    }
}

fn decode_pi(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    let row_type = string(&event.raw, "/type")?;
    match row_type {
        "message_update" => {
            let content = event
                .raw
                .pointer("/message/content")
                .or_else(|| event.raw.get("content"));
            Ok(content
                .and_then(text_from_parts)
                .map(|text| authored(&text)))
        }
        "tool_execution_start" => Ok(Some(tool(
            SemanticKind::ToolCallStarted,
            LifecyclePhase::Started,
            event.raw.get("toolName").and_then(|v| v.as_str()),
            event.raw.get("toolCallId").and_then(|v| v.as_str()),
        ))),
        "tool_execution_end" => Ok(Some(tool(
            if event.raw.get("isError").and_then(|v| v.as_bool()) == Some(true) {
                SemanticKind::ToolCallFailed
            } else {
                SemanticKind::ToolCallCompleted
            },
            LifecyclePhase::Terminal,
            event.raw.get("toolName").and_then(|v| v.as_str()),
            event.raw.get("toolCallId").and_then(|v| v.as_str()),
        ))),
        "turn_end" | "agent_settled" => Ok(Some(turn("completed"))),
        "interaction_required" => Ok(Some(interaction(&event.raw))),
        "artifact_created" => Ok(Some(artifact(&event.raw, ""))),
        "transport_interrupted" => Ok(Some(transport("provider_transport_interrupted"))),
        _ => Ok(None),
    }
}

fn authored(text: &str) -> Decoded {
    let (text, truncated) = bounded(text, MAX_TEXT_CHARS);
    let mut decoded = private(
        ObservationPayload::AuthoredResponse { text },
        SemanticKind::AuthoredResponse,
        LifecyclePhase::Progress,
    );
    decoded.truncated = truncated;
    decoded
}

fn reasoning_drop() -> Decoded {
    let mut decoded = private(
        ObservationPayload::ReasoningSummary {
            summary: "provider-private reasoning omitted".into(),
        },
        SemanticKind::ReasoningSummary,
        LifecyclePhase::Progress,
    );
    decoded.private_reasoning = true;
    decoded.redacted = true;
    decoded
}

fn tool(
    kind: SemanticKind,
    phase: LifecyclePhase,
    name: Option<&str>,
    call_id: Option<&str>,
) -> Decoded {
    private(
        ObservationPayload::Tool {
            tool_name: safe_label(name.unwrap_or("tool")),
            call_id: call_id.map(safe_label),
            // Raw tool input/output is intentionally never projected.
            display_detail: Some("provider recorded a bounded tool lifecycle event".into()),
        },
        kind,
        phase,
    )
}

fn artifact(raw: &serde_json::Value, prefix: &str) -> Decoded {
    let pointer = |field: &str| {
        let path = if prefix.is_empty() {
            format!("/{field}")
        } else {
            format!("{prefix}/{field}")
        };
        raw.pointer(&path).and_then(|v| v.as_str())
    };
    private(
        ObservationPayload::Artifact {
            display_name: safe_label(pointer("name").unwrap_or("artifact")),
            media_type: pointer("mediaType").map(safe_label),
            content_digest: pointer("digest")
                .filter(|value| value.starts_with("sha256:"))
                .map(safe_label),
        },
        SemanticKind::ArtifactCreated,
        LifecyclePhase::Terminal,
    )
}

fn usage(raw: &serde_json::Value) -> Decoded {
    private(
        ObservationPayload::Usage {
            input_tokens: raw.pointer("/usage/input_tokens").and_then(|v| v.as_u64()),
            output_tokens: raw.pointer("/usage/output_tokens").and_then(|v| v.as_u64()),
            total_tokens: raw.pointer("/usage/total_tokens").and_then(|v| v.as_u64()),
        },
        SemanticKind::UsageReported,
        LifecyclePhase::Terminal,
    )
}

fn interaction(raw: &serde_json::Value) -> Decoded {
    let prompt = raw
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("Provider interaction is required");
    let (prompt, truncated) = bounded(prompt, MAX_DISPLAY_DETAIL_CHARS);
    let mut decoded = public_runtime(
        ObservationPayload::Interaction {
            reason_code: safe_label(
                raw.get("reasonCode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("provider_interaction_required"),
            ),
            prompt,
        },
        SemanticKind::InteractionRequired,
        LifecyclePhase::Requested,
        Completeness::Incomplete,
    );
    decoded.truncated = truncated;
    decoded
}

fn transport(reason: &str) -> Decoded {
    public_runtime(
        ObservationPayload::Transport {
            reason_code: reason.into(),
        },
        SemanticKind::TransportInterrupted,
        LifecyclePhase::Recovery,
        Completeness::Incomplete,
    )
}

fn turn(outcome: &str) -> Decoded {
    let (kind, completeness) = match outcome {
        "completed" => (SemanticKind::TurnCompleted, Completeness::Complete),
        "cancelled" => (SemanticKind::TurnCancelled, Completeness::Complete),
        _ => (SemanticKind::TurnFailed, Completeness::Incomplete),
    };
    private(
        ObservationPayload::Turn {
            outcome: outcome.into(),
            display_summary: None,
        },
        kind,
        LifecyclePhase::Terminal,
    )
    .with_completeness(completeness)
}

impl Decoded {
    fn with_completeness(mut self, completeness: Completeness) -> Self {
        self.completeness = completeness;
        self
    }
}

fn string<'a>(raw: &'a serde_json::Value, pointer: &str) -> Result<&'a str, DecodeError> {
    raw.pointer(pointer)
        .and_then(|value| value.as_str())
        .ok_or(DecodeError::Malformed("required event type"))
}

fn text_from_parts(value: &serde_json::Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.into());
    }
    let text = value
        .as_array()?
        .iter()
        .filter_map(|part| {
            matches!(
                part.get("type").and_then(|v| v.as_str()),
                Some("text" | "output_text")
            )
            .then(|| part.get("text").and_then(|v| v.as_str()))
            .flatten()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn safe_label(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(160)
        .collect()
}

fn bounded(value: &str, max: usize) -> (String, bool) {
    let mut chars = value.chars();
    let bounded = chars.by_ref().take(max).collect::<String>();
    (bounded, chars.next().is_some())
}

fn fingerprint(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).expect("serde_json::Value is serializable");
    format!("sha256:{:x}", Sha256::digest(bytes))
}

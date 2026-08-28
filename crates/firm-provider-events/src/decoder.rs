use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    Completeness, EffectCertainty, FragmentPayload, FragmentVisibility, LifecyclePhase,
    ProviderEventFragment, ProviderKind, ProviderNativeEventRecord, SemanticKind,
    PROVIDER_EVENT_ADAPTER_VERSION, PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION,
};

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
                SemanticKind::Reasoning,
                SemanticKind::AssistantResponse,
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
            &["session_bound", "message", "stream_event", "result"][..],
            false,
            &[
                SemanticKind::SessionMetadata,
                SemanticKind::Reasoning,
                SemanticKind::AssistantResponse,
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
                SemanticKind::Reasoning,
                SemanticKind::AssistantResponse,
                SemanticKind::ToolCallRequested,
                SemanticKind::ToolCallCompleted,
                SemanticKind::ArtifactCreated,
                SemanticKind::TransportInterrupted,
                SemanticKind::TurnCompleted,
                SemanticKind::TurnCancelled,
            ][..],
        ),
        ProviderKind::Pi => (
            &["rpc_event", "message"][..],
            false,
            &[
                SemanticKind::Reasoning,
                SemanticKind::AssistantResponse,
                SemanticKind::ToolCallStarted,
                SemanticKind::ToolCallCompleted,
                SemanticKind::ToolCallFailed,
                SemanticKind::ArtifactCreated,
                SemanticKind::InteractionRequired,
                SemanticKind::TransportInterrupted,
                SemanticKind::TurnCompleted,
                SemanticKind::TurnFailed,
            ][..],
        ),
        ProviderKind::DeepseekHarness => (
            &[
                "assistant/message",
                "tool/call",
                "tool/result",
                "turn/start",
                "turn/end",
            ][..],
            true,
            &[
                SemanticKind::Reasoning,
                SemanticKind::AssistantResponse,
                SemanticKind::ToolCallStarted,
                SemanticKind::ToolCallCompleted,
                SemanticKind::UsageReported,
                SemanticKind::TurnCompleted,
                SemanticKind::TurnFailed,
                SemanticKind::TurnCancelled,
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
    let mut supported_semantic_kinds = supported.to_vec();
    supported_semantic_kinds.push(SemanticKind::MalformedOrIncomplete);
    supported_semantic_kinds.push(SemanticKind::UnclassifiedNative);
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
        supported_semantic_kinds,
        redaction_policy: "preserve_exact_native_event_without_semantic_filtering".into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeContext {
    pub provider: ProviderKind,
    pub native_source_ref: String,
    pub agent_member_id: String,
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
    Record(Box<ProviderNativeEventRecord>),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DecodeError {
    #[error("decode context is incomplete")]
    InvalidContext,
    #[error("native event has no ordering position")]
    MissingOrdering,
    #[error("native event is malformed: {0}")]
    Malformed(&'static str),
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
            Ok(DecodeOutcome::Record(Box::new(malformed_record(
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
    let decoded = match decode_common(&event) {
        Ok(Some(common)) => Ok(vec![common]),
        Ok(None) => match context.provider {
            ProviderKind::Codex => decode_codex(&event).map(one_or_empty),
            ProviderKind::Claude => decode_claude(&event),
            ProviderKind::Kimi => decode_kimi(&event).map(one_or_empty),
            ProviderKind::Pi => decode_pi(&event).map(one_or_empty),
            ProviderKind::DeepseekHarness => decode_deepseek_harness(&event).map(one_or_empty),
        },
        Err(error) => Err(error),
    };
    // A complete, valid JSON row is always observable even when a provider
    // version emits an incomplete recognized shape. Semantic classification
    // failure is an operator diagnostic, never a reason to panic the live
    // stream or make the reopened native Session unreadable.
    let decoded = match decoded {
        Ok(decoded) => decoded,
        Err(DecodeError::Malformed(reason)) => vec![malformed(reason)],
        Err(error) => return Err(error),
    };
    let decoded = if decoded.is_empty() {
        vec![native(&event.raw)]
    } else {
        decoded
    };
    let source_fingerprint = fingerprint(&event.raw);
    let native_identity = event.native_event_id.clone().unwrap_or_else(|| {
        format!(
            "fingerprint:{}:{}",
            event.ordering_position,
            &source_fingerprint["sha256:".len()..]
        )
    });
    let record_id = format!(
        "{}:{}:{}",
        context.provider.as_str(),
        context.agent_session_id,
        native_identity
    );
    let causal_parent_id = decoded
        .iter()
        .find_map(|fragment| fragment.causal_parent_id.clone());
    let correlation_id = decoded
        .iter()
        .find_map(|fragment| fragment.correlation_id.clone());
    let fragments = decoded
        .into_iter()
        .enumerate()
        .map(|(index, decoded)| ProviderEventFragment {
            fragment_id: format!("{record_id}:fragment-{index}"),
            fragment_index: index as u32,
            semantic_kind: decoded.kind,
            lifecycle_phase: decoded.phase,
            completeness: decoded.completeness,
            effect_certainty: decoded.effect_certainty,
            visibility: decoded.visibility,
            payload: decoded.payload,
        })
        .collect();
    let record = ProviderNativeEventRecord {
        schema_version: PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION.into(),
        record_id,
        provider: context.provider,
        adapter_version: PROVIDER_EVENT_ADAPTER_VERSION.into(),
        native_source_ref: context.native_source_ref.clone(),
        agent_member_id: context.agent_member_id.clone(),
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
        causal_parent_id,
        correlation_id,
        runtime_command_id: context.runtime_command_id.clone(),
        occurred_at: event.occurred_at,
        observed_at: context.observed_at.clone(),
        native_event: event.raw,
        source_content_fingerprint: source_fingerprint,
        fragments,
    };
    record
        .validate()
        .map_err(|_| DecodeError::InvalidSemantic)?;
    Ok(DecodeOutcome::Record(Box::new(record)))
}

fn one_or_empty(decoded: Option<Decoded>) -> Vec<Decoded> {
    decoded.into_iter().collect()
}

fn malformed(reason: &'static str) -> Decoded {
    private(
        FragmentPayload::Malformed {
            reason_code: format!("recognized_native_shape_incomplete:{reason}"),
        },
        SemanticKind::MalformedOrIncomplete,
        LifecyclePhase::Recovery,
    )
}

fn decode_common(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    let row_type = event.raw.get("type").and_then(Value::as_str).unwrap_or("");
    Ok(match row_type {
        "runtime_started" => Some(public_runtime(
            FragmentPayload::Runtime {
                state: "started".into(),
            },
            SemanticKind::RuntimeStarted,
            LifecyclePhase::Started,
            Completeness::Partial,
        )),
        "runtime_ready" => Some(public_runtime(
            FragmentPayload::Runtime {
                state: "ready".into(),
            },
            SemanticKind::RuntimeReady,
            LifecyclePhase::Progress,
            Completeness::Complete,
        )),
        "runtime_stopped" => Some(public_runtime(
            FragmentPayload::Runtime {
                state: "stopped".into(),
            },
            SemanticKind::RuntimeStopped,
            LifecyclePhase::Terminal,
            Completeness::Complete,
        )),
        "interaction_required" => Some(interaction(&event.raw)),
        "interaction_resolved" => Some(public_runtime(
            FragmentPayload::Interaction {
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
            visibility: FragmentVisibility::TeamPublic,
            payload: FragmentPayload::Recovery {
                reason_code: "native_effect_unknown".into(),
            },
            causal_parent_id: None,
            correlation_id: None,
        }),
        _ => None,
    })
}

fn malformed_record(
    context: &DecodeContext,
    native_event_id: Option<String>,
    provider_turn_id: Option<String>,
    ordering_position: u64,
    occurred_at: Option<String>,
    line: &str,
) -> ProviderNativeEventRecord {
    let source_fingerprint = format!("sha256:{:x}", Sha256::digest(line.as_bytes()));
    let native_identity = native_event_id.clone().unwrap_or_else(|| {
        format!(
            "malformed:{ordering_position}:{}",
            &source_fingerprint["sha256:".len()..]
        )
    });
    ProviderNativeEventRecord {
        schema_version: PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION.into(),
        record_id: format!(
            "{}:{}:{native_identity}",
            context.provider.as_str(),
            context.agent_session_id
        ),
        provider: context.provider,
        adapter_version: PROVIDER_EVENT_ADAPTER_VERSION.into(),
        native_source_ref: context.native_source_ref.clone(),
        agent_member_id: context.agent_member_id.clone(),
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
        native_event: Value::String(line.to_owned()),
        source_content_fingerprint: source_fingerprint,
        fragments: vec![ProviderEventFragment {
            fragment_id: format!(
                "{}:{}:{native_identity}:fragment-0",
                context.provider.as_str(),
                context.agent_session_id
            ),
            fragment_index: 0,
            semantic_kind: SemanticKind::MalformedOrIncomplete,
            lifecycle_phase: LifecyclePhase::Recovery,
            completeness: Completeness::Incomplete,
            effect_certainty: EffectCertainty::None,
            visibility: FragmentVisibility::OperatorOnly,
            payload: FragmentPayload::Malformed {
                reason_code: "native_json_malformed".into(),
            },
        }],
    }
}

fn validate_context(context: &DecodeContext, event: &NativeEvent) -> Result<(), DecodeError> {
    if !context.native_source_ref.starts_with("provider-source:")
        || context.agent_member_id.trim().is_empty()
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

struct Decoded {
    kind: SemanticKind,
    phase: LifecyclePhase,
    completeness: Completeness,
    effect_certainty: EffectCertainty,
    visibility: FragmentVisibility,
    payload: FragmentPayload,
    causal_parent_id: Option<String>,
    correlation_id: Option<String>,
}

fn private(payload: FragmentPayload, kind: SemanticKind, phase: LifecyclePhase) -> Decoded {
    Decoded {
        kind,
        phase,
        completeness: if phase == LifecyclePhase::Terminal {
            Completeness::Complete
        } else {
            Completeness::Partial
        },
        effect_certainty: EffectCertainty::None,
        visibility: FragmentVisibility::TeamSession,
        payload,
        causal_parent_id: None,
        correlation_id: None,
    }
}

fn native(raw: &Value) -> Decoded {
    private(
        FragmentPayload::Native {
            event_type: raw.get("type").and_then(Value::as_str).map(str::to_owned),
        },
        SemanticKind::UnclassifiedNative,
        LifecyclePhase::Progress,
    )
}

fn public_runtime(
    payload: FragmentPayload,
    kind: SemanticKind,
    phase: LifecyclePhase,
    completeness: Completeness,
) -> Decoded {
    Decoded {
        kind,
        phase,
        completeness,
        effect_certainty: EffectCertainty::None,
        visibility: FragmentVisibility::TeamPublic,
        payload,
        causal_parent_id: None,
        correlation_id: None,
    }
}

fn decode_codex(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    if let Some(method) = event.raw.get("method").and_then(Value::as_str) {
        let params = event.raw.get("params").unwrap_or(&event.raw);
        return Ok(match method {
            "item/agentMessage/delta" => params.get("delta").and_then(Value::as_str).map(authored),
            "item/reasoning/summaryTextDelta" => {
                params.get("delta").and_then(Value::as_str).map(reasoning)
            }
            "item/started" => Some(tool(
                SemanticKind::ToolCallStarted,
                LifecyclePhase::Started,
                params
                    .pointer("/item/type")
                    .or_else(|| params.pointer("/item/name"))
                    .and_then(Value::as_str),
                params
                    .pointer("/item/id")
                    .or_else(|| params.pointer("/item/callId"))
                    .and_then(Value::as_str),
            )),
            "item/completed" => Some(tool(
                SemanticKind::ToolCallCompleted,
                LifecyclePhase::Terminal,
                params
                    .pointer("/item/type")
                    .or_else(|| params.pointer("/item/name"))
                    .and_then(Value::as_str),
                params
                    .pointer("/item/id")
                    .or_else(|| params.pointer("/item/callId"))
                    .and_then(Value::as_str),
            )),
            "turn/completed" => Some(turn("completed")),
            _ => None,
        });
    }
    let row_type = event.raw.get("type").and_then(Value::as_str).unwrap_or("");
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
        FragmentPayload::Usage {
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
    if let Some(turn) = raw.pointer("/data/turn").and_then(|value| value.as_u64()) {
        return Some(turn.to_string());
    }
    raw.pointer("/payload/turn_id")
        .or_else(|| raw.pointer("/turn_id"))
        .or_else(|| raw.pointer("/turnId"))
        .or_else(|| raw.pointer("/event/turn_id"))
        .or_else(|| raw.pointer("/event/turnId"))
        .and_then(|value| value.as_str())
        .map(str::to_owned)
}

fn decode_claude(event: &NativeEvent) -> Result<Vec<Decoded>, DecodeError> {
    if let Some(name) = event
        .raw
        .get("event")
        .and_then(Value::as_str)
        .filter(|_| event.raw.get("type").is_none())
    {
        let data = event.raw.get("data").unwrap_or(&Value::Null);
        return match name {
            "session_bound" => Ok(vec![private(
                FragmentPayload::SessionMetadata {
                    native_session_id: data
                        .get("sessionId")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                },
                SemanticKind::SessionMetadata,
                LifecyclePhase::Started,
            )]),
            "assistant_message" => decode_claude_content(
                data.pointer("/message/content")
                    .or_else(|| data.get("content"))
                    .unwrap_or(data),
            ),
            "turn_complete" => Ok(vec![turn(
                if data.get("isError").and_then(Value::as_bool) == Some(true) {
                    "failed"
                } else {
                    "completed"
                },
            )]),
            "provider_error" => Ok(vec![transport("provider_error")]),
            _ => Ok(vec![]),
        };
    }
    let row_type = event.raw.get("type").and_then(Value::as_str).unwrap_or("");
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
                .map(authored)
                .into_iter()
                .collect()),
            Some("message_stop") => Ok(vec![turn("completed")]),
            Some("message_start" | "content_block_start" | "content_block_stop") => Ok(vec![]),
            _ => Ok(vec![]),
        },
        "result" => Ok(vec![usage(&event.raw)]),
        "transport_interrupted" => Ok(vec![transport("provider_transport_interrupted")]),
        "cancelled" => Ok(vec![turn("cancelled")]),
        _ => Ok(vec![]),
    }
}

fn decode_claude_content(content: &serde_json::Value) -> Result<Vec<Decoded>, DecodeError> {
    if let Some(text) = content.as_str() {
        return Ok(vec![authored(text)]);
    }
    let parts = content
        .as_array()
        .ok_or(DecodeError::Malformed("assistant content array"))?;
    let mut decoded = Vec::new();
    for part in parts {
        match part.get("type").and_then(|v| v.as_str()) {
            Some("thinking" | "reasoning") => {
                let text = part
                    .get("thinking")
                    .or_else(|| part.get("text"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("provider-native reasoning event");
                decoded.push(reasoning(text));
            }
            Some("text") => {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    decoded.push(authored(text));
                }
            }
            Some("tool_use") => {
                decoded.push(tool(
                    SemanticKind::ToolCallRequested,
                    LifecyclePhase::Requested,
                    part.get("name").and_then(|v| v.as_str()),
                    part.get("id").and_then(|v| v.as_str()),
                ));
            }
            Some("tool_result") => {
                decoded.push(tool(
                    SemanticKind::ToolCallCompleted,
                    LifecyclePhase::Terminal,
                    Some("tool"),
                    part.get("tool_use_id").and_then(|v| v.as_str()),
                ));
            }
            _ => {}
        }
    }
    Ok(decoded)
}

fn decode_kimi(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    if let Some(kind) = event.raw.get("sessionUpdate").and_then(Value::as_str) {
        return Ok(match kind {
            "agent_thought_chunk" => event
                .raw
                .pointer("/content/text")
                .and_then(Value::as_str)
                .map(reasoning),
            "agent_message_chunk" => event
                .raw
                .pointer("/content/text")
                .and_then(Value::as_str)
                .map(authored),
            "tool_call" => Some(tool(
                SemanticKind::ToolCallStarted,
                LifecyclePhase::Started,
                event
                    .raw
                    .get("title")
                    .or_else(|| event.raw.get("name"))
                    .and_then(Value::as_str),
                event
                    .raw
                    .get("toolCallId")
                    .or_else(|| event.raw.get("id"))
                    .and_then(Value::as_str),
            )),
            "tool_call_update" => Some(tool(
                if matches!(
                    event.raw.get("status").and_then(Value::as_str),
                    Some("failed" | "error" | "cancelled" | "canceled")
                ) {
                    SemanticKind::ToolCallFailed
                } else {
                    SemanticKind::ToolCallCompleted
                },
                LifecyclePhase::Terminal,
                event.raw.get("title").and_then(Value::as_str),
                event
                    .raw
                    .get("toolCallId")
                    .or_else(|| event.raw.get("id"))
                    .and_then(Value::as_str),
            )),
            _ => None,
        });
    }
    if event.raw.get("method").and_then(Value::as_str) == Some("session/request_permission") {
        return Ok(Some(interaction(&event.raw)));
    }
    let row_type = event.raw.get("type").and_then(Value::as_str).unwrap_or("");
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
                    event
                        .raw
                        .pointer("/event/toolCallId")
                        .or_else(|| event.raw.pointer("/event/id"))
                        .and_then(|v| v.as_str()),
                ))),
                "tool.result" => Ok(Some(tool(
                    SemanticKind::ToolCallCompleted,
                    LifecyclePhase::Terminal,
                    Some("tool"),
                    event
                        .raw
                        .pointer("/event/toolCallId")
                        .or_else(|| event.raw.pointer("/event/id"))
                        .and_then(|v| v.as_str()),
                ))),
                "step.end"
                    if event
                        .raw
                        .pointer("/event/finishReason")
                        .and_then(|v| v.as_str())
                        == Some("end_turn") =>
                {
                    Ok(Some(turn("completed")))
                }
                "artifact.created" => Ok(Some(artifact(&event.raw, "/event"))),
                _ => Ok(None),
            }
        }
        "turn.end" | "turn_end" => Ok(Some(turn("completed"))),
        "turn.cancel" | "turn.cancelled" => Ok(Some(turn("cancelled"))),
        "transport_interrupted" => Ok(Some(transport("provider_transport_interrupted"))),
        _ => Ok(None),
    }
}

fn decode_pi(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    let row_type = event.raw.get("type").and_then(Value::as_str).unwrap_or("");
    match row_type {
        // Pi's provider-native JSONL persists completed messages rather than
        // replaying the transient RPC `message_update` family. This semantic
        // classification is additive: the exact unfiltered native row is
        // retained on every observation for the Team-scoped Session view.
        "message" => decode_pi_persisted_message(&event.raw),
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

fn decode_pi_persisted_message(raw: &Value) -> Result<Option<Decoded>, DecodeError> {
    if raw.pointer("/message/role").and_then(Value::as_str) != Some("assistant") {
        return Ok(None);
    }
    let content = raw
        .pointer("/message/content")
        .ok_or(DecodeError::Malformed("Pi assistant content"))?;
    if let Some(text) = text_from_parts(content) {
        return Ok(Some(authored(&text)));
    }
    let stop_reason = raw.pointer("/message/stopReason").and_then(Value::as_str);
    if stop_reason == Some("error") {
        return Ok(Some(turn("failed")));
    }
    if content.as_array().is_some_and(|parts| {
        parts.iter().any(|part| {
            matches!(
                part.get("type").and_then(Value::as_str),
                Some("thinking" | "reasoning")
            )
        })
    }) {
        return Ok(Some(reasoning_drop()));
    }
    Ok(None)
}

fn decode_deepseek_harness(event: &NativeEvent) -> Result<Option<Decoded>, DecodeError> {
    if let Some(name) = event.raw.get("event").and_then(Value::as_str) {
        let data = event.raw.get("data").unwrap_or(&Value::Null);
        return Ok(match name {
            "provider_activity" => match data.get("kind").and_then(Value::as_str) {
                Some("thinking") => data.get("summary").and_then(Value::as_str).map(reasoning),
                Some("response_streaming") => {
                    data.get("summary").and_then(Value::as_str).map(authored)
                }
                Some("tool_started") => Some(tool(
                    SemanticKind::ToolCallStarted,
                    LifecyclePhase::Started,
                    data.get("toolName").and_then(Value::as_str),
                    data.get("callId").and_then(Value::as_str),
                )),
                Some("tool_completed") => Some(tool(
                    SemanticKind::ToolCallCompleted,
                    LifecyclePhase::Terminal,
                    data.get("toolName").and_then(Value::as_str),
                    data.get("callId").and_then(Value::as_str),
                )),
                Some("tool_failed") => Some(tool(
                    SemanticKind::ToolCallFailed,
                    LifecyclePhase::Terminal,
                    data.get("toolName").and_then(Value::as_str),
                    data.get("callId").and_then(Value::as_str),
                )),
                _ => None,
            },
            "assistant_message" => text_from_parts(
                data.pointer("/message/content")
                    .or_else(|| data.get("content"))
                    .unwrap_or(data),
            )
            .map(|text| authored(&text)),
            "turn_complete" => Some(turn("completed")),
            _ => None,
        });
    }
    let row_type = event.raw.get("type").and_then(Value::as_str).unwrap_or("");
    match row_type {
        "assistant/message" => {
            let content = event
                .raw
                .pointer("/data/message/content")
                .and_then(Value::as_array)
                .ok_or(DecodeError::Malformed("DeepSeek assistant content"))?;
            let mut saw_reasoning = false;
            let mut text_parts = Vec::new();
            for part in content {
                match part.get("type").and_then(Value::as_str) {
                    Some("reasoning") => {
                        saw_reasoning = true;
                        continue;
                    }
                    Some("text") => {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            text_parts.push(text);
                        }
                    }
                    // The official store also writes an exact tool/call row;
                    // projecting the embedded copy would duplicate it.
                    Some("tool-call") => {}
                    _ => {}
                }
            }
            if text_parts.is_empty() {
                Ok(saw_reasoning.then(reasoning_drop))
            } else {
                Ok(Some(authored(&text_parts.join(""))))
            }
        }
        "tool/call" => Ok(Some(tool(
            SemanticKind::ToolCallStarted,
            LifecyclePhase::Started,
            event.raw.pointer("/data/name").and_then(Value::as_str),
            event.raw.pointer("/data/callId").and_then(Value::as_str),
        ))),
        "tool/result" => Ok(Some(tool(
            SemanticKind::ToolCallCompleted,
            LifecyclePhase::Terminal,
            Some("tool"),
            event
                .raw
                .pointer("/data/message/source/callId")
                .and_then(Value::as_str),
        ))),
        "turn/end" => {
            let kind = event
                .raw
                .pointer("/data/reason/kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            Ok(Some(turn(match kind {
                "completed" => "completed",
                "cancelled" | "canceled" => "cancelled",
                _ => "failed",
            })))
        }
        "assistant/chunk"
            if event
                .raw
                .pointer("/data/chunk/type")
                .and_then(Value::as_str)
                == Some("usage") =>
        {
            let usage = event.raw.pointer("/data/chunk/usage");
            Ok(Some(private(
                FragmentPayload::Usage {
                    input_tokens: usage
                        .and_then(|value| value.get("inputTokens"))
                        .and_then(Value::as_u64),
                    output_tokens: usage
                        .and_then(|value| value.get("outputTokens"))
                        .and_then(Value::as_u64),
                    total_tokens: None,
                },
                SemanticKind::UsageReported,
                LifecyclePhase::Progress,
            )))
        }
        // Other chunk rows contain private reasoning deltas or duplicate the
        // final assistant/message. They are deliberately not projected.
        "assistant/chunk" | "turn/start" | "agent/inbox/spliced" => Ok(None),
        _ => Ok(None),
    }
}

fn authored(text: &str) -> Decoded {
    private(
        FragmentPayload::AssistantResponse {
            text: text.to_owned(),
        },
        SemanticKind::AssistantResponse,
        LifecyclePhase::Progress,
    )
}

fn reasoning_drop() -> Decoded {
    reasoning("provider-native reasoning event")
}

fn reasoning(text: &str) -> Decoded {
    private(
        FragmentPayload::Reasoning {
            text: text.to_owned(),
        },
        SemanticKind::Reasoning,
        LifecyclePhase::Progress,
    )
}

fn tool(
    kind: SemanticKind,
    phase: LifecyclePhase,
    name: Option<&str>,
    call_id: Option<&str>,
) -> Decoded {
    private(
        FragmentPayload::Tool {
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
        FragmentPayload::Artifact {
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
        FragmentPayload::Usage {
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
    public_runtime(
        FragmentPayload::Interaction {
            reason_code: safe_label(
                raw.get("reasonCode")
                    .and_then(|v| v.as_str())
                    .unwrap_or("provider_interaction_required"),
            ),
            prompt: prompt.to_owned(),
        },
        SemanticKind::InteractionRequired,
        LifecyclePhase::Requested,
        Completeness::Incomplete,
    )
}

fn transport(reason: &str) -> Decoded {
    public_runtime(
        FragmentPayload::Transport {
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
        FragmentPayload::Turn {
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

fn fingerprint(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).expect("serde_json::Value is serializable");
    format!("sha256:{:x}", Sha256::digest(bytes))
}

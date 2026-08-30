use std::collections::BTreeMap;

use serde_json::Value;
use thiserror::Error;

use crate::{
    ContentAvailability, ContentUnavailableReason, NativeClassificationReason,
    PersistedCompleteness, PersistedContentReference, PersistedEventFragment,
    PersistedFragmentPayload, PersistedNativeRow, ProviderKind, ProviderNativeEventRecord,
    SessionLifecyclePhase, SessionSemanticKind, ToolCallOutcome, ToolOperationCategory,
    PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION, PROVIDER_NATIVE_EVENT_RECORD_V3_SCHEMA_VERSION,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedProjectionContext {
    pub native_source_ref: String,
    pub agent_member_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    pub observed_at: String,
}

#[derive(Debug, Error)]
pub enum PersistedProjectionError {
    #[error("persisted Session projection context is incomplete")]
    InvalidContext,
    #[error("persisted provider row is invalid: {0}")]
    InvalidRow(#[from] crate::PersistedRecordValidationError),
    #[error("persisted provider row produced an invalid v3 record")]
    InvalidRecord,
}

#[derive(Debug, Clone)]
struct FragmentDraft {
    semantic_kind: SessionSemanticKind,
    lifecycle_phase: SessionLifecyclePhase,
    completeness: PersistedCompleteness,
    content_availability: ContentAvailability,
    content_unavailable_reason: Option<ContentUnavailableReason>,
    payload: PersistedFragmentPayload,
}

struct ToolDraft<'a> {
    name: Option<&'a str>,
    call_id: &'a str,
    parent_call_id: Option<&'a str>,
    arguments: Option<PersistedContentReference>,
    result: Option<PersistedContentReference>,
    error: Option<PersistedContentReference>,
    primary_target: Option<String>,
}

/// One stateful projector is used for every row in a persisted source. State is
/// limited to exact provider call-id → tool-name joins; it is disposable and
/// never becomes Harness authority.
pub struct PersistedSessionProjector {
    context: PersistedProjectionContext,
    tool_names: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PersistedProjectionSeed {
    tool_names: BTreeMap<String, String>,
}

impl PersistedProjectionSeed {
    pub(crate) fn observe(&mut self, row: &PersistedNativeRow) {
        if row.native_event.is_string() {
            return;
        }
        match row.provider {
            ProviderKind::Codex => {
                project_codex(&row.native_event, &mut self.tool_names);
            }
            ProviderKind::Claude => {
                project_claude(&row.native_event, &mut self.tool_names);
            }
            ProviderKind::Kimi => {
                project_kimi(&row.native_event, &mut self.tool_names);
            }
            ProviderKind::Pi => {
                project_pi(&row.native_event, &mut self.tool_names);
            }
            ProviderKind::DeepseekHarness => {
                project_deepseek_harness(&row.native_event, &mut self.tool_names);
            }
        }
    }
}

impl PersistedSessionProjector {
    pub fn new(context: PersistedProjectionContext) -> Result<Self, PersistedProjectionError> {
        Self::with_seed(context, PersistedProjectionSeed::default())
    }

    pub(crate) fn with_seed(
        context: PersistedProjectionContext,
        seed: PersistedProjectionSeed,
    ) -> Result<Self, PersistedProjectionError> {
        if context.native_source_ref.trim().is_empty()
            || context.agent_member_id.trim().is_empty()
            || context.agent_session_id.trim().is_empty()
            || context.agent_session_generation == 0
            || context.observed_at.trim().is_empty()
        {
            return Err(PersistedProjectionError::InvalidContext);
        }
        Ok(Self {
            context,
            tool_names: seed.tool_names,
        })
    }

    pub fn project(
        &mut self,
        row: PersistedNativeRow,
    ) -> Result<ProviderNativeEventRecord, PersistedProjectionError> {
        row.validate()?;
        let record_id =
            ProviderNativeEventRecord::stable_record_id(&row.source_generation, &row.row_locator);
        let (provider_thread_id, provider_turn_id, provider_event_id) =
            provider_identity(row.provider, &row.native_event);
        let mut drafts = if row.native_event.is_string() {
            vec![malformed("invalid_complete_json_row")]
        } else {
            match row.provider {
                ProviderKind::Codex => project_codex(&row.native_event, &mut self.tool_names),
                ProviderKind::Claude => project_claude(&row.native_event, &mut self.tool_names),
                ProviderKind::Kimi => project_kimi(&row.native_event, &mut self.tool_names),
                ProviderKind::Pi => project_pi(&row.native_event, &mut self.tool_names),
                ProviderKind::DeepseekHarness => {
                    project_deepseek_harness(&row.native_event, &mut self.tool_names)
                }
            }
        };
        if drafts.is_empty() {
            drafts.push(unclassified(&row.native_event));
        }
        let fragments = drafts
            .into_iter()
            .enumerate()
            .map(|(index, draft)| PersistedEventFragment {
                fragment_id: format!("{record_id}:fragment-{index}"),
                fragment_index: index as u32,
                semantic_kind: draft.semantic_kind,
                lifecycle_phase: draft.lifecycle_phase,
                completeness: draft.completeness,
                content_availability: draft.content_availability,
                content_unavailable_reason: draft.content_unavailable_reason,
                payload: draft.payload,
            })
            .collect();
        let record = ProviderNativeEventRecord {
            schema_version: PROVIDER_NATIVE_EVENT_RECORD_V3_SCHEMA_VERSION.into(),
            record_id,
            provider: row.provider,
            adapter_version: PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION.into(),
            native_source_ref: self.context.native_source_ref.clone(),
            source_generation: row.source_generation,
            row_locator: row.row_locator,
            ordering_key: row.ordering_key,
            source_content_fingerprint: row.content_fingerprint,
            agent_member_id: self.context.agent_member_id.clone(),
            agent_session_id: self.context.agent_session_id.clone(),
            agent_session_generation: self.context.agent_session_generation,
            provider_thread_id,
            provider_turn_id,
            provider_event_id,
            occurred_at: row.occurred_at,
            observed_at: self.context.observed_at.clone(),
            native_event: row.native_event,
            fragments,
        };
        record
            .validate()
            .map_err(|_| PersistedProjectionError::InvalidRecord)?;
        Ok(record)
    }
}

fn project_codex(raw: &Value, tools: &mut BTreeMap<String, String>) -> Vec<FragmentDraft> {
    let row_type = string(raw, "/type").unwrap_or_default();
    let payload = raw.pointer("/payload").unwrap_or(&Value::Null);
    match row_type {
        "session_meta" => vec![FragmentDraft {
            semantic_kind: SessionSemanticKind::SessionMetadata,
            lifecycle_phase: SessionLifecyclePhase::Started,
            completeness: PersistedCompleteness::Complete,
            content_availability: ContentAvailability::Available,
            content_unavailable_reason: None,
            payload: PersistedFragmentPayload::SessionMetadata {
                native_session_id: string(payload, "/id").map(str::to_owned),
            },
        }],
        "response_item" => match string(payload, "/type").unwrap_or_default() {
            "message" if string(payload, "/role") == Some("assistant") => response_parts(
                payload.pointer("/content").unwrap_or(&Value::Null),
                &["output_text", "text"],
            ),
            "reasoning" => vec![reasoning(first_text(
                payload,
                &["/summary_text", "/text", "/content/0/text"],
            ))],
            "function_call" | "custom_tool_call" | "local_shell_call" | "web_search_call" => {
                tool_requested(
                    tools,
                    first_text(payload, &["/name", "/tool_name"]),
                    first_text(payload, &["/call_id", "/id"]),
                    first_text(payload, &["/parent_call_id", "/parentCallId"]),
                    Some(content_reference(
                        raw,
                        &["/payload/arguments", "/payload/input"],
                    )),
                    primary_target(raw, &["/payload/arguments", "/payload/input"]),
                )
                .into_iter()
                .collect()
            }
            "function_call_output" | "custom_tool_call_output" | "local_shell_call_output" => {
                tool_terminal(
                    tools,
                    first_text(payload, &["/call_id", "/id"]),
                    string(payload, "/status") == Some("failed"),
                    Some(content_reference(
                        raw,
                        &["/payload/output", "/payload/result"],
                    )),
                )
                .into_iter()
                .collect()
            }
            _ => Vec::new(),
        },
        "event_msg" => match string(payload, "/type").unwrap_or_default() {
            "agent_message" => vec![assistant(first_text(payload, &["/message", "/text"]))],
            "agent_reasoning" | "reasoning" => {
                vec![reasoning(first_text(payload, &["/text", "/message"]))]
            }
            "token_count" => vec![usage(payload.pointer("/info/total_token_usage"))],
            "task_complete" | "turn_completed" => vec![turn("completed")],
            "turn_failed" | "task_failed" => vec![turn("failed")],
            "turn_cancelled" | "turn_canceled" | "task_cancelled" => {
                vec![turn("cancelled")]
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn project_claude(raw: &Value, tools: &mut BTreeMap<String, String>) -> Vec<FragmentDraft> {
    match string(raw, "/type").unwrap_or_default() {
        "system" if string(raw, "/subtype") == Some("init") => vec![FragmentDraft {
            semantic_kind: SessionSemanticKind::SessionMetadata,
            lifecycle_phase: SessionLifecyclePhase::Started,
            completeness: PersistedCompleteness::Complete,
            content_availability: ContentAvailability::Available,
            content_unavailable_reason: None,
            payload: PersistedFragmentPayload::SessionMetadata {
                native_session_id: first_text(raw, &["/session_id", "/sessionId"])
                    .map(str::to_owned),
            },
        }],
        "assistant" => claude_content(raw, tools),
        // Claude protocol role=user may contain the original user prompt as
        // well as provider tool results. Only tool_result is provider output.
        "user" => claude_tool_results(raw, tools),
        "result" => {
            let mut fragments = Vec::new();
            if raw.get("usage").is_some() {
                fragments.push(usage(raw.get("usage")));
            }
            fragments.push(turn(
                if string(raw, "/subtype").is_some_and(|value| {
                    matches!(
                        value,
                        "error" | "error_max_turns" | "error_during_execution"
                    )
                }) || raw.get("is_error").and_then(Value::as_bool) == Some(true)
                {
                    "failed"
                } else {
                    "completed"
                },
            ));
            fragments
        }
        _ => Vec::new(),
    }
}

fn claude_tool_results(raw: &Value, tools: &BTreeMap<String, String>) -> Vec<FragmentDraft> {
    raw.pointer("/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, part)| string(part, "/type") == Some("tool_result"))
        .filter_map(|(index, part)| {
            let pointer = format!("/message/content/{index}/content");
            tool_terminal(
                tools,
                string(part, "/tool_use_id"),
                part.get("is_error").and_then(Value::as_bool) == Some(true),
                Some(content_reference(raw, &[&pointer])),
            )
        })
        .collect()
}

fn claude_content(raw: &Value, tools: &mut BTreeMap<String, String>) -> Vec<FragmentDraft> {
    let Some(parts) = raw.pointer("/message/content").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut fragments = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        match string(part, "/type").unwrap_or_default() {
            "thinking" | "reasoning" => {
                fragments.push(reasoning(first_text(part, &["/thinking", "/text"])))
            }
            "text" => fragments.push(assistant(string(part, "/text"))),
            "tool_use" => fragments.extend(tool_requested(
                tools,
                string(part, "/name"),
                string(part, "/id"),
                first_text(part, &["/parent_tool_use_id", "/parent_call_id"]),
                Some(content_reference(
                    raw,
                    &[&format!("/message/content/{index}/input")],
                )),
                primary_target(raw, &[&format!("/message/content/{index}/input")]),
            )),
            "tool_result" => fragments.extend(tool_terminal(
                tools,
                string(part, "/tool_use_id"),
                part.get("is_error").and_then(Value::as_bool) == Some(true),
                Some(content_reference(
                    raw,
                    &[&format!("/message/content/{index}/content")],
                )),
            )),
            _ => {}
        }
    }
    fragments
}

fn project_kimi(raw: &Value, tools: &mut BTreeMap<String, String>) -> Vec<FragmentDraft> {
    if string(raw, "/type") != Some("context.append_loop_event") {
        return Vec::new();
    }
    let event = raw.pointer("/event").unwrap_or(&Value::Null);
    match string(event, "/type").unwrap_or_default() {
        "content.part" => match string(event, "/part/type").unwrap_or_default() {
            "think" | "thinking" => vec![reasoning(first_text(
                event,
                &["/part/text", "/part/think", "/part/thinking"],
            ))],
            "text" => vec![assistant(string(event, "/part/text"))],
            _ => Vec::new(),
        },
        "tool.call" => tool_requested(
            tools,
            string(event, "/name"),
            first_text(event, &["/toolCallId", "/id"]),
            first_text(event, &["/parentToolCallId", "/parent_call_id"]),
            Some(content_reference(raw, &["/event/args", "/event/arguments"])),
            primary_target(raw, &["/event/args", "/event/arguments"]),
        ),
        "tool.result" => tool_terminal(
            tools,
            first_text(event, &["/toolCallId", "/id"]),
            string(event, "/status").is_some_and(|value| matches!(value, "failed" | "error")),
            Some(content_reference(raw, &["/event/result", "/event/error"])),
        )
        .into_iter()
        .collect(),
        "artifact.created" => first_text(event, &["/name", "/displayName"])
            .map(|name| artifact(name, event))
            .into_iter()
            .collect(),
        "step.end" => vec![turn(
            match string(event, "/finishReason").unwrap_or_default() {
                "end_turn" | "completed" => "completed",
                "cancelled" | "canceled" => "cancelled",
                _ => "failed",
            },
        )],
        _ => Vec::new(),
    }
}

fn project_pi(raw: &Value, tools: &mut BTreeMap<String, String>) -> Vec<FragmentDraft> {
    match string(raw, "/type").unwrap_or_default() {
        "session" => vec![FragmentDraft {
            semantic_kind: SessionSemanticKind::SessionMetadata,
            lifecycle_phase: SessionLifecyclePhase::Started,
            completeness: PersistedCompleteness::Complete,
            content_availability: ContentAvailability::Available,
            content_unavailable_reason: None,
            payload: PersistedFragmentPayload::SessionMetadata {
                native_session_id: first_text(raw, &["/id", "/sessionId"]).map(str::to_owned),
            },
        }],
        "message" if string(raw, "/message/role") == Some("assistant") => {
            let mut fragments = pi_content(raw, tools);
            if let Some(reason) = string(raw, "/message/stopReason") {
                fragments.push(turn(match reason {
                    "error" | "failed" => "failed",
                    "cancelled" | "canceled" => "cancelled",
                    _ => "completed",
                }));
            }
            fragments
        }
        "message" if string(raw, "/message/role") == Some("tool") => pi_tool_results(raw, tools),
        // User and system rows remain available in native_event but cannot be
        // relabeled as provider-authored assistant output.
        "message" => Vec::new(),
        "artifact" | "artifact_created" => first_text(raw, &["/name", "/displayName"])
            .map(|name| artifact(name, raw))
            .into_iter()
            .collect(),
        // message_update and tool_execution_* are RPC callback families, not
        // managed Pi persisted Session rows. They remain raw/unclassified.
        _ => Vec::new(),
    }
}

fn pi_content(raw: &Value, tools: &mut BTreeMap<String, String>) -> Vec<FragmentDraft> {
    let Some(parts) = raw.pointer("/message/content").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut fragments = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        match string(part, "/type").unwrap_or_default() {
            "text" => fragments.push(assistant(string(part, "/text"))),
            "toolCall" | "tool_call" | "tool-call" => fragments.extend(tool_requested(
                tools,
                first_text(part, &["/name", "/toolName"]),
                first_text(part, &["/id", "/toolCallId"]),
                first_text(part, &["/parentToolCallId", "/parent_call_id"]),
                Some(content_reference(
                    raw,
                    &[
                        &format!("/message/content/{index}/arguments"),
                        &format!("/message/content/{index}/input"),
                    ],
                )),
                primary_target(
                    raw,
                    &[
                        &format!("/message/content/{index}/arguments"),
                        &format!("/message/content/{index}/input"),
                    ],
                ),
            )),
            "artifact" => {
                if let Some(name) = first_text(part, &["/name", "/displayName"]) {
                    fragments.push(artifact(name, part));
                }
            }
            // Team-managed Pi is launched with thinking off. A persisted
            // thinking block is retained only in native_event and never grows
            // a Reasoning capability claim.
            "thinking" | "reasoning" => {}
            _ => {}
        }
    }
    fragments
}

fn pi_tool_results(raw: &Value, tools: &BTreeMap<String, String>) -> Vec<FragmentDraft> {
    raw.pointer("/message/content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(_, part)| {
            matches!(
                string(part, "/type"),
                Some("toolResult" | "tool_result" | "tool-result")
            )
        })
        .filter_map(|(index, part)| {
            let content_pointer = format!("/message/content/{index}/content");
            let result_pointer = format!("/message/content/{index}/result");
            tool_terminal(
                tools,
                first_text(part, &["/toolCallId", "/tool_use_id", "/id"]),
                part.get("isError").and_then(Value::as_bool) == Some(true)
                    || part.get("is_error").and_then(Value::as_bool) == Some(true),
                Some(content_reference(raw, &[&content_pointer, &result_pointer])),
            )
        })
        .collect()
}

fn project_deepseek_harness(
    raw: &Value,
    tools: &mut BTreeMap<String, String>,
) -> Vec<FragmentDraft> {
    match string(raw, "/type").unwrap_or_default() {
        "assistant/message" => response_parts(
            raw.pointer("/data/message/content").unwrap_or(&Value::Null),
            &["text"],
        ),
        "tool/call" => tool_started(
            tools,
            string(raw, "/data/name"),
            string(raw, "/data/callId"),
            first_text(raw, &["/data/parentCallId", "/data/parent_call_id"]),
            Some(content_reference(raw, &["/data/arguments"])),
            primary_target(raw, &["/data/arguments"]),
        ),
        "tool/result" => tool_terminal(
            tools,
            string(raw, "/data/message/source/callId"),
            string(raw, "/data/status").is_some_and(|value| matches!(value, "failed" | "error")),
            Some(content_reference(raw, &["/data/result", "/data/error"])),
        )
        .into_iter()
        .collect(),
        "assistant/chunk" if string(raw, "/data/chunk/type") == Some("usage") => {
            vec![usage(raw.pointer("/data/chunk/usage"))]
        }
        "turn/end" => vec![turn(
            match string(raw, "/data/reason/kind").unwrap_or_default() {
                "completed" => "completed",
                "cancelled" | "canceled" => "cancelled",
                _ => "failed",
            },
        )],
        _ => Vec::new(),
    }
}

fn response_parts(content: &Value, accepted_types: &[&str]) -> Vec<FragmentDraft> {
    let Some(parts) = content.as_array() else {
        return Vec::new();
    };
    parts
        .iter()
        .filter(|part| string(part, "/type").is_some_and(|kind| accepted_types.contains(&kind)))
        .map(|part| assistant(string(part, "/text")))
        .collect()
}

fn assistant(text: Option<&str>) -> FragmentDraft {
    text_fragment(SessionSemanticKind::AssistantResponse, text, |text| {
        PersistedFragmentPayload::AssistantResponse { text }
    })
}

fn reasoning(text: Option<&str>) -> FragmentDraft {
    text_fragment(SessionSemanticKind::Reasoning, text, |text| {
        PersistedFragmentPayload::Reasoning { text }
    })
}

fn text_fragment(
    semantic_kind: SessionSemanticKind,
    text: Option<&str>,
    payload: impl FnOnce(Option<String>) -> PersistedFragmentPayload,
) -> FragmentDraft {
    let exact = text.filter(|value| !value.is_empty()).map(str::to_owned);
    FragmentDraft {
        semantic_kind,
        lifecycle_phase: SessionLifecyclePhase::Progress,
        completeness: PersistedCompleteness::Complete,
        content_availability: if exact.is_some() {
            ContentAvailability::Available
        } else {
            ContentAvailability::Unavailable
        },
        content_unavailable_reason: exact
            .is_none()
            .then_some(ContentUnavailableReason::ProviderAbsent),
        payload: payload(exact),
    }
}

fn tool_requested(
    tools: &mut BTreeMap<String, String>,
    name: Option<&str>,
    call_id: Option<&str>,
    parent_call_id: Option<&str>,
    arguments: Option<PersistedContentReference>,
    primary_target: Option<String>,
) -> Vec<FragmentDraft> {
    let (Some(name), Some(call_id)) = (nonempty(name), nonempty(call_id)) else {
        return Vec::new();
    };
    tools.insert(call_id.to_owned(), name.to_owned());
    vec![tool(
        SessionSemanticKind::ToolCallRequested,
        SessionLifecyclePhase::Requested,
        ToolDraft {
            name: Some(name),
            call_id,
            parent_call_id,
            arguments,
            result: None,
            error: None,
            primary_target,
        },
    )]
}

fn tool_started(
    tools: &mut BTreeMap<String, String>,
    name: Option<&str>,
    call_id: Option<&str>,
    parent_call_id: Option<&str>,
    arguments: Option<PersistedContentReference>,
    primary_target: Option<String>,
) -> Vec<FragmentDraft> {
    let (Some(name), Some(call_id)) = (nonempty(name), nonempty(call_id)) else {
        return Vec::new();
    };
    tools.insert(call_id.to_owned(), name.to_owned());
    vec![tool(
        SessionSemanticKind::ToolCallStarted,
        SessionLifecyclePhase::Started,
        ToolDraft {
            name: Some(name),
            call_id,
            parent_call_id,
            arguments,
            result: None,
            error: None,
            primary_target,
        },
    )]
}

fn tool_terminal(
    tools: &BTreeMap<String, String>,
    call_id: Option<&str>,
    failed: bool,
    result: Option<PersistedContentReference>,
) -> Option<FragmentDraft> {
    let call_id = nonempty(call_id)?;
    let name = tools.get(call_id).map(String::as_str);
    let (result, error) = if failed {
        (None, result)
    } else {
        (result, None)
    };
    Some(tool(
        if failed {
            SessionSemanticKind::ToolCallFailed
        } else {
            SessionSemanticKind::ToolCallCompleted
        },
        SessionLifecyclePhase::Terminal,
        ToolDraft {
            name,
            call_id,
            parent_call_id: None,
            arguments: None,
            result,
            error,
            primary_target: None,
        },
    ))
}

fn tool(
    semantic_kind: SessionSemanticKind,
    lifecycle_phase: SessionLifecyclePhase,
    details: ToolDraft<'_>,
) -> FragmentDraft {
    let outcome = match semantic_kind {
        SessionSemanticKind::ToolCallRequested => ToolCallOutcome::Requested,
        SessionSemanticKind::ToolCallStarted => ToolCallOutcome::Started,
        SessionSemanticKind::ToolCallCompleted => ToolCallOutcome::Completed,
        SessionSemanticKind::ToolCallFailed => ToolCallOutcome::Failed,
        _ => unreachable!("tool fragment semantic kind"),
    };
    FragmentDraft {
        semantic_kind,
        lifecycle_phase,
        completeness: PersistedCompleteness::Complete,
        content_availability: ContentAvailability::Available,
        content_unavailable_reason: None,
        payload: PersistedFragmentPayload::Tool {
            tool_name: details.name.map(str::to_owned),
            tool_name_unavailable_reason: details
                .name
                .is_none()
                .then_some(ContentUnavailableReason::RelatedRecordMissing),
            call_id: Some(details.call_id.to_owned()),
            parent_call_id: details.parent_call_id.map(str::to_owned),
            operation_category: Some(
                details
                    .name
                    .map(tool_operation_category)
                    .unwrap_or(ToolOperationCategory::Other),
            ),
            primary_target: details.primary_target,
            arguments: details.arguments,
            result: details.result,
            error: details.error,
            outcome: Some(outcome),
            display_detail: None,
        },
    }
}

fn content_reference(raw: &Value, pointers: &[&str]) -> PersistedContentReference {
    if let Some(pointer) = pointers
        .iter()
        .find(|pointer| raw.pointer(pointer).is_some_and(|value| !value.is_null()))
    {
        PersistedContentReference {
            availability: ContentAvailability::Available,
            unavailable_reason: None,
            json_pointer: Some((*pointer).to_owned()),
        }
    } else {
        PersistedContentReference {
            availability: ContentAvailability::Unavailable,
            unavailable_reason: Some(ContentUnavailableReason::ProviderAbsent),
            json_pointer: None,
        }
    }
}

fn primary_target(raw: &Value, pointers: &[&str]) -> Option<String> {
    let arguments = pointers.iter().find_map(|pointer| raw.pointer(pointer))?;
    if let Some(value) = arguments.as_str().and_then(bounded_summary) {
        return Some(value.to_owned());
    }
    [
        "file_path",
        "filePath",
        "path",
        "command",
        "cmd",
        "query",
        "url",
        "pattern",
        "task",
        "prompt",
        "name",
    ]
    .iter()
    .find_map(|key| arguments.get(key).and_then(Value::as_str))
    .and_then(bounded_summary)
    .map(str::to_owned)
}

fn bounded_summary(value: &str) -> Option<&str> {
    (!value.is_empty() && value.chars().count() <= 512).then_some(value)
}

fn tool_operation_category(name: &str) -> ToolOperationCategory {
    match name.to_ascii_lowercase().as_str() {
        "read" | "read_file" | "cat" => ToolOperationCategory::Read,
        "search" | "grep" | "glob" | "rg" | "find" => ToolOperationCategory::Search,
        "bash" | "shell" | "exec" | "exec_command" | "local_shell" => {
            ToolOperationCategory::Command
        }
        "write" | "write_file" | "create_file" => ToolOperationCategory::Write,
        "edit" | "apply_patch" | "replace" => ToolOperationCategory::Edit,
        "web_search" | "web_search_call" | "fetch" | "http" => ToolOperationCategory::Network,
        "task" | "subagent" | "spawn_agent" | "agent" => ToolOperationCategory::Subagent,
        _ => ToolOperationCategory::Other,
    }
}

fn usage(value: Option<&Value>) -> FragmentDraft {
    FragmentDraft {
        semantic_kind: SessionSemanticKind::UsageReported,
        lifecycle_phase: SessionLifecyclePhase::Progress,
        completeness: PersistedCompleteness::Complete,
        content_availability: ContentAvailability::Available,
        content_unavailable_reason: None,
        payload: PersistedFragmentPayload::Usage {
            input_tokens: value.and_then(|value| {
                value
                    .get("input_tokens")
                    .or_else(|| value.get("inputTokens"))
                    .and_then(Value::as_u64)
            }),
            output_tokens: value.and_then(|value| {
                value
                    .get("output_tokens")
                    .or_else(|| value.get("outputTokens"))
                    .and_then(Value::as_u64)
            }),
            total_tokens: value.and_then(|value| {
                value
                    .get("total_tokens")
                    .or_else(|| value.get("totalTokens"))
                    .and_then(Value::as_u64)
            }),
        },
    }
}

fn artifact(name: &str, value: &Value) -> FragmentDraft {
    FragmentDraft {
        semantic_kind: SessionSemanticKind::ArtifactCreated,
        lifecycle_phase: SessionLifecyclePhase::Terminal,
        completeness: PersistedCompleteness::Complete,
        content_availability: ContentAvailability::Available,
        content_unavailable_reason: None,
        payload: PersistedFragmentPayload::Artifact {
            display_name: name.to_owned(),
            media_type: first_text(value, &["/mediaType", "/media_type"]).map(str::to_owned),
            content_digest: first_text(value, &["/digest", "/contentDigest"])
                .filter(|value| value.starts_with("sha256:"))
                .map(str::to_owned),
        },
    }
}

fn turn(outcome: &str) -> FragmentDraft {
    FragmentDraft {
        semantic_kind: match outcome {
            "completed" => SessionSemanticKind::TurnCompleted,
            "cancelled" => SessionSemanticKind::TurnCancelled,
            _ => SessionSemanticKind::TurnFailed,
        },
        lifecycle_phase: SessionLifecyclePhase::Terminal,
        completeness: PersistedCompleteness::Complete,
        content_availability: ContentAvailability::Available,
        content_unavailable_reason: None,
        payload: PersistedFragmentPayload::Turn {
            outcome: outcome.to_owned(),
            display_summary: None,
        },
    }
}

fn malformed(reason: &str) -> FragmentDraft {
    FragmentDraft {
        semantic_kind: SessionSemanticKind::MalformedOrIncomplete,
        lifecycle_phase: SessionLifecyclePhase::Terminal,
        completeness: PersistedCompleteness::Incomplete,
        content_availability: ContentAvailability::Available,
        content_unavailable_reason: None,
        payload: PersistedFragmentPayload::Malformed {
            reason_code: reason.to_owned(),
        },
    }
}

fn unclassified(raw: &Value) -> FragmentDraft {
    let event_type = event_type(raw);
    let event_subtype = first_text(
        raw,
        &[
            "/subtype",
            "/event/subtype",
            "/event/type",
            "/payload/subtype",
            "/payload/type",
            "/data/subtype",
            "/data/type",
            "/data/chunk/type",
            "/message/subtype",
            "/message/type",
        ],
    )
    .filter(|value| Some(*value) != event_type);
    FragmentDraft {
        semantic_kind: SessionSemanticKind::UnclassifiedNative,
        lifecycle_phase: SessionLifecyclePhase::Progress,
        completeness: PersistedCompleteness::Complete,
        content_availability: ContentAvailability::Available,
        content_unavailable_reason: None,
        payload: PersistedFragmentPayload::Native {
            event_type: event_type
                .filter(|value| value.chars().count() <= 256)
                .map(str::to_owned),
            event_subtype: event_subtype
                .filter(|value| value.chars().count() <= 256)
                .map(str::to_owned),
            classification_reason: Some(if event_type.is_some() {
                NativeClassificationReason::UnsupportedEventType
            } else {
                NativeClassificationReason::MissingEventType
            }),
        },
    }
}

fn provider_identity(
    provider: ProviderKind,
    raw: &Value,
) -> (Option<String>, Option<String>, Option<String>) {
    let thread =
        first_text(raw, &["/thread_id", "/session_id", "/payload/session_id"]).map(str::to_owned);
    let turn = first_text(
        raw,
        &[
            "/turn_id",
            "/payload/turn_id",
            "/event/turnId",
            "/data/turn",
        ],
    )
    .map(str::to_owned)
    .or_else(|| {
        raw.pointer("/data/turn")
            .and_then(Value::as_u64)
            .map(|v| v.to_string())
    });
    let event = match provider {
        ProviderKind::Codex => first_text(raw, &["/payload/id", "/payload/call_id"]),
        ProviderKind::Claude => first_text(raw, &["/uuid", "/message/id", "/id"]),
        ProviderKind::Kimi => first_text(raw, &["/event/uuid", "/event/id"]),
        ProviderKind::Pi => first_text(raw, &["/id", "/message/id"]),
        ProviderKind::DeepseekHarness => first_text(raw, &["/id"]),
    }
    .map(str::to_owned)
    .or_else(|| {
        raw.get("seq")
            .and_then(Value::as_u64)
            .map(|v| v.to_string())
    });
    (thread, turn, event)
}

fn event_type(value: &Value) -> Option<&str> {
    string(value, "/type")
        .or_else(|| string(value, "/event/type"))
        .or_else(|| string(value, "/payload/type"))
}

fn first_text<'a>(value: &'a Value, pointers: &[&str]) -> Option<&'a str> {
    pointers.iter().find_map(|pointer| string(value, pointer))
}

fn string<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn nonempty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

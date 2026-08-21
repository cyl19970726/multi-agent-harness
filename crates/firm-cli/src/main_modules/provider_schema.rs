use super::*;

/// Normalize a `schema=` dict into a real JSON Schema suitable for the providers'
/// native structured-output flags (claude `--json-schema`, codex `--output-schema`).
/// Two input shapes are accepted: an ALREADY-valid JSON Schema (has `type` or
/// `properties`) is passed through unchanged; the legacy flat `{ key: "hint" }`
/// form is wrapped into `{ type:object, properties:{...}, required:[keys],
/// additionalProperties:false }`.
///
/// A flat hint that is a WELL-KNOWN type word (`bool`/`int`/`number`/…) becomes a
/// real JSON-Schema scalar type, so the provider returns — and the workflow script
/// reads back — a real bool/int/number instead of a string (issue #139 item 5:
/// `{ "ok": "bool" }` used to yield the STRING `"true"`, making `if res["ok"]:`
/// always truthy). Any other hint stays a `string` field with the hint kept as its
/// `description`, exactly as before.
pub(super) fn schema_to_json_schema(schema: &serde_json::Value) -> serde_json::Value {
    let Some(obj) = schema.as_object() else {
        return schema.clone();
    };
    if obj.contains_key("type") || obj.contains_key("properties") {
        return schema.clone();
    }
    let mut props = serde_json::Map::new();
    for (k, v) in obj {
        let hint = v.as_str().unwrap_or("");
        let json_type = match hint.trim().to_ascii_lowercase().as_str() {
            "bool" | "boolean" => "boolean",
            "int" | "integer" => "integer",
            "number" | "float" | "double" => "number",
            _ => "string",
        };
        let mut field = serde_json::Map::new();
        field.insert("type".into(), serde_json::Value::from(json_type));
        // Keep the hint as the description only when it carries real meaning — a
        // bare type word ("bool") becomes the type and needs no description.
        if json_type == "string" && !hint.is_empty() {
            field.insert("description".into(), serde_json::Value::from(hint));
        }
        props.insert(k.clone(), serde_json::Value::Object(field));
    }
    serde_json::json!({
        "type": "object",
        "properties": props,
        "required": obj.keys().cloned().collect::<Vec<_>>(),
        "additionalProperties": false,
    })
}

/// The REQUIRED top-level keys a schema declares. The schema is a JSON object;
/// its keys ARE the required keys the structured reply must carry. A non-object
/// schema (or one with no keys) declares no required keys.
pub(super) fn extract_json_object(reply: &str) -> Option<serde_json::Value> {
    let trimmed = reply.trim();

    // 1. Strip a surrounding ```json ... ``` (or ``` ... ```) fence if present.
    let unfenced = strip_code_fence(trimmed);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(unfenced.trim()) {
        if value.is_object() {
            return Some(value);
        }
    }

    // 2. Fall back to the first balanced `{ ... }` object in the text.
    if let Some(slice) = first_balanced_object(trimmed) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(slice) {
            if value.is_object() {
                return Some(value);
            }
        }
    }
    None
}

/// Strip a single surrounding triple-backtick fence from `text` if it both starts
/// with ``` (optionally ```json / ```JSON) and ends with ```. Returns the inner
/// body; otherwise returns `text` unchanged.
pub(super) fn strip_code_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    // Drop an optional language tag on the opening fence line.
    let body = match rest.split_once('\n') {
        Some((_lang, after)) => after,
        None => rest,
    };
    body.strip_suffix("```").unwrap_or(body)
}

/// Return the first balanced `{ ... }` object substring of `text`, honoring JSON
/// string literals (so braces inside strings do not affect nesting). `None` when
/// there is no balanced object.
pub(super) fn first_balanced_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, &byte) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=start + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

#[derive(Debug)]
pub(super) struct TokenUsage {
    pub(super) input: u64,
    pub(super) output: u64,
    pub(super) total: u64,
}

impl TokenUsage {
    pub(super) fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "input": self.input,
            "output": self.output,
            "total": self.total,
        })
    }
}

/// Parse codex `turn.completed` usage into a normalized [`TokenUsage`]. Codex
/// `exec --json` emits `{"type":"turn.completed","usage":{...}}` (some builds nest
/// the usage under `turn`). The usage object carries `input_tokens`,
/// `output_tokens`, and the SUBSET counters `cached_input_tokens` /
/// `reasoning_output_tokens` (already included in input/output respectively).
/// Returns `None` when no terminal usage object is present.
pub(super) fn parse_codex_usage(events: &[serde_json::Value]) -> Option<TokenUsage> {
    events.iter().rev().find_map(|payload| {
        let ty = payload.get("type").and_then(|t| t.as_str())?;
        if ty != "turn.completed" && ty != "turn_completed" {
            return None;
        }
        let usage = payload
            .get("usage")
            .or_else(|| payload.get("turn").and_then(|t| t.get("usage")))?;
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Some(TokenUsage {
            input,
            output,
            total: input.saturating_add(output),
        })
    })
}

/// Parse claude `result` usage into a normalized [`TokenUsage`]. Claude
/// `--output-format stream-json` emits a terminal `{"type":"result","usage":{
/// "input_tokens":N,"output_tokens":N,...}}`. Returns `None` when no result usage
/// is present.
pub(super) fn parse_claude_usage(events: &[serde_json::Value]) -> Option<TokenUsage> {
    events.iter().rev().find_map(|payload| {
        if payload.get("type").and_then(|t| t.as_str()) != Some("result") {
            return None;
        }
        let usage = payload.get("usage")?;
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Some(TokenUsage {
            input,
            output,
            total: input.saturating_add(output),
        })
    })
}

/// The model a worker actually ran, when the provider reports it. Claude
/// `--output-format stream-json` emits a `{"type":"system","subtype":"init",
/// "model":"claude-…"}` frame; codex `exec --json` carries none (returns `None`).
pub(super) fn parse_worker_model(events: &[serde_json::Value]) -> Option<String> {
    events.iter().find_map(|payload| {
        if payload.get("type").and_then(|t| t.as_str()) != Some("system") {
            return None;
        }
        payload
            .get("model")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string())
    })
}

/// Parse claude's terminal `result` frame for the two extras it carries:
/// `structured_output` (a schema-validated object, present only when the worker
/// ran with `--json-schema`) and `total_cost_usd` (the billed turn cost). Returns
/// `(structured, cost_usd)`, each `None` when absent.
pub(super) fn parse_claude_result_extras(
    events: &[serde_json::Value],
) -> (Option<serde_json::Value>, Option<f64>) {
    events
        .iter()
        .rev()
        .find_map(|payload| {
            if payload.get("type").and_then(|t| t.as_str()) != Some("result") {
                return None;
            }
            let structured = payload
                .get("structured_output")
                .filter(|v| v.is_object())
                .cloned();
            let cost = payload.get("total_cost_usd").and_then(|v| v.as_f64());
            Some((structured, cost))
        })
        .unwrap_or((None, None))
}

pub(super) fn codex_delivery_telemetry(
    raw_events: &[serde_json::Value],
    spec: &LaunchSpec,
) -> (Option<TokenUsage>, Option<f64>, Option<String>) {
    (parse_codex_usage(raw_events), None, spec.model.clone())
}

pub(super) fn codex_delivery_structured(
    reply: Option<&str>,
    spec: &LaunchSpec,
) -> Option<serde_json::Value> {
    spec.output_schema
        .as_ref()
        .and_then(|_| reply.and_then(extract_json_object))
}

/// The structured output is the turn's ANSWER, so it is surfaced only on a
/// SUCCEEDED delivery. A failed/stale turn may have emitted partial or
/// schema-violating JSON that must not be reported as the structured result.
pub(super) fn structured_for_status(
    status: &ProviderExecutionStatus,
    structured: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match status {
        ProviderExecutionStatus::Succeeded => structured,
        _ => None,
    }
}

pub(super) fn claude_delivery_telemetry(
    raw_events: &[serde_json::Value],
) -> (
    Option<TokenUsage>,
    Option<f64>,
    Option<String>,
    Option<serde_json::Value>,
) {
    let (structured, cost_usd) = parse_claude_result_extras(raw_events);
    (
        parse_claude_usage(raw_events),
        cost_usd,
        parse_worker_model(raw_events),
        structured,
    )
}

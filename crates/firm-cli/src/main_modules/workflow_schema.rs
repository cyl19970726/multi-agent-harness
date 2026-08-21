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
pub(super) fn schema_required_keys(schema: &serde_json::Value) -> Vec<String> {
    schema
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Build the schema instruction appended to a structured-mode prompt: tell the
/// worker to reply with ONLY a single JSON object carrying the schema's top-level
/// keys (no prose, no markdown fences), and inline the compact schema as a shape
/// hint. Returned with a leading separator so it can be concatenated onto a prompt.
pub(super) fn schema_instruction(schema: &serde_json::Value) -> String {
    let keys = schema_required_keys(schema).join(", ");
    let compact = serde_json::to_string(schema).unwrap_or_else(|_| "{}".to_string());
    format!(
        "\n\nRespond with ONLY a single JSON object with these top-level keys: [{keys}]. \
         No prose, no markdown fences. Shape hint: {compact}"
    )
}

/// Extract a JSON OBJECT from a worker reply, robustly: first strip a leading /
/// trailing triple-backtick fence (```json ... ``` or ``` ... ```) and try to
/// parse the whole thing; failing that, take the FIRST balanced `{ ... }` object
/// substring and parse it. Returns the parsed value only when it is a JSON object.
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

/// Whether `obj` (a parsed structured reply) contains EVERY required top-level
/// key. An empty required set is vacuously satisfied.
pub(super) fn object_has_required_keys(obj: &serde_json::Value, required: &[String]) -> bool {
    match obj.as_object() {
        Some(map) => required.iter().all(|key| map.contains_key(key)),
        None => false,
    }
}

/// Maximum worktree-diff text we store on a step result. Diffs above this are
/// truncated to the cap and flagged with `worktree_diff_truncated: true` so the
/// dashboard can render a "diff truncated" hint without choking on a huge blob.
pub(super) const WORKTREE_DIFF_CAP: usize = 20_000;
pub(super) const SCHEMA_CORRECTION_RETRY_TIMEOUT_MS: u64 = 60_000;

pub(super) fn schema_correction_retry_limits(
    idle_timeout_ms: u64,
    leaf_wall_clock_ms: Option<u64>,
) -> (u64, Option<u64>) {
    let retry_idle_timeout_ms = idle_timeout_ms.min(SCHEMA_CORRECTION_RETRY_TIMEOUT_MS);
    let retry_wall_clock_ms = Some(
        leaf_wall_clock_ms
            .unwrap_or(SCHEMA_CORRECTION_RETRY_TIMEOUT_MS)
            .min(SCHEMA_CORRECTION_RETRY_TIMEOUT_MS),
    );
    (retry_idle_timeout_ms, retry_wall_clock_ms)
}

pub(super) fn schema_failure_detail(
    required_keys: &[String],
    retry_attempted: bool,
    retry_timed_out: bool,
) -> String {
    let retry_detail = if retry_timed_out {
        "schema correction retry timed out before producing valid JSON"
    } else if retry_attempted {
        "schema correction retry returned no valid JSON with required keys"
    } else {
        "worker reply was not a JSON object with required keys"
    };
    format!("{retry_detail} [{}]", required_keys.join(", "))
}

/// Assemble the observability `details` object merged onto the step's `result`
/// JSON (see `workflow::step_result_json`): the model the worker ran, exit code,
/// duration, normalized token usage, a structured failure (when the step failed),
/// and the FULL worktree diff text (capped) for the isolation path. Keys here are
/// additive — the base step_result_json keys win on any collision.
pub(super) fn build_step_details(
    spec: &workflow::AgentStepSpec,
    spawn: &EphemeralSpawn,
    effective_model: Option<&str>,
    duration_ms: u64,
    diff: Option<&str>,
    worktree_changed_paths: Option<&[String]>,
) -> serde_json::Value {
    // The node's requested model wins; otherwise fall back to the model the
    // worker reported in its own output (claude's init frame).
    let model = effective_model
        .map(|model| model.to_string())
        .or_else(|| spawn.model.clone());
    let mut details = serde_json::json!({
        "model": model,
        "native_session": spawn.native_session,
        "exit_code": spawn.exit_code,
        "duration_ms": duration_ms,
        "persist_changes": spec.persist_changes.clone(),
        "write_mode": spec.write_mode.clone(),
        "owned_paths": spec.owned_paths.clone(),
        "artifact_root": spec.artifact_root.clone(),
        "write_roots": spec.write_roots.clone(),
        "auto_apply_on_verdict": spec.auto_apply_on_verdict,
        // D3a: whether this leaf was DECLARED writable. A read-only leaf that runs
        // isolated only because its provider can't enforce read-only (#167 kimi)
        // also produces a `worktree_diff`, so persistence must key on `writable`
        // to swallow that unauthorized diff instead of persisting it.
        "writable": spec.writable,
    });
    let map = details
        .as_object_mut()
        .expect("json! object is always an object");

    if let Some(tokens) = spawn.tokens {
        map.insert("tokens".into(), tokens.to_json());
    }

    if let Some(cost) = spawn.cost_usd {
        if let Some(n) = serde_json::Number::from_f64(cost) {
            map.insert("cost_usd".into(), serde_json::Value::Number(n));
        }
    }

    if let Some(reason) = classify_failure_reason(spawn.ok, spawn.exit_code, spawn.timed_out) {
        let detail = if spawn.stderr.trim().is_empty() {
            format!("{} worker step failed ({reason})", spec.provider)
        } else {
            spawn.stderr.trim().to_string()
        };
        map.insert(
            "failure".into(),
            serde_json::json!({
                "failed": true,
                "reason": reason,
                "detail": detail,
            }),
        );
    }
    if spawn.wall_timed_out {
        map.insert("wall_timed_out".into(), serde_json::Value::Bool(true));
    }

    if let Some(diff) = diff {
        let (text, truncated) = if diff.len() > WORKTREE_DIFF_CAP {
            (truncate_on_char_boundary(diff, WORKTREE_DIFF_CAP), true)
        } else {
            (diff, false)
        };
        map.insert(
            "worktree_diff".into(),
            serde_json::Value::String(text.to_string()),
        );
        map.insert(
            "worktree_diff_truncated".into(),
            serde_json::Value::Bool(truncated),
        );
        // The full, uncapped diff for the retained Workflow patch pipeline.
        // `worktree_diff` above is CAPPED for dashboard display, so a truncated diff
        // would fail to apply (and falsely fail a passing phase); landing reads this
        // uncapped copy and falls back to `worktree_diff` only when absent (e.g. an
        // old run / a mock that carries only `worktree_diff`).
        if truncated {
            map.insert(
                "landing_diff".into(),
                serde_json::Value::String(diff.to_string()),
            );
        }
    }

    // D4a: the robustly-enumerated changed paths (both rename sides + all
    // adds/mods/deletes) captured from the worktree by name-status. Persist /
    // landing read this instead of re-parsing `diff --git` headers off the text.
    if let Some(changed) = worktree_changed_paths {
        map.insert("worktree_changed_paths".into(), serde_json::json!(changed));
    }

    if !spawn.warnings.is_empty() {
        map.insert(
            "observability_warnings".into(),
            serde_json::Value::Array(
                spawn
                    .warnings
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }

    details
}

/// The outcome of one ephemeral worker process: whether the turn succeeded, the
/// parsed terminal reply text (if any), the raw NDJSON the worker emitted, and
/// any stderr (for failure summaries).
pub(super) struct EphemeralSpawn {
    pub(super) ok: bool,
    pub(super) reply: Option<String>,
    /// Reference to the provider-owned session discovered from the native
    /// stream. The Harness execution key is intentionally not a session id.
    pub(super) native_session: Option<NativeSessionRef>,
    pub(super) stderr: String,
    /// Process exit code; `None` when the worker was killed on timeout / signal.
    pub(super) exit_code: Option<i32>,
    /// True when the per-node timeout fired (the worker was killed mid-turn).
    pub(super) timed_out: bool,
    /// True when the timeout was the per-leaf wall-clock cap, not idle silence.
    pub(super) wall_timed_out: bool,
    /// Normalized token usage parsed from the terminal event, when present:
    /// `{ input, output, total }`. `None` when the stream carried no usage.
    pub(super) tokens: Option<TokenUsage>,
    /// The model the worker actually ran, parsed from its output when the
    /// provider reports it (claude's `system`/`init` event). `None` for codex,
    /// whose `exec --json` stream carries no model — the node's requested
    /// `spec.model` is the only signal there.
    pub(super) model: Option<String>,
    /// The provider-validated structured object, when the worker ran with a
    /// native schema flag (claude `--json-schema` → `result.structured_output`;
    /// codex `--output-schema` → the schema-constrained reply). `None` for
    /// text-mode steps or when no native structured output was produced — the
    /// caller then falls back to extracting JSON from the reply text.
    pub(super) structured: Option<serde_json::Value>,
    /// Billed cost in USD for the turn, when the provider reports it (claude's
    /// `result.total_cost_usd`). `None` for codex, which emits only token usage.
    pub(super) cost_usd: Option<f64>,
    /// Advisory observability issues from the streaming path. These never affect
    /// step success semantics.
    pub(super) warnings: Vec<String>,
}

/// Normalized token usage for one worker turn, provider-agnostic. Parsed from the
/// codex `turn.completed.usage` or the claude `result.usage` shape and reduced to
/// the three numbers the dashboard surfaces. `total` is `input + output` (codex's
/// `cached_input_tokens` is a SUBSET of `input_tokens`, not additive, and
/// `reasoning_output_tokens` is a SUBSET of `output_tokens`, so they are not
/// re-added here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

pub(super) fn codex_delivery_structured(reply: Option<&str>, spec: &LaunchSpec) -> Option<serde_json::Value> {
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

/// Classify WHY a step failed, into a stable `reason` tag the dashboard groups on.
/// Precedence: a fired timeout dominates (the worker never reached a clean turn);
/// then a non-zero / absent exit code; then a delivery that exited 0 but produced
/// no successful terminal event (`ok == false` with a clean exit == a delivery
/// problem, e.g. an auth/usage-limit `result` with `subtype != "success"`).
/// Returns `None` when the step succeeded.
pub(super) fn classify_failure_reason(
    ok: bool,
    exit_code: Option<i32>,
    timed_out: bool,
) -> Option<&'static str> {
    if ok {
        return None;
    }
    if timed_out {
        return Some("timeout");
    }
    match exit_code {
        // Clean exit (0) but the delivery still failed == a delivery-layer
        // problem: a `result`/turn that completed the process but reported no
        // successful turn (e.g. an auth or usage-limit terminal).
        Some(0) => Some("delivery"),
        // A non-zero code, or no code at all (killed by a signal), is a process
        // exit failure.
        _ => Some("exit"),
    }
}

pub(super) fn apply_codex_ephemeral_model_effort_service_tier_args(
    cmd: &mut Command,
    model: Option<&str>,
    effort: Option<&str>,
    service_tier: Option<&str>,
) {
    if let Some(model) = model {
        cmd.arg("-m").arg(model);
    }
    // Codex takes both reasoning effort and service tier as config overrides.
    if let Some(effort) = effort {
        cmd.arg("-c")
            .arg(format!("model_reasoning_effort={effort}"));
    }
    if let Some(tier) = service_tier {
        cmd.arg("-c").arg(format!("service_tier={tier}"));
    }
}

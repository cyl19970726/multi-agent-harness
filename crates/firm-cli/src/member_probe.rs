//! Daemon-side member progress probe — reads the provider-native session wire
//! and classifies the member's state without calling into the provider process.
//!
//! The classification feeds the supervisor daemon loop: PRODUCING members are
//! left alone, FAILING members generate a `member_distress` host attention,
//! and WAIT_LOOP members get a steer suggestion after ≥ 3 consecutive probes.
//!
//! Wire parsing is lightweight — tail of the JSONL file, no external process.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use harness_core::{MemberProbeClassification, NativeSessionRef};

use crate::{CliError, CliResult};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Probe result — the classification plus raw counts for display.
#[derive(Debug, Clone)]
pub struct MemberProbeResult {
    pub member_run_id: String,
    pub classification: MemberProbeClassification,
    /// Total JSONL lines parsed from the wire tail.
    pub lines_parsed: usize,
    /// Wire file path that was probed (None if file not found).
    pub wire_path: Option<PathBuf>,
}

/// Probe a member by reading its provider-native session wire.
///
/// `stale_threshold` is how long since the last file modification before
/// the session is considered DEAD. Default: 600 seconds (10 minutes).
pub fn probe_member(
    session: &NativeSessionRef,
    member_run_id: &str,
    tail_lines: usize,
    stale_threshold: Duration,
) -> CliResult<MemberProbeResult> {
    let wire_path = locate_wire(session)?;
    let wire_path = match wire_path {
        Some(p) => p,
        None => {
            return Ok(MemberProbeResult {
                member_run_id: member_run_id.to_string(),
                classification: MemberProbeClassification::Dead {
                    last_modified_secs_ago: 0,
                },
                lines_parsed: 0,
                wire_path: None,
            });
        }
    };

    // Check staleness first.
    if let Ok(metadata) = fs::metadata(&wire_path) {
        if let Ok(modified) = metadata.modified() {
            if let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                if elapsed > stale_threshold {
                    return Ok(MemberProbeResult {
                        member_run_id: member_run_id.to_string(),
                        classification: MemberProbeClassification::Dead {
                            last_modified_secs_ago: elapsed.as_secs(),
                        },
                        lines_parsed: 0,
                        wire_path: Some(wire_path),
                    });
                }
            }
        }
    }

    // Read the tail of the wire file.
    let lines = tail_jsonl(&wire_path, tail_lines)?;
    let lines_parsed = lines.len();

    // Parse tool calls from the tail.
    let calls = parse_tool_calls(&lines, &session.provider);

    // Classify.
    let classification = classify(&calls);

    Ok(MemberProbeResult {
        member_run_id: member_run_id.to_string(),
        classification,
        lines_parsed,
        wire_path: Some(wire_path),
    })
}

// ---------------------------------------------------------------------------
// Wire location
// ---------------------------------------------------------------------------

/// Locate the wire.jsonl for a provider-native session.
fn locate_wire(session: &NativeSessionRef) -> CliResult<Option<PathBuf>> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("HOME is unavailable for session discovery".into()))?;

    match session.provider.as_str() {
        "codex" => Ok(find_file(
            &std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"))
                .join("sessions"),
            &format!("{}.jsonl", session.native_session_id),
            5,
        )),
        "kimi" => Ok(find_kimi_wire(
            &home.join(".kimi-code").join("sessions"),
            &session.native_session_id,
            4,
        )),
        "claude" => Ok(find_file(
            &home.join(".claude").join("projects"),
            &format!("{}.jsonl", session.native_session_id),
            4,
        )),
        _ => Ok(None),
    }
}

fn find_file(root: &Path, suffix: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !root.is_dir() {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(suffix))
        {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_file(&path, suffix, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

fn find_kimi_wire(root: &Path, session_dir: &str, depth: usize) -> Option<PathBuf> {
    if depth == 0 || !root.is_dir() {
        return None;
    }
    let entries = fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let expected = if session_dir.starts_with("session_") {
                session_dir.to_string()
            } else {
                format!("session_{session_dir}")
            };
            if path.file_name().and_then(|n| n.to_str()) == Some(expected.as_str()) {
                let wire = path.join("agents").join("main").join("wire.jsonl");
                if wire.is_file() {
                    return Some(wire);
                }
            }
            if let Some(found) = find_kimi_wire(&path, session_dir, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Tail reader
// ---------------------------------------------------------------------------

/// Read the last `n` lines from a JSONL file efficiently by seeking from EOF.
fn tail_jsonl(path: &Path, n: usize) -> CliResult<Vec<serde_json::Value>> {
    let mut file = fs::File::open(path)?;
    let file_len = file.metadata()?.len();

    if file_len == 0 {
        return Ok(vec![]);
    }

    // Read up to 64 KB from the end — enough for n lines of typical JSONL.
    let chunk_size = (n as u64 * 512).min(file_len).max(4096);
    let seek_pos = file_len.saturating_sub(chunk_size);
    file.seek(SeekFrom::Start(seek_pos))?;

    let mut buf = String::new();
    let bytes_read = BufReader::new(&mut file).read_line(&mut buf)?;
    // If we seeked past the start, skip the partial first line.
    let content = if seek_pos > 0 && bytes_read > 0 {
        // Drop the first (partial) line; read the rest.
        let mut rest = String::new();
        BufReader::new(&mut file).read_line(&mut rest)?;
        // Actually, simpler: just re-read the whole tail chunk as lines.
        drop(file);
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let all_lines: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
        let start = all_lines.len().saturating_sub(n);
        all_lines.into_iter().skip(start).collect::<Vec<_>>()
    } else {
        // Small file — read all lines.
        drop(file);
        let file = fs::File::open(path)?;
        BufReader::new(file)
            .lines()
            .filter_map(|l| l.ok())
            .collect::<Vec<_>>()
    };

    let mut values = Vec::with_capacity(content.len());
    for line in &content {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            values.push(v);
        }
    }
    Ok(values)
}

// ---------------------------------------------------------------------------
// Tool call extraction
// ---------------------------------------------------------------------------

/// Parsed tool call from a provider wire record.
#[derive(Debug, Clone)]
struct ParsedToolCall {
    name: String,
    /// Whether this call succeeded (no error / non-zero exit).
    succeeded: bool,
    /// A fingerprint for detecting repeated identical calls.
    fingerprint: String,
}

/// Is this tool name an edit-capable tool?
fn is_edit_tool(name: &str) -> bool {
    matches!(
        name,
        "Write" | "Edit" | "Bash" | "write" | "edit" | "bash"
    )
}

/// Is this tool name an investigation tool (read / search / exec)?
fn is_investigation_tool(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "read"
        || lower == "glob"
        || lower == "grep"
        || lower == "bash"
        || lower == "websearch"
        || lower == "fetchurl"
        || name == "Read"
        || name == "Glob"
        || name == "Grep"
        || name == "WebSearch"
        || name == "FetchURL"
}

/// Parse tool calls from wire lines, provider-aware.
fn parse_tool_calls(lines: &[serde_json::Value], provider: &str) -> Vec<ParsedToolCall> {
    match provider {
        "codex" => parse_codex_tool_calls(lines),
        "kimi" => parse_kimi_tool_calls(lines),
        "claude" => parse_claude_tool_calls(lines),
        _ => vec![],
    }
}

/// Codex wire: tool calls are `response_item` rows with `function_call` /
/// `function_call_output` payloads.
fn parse_codex_tool_calls(lines: &[serde_json::Value]) -> Vec<ParsedToolCall> {
    // Two-pass: collect function_call names, then match outputs.
    let mut calls: Vec<(String, String)> = Vec::new(); // (name, fingerprint)
    let mut output_status: HashMap<String, bool> = HashMap::new(); // fingerprint -> success
    // Use index as a simple call_id since we process sequentially.
    let mut call_idx = 0u32;

    for line in lines {
        let row_type = line.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if row_type != "response_item" {
            continue;
        }
        let payload = match line.get("payload") {
            Some(p) => p,
            None => continue,
        };
        let payload_type = payload.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match payload_type {
            "function_call" => {
                let name = payload
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let args = payload
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let fingerprint = format!("{name}:{}", args.chars().take(120).collect::<String>());
                calls.push((name, fingerprint));
            }
            "function_call_output" => {
                // Codex function_call_output has "output" field; check for error indicators.
                let output_text = payload
                    .get("output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let failed = is_failed_output(output_text);
                // Associate with the most recent call (same index).
                if let Some(idx) = calls.len().checked_sub(1) {
                    output_status.insert(format!("{}", idx), !failed);
                }
                call_idx += 1;
            }
            _ => {}
        }
    }

    calls
        .into_iter()
        .enumerate()
        .map(|(idx, (name, fingerprint))| ParsedToolCall {
            succeeded: output_status.get(&format!("{}", idx)).copied().unwrap_or(true),
            name,
            fingerprint,
        })
        .collect()
}

/// Kimi wire: tool calls are `context.append_loop_event` rows with
/// `tool.call` / `tool.result` events.
fn parse_kimi_tool_calls(lines: &[serde_json::Value]) -> Vec<ParsedToolCall> {
    let mut calls: Vec<(String, String)> = Vec::new();
    let mut output_status: HashMap<String, bool> = HashMap::new();

    for line in lines {
        let row_type = line.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if row_type != "context.append_loop_event" {
            continue;
        }
        let event = match line.get("event") {
            Some(e) => e,
            None => continue,
        };
        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

        match event_type {
            "tool.call" => {
                let name = event
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let desc = event
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let fingerprint =
                    format!("{name}:{}", desc.chars().take(120).collect::<String>());
                calls.push((name, fingerprint));
            }
            "tool.result" => {
                // Kimi tool.result may have error indicators.
                let result_text = event
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let failed = is_failed_output(result_text);
                if let Some(idx) = calls.len().checked_sub(1) {
                    output_status.insert(format!("{}", idx), !failed);
                }
            }
            _ => {}
        }
    }

    calls
        .into_iter()
        .enumerate()
        .map(|(idx, (name, fingerprint))| ParsedToolCall {
            succeeded: output_status.get(&format!("{}", idx)).copied().unwrap_or(true),
            name,
            fingerprint,
        })
        .collect()
}

/// Claude wire: tool calls are `assistant` rows with `tool_use` / `tool_result` parts.
fn parse_claude_tool_calls(lines: &[serde_json::Value]) -> Vec<ParsedToolCall> {
    let mut calls: Vec<(String, String)> = Vec::new();
    let mut output_status: HashMap<String, bool> = HashMap::new();

    for line in lines {
        let row_type = line.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if row_type != "assistant" {
            continue;
        }
        let content = match line.pointer("/message/content") {
            Some(c) => c,
            None => continue,
        };
        let parts = match content.as_array() {
            Some(p) => p,
            None => continue,
        };

        for part in parts {
            match part.get("type").and_then(|v| v.as_str()) {
                Some("tool_use") => {
                    let name = part
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let input = part
                        .get("input")
                        .map(|v| v.to_string())
                        .unwrap_or_default();
                    let fingerprint =
                        format!("{name}:{}", input.chars().take(120).collect::<String>());
                    calls.push((name, fingerprint));
                }
                Some("tool_result") => {
                    let content_text = part
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let failed = is_failed_output(content_text);
                    if let Some(idx) = calls.len().checked_sub(1) {
                        output_status.insert(format!("{}", idx), !failed);
                    }
                }
                _ => {}
            }
        }
    }

    calls
        .into_iter()
        .enumerate()
        .map(|(idx, (name, fingerprint))| ParsedToolCall {
            succeeded: output_status.get(&format!("{}", idx)).copied().unwrap_or(true),
            name,
            fingerprint,
        })
        .collect()
}

/// Heuristic: does the tool output look like a failure?
fn is_failed_output(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let lower = text.to_lowercase();
    // Common error indicators in tool output.
    lower.contains("error")
        || lower.contains("failed")
        || lower.contains("command failed with exit code")
        || lower.contains("permission denied")
        || lower.contains("not found")
        || lower.contains("cannot")
        || lower.contains("refused")
        || lower.contains("timeout")
        || lower.contains("panic")
        || lower.contains("traceback")
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Classify a member from its parsed tool calls.
fn classify(calls: &[ParsedToolCall]) -> MemberProbeClassification {
    if calls.is_empty() {
        return MemberProbeClassification::Investigating {
            reads: 0,
            execs: 0,
            searches: 0,
        };
    }

    let edit_count = calls
        .iter()
        .filter(|c| is_edit_tool(&c.name))
        .count() as u32;

    let total = calls.len() as u32;
    let failed = calls.iter().filter(|c| !c.succeeded).count() as u32;

    let read_count = calls
        .iter()
        .filter(|c| {
            let n = &c.name;
            n == "Read" || n == "read"
        })
        .count() as u32;
    let exec_count = calls
        .iter()
        .filter(|c| {
            let n = &c.name;
            n == "Bash" || n == "bash"
        })
        .count() as u32;
    let search_count = calls
        .iter()
        .filter(|c| {
            let n = &c.name;
            n == "Glob" || n == "glob" || n == "Grep" || n == "grep" || n == "WebSearch"
                || n == "websearch" || n == "FetchURL" || n == "fetchurl"
        })
        .count() as u32;

    // Precedence: edits > 0 → PRODUCING
    if edit_count > 0 {
        return MemberProbeClassification::Producing { edit_count };
    }

    // Check for WAIT_LOOP: ≥ 5 repeated identical calls.
    let mut freq: HashMap<&str, u32> = HashMap::new();
    for call in calls {
        *freq.entry(&call.fingerprint).or_default() += 1;
    }
    let max_repeat = freq.values().max().copied().unwrap_or(0);
    if max_repeat >= 5 {
        let (repeated_call, count) = freq
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .unwrap_or(("unknown".to_string(), max_repeat));
        return MemberProbeClassification::WaitLoop {
            repeated_call: repeated_call.to_string(),
            repetition_count: count,
        };
    }

    // High fail rate + 0 edits → FAILING
    if total > 0 && failed > 0 && (failed as f64 / total as f64) >= 0.5 {
        return MemberProbeClassification::Failing {
            total_tool_calls: total,
            failed_tool_calls: failed,
        };
    }

    // Has investigation-like tool calls but no edits → INVESTIGATING
    MemberProbeClassification::Investigating {
        reads: read_count,
        execs: exec_count,
        searches: search_count,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Classification tests ──────────────────────────────────────────────

    #[test]
    fn empty_calls_is_investigating() {
        let classification = classify(&[]);
        assert_eq!(
            classification,
            MemberProbeClassification::Investigating {
                reads: 0,
                execs: 0,
                searches: 0,
            }
        );
    }

    #[test]
    fn edit_tool_is_producing() {
        let calls = vec![
            ParsedToolCall {
                name: "Read".into(),
                succeeded: true,
                fingerprint: "Read:/tmp/foo".into(),
            },
            ParsedToolCall {
                name: "Write".into(),
                succeeded: true,
                fingerprint: "Write:/tmp/bar".into(),
            },
        ];
        let classification = classify(&calls);
        assert_eq!(
            classification,
            MemberProbeClassification::Producing { edit_count: 1 }
        );
    }

    #[test]
    fn edit_tool_is_producing_even_with_failures() {
        let calls = vec![
            ParsedToolCall {
                name: "Bash".into(),
                succeeded: false,
                fingerprint: "Bash:fail".into(),
            },
            ParsedToolCall {
                name: "Edit".into(),
                succeeded: true,
                fingerprint: "Edit:fix".into(),
            },
        ];
        let classification = classify(&calls);
        assert!(matches!(
            classification,
            MemberProbeClassification::Producing { .. }
        ));
    }

    #[test]
    fn high_fail_rate_zero_edits_is_failing() {
        let calls = vec![
            ParsedToolCall {
                name: "Bash".into(),
                succeeded: false,
                fingerprint: "Bash:fail1".into(),
            },
            ParsedToolCall {
                name: "Bash".into(),
                succeeded: false,
                fingerprint: "Bash:fail2".into(),
            },
            ParsedToolCall {
                name: "Read".into(),
                succeeded: true,
                fingerprint: "Read:ok".into(),
            },
        ];
        let classification = classify(&calls);
        assert_eq!(
            classification,
            MemberProbeClassification::Failing {
                total_tool_calls: 3,
                failed_tool_calls: 2,
            }
        );
    }

    #[test]
    fn low_fail_rate_no_edits_is_investigating() {
        let calls = vec![
            ParsedToolCall {
                name: "Bash".into(),
                succeeded: false,
                fingerprint: "Bash:fail".into(),
            },
            ParsedToolCall {
                name: "Read".into(),
                succeeded: true,
                fingerprint: "Read:1".into(),
            },
            ParsedToolCall {
                name: "Grep".into(),
                succeeded: true,
                fingerprint: "Grep:1".into(),
            },
            ParsedToolCall {
                name: "Glob".into(),
                succeeded: true,
                fingerprint: "Glob:1".into(),
            },
        ];
        let classification = classify(&calls);
        // 1 failure / 4 total = 25% — below 50% threshold
        assert!(matches!(
            classification,
            MemberProbeClassification::Investigating { .. }
        ));
    }

    #[test]
    fn repeated_calls_is_wait_loop() {
        let mut calls = Vec::new();
        for i in 0..6 {
            calls.push(ParsedToolCall {
                name: "Read".into(),
                succeeded: true,
                fingerprint: format!("Read:same-args"),
            });
        }
        // Add a different call to have some variety
        calls.push(ParsedToolCall {
            name: "Bash".into(),
            succeeded: true,
            fingerprint: "Bash:other".into(),
        });

        let classification = classify(&calls);
        assert!(matches!(
            classification,
            MemberProbeClassification::WaitLoop { repetition_count: 6, .. }
        ));
    }

    #[test]
    fn repeated_below_5_is_not_wait_loop() {
        let mut calls = Vec::new();
        for _ in 0..4 {
            calls.push(ParsedToolCall {
                name: "Read".into(),
                succeeded: true,
                fingerprint: "Read:same".into(),
            });
        }
        let classification = classify(&calls);
        // 4 repeats < 5 threshold → not WaitLoop, falls through to Investigating
        assert!(matches!(
            classification,
            MemberProbeClassification::Investigating { .. }
        ));
    }

    #[test]
    fn all_successful_reads_is_investigating() {
        let calls = vec![
            ParsedToolCall {
                name: "Read".into(),
                succeeded: true,
                fingerprint: "Read:a".into(),
            },
            ParsedToolCall {
                name: "Glob".into(),
                succeeded: true,
                fingerprint: "Glob:b".into(),
            },
            ParsedToolCall {
                name: "Grep".into(),
                succeeded: true,
                fingerprint: "Grep:c".into(),
            },
        ];
        let classification = classify(&calls);
        assert_eq!(
            classification,
            MemberProbeClassification::Investigating {
                reads: 1,
                execs: 0,
                searches: 2,
            }
        );
    }

    // ── is_failed_output tests ─────────────────────────────────────────────

    #[test]
    fn detects_error_in_output() {
        assert!(is_failed_output("error: file not found"));
        assert!(is_failed_output("ERROR: something went wrong"));
        assert!(is_failed_output("command failed with exit code: 1"));
        assert!(is_failed_output("permission denied"));
        assert!(is_failed_output("not found"));
        assert!(is_failed_output("cannot connect"));
        assert!(is_failed_output("connection refused"));
        assert!(is_failed_output("timeout"));
        assert!(is_failed_output("panic at line 42"));
        assert!(is_failed_output("traceback (most recent call last)"));
    }

    #[test]
    fn normal_output_is_not_failed() {
        assert!(!is_failed_output(""));
        assert!(!is_failed_output("file written successfully"));
        assert!(!is_failed_output("ok"));
        assert!(!is_failed_output("42"));
        assert!(!is_failed_output("src/main.rs updated"));
    }

    // ── is_edit_tool tests ─────────────────────────────────────────────────

    #[test]
    fn edit_tools_recognized() {
        assert!(is_edit_tool("Write"));
        assert!(is_edit_tool("Edit"));
        assert!(!is_edit_tool("Read"));
        assert!(!is_edit_tool("Glob"));
    }

    // ── Codex wire parsing tests ───────────────────────────────────────────

    #[test]
    fn parse_codex_function_calls() {
        let lines: Vec<serde_json::Value> = vec![
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "name": "Read",
                    "arguments": "{\"path\":\"/tmp/foo\"}"
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "output": "line 1: hello"
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "name": "Write",
                    "arguments": "{\"path\":\"/tmp/bar\",\"content\":\"x\"}"
                }
            }),
        ];
        let calls = parse_codex_tool_calls(&lines);
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "Read");
        assert!(calls[0].succeeded);
        assert_eq!(calls[1].name, "Write");
    }

    #[test]
    fn parse_codex_failed_output() {
        let lines: Vec<serde_json::Value> = vec![
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call",
                    "name": "Bash",
                    "arguments": "{\"command\":\"rm -rf /\"}"
                }
            }),
            serde_json::json!({
                "type": "response_item",
                "payload": {
                    "type": "function_call_output",
                    "output": "command failed with exit code: 1"
                }
            }),
        ];
        let calls = parse_codex_tool_calls(&lines);
        assert_eq!(calls.len(), 1);
        assert!(!calls[0].succeeded);
    }

    // ── Kimi wire parsing tests ────────────────────────────────────────────

    #[test]
    fn parse_kimi_tool_calls() {
        let lines: Vec<serde_json::Value> = vec![
            serde_json::json!({
                "type": "context.append_loop_event",
                "event": {
                    "type": "tool.call",
                    "name": "Read",
                    "description": "reading /tmp/foo"
                }
            }),
            serde_json::json!({
                "type": "context.append_loop_event",
                "event": {
                    "type": "tool.result",
                    "description": "file contents here"
                }
            }),
        ];
        let calls = parse_kimi_tool_calls(&lines);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "Read");
        assert!(calls[0].succeeded);
    }
}

//! Live canary: Pi RPC protocol integration verification.
//!
//! This test spawns pi --mode rpc with a real pi binary and exercises the
//! full protocol round-trip: get_state → set_auto_compaction → prompt → agent_settled.
//! The prompt asks pi to write a small file to a temp dir, proving tool
//! execution works through the RPC interface.
//!
//! This test is gated behind the `pi-canary` feature so it doesn't run in
//! normal CI (which doesn't have pi installed).

#![cfg(feature = "pi-canary")]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn pi_rpc_handshake_and_basic_prompt() {
    let version = Command::new("pi")
        .arg("--version")
        .output()
        .expect("probe pi version");
    assert!(version.status.success(), "pi --version must succeed");
    let exact_version = String::from_utf8_lossy(&version.stdout).trim().to_string();
    assert_eq!(
        exact_version, "0.84.2",
        "this canary is evidence only for the reviewed Pi 0.84.2 RPC contract"
    );

    let tmp = unique_temp_dir();
    std::fs::create_dir_all(&tmp).expect("create temp dir");

    // The file-writing canary exercises the production trusted-FullAccess
    // composition, whose honest mapping passes no --tools restriction. The
    // separate deterministic tests cover ReadOnly argv and WorkspaceWrite
    // fail-closed admission.
    let mut child = Command::new("pi")
        .args([
            "--mode",
            "rpc",
            "--no-context-files",
            "--no-extensions",
            "--thinking",
            "off",
            "--session-dir",
        ])
        .arg(&tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pi --mode rpc");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut stdout = BufReader::new(stdout);

    // 1. get_state handshake
    writeln!(stdin, r#"{{"id":"t1","type":"get_state"}}"#).unwrap();
    stdin.flush().unwrap();

    let state_response =
        read_response(&mut stdout, "t1", Duration::from_secs(10)).expect("get_state response");

    let data = state_response.get("data").expect("get_state data field");
    let session_file = data
        .get("sessionFile")
        .and_then(|v| v.as_str())
        .expect("sessionFile");
    assert!(
        session_file.starts_with('/'),
        "sessionFile must be absolute: {session_file}"
    );
    let _auto_compaction = data
        .get("autoCompactionEnabled")
        .and_then(|v| v.as_bool())
        .expect("autoCompactionEnabled");

    // 2. Disable auto-compaction explicitly. The provider default is local
    // configuration, not an adapter capability claim.
    writeln!(
        stdin,
        r#"{{"id":"t2","type":"set_auto_compaction","enabled":false}}"#
    )
    .unwrap();
    stdin.flush().unwrap();
    let compaction_response = read_response(&mut stdout, "t2", Duration::from_secs(5))
        .expect("set_auto_compaction response");
    assert!(
        compaction_response
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        "set_auto_compaction should succeed"
    );

    // 3. Send a simple prompt
    let output_path = tmp.join("pi-canary.txt");
    let prompt = format!(
        r#"Write the word "ACCEPTED" to the file {}. Do nothing else. When done, write only: ## RESULT\nDONE\n## SUMMARY\nThe file was written."#,
        output_path.display()
    );

    writeln!(
        stdin,
        r#"{{"id":"t3","type":"prompt","message":{}}}"#,
        serde_json::to_string(&prompt).unwrap()
    )
    .unwrap();
    stdin.flush().unwrap();

    // 4. Read events until agent_settled
    let mut found_settled = false;
    let mut final_text = String::new();
    for line in stdout.by_ref().lines() {
        let line = line.expect("read line");
        let frame: serde_json::Value = serde_json::from_str(&line).expect("parse JSON frame");
        let event_type = frame.get("type").and_then(|v| v.as_str());

        match event_type {
            Some("response") => {
                let id = frame.get("id").and_then(|v| v.as_str());
                if id == Some("t3") {
                    let success = frame
                        .get("success")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    assert!(success, "prompt accepted: {frame}");
                }
            }
            Some("turn_end") => {
                if let Some(text) = extract_text(&frame) {
                    final_text = text;
                }
            }
            Some("agent_settled") => {
                found_settled = true;
                break;
            }
            _ => {}
        }
    }

    assert!(found_settled, "should receive agent_settled");

    // 5. Verify the file was actually written
    let content =
        std::fs::read_to_string(&output_path).expect("pi should have written the output file");
    assert!(
        content.contains("ACCEPTED"),
        "file should contain ACCEPTED, got: {content}"
    );

    // 6. Verify final report text
    assert!(
        final_text.contains("DONE") || final_text.contains("SUMMARY"),
        "final text should contain RESULT/SUMMARY: {final_text}"
    );

    assert_native_session_has_no_thinking(Path::new(session_file));

    // 7. Observe the exact settled postcondition. `prompt success` only means
    // admission; `agent_settled` plus this passive state read proves this
    // execution cycle has no queued continuation left.
    writeln!(stdin, r#"{{"id":"t4","type":"get_state"}}"#).unwrap();
    stdin.flush().unwrap();
    let settled_state = read_response(&mut stdout, "t4", Duration::from_secs(5))
        .expect("settled get_state response");
    let settled_data = settled_state.get("data").expect("settled state data");
    assert_eq!(
        settled_data
            .get("isStreaming")
            .and_then(|value| value.as_bool()),
        Some(false),
        "agent_settled must be followed by an observed idle runtime"
    );
    assert_eq!(
        settled_data
            .get("pendingMessageCount")
            .and_then(|value| value.as_u64()),
        Some(0),
        "the canary must not leave native steering/follow-up input queued"
    );

    // 8. Release the live runtime. The native JSONL is provider-owned memory:
    // releasing the process must retain it until this isolated test fixture is
    // explicitly cleaned up.
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        Path::new(session_file).is_file(),
        "releasing Pi must retain the native session JSONL"
    );
    std::fs::remove_dir_all(&tmp).expect("remove temp dir");

    eprintln!("✅ Pi RPC live canary passed");
    eprintln!("   provider_version: {exact_version}");
    eprintln!("   session_file: {session_file}");
    eprintln!("   final_text: {final_text}");
}

fn unique_temp_dir() -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!("harness-pi-canary-{}-{nonce}", std::process::id()))
}

fn assert_native_session_has_no_thinking(path: &Path) {
    let raw = std::fs::read_to_string(path).expect("read Pi native session");
    for (index, line) in raw.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|error| panic!("parse native session line {}: {error}", index + 1));
        assert!(
            !contains_persisted_thinking(&value),
            "Pi native session line {} persisted thinking",
            index + 1
        );
    }
}

fn contains_persisted_thinking(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            map.get("type").and_then(serde_json::Value::as_str) == Some("thinking")
                || map.contains_key("thinkingSignature")
                || map.values().any(contains_persisted_thinking)
        }
        serde_json::Value::Array(values) => values.iter().any(contains_persisted_thinking),
        _ => false,
    }
}

/// Read one response frame with a specific id from an already-buffered RPC
/// stream. Reusing one reader is required: constructing a new `BufReader` for
/// every response can drop frames that were read ahead from the child.
fn read_response<R: BufRead>(
    reader: &mut R,
    expected_id: &str,
    timeout: Duration,
) -> Option<serde_json::Value> {
    let start = std::time::Instant::now();
    let mut line = String::new();

    while start.elapsed() < timeout {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {}
        }
        let frame: serde_json::Value = match serde_json::from_str(line.trim_end()) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let id = frame.get("id").and_then(|v| v.as_str());
        let is_response = frame.get("type").and_then(|v| v.as_str()) == Some("response");

        if is_response && id == Some(expected_id) {
            return Some(frame);
        }
    }
    None
}

/// Extract text from a pi turn_end message content blocks.
fn extract_text(frame: &serde_json::Value) -> Option<String> {
    let message = frame.get("message")?;
    let content = message.get("content")?.as_array()?;
    let mut text = String::new();
    for block in content {
        if block.get("type")?.as_str()? == "text" {
            if let Some(t) = block.get("text").and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(t);
            }
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

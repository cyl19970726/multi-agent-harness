use super::{
    confirm_pi_session_flush, ensure_session_has_no_persisted_thinking,
    value_contains_persisted_thinking, PermissionCeiling, PiRpcClient,
};

#[test]
fn prompt_and_agent_settled_share_one_provider_cycle_identity() {
    let dir = std::env::temp_dir().join(format!(
        "pi-rpc-cycle-correlation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let session_file = dir.join("session.jsonl");
    std::fs::write(&session_file, "{\"type\":\"agent_start\"}\n").unwrap();
    let shim = dir.join("pi");
    let script = format!(
        r##"#!/usr/bin/env python3
import sys, json
state_calls = 0
for line in sys.stdin:
    cmd = json.loads(line)
    cid = cmd.get('id')
    kind = cmd.get('type')
    if kind == 'get_state':
        state_calls += 1
        if state_calls == 1:
            print(json.dumps({{'type':'agent_settled', 'stale':True}}), flush=True)
        print(json.dumps({{'id': cid, 'type':'response', 'command':'get_state', 'success':True, 'data':{{'sessionFile':'{session_file}', 'autoCompactionEnabled':False, 'isStreaming':False, 'pendingMessageCount':0, 'steeringMode':'one-at-a-time', 'followUpMode':'one-at-a-time'}}}}), flush=True)
    elif kind == 'prompt':
        print(json.dumps({{'id': cid, 'type':'response', 'command':'prompt', 'success':True}}), flush=True)
        print(json.dumps({{'type':'turn_end', 'message':{{'content':[{{'type':'text', 'text':'done'}}]}}}}), flush=True)
        print(json.dumps({{'type':'agent_settled'}}), flush=True)
"##,
        session_file = session_file.display(),
    );
    std::fs::write(&shim, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).unwrap();
    }
    let mut client = PiRpcClient::spawn(
        shim.to_str().unwrap(),
        super::PiSpawnOptions {
            cwd: &dir,
            model: None,
            resume_session_file: None,
            session_dir: &dir,
            member_name: "cycle-correlation",
            collaboration_env: &[],
            tools: None,
            permission_ceiling: PermissionCeiling::FullAccess,
        },
    )
    .unwrap();
    let outcome = client
        .prompt(
            "one cycle",
            std::time::Duration::from_secs(2),
            |_| Ok(()),
            |_, _| Ok(()),
            |_| {},
            harness_runtime_contract::CycleControl::default,
        )
        .unwrap();
    assert_eq!(
        outcome.final_text, "done",
        "stale pre-dispatch idle was ignored"
    );
    let correlation = outcome.native_correlation;
    assert_eq!(
        correlation.terminal_provider_input_id.as_deref(),
        Some(correlation.provider_input_id.as_str())
    );
    assert_eq!(
        correlation.exact_terminal_ref.as_deref(),
        Some(format!("pi.agent_settled:{}", correlation.provider_input_id).as_str())
    );
    drop(client);
    std::fs::remove_dir_all(dir).unwrap();
}

/// Spawn a minimal fake `pi --mode rpc` shim and exercise the RPC-level
/// adapter surface: handshake, follow_up acknowledgement, queue
/// observation, and the --tools permission compilation in the spawn argv.
#[test]
fn follow_up_queue_snapshot_and_tools_compilation() {
    let dir = std::env::temp_dir().join(format!(
        "pi-rpc-rpc-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let session_file = dir.join("session.jsonl");
    std::fs::write(&session_file, "{\"type\":\"agent_start\"}\n").unwrap();
    let args_marker = dir.join("argv.json");
    let shim = dir.join("pi");
    let script = format!(
        r##"#!/usr/bin/env python3
import sys, json, os
with open('{args_marker}', 'w') as f:
    json.dump(sys.argv[1:], f)
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        cmd = json.loads(line)
    except json.JSONDecodeError:
        continue
    t = cmd.get('type', '')
    cid = cmd.get('id', '')
    if t == 'get_state':
        resp = {{'id': cid, 'type': 'response', 'command': 'get_state', 'success': True,
                 'data': {{'sessionFile': '{session_file}', 'autoCompactionEnabled': False,
                           'steeringMode': 'one-at-a-time', 'followUpMode': 'one-at-a-time',
                           'pendingMessageCount': 2, 'isStreaming': False}}}}
    elif t == 'follow_up':
        resp = {{'id': cid, 'type': 'response', 'command': 'follow_up', 'success': True}}
    else:
        resp = {{'id': cid, 'type': 'response', 'command': t, 'success': True}}
    print(json.dumps(resp), flush=True)
"##,
        args_marker = args_marker.display(),
        session_file = session_file.display(),
    );
    std::fs::write(&shim, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&shim).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&shim, perms).unwrap();
    }

    let mut client = PiRpcClient::spawn(
        shim.to_str().unwrap(),
        super::PiSpawnOptions {
            cwd: &dir,
            model: None,
            resume_session_file: None,
            session_dir: &dir,
            member_name: "rpc-test",
            collaboration_env: &[],
            tools: Some("read,grep,find,ls"),
            permission_ceiling: PermissionCeiling::ReadOnly,
        },
    )
    .expect("spawn shim");

    // Permission compilation proof: the allowlist is in the process argv.
    let argv: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(&args_marker).unwrap()).unwrap();
    let tools_pos = argv.iter().position(|arg| arg == "--tools");
    assert_eq!(
        tools_pos.map(|pos| argv[pos + 1].as_str()),
        Some("read,grep,find,ls"),
        "restricted ceiling must compile to --tools in the spawn argv: {argv:?}"
    );

    let ack = client.follow_up("queued at the native boundary").unwrap();
    assert_eq!(ack.get("success").and_then(|v| v.as_bool()), Some(true));

    let snapshot = client.queue_snapshot().unwrap();
    assert_eq!(
        snapshot["pending_message_count"].as_u64(),
        Some(2),
        "queue observation must surface the native pending count: {snapshot}"
    );
    assert_eq!(snapshot["steering_mode"].as_str(), Some("one-at-a-time"));

    let (children, children_evidence) = client.writable_children_drain_proof();
    assert_eq!(
        children,
        harness_core::agentfirm_api::RuntimePostconditionStatus::Satisfied,
        "reviewed ReadOnly argv proves writable-child non-creation: {children_evidence}"
    );
    let flush = confirm_pi_session_flush(&session_file)
        .expect("a complete JSONL line must receive file+directory sync evidence");
    assert!(flush.contains("sync_all confirmed"), "{flush}");

    drop(client);

    let full_access = PiRpcClient::spawn(
        shim.to_str().unwrap(),
        super::PiSpawnOptions {
            cwd: &dir,
            model: None,
            resume_session_file: None,
            session_dir: &dir,
            member_name: "rpc-full-access-test",
            collaboration_env: &[],
            tools: None,
            permission_ceiling: PermissionCeiling::FullAccess,
        },
    )
    .expect("spawn FullAccess shim");
    let (children, children_evidence) = full_access.writable_children_drain_proof();
    assert_eq!(
        children,
        harness_core::agentfirm_api::RuntimePostconditionStatus::Unknown,
        "FullAccess cannot claim child drain without a native job inventory: {children_evidence}"
    );
    assert!(children_evidence.contains("may escape the owned process group"));
    drop(full_access);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn flush_evidence_requires_a_complete_regular_jsonl_file() {
    let dir = std::env::temp_dir().join(format!(
        "pi-rpc-flush-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let session_file = dir.join("session.jsonl");
    std::fs::write(&session_file, "{\"type\":\"session\"}").expect("write incomplete session");
    let error = confirm_pi_session_flush(&session_file)
        .expect_err("path existence without a complete record is not flush proof");
    assert!(error.contains("incomplete final JSONL record"));

    std::fs::write(&session_file, "{\"type\":\"session\"}\n").expect("complete session");
    confirm_pi_session_flush(&session_file).expect("complete file can be durably synced");

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let linked = dir.join("linked-session.jsonl");
        symlink(&session_file, &linked).expect("create symlink fixture");
        let error = confirm_pi_session_flush(&linked)
            .expect_err("a symlink must not be promoted to native flush evidence");
        assert!(error.contains("regular non-symlink"));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn detects_persisted_thinking_blocks_without_rejecting_level_metadata() {
    assert!(value_contains_persisted_thinking(&serde_json::json!({
        "type": "message",
        "message": {"content": [{"type": "thinking", "thinking": "private"}]}
    })));
    assert!(value_contains_persisted_thinking(&serde_json::json!({
        "type": "message",
        "message": {"content": [{"type": "text", "thinkingSignature": "sig"}]}
    })));
    assert!(!value_contains_persisted_thinking(&serde_json::json!({
        "type": "thinking_level_change",
        "thinkingLevel": "off"
    })));
}

#[test]
fn rejects_a_native_session_that_would_replay_thinking() {
    let dir = std::env::temp_dir().join(format!(
        "harness-pi-rpc-thinking-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("session.jsonl");
    std::fs::write(
            &path,
            "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"content\":[{\"type\":\"thinking\",\"thinking\":\"private\"}]}}\n",
        )
        .expect("write session");
    let error = ensure_session_has_no_persisted_thinking(&path).unwrap_err();
    assert!(error.to_string().contains("persisted provider thinking"));
    std::fs::remove_dir_all(dir).expect("remove temp dir");
}

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::time::{SystemTime, UNIX_EPOCH};

use harness_core::{
    MemberActionStatus, RegistryDeliveryStatus, RegistryMessage, RegistryMessageIntent, SenderKind,
    WorkflowRunStatus, WorkflowStepStatus,
};

use super::*;

/// A fixed project id used by the single-project unit tests below; the
/// multi-project leakage coverage lives in tests/serve_sse_projects.rs.
const TEST_PID: &str = "_test";

fn unique_dir(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "harness-sse-test-{tag}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ))
}

fn test_message(id: &str) -> RegistryMessage {
    RegistryMessage {
        id: id.into(),
        task_id: Some("task-1".into()),
        from_agent_id: "leader".into(),
        to_agent_id: Some("agent-1".into()),
        channel: Some("assignment".into()),
        kind: RegistryMessageIntent::Message,
        delivery_status: RegistryDeliveryStatus::Queued,
        content: "Do the task".into(),
        evidence_ids: Vec::new(),
        created_at: "unix-ms:1".into(),
        delivery: None,
        sender_kind: SenderKind::Agent,
    }
}

fn test_workflow_run(id: &str) -> WorkflowRun {
    WorkflowRun {
        id: id.into(),
        workflow_name: "test".into(),
        project_binding_id: None,
        status: WorkflowRunStatus::Running,
        step_ids: Vec::new(),
        created_at: "unix-ms:1".into(),
        ended_at: None,
        summary: None,
        args: None,
        agents_spawned: 0,
        final_output: None,
        initiated_by: None,
        design_intent: None,
        spec: None,
        host_pid: None,
        dry_run: false,
        terminal_reason: None,
        partial_output_available: false,
    }
}

fn test_workflow_step(id: &str, run_id: &str) -> WorkflowStep {
    WorkflowStep {
        id: id.into(),
        run_id: run_id.into(),
        phase: "test".into(),
        label: "test-step".into(),
        native_session: None,
        status: WorkflowStepStatus::Running,
        output_summary: None,
        result: None,
        started_at: "unix-ms:1".into(),
        ended_at: None,
        terminal_reason: None,
        partial: false,
    }
}

fn message_frame(line: &str) -> Vec<SseEventFrame> {
    serde_json::from_str::<RegistryMessage>(line)
        .ok()
        .map(SseEventFrame::RegistryMessage)
        .into_iter()
        .collect()
}

fn workflow_run_frame(line: &str) -> Vec<SseEventFrame> {
    serde_json::from_str::<WorkflowRun>(line)
        .ok()
        .map(SseEventFrame::WorkflowRun)
        .into_iter()
        .collect()
}

fn workflow_step_frame(line: &str) -> Vec<SseEventFrame> {
    serde_json::from_str::<WorkflowStep>(line)
        .ok()
        .map(SseEventFrame::WorkflowStep)
        .into_iter()
        .collect()
}

fn test_member_action(id: &str) -> MemberAction {
    MemberAction {
        id: id.into(),
        seq: 1,
        team_run_id: "trun-1".into(),
        member_run_id: "mrun-1".into(),
        task_id: None,
        provider_call_id: None,
        action_type: "command_completed".into(),
        status: MemberActionStatus::Succeeded,
        provider_status: None,
        semantic_status: None,
        title: "Ran focused checks".into(),
        summary: "Focused checks passed".into(),
        evidence_refs: Vec::new(),
        started_at: "unix-ms:1".into(),
        completed_at: Some("unix-ms:2".into()),
    }
}

/// A JSONL row whose write is observed in two pieces (the watcher polls
/// after only the first half has hit the file) must be delivered exactly
/// once — never dropped as a torn line, never duplicated when it completes.
#[test]
fn torn_record_split_across_polls_delivered_exactly_once() {
    let root = unique_dir("torn");
    std::fs::create_dir_all(&root).expect("create root");
    let path = root.join("messages.jsonl");

    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let mut offsets: HashMap<(String, String), u64> = HashMap::new();

    // Two full rows as the store would write them: compact JSON + '\n'.
    let row_a = serde_json::to_string(&test_message("message-a")).expect("ser a");
    let row_b = serde_json::to_string(&test_message("message-b")).expect("ser b");
    let full = format!("{row_a}\n{row_b}\n");
    let bytes = full.as_bytes();

    // Split point lands mid-way through row_b (after row_a's newline), so
    // the first poll sees a complete row_a plus a torn fragment of row_b.
    let split = row_a.len() + 1 + (row_b.len() / 2);

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("open");
    file.write_all(&bytes[..split]).expect("write first half");
    file.flush().expect("flush first half");

    // Poll 1: row_a delivered, row_b fragment buffered (offset not advanced
    // past it).
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );

    // Poll 1.5: nothing new on disk, the torn fragment must NOT be emitted.
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );

    // Complete row_b.
    file.write_all(&bytes[split..]).expect("write second half");
    file.flush().expect("flush second half");

    // Poll 2: row_b now complete and delivered exactly once.
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );

    // Poll 3: idempotent — no re-delivery.
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );

    let mut received = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        match frame {
            SseEventFrame::ProjectionInvalidated(invalidation) => {
                assert_eq!(invalidation.ledger, "messages.jsonl");
                received.push(invalidation.revision)
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }

    assert_eq!(
        received,
        vec![1, 2],
        "each completed append invalidates exactly once; torn fragments do not"
    );

    std::fs::remove_dir_all(&root).expect("cleanup");
}

/// The complete-line path must broadcast each appended row once and advance
/// past them so a follow-up poll with no new bytes emits nothing.
#[test]
fn complete_rows_broadcast_once_and_offset_advances() {
    let root = unique_dir("complete");
    std::fs::create_dir_all(&root).expect("create root");
    let path = root.join("messages.jsonl");

    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let mut offsets: HashMap<(String, String), u64> = HashMap::new();

    let row = serde_json::to_string(&test_message("message-once")).expect("ser");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("open");
    file.write_all(format!("{row}\n").as_bytes())
        .expect("write");
    file.flush().expect("flush");

    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );

    let mut count = 0;
    while rx.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(
        count, 1,
        "complete row broadcast exactly once across two polls"
    );

    std::fs::remove_dir_all(&root).expect("cleanup");
}

/// The generalized append parser must preserve the old single-frame file
/// behavior: valid rows emit one frame, malformed rows emit zero frames.
#[test]
fn single_frame_rows_still_emit_one_and_parse_failures_emit_zero() {
    let root = unique_dir("single-frame");
    std::fs::create_dir_all(&root).expect("create root");
    let path = root.join("messages.jsonl");

    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let mut offsets: HashMap<(String, String), u64> = HashMap::new();

    let row = serde_json::to_string(&test_message("message-valid")).expect("ser");
    std::fs::write(&path, format!("{row}\nnot-json\n")).expect("write rows");

    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );

    let mut received = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        match frame {
            SseEventFrame::ProjectionInvalidated(invalidation) => {
                received.push(invalidation.ledger)
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }

    assert_eq!(received, vec!["messages.jsonl".to_string()]);

    std::fs::remove_dir_all(&root).expect("cleanup");
}

/// A store file that SHRINKS (lease compaction rewrites it in place) must not
/// silence the watcher. Regression found by an independent reviewer on the
/// lease-compaction change: the grew-only guard `current_size <= consumed`
/// meant a 23 MB lease file compacted to a few hundred bytes would emit
/// nothing until it regrew past 23 MB.
#[test]
fn compacted_file_is_rebroadcast_rather_than_silently_skipped() {
    let root = unique_dir("compaction-truncate");
    std::fs::create_dir_all(&root).expect("create root");
    let path = root.join("messages.jsonl");

    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let mut offsets: HashMap<(String, String), u64> = HashMap::new();

    // Grow the file well past what the compacted version will occupy.
    let mut grown = String::new();
    // Stay under the bounded(100) subscriber channel: an overflowing
    // try_send drops the client from the manager and the test would then
    // measure nothing rather than the truncation behaviour.
    for index in 0..50 {
        let row = serde_json::to_string(&test_message(&format!("message-{index}"))).expect("ser");
        grown.push_str(&row);
        grown.push('\n');
    }
    std::fs::write(&path, &grown).expect("write grown rows");
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );
    while rx.try_recv().is_ok() {}
    let consumed_before = offsets
        .get(&(TEST_PID.to_string(), "messages.jsonl".to_string()))
        .copied()
        .expect("offset recorded");
    assert!(consumed_before > 0);

    // Compaction: same file, far smaller, carrying current state.
    let compacted = serde_json::to_string(&test_message("message-after-compaction")).expect("ser");
    std::fs::write(&path, format!("{compacted}\n")).expect("write compacted");
    assert!(
        std::fs::metadata(&path).expect("meta").len() < consumed_before,
        "compacted file must be smaller than the consumed offset"
    );

    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );

    let mut received = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        match frame {
            SseEventFrame::ProjectionInvalidated(invalidation) => {
                received.push(invalidation.ledger)
            }
            other => panic!("unexpected frame {other:?}"),
        }
    }
    assert_eq!(
        received,
        vec!["messages.jsonl".to_string()],
        "post-compaction state must invalidate connected clients"
    );

    std::fs::remove_dir_all(&root).expect("cleanup");
}

/// Workflow runs and steps should be streamed via SSE like other events (WP2).
#[test]
fn workflow_run_and_step_broadcast_exactly_once() {
    let root = unique_dir("workflow");
    std::fs::create_dir_all(&root).expect("create root");
    let run_path = root.join("workflow_runs.jsonl");
    let step_path = root.join("workflow_steps.jsonl");

    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let mut offsets: HashMap<(String, String), u64> = HashMap::new();

    // Write a workflow run and a step
    let run = test_workflow_run("run-1");
    let step = test_workflow_step("step-1", "run-1");
    let run_row = serde_json::to_string(&run).expect("ser run");
    let step_row = serde_json::to_string(&step).expect("ser step");

    let mut run_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&run_path)
        .expect("open run");
    run_file
        .write_all(format!("{run_row}\n").as_bytes())
        .expect("write run");
    run_file.flush().expect("flush run");

    let mut step_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&step_path)
        .expect("open step");
    step_file
        .write_all(format!("{step_row}\n").as_bytes())
        .expect("write step");
    step_file.flush().expect("flush step");

    // Poll both files
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "workflow_runs.jsonl",
        &mut offsets,
        workflow_run_frame,
        &manager,
    );
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "workflow_steps.jsonl",
        &mut offsets,
        workflow_step_frame,
        &manager,
    );

    let mut ledgers = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        match frame {
            SseEventFrame::ProjectionInvalidated(invalidation) => ledgers.push(invalidation.ledger),
            other => panic!("unexpected frame {other:?}"),
        }
    }

    assert_eq!(ledgers, ["workflow_runs.jsonl", "workflow_steps.jsonl"]);

    std::fs::remove_dir_all(&root).expect("cleanup");
}

/// Member actions are durable Agent Team execution records. They must take
/// the same project-scoped tail path as the attempt/member/message rows so
/// a background HTTP start updates an already-open console without polling.
#[test]
fn member_action_broadcasts_once_and_stays_project_scoped() {
    let root = unique_dir("member-action");
    std::fs::create_dir_all(&root).expect("create root");
    let path = root.join("member_actions.jsonl");
    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let other_project_rx = manager.subscribe("other-project");
    let mut offsets: HashMap<(String, String), u64> = HashMap::new();

    let row = serde_json::to_string(&test_member_action("mact-1")).expect("serialize");
    let mut legacy_thinking = test_member_action("mact-thinking");
    legacy_thinking.action_type = "thinking".into();
    let thinking_row = serde_json::to_string(&legacy_thinking).expect("serialize thinking");
    std::fs::write(&path, format!("{row}\n{thinking_row}\n")).expect("write rows");

    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "member_actions.jsonl",
        &mut offsets,
        member_action_frames,
        &manager,
    );
    check_and_broadcast_appends(
        TEST_PID,
        &root,
        "member_actions.jsonl",
        &mut offsets,
        member_action_frames,
        &manager,
    );

    match rx.try_recv() {
        Ok(SseEventFrame::ProjectionInvalidated(invalidation)) => {
            assert_eq!(invalidation.ledger, "member_actions.jsonl")
        }
        other => panic!("expected member action frame, got {other:?}"),
    }
    assert!(
        rx.try_recv().is_err(),
        "action must broadcast exactly once and thinking rows must not emit"
    );
    assert!(
        other_project_rx.try_recv().is_err(),
        "member action must not cross project subscriptions"
    );
    assert!(offsets.contains_key(&(TEST_PID.to_string(), "member_actions.jsonl".to_string())));

    std::fs::remove_dir_all(&root).expect("cleanup");
}

/// A frame broadcast to project A must reach A's subscriber and NOT B's, and
/// the offset map keys by (project, filename) so two projects with the same
/// filename are independent (multi-project P6 leakage guard).
#[test]
fn broadcast_is_isolated_per_project() {
    let manager = SseManager::new();
    let rx_a = manager.subscribe("proj-a");
    let rx_b = manager.subscribe("proj-b");

    manager.broadcast(
        "proj-a",
        SseEventFrame::RegistryMessage(test_message("only-a")),
    );

    // A receives it.
    match rx_a.try_recv() {
        Ok(SseEventFrame::RegistryMessage(m)) => assert_eq!(m.id, "only-a"),
        other => panic!("project A should receive its own frame, got {other:?}"),
    }
    // B receives nothing.
    assert!(
        rx_b.try_recv().is_err(),
        "project B must not see project A's frame"
    );
    assert_eq!(manager.client_count("proj-a"), 1);
    assert_eq!(manager.client_count("proj-b"), 1);
}

/// Identical filenames across two coordination stores are tracked independently:
/// appending to A's `messages.jsonl` advances only A's offset and broadcasts
/// only to A.
#[test]
fn offsets_and_broadcasts_independent_across_projects() {
    let root_a = unique_dir("iso-a");
    let root_b = unique_dir("iso-b");
    std::fs::create_dir_all(&root_a).expect("a");
    std::fs::create_dir_all(&root_b).expect("b");

    let manager = SseManager::new();
    let rx_a = manager.subscribe("proj-a");
    let rx_b = manager.subscribe("proj-b");
    let mut offsets: HashMap<(String, String), u64> = HashMap::new();

    // Write a row only into project A's messages.jsonl.
    let row = serde_json::to_string(&test_message("a-row")).expect("ser");
    std::fs::write(root_a.join("messages.jsonl"), format!("{row}\n")).expect("write a");

    check_and_broadcast_appends(
        "proj-a",
        &root_a,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );
    // Project B has no such file → no-op, no offset entry.
    check_and_broadcast_appends(
        "proj-b",
        &root_b,
        "messages.jsonl",
        &mut offsets,
        message_frame,
        &manager,
    );

    match rx_a.try_recv() {
        Ok(SseEventFrame::ProjectionInvalidated(invalidation)) => {
            assert_eq!(invalidation.ledger, "messages.jsonl")
        }
        other => panic!("A should receive its row, got {other:?}"),
    }
    assert!(rx_b.try_recv().is_err(), "B must not see A's row");

    // A's offset advanced; B's is absent (no file to read).
    assert!(offsets.contains_key(&("proj-a".to_string(), "messages.jsonl".to_string())));
    assert!(!offsets.contains_key(&("proj-b".to_string(), "messages.jsonl".to_string())));

    std::fs::remove_dir_all(&root_a).expect("cleanup a");
    std::fs::remove_dir_all(&root_b).expect("cleanup b");
}

/// Current Mission and Agent Team ledgers are tail-able sources for the
/// console read model. Legacy Wave rows are snapshot-only historical data
/// and never invalidate current coordination state.
#[test]
fn native_mission_and_team_ledgers_emit_typed_frames() {
    let root = unique_dir("native-ledgers");
    std::fs::create_dir_all(&root).expect("create root");
    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let other_project_rx = manager.subscribe("other-project");
    let mut offsets: HashMap<(String, String), u64> = HashMap::new();

    let rows = [
        (
            "missions.jsonl",
            include_str!("../../../schemas/fixtures/mission/valid/basic.json"),
        ),
        (
            "team_runs.jsonl",
            include_str!("../../../schemas/fixtures/agent-team-run/valid/basic.json"),
        ),
    ];
    for (filename, row) in rows {
        // Fixture files are pretty-printed JSON, whereas a JSONL ledger has
        // one compact record per physical line.
        let compact = serde_json::from_str::<serde_json::Value>(row)
            .expect("fixture JSON")
            .to_string();
        std::fs::write(root.join(filename), format!("{compact}\n")).expect("write row");
    }

    poll_project(TEST_PID, &root, &mut offsets, &manager);

    let mut ledgers = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        match frame {
            SseEventFrame::ProjectionInvalidated(invalidation) => ledgers.push(invalidation.ledger),
            other => panic!("unexpected native-ledger frame {other:?}"),
        }
    }

    assert_eq!(ledgers, vec!["missions.jsonl", "team_runs.jsonl"]);
    for (filename, _) in rows {
        assert!(
            offsets.contains_key(&(TEST_PID.to_string(), filename.to_string())),
            "native ledger {filename} must receive a project-scoped offset"
        );
    }
    assert!(
        other_project_rx.try_recv().is_err(),
        "native ledger frames must stay inside their subscribed project"
    );

    std::fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn scoped_invalidations_do_not_leak_between_spaces_or_companies() {
    let manager = SseManager::new();
    let a_x = manager.subscribe_scoped("space-a", Some("company-x"));
    let a_y = manager.subscribe_scoped("space-a", Some("company-y"));
    let b_x = manager.subscribe_scoped("space-b", Some("company-x"));
    let b_y = manager.subscribe_scoped("space-b", Some("company-y"));

    manager.invalidate_company("company-x", "company_os_milestones.jsonl", "append");
    for rx in [&a_x, &b_x] {
        match rx.try_recv() {
            Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
                assert_eq!(frame.scope, "company");
                assert_eq!(frame.scope_id, "company-x");
                assert_eq!(frame.revision, 1);
                assert_eq!(frame.stream_epoch, manager.stream_epoch());
            }
            other => panic!("company-x subscriber missing invalidation: {other:?}"),
        }
    }
    assert!(
        a_y.try_recv().is_err(),
        "company X leaked to space A/company Y"
    );
    assert!(
        b_y.try_recv().is_err(),
        "company X leaked to space B/company Y"
    );

    manager.invalidate_execution_space("space-a", "work_operations.jsonl", "append");
    for rx in [&a_x, &a_y] {
        match rx.try_recv() {
            Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
                assert_eq!(frame.scope, "execution_space");
                assert_eq!(frame.scope_id, "space-a");
            }
            other => panic!("space-a subscriber missing invalidation: {other:?}"),
        }
    }
    assert!(
        b_x.try_recv().is_err(),
        "space A leaked to space B/company X"
    );
    assert!(
        b_y.try_recv().is_err(),
        "space A leaked to space B/company Y"
    );
}

#[test]
fn snapshot_only_execution_ledgers_have_an_invalidation_path() {
    for ledger in [
        "teams.jsonl",
        "provider_launch_profiles.jsonl",
        "durable_agent_provider_launch_profiles.jsonl",
        "provider_processes.jsonl",
        "evidence.jsonl",
        "provider_child_threads.jsonl",
        "workflow_patches.jsonl",
        "workflow_artifact_manifests.jsonl",
        "delegation_runs.jsonl",
        "work_operations.jsonl",
        "work_delivery_updates.jsonl",
    ] {
        assert!(
            EXECUTION_INVALIDATION_FILES.contains(&ledger),
            "snapshot-visible ledger {ledger} has neither a typed delta nor invalidation"
        );
    }
}

#[test]
fn invalidation_watcher_handles_complete_appends_truncation_and_atomic_replace() {
    let root = unique_dir("projection-invalidation");
    std::fs::create_dir_all(&root).expect("create root");
    let path = root.join("work_operations.jsonl");
    std::fs::write(&path, "{\"v\":1}\n").expect("seed ledger");

    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let mut states = HashMap::new();
    seed_invalidation_files(
        "execution_space",
        TEST_PID,
        &root,
        ["work_operations.jsonl"],
        &mut states,
    );

    // A torn external write must not claim convergence until its newline is
    // durable; completing it emits exactly one append invalidation.
    let mut file = OpenOptions::new().append(true).open(&path).expect("append");
    file.write_all(b"{\"v\":2}").expect("write torn row");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "work_operations.jsonl",
        &mut states,
        &manager,
        true,
    );
    assert!(rx.try_recv().is_err(), "torn row must not invalidate yet");
    file.write_all(b"\n").expect("complete row");
    file.flush().expect("flush row");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "work_operations.jsonl",
        &mut states,
        &manager,
        true,
    );
    match rx.try_recv() {
        Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
            assert_eq!(frame.reason, "append");
            assert_eq!(frame.revision, 1);
        }
        other => panic!("complete append missing invalidation: {other:?}"),
    }
    drop(file);

    // Atomic replacement with the same byte length changes inode, not
    // length. A length-only watcher would stay falsely healthy forever.
    let replacement = root.join("work_operations.jsonl.replace");
    let same_len = std::fs::metadata(&path).expect("metadata").len();
    let replacement_bytes = vec![b' '; same_len.saturating_sub(1) as usize];
    let mut replacement_content = replacement_bytes;
    replacement_content.push(b'\n');
    std::fs::write(&replacement, replacement_content).expect("write replacement");
    std::fs::rename(&replacement, &path).expect("atomic replace");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "work_operations.jsonl",
        &mut states,
        &manager,
        true,
    );
    match rx.try_recv() {
        Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
            assert_eq!(frame.reason, "replace");
            assert_eq!(frame.revision, 2);
        }
        other => panic!("same-size replacement missing invalidation: {other:?}"),
    }

    std::fs::write(&path, "").expect("truncate ledger");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "work_operations.jsonl",
        &mut states,
        &manager,
        true,
    );
    match rx.try_recv() {
        Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
            assert_eq!(frame.reason, "truncate");
            assert_eq!(frame.revision, 3);
        }
        other => panic!("truncation missing invalidation: {other:?}"),
    }

    std::fs::remove_file(&path).expect("delete ledger");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "work_operations.jsonl",
        &mut states,
        &manager,
        true,
    );
    match rx.try_recv() {
        Ok(SseEventFrame::ProjectionInvalidated(frame)) => {
            assert_eq!(frame.reason, "delete");
            assert_eq!(frame.revision, 4);
        }
        other => panic!("deletion missing invalidation: {other:?}"),
    }

    std::fs::remove_dir_all(&root).expect("cleanup");
}

#[test]
fn typed_ledger_replace_delete_and_recreate_invalidate_without_append_noise() {
    let root = unique_dir("typed-projection-invalidation");
    std::fs::create_dir_all(&root).expect("create root");
    let path = root.join("missions.jsonl");
    std::fs::write(&path, "{\"id\":\"before\"}\n").expect("seed typed ledger");

    let manager = SseManager::new();
    let rx = manager.subscribe(TEST_PID);
    let mut states = HashMap::new();
    seed_invalidation_files(
        "execution_space",
        TEST_PID,
        &root,
        ["missions.jsonl"],
        &mut states,
    );

    // Ordinary appends use typed frames and must not force a full refetch.
    let mut file = OpenOptions::new().append(true).open(&path).expect("append");
    file.write_all(b"{\"id\":\"append\"}\n")
        .expect("append row");
    file.flush().expect("flush append");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "missions.jsonl",
        &mut states,
        &manager,
        false,
    );
    assert!(
        rx.try_recv().is_err(),
        "typed append should stay incremental"
    );
    drop(file);

    let replacement = root.join("missions.jsonl.replace");
    let len = std::fs::metadata(&path).expect("metadata").len();
    let mut bytes = vec![b' '; len.saturating_sub(1) as usize];
    bytes.push(b'\n');
    std::fs::write(&replacement, bytes).expect("write same-size replacement");
    std::fs::rename(&replacement, &path).expect("atomic replace");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "missions.jsonl",
        &mut states,
        &manager,
        false,
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(SseEventFrame::ProjectionInvalidated(ProjectionInvalidation { reason, .. }))
            if reason == "replace"
    ));

    std::fs::remove_file(&path).expect("delete typed ledger");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "missions.jsonl",
        &mut states,
        &manager,
        false,
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(SseEventFrame::ProjectionInvalidated(ProjectionInvalidation { reason, .. }))
            if reason == "delete"
    ));

    std::fs::write(&path, "{\"id\":\"after!\"}\n").expect("recreate typed ledger");
    poll_invalidation_file(
        "execution_space",
        TEST_PID,
        &root,
        "missions.jsonl",
        &mut states,
        &manager,
        false,
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(SseEventFrame::ProjectionInvalidated(ProjectionInvalidation { reason, .. }))
            if reason == "replace"
    ));

    std::fs::remove_dir_all(&root).expect("cleanup");
}

/// Transient provider activity is sent only to the current exact owner in
/// its project. A same-project Host, sibling, anonymous subscriber, later
/// owner subscriber, and other project all receive no replay or payload.
#[test]
fn live_provider_activity_is_direct_only_and_project_isolated() {
    let manager = SseManager::new();
    let owner =
        manager.subscribe_scoped_private("space-a", None, Some("agent-owner"), Some("project-a"));
    let host =
        manager.subscribe_scoped_private("space-a", None, Some("agent-host"), Some("project-a"));
    let other_binding =
        manager.subscribe_scoped_private("space-a", None, Some("agent-owner"), Some("project-b"));
    let anonymous = manager.subscribe("space-a");
    let other_project =
        manager.subscribe_scoped_private("space-b", None, Some("agent-owner"), Some("project-a"));
    let activity = serde_json::json!({
        "member_run_id": "mrun-a",
        "status": "working",
        "summary": "Reading the current implementation"
    });

    manager.broadcast_live_provider_activity(
        "space-a",
        "project-a",
        "agent-owner",
        activity.clone(),
    );
    let late_owner =
        manager.subscribe_scoped_private("space-a", None, Some("agent-owner"), Some("project-a"));

    match owner.try_recv() {
        Ok(SseEventFrame::LiveProviderActivity(value)) => assert_eq!(value, activity),
        other => panic!("exact owner should receive transient activity, got {other:?}"),
    }
    assert!(
        host.try_recv().is_err(),
        "Host must not see Member-private live activity"
    );
    assert!(
        anonymous.try_recv().is_err(),
        "anonymous stream must not see private activity"
    );
    assert!(
        other_binding.try_recv().is_err(),
        "another Project Binding in the same Execution Space must not see activity"
    );
    assert!(
        other_project.try_recv().is_err(),
        "another project must not see activity"
    );
    assert!(
        late_owner.try_recv().is_err(),
        "a later owner subscriber receives no replay"
    );
    assert!(
        !WATCHED_FILES
            .iter()
            .any(|filename| filename.contains("activity")),
        "member activity must never be read from a JSONL watcher"
    );
}

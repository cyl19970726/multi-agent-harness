use firm_provider_events::{
    adapter_manifest, decode_native_event, decode_native_json_line, read_transcript_batch,
    Completeness, DecodeContext, DecodeOutcome, EffectCertainty, FoldOutcome, NativeEvent,
    ObservationPayload, ObservationVisibility, ProjectionAccessError, ProjectionAuthority,
    ProjectionStore, ProjectionStoreError, ProjectionViewer, ProviderEventFold,
    ProviderEventFoldError, ProviderKind, SemanticKind, TranscriptCursor, TranscriptReadBoundary,
    TranscriptReadError, PROVIDER_OBSERVATION_SCHEMA_VERSION,
};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn context(provider: ProviderKind) -> DecodeContext {
    DecodeContext {
        provider,
        native_source_ref: format!("evidence:provider-session:source:12:{}", provider.as_str()),
        agent_identity_id: "agent-1".into(),
        agent_session_id: "session-1".into(),
        agent_session_generation: 7,
        node_daemon_id: "daemon-1".into(),
        node_daemon_generation: 4,
        provider_thread_id: Some("thread-1".into()),
        runtime_command_id: None,
        observed_at: "2026-08-13T08:00:00Z".into(),
    }
}

fn decode(provider: ProviderKind, position: u64, raw: serde_json::Value) -> DecodeOutcome {
    decode_native_event(
        &context(provider),
        NativeEvent {
            native_event_id: Some(format!("native-{position}")),
            provider_turn_id: Some("turn-1".into()),
            ordering_position: position,
            occurred_at: Some(format!("2026-08-13T08:00:{position:02}Z")),
            raw,
        },
    )
    .expect("decode")
}

fn observation(outcome: DecodeOutcome) -> firm_provider_events::ProviderObservation {
    match outcome {
        DecodeOutcome::Observation(value) => *value,
        other => panic!("expected observation, got {other:?}"),
    }
}

#[test]
fn four_provider_manifests_are_closed_and_truthful() {
    for provider in [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Kimi,
        ProviderKind::Pi,
    ] {
        let manifest = adapter_manifest(provider);
        assert_eq!(manifest.provider, provider);
        assert!(!manifest.native_families.is_empty());
        assert!(manifest.streaming);
        assert!(manifest.terminal_events);
        assert!(manifest.tool_events);
        assert!(!manifest.supported_semantic_kinds.is_empty());
        assert!(!manifest.redaction_policy.is_empty());
    }
}

#[test]
fn rust_adapter_manifests_match_the_versioned_json_contract() {
    let expected: Vec<firm_provider_events::AdapterManifest> = serde_json::from_str(include_str!(
        "../../../schemas/provider-events/adapters.v1.json"
    ))
    .expect("adapter manifest JSON");
    let actual = [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Kimi,
        ProviderKind::Pi,
    ]
    .into_iter()
    .map(adapter_manifest)
    .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn faithful_text_tool_terminal_paths_map_without_raw_tool_io() {
    let cases = [
        (
            ProviderKind::Codex,
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"done"}}),
            SemanticKind::AuthoredResponse,
        ),
        (
            ProviderKind::Claude,
            json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"call-1","name":"Read","input":{"secret":"never-project"}}]}}),
            SemanticKind::ToolCallRequested,
        ),
        (
            ProviderKind::Kimi,
            json!({"type":"context.append_loop_event","event":{"type":"tool.result","id":"call-1","output":"never-project"}}),
            SemanticKind::ToolCallCompleted,
        ),
        (
            ProviderKind::Pi,
            json!({"type":"turn_end","message":{"content":[{"type":"text","text":"done"}]}}),
            SemanticKind::TurnCompleted,
        ),
    ];
    for (provider, raw, expected_kind) in cases {
        let observation = observation(decode(provider, 1, raw));
        assert_eq!(observation.semantic_kind, expected_kind);
        assert_eq!(
            observation.visibility,
            ObservationVisibility::SessionOwnerPrivate
        );
        assert_eq!(
            observation.schema_version,
            PROVIDER_OBSERVATION_SCHEMA_VERSION
        );
        let projected = serde_json::to_string(&observation).expect("observation JSON");
        assert!(!projected.contains("never-project"));
    }
}

#[test]
fn private_reasoning_is_structurally_dropped_for_all_providers() {
    let cases = [
        (
            ProviderKind::Codex,
            json!({"type":"event_msg","payload":{"type":"agent_reasoning","text":"secret"}}),
        ),
        (
            ProviderKind::Claude,
            json!({"type":"assistant","message":{"content":[{"type":"thinking","thinking":"secret"}]}}),
        ),
        (
            ProviderKind::Kimi,
            json!({"type":"context.append_loop_event","event":{"type":"content.part","part":{"type":"think","think":"secret"}}}),
        ),
    ];
    for (provider, raw) in cases {
        assert_eq!(decode(provider, 1, raw), DecodeOutcome::DroppedPrivate);
    }
}

#[test]
fn server_context_cannot_be_selected_by_native_body() {
    let error = decode_native_event(
        &context(ProviderKind::Pi),
        NativeEvent {
            native_event_id: Some("hostile".into()),
            provider_turn_id: Some("turn-1".into()),
            ordering_position: 1,
            occurred_at: None,
            raw: json!({
                "type":"turn_end",
                "agent_session_id":"victim-session",
                "node_daemon_generation":999,
                "visibility":"team_public"
            }),
        },
    )
    .expect_err("authority injection rejects");
    assert_eq!(
        error.to_string(),
        "provider event attempted to select server authority"
    );
}

#[test]
fn exact_replay_conflict_late_order_restart_and_cursor_are_deterministic() {
    let first = observation(decode(
        ProviderKind::Codex,
        2,
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"later"}}),
    ));
    let late = observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"response_item","payload":{"type":"function_call","name":"Read","call_id":"call-1"}}),
    ));
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    assert_eq!(fold.ingest(first.clone()), Ok(FoldOutcome::Inserted));
    assert_eq!(fold.ingest(first.clone()), Ok(FoldOutcome::Replay));
    assert_eq!(fold.ingest(late), Ok(FoldOutcome::Inserted));
    let projection = fold.session_projection(300);
    assert_eq!(projection.episodes.len(), 1);
    assert_eq!(projection.episodes[0].observations[0].ordering_position, 1);
    assert_eq!(projection.episodes[0].observations[1].ordering_position, 2);

    let bytes = serde_json::to_vec(&fold).expect("durable fold");
    let resumed: ProviderEventFold = serde_json::from_slice(&bytes).expect("resume fold");
    assert_eq!(fold.cursor(), resumed.cursor());
    assert_eq!(
        fold.session_projection(300),
        resumed.session_projection(300)
    );

    let mut changed = first;
    changed.observed_at = "2026-08-13T09:00:00Z".into();
    assert_eq!(
        fold.ingest(changed),
        Err(ProviderEventFoldError::IdentityConflict)
    );
}

#[test]
fn same_fingerprint_with_changed_envelope_is_not_a_replay() {
    let first = observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"turn/completed"}),
    ));
    let mut changed = first.clone();
    changed.agent_identity_id = "impersonated-agent".into();
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    assert_eq!(fold.ingest(first), Ok(FoldOutcome::Inserted));
    assert_eq!(
        fold.ingest(changed),
        Err(ProviderEventFoldError::IdentityConflict)
    );
}

#[test]
fn episode_order_follows_native_position_not_turn_identifier_sort() {
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    let mut first = observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"turn/completed"}),
    ));
    first.provider_turn_id = Some("z-first".into());
    let mut second = observation(decode(
        ProviderKind::Codex,
        2,
        json!({"type":"turn/completed"}),
    ));
    second.provider_turn_id = Some("a-second".into());
    fold.ingest(second).unwrap();
    fold.ingest(first).unwrap();
    let projection = fold.session_projection(300);
    assert_eq!(projection.episodes[0].episode_id, "z-first");
    assert_eq!(projection.episodes[1].episode_id, "a-second");
}

#[test]
fn stale_generation_fails_before_projection_change() {
    let mut stale_context = context(ProviderKind::Claude);
    stale_context.agent_session_generation = 6;
    let stale = match decode_native_event(
        &stale_context,
        NativeEvent {
            native_event_id: Some("stale".into()),
            provider_turn_id: Some("turn-1".into()),
            ordering_position: 1,
            occurred_at: None,
            raw: json!({"type":"result","usage":{"input_tokens":1,"output_tokens":2}}),
        },
    )
    .expect("decode")
    {
        DecodeOutcome::Observation(value) => *value,
        other => panic!("unexpected {other:?}"),
    };
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    let before = fold.cursor();
    assert_eq!(
        fold.ingest(stale),
        Err(ProviderEventFoldError::AuthorityMismatch)
    );
    assert_eq!(before, fold.cursor());
    assert!(fold.session_projection(300).episodes.is_empty());
}

#[test]
fn team_projection_is_allowlist_not_filtered_private_copy() {
    let private = observation(decode(
        ProviderKind::Pi,
        1,
        json!({"type":"message_update","content":[{"type":"text","text":"private answer"}]}),
    ));
    let public = observation(decode(
        ProviderKind::Pi,
        2,
        json!({"type":"interaction_required","reasonCode":"approval_required","prompt":"Approve bounded tool use"}),
    ));
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    fold.ingest(private).expect("private");
    fold.ingest(public).expect("public");
    let team = fold.team_public_projection();
    assert_eq!(team.len(), 1);
    assert_eq!(team[0].semantic_kind, SemanticKind::InteractionRequired);
    assert!(!serde_json::to_string(&team)
        .unwrap()
        .contains("private answer"));
}

#[test]
fn unknown_effect_is_not_silently_completed() {
    let mut observation = observation(decode(
        ProviderKind::Kimi,
        1,
        json!({"type":"transport_interrupted"}),
    ));
    observation.runtime_command_id = Some("command-1".into());
    observation.effect_certainty = EffectCertainty::Unknown;
    observation.completeness = Completeness::RecoveryRequired;
    observation.semantic_kind = SemanticKind::CommandRecoveryRequired;
    observation.payload = ObservationPayload::Recovery {
        reason_code: "native_effect_unknown".into(),
    };
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    fold.ingest(observation).expect("recovery observation");
    let projection = fold.session_projection(300);
    assert!(projection.episodes[0].incomplete);
    assert_eq!(
        fold.runtime_command_ids(),
        ["command-1"].into_iter().collect()
    );
}

fn authority() -> ProjectionAuthority {
    ProjectionAuthority {
        project_id: "project-1".into(),
        team_id: "team-1".into(),
        agent_identity_id: "agent-1".into(),
        agent_session_id: "session-1".into(),
        agent_session_generation: 7,
    }
}

fn viewer(agent_identity_id: &str) -> ProjectionViewer {
    ProjectionViewer {
        project_id: "project-1".into(),
        team_id: "team-1".into(),
        agent_identity_id: agent_identity_id.into(),
        is_team_host: false,
    }
}

#[test]
fn private_session_requires_exact_owner_while_team_projection_is_bounded() {
    let private = observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"private"}}),
    ));
    let public = observation(decode(
        ProviderKind::Pi,
        2,
        json!({"type":"interaction_required","reasonCode":"decision","prompt":"Need a decision"}),
    ));
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    fold.ingest(private).unwrap();
    fold.ingest(public).unwrap();

    assert!(firm_provider_events::project_private_session(
        &fold,
        &authority(),
        &viewer("agent-1"),
        300
    )
    .is_ok());
    assert_eq!(
        firm_provider_events::project_private_session(
            &fold,
            &authority(),
            &viewer("sibling-agent"),
            300
        ),
        Err(ProjectionAccessError::NotSessionOwner)
    );
    let team =
        firm_provider_events::project_team_activity(&fold, &authority(), &viewer("sibling-agent"))
            .unwrap();
    assert_eq!(team.len(), 1);
    assert!(!serde_json::to_string(&team).unwrap().contains("private"));

    let mut cross_team = viewer("agent-1");
    cross_team.team_id = "team-2".into();
    assert_eq!(
        firm_provider_events::project_team_activity(&fold, &authority(), &cross_team),
        Err(ProjectionAccessError::CrossTeam)
    );
}

fn unique_temp_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "agentfirm-provider-events-{label}-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn durable_store_resumes_exact_cursor_and_rejects_torn_snapshot() {
    let root = unique_temp_path("resume");
    let path = root.join("projection.json");
    let store = ProjectionStore::new(&path);
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    store
        .ingest(
            &mut fold,
            observation(decode(
                ProviderKind::Claude,
                1,
                json!({"type":"assistant","message":{"content":"hello"}}),
            )),
        )
        .unwrap();
    let resumed = store.load().unwrap().unwrap();
    assert_eq!(resumed.cursor(), fold.cursor());
    assert_eq!(
        resumed.session_projection(300),
        fold.session_projection(300)
    );

    fs::write(&path, b"{\"schema_version\":").unwrap();
    assert!(matches!(
        store.load(),
        Err(ProjectionStoreError::Malformed(_))
    ));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn failed_snapshot_write_does_not_advance_live_fold() {
    let root = unique_temp_path("failed-write");
    fs::create_dir_all(&root).unwrap();
    let store = ProjectionStore::new(&root);
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    let before = fold.cursor();
    let result = store.ingest(
        &mut fold,
        observation(decode(ProviderKind::Pi, 1, json!({"type":"turn_end"}))),
    );
    assert!(matches!(result, Err(ProjectionStoreError::Io(_))));
    assert_eq!(before, fold.cursor());
    assert!(fold.session_projection(300).episodes.is_empty());
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn impossible_effect_and_public_private_semantics_fail_before_fold_change() {
    let mut effect = observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"turn/completed"}),
    ));
    effect.effect_certainty = EffectCertainty::Applied;
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    let cursor = fold.cursor();
    assert!(matches!(
        fold.ingest(effect),
        Err(ProviderEventFoldError::InvalidObservation(_))
    ));
    assert_eq!(fold.cursor(), cursor);

    let mut leaked = observation(decode(
        ProviderKind::Codex,
        2,
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"secret"}}),
    ));
    leaked.visibility = ObservationVisibility::TeamPublic;
    assert!(matches!(
        fold.ingest(leaked),
        Err(ProviderEventFoldError::InvalidObservation(_))
    ));
    assert_eq!(fold.cursor(), cursor);
}

#[test]
fn four_provider_jsonl_corpora_decode_and_malformed_lines_are_bounded() {
    let cases = [
        (ProviderKind::Codex, include_str!("fixtures/codex.jsonl")),
        (ProviderKind::Claude, include_str!("fixtures/claude.jsonl")),
        (ProviderKind::Kimi, include_str!("fixtures/kimi.jsonl")),
        (ProviderKind::Pi, include_str!("fixtures/pi.jsonl")),
    ];
    for (provider, corpus) in cases {
        let observations = corpus
            .lines()
            .enumerate()
            .map(|(index, line)| {
                observation(
                    decode_native_json_line(
                        &context(provider),
                        Some(format!("corpus-{index}")),
                        Some("corpus-turn".into()),
                        index as u64 + 1,
                        None,
                        line,
                    )
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(observations.len(), 3);
        let serialized = serde_json::to_string(&observations).unwrap();
        assert!(!serialized.contains("not projected"));
        assert!(observations
            .iter()
            .any(|item| item.semantic_kind == SemanticKind::TurnCompleted));
    }

    let malformed = observation(
        decode_native_json_line(
            &context(ProviderKind::Pi),
            None,
            Some("turn-malformed".into()),
            9,
            None,
            "{private transcript fragment",
        )
        .unwrap(),
    );
    assert_eq!(malformed.semantic_kind, SemanticKind::MalformedOrIncomplete);
    assert_eq!(malformed.visibility, ObservationVisibility::OperatorOnly);
    assert!(malformed.redacted);
    assert!(!serde_json::to_string(&malformed)
        .unwrap()
        .contains("private transcript"));
}

#[test]
fn transcript_reader_resumes_by_byte_offset_and_holds_incomplete_tail() {
    let root = unique_temp_path("reader");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("session.jsonl");
    fs::write(
        &path,
        b"{\"type\":\"turn/completed\"}\n{\"type\":\"turn/cancelled\"}\n{\"type\":",
    )
    .unwrap();
    let boundary = TranscriptReadBoundary {
        allowed_root: root.clone(),
        transcript_path: path.clone(),
    };
    let first = read_transcript_batch(
        &context(ProviderKind::Codex),
        &boundary,
        TranscriptCursor::default(),
        1,
    )
    .unwrap();
    assert_eq!(first.outcomes.len(), 1);
    assert!(first.incomplete_tail);
    let second =
        read_transcript_batch(&context(ProviderKind::Codex), &boundary, first.cursor, 10).unwrap();
    assert_eq!(second.outcomes.len(), 1);
    assert!(second.incomplete_tail);

    fs::write(&path, b"{}\n").unwrap();
    assert!(matches!(
        read_transcript_batch(&context(ProviderKind::Codex), &boundary, second.cursor, 10),
        Err(TranscriptReadError::CursorBeyondEnd)
    ));
    fs::remove_dir_all(&root).unwrap();
}

#[cfg(unix)]
#[test]
fn transcript_reader_rejects_symlink_and_root_escape() {
    use std::os::unix::fs::symlink;
    let root = unique_temp_path("reader-root");
    let outside = unique_temp_path("reader-outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let outside_file = outside.join("session.jsonl");
    fs::write(&outside_file, b"{\"type\":\"turn/completed\"}\n").unwrap();
    let link = root.join("session.jsonl");
    symlink(&outside_file, &link).unwrap();
    let result = read_transcript_batch(
        &context(ProviderKind::Codex),
        &TranscriptReadBoundary {
            allowed_root: root.clone(),
            transcript_path: link,
        },
        TranscriptCursor::default(),
        10,
    );
    assert!(matches!(
        result,
        Err(TranscriptReadError::InvalidSourceType)
    ));
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

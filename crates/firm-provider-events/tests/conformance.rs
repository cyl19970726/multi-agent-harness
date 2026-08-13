use firm_provider_events::{
    adapter_manifest, decode_native_event, decode_native_json_line, read_latest_transcript_batch,
    read_transcript_batch, Completeness, DecodeContext, DecodeOutcome, EffectCertainty,
    FoldOutcome, NativeEvent, ObservationPayload, ObservationVisibility, ProjectionAccessError,
    ProjectionAuthority, ProjectionViewer, ProviderEventFold, ProviderEventFoldError, ProviderKind,
    ProviderProjectionService, ProviderProjectionServiceError, SemanticKind,
    TranscriptReadBoundary, TranscriptReadError, TransientReadPosition,
    PROVIDER_OBSERVATION_SCHEMA_VERSION,
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
        native_source_ref: format!("provider-source:session:source:12:{}", provider.as_str()),
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
fn exact_duplicate_conflict_and_late_order_are_deterministic_in_memory() {
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
fn missing_terminal_is_explicitly_incomplete() {
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    fold.ingest(observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"streaming"}}),
    )))
    .unwrap();
    let projection = fold.session_projection(300);
    assert!(!projection.episodes[0].terminal);
    assert!(projection.episodes[0].incomplete);
}

#[test]
fn truncation_is_explicit_and_retains_the_latest_native_positions() {
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    for position in 1..=5 {
        fold.ingest(observation(decode(
            ProviderKind::Codex,
            position,
            json!({"type":"event_msg","payload":{"type":"agent_message","message":format!("row-{position}")}}),
        )))
        .unwrap();
    }
    let projection = fold.session_projection(2);
    assert!(projection.truncated);
    assert_eq!(projection.episodes[0].observations[0].ordering_position, 4);
    assert_eq!(projection.episodes[0].observations[1].ordering_position, 5);
}

#[test]
fn zero_generation_and_unscoped_source_are_rejected_before_decode() {
    let mut invalid = context(ProviderKind::Pi);
    invalid.agent_session_generation = 0;
    assert!(decode_native_event(
        &invalid,
        NativeEvent {
            native_event_id: None,
            provider_turn_id: None,
            ordering_position: 1,
            occurred_at: None,
            raw: json!({"type":"turn_end"}),
        }
    )
    .is_err());
    let mut invalid = context(ProviderKind::Pi);
    invalid.native_source_ref = "/private/provider/path".into();
    assert!(decode_native_event(
        &invalid,
        NativeEvent {
            native_event_id: None,
            provider_turn_id: None,
            ordering_position: 1,
            occurred_at: None,
            raw: json!({"type":"turn_end"}),
        }
    )
    .is_err());
}

#[test]
fn authored_content_redacts_secret_shapes_and_absolute_private_paths() {
    let observation = observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"token=super-secret sk-12345678901234567890 /Users/alice/.ssh/id_ed25519"}}),
    ));
    let serialized = serde_json::to_string(&observation).unwrap();
    assert!(observation.redacted);
    assert!(!serialized.contains("super-secret"));
    assert!(!serialized.contains("sk-123"));
    assert!(!serialized.contains("/Users/alice"));
    assert!(serialized.contains("REDACTED"));
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
    let before = fold.snapshot_fingerprint();
    assert_eq!(
        fold.ingest(stale),
        Err(ProviderEventFoldError::AuthorityMismatch)
    );
    assert_eq!(before, fold.snapshot_fingerprint());
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
        execution_space_id: "space-1".into(),
        project_binding_id: "project-1".into(),
        team_id: "team-1".into(),
        agent_identity_id: "agent-1".into(),
        agent_session_id: "session-1".into(),
        agent_session_generation: 7,
    }
}

fn viewer(agent_identity_id: &str) -> ProjectionViewer {
    ProjectionViewer {
        execution_space_id: "space-1".into(),
        project_binding_id: "project-1".into(),
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

    let mut cross_space = viewer("agent-1");
    cross_space.execution_space_id = "space-2".into();
    assert_eq!(
        firm_provider_events::project_private_session(&fold, &authority(), &cross_space, 300),
        Err(ProjectionAccessError::CrossExecutionSpace)
    );

    let mut cross_binding = viewer("agent-1");
    cross_binding.project_binding_id = "project-2".into();
    assert_eq!(
        firm_provider_events::project_private_session(&fold, &authority(), &cross_binding, 300),
        Err(ProjectionAccessError::CrossProjectBinding)
    );

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
fn impossible_effect_and_public_private_semantics_fail_before_fold_change() {
    let mut effect = observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"turn/completed"}),
    ));
    effect.effect_certainty = EffectCertainty::Applied;
    let mut fold = ProviderEventFold::new("session-1", 7, "daemon-1", 4);
    let before_fingerprint = fold.snapshot_fingerprint();
    assert!(matches!(
        fold.ingest(effect),
        Err(ProviderEventFoldError::InvalidObservation(_))
    ));
    assert_eq!(fold.snapshot_fingerprint(), before_fingerprint);

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
    assert_eq!(fold.snapshot_fingerprint(), before_fingerprint);
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
                        (provider != ProviderKind::Kimi).then(|| "corpus-turn".into()),
                        index as u64 + 1,
                        None,
                        line,
                    )
                    .unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            observations.len() >= 3,
            "{provider:?} corpus must exercise text, tool, and terminal paths"
        );
        let serialized = serde_json::to_string(&observations).unwrap();
        assert!(!serialized.contains("not projected"));
        assert!(!serialized.contains("private tool output"));
        assert!(observations
            .iter()
            .any(|item| item.semantic_kind == SemanticKind::TurnCompleted));
        if provider == ProviderKind::Kimi {
            assert!(observations.iter().any(|item| {
                matches!(
                    &item.payload,
                    ObservationPayload::Tool { call_id, .. }
                        if call_id.as_deref() == Some("kimi-call")
                )
            }));
            assert!(observations
                .iter()
                .all(|item| item.provider_turn_id.as_deref() == Some("kimi-turn")));
        }
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
fn transcript_reader_uses_disposable_position_and_holds_incomplete_tail() {
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
        TransientReadPosition::default(),
        1,
    )
    .unwrap();
    assert_eq!(first.outcomes.len(), 1);
    assert!(first.incomplete_tail);
    let second = read_transcript_batch(
        &context(ProviderKind::Codex),
        &boundary,
        first.next_position,
        10,
    )
    .unwrap();
    assert_eq!(second.outcomes.len(), 1);
    assert!(second.incomplete_tail);

    fs::write(&path, b"{}\n").unwrap();
    assert!(matches!(
        read_transcript_batch(
            &context(ProviderKind::Codex),
            &boundary,
            second.next_position,
            10
        ),
        Err(TranscriptReadError::SourceChanged)
    ));
    fs::remove_dir_all(&root).unwrap();
}

#[test]
fn latest_reader_keeps_the_real_tail_with_turn_context_and_explicit_truncation() {
    let root = unique_temp_path("latest-reader");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("session.jsonl");
    fs::write(
        &path,
        concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-old\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"old\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-old\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-new\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"new\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-new\"}}\n",
            "{\"type\":"
        ),
    )
    .unwrap();
    let boundary = TranscriptReadBoundary {
        allowed_root: root.clone(),
        transcript_path: path,
    };
    let latest = read_latest_transcript_batch(&context(ProviderKind::Codex), &boundary, 3)
        .expect("latest provider tail");
    assert_eq!(latest.outcomes.len(), 3);
    assert!(latest.source_truncated);
    assert!(latest.incomplete_tail);
    let observations = latest
        .outcomes
        .into_iter()
        .filter_map(|outcome| match outcome {
            DecodeOutcome::Observation(value) => Some(value),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(observations.len(), 2);
    assert!(observations
        .iter()
        .all(|item| item.provider_turn_id.as_deref() == Some("turn-new")));
    assert!(observations.iter().any(|item| {
        matches!(
            &item.payload,
            ObservationPayload::AuthoredResponse { text } if text == "new"
        )
    }));
    assert!(!observations.iter().any(|item| {
        matches!(
            &item.payload,
            ObservationPayload::AuthoredResponse { text } if text == "old"
        )
    }));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn latest_service_marks_a_source_tail_as_truncated_without_persisting_a_cursor() {
    let root = unique_temp_path("latest-service");
    fs::create_dir_all(&root).unwrap();
    let transcript = root.join("provider.jsonl");
    fs::write(
        &transcript,
        concat!(
            "{\"type\":\"turn/completed\",\"turn_id\":\"old\"}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"new\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"latest\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"new\"}}\n"
        ),
    )
    .unwrap();
    let boundary = TranscriptReadBoundary {
        allowed_root: root.clone(),
        transcript_path: transcript,
    };
    let mut service = ProviderProjectionService::open(context(ProviderKind::Codex));
    assert_eq!(service.refresh_latest(&boundary, 3).unwrap(), 3);
    let projection = service
        .private_session(&authority(), &viewer("agent-1"), 300)
        .unwrap();
    assert!(projection.truncated);
    assert_eq!(projection.episodes.len(), 1);
    assert_eq!(
        projection.episodes[0].provider_turn_id.as_deref(),
        Some("new")
    );
    assert!(projection.episodes[0].terminal);
    assert_eq!(
        service.transient_position(),
        &TransientReadPosition::default(),
        "latest reads expose no resumable cursor"
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
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
        TransientReadPosition::default(),
        10,
    );
    assert!(matches!(
        result,
        Err(TranscriptReadError::InvalidSourceType)
    ));
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(outside).unwrap();
}

#[test]
fn on_demand_service_writes_no_projection_files_and_restart_discards_state() {
    let root = unique_temp_path("pipeline");
    fs::create_dir_all(&root).unwrap();
    let transcript = root.join("provider.jsonl");
    fs::write(
        &transcript,
        b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"ok\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_reasoning\",\"text\":\"private\"}}\n",
    )
    .unwrap();
    let boundary = TranscriptReadBoundary {
        allowed_root: root.clone(),
        transcript_path: transcript,
    };
    let mut service = ProviderProjectionService::open(context(ProviderKind::Codex));
    service.refresh(&boundary, 10).unwrap();
    assert_eq!(
        service
            .private_session(&authority(), &viewer("agent-1"), 300)
            .unwrap()
            .episodes[0]
            .observations
            .len(),
        1
    );
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "only provider source exists"
    );
    let restarted = ProviderProjectionService::open(context(ProviderKind::Codex));
    assert!(restarted
        .private_session(&authority(), &viewer("agent-1"), 300)
        .unwrap()
        .episodes
        .is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_turn_context_survives_one_read_call_and_closes_on_terminal() {
    let root = unique_temp_path("turn-transient-position");
    fs::create_dir_all(&root).unwrap();
    let transcript = root.join("codex.jsonl");
    fs::write(
        &transcript,
        b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-9\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"one\"}}\n{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-9\"}}\n",
    )
    .unwrap();
    let boundary = TranscriptReadBoundary {
        allowed_root: root.clone(),
        transcript_path: transcript,
    };
    let first = read_transcript_batch(
        &context(ProviderKind::Codex),
        &boundary,
        TransientReadPosition::default(),
        2,
    )
    .unwrap();
    assert_eq!(
        first.next_position.active_provider_turn_id.as_deref(),
        Some("turn-9")
    );
    let observation = first
        .outcomes
        .into_iter()
        .find_map(|outcome| match outcome {
            DecodeOutcome::Observation(value) => Some(value),
            _ => None,
        })
        .unwrap();
    assert_eq!(observation.provider_turn_id.as_deref(), Some("turn-9"));
    let second = read_transcript_batch(
        &context(ProviderKind::Codex),
        &boundary,
        first.next_position,
        2,
    )
    .unwrap();
    assert_eq!(second.next_position.active_provider_turn_id, None);
    let terminal = second
        .outcomes
        .into_iter()
        .find_map(|outcome| match outcome {
            DecodeOutcome::Observation(value) => Some(value),
            _ => None,
        })
        .unwrap();
    assert_eq!(terminal.provider_turn_id.as_deref(), Some("turn-9"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn service_projects_private_and_public_views_on_demand_without_persistence() {
    let root = unique_temp_path("service");
    fs::create_dir_all(&root).unwrap();
    let transcript = root.join("pi.jsonl");
    fs::write(
        &transcript,
        b"{\"type\":\"message_update\",\"content\":[{\"type\":\"text\",\"text\":\"private\"}]}\n{\"type\":\"interaction_required\",\"reasonCode\":\"decision\",\"prompt\":\"Need decision\"}\n",
    )
    .unwrap();
    let boundary = TranscriptReadBoundary {
        allowed_root: root.clone(),
        transcript_path: transcript,
    };
    let mut service = ProviderProjectionService::open(context(ProviderKind::Pi));
    assert_eq!(service.refresh(&boundary, 10).unwrap(), 2);
    assert_eq!(
        service
            .private_session(&authority(), &viewer("agent-1"), 300)
            .unwrap()
            .episodes
            .iter()
            .map(|episode| episode.observations.len())
            .sum::<usize>(),
        2
    );
    assert_eq!(
        service
            .team_activity(&authority(), &viewer("sibling"))
            .unwrap()
            .len(),
        1
    );
    assert!(matches!(
        service.private_session(&authority(), &viewer("sibling"), 300),
        Err(ProviderProjectionServiceError::Access(
            ProjectionAccessError::NotSessionOwner
        ))
    ));
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "service creates no mirror"
    );
    fs::remove_dir_all(root).unwrap();
}

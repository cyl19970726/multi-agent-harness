use firm_provider_events::{
    adapter_manifest, decode_native_event, decode_native_json_line, read_transcript_page,
    Completeness, ContentAvailability, DecodeContext, DecodeOutcome, EffectCertainty, FoldOutcome,
    FragmentPayload, FragmentVisibility, NativeEvent, NativeSessionReaderAuthority,
    OrderingKeyKind, PersistedAdapterManifest, PersistedCompleteness, PersistedEventFragment,
    PersistedFragmentPayload, PersistedNativeRow, PersistedOrderingKey, PersistedPageCursor,
    PersistedReaderReachability, PersistedRecordValidationError, PersistedSemanticCapability,
    PersistedSessionPage, PersistedSourceReset, PersistedTailMode, ProjectionAccessError,
    ProjectionAuthority, ProjectionReadScope, ProviderEventFold, ProviderEventFoldError,
    ProviderKind, ProviderNativeEventRecord, ProviderProjectionService, SemanticKind,
    SessionLifecyclePhase, SessionSemanticKind, SourceResetReason, TranscriptReadBoundary,
    TranscriptReadError, PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION,
    PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION, PROVIDER_NATIVE_EVENT_RECORD_V3_SCHEMA_VERSION,
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
        agent_member_id: "agent-1".into(),
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

fn observation(outcome: DecodeOutcome) -> firm_provider_events::LegacyProviderNativeEventRecordV2 {
    let DecodeOutcome::Record(value) = outcome;
    *value
}

fn persisted_record(
    source_generation: &str,
    row_locator: &str,
    ordering_value: u64,
) -> ProviderNativeEventRecord {
    let record_id = ProviderNativeEventRecord::stable_record_id(source_generation, row_locator);
    ProviderNativeEventRecord {
        schema_version: PROVIDER_NATIVE_EVENT_RECORD_V3_SCHEMA_VERSION.into(),
        record_id: record_id.clone(),
        provider: ProviderKind::Codex,
        adapter_version: PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION.into(),
        native_source_ref: "provider-source:codex:session-1".into(),
        source_generation: source_generation.into(),
        row_locator: row_locator.into(),
        ordering_key: PersistedOrderingKey {
            kind: OrderingKeyKind::ProviderOrdinal,
            value: ordering_value,
        },
        source_content_fingerprint:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        agent_member_id: "agent-1".into(),
        agent_session_id: "session-1".into(),
        agent_session_generation: 7,
        provider_thread_id: Some("thread-1".into()),
        provider_turn_id: Some("turn-1".into()),
        provider_event_id: Some("reasoning-1".into()),
        occurred_at: Some("2026-08-29T08:00:00Z".into()),
        observed_at: "2026-08-29T08:00:01Z".into(),
        native_event: json!({"type":"response_item","payload":{"type":"reasoning"}}),
        fragments: vec![PersistedEventFragment {
            fragment_id: format!("{record_id}:fragment-0"),
            fragment_index: 0,
            semantic_kind: SessionSemanticKind::Reasoning,
            lifecycle_phase: SessionLifecyclePhase::Progress,
            completeness: PersistedCompleteness::Complete,
            content_availability: ContentAvailability::Unavailable,
            payload: PersistedFragmentPayload::Reasoning { text: None },
        }],
    }
}

#[test]
fn v3_persisted_record_identity_excludes_reader_daemon_generation() {
    let record = persisted_record(
        "source-generation:codex:rollout:file-1",
        "row-locator:provider-id:reasoning-1",
        2,
    );
    record.validate().expect("valid persisted row");
    let first = PersistedSessionPage {
        reader_authority: NativeSessionReaderAuthority {
            node_id: "node-1".into(),
            node_daemon_id: "daemon-1".into(),
            node_daemon_generation: 1,
            agent_session_id: "session-1".into(),
            agent_session_generation: 7,
        },
        source_generation: record.source_generation.clone(),
        records: vec![record.clone()],
        snapshot_watermark: Some(record.ordering_key),
        has_more: false,
        next_cursor: None,
        incomplete_tail: false,
        source_reset: None,
    };
    let mut successor = first.clone();
    successor.reader_authority.node_daemon_generation = 2;
    first.validate().expect("predecessor reader authority");
    successor.validate().expect("successor reader authority");
    assert_eq!(first.records[0].record_id, successor.records[0].record_id);
    assert_eq!(first.records[0], successor.records[0]);
}

#[test]
fn v3_content_unavailable_is_absent_not_a_placeholder() {
    let mut record = persisted_record(
        "source-generation:codex:rollout:file-1",
        "row-locator:provider-id:reasoning-1",
        2,
    );
    record.validate().expect("absent reasoning is explicit");
    record.fragments[0].payload = PersistedFragmentPayload::Reasoning {
        text: Some("Reasoning content unavailable".into()),
    };
    assert_eq!(
        record.validate(),
        Err(PersistedRecordValidationError::InvalidFragments)
    );
}

#[test]
fn v3_cursor_and_source_reset_are_generation_scoped() {
    let record = persisted_record(
        "source-generation:codex:rollout:file-2",
        "row-locator:offset:512:sha256:aaaaaaaa",
        512,
    );
    let mut page = PersistedSessionPage {
        reader_authority: NativeSessionReaderAuthority {
            node_id: "node-1".into(),
            node_daemon_id: "daemon-1".into(),
            node_daemon_generation: 2,
            agent_session_id: "session-1".into(),
            agent_session_generation: 7,
        },
        source_generation: record.source_generation.clone(),
        records: vec![record.clone()],
        snapshot_watermark: Some(record.ordering_key),
        has_more: true,
        next_cursor: Some(PersistedPageCursor {
            source_generation: record.source_generation.clone(),
            before: record.ordering_key,
        }),
        incomplete_tail: true,
        source_reset: Some(PersistedSourceReset {
            previous_source_generation: Some("source-generation:codex:rollout:file-1".into()),
            source_generation: record.source_generation.clone(),
            reason: SourceResetReason::Rotated,
        }),
    };
    page.validate().expect("generation-scoped page");
    page.next_cursor.as_mut().unwrap().source_generation =
        "source-generation:codex:rollout:file-1".into();
    assert_eq!(
        page.validate(),
        Err(PersistedRecordValidationError::PageIdentityConflict)
    );
    page.next_cursor.as_mut().unwrap().source_generation = page.source_generation.clone();
    page.next_cursor.as_mut().unwrap().before.kind = OrderingKeyKind::CompleteRowEndOffset;
    assert_eq!(
        page.validate(),
        Err(PersistedRecordValidationError::PageIdentityConflict)
    );
}

fn v3_fixture_files(relative: &str) -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/provider-events/fixtures/v3")
        .join(relative);
    let mut files = fs::read_dir(root)
        .expect("v3 fixture directory")
        .map(|entry| entry.expect("v3 fixture entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

#[test]
fn checked_in_v3_fixtures_match_the_closed_rust_contract() {
    for path in v3_fixture_files("valid") {
        let record: ProviderNativeEventRecord =
            serde_json::from_slice(&fs::read(&path).expect("read valid record fixture"))
                .expect("valid record fixture wire");
        record.validate().expect("valid record fixture contract");
    }
    for path in v3_fixture_files("invalid") {
        if let Ok(record) = serde_json::from_slice::<ProviderNativeEventRecord>(
            &fs::read(&path).expect("read invalid record fixture"),
        ) {
            assert!(
                record.validate().is_err(),
                "{} was accepted",
                path.display()
            );
        }
    }
    for path in v3_fixture_files("page/valid") {
        let page: PersistedSessionPage =
            serde_json::from_slice(&fs::read(&path).expect("read valid page fixture"))
                .expect("valid page fixture wire");
        page.validate().expect("valid page fixture contract");
    }
    for path in v3_fixture_files("page/invalid") {
        if let Ok(page) = serde_json::from_slice::<PersistedSessionPage>(
            &fs::read(&path).expect("read invalid page fixture"),
        ) {
            assert!(page.validate().is_err(), "{} was accepted", path.display());
        }
    }
    for path in v3_fixture_files("adapter/valid") {
        let manifest: PersistedAdapterManifest =
            serde_json::from_slice(&fs::read(&path).expect("read valid manifest fixture"))
                .expect("valid manifest fixture wire");
        manifest
            .validate()
            .expect("valid manifest fixture contract");
    }
    for path in v3_fixture_files("adapter/invalid") {
        if let Ok(manifest) = serde_json::from_slice::<PersistedAdapterManifest>(
            &fs::read(&path).expect("read invalid manifest fixture"),
        ) {
            assert!(
                manifest.validate().is_err(),
                "{} was accepted",
                path.display()
            );
        }
    }
}

#[test]
fn persisted_row_and_source_scoped_manifest_contracts_are_closed() {
    let row = PersistedNativeRow {
        provider: ProviderKind::Codex,
        source_generation: "source-generation:codex:rollout:file-1".into(),
        row_locator: "row-locator:provider-id:item-1".into(),
        ordering_key: PersistedOrderingKey {
            kind: OrderingKeyKind::ProviderOrdinal,
            value: 1,
        },
        content_fingerprint:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        occurred_at: None,
        native_event: json!({"type":"response_item"}),
    };
    row.validate()
        .expect("bounded opaque persisted row identity");

    let mut manifest = PersistedAdapterManifest {
        provider: ProviderKind::Codex,
        adapter_version: PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION.into(),
        persisted_source_families: vec!["codex_rollout_jsonl".into()],
        format_version_fences: vec!["rollout_jsonl_v1".into()],
        source_generation: true,
        stable_row_locator: true,
        pagination: true,
        tail_mode: PersistedTailMode::Incremental,
        reader_reachability: vec![PersistedReaderReachability::Local],
        semantic_capabilities: vec![PersistedSemanticCapability {
            semantic_kind: SessionSemanticKind::Reasoning,
            phases: vec![SessionLifecyclePhase::Progress],
            content_availability: vec![
                ContentAvailability::Available,
                ContentAvailability::Unavailable,
            ],
        }],
    };
    manifest.validate().expect("source-scoped manifest");
    manifest
        .semantic_capabilities
        .push(PersistedSemanticCapability {
            semantic_kind: SessionSemanticKind::Reasoning,
            phases: vec![SessionLifecyclePhase::Progress],
            content_availability: vec![ContentAvailability::Available],
        });
    assert_eq!(
        manifest.validate(),
        Err(PersistedRecordValidationError::InvalidManifest)
    );

    manifest.semantic_capabilities.pop();
    manifest.semantic_capabilities[0]
        .phases
        .push(SessionLifecyclePhase::Progress);
    assert_eq!(
        manifest.validate(),
        Err(PersistedRecordValidationError::InvalidManifest)
    );
}

#[test]
fn five_provider_manifests_are_closed_and_truthful() {
    for provider in [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Kimi,
        ProviderKind::Pi,
        ProviderKind::DeepseekHarness,
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
fn claude_live_and_reopened_rows_share_one_lossless_multifragment_record() {
    let raw = json!({
        "event":"assistant_message",
        "data":{"content":[
            {"type":"thinking","thinking":"inspect the exact authority"},
            {"type":"text","text":"I found the mismatch."},
            {"type":"tool_use","id":"tool-1","name":"Read","input":{"path":"/tmp/private"}}
        ]}
    });
    let live = observation(decode(ProviderKind::Claude, 1, raw.clone()));
    let reopened = observation(
        decode_native_json_line(
            &context(ProviderKind::Claude),
            Some("native-1".into()),
            Some("turn-1".into()),
            1,
            Some("2026-08-13T08:00:01Z".into()),
            &serde_json::to_string(&raw).unwrap(),
        )
        .expect("reopened decode"),
    );
    assert_eq!(live, reopened);
    assert_eq!(live.fragments.len(), 3);
    assert_eq!(live.fragments[0].semantic_kind, SemanticKind::Reasoning);
    assert_eq!(
        live.fragments[1].semantic_kind,
        SemanticKind::AssistantResponse
    );
    assert_eq!(
        live.fragments[2].semantic_kind,
        SemanticKind::ToolCallRequested
    );
    assert_eq!(live.native_event, raw);
}

#[test]
fn incomplete_recognized_json_is_preserved_in_direct_and_reopened_reads() {
    let raw = json!({"type":"event_msg","payload":{"type":"agent_message"}});
    let direct = observation(decode(ProviderKind::Codex, 1, raw.clone()));
    assert_eq!(
        direct.fragments[0].semantic_kind,
        SemanticKind::MalformedOrIncomplete
    );
    assert_eq!(direct.native_event, raw);

    let root = unique_temp_path("incomplete-recognized-row");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("session.jsonl");
    fs::write(
        &path,
        "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\"}}\n",
    )
    .unwrap();
    let page = read_transcript_page(
        &context(ProviderKind::Codex),
        &TranscriptReadBoundary {
            allowed_root: root.clone(),
            transcript_path: path,
        },
        None,
        10,
    )
    .expect("incomplete recognized row remains readable");
    let reopened = observation(page.outcomes.into_iter().next().unwrap());
    assert_eq!(
        reopened.fragments[0].semantic_kind,
        SemanticKind::MalformedOrIncomplete
    );
    assert_eq!(reopened.native_event, raw);
    fs::remove_dir_all(root).unwrap();
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
        ProviderKind::DeepseekHarness,
    ]
    .into_iter()
    .map(adapter_manifest)
    .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn faithful_text_tool_terminal_paths_preserve_exact_raw_provider_events() {
    let cases = [
        (
            ProviderKind::Codex,
            json!({"type":"event_msg","payload":{"type":"agent_message","message":"done"}}),
            SemanticKind::AssistantResponse,
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
        (
            ProviderKind::DeepseekHarness,
            json!({"type":"turn/end","data":{"turn":1,"reason":{"kind":"completed"}}}),
            SemanticKind::TurnCompleted,
        ),
    ];
    for (provider, raw, expected_kind) in cases {
        let observation = observation(decode(provider, 1, raw));
        assert_eq!(observation.fragments[0].semantic_kind, expected_kind);
        assert_eq!(
            observation.fragments[0].visibility,
            FragmentVisibility::TeamSession
        );
        assert_eq!(
            observation.schema_version,
            PROVIDER_NATIVE_EVENT_RECORD_SCHEMA_VERSION
        );
        let projected = serde_json::to_string(&observation).expect("observation JSON");
        if projected.contains("never-project") {
            assert!(observation
                .native_event
                .to_string()
                .contains("never-project"));
        }
    }
}

#[test]
fn provider_reasoning_is_preserved_for_local_session_reads() {
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
        (
            ProviderKind::DeepseekHarness,
            json!({"type":"assistant/message","data":{"turn":1,"message":{"content":[{"type":"reasoning","text":"secret"}]}}}),
        ),
    ];
    for (provider, raw) in cases {
        let observation = observation(decode(provider, 1, raw));
        assert_eq!(
            observation.fragments[0].semantic_kind,
            SemanticKind::Reasoning
        );
        assert!(observation.native_event.to_string().contains("secret"));
    }
}

#[test]
fn server_context_remains_authoritative_while_hostile_native_fields_stay_visible() {
    let observation = observation(
        decode_native_event(
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
        .expect("native event remains readable"),
    );
    assert_eq!(observation.agent_session_id, "session-1");
    assert_eq!(observation.node_daemon_generation, 4);
    assert_eq!(
        observation.fragments[0].visibility,
        FragmentVisibility::TeamSession
    );
    assert_eq!(
        observation.native_event["agent_session_id"],
        "victim-session"
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
    assert_eq!(projection.episodes[0].records[0].ordering_position, 1);
    assert_eq!(projection.episodes[0].records[1].ordering_position, 2);

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
    changed.agent_member_id = "impersonated-agent".into();
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
    assert_eq!(projection.episodes[0].records[0].ordering_position, 4);
    assert_eq!(projection.episodes[0].records[1].ordering_position, 5);
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
fn authored_content_preserves_exact_provider_native_payload_without_filtering() {
    let observation = observation(decode(
        ProviderKind::Codex,
        1,
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"token=super-secret sk-12345678901234567890 /Users/alice/.ssh/id_ed25519 /tmp/provider.log C:\\Users\\alice\\secret Bearer bearer-secret"}}),
    ));
    let serialized = serde_json::to_string(&observation).unwrap();
    assert_eq!(observation.native_event["payload"]["type"], "agent_message");
    assert!(serialized.contains("super-secret"));
    assert!(serialized.contains("sk-123"));
    assert!(serialized.contains("/Users/alice"));
    assert!(serialized.contains("/tmp/provider.log"));
    assert!(serialized.contains("C:\\\\Users"));
    assert!(serialized.contains("bearer-secret"));
}

#[test]
fn stale_generation_fails_before_projection_change() {
    let mut stale_context = context(ProviderKind::Claude);
    stale_context.agent_session_generation = 6;
    let DecodeOutcome::Record(stale) = decode_native_event(
        &stale_context,
        NativeEvent {
            native_event_id: Some("stale".into()),
            provider_turn_id: Some("turn-1".into()),
            ordering_position: 1,
            occurred_at: None,
            raw: json!({"type":"result","usage":{"input_tokens":1,"output_tokens":2}}),
        },
    )
    .expect("decode");
    let stale = *stale;
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
    observation.fragments[0].effect_certainty = EffectCertainty::Unknown;
    observation.fragments[0].completeness = Completeness::RecoveryRequired;
    observation.fragments[0].semantic_kind = SemanticKind::CommandRecoveryRequired;
    observation.fragments[0].payload = FragmentPayload::Recovery {
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
        agent_session_id: "session-1".into(),
        agent_session_generation: 7,
    }
}

fn read_scope() -> ProjectionReadScope {
    ProjectionReadScope {
        execution_space_id: "space-1".into(),
        project_binding_id: "project-1".into(),
        team_id: "team-1".into(),
    }
}

#[test]
fn native_session_requires_exact_read_scope_without_content_visibility_policy() {
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

    assert!(
        firm_provider_events::project_team_session(&fold, &authority(), &read_scope(), 300).is_ok()
    );
    assert!(
        firm_provider_events::project_team_session(&fold, &authority(), &read_scope(), 300).is_ok()
    );
    let team =
        firm_provider_events::project_team_activity(&fold, &authority(), &read_scope()).unwrap();
    assert_eq!(team.len(), 1);
    assert!(!serde_json::to_string(&team).unwrap().contains("private"));

    let mut cross_space = read_scope();
    cross_space.execution_space_id = "space-2".into();
    assert_eq!(
        firm_provider_events::project_team_session(&fold, &authority(), &cross_space, 300),
        Err(ProjectionAccessError::CrossExecutionSpace)
    );

    let mut cross_binding = read_scope();
    cross_binding.project_binding_id = "project-2".into();
    assert_eq!(
        firm_provider_events::project_team_session(&fold, &authority(), &cross_binding, 300),
        Err(ProjectionAccessError::CrossProjectBinding)
    );

    let mut cross_team = read_scope();
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
    effect.fragments[0].effect_certainty = EffectCertainty::Applied;
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
    leaked.fragments[0].visibility = FragmentVisibility::TeamPublic;
    assert!(matches!(
        fold.ingest(leaked),
        Err(ProviderEventFoldError::InvalidObservation(_))
    ));
    assert_eq!(fold.snapshot_fingerprint(), before_fingerprint);
}

#[test]
fn five_provider_jsonl_corpora_decode_and_malformed_lines_are_bounded() {
    let cases = [
        (ProviderKind::Codex, include_str!("fixtures/codex.jsonl")),
        (ProviderKind::Claude, include_str!("fixtures/claude.jsonl")),
        (ProviderKind::Kimi, include_str!("fixtures/kimi.jsonl")),
        (ProviderKind::Pi, include_str!("fixtures/pi.jsonl")),
        (
            ProviderKind::DeepseekHarness,
            include_str!("fixtures/deepseek_harness.jsonl"),
        ),
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
        assert!(serialized.contains("not projected") || serialized.contains("private tool output"));
        assert!(observations
            .iter()
            .any(|item| item.fragments[0].semantic_kind == SemanticKind::TurnCompleted));
        if provider == ProviderKind::Kimi {
            assert!(observations.iter().any(|item| {
                matches!(
                    &item.fragments[0].payload,
                    FragmentPayload::Tool { call_id, .. }
                        if call_id.as_deref() == Some("kimi-call")
                )
            }));
            assert!(observations
                .iter()
                .all(|item| item.provider_turn_id.as_deref() == Some("kimi-turn")));
        } else if provider == ProviderKind::DeepseekHarness {
            assert!(observations.iter().any(|item| {
                matches!(
                    &item.fragments[0].payload,
                    FragmentPayload::AssistantResponse { text }
                        if text == "done together"
                )
            }));
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
    assert_eq!(
        malformed.fragments[0].semantic_kind,
        SemanticKind::MalformedOrIncomplete
    );
    assert_eq!(
        malformed.fragments[0].visibility,
        FragmentVisibility::OperatorOnly
    );
    assert!(matches!(
        malformed.native_event,
        serde_json::Value::String(_)
    ));
    assert!(serde_json::to_string(&malformed)
        .unwrap()
        .contains("private transcript"));
}

#[test]
fn pi_persisted_session_preserves_authored_failure_and_user_native_events() {
    let authored = observation(
        decode_native_json_line(
            &context(ProviderKind::Pi),
            Some("pi-session-message".into()),
            None,
            1,
            Some("2026-08-25T07:53:01Z".into()),
            r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"pi persisted answer"}],"stopReason":"stop"}}"#,
        )
        .unwrap(),
    );
    assert!(matches!(
        authored.fragments[0].payload,
        FragmentPayload::AssistantResponse { ref text } if text == "pi persisted answer"
    ));

    let failed = observation(
        decode_native_json_line(
            &context(ProviderKind::Pi),
            Some("pi-session-error".into()),
            None,
            2,
            Some("2026-08-25T07:53:14Z".into()),
            r#"{"type":"message","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"secret provider detail"}}"#,
        )
        .unwrap(),
    );
    assert_eq!(failed.fragments[0].semantic_kind, SemanticKind::TurnFailed);
    let serialized = serde_json::to_string(&failed).unwrap();
    assert!(serialized.contains("secret provider detail"));

    assert!(matches!(
        decode_native_json_line(
            &context(ProviderKind::Pi),
            Some("pi-user-message".into()),
            None,
            3,
            None,
            r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"private prompt"}]}}"#,
        )
        .unwrap(),
        DecodeOutcome::Record(_)
    ));
}

#[test]
fn codex_adjacent_native_message_envelopes_project_one_authored_response() {
    let root = unique_temp_path("codex-message-mirror");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("session.jsonl");
    fs::write(
        &path,
        concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_started\",\"turn_id\":\"turn-1\"}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"same authored response\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"same authored response\"}]}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"turn-1\"}}\n",
        ),
    )
    .unwrap();
    let boundary = TranscriptReadBoundary {
        allowed_root: root.clone(),
        transcript_path: path,
    };
    let latest = read_transcript_page(&context(ProviderKind::Codex), &boundary, None, 10)
        .expect("paged Codex projection");
    let observations = latest
        .outcomes
        .into_iter()
        .map(|outcome| {
            let DecodeOutcome::Record(value) = outcome;
            *value
        })
        .collect::<Vec<_>>();
    assert_eq!(
        observations
            .iter()
            .filter(|item| item.fragments[0].semantic_kind == SemanticKind::AssistantResponse)
            .count(),
        2
    );
    assert!(observations
        .iter()
        .any(|item| item.fragments[0].semantic_kind == SemanticKind::TurnCompleted));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_messages_without_proven_same_turn_are_never_folded() {
    let root = unique_temp_path("codex-message-without-turn");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("session.jsonl");
    fs::write(
        &path,
        concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"same text\"}}\n",
            "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"same text\"}]}}\n",
        ),
    )
    .unwrap();
    let latest = read_transcript_page(
        &context(ProviderKind::Codex),
        &TranscriptReadBoundary {
            allowed_root: root.clone(),
            transcript_path: path,
        },
        None,
        10,
    )
    .unwrap();
    assert_eq!(latest.outcomes.len(), 2);
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
    let result = read_transcript_page(
        &context(ProviderKind::Codex),
        &TranscriptReadBoundary {
            allowed_root: root.clone(),
            transcript_path: link,
        },
        None,
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
fn service_projects_team_and_public_views_on_demand_without_persistence() {
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
    assert_eq!(service.refresh_page(&boundary, None, 10).unwrap(), 2);
    assert_eq!(
        service
            .team_session(&authority(), &read_scope(), 300)
            .unwrap()
            .episodes
            .iter()
            .map(|episode| episode.records.len())
            .sum::<usize>(),
        2
    );
    assert_eq!(
        service
            .team_activity(&authority(), &read_scope())
            .unwrap()
            .len(),
        1
    );
    assert!(service
        .team_session(&authority(), &read_scope(), 300)
        .is_ok());
    assert_eq!(
        fs::read_dir(&root).unwrap().count(),
        1,
        "service creates no mirror"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_session_pages_are_lossless_non_overlapping_and_provider_ordered() {
    let root = unique_temp_path("paged-native-session");
    fs::create_dir_all(&root).unwrap();
    let transcript = root.join("codex.jsonl");
    let rows = (1..=5)
        .map(|index| {
            json!({
                "type":"event_msg",
                "payload":{"type":"agent_message","message":format!("raw-{index}")},
                "provider_extra":{"index":index}
            })
            .to_string()
        })
        .collect::<Vec<_>>();
    fs::write(&transcript, format!("{}\n", rows.join("\n"))).unwrap();
    let boundary = TranscriptReadBoundary {
        allowed_root: root.clone(),
        transcript_path: transcript,
    };

    let first = read_transcript_page(&context(ProviderKind::Codex), &boundary, None, 2).unwrap();
    assert!(first.has_more);
    assert_eq!(first.next_before_position, Some(4));
    let second = read_transcript_page(
        &context(ProviderKind::Codex),
        &boundary,
        first.next_before_position,
        2,
    )
    .unwrap();
    assert!(second.has_more);
    assert_eq!(second.next_before_position, Some(2));
    let third = read_transcript_page(
        &context(ProviderKind::Codex),
        &boundary,
        second.next_before_position,
        2,
    )
    .unwrap();
    assert!(!third.has_more);
    assert_eq!(third.next_before_position, None);

    let positions = third
        .outcomes
        .iter()
        .chain(second.outcomes.iter())
        .chain(first.outcomes.iter())
        .map(|outcome| {
            let DecodeOutcome::Record(observation) = outcome;
            assert_eq!(
                observation.native_event["provider_extra"]["index"],
                observation.ordering_position
            );
            observation.ordering_position
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, vec![1, 2, 3, 4, 5]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn paged_native_session_does_not_shorten_a_large_original_event() {
    let root = unique_temp_path("large-native-event");
    fs::create_dir_all(&root).unwrap();
    let transcript = root.join("codex.jsonl");
    let original = "x".repeat(1024 * 1024 + 257);
    fs::write(
        &transcript,
        format!(
            "{}\n",
            json!({
                "type":"response_item",
                "payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":original}]},
                "provider_raw_blob":original
            })
        ),
    )
    .unwrap();
    let page = read_transcript_page(
        &context(ProviderKind::Codex),
        &TranscriptReadBoundary {
            allowed_root: root.clone(),
            transcript_path: transcript,
        },
        None,
        1,
    )
    .unwrap();
    let DecodeOutcome::Record(observation) = &page.outcomes[0];
    assert_eq!(
        observation.native_event["provider_raw_blob"]
            .as_str()
            .unwrap()
            .len(),
        1024 * 1024 + 257
    );
    assert!(!page.has_more);
    fs::remove_dir_all(root).unwrap();
}

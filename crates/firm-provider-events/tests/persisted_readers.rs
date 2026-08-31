use std::{
    collections::BTreeSet,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use firm_provider_events::{
    persisted_adapter_manifest, read_persisted_file_page, read_persisted_jsonl_snapshot,
    read_persisted_jsonl_snapshot_after, ContentAvailability, ContentUnavailableReason,
    NativeClassificationReason, PersistedFileBoundary, PersistedFragmentPayload,
    PersistedProjectionContext, PersistedReaderSource, PersistedSessionProjector,
    PersistedTailMode, ProviderKind, SessionSemanticKind, ToolCallOutcome, ToolOperationCategory,
};

fn source(provider: ProviderKind) -> PersistedReaderSource {
    let manifest = persisted_adapter_manifest(provider);
    PersistedReaderSource {
        provider,
        native_session_id: format!("{provider:?}-session"),
        source_family: manifest.persisted_source_families[0].clone(),
        format_version_fence: manifest.format_version_fences[0].clone(),
    }
}

fn corpus(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Codex => include_str!("fixtures/v3/codex.jsonl"),
        ProviderKind::Claude => include_str!("fixtures/v3/claude.jsonl"),
        ProviderKind::Kimi => include_str!("fixtures/v3/kimi.jsonl"),
        ProviderKind::Pi => include_str!("fixtures/v3/pi.jsonl"),
        ProviderKind::DeepseekHarness => include_str!("fixtures/v3/deepseek_harness.jsonl"),
    }
}

fn capability_corpus(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Codex => include_str!("fixtures/v3/capabilities/codex.jsonl"),
        ProviderKind::Claude => include_str!("fixtures/v3/capabilities/claude.jsonl"),
        ProviderKind::Kimi => include_str!("fixtures/v3/capabilities/kimi.jsonl"),
        ProviderKind::Pi => include_str!("fixtures/v3/capabilities/pi.jsonl"),
        ProviderKind::DeepseekHarness => {
            include_str!("fixtures/v3/capabilities/deepseek_harness.jsonl")
        }
    }
}

fn project(
    provider: ProviderKind,
    content: &str,
) -> Vec<firm_provider_events::ProviderNativeEventRecord> {
    let page = read_persisted_jsonl_snapshot(&source(provider), content, None, 128)
        .expect("persisted rows");
    let mut projector = page
        .projector(PersistedProjectionContext {
            native_source_ref: page.native_source_ref.clone(),
            agent_member_id: "member-1".into(),
            agent_session_id: "session-1".into(),
            agent_session_generation: 7,
            observed_at: "2026-08-29T00:00:00Z".into(),
        })
        .expect("projector");
    page.rows
        .into_iter()
        .map(|row| projector.project(row).expect("v3 record"))
        .collect()
}

#[test]
fn paged_tool_result_keeps_exact_prior_tool_name_without_cursor_payload() {
    let content = concat!(
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"id\":\"call-1\",\"name\":\"Read\"}]}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"working\"}]}}\n",
        "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"call-1\"}]}}\n",
    );
    let page = read_persisted_jsonl_snapshot(&source(ProviderKind::Claude), content, None, 1)
        .expect("last persisted row");
    assert!(page.has_more);
    let mut projector = page
        .projector(PersistedProjectionContext {
            native_source_ref: page.native_source_ref.clone(),
            agent_member_id: "member-1".into(),
            agent_session_id: "session-1".into(),
            agent_session_generation: 1,
            observed_at: "2026-08-29T00:00:00Z".into(),
        })
        .expect("seeded projector");
    let record = projector
        .project(page.rows[0].clone())
        .expect("paged tool result");
    assert!(matches!(
        &record.fragments[0].payload,
        PersistedFragmentPayload::Tool { tool_name, call_id: Some(call_id), .. }
            if tool_name.as_deref() == Some("Read") && call_id == "call-1"
    ));
}

fn kinds(
    records: &[firm_provider_events::ProviderNativeEventRecord],
) -> BTreeSet<SessionSemanticKind> {
    records
        .iter()
        .flat_map(|record| {
            record
                .fragments
                .iter()
                .map(|fragment| fragment.semantic_kind)
        })
        .collect()
}

#[test]
fn five_provider_persisted_corpora_reach_only_honest_manifest_capabilities() {
    for provider in [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Kimi,
        ProviderKind::Pi,
        ProviderKind::DeepseekHarness,
    ] {
        let complete_corpus = format!(
            "{}{}{{\"type\":\"reviewed_unknown_row\"}}\n{{not-json}}\n",
            corpus(provider),
            capability_corpus(provider)
        );
        let records = project(provider, &complete_corpus);
        assert!(!records.is_empty(), "{provider:?} persisted corpus");
        assert!(records.iter().all(|record| record.validate().is_ok()));
        assert_eq!(
            records
                .iter()
                .map(|record| &record.record_id)
                .collect::<BTreeSet<_>>()
                .len(),
            records.len(),
            "{provider:?} record identity must be one-to-one with rows"
        );
        let claimed = persisted_adapter_manifest(provider)
            .semantic_capabilities
            .into_iter()
            .map(|capability| capability.semantic_kind)
            .collect::<BTreeSet<_>>();
        assert_eq!(kinds(&records), claimed, "{provider:?} claims");
    }
}

#[test]
fn five_provider_tools_expose_readable_exact_response_local_contract() {
    let expectations = [
        (
            ProviderKind::Codex,
            "exec",
            ToolOperationCategory::Command,
            "cargo test -p firm-provider-events",
        ),
        (
            ProviderKind::Claude,
            "Read",
            ToolOperationCategory::Read,
            "AGENTS.md",
        ),
        (
            ProviderKind::Kimi,
            "Read",
            ToolOperationCategory::Read,
            "docs/current/architecture/agent-runtime.md",
        ),
        (
            ProviderKind::Pi,
            "Read",
            ToolOperationCategory::Read,
            "package.json",
        ),
        (
            ProviderKind::DeepseekHarness,
            "bash",
            ToolOperationCategory::Command,
            "rg -n NodeDaemonLease crates/",
        ),
    ];
    for (provider, expected_name, expected_category, expected_target) in expectations {
        let records = project(provider, corpus(provider));
        let requested = records
            .iter()
            .flat_map(|record| {
                record
                    .fragments
                    .iter()
                    .map(move |fragment| (record, fragment))
            })
            .find(|(_, fragment)| {
                matches!(
                    fragment.semantic_kind,
                    SessionSemanticKind::ToolCallRequested | SessionSemanticKind::ToolCallStarted
                )
            })
            .expect("tool request/start");
        let PersistedFragmentPayload::Tool {
            tool_name,
            call_id,
            operation_category,
            primary_target,
            arguments: Some(arguments),
            outcome,
            ..
        } = &requested.1.payload
        else {
            panic!("readable tool request/start payload for {provider:?}");
        };
        assert_eq!(tool_name.as_deref(), Some(expected_name));
        assert!(call_id.as_ref().is_some_and(|value| !value.is_empty()));
        assert_eq!(*operation_category, Some(expected_category));
        assert_eq!(primary_target.as_deref(), Some(expected_target));
        assert_eq!(arguments.availability, ContentAvailability::Available);
        assert!(requested
            .0
            .native_event
            .pointer(
                arguments
                    .json_pointer
                    .as_deref()
                    .expect("arguments pointer")
            )
            .is_some());
        assert!(matches!(
            outcome,
            Some(ToolCallOutcome::Requested | ToolCallOutcome::Started)
        ));

        let completed = records
            .iter()
            .flat_map(|record| {
                record
                    .fragments
                    .iter()
                    .map(move |fragment| (record, fragment))
            })
            .find(|(_, fragment)| fragment.semantic_kind == SessionSemanticKind::ToolCallCompleted)
            .expect("tool completion");
        let PersistedFragmentPayload::Tool {
            tool_name,
            call_id: completed_call_id,
            result: Some(result),
            outcome: Some(ToolCallOutcome::Completed),
            ..
        } = &completed.1.payload
        else {
            panic!("readable tool result payload for {provider:?}");
        };
        assert_eq!(tool_name.as_deref(), Some(expected_name));
        assert_eq!(completed_call_id, call_id);
        assert_eq!(result.availability, ContentAvailability::Available);
        assert!(completed
            .0
            .native_event
            .pointer(result.json_pointer.as_deref().expect("result pointer"))
            .is_some());
    }
}

#[test]
fn five_provider_failed_tools_keep_typed_outcome_and_missing_content_reason() {
    for provider in [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Kimi,
        ProviderKind::Pi,
        ProviderKind::DeepseekHarness,
    ] {
        let records = project(provider, capability_corpus(provider));
        let failed = records
            .iter()
            .flat_map(|record| &record.fragments)
            .find(|fragment| fragment.semantic_kind == SessionSemanticKind::ToolCallFailed)
            .expect("failed tool fragment");
        assert!(matches!(
            &failed.payload,
            PersistedFragmentPayload::Tool {
                call_id: Some(call_id),
                outcome: Some(ToolCallOutcome::Failed),
                error: Some(error),
                ..
            } if !call_id.is_empty()
                && error.availability == ContentAvailability::Unavailable
                && error.unavailable_reason == Some(ContentUnavailableReason::ProviderAbsent)
                && error.json_pointer.is_none()
        ));
    }
}

#[test]
fn missing_related_tool_request_is_typed_and_never_guessed_by_adjacency() {
    let records = project(
        ProviderKind::Claude,
        concat!(
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"nearby but unrelated\"}]}}\n",
            "{\"type\":\"user\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"orphan-call\",\"content\":\"output\"}]}}\n",
        ),
    );
    let fragment = &records[1].fragments[0];
    assert_eq!(
        fragment.semantic_kind,
        SessionSemanticKind::ToolCallCompleted
    );
    assert!(matches!(
        &fragment.payload,
        PersistedFragmentPayload::Tool {
            tool_name: None,
            tool_name_unavailable_reason: Some(ContentUnavailableReason::RelatedRecordMissing),
            call_id: Some(call_id),
            ..
        } if call_id == "orphan-call"
    ));
}

#[test]
fn pi_role_tool_result_without_call_id_stays_structured_and_independent() {
    let records = project(
        ProviderKind::Pi,
        concat!(
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"toolCall\",\"id\":\"call-1\",\"name\":\"bash\",\"arguments\":{\"command\":\"exit 7\"}}]}}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"content\":[{\"type\":\"text\",\"text\":\"expected-tool-error\\nCommand exited with code 7\"}]}}\n",
        ),
    );
    let fragment = &records[1].fragments[0];
    assert!(matches!(
        &fragment.payload,
        PersistedFragmentPayload::Tool {
            call_id: None,
            tool_name: None,
            tool_name_unavailable_reason: Some(ContentUnavailableReason::RelatedRecordMissing),
            result: Some(result),
            outcome: Some(ToolCallOutcome::Completed),
            display_detail: Some(detail),
            ..
        } if result.json_pointer.as_deref() == Some("/message/content/0/text")
            && detail.contains("remains independent")
    ));
    assert_eq!(
        records[1]
            .native_event
            .pointer("/message/content/0/text")
            .and_then(serde_json::Value::as_str),
        Some("expected-tool-error\nCommand exited with code 7")
    );
}

#[test]
fn deepseek_reviewed_chunk_rows_project_without_losing_raw_boundaries() {
    let records = project(
        ProviderKind::DeepseekHarness,
        concat!(
            "{\"type\":\"reasoning-chunks\",\"seq0\":1,\"time0\":10,\"data\":{\"turn\":1,\"step\":1,\"index\":0,\"dt\":[1],\"texts\":[\"why \",\"now\"]}}\n",
            "{\"type\":\"text-chunks\",\"seq0\":3,\"time0\":12,\"data\":{\"turn\":1,\"step\":1,\"index\":1,\"dt\":[1],\"texts\":[\"done\",\".\"]}}\n",
            "{\"type\":\"tool-call-chunks\",\"seq0\":5,\"time0\":14,\"data\":{\"turn\":1,\"step\":1,\"index\":2,\"dt\":[1],\"id\":\"call-2\",\"name\":\"bash\",\"args\":[\"{\\\"command\\\":\\\"rg\\\"}\",\"\"]}}\n",
            "{\"type\":\"tool-call-chunks\",\"seq0\":7,\"time0\":16,\"data\":{\"turn\":1,\"step\":1,\"index\":2,\"dt\":[1],\"id\":\"call-2\",\"args\":[\" \",\"--files\"]}}\n",
        ),
    );
    assert_eq!(records[0].native_event["data"]["texts"][0], "why ");
    assert!(matches!(
        &records[0].fragments[0].payload,
        PersistedFragmentPayload::Reasoning { text: Some(text) } if text == "why now"
    ));
    assert_eq!(
        records[0].fragments[0].completeness,
        firm_provider_events::PersistedCompleteness::Partial
    );
    assert!(matches!(
        &records[1].fragments[0].payload,
        PersistedFragmentPayload::AssistantResponse { text: Some(text) } if text == "done."
    ));
    assert!(
        matches!(
            &records[2].fragments[0].payload,
            PersistedFragmentPayload::Tool {
                call_id: Some(call_id),
                tool_name: Some(name),
                arguments: Some(arguments),
                ..
            } if call_id == "call-2" && name == "bash" && arguments.json_pointer.as_deref() == Some("/data/args")
        ),
        "tool chunk projection: {:?}",
        records[2].fragments[0].payload
    );
    assert!(matches!(
        &records[3].fragments[0].payload,
        PersistedFragmentPayload::Tool {
            call_id: Some(call_id),
            tool_name: Some(name),
            arguments: Some(arguments),
            ..
        } if call_id == "call-2" && name == "bash" && arguments.json_pointer.as_deref() == Some("/data/args")
    ));
}

#[test]
fn unclassified_native_keeps_type_subtype_and_typed_reason() {
    let records = project(
        ProviderKind::Claude,
        "{\"type\":\"future_event\",\"subtype\":\"background_update\",\"payload\":{\"private\":true}}\n",
    );
    assert!(matches!(
        &records[0].fragments[0].payload,
        PersistedFragmentPayload::Native {
            event_type: Some(event_type),
            event_subtype: Some(event_subtype),
            classification_reason: Some(NativeClassificationReason::UnsupportedEventType),
        } if event_type == "future_event" && event_subtype == "background_update"
    ));
    assert_eq!(records[0].native_event["payload"]["private"], true);

    let codex = project(
        ProviderKind::Codex,
        "{\"type\":\"response_item\",\"payload\":{\"type\":\"future_item\",\"private\":true}}\n",
    );
    assert!(matches!(
        &codex[0].fragments[0].payload,
        PersistedFragmentPayload::Native {
            event_type: Some(event_type),
            event_subtype: Some(event_subtype),
            classification_reason: Some(NativeClassificationReason::UnsupportedEventType),
        } if event_type == "response_item" && event_subtype == "future_item"
    ));

    let kimi = project(
        ProviderKind::Kimi,
        "{\"type\":\"context.append_loop_event\",\"event\":{\"type\":\"future.event\",\"private\":true}}\n",
    );
    assert!(matches!(
        &kimi[0].fragments[0].payload,
        PersistedFragmentPayload::Native {
            event_type: Some(event_type),
            event_subtype: Some(event_subtype),
            classification_reason: Some(NativeClassificationReason::UnsupportedEventType),
        } if event_type == "context.append_loop_event" && event_subtype == "future.event"
    ));
}

#[test]
fn structured_provider_errors_skip_null_result_and_reference_exact_error() {
    let cases = [
        (
            ProviderKind::Kimi,
            concat!(
                "{\"type\":\"context.append_loop_event\",\"event\":{\"type\":\"tool.call\",\"name\":\"Read\",\"toolCallId\":\"call-1\"}}\n",
                "{\"type\":\"context.append_loop_event\",\"event\":{\"type\":\"tool.result\",\"toolCallId\":\"call-1\",\"status\":\"failed\",\"result\":null,\"error\":{\"code\":\"EIO\"}}}\n",
            ),
            "/event/error",
        ),
        (
            ProviderKind::DeepseekHarness,
            concat!(
                "{\"type\":\"tool/call\",\"data\":{\"name\":\"bash\",\"callId\":\"call-1\"}}\n",
                "{\"type\":\"tool/result\",\"data\":{\"message\":{\"source\":{\"callId\":\"call-1\"}},\"status\":\"failed\",\"result\":null,\"error\":{\"code\":\"EIO\"}}}\n",
            ),
            "/data/error",
        ),
    ];
    for (provider, content, expected_pointer) in cases {
        let records = project(provider, content);
        let (record, failed) = records
            .iter()
            .flat_map(|record| {
                record
                    .fragments
                    .iter()
                    .map(move |fragment| (record, fragment))
            })
            .find(|(_, fragment)| fragment.semantic_kind == SessionSemanticKind::ToolCallFailed)
            .expect("failed tool fragment");
        let PersistedFragmentPayload::Tool {
            error: Some(error),
            outcome: Some(ToolCallOutcome::Failed),
            ..
        } = &failed.payload
        else {
            panic!("structured provider error for {provider:?}");
        };
        assert_eq!(error.availability, ContentAvailability::Available);
        assert_eq!(error.json_pointer.as_deref(), Some(expected_pointer));
        assert!(!record
            .native_event
            .pointer(expected_pointer)
            .expect("exact error pointer")
            .is_null());
    }
}

#[test]
fn checked_in_persisted_manifests_are_exact_rust_claims() {
    let checked_in: Vec<firm_provider_events::PersistedAdapterManifest> = serde_json::from_str(
        include_str!("../../../schemas/provider-events/persisted-adapters.v3.json"),
    )
    .expect("persisted manifests JSON");
    let rust = [
        ProviderKind::Codex,
        ProviderKind::Claude,
        ProviderKind::Kimi,
        ProviderKind::Pi,
        ProviderKind::DeepseekHarness,
    ]
    .into_iter()
    .map(persisted_adapter_manifest)
    .collect::<Vec<_>>();
    assert_eq!(checked_in, rust);
    assert!(checked_in
        .iter()
        .all(|manifest| manifest.validate().is_ok()));
}

#[test]
fn codex_persisted_item_discriminator_and_unavailable_reasoning_are_exact() {
    let records = project(ProviderKind::Codex, corpus(ProviderKind::Codex));
    let reasoning = records
        .iter()
        .flat_map(|record| &record.fragments)
        .find(|fragment| fragment.semantic_kind == SessionSemanticKind::Reasoning)
        .expect("reasoning occurrence");
    assert_eq!(
        reasoning.content_availability,
        ContentAvailability::Unavailable
    );
    assert_eq!(
        reasoning.content_unavailable_reason,
        Some(ContentUnavailableReason::ProviderAbsent)
    );
    assert!(matches!(
        &reasoning.payload,
        PersistedFragmentPayload::Reasoning { text: None }
    ));
    assert!(records
        .iter()
        .flat_map(|record| &record.fragments)
        .any(|fragment| { fragment.semantic_kind == SessionSemanticKind::AssistantResponse }));
    assert!(records
        .iter()
        .flat_map(|record| &record.fragments)
        .any(|fragment| { fragment.semantic_kind == SessionSemanticKind::ToolCallCompleted }));
}

#[test]
fn claude_user_tool_result_reaches_the_exact_prior_call_without_user_message_semantics() {
    let records = project(ProviderKind::Claude, corpus(ProviderKind::Claude));
    let assistant_texts = records
        .iter()
        .flat_map(|record| &record.fragments)
        .filter_map(|fragment| match &fragment.payload {
            PersistedFragmentPayload::AssistantResponse { text } => text.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(assistant_texts, ["claude answer"]);
    let completed = records
        .iter()
        .flat_map(|record| &record.fragments)
        .find(|fragment| fragment.semantic_kind == SessionSemanticKind::ToolCallCompleted)
        .expect("Claude tool result");
    assert!(matches!(
        &completed.payload,
        PersistedFragmentPayload::Tool { tool_name, call_id: Some(call_id), .. }
            if tool_name.as_deref() == Some("Read") && call_id == "claude-call"
    ));
}

#[test]
fn kimi_uses_only_wire_rows_and_preserves_reasoning_response_tool_and_turn_order() {
    let records = project(ProviderKind::Kimi, corpus(ProviderKind::Kimi));
    let ordered = records
        .iter()
        .flat_map(|record| &record.fragments)
        .map(|fragment| fragment.semantic_kind)
        .collect::<Vec<_>>();
    assert_eq!(
        ordered,
        [
            SessionSemanticKind::Reasoning,
            SessionSemanticKind::AssistantResponse,
            SessionSemanticKind::ToolCallRequested,
            SessionSemanticKind::ToolCallCompleted,
            SessionSemanticKind::TurnCompleted,
        ]
    );
}

#[test]
fn managed_pi_manifest_and_projection_never_claim_reasoning_or_rpc_updates() {
    let manifest = persisted_adapter_manifest(ProviderKind::Pi);
    assert!(!manifest
        .semantic_capabilities
        .iter()
        .any(|capability| capability.semantic_kind == SessionSemanticKind::Reasoning));
    let records = project(ProviderKind::Pi, corpus(ProviderKind::Pi));
    let assistant_texts = records
        .iter()
        .flat_map(|record| &record.fragments)
        .filter_map(|fragment| match &fragment.payload {
            PersistedFragmentPayload::AssistantResponse { text } => text.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(assistant_texts, ["pi answer"]);
    assert!(!kinds(&records).contains(&SessionSemanticKind::Reasoning));
    assert!(kinds(&records).contains(&SessionSemanticKind::AssistantResponse));
    assert!(kinds(&records).contains(&SessionSemanticKind::ToolCallCompleted));

    let rpc = project(
        ProviderKind::Pi,
        "{\"type\":\"message_update\",\"content\":[{\"type\":\"text\",\"text\":\"live only\"}]}\n",
    );
    assert_eq!(
        rpc[0].fragments[0].semantic_kind,
        SessionSemanticKind::UnclassifiedNative
    );
}

#[test]
fn deepseek_official_reader_is_bounded_snapshot_diff_and_claims_reviewed_reasoning() {
    let manifest = persisted_adapter_manifest(ProviderKind::DeepseekHarness);
    assert_eq!(manifest.tail_mode, PersistedTailMode::BoundedSnapshotDiff);
    assert!(!manifest.pagination);
    assert!(manifest
        .semantic_capabilities
        .iter()
        .any(|capability| capability.semantic_kind == SessionSemanticKind::Reasoning));
    let records = project(
        ProviderKind::DeepseekHarness,
        corpus(ProviderKind::DeepseekHarness),
    );
    assert!(kinds(&records).contains(&SessionSemanticKind::AssistantResponse));
    assert!(kinds(&records).contains(&SessionSemanticKind::UsageReported));
    assert!(kinds(&records).contains(&SessionSemanticKind::TurnCompleted));
    assert!(
        records.iter().all(|record| record.occurred_at.is_none()),
        "DeepSeek's unitless numeric time is source ordering evidence, not a cross-plane timestamp"
    );
}

#[test]
fn reread_keeps_row_identity_and_format_fence_changes_source_generation() {
    let first_page = read_persisted_jsonl_snapshot(
        &source(ProviderKind::Codex),
        corpus(ProviderKind::Codex),
        None,
        128,
    )
    .expect("first read");
    let second_page = read_persisted_jsonl_snapshot(
        &source(ProviderKind::Codex),
        corpus(ProviderKind::Codex),
        None,
        128,
    )
    .expect("reopen read");
    assert_eq!(first_page.source_generation, second_page.source_generation);
    assert_eq!(first_page.rows, second_page.rows);
    let projection_context = |page: &firm_provider_events::PersistedRowPage, observed_at: &str| {
        PersistedProjectionContext {
            native_source_ref: page.native_source_ref.clone(),
            agent_member_id: "member-1".into(),
            agent_session_id: "session-1".into(),
            agent_session_generation: 7,
            observed_at: observed_at.into(),
        }
    };
    let mut initial = first_page
        .projector(projection_context(&first_page, "2026-08-29T00:00:00Z"))
        .expect("initial tail projector");
    let mut reopened = second_page
        .projector(projection_context(&second_page, "2026-08-29T00:01:00Z"))
        .expect("reopen projector");
    for (initial_row, reopened_row) in first_page.rows.iter().zip(&second_page.rows) {
        let initial_record = initial
            .project(initial_row.clone())
            .expect("initial record");
        let reopened_record = reopened
            .project(reopened_row.clone())
            .expect("reopened record");
        assert_eq!(initial_record.record_id, reopened_record.record_id);
        assert_eq!(initial_record.ordering_key, reopened_record.ordering_key);
        assert_eq!(initial_record.fragments, reopened_record.fragments);
    }

    let kimi_current = read_persisted_jsonl_snapshot(
        &source(ProviderKind::Kimi),
        corpus(ProviderKind::Kimi),
        None,
        128,
    )
    .expect("current Kimi format");
    let mut changed = source(ProviderKind::Kimi);
    changed.format_version_fence = "kimi.wire.legacy.v1".into();
    let changed_page =
        read_persisted_jsonl_snapshot(&changed, corpus(ProviderKind::Kimi), None, 128)
            .expect("format reset read");
    assert_ne!(
        kimi_current.source_generation,
        changed_page.source_generation
    );

    let mut unsupported = source(ProviderKind::Codex);
    unsupported.format_version_fence = "codex.rollout.future.v2".into();
    assert!(
        read_persisted_jsonl_snapshot(&unsupported, corpus(ProviderKind::Codex), None, 128,)
            .is_err()
    );
}

#[test]
fn complete_malformed_row_is_visible_but_incomplete_tail_is_not_consumed() {
    let malformed = read_persisted_jsonl_snapshot(
        &source(ProviderKind::Claude),
        "{not-json}\n{\"type\":\"assistant\"",
        None,
        10,
    )
    .expect("bounded malformed read");
    assert!(malformed.incomplete_tail);
    assert_eq!(malformed.rows.len(), 1);
    let mut projector = PersistedSessionProjector::new(PersistedProjectionContext {
        native_source_ref: malformed.native_source_ref,
        agent_member_id: "member-1".into(),
        agent_session_id: "session-1".into(),
        agent_session_generation: 1,
        observed_at: "2026-08-29T00:00:00Z".into(),
    })
    .expect("projector");
    let record = projector
        .project(malformed.rows[0].clone())
        .expect("malformed record");
    assert_eq!(
        record.fragments[0].semantic_kind,
        SessionSemanticKind::MalformedOrIncomplete
    );
}

#[test]
fn file_tail_and_reopen_keep_completed_row_identity_and_ignore_active_tail() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "firm-provider-events-v3-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary provider root");
    let transcript = root.join("session.jsonl");
    fs::write(
        &transcript,
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"first\"}]}}\n",
    )
    .expect("initial provider row");
    let boundary = PersistedFileBoundary {
        allowed_root: root.clone(),
        transcript_path: transcript.clone(),
    };
    let first = read_persisted_file_page(&source(ProviderKind::Claude), &boundary, None, 10)
        .expect("first file read");
    let first_identity = first.rows[0].clone();

    fs::write(
        &transcript,
        concat!(
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"first\"}]}}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"second\"}]}}\n",
            "{\"type\":\"assistant\""
        ),
    )
    .expect("provider append snapshot");
    let reopened = read_persisted_file_page(&source(ProviderKind::Claude), &boundary, None, 10)
        .expect("reopened file read");
    assert!(reopened.incomplete_tail);
    assert_eq!(reopened.rows.len(), 2);
    assert_eq!(reopened.rows[0], first_identity);

    fs::rename(&transcript, root.join("session.previous.jsonl")).expect("rotate provider file");
    fs::write(
        &transcript,
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"new incarnation\"}]}}\n",
    )
    .expect("replacement provider file");
    let reset = read_persisted_file_page(&source(ProviderKind::Claude), &boundary, None, 10)
        .expect("replacement file read");
    assert_ne!(reopened.source_generation, reset.source_generation);
    assert_eq!(reset.rows.len(), 1);

    fs::remove_dir_all(root).expect("remove temporary provider root");
}

#[test]
fn snapshot_watermark_then_forward_pages_have_no_gap_or_duplicate() {
    let initial = concat!(
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"one\"}]}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"two\"}]}}\n",
    );
    let snapshot = read_persisted_jsonl_snapshot(&source(ProviderKind::Claude), initial, None, 10)
        .expect("initial snapshot");
    let watermark = snapshot.snapshot_watermark.expect("snapshot watermark");
    let appended = format!(
        "{initial}{}{}{}",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"three\"}]}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"four\"}]}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"five\"}]}}\n",
    );
    let first =
        read_persisted_jsonl_snapshot_after(&source(ProviderKind::Claude), &appended, watermark, 2)
            .expect("first forward page");
    assert_eq!(first.rows.len(), 2);
    assert!(first.has_more);
    let next = first.rows.last().expect("last first-page row").ordering_key;
    let second =
        read_persisted_jsonl_snapshot_after(&source(ProviderKind::Claude), &appended, next, 2)
            .expect("second forward page");
    assert_eq!(second.rows.len(), 1);
    assert!(!second.has_more);
    let all = first
        .rows
        .iter()
        .chain(&second.rows)
        .map(|row| row.ordering_key.value)
        .collect::<BTreeSet<_>>();
    assert_eq!(all.len(), 3);
    assert!(all.iter().all(|position| *position > watermark.value));
}

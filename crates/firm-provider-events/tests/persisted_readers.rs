use std::{
    collections::BTreeSet,
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

use firm_provider_events::{
    persisted_adapter_manifest, read_persisted_file_page, read_persisted_jsonl_snapshot,
    read_persisted_jsonl_snapshot_after, ContentAvailability, PersistedFileBoundary,
    PersistedFragmentPayload, PersistedProjectionContext, PersistedReaderSource,
    PersistedSessionProjector, PersistedTailMode, ProviderKind, SessionSemanticKind,
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
            if tool_name == "Read" && call_id == "call-1"
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
            if tool_name == "Read" && call_id == "claude-call"
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
fn deepseek_official_reader_is_bounded_snapshot_diff_and_does_not_claim_reasoning() {
    let manifest = persisted_adapter_manifest(ProviderKind::DeepseekHarness);
    assert_eq!(manifest.tail_mode, PersistedTailMode::BoundedSnapshotDiff);
    assert!(!manifest.pagination);
    assert!(!manifest
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

use crate::{
    ContentAvailability, PersistedAdapterManifest, PersistedReaderReachability,
    PersistedSemanticCapability, PersistedTailMode, ProviderKind, SessionLifecyclePhase,
    SessionSemanticKind, PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION,
};

pub fn persisted_adapter_manifest(provider: ProviderKind) -> PersistedAdapterManifest {
    let (families, fences, pagination, tail_mode, capabilities) = match provider {
        ProviderKind::Codex => (
            vec!["codex_rollout_jsonl"],
            vec!["codex.rollout.session_meta.v1"],
            true,
            PersistedTailMode::Incremental,
            vec![
                capability(
                    SessionSemanticKind::SessionMetadata,
                    &[SessionLifecyclePhase::Started],
                    false,
                ),
                capability(
                    SessionSemanticKind::Reasoning,
                    &[SessionLifecyclePhase::Progress],
                    true,
                ),
                capability(
                    SessionSemanticKind::AssistantResponse,
                    &[SessionLifecyclePhase::Progress],
                    true,
                ),
                capability(
                    SessionSemanticKind::ToolCallRequested,
                    &[SessionLifecyclePhase::Requested],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::UsageReported,
                    &[SessionLifecyclePhase::Progress],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCancelled,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
            ],
        ),
        ProviderKind::Claude => (
            vec!["claude_native_session_jsonl"],
            vec!["claude.stream_json.v1"],
            true,
            PersistedTailMode::Incremental,
            vec![
                capability(
                    SessionSemanticKind::SessionMetadata,
                    &[SessionLifecyclePhase::Started],
                    false,
                ),
                capability(
                    SessionSemanticKind::Reasoning,
                    &[SessionLifecyclePhase::Progress],
                    true,
                ),
                capability(
                    SessionSemanticKind::AssistantResponse,
                    &[SessionLifecyclePhase::Progress],
                    true,
                ),
                capability(
                    SessionSemanticKind::ToolCallRequested,
                    &[SessionLifecyclePhase::Requested],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::UsageReported,
                    &[SessionLifecyclePhase::Progress],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
            ],
        ),
        ProviderKind::Kimi => (
            vec!["kimi_wire_jsonl"],
            vec!["kimi.wire.current.v1", "kimi.wire.legacy.v1"],
            true,
            PersistedTailMode::Incremental,
            vec![
                capability(
                    SessionSemanticKind::Reasoning,
                    &[SessionLifecyclePhase::Progress],
                    true,
                ),
                capability(
                    SessionSemanticKind::AssistantResponse,
                    &[SessionLifecyclePhase::Progress],
                    true,
                ),
                capability(
                    SessionSemanticKind::ToolCallRequested,
                    &[SessionLifecyclePhase::Requested],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::ArtifactCreated,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCancelled,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
            ],
        ),
        ProviderKind::Pi => (
            vec!["managed_pi_session_jsonl"],
            vec!["pi.session_jsonl.thinking_off.v1"],
            true,
            PersistedTailMode::Incremental,
            vec![
                capability(
                    SessionSemanticKind::SessionMetadata,
                    &[SessionLifecyclePhase::Started],
                    false,
                ),
                capability(
                    SessionSemanticKind::AssistantResponse,
                    &[SessionLifecyclePhase::Progress],
                    true,
                ),
                capability(
                    SessionSemanticKind::ToolCallRequested,
                    &[SessionLifecyclePhase::Requested],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::ArtifactCreated,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCancelled,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
            ],
        ),
        ProviderKind::DeepseekHarness => (
            vec!["deepseek_official_session_reader"],
            vec!["deepseek.session_reader.v1"],
            false,
            PersistedTailMode::BoundedSnapshotDiff,
            vec![
                capability(
                    SessionSemanticKind::AssistantResponse,
                    &[SessionLifecyclePhase::Progress],
                    true,
                ),
                capability(
                    SessionSemanticKind::ToolCallStarted,
                    &[SessionLifecyclePhase::Started],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::ToolCallFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::UsageReported,
                    &[SessionLifecyclePhase::Progress],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCompleted,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnFailed,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
                capability(
                    SessionSemanticKind::TurnCancelled,
                    &[SessionLifecyclePhase::Terminal],
                    false,
                ),
            ],
        ),
    };
    let mut semantic_capabilities = capabilities;
    semantic_capabilities.extend([
        capability(
            SessionSemanticKind::MalformedOrIncomplete,
            &[SessionLifecyclePhase::Terminal],
            false,
        ),
        capability(
            SessionSemanticKind::UnclassifiedNative,
            &[SessionLifecyclePhase::Progress],
            false,
        ),
    ]);
    PersistedAdapterManifest {
        provider,
        adapter_version: PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION.into(),
        persisted_source_families: families.into_iter().map(str::to_owned).collect(),
        format_version_fences: fences.into_iter().map(str::to_owned).collect(),
        source_generation: true,
        stable_row_locator: true,
        pagination,
        tail_mode,
        // Remote callers traverse the existing NodeGateway to the same
        // machine-local reader; they never receive a provider path or mount.
        reader_reachability: vec![
            PersistedReaderReachability::Local,
            PersistedReaderReachability::Remote,
        ],
        semantic_capabilities,
    }
}

fn capability(
    semantic_kind: SessionSemanticKind,
    phases: &[SessionLifecyclePhase],
    text_may_be_unavailable: bool,
) -> PersistedSemanticCapability {
    let mut content_availability = vec![ContentAvailability::Available];
    if text_may_be_unavailable {
        content_availability.push(ContentAvailability::Unavailable);
    }
    PersistedSemanticCapability {
        semantic_kind,
        phases: phases.to_vec(),
        content_availability,
    }
}

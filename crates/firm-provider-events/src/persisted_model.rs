use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const PROVIDER_NATIVE_EVENT_RECORD_V3_SCHEMA_VERSION: &str =
    "agentfirm.provider_native_event_record.v3";
pub const PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION: &str =
    "agentfirm.persisted_provider_event_adapter.v3";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Codex,
    Claude,
    Kimi,
    Pi,
    DeepseekHarness,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Kimi => "kimi",
            Self::Pi => "pi",
            Self::DeepseekHarness => "deepseek_harness",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSemanticKind {
    SessionMetadata,
    Reasoning,
    AssistantResponse,
    ToolCallRequested,
    ToolCallStarted,
    ToolCallCompleted,
    ToolCallFailed,
    ArtifactCreated,
    UsageReported,
    TurnCompleted,
    TurnFailed,
    TurnCancelled,
    MalformedOrIncomplete,
    UnclassifiedNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionLifecyclePhase {
    Requested,
    Started,
    Progress,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedCompleteness {
    Partial,
    Complete,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentAvailability {
    Available,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentUnavailableReason {
    ProviderAbsent,
    DecoderUnsupported,
    IncompleteTail,
    RelatedRecordMissing,
    Malformed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolOperationCategory {
    Read,
    Search,
    Command,
    Write,
    Edit,
    Network,
    Subagent,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallOutcome {
    Requested,
    Started,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeClassificationReason {
    UnsupportedEventType,
    MissingEventType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedContentReference {
    pub availability: ContentAvailability,
    #[serde(default)]
    pub unavailable_reason: Option<ContentUnavailableReason>,
    /// RFC 6901 pointer into this record's response-local `native_event`.
    #[serde(default)]
    pub json_pointer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PersistedFragmentPayload {
    Native {
        #[serde(default)]
        event_type: Option<String>,
        #[serde(default)]
        event_subtype: Option<String>,
        #[serde(default)]
        classification_reason: Option<NativeClassificationReason>,
    },
    SessionMetadata {
        #[serde(default)]
        native_session_id: Option<String>,
    },
    AssistantResponse {
        #[serde(default)]
        text: Option<String>,
    },
    Reasoning {
        #[serde(default)]
        text: Option<String>,
    },
    Tool {
        #[serde(default)]
        tool_name: Option<String>,
        #[serde(default)]
        tool_name_unavailable_reason: Option<ContentUnavailableReason>,
        #[serde(default)]
        call_id: Option<String>,
        #[serde(default)]
        parent_call_id: Option<String>,
        #[serde(default)]
        operation_category: Option<ToolOperationCategory>,
        #[serde(default)]
        primary_target: Option<String>,
        #[serde(default)]
        arguments: Option<PersistedContentReference>,
        #[serde(default)]
        result: Option<PersistedContentReference>,
        #[serde(default)]
        error: Option<PersistedContentReference>,
        #[serde(default)]
        outcome: Option<ToolCallOutcome>,
        #[serde(default)]
        display_detail: Option<String>,
    },
    Artifact {
        display_name: String,
        #[serde(default)]
        media_type: Option<String>,
        #[serde(default)]
        content_digest: Option<String>,
    },
    Usage {
        #[serde(default)]
        input_tokens: Option<u64>,
        #[serde(default)]
        output_tokens: Option<u64>,
        #[serde(default)]
        total_tokens: Option<u64>,
    },
    Turn {
        outcome: String,
        #[serde(default)]
        display_summary: Option<String>,
    },
    Malformed {
        reason_code: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedEventFragment {
    pub fragment_id: String,
    pub fragment_index: u32,
    pub semantic_kind: SessionSemanticKind,
    pub lifecycle_phase: SessionLifecyclePhase,
    pub completeness: PersistedCompleteness,
    pub content_availability: ContentAvailability,
    #[serde(default)]
    pub content_unavailable_reason: Option<ContentUnavailableReason>,
    pub payload: PersistedFragmentPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderingKeyKind {
    ProviderOrdinal,
    CompleteRowEndOffset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedOrderingKey {
    pub kind: OrderingKeyKind,
    pub value: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedNativeRow {
    pub provider: ProviderKind,
    /// Opaque provider/session/file-incarnation fingerprint. It never embeds
    /// NodeDaemon identity or generation.
    pub source_generation: String,
    /// Stable provider-native id when available, otherwise a generation-scoped
    /// physical-row locator. This is not a filesystem path.
    pub row_locator: String,
    pub ordering_key: PersistedOrderingKey,
    pub content_fingerprint: String,
    #[serde(default)]
    pub occurred_at: Option<String>,
    /// Exact provider-owned persisted row, response-local only.
    pub native_event: serde_json::Value,
}

impl PersistedNativeRow {
    pub fn validate(&self) -> Result<(), PersistedRecordValidationError> {
        if !opaque_locator(&self.source_generation, "source-generation:")
            || !opaque_locator(&self.row_locator, "row-locator:")
            || self.ordering_key.value == 0
            || !valid_sha256(&self.content_fingerprint)
        {
            return Err(PersistedRecordValidationError::InvalidSourceIdentity);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderNativeEventRecord {
    pub schema_version: String,
    pub record_id: String,
    pub provider: ProviderKind,
    pub adapter_version: String,
    /// Opaque scoped provider source reference; never a provider path.
    pub native_source_ref: String,
    pub source_generation: String,
    pub row_locator: String,
    pub ordering_key: PersistedOrderingKey,
    pub source_content_fingerprint: String,
    #[serde(alias = "agent_identity_id")]
    pub agent_member_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
    #[serde(default)]
    pub provider_thread_id: Option<String>,
    #[serde(default)]
    pub provider_turn_id: Option<String>,
    #[serde(default)]
    pub provider_event_id: Option<String>,
    #[serde(default)]
    pub occurred_at: Option<String>,
    pub observed_at: String,
    /// Exact response-local provider-native row. Harness never persists it.
    pub native_event: serde_json::Value,
    pub fragments: Vec<PersistedEventFragment>,
}

impl ProviderNativeEventRecord {
    pub fn stable_record_id(source_generation: &str, row_locator: &str) -> String {
        let mut digest = Sha256::new();
        for field in [source_generation, row_locator] {
            digest.update(field.as_bytes());
            digest.update([0]);
        }
        format!("native-row:sha256:{:x}", digest.finalize())
    }

    pub fn validate(&self) -> Result<(), PersistedRecordValidationError> {
        if self.schema_version != PROVIDER_NATIVE_EVENT_RECORD_V3_SCHEMA_VERSION
            || self.adapter_version != PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION
        {
            return Err(PersistedRecordValidationError::UnsupportedVersion);
        }
        if self.agent_member_id.is_empty()
            || self.agent_session_id.is_empty()
            || self.agent_session_generation == 0
            || self.observed_at.is_empty()
            || !opaque_locator(&self.native_source_ref, "provider-source:")
            || !opaque_locator(&self.source_generation, "source-generation:")
            || !opaque_locator(&self.row_locator, "row-locator:")
            || self.ordering_key.value == 0
            || !valid_sha256(&self.source_content_fingerprint)
        {
            return Err(PersistedRecordValidationError::InvalidSourceIdentity);
        }
        if self.record_id != Self::stable_record_id(&self.source_generation, &self.row_locator) {
            return Err(PersistedRecordValidationError::InvalidRecordIdentity);
        }
        if self.fragments.is_empty() {
            return Err(PersistedRecordValidationError::InvalidFragments);
        }
        for (index, fragment) in self.fragments.iter().enumerate() {
            if fragment.fragment_index as usize != index
                || fragment.fragment_id != format!("{}:fragment-{index}", self.record_id)
                || !payload_matches(fragment)
                || !content_availability_matches(fragment)
                || !content_references_resolve(fragment, &self.native_event)
            {
                return Err(PersistedRecordValidationError::InvalidFragments);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionReaderAuthority {
    pub node_id: String,
    pub node_daemon_id: String,
    pub node_daemon_generation: u64,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedPageCursor {
    pub source_generation: String,
    pub before: PersistedOrderingKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceResetReason {
    Truncated,
    Replaced,
    Rotated,
    Compacted,
    FormatChanged,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedSourceReset {
    #[serde(default)]
    pub previous_source_generation: Option<String>,
    pub source_generation: String,
    pub reason: SourceResetReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedSessionPage {
    pub reader_authority: NativeSessionReaderAuthority,
    pub source_generation: String,
    pub records: Vec<ProviderNativeEventRecord>,
    pub snapshot_watermark: Option<PersistedOrderingKey>,
    pub has_more: bool,
    #[serde(default)]
    pub next_cursor: Option<PersistedPageCursor>,
    pub incomplete_tail: bool,
    #[serde(default)]
    pub source_reset: Option<PersistedSourceReset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedTailMode {
    Incremental,
    BoundedSnapshotDiff,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistedReaderReachability {
    Local,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedSemanticCapability {
    pub semantic_kind: SessionSemanticKind,
    pub phases: Vec<SessionLifecyclePhase>,
    pub content_availability: Vec<ContentAvailability>,
}

/// Versioned claim surface for the persisted reader only. Runtime callbacks
/// and their union of capabilities are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedAdapterManifest {
    pub provider: ProviderKind,
    pub adapter_version: String,
    pub persisted_source_families: Vec<String>,
    pub format_version_fences: Vec<String>,
    pub source_generation: bool,
    pub stable_row_locator: bool,
    pub pagination: bool,
    pub tail_mode: PersistedTailMode,
    pub reader_reachability: Vec<PersistedReaderReachability>,
    pub semantic_capabilities: Vec<PersistedSemanticCapability>,
}

impl PersistedAdapterManifest {
    pub fn validate(&self) -> Result<(), PersistedRecordValidationError> {
        if self.adapter_version != PERSISTED_PROVIDER_EVENT_ADAPTER_VERSION
            || self.persisted_source_families.is_empty()
            || self.format_version_fences.is_empty()
            || !self.source_generation
            || !self.stable_row_locator
            || self.reader_reachability.is_empty()
            || self.semantic_capabilities.is_empty()
            || self
                .persisted_source_families
                .iter()
                .chain(&self.format_version_fences)
                .any(|value| value.is_empty() || value.len() > 256)
        {
            return Err(PersistedRecordValidationError::InvalidManifest);
        }
        let unique_source_families = self
            .persisted_source_families
            .iter()
            .collect::<BTreeSet<_>>();
        let unique_format_fences = self.format_version_fences.iter().collect::<BTreeSet<_>>();
        let unique_reachability = self.reader_reachability.iter().collect::<BTreeSet<_>>();
        if unique_source_families.len() != self.persisted_source_families.len()
            || unique_format_fences.len() != self.format_version_fences.len()
            || unique_reachability.len() != self.reader_reachability.len()
        {
            return Err(PersistedRecordValidationError::InvalidManifest);
        }
        let mut kinds = BTreeSet::new();
        if self.semantic_capabilities.iter().any(|capability| {
            !kinds.insert(capability.semantic_kind)
                || capability.phases.is_empty()
                || capability.content_availability.is_empty()
                || capability.phases.iter().collect::<BTreeSet<_>>().len()
                    != capability.phases.len()
                || capability
                    .content_availability
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != capability.content_availability.len()
                || !capability_content_availability_matches(capability)
        }) {
            return Err(PersistedRecordValidationError::InvalidManifest);
        }
        Ok(())
    }
}

impl PersistedSessionPage {
    pub fn validate(&self) -> Result<(), PersistedRecordValidationError> {
        if self.reader_authority.node_id.is_empty()
            || self.reader_authority.node_daemon_id.is_empty()
            || self.reader_authority.node_daemon_generation == 0
            || self.reader_authority.agent_session_id.is_empty()
            || self.reader_authority.agent_session_generation == 0
            || !opaque_locator(&self.source_generation, "source-generation:")
        {
            return Err(PersistedRecordValidationError::InvalidReaderAuthority);
        }
        if self.records.iter().any(|record| {
            record.validate().is_err()
                || record.source_generation != self.source_generation
                || record.agent_session_id != self.reader_authority.agent_session_id
                || record.agent_session_generation != self.reader_authority.agent_session_generation
        }) {
            return Err(PersistedRecordValidationError::PageIdentityConflict);
        }
        let ordering_kind = self
            .records
            .first()
            .map(|record| record.ordering_key.kind)
            .or_else(|| self.snapshot_watermark.map(|watermark| watermark.kind));
        if self.records.windows(2).any(|rows| {
            rows[0].ordering_key.kind != rows[1].ordering_key.kind
                || rows[0].ordering_key.value >= rows[1].ordering_key.value
        }) || self.snapshot_watermark.is_some_and(|watermark| {
            ordering_kind.is_some_and(|kind| watermark.kind != kind)
                || self
                    .records
                    .last()
                    .is_some_and(|record| watermark.value < record.ordering_key.value)
        }) {
            return Err(PersistedRecordValidationError::InvalidOrdering);
        }
        if self.has_more != self.next_cursor.is_some()
            || self.next_cursor.as_ref().is_some_and(|cursor| {
                cursor.source_generation != self.source_generation
                    || cursor.before.value == 0
                    || ordering_kind.is_some_and(|kind| cursor.before.kind != kind)
            })
            || self.source_reset.as_ref().is_some_and(|reset| {
                reset.source_generation != self.source_generation
                    || !opaque_locator(&reset.source_generation, "source-generation:")
                    || reset
                        .previous_source_generation
                        .as_ref()
                        .is_some_and(|previous| !opaque_locator(previous, "source-generation:"))
                    || reset
                        .previous_source_generation
                        .as_ref()
                        .is_some_and(|previous| previous == &reset.source_generation)
            })
        {
            return Err(PersistedRecordValidationError::PageIdentityConflict);
        }
        Ok(())
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn opaque_locator(value: &str, prefix: &str) -> bool {
    value.starts_with(prefix)
        && value.len() > prefix.len()
        && value.len() <= 512
        && !value.contains('/')
        && !value.contains('\\')
        && !value.contains("..")
}

fn payload_matches(fragment: &PersistedEventFragment) -> bool {
    match (fragment.semantic_kind, &fragment.payload) {
        (
            SessionSemanticKind::SessionMetadata,
            PersistedFragmentPayload::SessionMetadata { .. },
        )
        | (SessionSemanticKind::Reasoning, PersistedFragmentPayload::Reasoning { .. })
        | (
            SessionSemanticKind::AssistantResponse,
            PersistedFragmentPayload::AssistantResponse { .. },
        ) => true,
        (
            SessionSemanticKind::ToolCallRequested
            | SessionSemanticKind::ToolCallStarted
            | SessionSemanticKind::ToolCallCompleted
            | SessionSemanticKind::ToolCallFailed,
            PersistedFragmentPayload::Tool {
                tool_name,
                tool_name_unavailable_reason,
                call_id,
                parent_call_id,
                primary_target,
                arguments,
                result,
                error,
                outcome,
                ..
            },
        ) => {
            let expected_outcome = match fragment.semantic_kind {
                SessionSemanticKind::ToolCallRequested => ToolCallOutcome::Requested,
                SessionSemanticKind::ToolCallStarted => ToolCallOutcome::Started,
                SessionSemanticKind::ToolCallCompleted => ToolCallOutcome::Completed,
                SessionSemanticKind::ToolCallFailed => ToolCallOutcome::Failed,
                _ => unreachable!("tool semantic kind"),
            };
            call_id
                .as_ref()
                .is_none_or(|value| !value.is_empty() && value.len() <= 512)
                && parent_call_id
                    .as_ref()
                    .is_none_or(|value| !value.is_empty() && value.len() <= 512)
                && primary_target
                    .as_ref()
                    .is_none_or(|value| !value.is_empty() && value.chars().count() <= 512)
                && outcome.is_none_or(|outcome| outcome == expected_outcome)
                && match (tool_name, tool_name_unavailable_reason) {
                    (Some(name), None) => !name.is_empty() && name.len() <= 256,
                    (None, Some(ContentUnavailableReason::RelatedRecordMissing)) => matches!(
                        fragment.semantic_kind,
                        SessionSemanticKind::ToolCallCompleted
                            | SessionSemanticKind::ToolCallFailed
                    ),
                    _ => false,
                }
                && arguments.as_ref().is_none_or(content_reference_matches)
                && result.as_ref().is_none_or(content_reference_matches)
                && error.as_ref().is_none_or(content_reference_matches)
        }
        (
            SessionSemanticKind::ArtifactCreated,
            PersistedFragmentPayload::Artifact { display_name, .. },
        ) => !display_name.is_empty(),
        (SessionSemanticKind::UsageReported, PersistedFragmentPayload::Usage { .. }) => true,
        (
            SessionSemanticKind::TurnCompleted
            | SessionSemanticKind::TurnFailed
            | SessionSemanticKind::TurnCancelled,
            PersistedFragmentPayload::Turn { outcome, .. },
        ) => !outcome.is_empty(),
        (
            SessionSemanticKind::MalformedOrIncomplete,
            PersistedFragmentPayload::Malformed { reason_code },
        ) => !reason_code.is_empty(),
        (
            SessionSemanticKind::UnclassifiedNative,
            PersistedFragmentPayload::Native {
                event_type,
                event_subtype,
                ..
            },
        ) => {
            event_type
                .as_ref()
                .is_none_or(|value| !value.is_empty() && value.len() <= 256)
                && event_subtype
                    .as_ref()
                    .is_none_or(|value| !value.is_empty() && value.len() <= 256)
        }
        _ => false,
    }
}

fn content_availability_matches(fragment: &PersistedEventFragment) -> bool {
    let text = match &fragment.payload {
        PersistedFragmentPayload::AssistantResponse { text }
        | PersistedFragmentPayload::Reasoning { text } => Some(text),
        _ => None,
    };
    match (
        fragment.content_availability,
        fragment.content_unavailable_reason,
        text,
    ) {
        (ContentAvailability::Available, None, Some(Some(text))) => !text.is_empty(),
        (ContentAvailability::Unavailable, _, Some(None)) => true,
        (ContentAvailability::Available, None, None) => true,
        _ => false,
    }
}

fn content_reference_matches(reference: &PersistedContentReference) -> bool {
    match (
        reference.availability,
        reference.unavailable_reason,
        reference.json_pointer.as_deref(),
    ) {
        (ContentAvailability::Available, None, Some(pointer)) => {
            pointer.starts_with('/') && pointer.len() <= 512
        }
        (ContentAvailability::Unavailable, Some(_), None) => true,
        _ => false,
    }
}

fn content_references_resolve(
    fragment: &PersistedEventFragment,
    native_event: &serde_json::Value,
) -> bool {
    let PersistedFragmentPayload::Tool {
        arguments,
        result,
        error,
        ..
    } = &fragment.payload
    else {
        return true;
    };
    arguments
        .iter()
        .chain(result)
        .chain(error)
        .all(|reference| match reference.json_pointer.as_deref() {
            Some(pointer) => native_event.pointer(pointer).is_some(),
            None => true,
        })
}

fn capability_content_availability_matches(capability: &PersistedSemanticCapability) -> bool {
    match capability.semantic_kind {
        SessionSemanticKind::Reasoning | SessionSemanticKind::AssistantResponse => true,
        _ => capability.content_availability == [ContentAvailability::Available],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PersistedRecordValidationError {
    #[error("persisted provider event schema or adapter version is unsupported")]
    UnsupportedVersion,
    #[error("persisted provider source identity is invalid")]
    InvalidSourceIdentity,
    #[error(
        "persisted provider record identity is not derived from source generation and row locator"
    )]
    InvalidRecordIdentity,
    #[error("persisted provider event fragments are invalid")]
    InvalidFragments,
    #[error("native Session reader authority is invalid")]
    InvalidReaderAuthority,
    #[error("persisted Session page mixes source or Session identities")]
    PageIdentityConflict,
    #[error("persisted Session rows are not strictly ordered")]
    InvalidOrdering,
    #[error("persisted provider manifest is incomplete or internally inconsistent")]
    InvalidManifest,
}

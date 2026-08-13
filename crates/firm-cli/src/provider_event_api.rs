//! Provider-native historical projection and volatile live activity.
//!
//! Historical events are adapted on demand from provider-owned Session files.
//! Live activity exists only in this process and is delivered as an ephemeral
//! snapshot/SSE overlay. Neither path writes Harness ledgers or a mirror store.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use harness_core::NativeSessionRef;
use harness_provider_events::{
    DecodeContext, ProjectionAuthority, ProjectionViewer, ProviderKind,
    ProviderProjectionService, TranscriptReadBoundary,
};
use serde_json::{json, Value};

use crate::{CliError, CliResult};

const MAX_HISTORICAL_EVENTS: usize = 10_000;
const MAX_LIVE_ITEMS: usize = 24;
const LIVE_TTL_MS: u64 = 10_000;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct LiveProviderScope {
    pub project_id: String,
    pub team_run_id: String,
    pub member_run_id: String,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
}

#[derive(Clone, Debug)]
struct LiveProviderItem {
    locator: u64,
    kind: &'static str,
    provider: String,
    display_summary: String,
    emitted_unix_ms: u64,
    expires_unix_ms: u64,
}

#[derive(Default)]
struct LiveRegistry {
    next_locator: u64,
    items: BTreeMap<LiveProviderScope, VecDeque<LiveProviderItem>>,
}

static LIVE_REGISTRY: OnceLock<Mutex<LiveRegistry>> = OnceLock::new();

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn provider_kind(provider: &str) -> Option<ProviderKind> {
    match provider {
        "codex" => Some(ProviderKind::Codex),
        "claude" | "claude-code" | "claude_code" => Some(ProviderKind::Claude),
        "kimi" | "kimi-code" | "kimi_code" => Some(ProviderKind::Kimi),
        "pi" => Some(ProviderKind::Pi),
        _ => None,
    }
}

pub(crate) struct HistoricalProjectionRequest<'a> {
    pub project_id: &'a str,
    pub team_id: &'a str,
    pub agent_identity_id: &'a str,
    pub agent_session_id: &'a str,
    pub agent_session_generation: u64,
    pub node_daemon_id: &'a str,
    pub node_daemon_generation: u64,
    pub viewer_identity_id: &'a str,
    pub native_session: &'a NativeSessionRef,
}

pub(crate) fn read_historical_projection(
    request: HistoricalProjectionRequest<'_>,
) -> CliResult<Value> {
    let Some(provider) = provider_kind(&request.native_session.provider) else {
        return Err(CliError::Usage("provider adapter is unavailable".into()));
    };
    let Some((allowed_root, transcript_path)) =
        crate::native_session::locate_read_boundary(request.native_session)?
    else {
        return Err(CliError::Usage(
            "provider-native Session source is unavailable".into(),
        ));
    };
    let context = DecodeContext {
        provider,
        native_source_ref: format!(
            "provider-session:{}:{}:{}",
            provider.as_str(),
            request.agent_session_id,
            request.agent_session_generation
        ),
        agent_identity_id: request.agent_identity_id.to_string(),
        agent_session_id: request.agent_session_id.to_string(),
        agent_session_generation: request.agent_session_generation,
        node_daemon_id: request.node_daemon_id.to_string(),
        node_daemon_generation: request.node_daemon_generation,
        provider_thread_id: Some(request.native_session.native_session_id.clone()),
        runtime_command_id: None,
        observed_at: format!("unix-ms:{}", now_unix_ms()),
    };
    let authority = ProjectionAuthority {
        project_id: request.project_id.to_string(),
        team_id: request.team_id.to_string(),
        agent_identity_id: request.agent_identity_id.to_string(),
        agent_session_id: request.agent_session_id.to_string(),
        agent_session_generation: request.agent_session_generation,
    };
    let viewer = ProjectionViewer {
        project_id: request.project_id.to_string(),
        team_id: request.team_id.to_string(),
        agent_identity_id: request.viewer_identity_id.to_string(),
        is_team_host: false,
    };
    let mut service = ProviderProjectionService::open(context);
    service
        .refresh(
            &TranscriptReadBoundary {
                allowed_root,
                transcript_path,
            },
            MAX_HISTORICAL_EVENTS,
        )
        .map_err(|error| CliError::Usage(error.to_string()))?;
    serde_json::to_value(
        service
            .private_session(&authority, &viewer, 300)
            .map_err(|error| CliError::Usage(error.to_string()))?,
    )
    .map_err(Into::into)
}

pub(crate) fn record_live(
    scope: LiveProviderScope,
    provider: &str,
    kind: &'static str,
    display_summary: String,
) -> Value {
    let now = now_unix_ms();
    let expires = now.saturating_add(LIVE_TTL_MS);
    let registry = LIVE_REGISTRY.get_or_init(|| Mutex::new(LiveRegistry::default()));
    let mut registry = registry.lock().unwrap_or_else(|error| error.into_inner());
    purge_expired(&mut registry, now);
    registry.next_locator = registry.next_locator.saturating_add(1).max(1);
    let locator = registry.next_locator;
    let items = registry.items.entry(scope.clone()).or_default();
    items.push_back(LiveProviderItem {
        locator,
        kind,
        provider: provider.to_string(),
        display_summary,
        emitted_unix_ms: now,
        expires_unix_ms: expires,
    });
    while items.len() > MAX_LIVE_ITEMS {
        items.pop_front();
    }
    snapshot_locked(&registry, &scope, now).unwrap_or(Value::Null)
}

pub(crate) fn live_snapshot(scope: &LiveProviderScope) -> Option<Value> {
    let now = now_unix_ms();
    let registry = LIVE_REGISTRY.get_or_init(|| Mutex::new(LiveRegistry::default()));
    let mut registry = registry.lock().unwrap_or_else(|error| error.into_inner());
    purge_expired(&mut registry, now);
    snapshot_locked(&registry, scope, now)
}

pub(crate) fn clear_live(scope: &LiveProviderScope) {
    if let Some(registry) = LIVE_REGISTRY.get() {
        registry
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .items
            .remove(scope);
    }
}

fn purge_expired(registry: &mut LiveRegistry, now: u64) {
    registry.items.retain(|_, items| {
        items.retain(|item| item.expires_unix_ms > now);
        !items.is_empty()
    });
}

fn snapshot_locked(registry: &LiveRegistry, scope: &LiveProviderScope, now: u64) -> Option<Value> {
    let items = registry.items.get(scope)?;
    let expires_unix_ms = items.iter().map(|item| item.expires_unix_ms).max()?;
    Some(json!({
        "schema_version":"agentfirm.live_provider_activity.v1",
        "durability":"volatile_process_memory",
        "replayable":false,
        "project_id":scope.project_id,
        "team_run_id":scope.team_run_id,
        "member_run_id":scope.member_run_id,
        "agent_session_id":scope.agent_session_id,
        "agent_session_generation":scope.agent_session_generation,
        "runtime_snapshot_locator":format!("runtime-snapshot-{}", registry.next_locator.max(now)),
        "expires_unix_ms":expires_unix_ms,
        "items":items.iter().map(|item|json!({
            "runtime_event_locator":format!("runtime-event-{}",item.locator),
            "kind":item.kind,
            "provider":item.provider,
            "display_summary":item.display_summary,
            "emitted_unix_ms":item.emitted_unix_ms,
            "expires_unix_ms":item.expires_unix_ms,
        })).collect::<Vec<_>>()
    }))
}

#[cfg(test)]
pub(crate) fn reset_live_for_test() {
    if let Some(registry) = LIVE_REGISTRY.get() {
        *registry.lock().unwrap_or_else(|error| error.into_inner()) = LiveRegistry::default();
    }
}

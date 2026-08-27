//! Provider-native historical projection and volatile live activity.
//!
//! Historical events are adapted on demand from the provider-owned Session.
//! Live activity exists only in this process and is delivered as a bounded
//! snapshot/SSE overlay. Neither path writes Harness ledgers or a mirror store.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use harness_core::NativeSessionRef;
use harness_provider_events::{
    DecodeContext, ProjectionAuthority, ProjectionReadScope, ProviderKind,
    ProviderProjectionService, TranscriptReadBoundary,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::{CliError, CliResult};

pub(crate) const DEFAULT_SESSION_PAGE_SIZE: usize = 80;
pub(crate) const MAX_SESSION_PAGE_SIZE: usize = 200;
const MAX_LIVE_ITEMS: usize = 24;
pub(crate) const LIVE_TTL_MS: u64 = 10_000;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct LiveProviderScope {
    pub execution_space_id: String,
    pub project_id: String,
    pub team_run_id: String,
    pub member_run_id: String,
    pub member_run_generation: u64,
    pub agent_session_id: String,
    pub agent_session_generation: u64,
}

pub(crate) use harness_runtime_contract::LiveProviderActivityKind;

#[derive(Clone, Debug, Serialize)]
struct LiveProviderItem {
    runtime_event_locator: String,
    kind: LiveProviderActivityKind,
    provider: String,
    native_event: Value,
    emitted_unix_ms: u64,
    expires_unix_ms: u64,
}

#[derive(Default)]
struct LiveRegistry {
    next_locators: BTreeMap<LiveProviderScope, u64>,
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
        "deepseek" | "deepseek_harness" => Some(ProviderKind::DeepseekHarness),
        _ => None,
    }
}

pub(crate) struct HistoricalProjectionRequest<'a> {
    pub execution_space_id: &'a str,
    pub project_id: &'a str,
    pub team_id: &'a str,
    pub agent_member_id: &'a str,
    pub agent_session_id: &'a str,
    pub agent_session_generation: u64,
    pub node_daemon_id: &'a str,
    pub node_daemon_generation: u64,
    pub before_position: Option<u64>,
    pub page_limit: usize,
    pub native_session: &'a NativeSessionRef,
}

/// Decode one Team-scoped page directly from the provider-owned Session. The
/// service is disposable and no decoded row, fold state, fingerprint, or
/// cursor is persisted by Harness.
pub(crate) fn read_historical_projection(
    request: HistoricalProjectionRequest<'_>,
) -> CliResult<Value> {
    let Some(provider) = provider_kind(&request.native_session.provider) else {
        return Err(CliError::Usage("provider adapter is unavailable".into()));
    };
    let context = DecodeContext {
        provider,
        native_source_ref: format!(
            "provider-source:{}:{}:{}:{}:{}",
            provider.as_str(),
            request.execution_space_id,
            request.project_id,
            request.agent_session_id,
            request.agent_session_generation
        ),
        agent_member_id: request.agent_member_id.to_string(),
        agent_session_id: request.agent_session_id.to_string(),
        agent_session_generation: request.agent_session_generation,
        node_daemon_id: request.node_daemon_id.to_string(),
        node_daemon_generation: request.node_daemon_generation,
        provider_thread_id: Some(request.native_session.native_session_id.clone()),
        runtime_command_id: None,
        observed_at: format!("unix-ms:{}", now_unix_ms()),
    };
    let authority = ProjectionAuthority {
        execution_space_id: request.execution_space_id.to_string(),
        project_binding_id: request.project_id.to_string(),
        team_id: request.team_id.to_string(),
        agent_session_id: request.agent_session_id.to_string(),
        agent_session_generation: request.agent_session_generation,
    };
    let scope = ProjectionReadScope {
        execution_space_id: request.execution_space_id.to_string(),
        project_binding_id: request.project_id.to_string(),
        team_id: request.team_id.to_string(),
    };
    let mut service = ProviderProjectionService::open(context);
    let page_limit = request.page_limit.clamp(1, MAX_SESSION_PAGE_SIZE);
    if provider == ProviderKind::DeepseekHarness {
        let content = crate::native_session::read_deepseek_session_jsonl(request.native_session)?;
        service
            .refresh_jsonl_page(&content, request.before_position, page_limit)
            .map_err(|error| CliError::Usage(error.to_string()))?;
    } else {
        let Some((allowed_root, transcript_path)) = crate::native_session::locate_read_boundary(
            request.native_session,
            request.execution_space_id,
        )?
        else {
            return Err(CliError::Usage(
                "provider-native Session source is unavailable".into(),
            ));
        };
        service
            .refresh_page(
                &TranscriptReadBoundary {
                    allowed_root,
                    transcript_path,
                },
                request.before_position,
                page_limit,
            )
            .map_err(|error| CliError::Usage(error.to_string()))?;
    }
    let mut projection = serde_json::to_value(
        service
            .team_session(&authority, &scope, page_limit)
            .map_err(|error| CliError::Usage(error.to_string()))?,
    )?;
    projection["page"] = service.page_metadata(page_limit);
    Ok(projection)
}

pub(crate) fn record_live(
    scope: LiveProviderScope,
    provider: &str,
    kind: LiveProviderActivityKind,
    native_event: Value,
) -> Value {
    record_live_at(scope, provider, kind, native_event, now_unix_ms())
}

fn record_live_at(
    scope: LiveProviderScope,
    provider: &str,
    kind: LiveProviderActivityKind,
    native_event: Value,
    now: u64,
) -> Value {
    let expires = now.saturating_add(LIVE_TTL_MS);
    let registry = LIVE_REGISTRY.get_or_init(|| Mutex::new(LiveRegistry::default()));
    let mut registry = registry.lock().unwrap_or_else(|error| error.into_inner());
    purge_expired(&mut registry, now);
    let locator = registry.next_locators.entry(scope.clone()).or_default();
    *locator = locator.saturating_add(1).max(1);
    let locator = *locator;
    let items = registry.items.entry(scope.clone()).or_default();
    items.push_back(LiveProviderItem {
        runtime_event_locator: format!("runtime-event-{locator}"),
        kind,
        provider: provider.to_string(),
        native_event,
        emitted_unix_ms: now,
        expires_unix_ms: expires,
    });
    while items.len() > MAX_LIVE_ITEMS {
        items.pop_front();
    }
    snapshot_locked(&registry, &scope).expect("recorded live scope must have a snapshot")
}

pub(crate) fn live_snapshot(scope: &LiveProviderScope) -> Option<Value> {
    live_snapshot_at(scope, now_unix_ms())
}

fn live_snapshot_at(scope: &LiveProviderScope, now: u64) -> Option<Value> {
    let registry = LIVE_REGISTRY.get_or_init(|| Mutex::new(LiveRegistry::default()));
    let mut registry = registry.lock().unwrap_or_else(|error| error.into_inner());
    purge_expired(&mut registry, now);
    snapshot_locked(&registry, scope)
}

/// Terminal provider state invalidates the whole turn overlay immediately.
/// The returned event is suitable for the live SSE stream and intentionally
/// contains no activity payload that a reconnecting client could replay.
pub(crate) fn clear_live_terminal(scope: &LiveProviderScope) -> Value {
    clear_live(scope);
    live_event("terminal", scope, None)
}

pub(crate) fn updated_live_event(scope: &LiveProviderScope, activity: Value) -> Value {
    live_event("updated", scope, Some(activity))
}

fn live_event(reason: &str, scope: &LiveProviderScope, activity: Option<Value>) -> Value {
    json!({
        "schema_version":"agentfirm.live_provider_activity_event.v1",
        "reason":reason,
        "scope":scope,
        "activity":activity,
    })
}

pub(crate) fn clear_live(scope: &LiveProviderScope) {
    if let Some(registry) = LIVE_REGISTRY.get() {
        let mut registry = registry.lock().unwrap_or_else(|error| error.into_inner());
        registry.items.remove(scope);
        registry.next_locators.remove(scope);
    }
}

/// Drop every volatile overlay owned by one AgentIdentity in one Execution
/// Space. SSE connection boundaries call this both before subscribing and
/// after disconnect so reconnecting clients cannot recover an earlier live
/// snapshot through the RoleView GET surface.
pub(crate) fn clear_live_for_agent(
    store: &harness_store::HarnessStore,
    execution_space_id: &str,
    project_id: &str,
    agent_member_id: &str,
) -> CliResult<()> {
    let session_ids = store
        .fabric_agent_sessions(execution_space_id)?
        .into_iter()
        .filter(|session| session.execution_space_id == execution_space_id)
        .filter(|session| session.agent_member_id == agent_member_id)
        .map(|session| session.id)
        .collect::<BTreeSet<_>>();
    clear_live_for_session_ids(execution_space_id, project_id, &session_ids);
    Ok(())
}

fn clear_live_for_session_ids(
    execution_space_id: &str,
    project_id: &str,
    session_ids: &BTreeSet<String>,
) {
    if let Some(registry) = LIVE_REGISTRY.get() {
        let mut registry = registry.lock().unwrap_or_else(|error| error.into_inner());
        registry.items.retain(|scope, _| {
            scope.execution_space_id != execution_space_id
                || scope.project_id != project_id
                || !session_ids.contains(&scope.agent_session_id)
        });
        registry.next_locators.retain(|scope, _| {
            scope.execution_space_id != execution_space_id
                || scope.project_id != project_id
                || !session_ids.contains(&scope.agent_session_id)
        });
    }
}

/// Resolve the exact canonical AgentSession scope for a provider update. The
/// provider transport never supplies a session id: accepting one would allow a
/// caller to fabricate or cross-bind private runtime activity.
pub(crate) fn exact_live_scope(
    store: &harness_store::HarnessStore,
    execution_space_id: &str,
    project_id: &str,
    team_run_id: &str,
    member_run: &harness_core::ProviderRuntimeProjection,
) -> Result<LiveProviderScope, &'static str> {
    if member_run.team_run_id != team_run_id {
        return Err("member run does not belong to the selected TeamRun");
    }
    // The first live frames can precede terminal settlement of the provider's
    // native Session id onto MemberRun. They are still attributable without a
    // client-supplied Session selector: the exact Active AgentSession is bound
    // to this TeamSupervisor, AgentMember, provider and NodeDaemon generation.
    // Once MemberRun has a native binding, require it as an additional fence.
    // A live Supervisor exists only after start preflight. Use that boundary's
    // persisted ProviderProfile observation when TeamRun creation left the
    // durable resume locator versionless.
    let expected_native = crate::expected_agentfirm_native_session_ref(member_run, true);
    let run = store
        .team_runs()
        .map_err(|_| "TeamRun registry is unavailable")?
        .into_iter()
        .rev()
        .find(|run| run.id == team_run_id)
        .ok_or("TeamRun does not exist in the selected Execution Space")?;
    if run.project_binding_id != project_id {
        return Err("TeamRun belongs to another Project Binding");
    }
    let sessions = store
        .fabric_agent_sessions(execution_space_id)
        .map_err(|_| "AgentSession registry is unavailable")?;
    let supervisor = store
        .latest_team_supervisor_lease(team_run_id)
        .map_err(|_| "TeamSupervisor registry is unavailable")?
        .filter(|lease| {
            lease.status == harness_core::TeamSupervisorLeaseStatus::Active
                && lease.expires_unix_ms > now_unix_ms()
                && lease.execution_space_id == execution_space_id
                && lease.project_binding_id == project_id
        })
        .ok_or("TeamRun has no exact active TeamSupervisor generation")?;
    let current = sessions
        .into_iter()
        .filter(|session| session.agent_member_id == member_run.agent_member_id)
        .filter(|session| session.execution_space_id == execution_space_id)
        .filter(|session| session.provider_kind == member_run.provider)
        .filter(|session| {
            expected_native.as_ref().is_none_or(|expected| {
                crate::agentfirm_native_session_identity_matches(
                    session.native_session_ref.as_ref(),
                    Some(expected),
                )
            })
        })
        .filter(|session| {
            session.lifecycle == harness_core::agentfirm_api::AgentSessionStatus::Active
        })
        .filter(|session| {
            matches!(
                &session.control_state.driver_ref,
                harness_core::agentfirm_api::RuntimeDriverRef::TeamSupervisor {
                    team_run_id: driven_run,
                    team_supervisor_id,
                    team_supervisor_generation,
                } if driven_run == team_run_id
                    && team_supervisor_id == &supervisor.supervisor_id
                    && *team_supervisor_generation == supervisor.generation
            )
        })
        .filter(|session| {
            store
                .latest_node_daemon_lease(&session.node_id)
                .ok()
                .flatten()
                .is_some_and(|lease| {
                    lease.status == harness_core::NodeDaemonLeaseStatus::Active
                        && lease.expires_unix_ms > now_unix_ms()
                        && lease.daemon_id == session.node_daemon_id
                        && lease.generation == session.node_daemon_generation
                })
        })
        .collect::<Vec<_>>();
    match current.as_slice() {
        [session] => Ok(LiveProviderScope {
            execution_space_id: execution_space_id.to_string(),
            project_id: project_id.to_string(),
            team_run_id: team_run_id.to_string(),
            member_run_id: member_run.id.clone(),
            member_run_generation: member_run.runtime_generation,
            agent_session_id: session.id.clone(),
            agent_session_generation: session.runtime_generation,
        }),
        [] => Err("no exact current AgentSession binds this Member identity and provider"),
        _ => {
            Err("multiple current AgentSessions ambiguously bind this Member identity and provider")
        }
    }
}

fn purge_expired(registry: &mut LiveRegistry, now: u64) {
    registry.items.retain(|_, items| {
        items.retain(|item| item.expires_unix_ms > now);
        !items.is_empty()
    });
    registry
        .next_locators
        .retain(|scope, _| registry.items.contains_key(scope));
}

fn snapshot_locked(registry: &LiveRegistry, scope: &LiveProviderScope) -> Option<Value> {
    let items = registry.items.get(scope)?;
    let snapshot_locator = registry.next_locators.get(scope)?;
    let expires_unix_ms = items.iter().map(|item| item.expires_unix_ms).max()?;
    Some(json!({
        "schema_version":"agentfirm.live_provider_activity.v1",
        "durability":"volatile_process_memory",
        "replayable":false,
        "execution_space_id":scope.execution_space_id,
        "project_id":scope.project_id,
        "team_run_id":scope.team_run_id,
        "member_run_id":scope.member_run_id,
        "member_run_generation":scope.member_run_generation,
        "agent_session_id":scope.agent_session_id,
        "agent_session_generation":scope.agent_session_generation,
        "runtime_snapshot_locator":format!("runtime-snapshot-{snapshot_locator}"),
        "expires_unix_ms":expires_unix_ms,
        "items":items,
    }))
}

#[cfg(test)]
pub(crate) fn reset_live_for_test() {
    if let Some(registry) = LIVE_REGISTRY.get() {
        *registry.lock().unwrap_or_else(|error| error.into_inner()) = LiveRegistry::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn test_guard() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn scope(execution_space_id: &str, project_id: &str, generation: u64) -> LiveProviderScope {
        LiveProviderScope {
            execution_space_id: execution_space_id.into(),
            project_id: project_id.into(),
            team_run_id: "team-run-1".into(),
            member_run_id: "member-run-1".into(),
            member_run_generation: generation,
            agent_session_id: format!("agent-session-{generation}"),
            agent_session_generation: generation,
        }
    }

    fn reopened_scope(member_run_generation: u64) -> LiveProviderScope {
        LiveProviderScope {
            execution_space_id: "space-a".into(),
            project_id: "project-a".into(),
            team_run_id: "team-run-1".into(),
            member_run_id: "member-run-1".into(),
            member_run_generation,
            agent_session_id: "agent-session-stable".into(),
            agent_session_generation: 1,
        }
    }

    #[test]
    fn live_activity_is_exact_scope_ttl_bounded_and_not_cross_project() {
        let _guard = test_guard();
        reset_live_for_test();
        let project_a = scope("space-a", "project-a", 1);
        let project_b = scope("space-a", "project-b", 1);
        let activity = record_live_at(
            project_a.clone(),
            "kimi",
            LiveProviderActivityKind::Thinking,
            serde_json::json!({"type": "thinking"}),
            100,
        );
        assert_eq!(activity["project_id"], "project-a");
        assert_eq!(activity["execution_space_id"], "space-a");
        assert_eq!(activity["agent_session_id"], "agent-session-1");
        assert!(live_snapshot_at(&project_b, 101).is_none());
        assert!(live_snapshot_at(&project_a, 100 + LIVE_TTL_MS - 1).is_some());
        assert!(live_snapshot_at(&project_a, 100 + LIVE_TTL_MS).is_none());
    }

    #[test]
    fn terminal_clear_does_not_cross_session_generation() {
        let _guard = test_guard();
        reset_live_for_test();
        let generation_one = scope("space-a", "project-a", 1);
        let generation_two = scope("space-a", "project-a", 2);
        record_live_at(
            generation_one.clone(),
            "codex",
            LiveProviderActivityKind::ToolStarted,
            serde_json::json!({"type": "tool_started"}),
            100,
        );
        record_live_at(
            generation_two.clone(),
            "codex",
            LiveProviderActivityKind::ResponseStreaming,
            serde_json::json!({"type": "response_streaming"}),
            100,
        );
        let event = clear_live_terminal(&generation_one);
        assert_eq!(event["reason"], "terminal");
        assert!(event["activity"].is_null());
        assert!(live_snapshot_at(&generation_one, 101).is_none());
        assert!(live_snapshot_at(&generation_two, 101).is_some());
    }

    #[test]
    fn stale_adapter_terminal_does_not_clear_reopened_generation_activity() {
        let _guard = test_guard();
        reset_live_for_test();
        let old_adapter = reopened_scope(1);
        let reopened_adapter = reopened_scope(2);
        record_live_at(
            reopened_adapter.clone(),
            "codex",
            LiveProviderActivityKind::ResponseStreaming,
            serde_json::json!({"type": "response_streaming"}),
            100,
        );
        clear_live_terminal(&old_adapter);
        assert!(live_snapshot_at(&reopened_adapter, 101).is_some());
    }

    #[test]
    fn connection_clear_is_owner_session_scoped_and_project_isolated() {
        let _guard = test_guard();
        reset_live_for_test();
        let owner_one = scope("space-a", "project-a", 1);
        let owner_two = scope("space-a", "project-a", 2);
        let other_project = scope("space-a", "project-b", 1);
        for candidate in [&owner_one, &owner_two, &other_project] {
            record_live_at(
                candidate.clone(),
                "claude",
                LiveProviderActivityKind::Thinking,
                serde_json::json!({"type": "thinking"}),
                100,
            );
        }
        clear_live_for_session_ids(
            "space-a",
            "project-a",
            &BTreeSet::from([owner_one.agent_session_id.clone()]),
        );
        assert!(live_snapshot_at(&owner_one, 101).is_none());
        assert!(live_snapshot_at(&owner_two, 101).is_some());
        assert!(live_snapshot_at(&other_project, 101).is_some());
    }

    #[test]
    fn volatile_locators_are_scope_local_and_reset_with_the_overlay() {
        let _guard = test_guard();
        reset_live_for_test();
        let owner_one = scope("space-a", "project-a", 1);
        let mut owner_two = owner_one.clone();
        owner_two.member_run_id = "member-run-2".into();
        owner_two.agent_session_id = "agent-session-2".into();

        let first = record_live_at(
            owner_one.clone(),
            "codex",
            LiveProviderActivityKind::Thinking,
            serde_json::json!({"type": "thinking"}),
            100,
        );
        let sibling = record_live_at(
            owner_two,
            "kimi",
            LiveProviderActivityKind::Thinking,
            serde_json::json!({"type": "thinking"}),
            101,
        );
        assert_eq!(first["runtime_snapshot_locator"], "runtime-snapshot-1");
        assert_eq!(sibling["runtime_snapshot_locator"], "runtime-snapshot-1");
        assert_eq!(
            first["items"][0]["runtime_event_locator"],
            "runtime-event-1"
        );
        assert_eq!(
            sibling["items"][0]["runtime_event_locator"],
            "runtime-event-1"
        );

        clear_live(&owner_one);
        let after_disconnect = record_live_at(
            owner_one,
            "codex",
            LiveProviderActivityKind::ToolStarted,
            serde_json::json!({"type": "tool_started"}),
            102,
        );
        assert_eq!(
            after_disconnect["runtime_snapshot_locator"], "runtime-snapshot-1",
            "disconnect/terminal cleanup must discard locator history"
        );
    }
}

#![recursion_limit = "256"]

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::panic::{catch_unwind, resume_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver as ControlReceiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use harness_core::{
    build_launch_spec, content_hash_hex16, provider_interaction_response_id, AgentTeam,
    AgentTeamRun, AgentTeamStatus, ControlTopology, DelegationRun, Evidence, ExecutionNode,
    ExecutionNodeStatus, ExecutionSpace, GitHubLink, GitHubLinkKind, HostAttention,
    HostAttentionStatus, HostBindingLease, HostBindingLeaseOwnerKind, HostControlMode,
    HostDispatchConfig, LaunchMcp, LaunchSpec, LegacyWave, MemberAction, MemberActionStatus,
    MemberCoordinationStatus, MemberExecutionDriver, MemberRunStatus, MemberWorkspaceSnapshot,
    MessageTerminalSource, MissionLogEntry, NativeSessionAvailability, NativeSessionRef,
    NodeDaemonLeaseStatus, NodeProjectRegistration, NodeProjectRegistrationStatus,
    OrdinaryMessageBoundary, ProjectContext, ProjectKind, ProviderAccountRef, ProviderCapabilities,
    ProviderCapacityConfidence, ProviderCapacityEvidence, ProviderCapacitySnapshot,
    ProviderCapacityState, ProviderCompatibilityAdmission, ProviderCompatibilityAdmissionLifecycle,
    ProviderCompatibilityAdmissionPolicy, ProviderCompatibilityBlockBoundary,
    ProviderCompatibilityBlockCause, ProviderCompatibilityBlockSource, ProviderCompatibilityStatus,
    ProviderDispatchAttempt, ProviderDispatchIntent, ProviderEventFidelity,
    ProviderExecutionControls, ProviderExecutionStatus, ProviderFeatureMode,
    ProviderIntegrationProfile, ProviderInteractionMessageOption, ProviderInteractionMode,
    ProviderInteractionRequestBody, ProviderInteractionResponseBody, ProviderInteractionType,
    ProviderLaunchConfig, ProviderLaunchProfile, ProviderLaunchStatus, ProviderProcess,
    ProviderProcessHealth, ProviderProcessStatus, ProviderResponseIntent,
    ProviderRuntimeContextFact, ProviderRuntimeProjection, ProviderWorkDispatch,
    ProviderWorkDispatchStatus, RegistryDeliveryAttempt, RegistryDeliveryStatus, RegistryMessage,
    RegistryMessageIntent, Review, SecurityEnforcementLocus, SecurityEnforcementLocusKind,
    SenderKind, TeamActorKind, TeamActorRef, TeamDeliveryPolicy, TeamDeliveryStatus,
    TeamMemberCloseRequest, TeamMemberCloseStatus, TeamMessageProjection, TeamRecipientKind,
    TeamRecipientRef, TeamRunEvent, TeamRunEventSourceKind, TeamRunStatus, TeamSupervisorLease,
    Validate, Work, WorkCausationRef, WorkClaimMode, WorkCommandContext, WorkCondition,
    WorkDelegation, WorkDelegationState, WorkPhase, WorkPriority, WorkRef, WorkResolution,
    EXECUTION_MODE_EXTERNAL_INTERACTIVE,
};
use harness_store::{
    canonical_surface, CanonicalMemberRunAdmission, HarnessStore, HostAttentionClaimResult,
    MessageDeliveryClaimResult, StoreError,
};

// Mission/MissionStatus remain only for cfg(test) legacy-history fixtures.
#[cfg(test)]
use harness_core::{Mission, MissionStatus};
use thiserror::Error;

mod agentfirm_api;
mod claude_team_runtime;
mod codex_app_server;
mod codex_team_runtime;
mod execution_space;
mod execution_space_commands;
#[cfg(unix)]
mod fabric_runtime;
mod host_dispatcher;
mod kimi_acp;
mod kimi_team_runtime;
mod legacy_company_os;
mod legacy_export;
mod mcp;
mod native_session;
mod pi_rpc;
mod project;
mod project_commands;
mod provider_adapter;
mod provider_event_api;
#[cfg(unix)]
mod remote_fabric;
mod role_actions_api;
mod role_views_api;
mod runtime_adapter;
mod runtime_adapter_contract;
mod sse;
mod store_resolution;
#[cfg(unix)]
mod supervisor_daemon;
mod supervisor_wake;

#[path = "main_modules/http_protocol.rs"]
mod http_protocol;
use http_protocol::*;
#[path = "main_modules/http_exchange.rs"]
mod http_exchange;
use http_exchange::*;
#[path = "main_modules/http_get_routes.rs"]
mod http_get_routes;
#[path = "main_modules/http_post_routes.rs"]
mod http_post_routes;
#[path = "main_modules/http_trust_routes.rs"]
mod http_trust_routes;

#[path = "main_modules/user_commands.rs"]
mod user_commands;
use user_commands::*;
#[path = "main_modules/node_team_commands.rs"]
mod node_team_commands;
use node_team_commands::*;
#[path = "main_modules/team_provider_profiles.rs"]
mod team_provider_profiles;
use team_provider_profiles::*;
#[path = "main_modules/provider_native_identity.rs"]
mod provider_native_identity;
use provider_native_identity::*;
#[path = "main_modules/provider_capacity.rs"]
mod provider_capacity;
use provider_capacity::*;
#[path = "main_modules/team_run_setup.rs"]
mod team_run_setup;
use team_run_setup::*;
#[path = "main_modules/team_messaging.rs"]
mod team_messaging;
use team_messaging::*;
#[path = "main_modules/team_recovery_work.rs"]
mod team_recovery_work;
use team_recovery_work::*;
#[path = "main_modules/work_cli.rs"]
mod work_cli;
use work_cli::*;
#[path = "main_modules/host_binding.rs"]
mod host_binding;
use host_binding::*;
#[path = "main_modules/team_run_cli.rs"]
mod team_run_cli;
use team_run_cli::*;
#[path = "main_modules/runtime_effects.rs"]
mod runtime_effects;
use runtime_effects::*;
#[path = "main_modules/supervisor_control.rs"]
mod supervisor_control;
use supervisor_control::*;
#[path = "main_modules/member_work_coordination.rs"]
mod member_work_coordination;
use member_work_coordination::*;
#[path = "main_modules/managed_host_delivery.rs"]
mod managed_host_delivery;
use managed_host_delivery::*;
#[path = "main_modules/member_lifecycle.rs"]
mod member_lifecycle;
use member_lifecycle::*;
#[path = "main_modules/member_orchestration.rs"]
mod member_orchestration;
use member_orchestration::*;
#[path = "main_modules/provider_runners.rs"]
mod provider_runners;
use provider_runners::*;
#[path = "main_modules/pi_runner_state.rs"]
mod pi_runner_state;
use pi_runner_state::*;
#[path = "main_modules/provider_interactions.rs"]
mod provider_interactions;
use provider_interactions::*;
#[path = "main_modules/dashboard_server.rs"]
mod dashboard_server;
use dashboard_server::*;
#[path = "main_modules/http_action_dispatch.rs"]
mod http_action_dispatch;
use http_action_dispatch::*;
#[path = "main_modules/http_member_control.rs"]
mod http_member_control;
use http_member_control::*;
#[path = "main_modules/http_team_actions.rs"]
mod http_team_actions;
use http_team_actions::*;
#[path = "main_modules/http_io.rs"]
mod http_io;
use http_io::*;
#[path = "main_modules/runtime_delivery_setup.rs"]
mod runtime_delivery_setup;
use runtime_delivery_setup::*;
#[path = "main_modules/provider_schema.rs"]
mod provider_schema;
use provider_schema::*;
#[path = "main_modules/delivery_gateway.rs"]
mod delivery_gateway;
use delivery_gateway::*;
#[path = "main_modules/gateway_runtime.rs"]
mod gateway_runtime;
use gateway_runtime::*;
#[path = "main_modules/dashboard_projection.rs"]
mod dashboard_projection;
use dashboard_projection::*;
#[path = "main_modules/process_reaper.rs"]
mod process_reaper;
use process_reaper::*;
#[path = "main_modules/provider_adapters.rs"]
mod provider_adapters;
use provider_adapters::*;
#[path = "main_modules/codex_claude_adapters.rs"]
mod codex_claude_adapters;
use codex_claude_adapters::*;
#[path = "main_modules/cli_utilities.rs"]
mod cli_utilities;
use cli_utilities::*;

use execution_space_commands::{
    execution_space_command, execution_space_json, EXECUTION_LEDGER_NAMES,
};
#[cfg(test)]
use execution_space_commands::{
    execution_space_migrate_from_project, execution_space_migrate_from_project_with_activate,
    execution_space_migrate_from_project_with_hooks,
    execution_space_migrate_from_project_with_publish_hook,
};
use project_commands::project_command;
use store_resolution::*;

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("{0}")]
    SupervisorLeaseLost(String),
    #[error("store error: {0}")]
    Store(#[from] harness_store::StoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

type CliResult<T> = Result<T, CliError>;

impl CliError {
    fn is_supervisor_lease_lost(&self) -> bool {
        matches!(self, Self::SupervisorLeaseLost(_))
    }

    fn is_provider_compatibility_blocked(&self) -> bool {
        matches!(self, Self::Usage(message) if message.starts_with("PROVIDER_COMPATIBILITY_BLOCKED:"))
    }
}

/// Whether canonical Message fabric still exposes a Host delivery that has not
/// reached acknowledgement. The compatibility TeamMessage delivery policy is
/// not authority for this current status projection.
pub(crate) fn has_actionable_unacknowledged_host_delivery(message: &TeamMessageProjection) -> bool {
    message.deliveries.iter().any(|delivery| {
        delivery.member_id == "host"
            && matches!(
                delivery.status,
                TeamDeliveryStatus::Queued
                    | TeamDeliveryStatus::Claimed
                    | TeamDeliveryStatus::Delivered
            )
    })
}

fn store_conflict_as_usage<T>(result: Result<T, StoreError>) -> CliResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(StoreError::Conflict(message)) => Err(CliError::Usage(message)),
        Err(error) => Err(CliError::Store(error)),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

/// `harness init` routing (goal-multi-project init-routing task).
///
/// Instead of blindly materializing `./.harness`, `init` registers the SELECTED
/// project in the centralized registry and creates its store under
/// `~/.harness/projects/<id>/`, writing `metadata.json` to pin identity and the
/// `ACTIVE_PROJECT` marker so subsequent commands converge.
///
/// Which project is initialized:
/// - `--store`/`FIRM_ROOT` (`HARNESS_ROOT` alias) → that raw path is materialized exactly as
///   before (no registry entry), so compatibility tests keep passing.
/// - `--project <id|path>`             → the explicitly selected project root.
/// - otherwise                         → the CURRENT DIRECTORY (the dir the user
///   ran `init` in), NOT `_global` and NOT an ancestor's local `.harness`. This
///   preserves the historical "init targets here" intent while routing the store
///   centrally. The key invariant — never silently adopt an ancestor's local
///   `.harness` as the canonical store — holds because `resolve_store` skips the
///   cwd walk-up for `init`.
fn init_routed(store: &HarnessStore, resolved: &ResolvedStore) -> CliResult<()> {
    // Override path (`--store`/`FIRM_ROOT`/`HARNESS_ROOT`): raw-path behavior.
    if matches!(
        resolved.source,
        StoreSource::StoreFlag | StoreSource::FirmRootEnv | StoreSource::HarnessRootEnv
    ) {
        store.init()?;
        println!("initialized {}", store.root().display());
        return Ok(());
    }

    let firm_home = project::firm_home().map_err(project_err)?;
    // An explicit `--project`/`FIRM_PROJECT` selector pins the root via the
    // resolved context; otherwise `init` materializes the CURRENT directory as a
    // project (never the GLOBAL default, never an ancestor's `.harness`).
    let project_root = match resolved.source {
        StoreSource::ProjectFlag | StoreSource::ProjectEnv => resolved
            .context
            .as_ref()
            .map(|c| c.project_root.clone())
            .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
        _ => env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let ctx = project::register_and_activate(&firm_home, &project_root, &now_string())
        .map_err(project_err)?;
    let registered = HarnessStore::new(ctx.store_root.clone());
    registered.init()?;
    let existing_execution_rows = EXECUTION_LEDGER_NAMES
        .iter()
        .map(|ledger| count_non_empty_lines(&ctx.store_root.join(ledger)).unwrap_or(0))
        .sum::<u64>();
    if execution_space::active_space_id(&firm_home)
        .map_err(execution_space_err)?
        .is_none()
        && existing_execution_rows == 0
    {
        let space = execution_space::register_and_activate(
            &firm_home,
            &ctx.id,
            &format!("{} execution", ctx.id),
            Some(ctx.id.clone()),
            None,
            &now_string(),
        )
        .map_err(execution_space_err)?;
        HarnessStore::new(space.store_root.clone()).init()?;
        println!(
            "initialized project binding {} (root {}) and execution space {} ({})",
            ctx.id,
            ctx.project_root.display(),
            space.id,
            space.store_root.display()
        );
    } else {
        println!(
            "initialized project binding {} (root {}); compatibility store {} retained",
            ctx.id,
            ctx.project_root.display(),
            ctx.store_root.display()
        );
        if existing_execution_rows > 0 {
            eprintln!(
                "note: existing project-scoped execution rows were not moved; run `harness space migrate-from-project --from-project {} --id <space-id>` explicitly",
                ctx.id
            );
        }
    }
    Ok(())
}

/// Map a `project::ProjectError` onto `CliError` at the command boundary.
fn project_err(e: project::ProjectError) -> CliError {
    match e {
        project::ProjectError::Io(io) => CliError::Io(io),
        project::ProjectError::Json(j) => CliError::Json(j),
        project::ProjectError::NoHome => {
            CliError::Usage("could not determine home directory".to_string())
        }
    }
}

fn execution_space_err(error: execution_space::ExecutionSpaceError) -> CliError {
    match error {
        execution_space::ExecutionSpaceError::Io(error) => CliError::Io(error),
        execution_space::ExecutionSpaceError::Json(error) => CliError::Json(error),
        execution_space::ExecutionSpaceError::InvalidId(id) => CliError::Usage(format!(
            "invalid execution space id `{id}`; use letters, digits, '.', '_' or '-'"
        )),
        execution_space::ExecutionSpaceError::NoHome => {
            CliError::Usage("could not determine harness home".to_string())
        }
    }
}

fn run() -> CliResult<()> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    // Build identity is store-less and deterministic: it is safe to use as an
    // exact-revision preflight before selecting or opening any project store.
    if args.as_slice() == ["--build-info"] {
        return print_json(&serde_json::json!({
            "git_rev": build_git_rev(),
            "package_version": env!("CARGO_PKG_VERSION"),
        }));
    }
    // Optional debug flag: print which store was chosen and why (P7 "no silent
    // fallback"). Stripped before resolution so subcommands never see it.
    let store_source_debug = take_flag(&mut args, "--store-source");
    // `governance` is store-LESS: it gates a project's files (docs/skills) and
    // must run identically on any project — including a non-harness, no-node
    // repo — without resolving (or emitting deprecation noise about) a harness
    // store. Route it before `resolve_store` so it never touches the store.
    if args.first().map(String::as_str) == Some("governance") {
        return governance_command(&args[1..]);
    }
    // Legacy export is deliberately resolved outside the normal store fallback
    // chain. It requires one valid explicit project and never falls back to
    // cwd/current/global when the selector is invalid. Verification is fully
    // offline and resolves no live store.
    if args.first().map(String::as_str) == Some("legacy-goal-task") {
        return legacy_goal_task_command(&mut args);
    }
    // Legacy Company OS export/verify shares the same store-LESS discipline:
    // export enumerates every record store under the resolved Firm home
    // (machine-wide, so no project/space/company selector applies), and
    // verification is fully offline and resolves no live store.
    if args.first().map(String::as_str) == Some("legacy-company-os") {
        return legacy_company_os_command(&mut args);
    }
    // `cheatsheet` is store-LESS: it prints operating knowledge for the Host
    // and must not require a store, project, or space.
    if args.first().map(String::as_str) == Some("cheatsheet") {
        return cheatsheet_command(&args[1..]);
    }
    // Resolve the store root FIRST (strips a global `--store`/`--project` from
    // `args` so the subcommand parsers never see them). Commands started from
    // different working directories converge on one coordination store through
    // the current Execution Space selection.
    let command = command_name_for_resolution(&args);
    let resolved = resolve_store(&mut args, command.as_deref())?;
    if store_source_debug {
        eprintln!(
            "store-source: {:?} root={}",
            resolved.source,
            resolved.root.display()
        );
    }
    if args.is_empty() || args[0] == "help" || args[0] == "--help" {
        print_help();
        return Ok(());
    }

    let store = match resolved.provider_compatibility_scope() {
        Some((project_id, store_id)) => HarnessStore::new(resolved.root.clone())
            .with_provider_compatibility_scope(project_id, store_id),
        None => HarnessStore::new(resolved.root.clone()),
    };
    match args[0].as_str() {
        "init" => {
            init_routed(&store, &resolved)?;
        }
        "project" => project_command(&args[1..])?,
        "space" => execution_space_command(&args[1..])?,
        "agent" => return Err(retired_surface_error("agent")),
        "org" => org_command(&store, &args[1..])?,
        "node" => node_command(&store, &resolved, &args[1..])?,
        "team" => team_command(&store, &resolved, &args[1..])?,
        "mission" => mission_command(&store, &args[1..])?,
        "legacy" => legacy_command(&store, &args[1..])?,
        "wave" => {
            let subcommand = args.get(1).map(String::as_str).unwrap_or("help");
            if retired_wave_write_command(subcommand) {
                return Err(retired_wave_write_error(subcommand));
            }
            return Err(CliError::Usage(
                "Wave is Legacy-only (ADR 0051). Historical reads moved to `harness legacy wave list|show|history`; current coordination uses durable AgentTeam, Team-run Work, and identity-first Messages.".to_string(),
            ));
        }
        "team-run" => team_run_command(&store, &resolved, &args[1..])?,
        "member-run" => member_run_command(&store, &args[1..])?,
        "member" => member_command(&store, &args[1..])?,
        "member-trust" => member_trust_command(&store, &resolved, &args[1..])?,
        "provider" => provider_command(&store, &resolved, &args[1..])?,
        "company" => company_command(&store, &args[1..])?,
        "work" => global_work_command(&args[1..])?,
        "dashboard" => dashboard_command(&store, &resolved, &args[1..])?,
        "workflow" => return Err(retired_dynamic_workflow_error()),
        "hook" => return Err(retired_provider_hook_error()),
        "serve" => serve_command(&store, &resolved, &args[1..])?,
        #[cfg(unix)]
        "fabric" => fabric_runtime::fabric_command(&store, &resolved, &args[1..])?,
        "mcp" => mcp::run(&store, &resolved)?,
        #[cfg(unix)]
        "daemon" => daemon_command(&args[1..])?,
        command if retired_command(command) => return Err(retired_surface_error(command)),
        command => return Err(CliError::Usage(format!("unknown command: {command}"))),
    }
    Ok(())
}

fn retired_dynamic_workflow_error() -> CliError {
    CliError::Usage(
        "`harness workflow` is retired. Current execution uses Agent Team or Host execution; historical Dynamic Workflow data is available only through the documented legacy export and verify path."
            .to_string(),
    )
}

fn retired_provider_hook_error() -> CliError {
    CliError::Usage(
        "`harness hook` is retired with Dynamic Workflow provider-hook ingestion. Provider-native events stay in provider-native session storage."
            .to_string(),
    )
}

fn handle_http_connection(
    projects: &ServeProjects,
    mut stream: TcpStream,
    sse_manager: sse::SseManager,
) -> CliResult<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default().to_string();
    let path_only = path.split('?').next().unwrap_or_default().to_string();
    // `?space=<id>` selects the coordination store. `?project=<id>` independently
    // selects the provider workspace binding. In compatibility mode only, the
    // old `?project` store selector remains readable.
    let project_param = query_param(&path, "project");
    let space_param = query_param(&path, "space");
    let store_selector = if projects.default_space.is_some() {
        space_param.as_deref()
    } else {
        space_param.as_deref().or(project_param.as_deref())
    };
    let (project_id, store_owned) = match projects.store_for(store_selector) {
        Ok(resolved) => resolved,
        Err(error) => {
            let detail = error.to_string();
            write_http_json(
                &mut stream,
                "404 Not Found",
                &serde_json::json!({
                    "ok": false,
                    "error": "execution_space_not_found",
                    "detail": detail,
                }),
            )?;
            return Ok(());
        }
    };
    // DOC-108: `/v1/company-os/*` reads and writes are retired; the retired
    // tombstone below answers them directly and no Company Store is resolved.
    let company_os_path = path_only.starts_with("/v1/company-os/");
    let mut content_length = 0usize;
    let mut trust_transport_token = None;
    let mut trust_idempotency_key = None;
    let mut trust_expected_version = None;
    let mut trust_confirmed_action = None;
    let mut trust_identity_override_header = false;
    let mut live_provider_activity_token = None;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
            if name.eq_ignore_ascii_case("x-agentfirm-token") {
                trust_transport_token = Some(value.trim().to_string());
            }
            if name.eq_ignore_ascii_case("idempotency-key") {
                trust_idempotency_key = Some(value.trim().to_string());
            }
            if name.eq_ignore_ascii_case("if-match") {
                trust_expected_version = value.trim().trim_matches('"').parse::<u64>().ok();
            }
            if name.eq_ignore_ascii_case("x-agentfirm-confirm") {
                trust_confirmed_action = Some(value.trim().to_string());
            }
            if name.eq_ignore_ascii_case("x-agentfirm-live-token") {
                live_provider_activity_token = Some(value.trim().to_string());
            }
            if matches!(
                name.to_ascii_lowercase().as_str(),
                "x-agentfirm-actor-kind"
                    | "x-agentfirm-actor-id"
                    | "x-agentfirm-authority-kind"
                    | "x-agentfirm-authority-id"
            ) {
                trust_identity_override_header = true;
            }
        }
    }
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    if method == "OPTIONS" {
        write_http_response(&mut stream, "204 No Content", "application/json", b"{}")?;
        return Ok(());
    }
    if method != "GET" && method != "POST" {
        write_http_json(
            &mut stream,
            "405 Method Not Allowed",
            &serde_json::json!({"error": "method_not_allowed"}),
        )?;
        return Ok(());
    }
    let mut exchange = HttpExchange {
        projects,
        stream: &mut stream,
        sse_manager,
        method: method.to_string(),
        path,
        path_only,
        project_param,
        project_id,
        store: store_owned,
        company_os_path,
        body,
        trust_transport_token,
        trust_idempotency_key,
        trust_expected_version,
        trust_confirmed_action,
        trust_identity_override_header,
        live_provider_activity_token,
    };
    if exchange.handle_trust_routes()?
        || exchange.handle_get_routes()?
        || exchange.handle_dashboard_post()?
    {
        return Ok(());
    }
    Ok(())
}
#[cfg(test)]
#[path = "main_tests/general.rs"]
mod tests;

#[cfg(test)]
#[path = "main_tests/team_member_assignment.rs"]
mod team_member_assignment_tests;

#[cfg(test)]
#[path = "main_tests/sse.rs"]
mod sse_tests;

// ── Tests for team-run recover decision logic ────────────────────

#[cfg(test)]
#[path = "main_tests/team_run_recover.rs"]
mod tests_team_run_recover;

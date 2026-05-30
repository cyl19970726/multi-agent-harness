use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use harness_core::{
    AgentEvent, AgentMember, AgentMemberStatus, AgentProviderConfig, AgentRuntime,
    AgentRuntimeHealth, AgentRuntimeStatus, AgentTeam, AgentTeamStatus, Decision, EvaluationOutcome,
    Evidence, Gap, GapSeverity, GapStatus, Goal, GoalCase, GoalDesign, GoalEvaluation, GoalStatus,
    Message, MessageDelivery, MessageDeliveryStatus, MessageKind, MessageTerminalSource, Proposal,
    ProposalStatus, ProviderChildThread, ProviderChildThreadStatus, ProviderSession,
    ProviderSessionStatus, Review, ReviewVerdict, Task, TaskStatus, Vision,
};
use harness_store::{HarnessStore, MessageDeliveryClaimResult};
use thiserror::Error;
use tungstenite::client::IntoClientRequest;
use tungstenite::{Message as WebSocketMessage, WebSocket};

unsafe extern "C" {
    fn setsid() -> i32;
}

#[derive(Debug, Error)]
enum CliError {
    #[error("{0}")]
    Usage(String),
    #[error("store error: {0}")]
    Store(#[from] harness_store::StoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

type CliResult<T> = Result<T, CliError>;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> CliResult<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || args[0] == "help" || args[0] == "--help" {
        print_help();
        return Ok(());
    }

    let store = HarnessStore::new(env::var("HARNESS_ROOT").unwrap_or_else(|_| ".harness".into()));
    match args[0].as_str() {
        "init" => {
            store.init()?;
            println!("initialized {}", store.root().display());
        }
        "agent" => agent_command(&store, &args[1..])?,
        "team" => team_command(&store, &args[1..])?,
        "member" => member_command(&store, &args[1..])?,
        "goal" => goal_command(&store, &args[1..])?,
        "task" => task_command(&store, &args[1..])?,
        "message" => message_command(&store, &args[1..])?,
        "event" => event_command(&store, &args[1..])?,
        "proposal" => proposal_command(&store, &args[1..])?,
        "git" => git_command(&store, &args[1..])?,
        "review" => review_command(&store, &args[1..])?,
        "gap" => gap_command(&store, &args[1..])?,
        "goal-design" => goal_design_command(&store, &args[1..])?,
        "goal-evaluation" => goal_evaluation_command(&store, &args[1..])?,
        "goal-case" => goal_case_command(&store, &args[1..])?,
        "vision" => vision_command(&store, &args[1..])?,
        "evidence" => evidence_command(&store, &args[1..])?,
        "decision" => decision_command(&store, &args[1..])?,
        "autonomy" => autonomy_command(&store, &args[1..])?,
        "dashboard" => dashboard_command(&store, &args[1..])?,
        "board" => board_command(&store)?,
        "codex" => codex_command(&store, &args[1..])?,
        "hook" => hook_command(&store, &args[1..])?,
        "serve" => serve_command(&store, &args[1..])?,
        command => return Err(CliError::Usage(format!("unknown command: {command}"))),
    }
    Ok(())
}

fn member_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "member register|list")?;
    match args[0].as_str() {
        "register" => {
            let member = build_member_from_args(args, AgentMemberStatus::Idle)?;
            store.append_member(&member)?;
            print_json(&member)?;
        }
        "list" => print_json(&store.members()?)?,
        other => return Err(CliError::Usage(format!("unknown member command: {other}"))),
    }
    Ok(())
}

fn agent_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "agent create|list|show|start|health|hooks|send|deliver|retry-delivery|reconcile-session|gateway|ingest|close",
    )?;
    match args[0].as_str() {
        "create" => {
            let mut member = build_member_from_args(args, AgentMemberStatus::Creating)?;
            let prompt_ref = ensure_agent_prompt(store, &member, args)?;
            member.prompt_ref = Some(prompt_ref);
            if has_flag(args, "--start") {
                store.append_member(&member)?;
                let runtime = start_codex_runtime(store, &member)?;
                let now = now_string();
                member.status = AgentMemberStatus::Idle;
                member.provider_runtime_id = Some(runtime.id.clone());
                member.control_endpoint = runtime.control_endpoint.clone();
                member.last_seen_at = Some(now);
                store.append_runtime(&runtime)?;
                append_agent_event(
                    store,
                    &member.id,
                    Some(runtime.id.as_str()),
                    None,
                    "runtime_started",
                    "Codex app-server runtime started",
                    None,
                )?;
            } else {
                member.status = AgentMemberStatus::Idle;
            }
            store.append_member(&member)?;
            append_agent_event(
                store,
                &member.id,
                member.provider_runtime_id.as_deref(),
                None,
                "agent_created",
                "Agent Member created",
                member.prompt_ref.as_deref(),
            )?;
            print_json(&member)?;
        }
        "list" => print_json(&latest_members(store)?.into_values().collect::<Vec<_>>())?,
        "start" => {
            let id = required(args, "--id").or_else(|_| required(args, "--agent"))?;
            let member = start_agent_runtime(store, &id)?;
            print_json(&member)?;
        }
        "health" => {
            let id = required(args, "--id").or_else(|_| required(args, "--agent"))?;
            print_json(&agent_health(store, &id)?)?;
        }
        "hooks" => {
            let id = required(args, "--id").or_else(|_| required(args, "--agent"))?;
            let timeout_ms = value(args, "--timeout-ms")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(3_000);
            print_json(&probe_agent_hooks(
                store,
                &id,
                timeout_ms,
                has_flag(args, "--trust"),
            )?)?;
        }
        "show" => {
            let id = required(args, "--id")?;
            let member = latest_member(store, &id)?;
            let runtimes: Vec<_> = store
                .runtimes()?
                .into_iter()
                .filter(|runtime| runtime.agent_member_id == id)
                .collect();
            let events: Vec<_> = store
                .events()?
                .into_iter()
                .filter(|event| event.agent_member_id == id)
                .collect();
            let proposals: Vec<_> = store
                .proposals()?
                .into_iter()
                .filter(|proposal| proposal.agent_member_id == id)
                .collect();
            let messages: Vec<_> = store
                .messages()?
                .into_iter()
                .filter(|message| {
                    message.from_agent_id == id
                        || message.to_agent_id.as_deref() == Some(id.as_str())
                })
                .collect();
            let provider_child_threads: Vec<_> = store
                .provider_child_threads()?
                .into_iter()
                .filter(|thread| thread.agent_member_id == id)
                .collect();
            print_json(&serde_json::json!({
                "member": member,
                "runtimes": runtimes,
                "events": events,
                "proposals": proposals,
                "messages": messages,
                "provider_child_threads": provider_child_threads
            }))?;
        }
        "send" => {
            let to_agent_id = required(args, "--to")?;
            let target = latest_member(store, &to_agent_id)?;
            ensure_member_accepts_delivery(&target)?;
            let message = Message {
                id: value(args, "--id").unwrap_or_else(|| generated_id("msg")),
                task_id: value(args, "--task"),
                from_agent_id: required(args, "--from")?,
                to_agent_id: Some(to_agent_id.clone()),
                channel: Some(value(args, "--channel").unwrap_or_else(|| "agent-direct".into())),
                kind: parse_message_kind(
                    &value(args, "--kind").unwrap_or_else(|| "message".into()),
                )?,
                delivery_status: MessageDeliveryStatus::Queued,
                content: required(args, "--content")?,
                evidence_ids: many(args, "--evidence"),
                created_at: now_string(),
                delivery: None,
            };
            store.append_message(&message)?;
            append_agent_event(
                store,
                &target.id,
                target.provider_runtime_id.as_deref(),
                message.task_id.as_deref(),
                "message_queued",
                "Message queued for Agent Member",
                None,
            )?;
            print_json(&message)?;
        }
        "deliver" => deliver_agent_messages(store, args)?,
        "retry-delivery" => {
            let result = retry_delivery_value(
                store,
                &required(args, "--agent").or_else(|_| required(args, "--id"))?,
                &required(args, "--message")?,
                value(args, "--session").as_deref(),
                &value(args, "--reason").unwrap_or_else(|| "operator requested retry".into()),
                has_flag(args, "--force"),
            )?;
            print_json(&result)?;
        }
        "reconcile-session" => {
            let result = reconcile_provider_session_value(
                store,
                &required(args, "--agent").or_else(|_| required(args, "--id"))?,
                &required(args, "--session")?,
                parse_provider_session_status(&required(args, "--status")?)?,
                parse_terminal_source(
                    value(args, "--terminal-source")
                        .as_deref()
                        .unwrap_or("unknown"),
                )?,
                &value(args, "--reason").unwrap_or_else(|| "operator reconciliation".into()),
            )?;
            print_json(&result)?;
        }
        "gateway" => run_provider_gateway(store, args)?,
        "ingest" => {
            let agent_id = required(args, "--agent")?;
            let before_events = store.events()?.len();
            ingest_provider_output(
                store,
                &agent_id,
                value(args, "--runtime").as_deref(),
                value(args, "--task").as_deref(),
                &required(args, "--source")?,
            )?;
            let after_events = store.events()?.len();
            print_json(&serde_json::json!({
                "agent_member_id": agent_id,
                "events_ingested": after_events.saturating_sub(before_events)
            }))?;
        }
        "close" => {
            let id = required(args, "--id")?;
            print_json(&close_agent_member_value(store, &id)?)?;
        }
        other => return Err(CliError::Usage(format!("unknown agent command: {other}"))),
    }
    Ok(())
}

fn team_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "team create|list|show|close")?;
    match args[0].as_str() {
        "create" => {
            let team = AgentTeam {
                id: value(args, "--id").unwrap_or_else(|| generated_id("team")),
                name: required(args, "--name")?,
                description: required(args, "--description")?,
                owner_agent_id: required(args, "--owner")?,
                status: AgentTeamStatus::Active,
                member_ids: many(args, "--member"),
                created_at: now_string(),
                updated_at: now_string(),
            };
            store.append_team(&team)?;
            print_json(&team)?;
        }
        "list" => {
            let teams = latest_teams(store)?
                .into_values()
                .filter(|team| has_flag(args, "--all") || team.status == AgentTeamStatus::Active)
                .collect::<Vec<_>>();
            print_json(&teams)?
        }
        "show" => {
            let id = required(args, "--id")?;
            let team = latest_teams(store)?
                .remove(&id)
                .ok_or_else(|| CliError::Usage(format!("team not found: {id}")))?;
            print_json(&team)?;
        }
        "close" => {
            let id = required(args, "--id")?;
            let mut team = latest_teams(store)?
                .remove(&id)
                .ok_or_else(|| CliError::Usage(format!("team not found: {id}")))?;
            team.status = AgentTeamStatus::Closed;
            team.updated_at = now_string();
            store.append_team(&team)?;
            print_json(&team)?;
        }
        other => return Err(CliError::Usage(format!("unknown team command: {other}"))),
    }
    Ok(())
}

fn goal_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "goal create|list|learning-status")?;
    match args[0].as_str() {
        "create" => {
            let goal = Goal {
                id: value(args, "--id").unwrap_or_else(|| generated_id("goal")),
                title: required(args, "--title")?,
                objective: required(args, "--objective")?,
                owner_agent_id: required(args, "--owner")?,
                status: GoalStatus::Active,
                success_criteria: many(args, "--success"),
                priority: value(args, "--priority").unwrap_or_else(|| "p0".into()),
                created_at: now_string(),
                updated_at: now_string(),
                vision_id: value(args, "--vision"),
                goal_design_id: value(args, "--goal-design"),
                closed_by_decision_id: value(args, "--closed-by-decision"),
            };
            store.append_goal(&goal)?;
            print_json(&goal)?;
        }
        "learning-status" => {
            let goal_id = required(args, "--id").or_else(|_| required(args, "--goal"))?;
            let status = goal_learning_status(store, &goal_id)?;
            print_json(&status.to_json())?;
            if has_flag(args, "--strict") {
                let waiver_decision_id = value(args, "--waiver-decision");
                status.require_for_gate(
                    store,
                    has_flag(args, "--require-evaluation"),
                    has_flag(args, "--allow-waiver"),
                    waiver_decision_id.as_deref(),
                )?;
            }
        }
        "list" => print_json(&store.goals()?)?,
        other => return Err(CliError::Usage(format!("unknown goal command: {other}"))),
    }
    Ok(())
}

fn task_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "task create|assign|status|list|show")?;
    match args[0].as_str() {
        "create" => {
            let task = Task {
                id: value(args, "--id").unwrap_or_else(|| generated_id("task")),
                goal_id: value(args, "--goal"),
                parent_task_id: value(args, "--parent"),
                title: required(args, "--title")?,
                objective: required(args, "--objective")?,
                owner_agent_id: required(args, "--owner")?,
                assignee_agent_id: value(args, "--assignee"),
                reviewer_agent_id: value(args, "--reviewer"),
                status: TaskStatus::Planned,
                depends_on_task_ids: many(args, "--depends-on"),
                workspace_ref: value(args, "--workspace"),
                branch_ref: value(args, "--branch"),
                pr_ref: value(args, "--pr"),
                owned_paths: many(args, "--owned-path"),
                acceptance_criteria: many(args, "--acceptance"),
                created_at: now_string(),
                updated_at: now_string(),
                phase: value(args, "--phase"),
                scope_refs: many(args, "--scope-ref"),
                requires_human_approval: has_flag(args, "--requires-human-approval"),
                verdict_decision_id: value(args, "--verdict-decision"),
            };
            store.append_task(&task)?;
            print_json(&task)?;
        }
        "assign" => {
            let task_id = required(args, "--id")?;
            let assignee = required(args, "--assignee")?;
            let mut task = latest_task(store, &task_id)?;
            if let Some(goal_id) = task.goal_id.as_deref() {
                let status = goal_learning_status(store, goal_id)?;
                if status.goal_design.is_empty() {
                    if has_flag(args, "--allow-missing-goal-design") {
                        let waiver_decision_id = value(args, "--waiver-decision");
                        status.require_valid_waiver(store, waiver_decision_id.as_deref())?;
                    } else {
                        return Err(CliError::Usage(format!(
                            "task {task_id} cannot be assigned before goal {goal_id} has goal_design evidence; use --allow-missing-goal-design with --waiver-decision <id> only for an explicit design-stage waiver"
                        )));
                    }
                }
            }
            task.assignee_agent_id = Some(assignee.clone());
            task.status = TaskStatus::Assigned;
            task.updated_at = now_string();
            store.append_task(&task)?;
            let message = Message {
                id: generated_id("msg"),
                task_id: Some(task.id.clone()),
                from_agent_id: task.owner_agent_id.clone(),
                to_agent_id: Some(assignee),
                channel: Some(value(args, "--channel").unwrap_or_else(|| "task-assignment".into())),
                kind: MessageKind::Task,
                delivery_status: MessageDeliveryStatus::Queued,
                content: format!("Assigned task {}", task.id),
                evidence_ids: Vec::new(),
                created_at: now_string(),
                delivery: None,
            };
            store.append_message(&message)?;
            print_json(&task)?;
        }
        "status" => {
            let task_id = required(args, "--id")?;
            let mut task = latest_task(store, &task_id)?;
            task.status = parse_task_status(&required(args, "--status")?)?;
            task.updated_at = now_string();
            store.append_task(&task)?;
            print_json(&task)?;
        }
        "list" => print_json(&latest_tasks(store)?.into_values().collect::<Vec<_>>())?,
        "show" => print_json(&latest_task(store, &required(args, "--id")?)?)?,
        other => return Err(CliError::Usage(format!("unknown task command: {other}"))),
    }
    Ok(())
}

fn message_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "message send|list|status")?;
    match args[0].as_str() {
        "send" => {
            let message = Message {
                id: value(args, "--id").unwrap_or_else(|| generated_id("msg")),
                task_id: value(args, "--task"),
                from_agent_id: required(args, "--from")?,
                to_agent_id: value(args, "--to"),
                channel: value(args, "--channel"),
                kind: parse_message_kind(
                    &value(args, "--kind").unwrap_or_else(|| "message".into()),
                )?,
                delivery_status: MessageDeliveryStatus::Queued,
                content: required(args, "--content")?,
                evidence_ids: many(args, "--evidence"),
                created_at: now_string(),
                delivery: None,
            };
            store.append_message(&message)?;
            print_json(&message)?;
        }
        "list" => {
            let mut messages = latest_messages_in_append_order(store)?;
            if let Some(channel) = value(args, "--channel") {
                messages.retain(|message| message.channel.as_deref() == Some(channel.as_str()));
            }
            if let Some(task_id) = value(args, "--task") {
                messages.retain(|message| message.task_id.as_deref() == Some(task_id.as_str()));
            }
            print_json(&messages)?;
        }
        "status" => {
            let id = required(args, "--id")?;
            let mut message = latest_message(store, &id)?;
            message.delivery_status = parse_delivery_status(&required(args, "--status")?)?;
            store.append_message(&message)?;
            print_json(&message)?;
        }
        other => return Err(CliError::Usage(format!("unknown message command: {other}"))),
    }
    Ok(())
}

fn event_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "event add|list")?;
    match args[0].as_str() {
        "add" => {
            let event = AgentEvent {
                id: value(args, "--id").unwrap_or_else(|| generated_id("event")),
                agent_member_id: required(args, "--agent")?,
                provider_runtime_id: value(args, "--runtime"),
                task_id: value(args, "--task"),
                provider: value(args, "--provider").unwrap_or_else(|| "codex".into()),
                provider_thread_id: value(args, "--provider-thread"),
                provider_turn_id: value(args, "--provider-turn"),
                provider_child_thread_id: value(args, "--provider-child-thread"),
                event_type: required(args, "--type")?,
                summary: required(args, "--summary")?,
                payload_ref: value(args, "--payload-ref"),
                created_at: now_string(),
            };
            store.append_event(&event)?;
            print_json(&event)?;
        }
        "list" => {
            let mut events = store.events()?;
            if let Some(agent_id) = value(args, "--agent") {
                events.retain(|event| event.agent_member_id == agent_id);
            }
            if let Some(task_id) = value(args, "--task") {
                events.retain(|event| event.task_id.as_deref() == Some(task_id.as_str()));
            }
            print_json(&events)?;
        }
        other => return Err(CliError::Usage(format!("unknown event command: {other}"))),
    }
    Ok(())
}

fn hook_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "hook record --agent <agent> [--runtime <runtime>] [--task <task>]",
    )?;
    match args[0].as_str() {
        "record" => {
            record_codex_hook_event(store, args)?;
        }
        other => return Err(CliError::Usage(format!("unknown hook command: {other}"))),
    }
    Ok(())
}

fn record_codex_hook_event(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    store.init()?;
    let agent_id = value(args, "--agent")
        .or_else(|| env::var("HARNESS_AGENT_MEMBER_ID").ok())
        .ok_or_else(|| CliError::Usage("--agent is required".into()))?;
    let runtime_id = value(args, "--runtime").or_else(|| env::var("HARNESS_AGENT_RUNTIME_ID").ok());
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin)?;
    let payload = parse_hook_payload(&stdin);
    let hook_event_name = value(args, "--event")
        .or_else(|| json_str(&payload, "hook_event_name"))
        .unwrap_or_else(|| "unknown".into());
    let task_id = value(args, "--task")
        .or_else(|| env::var("HARNESS_TASK_ID").ok())
        .or_else(|| {
            latest_member(store, &agent_id)
                .ok()
                .and_then(|member| member.current_task_id)
        });
    let provider_thread_id = thread_id_from_container(&payload);
    let provider_turn_id =
        json_str(&payload, "turn_id").or_else(|| turn_id_from_container(&payload));
    let event_id = generated_id("event");
    let payload_ref = persist_hook_payload(store, &event_id, &payload)?;
    let now = now_string();
    let event = AgentEvent {
        id: event_id,
        agent_member_id: agent_id.clone(),
        provider_runtime_id: runtime_id.clone(),
        task_id: task_id.clone(),
        provider: "codex".into(),
        provider_thread_id: provider_thread_id.clone(),
        provider_turn_id: provider_turn_id.clone(),
        provider_child_thread_id: json_str(&payload, "agent_id"),
        event_type: format!("codex_hook.{hook_event_name}"),
        summary: codex_hook_summary(&hook_event_name, &payload),
        payload_ref: Some(payload_ref),
        created_at: now.clone(),
    };
    store.append_event(&event)?;
    if let Ok(mut member) = latest_member(store, &agent_id) {
        member.last_seen_at = Some(now.clone());
        member.status = if hook_event_name.eq_ignore_ascii_case("stop") {
            member.current_task_id = None;
            AgentMemberStatus::Idle
        } else {
            AgentMemberStatus::Running
        };
        store.append_member(&member)?;
    }
    if let Some(runtime_id) = runtime_id {
        if let Some(mut runtime) = latest_runtime(store, &runtime_id)? {
            runtime.last_event_at = Some(now);
            store.append_runtime(&runtime)?;
        }
    }
    if hook_event_name.eq_ignore_ascii_case("stop") {
        reconcile_running_provider_sessions(
            store,
            &agent_id,
            task_id.as_deref(),
            provider_thread_id.as_deref(),
            provider_turn_id.as_deref(),
            MessageTerminalSource::HookStop,
        )?;
    }
    Ok(())
}

fn proposal_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "proposal create|from-diff|list|status")?;
    match args[0].as_str() {
        "create" => {
            let proposal = Proposal {
                id: value(args, "--id").unwrap_or_else(|| generated_id("proposal")),
                task_id: required(args, "--task")?,
                agent_member_id: required(args, "--agent")?,
                title: required(args, "--title")?,
                summary: required(args, "--summary")?,
                status: ProposalStatus::Draft,
                changed_paths: many(args, "--changed-path"),
                evidence_ids: many(args, "--evidence"),
                created_at: now_string(),
                updated_at: now_string(),
            };
            store.append_proposal(&proposal)?;
            print_json(&proposal)?;
        }
        "from-diff" => proposal_from_diff(store, args)?,
        "list" => {
            let mut proposals = store.proposals()?;
            if let Some(agent_id) = value(args, "--agent") {
                proposals.retain(|proposal| proposal.agent_member_id == agent_id);
            }
            if let Some(task_id) = value(args, "--task") {
                proposals.retain(|proposal| proposal.task_id == task_id);
            }
            print_json(&proposals)?;
        }
        "status" => {
            let id = required(args, "--id")?;
            let mut proposal = latest_proposal(store, &id)?;
            proposal.status = parse_proposal_status(&required(args, "--status")?)?;
            proposal.updated_at = now_string();
            store.append_proposal(&proposal)?;
            print_json(&proposal)?;
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown proposal command: {other}"
            )))
        }
    }
    Ok(())
}

fn evidence_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "evidence add|list")?;
    match args[0].as_str() {
        "add" => {
            let evidence = Evidence {
                id: value(args, "--id").unwrap_or_else(|| generated_id("evidence")),
                task_id: value(args, "--task"),
                source_type: required(args, "--source-type")?,
                source_ref: required(args, "--source-ref")?,
                summary: required(args, "--summary")?,
                created_at: now_string(),
                evidence_kind: value(args, "--evidence-kind"),
                goal_id: value(args, "--goal"),
            };
            store.append_evidence(&evidence)?;
            print_json(&evidence)?;
        }
        "list" => print_json(&store.evidence()?)?,
        other => {
            return Err(CliError::Usage(format!(
                "unknown evidence command: {other}"
            )))
        }
    }
    Ok(())
}

fn decision_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "decision record|list")?;
    match args[0].as_str() {
        "record" => {
            let decision = Decision {
                id: value(args, "--id").unwrap_or_else(|| generated_id("decision")),
                task_id: required(args, "--task")?,
                decision: required(args, "--decision")?,
                rationale: required(args, "--rationale")?,
                evidence_ids: many(args, "--evidence"),
                created_at: now_string(),
                decision_kind: value(args, "--decision-kind"),
                goal_id: value(args, "--goal"),
                is_waiver: has_flag(args, "--waiver"),
                follow_up_task_id: value(args, "--follow-up-task"),
            };
            store.append_decision(&decision)?;
            print_json(&decision)?;
        }
        "list" => print_json(&store.decisions()?)?,
        other => {
            return Err(CliError::Usage(format!(
                "unknown decision command: {other}"
            )))
        }
    }
    Ok(())
}

fn autonomy_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "autonomy observe|plan-next|decide|tick|loop")?;
    match args[0].as_str() {
        "observe" => print_json(&autonomy_observe_value(store, &args[1..])?)?,
        "plan-next" => print_json(&autonomy_plan_next_value(store, &args[1..])?)?,
        "decide" => print_json(&autonomy_decide_value(store, &args[1..])?)?,
        "tick" => print_json(&autonomy_tick_value(store, &args[1..])?)?,
        "loop" => run_autonomy_loop(store, &args[1..])?,
        other => {
            return Err(CliError::Usage(format!(
                "unknown autonomy command: {other}"
            )))
        }
    }
    Ok(())
}

fn autonomy_observe_value(store: &HarnessStore, args: &[String]) -> CliResult<serde_json::Value> {
    let goal_id = required(args, "--goal")?;
    let task_id = value(args, "--task");
    let observer = required(args, "--observer")?;
    let lead = required(args, "--lead")?;
    let kind = value(args, "--kind").unwrap_or_else(|| "goal_proposal".into());
    validate_autonomy_proposal_kind(&kind)?;
    let goal = latest_goals(store)?
        .remove(&goal_id)
        .ok_or_else(|| CliError::Usage(format!("goal not found: {goal_id}")))?;
    if let Some(task_id) = task_id.as_deref() {
        let task = latest_task(store, task_id)?;
        if task.goal_id.as_deref() != Some(goal_id.as_str()) {
            return Err(CliError::Usage(format!(
                "task {task_id} does not belong to goal {goal_id}"
            )));
        }
    }
    latest_member(store, &observer)?;
    let lead_member = latest_member(store, &lead)?;
    ensure_member_accepts_delivery(&lead_member)?;
    let summary = value(args, "--summary").unwrap_or_else(|| {
        autonomy_observation_summary(store, &goal_id)
            .unwrap_or_else(|_| format!("{observer} proposes {kind} for goal {goal_id}"))
    });
    let title = value(args, "--title").unwrap_or_else(|| format!("{kind}: {}", goal.title));
    let evidence = autonomy_evidence(
        store,
        task_id.clone(),
        &kind,
        &summary,
        &format!("# {title}\n\nkind: {kind}\ngoal: {goal_id}\nobserver: {observer}\nlead: {lead}\n\n{summary}\n"),
    )?;
    let message = Message {
        id: value(args, "--message-id").unwrap_or_else(|| generated_id("msg")),
        task_id,
        from_agent_id: observer.clone(),
        to_agent_id: Some(lead.clone()),
        channel: Some(value(args, "--channel").unwrap_or_else(|| "observer-proposal".into())),
        kind: MessageKind::Message,
        delivery_status: MessageDeliveryStatus::Queued,
        content: format!("{title}\n\n{summary}"),
        evidence_ids: vec![evidence.id.clone()],
        created_at: now_string(),
        delivery: None,
    };
    store.append_evidence(&evidence)?;
    store.append_message(&message)?;
    append_agent_event(
        store,
        &observer,
        None,
        message.task_id.as_deref(),
        "autonomy_proposal_created",
        &format!("Observer created {kind}"),
        Some(evidence.source_ref.as_str()),
    )?;
    Ok(serde_json::json!({
        "goal_id": goal_id,
        "proposal": evidence,
        "message": message
    }))
}

fn autonomy_plan_next_value(store: &HarnessStore, args: &[String]) -> CliResult<serde_json::Value> {
    let goal_id = required(args, "--goal")?;
    let task_id = required(args, "--task")?;
    let observer = required(args, "--observer")?;
    let lead = required(args, "--lead")?;
    let task = latest_task(store, &task_id)?;
    if task.goal_id.as_deref() != Some(goal_id.as_str()) {
        return Err(CliError::Usage(format!(
            "task {task_id} does not belong to goal {goal_id}"
        )));
    }
    latest_member(store, &observer)?;
    let lead_member = latest_member(store, &lead)?;
    ensure_member_accepts_delivery(&lead_member)?;
    let status = goal_learning_status(store, &goal_id)?;
    let status_json = status.to_json();
    let warnings = status.warnings(true);
    let vision_ref = value(args, "--vision-ref");
    let vision_summary = value(args, "--vision-summary").unwrap_or_else(|| {
        vision_ref
            .as_ref()
            .map(|vision_ref| format!("Vision reference: {vision_ref}"))
            .unwrap_or_else(|| "No explicit vision reference supplied.".into())
    });
    let summary = value(args, "--summary").unwrap_or_else(|| {
        if warnings.is_empty() {
            format!(
                "Next-round plan for {goal_id}: prior goal has complete learning evidence; compare GoalEvaluation with vision and propose the next goal."
            )
        } else {
            format!(
                "Next-round plan for {goal_id}: unresolved warnings require follow-up: {}",
                warnings.join("; ")
            )
        }
    });
    let plan = autonomy_evidence(
        store,
        Some(task_id.clone()),
        "next_round_plan",
        &summary,
        &format!(
            "# Next Round Plan\n\ngoal: {goal_id}\nobserver: {observer}\nlead: {lead}\nvision_ref: {}\nvision_summary: {vision_summary}\n\nsummary: {summary}\n\nstatus:\n```json\n{}\n```\n",
            vision_ref.as_deref().unwrap_or("-"),
            serde_json::to_string_pretty(&status_json).expect("serialize goal learning status")
        ),
    )?;
    let proposal_summary = value(args, "--proposal-summary").unwrap_or_else(|| {
        format!(
            "Observer proposes the next goal/task graph from GoalEvaluation and dashboard learning for {goal_id}."
        )
    });
    let proposal = autonomy_evidence(
        store,
        Some(task_id.clone()),
        "goal_proposal",
        &proposal_summary,
        &format!(
            "# Goal Proposal\n\ngoal: {goal_id}\nobserver: {observer}\nlead: {lead}\nsource_plan: {}\nvision_ref: {}\n\n{proposal_summary}\n",
            plan.id,
            vision_ref.as_deref().unwrap_or("-")
        ),
    )?;
    store.append_evidence(&plan)?;
    store.append_evidence(&proposal)?;
    let message = Message {
        id: value(args, "--message-id").unwrap_or_else(|| generated_id("msg")),
        task_id: Some(task_id.clone()),
        from_agent_id: observer.clone(),
        to_agent_id: Some(lead.clone()),
        channel: Some(value(args, "--channel").unwrap_or_else(|| "next-round-proposal".into())),
        kind: MessageKind::Message,
        delivery_status: MessageDeliveryStatus::Queued,
        content: format!("Next-round proposal for {goal_id}\n\n{proposal_summary}"),
        evidence_ids: vec![plan.id.clone(), proposal.id.clone()],
        created_at: now_string(),
        delivery: None,
    };
    store.append_message(&message)?;
    append_agent_event(
        store,
        &observer,
        None,
        Some(task_id.as_str()),
        "next_round_planned",
        "Observer created next-round plan and goal proposal",
        Some(plan.source_ref.as_str()),
    )?;
    Ok(serde_json::json!({
        "goal_id": goal_id,
        "plan": plan,
        "proposal": proposal,
        "message": message
    }))
}

fn autonomy_decide_value(store: &HarnessStore, args: &[String]) -> CliResult<serde_json::Value> {
    let task_id = required(args, "--task")?;
    let lead = required(args, "--lead")?;
    let proposal_id = required(args, "--proposal")?;
    let disposition = required(args, "--decision")?;
    validate_autonomy_disposition(&disposition)?;
    latest_member(store, &lead)?;
    let source_task = latest_task(store, &task_id)?;
    let evidence_by_id = latest_evidence(store)?;
    let proposal = evidence_by_id
        .get(&proposal_id)
        .ok_or_else(|| CliError::Usage(format!("proposal evidence not found: {proposal_id}")))?;
    if !autonomy_proposal_source_type(&proposal.source_type) {
        return Err(CliError::Usage(format!(
            "evidence {proposal_id} is {}, not an autonomous proposal",
            proposal.source_type
        )));
    }
    if proposal.task_id.as_deref() != Some(task_id.as_str()) {
        return Err(CliError::Usage(format!(
            "proposal {proposal_id} is not attached to task {task_id}"
        )));
    }
    let mut evidence_ids = vec![proposal_id.clone()];
    evidence_ids.extend(many(args, "--evidence"));
    evidence_ids.sort();
    evidence_ids.dedup();
    for evidence_id in &evidence_ids {
        if !evidence_by_id.contains_key(evidence_id) {
            return Err(CliError::Usage(format!(
                "decision references missing evidence {evidence_id}"
            )));
        }
    }
    let decision = Decision {
        id: value(args, "--id").unwrap_or_else(|| generated_id("decision")),
        task_id: task_id.clone(),
        decision: format!("autonomy {disposition} by {lead}"),
        rationale: required(args, "--rationale")?,
        evidence_ids: evidence_ids.clone(),
        created_at: now_string(),
        decision_kind: Some("verdict".to_string()),
        goal_id: None,
        is_waiver: false,
        follow_up_task_id: None,
    };
    store.append_decision(&decision)?;

    let mut created_goal = None;
    let mut created_task = None;
    let mut goal_design = None;
    let mut assignment_message = None;
    if disposition == "accept" {
        if let Some(goal_id) = value(args, "--create-goal") {
            let goal = Goal {
                id: goal_id,
                title: required(args, "--goal-title")?,
                objective: required(args, "--goal-objective")?,
                owner_agent_id: lead.clone(),
                status: GoalStatus::Active,
                success_criteria: many(args, "--goal-success"),
                priority: value(args, "--priority").unwrap_or_else(|| "p0".into()),
                created_at: now_string(),
                updated_at: now_string(),
                vision_id: value(args, "--goal-vision"),
                goal_design_id: None,
                closed_by_decision_id: None,
            };
            store.append_goal(&goal)?;
            created_goal = Some(goal);
        }
        if let Some(next_task_id) = value(args, "--create-task") {
            let next_goal_id = created_goal
                .as_ref()
                .map(|goal| goal.id.clone())
                .or_else(|| value(args, "--task-goal"))
                .or(source_task.goal_id.clone());
            let assignee = value(args, "--assignee");
            let reviewer = value(args, "--reviewer");
            let task = Task {
                id: next_task_id,
                goal_id: next_goal_id.clone(),
                parent_task_id: Some(source_task.id.clone()),
                title: required(args, "--task-title")?,
                objective: required(args, "--task-objective")?,
                owner_agent_id: lead.clone(),
                assignee_agent_id: assignee.clone(),
                reviewer_agent_id: reviewer,
                status: if assignee.is_some() {
                    TaskStatus::Assigned
                } else {
                    TaskStatus::Planned
                },
                depends_on_task_ids: many(args, "--depends-on"),
                workspace_ref: value(args, "--workspace"),
                branch_ref: value(args, "--branch"),
                pr_ref: value(args, "--pr"),
                owned_paths: many(args, "--owned-path"),
                acceptance_criteria: many(args, "--acceptance"),
                created_at: now_string(),
                updated_at: now_string(),
                phase: value(args, "--task-phase"),
                scope_refs: many(args, "--task-scope-ref"),
                requires_human_approval: has_flag(args, "--task-requires-human-approval"),
                verdict_decision_id: None,
            };
            let design = autonomy_evidence(
                store,
                Some(task.id.clone()),
                "goal_design",
                &format!(
                    "GoalDesign generated from accepted autonomous proposal {proposal_id}."
                ),
                &format!(
                    "# Goal Design\n\nsource_goal: {}\nsource_task: {}\nproposal: {proposal_id}\ndecision: {}\n\nobjective: {}\n",
                    source_task.goal_id.as_deref().unwrap_or("-"),
                    source_task.id,
                    decision.id,
                    task.objective
                ),
            )?;
            store.append_evidence(&design)?;
            store.append_task(&task)?;
            if let Some(assignee_id) = assignee {
                let assignee_member = latest_member(store, &assignee_id)?;
                ensure_member_accepts_delivery(&assignee_member)?;
                let message = Message {
                    id: generated_id("msg"),
                    task_id: Some(task.id.clone()),
                    from_agent_id: lead.clone(),
                    to_agent_id: Some(assignee_id),
                    channel: Some("next-round-task-assignment".into()),
                    kind: MessageKind::Task,
                    delivery_status: MessageDeliveryStatus::Queued,
                    content: format!(
                        "Assigned next-round task {} from proposal {proposal_id}",
                        task.id
                    ),
                    evidence_ids: vec![proposal_id.clone()],
                    created_at: now_string(),
                    delivery: None,
                };
                store.append_message(&message)?;
                assignment_message = Some(message);
            }
            goal_design = Some(design);
            created_task = Some(task);
        }
    }
    append_agent_event(
        store,
        &lead,
        None,
        Some(task_id.as_str()),
        "autonomy_proposal_decided",
        &format!("Lead {disposition} autonomous proposal {proposal_id}"),
        None,
    )?;
    Ok(serde_json::json!({
        "decision": decision,
        "created_goal": created_goal,
        "created_task": created_task,
        "goal_design": goal_design,
        "assignment_message": assignment_message
    }))
}

#[derive(Debug, Clone)]
struct AutonomyTickOptions {
    observer: String,
    lead: String,
    assignee: Option<String>,
    reviewer: Option<String>,
    goal_filter: Option<String>,
    vision_ref: Option<String>,
    vision_summary: Option<String>,
    auto_accept: bool,
    force: bool,
    max_new_goals: usize,
    dry_run: bool,
    start_runtime: bool,
    timeout_ms: u64,
    claim_ttl_ms: u64,
    goal_prefix: String,
    task_prefix: String,
    workspace: Option<String>,
    branch: Option<String>,
    owned_paths: Vec<String>,
    acceptance: Vec<String>,
    goal_success: Vec<String>,
}

#[derive(Debug, Clone)]
struct AutonomyCandidate {
    goal_id: String,
    source_task_id: String,
    evaluation_evidence_id: String,
}

fn autonomy_tick_value(store: &HarnessStore, args: &[String]) -> CliResult<serde_json::Value> {
    let options = parse_autonomy_tick_options(args)?;
    let gateway_before = provider_gateway_tick_value(
        store,
        GatewayOptions {
            dry_run: options.dry_run,
            start_runtime: options.start_runtime,
            timeout_ms: options.timeout_ms,
            claim_ttl_ms: options.claim_ttl_ms,
        },
    )?;
    let scheduled = schedule_autonomy_next_rounds(store, &options)?;
    let gateway_after = if scheduled.iter().any(|item| item.get("decision").is_some()) {
        provider_gateway_tick_value(
            store,
            GatewayOptions {
                dry_run: options.dry_run,
                start_runtime: options.start_runtime,
                timeout_ms: options.timeout_ms,
                claim_ttl_ms: options.claim_ttl_ms,
            },
        )?
    } else {
        serde_json::json!({
            "generated_at": now_string(),
            "agent_count": 0,
            "expired_claims": [],
            "results": [],
            "note": "no accepted generated assignments to deliver"
        })
    };
    Ok(serde_json::json!({
        "generated_at": now_string(),
        "gateway_before": gateway_before,
        "scheduled": scheduled,
        "gateway_after": gateway_after
    }))
}

fn run_autonomy_loop(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let forever = has_flag(args, "--forever");
    let iterations = value(args, "--iterations")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    let interval_ms = value(args, "--interval-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1_000);
    let mut results = Vec::new();
    let mut iteration = 0usize;
    loop {
        iteration += 1;
        let tick = autonomy_tick_value(store, args)?;
        if forever {
            print_json(&serde_json::json!({
                "iteration": iteration,
                "tick": tick
            }))?;
        } else {
            results.push(serde_json::json!({
                "iteration": iteration,
                "tick": tick
            }));
        }
        if !forever && iteration >= iterations {
            break;
        }
        std::thread::sleep(Duration::from_millis(interval_ms));
    }
    if !forever {
        print_json(&serde_json::json!({
            "iterations": iterations,
            "results": results
        }))?;
    }
    Ok(())
}

fn parse_autonomy_tick_options(args: &[String]) -> CliResult<AutonomyTickOptions> {
    let max_new_goals = value(args, "--max-new-goals")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    Ok(AutonomyTickOptions {
        observer: required(args, "--observer")?,
        lead: required(args, "--lead")?,
        assignee: value(args, "--assignee"),
        reviewer: value(args, "--reviewer"),
        goal_filter: value(args, "--goal"),
        vision_ref: value(args, "--vision-ref"),
        vision_summary: value(args, "--vision-summary"),
        auto_accept: has_flag(args, "--auto-accept"),
        force: has_flag(args, "--force"),
        max_new_goals,
        dry_run: has_flag(args, "--dry-run"),
        start_runtime: has_flag(args, "--start-runtime"),
        timeout_ms: value(args, "--timeout-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3_000),
        claim_ttl_ms: value(args, "--claim-ttl-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(300_000),
        goal_prefix: value(args, "--goal-prefix").unwrap_or_else(|| "goal-autonomous-round".into()),
        task_prefix: value(args, "--task-prefix").unwrap_or_else(|| "task-autonomous-round".into()),
        workspace: value(args, "--workspace"),
        branch: value(args, "--branch"),
        owned_paths: many(args, "--owned-path"),
        acceptance: many(args, "--acceptance"),
        goal_success: many(args, "--goal-success"),
    })
}

fn schedule_autonomy_next_rounds(
    store: &HarnessStore,
    options: &AutonomyTickOptions,
) -> CliResult<Vec<serde_json::Value>> {
    if options.vision_ref.is_none() && options.vision_summary.is_none() {
        return Err(CliError::Usage(
            "autonomy tick/loop requires --vision-ref or --vision-summary".into(),
        ));
    }
    latest_member(store, &options.observer)?;
    latest_member(store, &options.lead)?;
    if let Some(assignee) = options.assignee.as_deref() {
        let member = latest_member(store, assignee)?;
        ensure_member_accepts_delivery(&member)?;
    }
    if let Some(reviewer) = options.reviewer.as_deref() {
        latest_member(store, reviewer)?;
    }
    let candidates = autonomy_next_round_candidates(store, options)?;
    let mut scheduled = Vec::new();
    for candidate in candidates.into_iter().take(options.max_new_goals) {
        let close_result = close_goal_for_next_round(store, options, &candidate)?;
        let planned =
            autonomy_plan_next_value(store, &autonomy_plan_next_args(&candidate, options))?;
        let mut row = serde_json::json!({
            "goal_id": candidate.goal_id,
            "source_task_id": candidate.source_task_id,
            "evaluation_evidence_id": candidate.evaluation_evidence_id,
            "goal_close": close_result,
            "plan": planned.get("plan").cloned(),
            "proposal": planned.get("proposal").cloned(),
            "message": planned.get("message").cloned()
        });
        if options.auto_accept {
            let proposal_id = planned
                .get("proposal")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .ok_or_else(|| CliError::Usage("planned proposal missing id".into()))?;
            let plan_id = planned
                .get("plan")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .ok_or_else(|| CliError::Usage("planned next_round_plan missing id".into()))?;
            let decision = accept_scheduled_next_round(
                store,
                options,
                row.get("goal_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("-"),
                row.get("source_task_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("-"),
                proposal_id,
                plan_id,
            )?;
            row.as_object_mut()
                .expect("row is object")
                .insert("decision".into(), decision);
        }
        scheduled.push(row);
    }
    Ok(scheduled)
}

fn autonomy_plan_next_args(
    candidate: &AutonomyCandidate,
    options: &AutonomyTickOptions,
) -> Vec<String> {
    let mut args = vec![
        "--goal".into(),
        candidate.goal_id.clone(),
        "--task".into(),
        candidate.source_task_id.clone(),
        "--observer".into(),
        options.observer.clone(),
        "--lead".into(),
        options.lead.clone(),
        "--proposal-summary".into(),
        format!(
            "Scheduler compared completed goal {} with vision and proposed the next goal from source task {}.",
            candidate.goal_id, candidate.source_task_id
        ),
    ];
    if let Some(vision_ref) = &options.vision_ref {
        push_arg(&mut args, "--vision-ref", vision_ref);
    }
    if let Some(vision_summary) = &options.vision_summary {
        push_arg(&mut args, "--vision-summary", vision_summary);
    }
    args
}

fn autonomy_next_round_candidates(
    store: &HarnessStore,
    options: &AutonomyTickOptions,
) -> CliResult<Vec<AutonomyCandidate>> {
    let goals = latest_goals(store)?;
    let evidence = latest_evidence(store)?;
    let mut candidates = Vec::new();
    for goal in goals.values() {
        if goal.status != GoalStatus::Active {
            continue;
        }
        if options
            .goal_filter
            .as_ref()
            .is_some_and(|goal_id| goal_id != &goal.id)
        {
            continue;
        }
        let status = goal_learning_status(store, &goal.id)?;
        if !goal_task_graph_complete(&status.task_ids, store)? {
            continue;
        }
        if !status.warnings(true).is_empty() {
            continue;
        }
        if !options.force && goal_has_next_round_plan(&status.task_ids, &evidence) {
            continue;
        }
        let Some((evaluation_evidence_id, source_task_id)) =
            latest_goal_evaluation_task(&status.goal_evaluation)
        else {
            continue;
        };
        candidates.push(AutonomyCandidate {
            goal_id: goal.id.clone(),
            source_task_id,
            evaluation_evidence_id,
        });
    }
    Ok(candidates)
}

fn goal_task_graph_complete(task_ids: &[String], store: &HarnessStore) -> CliResult<bool> {
    if task_ids.is_empty() {
        return Ok(false);
    }
    let tasks = latest_tasks(store)?;
    Ok(task_ids.iter().all(|task_id| {
        tasks
            .get(task_id)
            .is_some_and(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Archived))
    }))
}

fn goal_has_next_round_plan(
    task_ids: &[String],
    evidence_by_id: &BTreeMap<String, Evidence>,
) -> bool {
    evidence_by_id.values().any(|item| {
        item.source_type == "next_round_plan"
            && item
                .task_id
                .as_ref()
                .is_some_and(|task_id| task_ids.contains(task_id))
    })
}

fn latest_goal_evaluation_task(evidence: &[Evidence]) -> Option<(String, String)> {
    evidence
        .iter()
        .filter_map(|item| {
            Some((
                parse_unix_ms(&item.created_at).unwrap_or_default(),
                item.id.clone(),
                item.task_id.clone()?,
            ))
        })
        .max_by_key(|(created_at, _, _)| *created_at)
        .map(|(_, evidence_id, task_id)| (evidence_id, task_id))
}

fn close_goal_for_next_round(
    store: &HarnessStore,
    options: &AutonomyTickOptions,
    candidate: &AutonomyCandidate,
) -> CliResult<serde_json::Value> {
    let mut goal = latest_goals(store)?
        .remove(&candidate.goal_id)
        .ok_or_else(|| CliError::Usage(format!("goal not found: {}", candidate.goal_id)))?;
    if goal.status != GoalStatus::Complete {
        goal.status = GoalStatus::Complete;
        goal.updated_at = now_string();
        store.append_goal(&goal)?;
    }
    let decision = Decision {
        id: generated_id("decision"),
        task_id: candidate.source_task_id.clone(),
        decision: format!("autonomy goal_complete by {}", options.lead),
        rationale: format!(
            "GoalClose gate passed for {}; task graph is complete and GoalEvaluation {} is present. Runner will compare this goal with vision before proposing the next goal.",
            candidate.goal_id, candidate.evaluation_evidence_id
        ),
        evidence_ids: vec![candidate.evaluation_evidence_id.clone()],
        created_at: now_string(),
        decision_kind: Some("closeout".to_string()),
        goal_id: Some(candidate.goal_id.clone()),
        is_waiver: false,
        follow_up_task_id: None,
    };
    store.append_decision(&decision)?;
    append_agent_event(
        store,
        &options.lead,
        None,
        Some(candidate.source_task_id.as_str()),
        "autonomy_goal_closed",
        &format!("Goal {} marked complete by runner", candidate.goal_id),
        None,
    )?;
    Ok(serde_json::json!({
        "goal": goal,
        "decision": decision
    }))
}

fn accept_scheduled_next_round(
    store: &HarnessStore,
    options: &AutonomyTickOptions,
    source_goal_id: &str,
    source_task_id: &str,
    proposal_id: &str,
    plan_id: &str,
) -> CliResult<serde_json::Value> {
    let next_goal_id = generated_id(&options.goal_prefix);
    let next_task_id = generated_id(&options.task_prefix);
    let mut args = vec![
        "--task".into(),
        source_task_id.into(),
        "--lead".into(),
        options.lead.clone(),
        "--proposal".into(),
        proposal_id.into(),
        "--decision".into(),
        "accept".into(),
        "--rationale".into(),
        format!(
            "Autonomy runner accepted scheduler proposal {proposal_id} from goal {source_goal_id}."
        ),
        "--evidence".into(),
        plan_id.into(),
        "--create-goal".into(),
        next_goal_id.clone(),
        "--goal-title".into(),
        format!("Next autonomous round from {source_goal_id}"),
        "--goal-objective".into(),
        format!(
            "Continue from evaluated goal {source_goal_id} and source task {source_task_id} through the autonomous runner."
        ),
        "--create-task".into(),
        next_task_id.clone(),
        "--task-title".into(),
        format!("Follow-up: continue from {source_task_id}"),
        "--task-objective".into(),
        format!(
            "Execute the next autonomous runner task generated from proposal {proposal_id}."
        ),
    ];
    let goal_success = if options.goal_success.is_empty() {
        vec!["Generated next-round task is assigned and visible in Dashboard state.".into()]
    } else {
        options.goal_success.clone()
    };
    push_repeated_args(&mut args, "--goal-success", &goal_success);
    if let Some(assignee) = &options.assignee {
        push_arg(&mut args, "--assignee", assignee);
    }
    if let Some(reviewer) = &options.reviewer {
        push_arg(&mut args, "--reviewer", reviewer);
    }
    if let Some(workspace) = &options.workspace {
        push_arg(&mut args, "--workspace", workspace);
    }
    if let Some(branch) = &options.branch {
        push_arg(&mut args, "--branch", branch);
    }
    push_repeated_args(&mut args, "--owned-path", &options.owned_paths);
    let acceptance = if options.acceptance.is_empty() {
        vec![
            "Standing runner assignment is delivered or records terminal delivery evidence.".into(),
        ]
    } else {
        options.acceptance.clone()
    };
    push_repeated_args(&mut args, "--acceptance", &acceptance);
    autonomy_decide_value(store, &args)
}

fn push_arg(args: &mut Vec<String>, name: &str, value: &str) {
    args.push(name.into());
    args.push(value.into());
}

fn push_repeated_args(args: &mut Vec<String>, name: &str, values: &[String]) {
    for value in values {
        push_arg(args, name, value);
    }
}

fn autonomy_evidence(
    store: &HarnessStore,
    task_id: Option<String>,
    source_type: &str,
    summary: &str,
    body: &str,
) -> CliResult<Evidence> {
    let evidence_id = generated_id("evidence");
    let source_ref = write_autonomy_artifact(store, &evidence_id, body)?;
    Ok(Evidence {
        id: evidence_id,
        task_id,
        source_type: source_type.into(),
        source_ref,
        summary: summary.into(),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    })
}

fn write_autonomy_artifact(
    store: &HarnessStore,
    evidence_id: &str,
    body: &str,
) -> CliResult<String> {
    let dir = store.root().join("autonomy");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{evidence_id}.md"));
    fs::write(&path, body)?;
    Ok(path.display().to_string())
}

fn autonomy_observation_summary(store: &HarnessStore, goal_id: &str) -> CliResult<String> {
    let status = goal_learning_status(store, goal_id)?;
    let warnings = status.warnings(true);
    if warnings.is_empty() {
        Ok(format!(
            "Observer found goal {goal_id} has complete learning evidence and is ready for a follow-up proposal."
        ))
    } else {
        Ok(format!(
            "Observer found goal {goal_id} warnings: {}",
            warnings.join("; ")
        ))
    }
}

fn validate_autonomy_proposal_kind(kind: &str) -> CliResult<()> {
    if autonomy_proposal_source_type(kind) {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "unknown autonomy proposal kind: {kind}"
        )))
    }
}

fn autonomy_proposal_source_type(source_type: &str) -> bool {
    matches!(
        source_type,
        "goal_proposal" | "graph_change_proposal" | "blocker" | "follow_up"
    )
}

fn validate_autonomy_disposition(disposition: &str) -> CliResult<()> {
    match disposition {
        "accept" | "reject" | "defer" | "request_evidence" => Ok(()),
        other => Err(CliError::Usage(format!(
            "unknown autonomy decision: {other}"
        ))),
    }
}

fn git_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "git worktree-create|attach|status|changed-paths")?;
    match args[0].as_str() {
        "worktree-create" => {
            let task_id = required(args, "--task")?;
            let repo = required(args, "--repo")?;
            let path = required(args, "--path")?;
            let branch = required(args, "--branch")?;
            let base = value(args, "--base").unwrap_or_else(|| "HEAD".into());
            if !has_flag(args, "--no-create") {
                let status = Command::new("git")
                    .args(["-C", &repo, "worktree", "add", "-b", &branch, &path, &base])
                    .status()?;
                if !status.success() {
                    return Err(CliError::Usage(format!(
                        "git worktree add failed for task {task_id}"
                    )));
                }
            }
            let mut task = latest_task(store, &task_id)?;
            task.workspace_ref = Some(path.clone());
            task.branch_ref = Some(branch.clone());
            task.updated_at = now_string();
            store.append_task(&task)?;
            let evidence = Evidence {
                id: generated_id("evidence"),
                task_id: Some(task_id),
                source_type: "git_worktree".into(),
                source_ref: path,
                summary: format!("Attached git worktree branch {branch} from {base}"),
                created_at: now_string(),
                evidence_kind: None,
                goal_id: None,
            };
            store.append_evidence(&evidence)?;
            print_json(&serde_json::json!({ "task": task, "evidence": evidence }))?;
        }
        "attach" => {
            let task_id = required(args, "--task")?;
            let mut task = latest_task(store, &task_id)?;
            task.workspace_ref = Some(required(args, "--workspace")?);
            task.branch_ref = Some(required(args, "--branch")?);
            task.pr_ref = value(args, "--pr").or(task.pr_ref);
            let owned_paths = many(args, "--owned-path");
            if !owned_paths.is_empty() {
                task.owned_paths = owned_paths;
            }
            task.updated_at = now_string();
            store.append_task(&task)?;
            print_json(&task)?;
        }
        "status" => {
            let task = if let Some(task_id) = value(args, "--task") {
                Some(latest_task(store, &task_id)?)
            } else {
                None
            };
            let worktree = value(args, "--worktree")
                .or_else(|| task.as_ref().and_then(|task| task.workspace_ref.clone()))
                .ok_or_else(|| {
                    CliError::Usage("--worktree or --task with workspace_ref is required".into())
                })?;
            let base = value(args, "--base").unwrap_or_else(|| "HEAD".into());
            print_json(&git_status_snapshot(
                &worktree,
                &base,
                task.as_ref()
                    .map(|task| task.owned_paths.as_slice())
                    .unwrap_or(&[]),
            )?)?;
        }
        "changed-paths" => {
            let worktree = required(args, "--worktree")?;
            let base = value(args, "--base").unwrap_or_else(|| "HEAD".into());
            print_json(&git_changed_paths(&worktree, &base)?)?;
        }
        other => return Err(CliError::Usage(format!("unknown git command: {other}"))),
    }
    Ok(())
}

fn review_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "review create|list|gate")?;
    match args[0].as_str() {
        "create" => review_create(store, &args[1..]),
        "list" => {
            print_json(&store.reviews()?)?;
            Ok(())
        }
        "gate" => review_gate(store, args),
        other => Err(CliError::Usage(format!("unknown review command: {other}"))),
    }
}

fn review_create(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let task_id = value(args, "--task");
    let goal_id = value(args, "--goal");
    if task_id.is_none() && goal_id.is_none() {
        return Err(CliError::Usage(
            "review create requires --task or --goal".into(),
        ));
    }
    let review = Review {
        id: value(args, "--id").unwrap_or_else(|| generated_id("review")),
        task_id,
        goal_id,
        reviewer_agent_id: required(args, "--reviewer")?,
        review_kind: required(args, "--kind")?,
        verdict: ReviewVerdict::from(required(args, "--verdict")?),
        summary: required(args, "--summary")?,
        blockers: many(args, "--blocker"),
        residual_risk: value(args, "--residual-risk"),
        missing_validation: many(args, "--missing-validation"),
        evidence_ids: many(args, "--evidence"),
        created_at: now_string(),
    };
    store.append_review(&review)?;
    print_json(&review)?;
    Ok(())
}

fn gap_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "gap create|list|export")?;
    match args[0].as_str() {
        "create" => gap_create(store, &args[1..]),
        "list" => {
            print_json(&latest_gaps_in_append_order(store)?)?;
            Ok(())
        }
        "export" => gap_export(store),
        other => Err(CliError::Usage(format!("unknown gap command: {other}"))),
    }
}

fn gap_create(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let now = now_string();
    let gap = Gap {
        id: value(args, "--id").unwrap_or_else(|| generated_id("gap")),
        goal_id: value(args, "--goal"),
        task_id: value(args, "--task"),
        category: required(args, "--category")?,
        severity: parse_gap_severity(&required(args, "--severity")?)?,
        status: match value(args, "--status") {
            Some(raw) => parse_gap_status(&raw)?,
            None => GapStatus::Open,
        },
        summary: required(args, "--summary")?,
        evidence_ids: many(args, "--evidence"),
        next_step: value(args, "--next-step"),
        owner_agent_id: value(args, "--owner"),
        repro_ref: value(args, "--repro"),
        closing_test_ref: value(args, "--closing-test"),
        created_at: now.clone(),
        updated_at: now,
    };
    store.append_gap(&gap)?;
    print_json(&gap)?;
    Ok(())
}

/// Print a markdown projection of the Gap ledger (the generated successor to the
/// flat-file gap inbox). Open/in-progress gaps first, grouped by severity.
fn gap_export(store: &HarnessStore) -> CliResult<()> {
    let gaps = latest_gaps_in_append_order(store)?;
    println!("# Gap ledger\n");
    if gaps.is_empty() {
        println!("_No gaps recorded._");
        return Ok(());
    }
    for severity in [GapSeverity::P0, GapSeverity::P1, GapSeverity::P2] {
        let rows: Vec<&Gap> = gaps
            .iter()
            .filter(|gap| gap.severity == severity)
            .collect();
        if rows.is_empty() {
            continue;
        }
        println!("## {}\n", gap_severity_label(&severity).to_uppercase());
        for gap in rows {
            let checkbox = if matches!(gap.status, GapStatus::Fixed | GapStatus::Wontfix) {
                "x"
            } else {
                " "
            };
            println!(
                "- [{}] {} | {} | {} | {} | evidence={} | next={}",
                checkbox,
                gap.id,
                gap.category,
                gap_status_label(&gap.status),
                gap.summary,
                if gap.evidence_ids.is_empty() {
                    "-".to_string()
                } else {
                    gap.evidence_ids.join(",")
                },
                gap.next_step.as_deref().unwrap_or("-"),
            );
        }
        println!();
    }
    Ok(())
}

fn parse_gap_severity(value: &str) -> CliResult<GapSeverity> {
    match value {
        "p0" => Ok(GapSeverity::P0),
        "p1" => Ok(GapSeverity::P1),
        "p2" => Ok(GapSeverity::P2),
        other => Err(CliError::Usage(format!(
            "unknown gap severity: {other} (expected p0|p1|p2)"
        ))),
    }
}

fn parse_gap_status(value: &str) -> CliResult<GapStatus> {
    match value {
        "open" => Ok(GapStatus::Open),
        "in_progress" => Ok(GapStatus::InProgress),
        "fixed" => Ok(GapStatus::Fixed),
        "blocked" => Ok(GapStatus::Blocked),
        "deferred" => Ok(GapStatus::Deferred),
        "wontfix" => Ok(GapStatus::Wontfix),
        other => Err(CliError::Usage(format!(
            "unknown gap status: {other} (expected open|in_progress|fixed|blocked|deferred|wontfix)"
        ))),
    }
}

fn gap_severity_label(severity: &GapSeverity) -> &'static str {
    match severity {
        GapSeverity::P0 => "p0",
        GapSeverity::P1 => "p1",
        GapSeverity::P2 => "p2",
    }
}

fn gap_status_label(status: &GapStatus) -> &'static str {
    match status {
        GapStatus::Open => "open",
        GapStatus::InProgress => "in_progress",
        GapStatus::Fixed => "fixed",
        GapStatus::Blocked => "blocked",
        GapStatus::Deferred => "deferred",
        GapStatus::Wontfix => "wontfix",
    }
}

fn goal_design_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "goal-design create|list")?;
    match args[0].as_str() {
        "create" => goal_design_create(store, &args[1..]),
        "list" => {
            print_json(&latest_goal_designs_in_append_order(store)?)?;
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown goal-design command: {other}"
        ))),
    }
}

fn goal_design_create(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let design = GoalDesign {
        id: value(args, "--id").unwrap_or_else(|| generated_id("goal-design")),
        goal_id: required(args, "--goal")?,
        scenario_summary: required(args, "--scenario")?,
        non_goals: many(args, "--non-goal"),
        risk_and_permission_boundaries: required(args, "--risk-boundaries")?,
        required_infra: many(args, "--required-infra"),
        agent_team: value(args, "--team"),
        task_graph: many(args, "--task"),
        evidence_plan: many(args, "--evidence-plan"),
        acceptance_gates: many(args, "--acceptance-gate"),
        created_at: now_string(),
    };
    store.append_goal_design(&design)?;
    print_json(&design)?;
    Ok(())
}

fn goal_evaluation_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "goal-evaluation create|list")?;
    match args[0].as_str() {
        "create" => goal_evaluation_create(store, &args[1..]),
        "list" => {
            print_json(&latest_goal_evaluations_in_append_order(store)?)?;
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown goal-evaluation command: {other}"
        ))),
    }
}

fn goal_evaluation_create(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let evaluation = GoalEvaluation {
        id: value(args, "--id").unwrap_or_else(|| generated_id("goal-evaluation")),
        goal_id: required(args, "--goal")?,
        evaluator_agent_id: required(args, "--evaluator")?,
        outcome: EvaluationOutcome::from(required(args, "--outcome")?),
        what_worked: required(args, "--what-worked")?,
        what_failed: required(args, "--what-failed")?,
        missing_infra: many(args, "--missing-infra"),
        missing_evidence: many(args, "--missing-evidence"),
        team_design_feedback: value(args, "--team-feedback").unwrap_or_default(),
        task_graph_feedback: value(args, "--task-graph-feedback").unwrap_or_default(),
        dashboard_feedback: value(args, "--dashboard-feedback").unwrap_or_default(),
        reusable_patterns: many(args, "--pattern"),
        anti_patterns: many(args, "--anti-pattern"),
        follow_up_task_ids: many(args, "--follow-up-task"),
        proposed_goal_ids: many(args, "--proposed-goal"),
        created_at: now_string(),
    };
    store.append_goal_evaluation(&evaluation)?;
    print_json(&evaluation)?;
    Ok(())
}

fn goal_case_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "goal-case create|list")?;
    match args[0].as_str() {
        "create" => goal_case_create(store, &args[1..]),
        "list" => {
            print_json(&latest_goal_cases_in_append_order(store)?)?;
            Ok(())
        }
        other => Err(CliError::Usage(format!("unknown goal-case command: {other}"))),
    }
}

fn goal_case_create(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let case = GoalCase {
        case_id: value(args, "--id").unwrap_or_else(|| generated_id("goal-case")),
        source_goal_id: required(args, "--goal")?,
        scenario_type: required(args, "--scenario-type")?,
        project_adapter: value(args, "--adapter"),
        goal_design_ref: value(args, "--design-ref"),
        evaluation_ref: value(args, "--evaluation-ref"),
        reusable_patterns: many(args, "--pattern"),
        anti_patterns: many(args, "--anti-pattern"),
        follow_up_refs: many(args, "--follow-up"),
        tags: many(args, "--tag"),
        created_at: now_string(),
    };
    store.append_goal_case(&case)?;
    print_json(&case)?;
    Ok(())
}

fn vision_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "vision create|list")?;
    match args[0].as_str() {
        "create" => vision_create(store, &args[1..]),
        "list" => {
            print_json(&latest_visions_in_append_order(store)?)?;
            Ok(())
        }
        other => Err(CliError::Usage(format!("unknown vision command: {other}"))),
    }
}

fn vision_create(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let vision = Vision {
        id: value(args, "--id").unwrap_or_else(|| generated_id("vision")),
        summary: required(args, "--summary")?,
        source_refs: many(args, "--source-ref"),
        created_at: now_string(),
    };
    store.append_vision(&vision)?;
    print_json(&vision)?;
    Ok(())
}

fn dashboard_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "dashboard snapshot")?;
    match args[0].as_str() {
        "snapshot" => print_json(&dashboard_snapshot(store)?)?,
        other => {
            return Err(CliError::Usage(format!(
                "unknown dashboard command: {other}"
            )))
        }
    }
    Ok(())
}

fn board_command(store: &HarnessStore) -> CliResult<()> {
    let tasks = latest_tasks(store)?;
    let messages = latest_messages_in_append_order(store)?;
    let evidence = store.evidence()?;
    let decisions = store.decisions()?;
    let sessions = latest_provider_sessions_in_append_order(store)?;
    let columns = [
        TaskStatus::Planned,
        TaskStatus::Assigned,
        TaskStatus::Running,
        TaskStatus::Blocked,
        TaskStatus::Review,
        TaskStatus::Done,
        TaskStatus::Archived,
    ];

    for column in columns {
        println!("## {}", status_label(&column));
        for task in tasks.values().filter(|task| task.status == column) {
            let message_count = messages
                .iter()
                .filter(|message| message.task_id.as_ref() == Some(&task.id))
                .count();
            let evidence_count = evidence
                .iter()
                .filter(|item| item.task_id.as_ref() == Some(&task.id))
                .count();
            let decision_count = decisions
                .iter()
                .filter(|item| item.task_id == task.id)
                .count();
            let session_count = sessions
                .iter()
                .filter(|item| item.task_id.as_ref() == Some(&task.id))
                .count();
            println!(
                "- {} | owner={} assignee={} reviewer={} workspace={} branch={} pr={} evidence={} messages={} sessions={} decisions={} paths={}",
                task.id,
                task.owner_agent_id,
                task.assignee_agent_id.as_deref().unwrap_or("-"),
                task.reviewer_agent_id.as_deref().unwrap_or("-"),
                task.workspace_ref.as_deref().unwrap_or("-"),
                task.branch_ref.as_deref().unwrap_or("-"),
                task.pr_ref.as_deref().unwrap_or("-"),
                evidence_count,
                message_count,
                session_count,
                decision_count,
                task.owned_paths.join(",")
            );
            println!("  {}", task.title);
        }
    }
    Ok(())
}

fn codex_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "codex run|review")?;
    match args[0].as_str() {
        "run" => codex_run(store, &args[1..]),
        "review" => codex_review(store, &args[1..]),
        other => Err(CliError::Usage(format!("unknown codex command: {other}"))),
    }
}

fn serve_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let addr = value(args, "--addr").unwrap_or_else(|| "127.0.0.1:8787".into());
    let once = has_flag(args, "--once");
    let listener = TcpListener::bind(&addr)?;
    println!("serving harness API on http://{addr}");
    for stream in listener.incoming() {
        handle_http_connection(store, stream?)?;
        if once {
            break;
        }
    }
    Ok(())
}

fn handle_http_connection(store: &HarnessStore, mut stream: TcpStream) -> CliResult<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path = parts.next().unwrap_or_default().to_string();
    let path_only = path.split('?').next().unwrap_or_default().to_string();
    let mut content_length = 0usize;
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

    if method == "GET" {
        match path_only.as_str() {
            "/health" | "/v1/health" => write_http_json(
                &mut stream,
                "200 OK",
                &serde_json::json!({"status": "ok", "generated_at": now_string()}),
            )?,
            "/v1/snapshot" | "/v1/dashboard/snapshot" => {
                write_http_json(&mut stream, "200 OK", &dashboard_snapshot(store)?)?
            }
            "/v1/events" => write_http_json(&mut stream, "200 OK", &store.events()?)?,
            _ => write_http_json(
                &mut stream,
                "404 Not Found",
                &serde_json::json!({"error": "not_found", "path": path_only}),
            )?,
        }
        return Ok(());
    }

    let body_json = if body.is_empty() {
        serde_json::json!({})
    } else {
        match serde_json::from_slice::<serde_json::Value>(&body) {
            Ok(value) => value,
            Err(error) => {
                write_http_json(
                    &mut stream,
                    "400 Bad Request",
                    &serde_json::json!({"ok": false, "error": format!("invalid JSON body: {error}")}),
                )?;
                return Ok(());
            }
        }
    };
    match handle_http_action(store, &path_only, &body_json) {
        Ok(response) => write_http_json(
            &mut stream,
            "200 OK",
            &serde_json::json!({"ok": true, "result": response, "snapshot": dashboard_snapshot(store)?}),
        )?,
        Err(error) => write_http_json(
            &mut stream,
            "400 Bad Request",
            &serde_json::json!({"ok": false, "error": error.to_string()}),
        )?,
    }
    Ok(())
}

fn handle_http_action(
    store: &HarnessStore,
    path: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    if path == "/v1/messages" {
        return create_message_value(store, body);
    }
    if path == "/v1/gateway/tick" {
        return provider_gateway_tick_value(
            store,
            GatewayOptions {
                dry_run: json_bool(body, "dry_run").unwrap_or(false),
                start_runtime: json_bool(body, "start_runtime").unwrap_or(false),
                timeout_ms: json_u64(body, "timeout_ms").unwrap_or(3_000),
                claim_ttl_ms: json_u64(body, "claim_ttl_ms").unwrap_or(300_000),
            },
        );
    }
    if let Some(agent_id) = path
        .strip_prefix("/v1/agents/")
        .and_then(|rest| rest.strip_suffix("/deliver"))
    {
        return deliver_agent_messages_value(
            store,
            DeliveryOptions {
                agent_id: agent_id.into(),
                message_filter: json_string(body, "message_id"),
                dry_run: json_bool(body, "dry_run").unwrap_or(false),
                start_runtime: json_bool(body, "start_runtime").unwrap_or(false),
                timeout_ms: json_u64(body, "timeout_ms").unwrap_or(3_000),
            },
        );
    }
    if let Some(agent_id) = path
        .strip_prefix("/v1/agents/")
        .and_then(|rest| rest.strip_suffix("/retry-delivery"))
    {
        return retry_delivery_value(
            store,
            agent_id,
            &required_json_string(body, "message_id")?,
            json_string(body, "session_id").as_deref(),
            json_string(body, "reason")
                .as_deref()
                .unwrap_or("dashboard requested retry"),
            json_bool(body, "force").unwrap_or(false),
        );
    }
    if let Some(agent_id) = path
        .strip_prefix("/v1/agents/")
        .and_then(|rest| rest.strip_suffix("/reconcile-session"))
    {
        return reconcile_provider_session_value(
            store,
            agent_id,
            &required_json_string(body, "session_id")?,
            parse_provider_session_status(
                json_string(body, "status").as_deref().unwrap_or("failed"),
            )?,
            parse_terminal_source(
                json_string(body, "terminal_source")
                    .as_deref()
                    .unwrap_or("failed"),
            )?,
            json_string(body, "reason")
                .as_deref()
                .unwrap_or("dashboard reconciliation"),
        );
    }
    if let Some(agent_id) = path
        .strip_prefix("/v1/agents/")
        .and_then(|rest| rest.strip_suffix("/close"))
    {
        return Ok(serde_json::to_value(close_agent_member_value(
            store, agent_id,
        )?)?);
    }
    if let Some(task_id) = path
        .strip_prefix("/v1/tasks/")
        .and_then(|rest| rest.strip_suffix("/request-review"))
    {
        return request_task_review_value(store, task_id, body);
    }
    Err(CliError::Usage(format!("unknown action path: {path}")))
}

fn create_message_value(
    store: &HarnessStore,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let to_agent_id = json_string(body, "to_agent_id").or_else(|| json_string(body, "to"));
    let target = to_agent_id
        .as_deref()
        .map(|agent_id| latest_member(store, agent_id))
        .transpose()?;
    if let Some(member) = target.as_ref() {
        ensure_member_accepts_delivery(member)?;
    }
    let message = Message {
        id: json_string(body, "id").unwrap_or_else(|| generated_id("msg")),
        task_id: json_string(body, "task_id").or_else(|| json_string(body, "task")),
        from_agent_id: required_json_string(body, "from_agent_id")
            .or_else(|_| required_json_string(body, "from"))?,
        to_agent_id,
        channel: json_string(body, "channel"),
        kind: parse_message_kind(json_string(body, "kind").as_deref().unwrap_or("message"))?,
        delivery_status: MessageDeliveryStatus::Queued,
        content: required_json_string(body, "content")?,
        evidence_ids: json_string_array(body, "evidence_ids"),
        created_at: now_string(),
        delivery: None,
    };
    store.append_message(&message)?;
    if let Some(member) = target.as_ref() {
        append_agent_event(
            store,
            &member.id,
            member.provider_runtime_id.as_deref(),
            message.task_id.as_deref(),
            "message_queued",
            "Message queued for Agent Member",
            None,
        )?;
    }
    Ok(serde_json::to_value(message)?)
}

fn request_task_review_value(
    store: &HarnessStore,
    task_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let mut task = latest_task(store, task_id)?;
    let reviewer = json_string(body, "to_agent_id")
        .or_else(|| json_string(body, "reviewer_agent_id"))
        .or_else(|| task.reviewer_agent_id.clone())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "task {task_id} has no reviewer; provide to_agent_id"
            ))
        })?;
    let reviewer_member = latest_member(store, &reviewer)?;
    ensure_member_accepts_delivery(&reviewer_member)?;
    let from_agent_id =
        json_string(body, "from_agent_id").unwrap_or_else(|| task.owner_agent_id.clone());
    let message = Message {
        id: generated_id("msg"),
        task_id: Some(task.id.clone()),
        from_agent_id,
        to_agent_id: Some(reviewer.clone()),
        channel: Some("review-request".into()),
        kind: MessageKind::Message,
        delivery_status: MessageDeliveryStatus::Queued,
        content: json_string(body, "content")
            .unwrap_or_else(|| format!("Please review task {}", task.id)),
        evidence_ids: json_string_array(body, "evidence_ids"),
        created_at: now_string(),
        delivery: None,
    };
    store.append_message(&message)?;
    task.status = TaskStatus::Review;
    task.updated_at = now_string();
    store.append_task(&task)?;
    append_agent_event(
        store,
        &reviewer,
        reviewer_member.provider_runtime_id.as_deref(),
        Some(task_id),
        "review_requested",
        "Task review requested",
        None,
    )?;
    Ok(serde_json::json!({
        "task": task,
        "message": message
    }))
}

fn json_string(body: &serde_json::Value, key: &str) -> Option<String> {
    body.get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn required_json_string(body: &serde_json::Value, key: &str) -> CliResult<String> {
    json_string(body, key).ok_or_else(|| CliError::Usage(format!("missing JSON field: {key}")))
}

fn json_bool(body: &serde_json::Value, key: &str) -> Option<bool> {
    body.get(key).and_then(|value| value.as_bool())
}

fn json_u64(body: &serde_json::Value, key: &str) -> Option<u64> {
    body.get(key).and_then(|value| value.as_u64())
}

fn json_string_array(body: &serde_json::Value, key: &str) -> Vec<String> {
    body.get(key)
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn write_http_json<T: serde::Serialize>(
    stream: &mut TcpStream,
    status: &str,
    value: &T,
) -> CliResult<()> {
    let body = serde_json::to_vec_pretty(value).expect("serialize http json");
    write_http_response(stream, status, "application/json", &body)
}

fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> CliResult<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

fn codex_run(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let task_id = required(args, "--task")?;
    let agent_id = required(args, "--agent")?;
    let worktree = required(args, "--worktree")?;
    let prompt = required(args, "--prompt")?;
    let session_id = value(args, "--id").unwrap_or_else(|| generated_id("session"));
    let session_dir = store.root().join("provider-sessions").join(&session_id);
    fs::create_dir_all(&session_dir)?;

    let jsonl_ref = session_dir.join("events.jsonl");
    let last_message_ref = session_dir.join("last-message.md");
    let stdout_ref = session_dir.join("stdout.log");
    let started_at = now_string();
    let sandbox = value(args, "--sandbox").unwrap_or_else(|| "workspace-write".into());

    let mut command_args = vec![
        "exec".to_string(),
        "-C".to_string(),
        worktree.clone(),
        "--sandbox".to_string(),
        sandbox,
        "--json".to_string(),
        "--output-last-message".to_string(),
        last_message_ref.display().to_string(),
    ];
    if let Some(model) = value(args, "--model") {
        command_args.push("--model".into());
        command_args.push(model);
    }
    command_args.push(prompt.clone());

    let output = Command::new("codex").args(&command_args).output()?;
    fs::write(&jsonl_ref, &output.stdout)?;
    fs::write(&stdout_ref, &output.stderr)?;

    let exit_code = output.status.code();
    let status = if output.status.success() {
        ProviderSessionStatus::Succeeded
    } else {
        ProviderSessionStatus::Failed
    };
    let evidence_id = generated_id("evidence");
    let session_ref = session_dir.display().to_string();
    let evidence = Evidence {
        id: evidence_id.clone(),
        task_id: Some(task_id.clone()),
        source_type: "codex_provider_session".into(),
        source_ref: session_ref.clone(),
        summary: format!("Codex provider session {session_id} for task {task_id}"),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;

    let session = ProviderSession {
        id: session_id.clone(),
        provider: "codex".into(),
        agent_member_id: agent_id.clone(),
        task_id: Some(task_id.clone()),
        workspace_ref: Some(worktree),
        provider_thread_id: None,
        provider_turn_id: None,
        terminal_source: Some(if exit_code == Some(0) {
            MessageTerminalSource::Unknown
        } else {
            MessageTerminalSource::Failed
        }),
        status: status.clone(),
        command: "codex".into(),
        args: command_args,
        prompt_ref: Some(prompt),
        prompt_summary: None,
        provider_session_ref: None,
        stdout_ref: Some(stdout_ref.display().to_string()),
        jsonl_ref: Some(jsonl_ref.display().to_string()),
        transcript_ref: None,
        last_message_ref: Some(last_message_ref.display().to_string()),
        exit_code,
        started_at,
        ended_at: Some(now_string()),
        evidence_ids: vec![evidence_id.clone()],
    };
    store.append_provider_session(&session)?;

    let report = Message {
        id: generated_id("msg"),
        task_id: Some(task_id),
        from_agent_id: agent_id,
        to_agent_id: None,
        channel: Some("provider-session".into()),
        kind: MessageKind::Report,
        delivery_status: MessageDeliveryStatus::Delivered,
        content: format!(
            "Codex provider session {session_id} finished with exit_code={exit_code:?}"
        ),
        evidence_ids: vec![evidence_id.clone()],
        created_at: now_string(),
        delivery: Some(MessageDelivery {
            provider_session_id: Some(session_id),
            provider_request_id: None,
            provider_thread_id: None,
            provider_turn_id: None,
            terminal_source: Some(MessageTerminalSource::Unknown),
            delivered_at: Some(now_string()),
            last_error: None,
        }),
    };
    store.append_message(&report)?;
    print_json(&session)?;
    Ok(())
}

fn codex_review(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let task_id = required(args, "--task")?;
    let agent_id = required(args, "--agent")?;
    let worktree = required(args, "--worktree")?;
    let base = value(args, "--base").unwrap_or_else(|| "master".into());
    let prompt = value(args, "--prompt");
    let session_id = value(args, "--id").unwrap_or_else(|| generated_id("session"));
    let session_dir = store.root().join("provider-sessions").join(&session_id);
    fs::create_dir_all(&session_dir)?;

    let stdout_ref = session_dir.join("review-stdout.log");
    let stderr_ref = session_dir.join("review-stderr.log");
    let started_at = now_string();
    let mut command_args = vec!["review".to_string(), "--base".to_string(), base];
    if has_flag(args, "--uncommitted") {
        command_args.push("--uncommitted".into());
    }
    if let Some(prompt) = prompt.clone() {
        command_args.push(prompt);
    }

    let output = Command::new("codex")
        .args(&command_args)
        .current_dir(&worktree)
        .output()?;
    fs::write(&stdout_ref, &output.stdout)?;
    fs::write(&stderr_ref, &output.stderr)?;

    let exit_code = output.status.code();
    let status = if output.status.success() {
        ProviderSessionStatus::Succeeded
    } else {
        ProviderSessionStatus::Failed
    };
    let evidence_id = generated_id("evidence");
    let session_ref = session_dir.display().to_string();
    let evidence = Evidence {
        id: evidence_id.clone(),
        task_id: Some(task_id.clone()),
        source_type: "codex_review_session".into(),
        source_ref: session_ref.clone(),
        summary: format!("Codex review session {session_id} for task {task_id}"),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;

    let session = ProviderSession {
        id: session_id.clone(),
        provider: "codex".into(),
        agent_member_id: agent_id.clone(),
        task_id: Some(task_id.clone()),
        workspace_ref: Some(worktree),
        provider_thread_id: None,
        provider_turn_id: None,
        terminal_source: Some(if exit_code == Some(0) {
            MessageTerminalSource::Unknown
        } else {
            MessageTerminalSource::Failed
        }),
        status: status.clone(),
        command: "codex".into(),
        args: command_args,
        prompt_ref: None,
        prompt_summary: prompt,
        provider_session_ref: None,
        stdout_ref: Some(stdout_ref.display().to_string()),
        jsonl_ref: None,
        transcript_ref: Some(stderr_ref.display().to_string()),
        last_message_ref: None,
        exit_code,
        started_at,
        ended_at: Some(now_string()),
        evidence_ids: vec![evidence_id.clone()],
    };
    store.append_provider_session(&session)?;

    let report = Message {
        id: generated_id("msg"),
        task_id: Some(task_id),
        from_agent_id: agent_id,
        to_agent_id: None,
        channel: Some("provider-review".into()),
        kind: MessageKind::Report,
        delivery_status: MessageDeliveryStatus::Delivered,
        content: format!("Codex review session {session_id} finished with exit_code={exit_code:?}"),
        evidence_ids: vec![evidence_id.clone()],
        created_at: now_string(),
        delivery: Some(MessageDelivery {
            provider_session_id: Some(session_id),
            provider_request_id: None,
            provider_thread_id: None,
            provider_turn_id: None,
            terminal_source: Some(MessageTerminalSource::Unknown),
            delivered_at: Some(now_string()),
            last_error: None,
        }),
    };
    store.append_message(&report)?;
    print_json(&session)?;
    Ok(())
}

fn start_agent_runtime(store: &HarnessStore, agent_id: &str) -> CliResult<AgentMember> {
    let mut member = latest_member(store, agent_id)?;
    ensure_member_accepts_delivery(&member)?;
    if let Some(runtime_id) = member.provider_runtime_id.as_deref() {
        if let Some(runtime) = latest_runtime(store, runtime_id)? {
            if runtime_is_alive(&runtime) {
                return Ok(member);
            }
        }
    }
    member.status = AgentMemberStatus::Creating;
    member.last_seen_at = Some(now_string());
    store.append_member(&member)?;
    let runtime = match start_codex_runtime(store, &member) {
        Ok(runtime) => runtime,
        Err(error) => {
            member.status = AgentMemberStatus::Error;
            member.last_seen_at = Some(now_string());
            store.append_member(&member)?;
            append_agent_event(
                store,
                &member.id,
                member.provider_runtime_id.as_deref(),
                None,
                "runtime_start_failed",
                &format!("Codex app-server runtime failed to start: {error}"),
                None,
            )?;
            return Err(error);
        }
    };
    member.status = AgentMemberStatus::Idle;
    member.provider_runtime_id = Some(runtime.id.clone());
    member.control_endpoint = runtime.control_endpoint.clone();
    member.last_seen_at = Some(now_string());
    store.append_runtime(&runtime)?;
    store.append_member(&member)?;
    append_agent_event(
        store,
        &member.id,
        Some(runtime.id.as_str()),
        None,
        "runtime_started",
        "Codex app-server runtime started",
        None,
    )?;
    Ok(member)
}

fn close_agent_member_value(store: &HarnessStore, agent_id: &str) -> CliResult<AgentMember> {
    let mut member = latest_member(store, agent_id)?;
    member.status = AgentMemberStatus::Closing;
    member.last_seen_at = Some(now_string());
    store.append_member(&member)?;

    let runtimes: Vec<_> = latest_runtimes(store)?
        .into_values()
        .filter(|runtime| runtime.agent_member_id == member.id)
        .filter(|runtime| runtime.status != AgentRuntimeStatus::Stopped)
        .collect();
    for mut runtime in runtimes {
        runtime.status = AgentRuntimeStatus::Stopping;
        runtime.last_event_at = Some(now_string());
        store.append_runtime(&runtime)?;
        if let Some(pid) = runtime.pid {
            if pid_is_alive(pid) {
                stop_pid(pid)?;
            }
        }
        runtime.status = AgentRuntimeStatus::Stopped;
        runtime.ended_at = Some(now_string());
        runtime.last_event_at = runtime.ended_at.clone();
        store.append_runtime(&runtime)?;
        append_agent_event(
            store,
            &member.id,
            Some(runtime.id.as_str()),
            None,
            "runtime_stopped",
            "Codex app-server runtime stopped",
            None,
        )?;
    }

    mark_running_provider_sessions_terminal(
        store,
        &member.id,
        ProviderSessionStatus::Canceled,
        Some(MessageTerminalSource::Failed),
    )?;
    member.status = AgentMemberStatus::Closed;
    member.current_task_id = None;
    member.last_seen_at = Some(now_string());
    store.append_member(&member)?;
    append_agent_event(
        store,
        &member.id,
        member.provider_runtime_id.as_deref(),
        None,
        "agent_closed",
        "Agent Member closed",
        None,
    )?;
    Ok(member)
}

fn ensure_member_accepts_delivery(member: &AgentMember) -> CliResult<()> {
    if member_status_rejects_delivery(&member.status) {
        return Err(CliError::Usage(format!(
            "agent {} is {:?}; closed, closing, or retired members cannot receive delivery or be restarted",
            member.id, member.status
        )));
    }
    Ok(())
}

fn member_status_rejects_delivery(status: &AgentMemberStatus) -> bool {
    matches!(
        status,
        AgentMemberStatus::Closing | AgentMemberStatus::Closed | AgentMemberStatus::Retired
    )
}

fn agent_health(store: &HarnessStore, agent_id: &str) -> CliResult<serde_json::Value> {
    let member = latest_member(store, agent_id)?;
    let mut runtime = member
        .provider_runtime_id
        .as_deref()
        .and_then(|runtime_id| latest_runtime(store, runtime_id).ok().flatten());
    let runtime_alive = runtime.as_ref().is_some_and(runtime_is_alive);
    let socket_path = runtime
        .as_ref()
        .and_then(|runtime| runtime.control_endpoint.as_deref())
        .and_then(|endpoint| socket_path_from_endpoint(endpoint).ok());
    let queued_messages = latest_messages_in_append_order(store)?
        .into_iter()
        .filter(|message| message.to_agent_id.as_deref() == Some(agent_id))
        .filter(|message| message.delivery_status == MessageDeliveryStatus::Queued)
        .count();
    let pid_alive = runtime
        .as_ref()
        .and_then(|runtime| runtime.pid)
        .is_some_and(pid_is_alive);
    let socket_exists = socket_path.as_ref().is_some_and(|path| path.exists());
    let protocol_probe = if pid_alive && socket_exists {
        let timeout_ms = env::var("HARNESS_AGENT_HEALTH_TIMEOUT_MS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1_500);
        socket_path
            .as_ref()
            .map(|path| probe_codex_protocol(path, timeout_ms))
            .transpose()?
            .flatten()
            .or_else(|| Some("unknown".into()))
    } else {
        Some("skipped: runtime process or socket is not available".into())
    };
    if let Some(runtime_value) = runtime.as_mut() {
        runtime_value.health.process_alive = pid_alive;
        runtime_value.health.socket_exists = socket_exists;
        runtime_value.health.protocol_probe = protocol_probe.clone();
        runtime_value.health.checked_at = Some(now_string());
        store.append_runtime(runtime_value)?;
    }
    Ok(serde_json::json!({
        "agent_member_id": member.id,
        "member_status": member.status,
        "runtime_id": runtime.as_ref().map(|runtime| runtime.id.clone()),
        "runtime_status": runtime.as_ref().map(|runtime| runtime.status.clone()),
        "pid": runtime.as_ref().and_then(|runtime| runtime.pid),
        "pid_alive": pid_alive,
        "socket_path": socket_path.as_ref().map(|path| path.display().to_string()),
        "socket_exists": socket_exists,
        "runtime_alive": runtime_alive,
        "health": {
            "process_alive": pid_alive,
            "socket_exists": socket_exists,
            "protocol_probe": protocol_probe,
            "delivery_probe": runtime.as_ref().and_then(|runtime| runtime.health.delivery_probe.clone())
        },
        "queued_messages": queued_messages,
        "provider_thread_id": member.provider_thread_id
    }))
}

fn probe_agent_hooks(
    store: &HarnessStore,
    agent_id: &str,
    timeout_ms: u64,
    trust: bool,
) -> CliResult<serde_json::Value> {
    let member = latest_member(store, agent_id)?;
    let runtime = member
        .provider_runtime_id
        .as_deref()
        .and_then(|runtime_id| latest_runtime(store, runtime_id).ok().flatten())
        .ok_or_else(|| CliError::Usage(format!("agent has no runtime: {agent_id}")))?;
    if !runtime_is_alive(&runtime) {
        return Err(CliError::Usage(format!(
            "agent runtime is not alive: {}",
            runtime.id
        )));
    }
    let endpoint = runtime.control_endpoint.as_deref().ok_or_else(|| {
        CliError::Usage(format!("runtime {} has no control endpoint", runtime.id))
    })?;
    let socket_path = socket_path_from_endpoint(endpoint)?;
    let probe_id = generated_id("hook-probe");
    let session_dir = store.root().join("provider-sessions").join(&probe_id);
    fs::create_dir_all(&session_dir)?;
    let cwd = member.worktree_ref.clone().or_else(|| {
        env::current_dir()
            .ok()
            .map(|path| path.display().to_string())
    });
    let list_request_id = generated_id("rpc");
    let list_request = build_hooks_list_request(&list_request_id, cwd.as_deref());
    let mut exchange = run_codex_app_server_exchange(
        &session_dir,
        &socket_path,
        "hooks-list",
        &[build_initialize_request(), list_request],
        timeout_ms,
    )?;
    let mut trust_write_ref = None;
    let mut trust_write_error = None;
    let mut hooks = hooks_from_list_response(&exchange.values, &list_request_id);

    if trust && exchange.failure_summary().is_none() && !hooks.is_empty() {
        let trust_request_id = generated_id("rpc");
        let trust_request = build_hooks_trust_request(&trust_request_id, &hooks)?;
        if hooks_trust_edit_count(&trust_request) > 0 {
            let verify_request_id = generated_id("rpc");
            let verify_request = build_hooks_list_request(&verify_request_id, cwd.as_deref());
            let trust_exchange = run_codex_app_server_exchange(
                &session_dir,
                &socket_path,
                "hooks-trust",
                &[build_initialize_request(), trust_request, verify_request],
                timeout_ms,
            )?;
            trust_write_ref = Some(trust_exchange.stdout_ref.display().to_string());
            if let Some(error) = trust_exchange.failure_summary() {
                trust_write_error = Some(error);
            } else {
                exchange = trust_exchange;
                hooks = hooks_from_list_response(&exchange.values, &verify_request_id);
            }
        }
    }
    if let Some(stdout_ref) = exchange.stdout_ref.to_str() {
        ingest_provider_output(
            store,
            &member.id,
            Some(runtime.id.as_str()),
            None,
            stdout_ref,
        )?;
    }
    let managed_hook_count = hooks.iter().filter(|hook| hook_is_managed(hook)).count();
    let trusted_hook_count = hooks.iter().filter(|hook| hook_is_trusted(hook)).count();
    let blocker = if let Some(error) = trust_write_error.clone() {
        Some(format!("hooks trust write failed: {error}"))
    } else if exchange.failure_summary().is_some() {
        Some("hooks/list failed".to_string())
    } else if hooks.is_empty() {
        Some("hooks/list returned no hooks for runtime cwd".to_string())
    } else if trust && trusted_hook_count == 0 {
        Some("hooks/list returned no trusted hook after trust write".to_string())
    } else if managed_hook_count == 0 {
        Some("hooks/list returned no managed or trusted hook".to_string())
    } else {
        None
    };
    if let Some(blocker) = blocker.as_deref() {
        let payload_ref = exchange.stdout_ref.display().to_string();
        append_agent_event(
            store,
            &member.id,
            Some(runtime.id.as_str()),
            None,
            "codex_hooks_blocked",
            blocker,
            Some(payload_ref.as_str()),
        )?;
    }
    let evidence = Evidence {
        id: generated_id("evidence"),
        task_id: None,
        source_type: "codex_hooks_probe".into(),
        source_ref: session_dir.display().to_string(),
        summary: format!(
            "Codex hooks/list probe for agent {} found {} hooks",
            member.id,
            hooks.len()
        ),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;
    Ok(serde_json::json!({
        "agent_member_id": member.id,
        "runtime_id": runtime.id,
        "provider_status": if exchange.failure_summary().is_some() { "failed" } else { "succeeded" },
        "hooks": hooks,
        "hook_count": hooks.len(),
        "managed_hook_count": managed_hook_count,
        "trusted_hook_count": trusted_hook_count,
        "trust_requested": trust,
        "trust_write_ref": trust_write_ref,
        "trust_write_error": trust_write_error,
        "blocker": blocker,
        "stdout_ref": exchange.stdout_ref,
        "stderr_ref": exchange.stderr_ref,
        "evidence_id": evidence.id
    }))
}

fn probe_codex_protocol(socket_path: &Path, timeout_ms: u64) -> CliResult<Option<String>> {
    let mut sent_values = Vec::new();
    let mut received_values = Vec::new();
    let initialize = build_initialize_request();
    let request_id = initialize
        .get("id")
        .and_then(|id| id.as_str())
        .unwrap_or_default()
        .to_string();
    match run_codex_websocket_exchange(
        socket_path,
        &[initialize],
        timeout_ms,
        &mut sent_values,
        &mut received_values,
    ) {
        Ok(()) => {
            let ok = received_values.iter().any(|value| {
                value.get("id").and_then(|id| id.as_str()) == Some(request_id.as_str())
                    && value.get("error").is_none()
            });
            Ok(Some(if ok {
                "pass: initialize response received".into()
            } else {
                "failed: initialize response missing".into()
            }))
        }
        Err(error) => Ok(Some(format!("failed: {error}"))),
    }
}

fn runtime_is_alive(runtime: &AgentRuntime) -> bool {
    let pid_alive = runtime.pid.is_some_and(pid_is_alive);
    let socket_alive = runtime
        .control_endpoint
        .as_deref()
        .and_then(|endpoint| socket_path_from_endpoint(endpoint).ok())
        .is_some_and(|path| path.exists());
    pid_alive && socket_alive && runtime.status == AgentRuntimeStatus::Running
}

fn pid_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn deliver_agent_messages(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let result = deliver_agent_messages_value(
        store,
        DeliveryOptions {
            agent_id: required(args, "--agent").or_else(|_| required(args, "--id"))?,
            message_filter: value(args, "--message"),
            dry_run: has_flag(args, "--dry-run"),
            start_runtime: has_flag(args, "--start-runtime"),
            timeout_ms: value(args, "--timeout-ms")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(3_000),
        },
    )?;
    print_json(&result)
}

#[derive(Debug, Clone)]
struct DeliveryOptions {
    agent_id: String,
    message_filter: Option<String>,
    dry_run: bool,
    start_runtime: bool,
    timeout_ms: u64,
}

fn deliver_agent_messages_value(
    store: &HarnessStore,
    options: DeliveryOptions,
) -> CliResult<serde_json::Value> {
    let DeliveryOptions {
        agent_id,
        message_filter,
        dry_run,
        start_runtime,
        timeout_ms,
    } = options;
    let mut member = latest_member(store, &agent_id)?;
    ensure_member_accepts_delivery(&member)?;
    let mut runtime = match member.provider_runtime_id.as_deref() {
        Some(runtime_id) => latest_runtime(store, runtime_id)?,
        None => None,
    };
    if runtime
        .as_ref()
        .is_some_and(|runtime| !runtime_is_alive(runtime))
    {
        append_agent_event(
            store,
            &member.id,
            member.provider_runtime_id.as_deref(),
            None,
            "runtime_stale",
            "Runtime pid or socket is not healthy",
            None,
        )?;
        mark_running_provider_sessions_terminal(
            store,
            &member.id,
            ProviderSessionStatus::Stale,
            Some(MessageTerminalSource::Failed),
        )?;
        runtime = None;
        member = latest_member(store, &agent_id)?;
        ensure_member_accepts_delivery(&member)?;
    }
    if has_unresolved_provider_session(store, &member.id)? {
        return Err(CliError::Usage(format!(
            "agent {} still has an unresolved provider turn; ingest a terminal provider event or close the runtime before delivering more messages",
            member.id
        )));
    }
    let queued: Vec<Message> = latest_messages_in_append_order(store)?
        .into_iter()
        .filter(|message| message.to_agent_id.as_deref() == Some(agent_id.as_str()))
        .filter(|message| message.delivery_status == MessageDeliveryStatus::Queued)
        .filter(|message| {
            message_filter
                .as_ref()
                .is_none_or(|message_id| message.id == *message_id)
        })
        .collect();

    if queued.is_empty() {
        return Ok(serde_json::json!({
            "agent_member_id": agent_id,
            "delivered": [],
            "note": "no queued messages"
        }));
    }

    let mut results = Vec::new();
    for message in queued {
        member = latest_member(store, &agent_id)?;
        ensure_member_accepts_delivery(&member)?;
        let delivery_id = generated_id("delivery");
        let claimed_message = match claim_message_for_delivery(
            store,
            &member,
            runtime.as_ref(),
            &message,
            &delivery_id,
        )? {
            Some(message) => message,
            None => continue,
        };

        member.status = AgentMemberStatus::Running;
        member.current_task_id = claimed_message.task_id.clone();
        member.last_seen_at = Some(now_string());
        store.append_member(&member)?;
        append_agent_event(
            store,
            &member.id,
            member.provider_runtime_id.as_deref(),
            claimed_message.task_id.as_deref(),
            "delivery_claimed",
            "Claimed message delivery before provider side effects",
            None,
        )?;

        let delivery = if dry_run {
            let provider_thread_id = member
                .provider_thread_id
                .clone()
                .or_else(|| Some(format!("dry-thread-{}", member.id)));
            let provider_turn_id = Some(format!("dry-turn-{}", claimed_message.id));
            let evidence_ids = record_claimed_delivery_terminal(
                store,
                &delivery_id,
                &claimed_message,
                ProviderSessionStatus::Succeeded,
                provider_thread_id.clone(),
                provider_turn_id.clone(),
                Some(MessageTerminalSource::DryRun),
                "dry-run delivery completed",
                Some("dry-run"),
                Some(0),
            )?;
            DeliveryOutcome {
                status: ProviderSessionStatus::Succeeded,
                provider_thread_id,
                provider_turn_id,
                terminal_source: Some(MessageTerminalSource::DryRun),
                stdout_ref: None,
                stderr_ref: None,
                request_ref: None,
                provider_request_id: None,
                provider_session_id: Some(delivery_id.clone()),
                evidence_ids,
                exit_code: Some(0),
                summary: "dry-run delivery completed".into(),
            }
        } else {
            let start_error = if runtime.is_none() && start_runtime {
                match start_agent_runtime(store, &agent_id) {
                    Ok(started_member) => {
                        member = started_member;
                        runtime = member
                            .provider_runtime_id
                            .as_deref()
                            .and_then(|runtime_id| {
                                latest_runtime(store, runtime_id).ok().flatten()
                            });
                        None
                    }
                    Err(error) => Some(error.to_string()),
                }
            } else {
                None
            };
            if let Some(error) = start_error {
                let summary = format!("Codex runtime start failed after claim: {error}");
                let evidence_ids = record_claimed_delivery_terminal(
                    store,
                    &delivery_id,
                    &claimed_message,
                    ProviderSessionStatus::Failed,
                    member.provider_thread_id.clone(),
                    None,
                    Some(MessageTerminalSource::Failed),
                    &summary,
                    None,
                    Some(1),
                )?;
                DeliveryOutcome {
                    status: ProviderSessionStatus::Failed,
                    provider_thread_id: member.provider_thread_id.clone(),
                    provider_turn_id: None,
                    terminal_source: Some(MessageTerminalSource::Failed),
                    stdout_ref: None,
                    stderr_ref: None,
                    request_ref: None,
                    provider_request_id: None,
                    provider_session_id: Some(delivery_id.clone()),
                    evidence_ids,
                    exit_code: Some(1),
                    summary,
                }
            } else if runtime.is_none() {
                let summary = format!("agent {agent_id} has no running provider runtime");
                let evidence_ids = record_claimed_delivery_terminal(
                    store,
                    &delivery_id,
                    &claimed_message,
                    ProviderSessionStatus::Failed,
                    member.provider_thread_id.clone(),
                    None,
                    Some(MessageTerminalSource::Failed),
                    &summary,
                    None,
                    Some(1),
                )?;
                DeliveryOutcome {
                    status: ProviderSessionStatus::Failed,
                    provider_thread_id: member.provider_thread_id.clone(),
                    provider_turn_id: None,
                    terminal_source: Some(MessageTerminalSource::Failed),
                    stdout_ref: None,
                    stderr_ref: None,
                    request_ref: None,
                    provider_request_id: None,
                    provider_session_id: Some(delivery_id.clone()),
                    evidence_ids,
                    exit_code: Some(1),
                    summary,
                }
            } else {
                let runtime = runtime.clone().expect("runtime checked");
                run_codex_delivery(
                    store,
                    &member,
                    &runtime,
                    &claimed_message,
                    &delivery_id,
                    timeout_ms,
                )?
            }
        };

        let delivery_unresolved = provider_status_blocks_delivery(&delivery.status);
        let mut delivered_message = latest_message(store, &claimed_message.id)?;
        delivered_message.delivery_status = message_status_for_delivery(&delivery.status);
        delivered_message.delivery = Some(MessageDelivery {
            provider_session_id: delivery.provider_session_id.clone(),
            provider_request_id: delivery.provider_request_id.clone(),
            provider_thread_id: delivery.provider_thread_id.clone(),
            provider_turn_id: delivery.provider_turn_id.clone(),
            terminal_source: delivery.terminal_source.clone(),
            delivered_at: Some(now_string()),
            last_error: delivery_error_message(&delivery.status, &delivery.summary),
        });
        store.append_message(&delivered_message)?;
        if delivery.provider_session_id.is_some() && !delivery_unresolved {
            let report = Message {
                id: generated_id("msg"),
                task_id: delivered_message.task_id.clone(),
                from_agent_id: member.id.clone(),
                to_agent_id: None,
                channel: Some("provider-report".into()),
                kind: MessageKind::Report,
                delivery_status: MessageDeliveryStatus::Delivered,
                content: delivery.summary.clone(),
                evidence_ids: delivery.evidence_ids.clone(),
                created_at: now_string(),
                delivery: delivered_message.delivery.clone(),
            };
            store.append_message(&report)?;
        }
        if let Some(thread_id) = delivery.provider_thread_id.clone() {
            member.provider_thread_id = Some(thread_id);
        }
        if let Some(mut runtime_value) = runtime.clone() {
            runtime_value.health.delivery_probe = Some(match &delivery.status {
                ProviderSessionStatus::Succeeded => {
                    format!(
                        "pass: {}",
                        delivery
                            .terminal_source
                            .as_ref()
                            .map(terminal_source_label)
                            .unwrap_or_else(|| "unknown terminal source".into())
                    )
                }
                ProviderSessionStatus::Running => format!("pending: {}", delivery.summary),
                ProviderSessionStatus::Stale => format!("stale: {}", delivery.summary),
                _ => format!("failed: {}", delivery.summary),
            });
            runtime_value.health.checked_at = Some(now_string());
            runtime_value.last_event_at = Some(now_string());
            store.append_runtime(&runtime_value)?;
            runtime = Some(runtime_value);
        }
        if delivery.status == ProviderSessionStatus::Running {
            member.status = AgentMemberStatus::Running;
            member.current_task_id = delivered_message.task_id.clone();
        } else if delivery.status == ProviderSessionStatus::Stale {
            member.status = AgentMemberStatus::Stale;
            member.current_task_id = delivered_message.task_id.clone();
        } else {
            member.status = AgentMemberStatus::Idle;
            member.current_task_id = None;
        }
        member.last_seen_at = Some(now_string());
        store.append_member(&member)?;
        append_agent_event(
            store,
            &member.id,
            member.provider_runtime_id.as_deref(),
            delivered_message.task_id.as_deref(),
            match &delivery.status {
                ProviderSessionStatus::Succeeded => "delivery_delivered",
                ProviderSessionStatus::Running => "delivery_running",
                ProviderSessionStatus::Stale => "delivery_stale",
                _ => "delivery_failed",
            },
            &delivery.summary,
            delivery
                .stdout_ref
                .as_deref()
                .or(delivery.stderr_ref.as_deref()),
        )?;

        if !delivery_unresolved {
            if let Some(stdout_ref) = delivery.stdout_ref.as_deref() {
                ingest_provider_output(
                    store,
                    &member.id,
                    member.provider_runtime_id.as_deref(),
                    delivered_message.task_id.as_deref(),
                    stdout_ref,
                )?;
            }
        }
        results.push(serde_json::json!({
            "message_id": delivered_message.id,
            "delivery_status": delivered_message.delivery_status,
            "provider_status": delivery.status,
            "provider_thread_id": member.provider_thread_id,
            "provider_turn_id": delivery.provider_turn_id,
            "terminal_source": delivery.terminal_source,
            "provider_request_id": delivery.provider_request_id,
            "request_ref": delivery.request_ref,
            "stdout_ref": delivery.stdout_ref,
            "stderr_ref": delivery.stderr_ref,
            "exit_code": delivery.exit_code
        }));
        if delivery_unresolved {
            break;
        }
    }

    Ok(serde_json::json!({
        "agent_member_id": agent_id,
        "delivered": results
    }))
}

#[derive(Debug, Clone)]
struct GatewayOptions {
    dry_run: bool,
    start_runtime: bool,
    timeout_ms: u64,
    claim_ttl_ms: u64,
}

fn run_provider_gateway(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let options = GatewayOptions {
        dry_run: has_flag(args, "--dry-run"),
        start_runtime: has_flag(args, "--start-runtime"),
        timeout_ms: value(args, "--timeout-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3_000),
        claim_ttl_ms: value(args, "--claim-ttl-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(300_000),
    };
    let once = has_flag(args, "--once");
    let interval_ms = value(args, "--interval-ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1_000);
    loop {
        let result = provider_gateway_tick_value(store, options.clone())?;
        print_json(&result)?;
        if once {
            break;
        }
        std::thread::sleep(Duration::from_millis(interval_ms));
    }
    Ok(())
}

fn provider_gateway_tick_value(
    store: &HarnessStore,
    options: GatewayOptions,
) -> CliResult<serde_json::Value> {
    let expired_claims = expire_safe_delivery_claims_value(store, options.claim_ttl_ms)?;
    let mut agent_ids = Vec::new();
    for message in latest_messages_in_append_order(store)? {
        if message.delivery_status == MessageDeliveryStatus::Queued {
            if let Some(agent_id) = message.to_agent_id {
                if !agent_ids.contains(&agent_id) {
                    agent_ids.push(agent_id);
                }
            }
        }
    }
    let mut results = Vec::new();
    for agent_id in agent_ids {
        match deliver_agent_messages_value(
            store,
            DeliveryOptions {
                agent_id: agent_id.clone(),
                message_filter: None,
                dry_run: options.dry_run,
                start_runtime: options.start_runtime,
                timeout_ms: options.timeout_ms,
            },
        ) {
            Ok(result) => results.push(serde_json::json!({
                "agent_member_id": agent_id,
                "ok": true,
                "result": result
            })),
            Err(error) => results.push(serde_json::json!({
                "agent_member_id": agent_id,
                "ok": false,
                "error": error.to_string()
            })),
        }
    }
    Ok(serde_json::json!({
        "generated_at": now_string(),
        "agent_count": results.len(),
        "expired_claims": expired_claims,
        "results": results
    }))
}

fn expire_safe_delivery_claims_value(
    store: &HarnessStore,
    claim_ttl_ms: u64,
) -> CliResult<Vec<serde_json::Value>> {
    if claim_ttl_ms == 0 {
        return Ok(Vec::new());
    }
    let now_ms = current_unix_ms();
    let messages = latest_messages(store)?;
    let sessions = latest_provider_sessions_in_append_order(store)?;
    let mut expired = Vec::new();
    for session in sessions {
        if session.status != ProviderSessionStatus::Running {
            continue;
        }
        let Some(started_ms) = parse_unix_ms(&session.started_at) else {
            continue;
        };
        if now_ms.saturating_sub(started_ms) < u128::from(claim_ttl_ms) {
            continue;
        }
        let Some(message) = messages.values().find(|message| {
            message.delivery_status == MessageDeliveryStatus::Acknowledged
                && message.delivery.as_ref().is_some_and(|delivery| {
                    delivery.provider_session_id.as_deref() == Some(session.id.as_str())
                        && delivery.provider_request_id.is_none()
                        && delivery.provider_turn_id.is_none()
                })
        }) else {
            continue;
        };
        if session.provider_turn_id.is_some() {
            continue;
        }
        let Some(agent_id) = message.to_agent_id.as_deref() else {
            continue;
        };
        match retry_delivery_value(
            store,
            agent_id,
            &message.id,
            Some(&session.id),
            "gateway expired unreconciled pre-provider delivery claim",
            false,
        ) {
            Ok(result) => expired.push(serde_json::json!({"ok": true, "result": result})),
            Err(error) => expired.push(serde_json::json!({
                "ok": false,
                "provider_session_id": session.id,
                "message_id": message.id,
                "error": error.to_string()
            })),
        }
    }
    Ok(expired)
}

#[derive(Debug)]
struct DeliveryOutcome {
    status: ProviderSessionStatus,
    provider_thread_id: Option<String>,
    provider_turn_id: Option<String>,
    terminal_source: Option<MessageTerminalSource>,
    stdout_ref: Option<String>,
    stderr_ref: Option<String>,
    request_ref: Option<String>,
    provider_request_id: Option<String>,
    provider_session_id: Option<String>,
    evidence_ids: Vec<String>,
    exit_code: Option<i32>,
    summary: String,
}

fn claim_message_for_delivery(
    store: &HarnessStore,
    member: &AgentMember,
    runtime: Option<&AgentRuntime>,
    message: &Message,
    delivery_id: &str,
) -> CliResult<Option<Message>> {
    let provider_session = build_claimed_provider_session(delivery_id, member, runtime, message);
    let delivery = MessageDelivery {
        provider_session_id: Some(delivery_id.to_string()),
        provider_request_id: None,
        provider_thread_id: member.provider_thread_id.clone(),
        provider_turn_id: None,
        terminal_source: None,
        delivered_at: None,
        last_error: None,
    };
    match store.claim_queued_message_delivery(
        &member.id,
        &message.id,
        delivery,
        provider_session,
    )? {
        MessageDeliveryClaimResult::Claimed(message) => Ok(Some(*message)),
        MessageDeliveryClaimResult::NotQueued => Ok(None),
        MessageDeliveryClaimResult::BlockedBySession(session_id) => Err(CliError::Usage(format!(
            "agent {} has unresolved provider session {}; cannot claim another delivery",
            member.id, session_id
        ))),
    }
}

fn retry_delivery_value(
    store: &HarnessStore,
    agent_id: &str,
    message_id: &str,
    session_id: Option<&str>,
    reason: &str,
    force: bool,
) -> CliResult<serde_json::Value> {
    let member = latest_member(store, agent_id)?;
    ensure_member_accepts_delivery(&member)?;
    let mut message = latest_message(store, message_id)?;
    if message.to_agent_id.as_deref() != Some(agent_id) {
        return Err(CliError::Usage(format!(
            "message {message_id} is not addressed to agent {agent_id}"
        )));
    }
    let delivery = message.delivery.clone().ok_or_else(|| {
        CliError::Usage(format!(
            "message {message_id} has no delivery claim to retry"
        ))
    })?;
    let session_id = session_id
        .map(str::to_string)
        .or(delivery.provider_session_id.clone())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "message {message_id} has no provider session id to retry"
            ))
        })?;
    let mut session = latest_provider_session(store, &session_id)?
        .ok_or_else(|| CliError::Usage(format!("provider session not found: {session_id}")))?;
    if session.agent_member_id != agent_id {
        return Err(CliError::Usage(format!(
            "provider session {session_id} does not belong to agent {agent_id}"
        )));
    }
    let safe_without_force = delivery.provider_request_id.is_none()
        && delivery.provider_turn_id.is_none()
        && session.provider_turn_id.is_none()
        && !matches!(session.status, ProviderSessionStatus::Succeeded);
    if !force && !safe_without_force {
        return Err(CliError::Usage(format!(
            "delivery retry for message {message_id} is not safe without --force; reconcile provider output first or pass --force explicitly"
        )));
    }

    let evidence_id = record_operator_evidence(
        store,
        message.task_id.clone(),
        "delivery_retry",
        &format!("provider-session:{session_id}"),
        reason,
    )?;
    session.status = ProviderSessionStatus::Canceled;
    session.terminal_source = Some(MessageTerminalSource::Failed);
    session.ended_at = Some(now_string());
    if !session.evidence_ids.contains(&evidence_id) {
        session.evidence_ids.push(evidence_id.clone());
    }
    store.append_provider_session(&session)?;

    message.delivery_status = MessageDeliveryStatus::Queued;
    message.delivery = None;
    store.append_message(&message)?;
    append_agent_event(
        store,
        agent_id,
        member.provider_runtime_id.as_deref(),
        message.task_id.as_deref(),
        "delivery_requeued",
        reason,
        None,
    )?;

    Ok(serde_json::json!({
        "agent_member_id": agent_id,
        "message_id": message_id,
        "provider_session_id": session_id,
        "delivery_status": message.delivery_status,
        "session_status": session.status,
        "evidence_id": evidence_id,
        "forced": force
    }))
}

fn reconcile_provider_session_value(
    store: &HarnessStore,
    agent_id: &str,
    session_id: &str,
    status: ProviderSessionStatus,
    terminal_source: MessageTerminalSource,
    reason: &str,
) -> CliResult<serde_json::Value> {
    if matches!(
        status,
        ProviderSessionStatus::Queued | ProviderSessionStatus::Running
    ) {
        return Err(CliError::Usage(
            "reconcile-session requires a terminal status".into(),
        ));
    }
    let mut session = latest_provider_session(store, session_id)?
        .ok_or_else(|| CliError::Usage(format!("provider session not found: {session_id}")))?;
    if session.agent_member_id != agent_id {
        return Err(CliError::Usage(format!(
            "provider session {session_id} does not belong to agent {agent_id}"
        )));
    }
    let evidence_id = record_operator_evidence(
        store,
        session.task_id.clone(),
        "provider_session_reconciliation",
        &format!("provider-session:{session_id}"),
        reason,
    )?;
    session.status = status.clone();
    session.terminal_source = Some(terminal_source.clone());
    session.ended_at = Some(now_string());
    if !session.evidence_ids.contains(&evidence_id) {
        session.evidence_ids.push(evidence_id.clone());
    }
    store.append_provider_session(&session)?;
    mark_delivery_messages_terminal(
        store,
        &session,
        status.clone(),
        Some(terminal_source.clone()),
    )?;
    if let Ok(mut member) = latest_member(store, agent_id) {
        if matches!(
            member.status,
            AgentMemberStatus::Running | AgentMemberStatus::Stale
        ) && member
            .current_task_id
            .as_ref()
            .map_or_else(|| true, |task_id| session.task_id.as_ref() == Some(task_id))
        {
            member.status = AgentMemberStatus::Idle;
            member.current_task_id = None;
            member.last_seen_at = Some(now_string());
            store.append_member(&member)?;
        }
    }
    append_agent_event(
        store,
        agent_id,
        None,
        session.task_id.as_deref(),
        "provider_session_reconciled",
        reason,
        None,
    )?;
    Ok(serde_json::json!({
        "agent_member_id": agent_id,
        "provider_session_id": session_id,
        "status": status,
        "terminal_source": terminal_source,
        "evidence_id": evidence_id
    }))
}

fn record_operator_evidence(
    store: &HarnessStore,
    task_id: Option<String>,
    source_type: &str,
    source_ref: &str,
    summary: &str,
) -> CliResult<String> {
    let evidence = Evidence {
        id: generated_id("evidence"),
        task_id,
        source_type: source_type.into(),
        source_ref: source_ref.into(),
        summary: summary.into(),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    let id = evidence.id.clone();
    store.append_evidence(&evidence)?;
    Ok(id)
}

fn build_claimed_provider_session(
    delivery_id: &str,
    member: &AgentMember,
    runtime: Option<&AgentRuntime>,
    message: &Message,
) -> ProviderSession {
    ProviderSession {
        id: delivery_id.into(),
        provider: "codex".into(),
        agent_member_id: member.id.clone(),
        task_id: message.task_id.clone(),
        workspace_ref: member.worktree_ref.clone(),
        provider_thread_id: member.provider_thread_id.clone(),
        provider_turn_id: None,
        terminal_source: None,
        status: ProviderSessionStatus::Running,
        command: "harness".into(),
        args: vec![
            "codex".into(),
            "message-delivery-claim".into(),
            message.id.clone(),
        ],
        prompt_ref: member.prompt_ref.clone(),
        prompt_summary: Some(format!("claimed delivery for message {}", message.id)),
        provider_session_ref: runtime.and_then(|runtime| runtime.control_endpoint.clone()),
        stdout_ref: None,
        jsonl_ref: None,
        transcript_ref: None,
        last_message_ref: None,
        exit_code: None,
        started_at: now_string(),
        ended_at: None,
        evidence_ids: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn record_claimed_delivery_terminal(
    store: &HarnessStore,
    delivery_id: &str,
    message: &Message,
    status: ProviderSessionStatus,
    provider_thread_id: Option<String>,
    provider_turn_id: Option<String>,
    terminal_source: Option<MessageTerminalSource>,
    summary: &str,
    source_ref: Option<&str>,
    exit_code: Option<i32>,
) -> CliResult<Vec<String>> {
    let evidence_id = generated_id("evidence");
    let evidence = Evidence {
        id: evidence_id.clone(),
        task_id: message.task_id.clone(),
        source_type: "codex_delivery_session".into(),
        source_ref: source_ref
            .map(str::to_string)
            .unwrap_or_else(|| format!("provider-session:{delivery_id}")),
        summary: summary.into(),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;

    let mut session = latest_provider_session(store, delivery_id)?.ok_or_else(|| {
        CliError::Usage(format!(
            "claimed provider session not found for delivery {delivery_id}"
        ))
    })?;
    session.status = status;
    session.provider_thread_id = provider_thread_id.or(session.provider_thread_id);
    session.provider_turn_id = provider_turn_id.or(session.provider_turn_id);
    session.terminal_source = terminal_source;
    session.exit_code = exit_code;
    session.ended_at = Some(now_string());
    if !session.evidence_ids.contains(&evidence_id) {
        session.evidence_ids.push(evidence_id.clone());
    }
    store.append_provider_session(&session)?;
    Ok(vec![evidence_id])
}

fn delivery_provider_accepted(status: &ProviderSessionStatus) -> bool {
    matches!(
        status,
        ProviderSessionStatus::Succeeded
            | ProviderSessionStatus::Running
            | ProviderSessionStatus::Stale
    )
}

fn message_status_for_delivery(status: &ProviderSessionStatus) -> MessageDeliveryStatus {
    message_status_for_terminal(status, None)
}

fn message_status_for_terminal(
    status: &ProviderSessionStatus,
    terminal_source: Option<&MessageTerminalSource>,
) -> MessageDeliveryStatus {
    match status {
        ProviderSessionStatus::Succeeded => MessageDeliveryStatus::Delivered,
        ProviderSessionStatus::Running => MessageDeliveryStatus::Acknowledged,
        ProviderSessionStatus::Stale if terminal_source != Some(&MessageTerminalSource::Failed) => {
            MessageDeliveryStatus::Acknowledged
        }
        _ => MessageDeliveryStatus::Failed,
    }
}

fn provider_status_blocks_delivery(status: &ProviderSessionStatus) -> bool {
    matches!(
        status,
        ProviderSessionStatus::Running | ProviderSessionStatus::Stale
    )
}

fn provider_session_blocks_delivery(session: &ProviderSession) -> bool {
    session.status == ProviderSessionStatus::Queued
        || session.status == ProviderSessionStatus::Running
        || (session.status == ProviderSessionStatus::Stale
            && session.terminal_source != Some(MessageTerminalSource::Failed))
}

fn provider_session_needs_terminal_update(session: &ProviderSession) -> bool {
    session.status == ProviderSessionStatus::Running
        || (session.status == ProviderSessionStatus::Stale
            && session.terminal_source != Some(MessageTerminalSource::Failed))
}

fn delivery_error_message(status: &ProviderSessionStatus, summary: &str) -> Option<String> {
    matches!(
        status,
        ProviderSessionStatus::Failed
            | ProviderSessionStatus::Canceled
            | ProviderSessionStatus::Stale
    )
    .then(|| summary.to_string())
}

fn delivery_exit_code(status: &ProviderSessionStatus, exit_code: Option<i32>) -> Option<i32> {
    match status {
        ProviderSessionStatus::Succeeded => Some(0),
        ProviderSessionStatus::Running | ProviderSessionStatus::Stale => None,
        _ => exit_code,
    }
}

fn run_codex_delivery(
    store: &HarnessStore,
    member: &AgentMember,
    runtime: &AgentRuntime,
    message: &Message,
    delivery_id: &str,
    timeout_ms: u64,
) -> CliResult<DeliveryOutcome> {
    let endpoint = runtime.control_endpoint.as_deref().ok_or_else(|| {
        CliError::Usage(format!("runtime {} has no control endpoint", runtime.id))
    })?;
    let socket_path = socket_path_from_endpoint(endpoint)?;
    let session_dir = store.root().join("provider-sessions").join(delivery_id);
    fs::create_dir_all(&session_dir)?;
    let started_at = now_string();
    let mut thread_id = member.provider_thread_id.clone();

    if thread_id.is_none() {
        let thread_request_id = generated_id("rpc");
        let thread_exchange = run_codex_app_server_exchange(
            &session_dir,
            &socket_path,
            "thread-start",
            &[
                build_initialize_request(),
                build_thread_start_request(member, &thread_request_id),
            ],
            timeout_ms,
        )?;
        let exit_code = thread_exchange.exit_code;
        let stdout_ref = Some(thread_exchange.stdout_ref.display().to_string());
        let stderr_ref = Some(thread_exchange.stderr_ref.display().to_string());

        if let Some(error) = thread_exchange.failure_summary() {
            let summary = format!("Codex thread/start failed: {error}");
            let evidence_id = record_delivery_provider_session(
                store,
                DeliverySessionRecord {
                    delivery_id,
                    member,
                    runtime,
                    message,
                    session_dir: &session_dir,
                    socket_path: &socket_path,
                    status: ProviderSessionStatus::Failed,
                    started_at,
                    stdout_ref: stdout_ref.clone(),
                    stderr_ref: stderr_ref.clone(),
                    exit_code,
                    provider_thread_id: None,
                    provider_turn_id: None,
                    terminal_source: Some(MessageTerminalSource::Failed),
                },
            )?;
            return Ok(DeliveryOutcome {
                status: ProviderSessionStatus::Failed,
                provider_thread_id: None,
                provider_turn_id: None,
                terminal_source: Some(MessageTerminalSource::Failed),
                stdout_ref,
                stderr_ref,
                request_ref: Some(session_dir.display().to_string()),
                provider_request_id: Some(thread_request_id),
                provider_session_id: Some(delivery_id.to_string()),
                evidence_ids: vec![evidence_id],
                exit_code,
                summary,
            });
        }

        thread_id = extract_thread_id(&thread_exchange.values, &thread_request_id);
        if thread_id.is_none() {
            let summary =
                "Codex thread/start produced no parseable thread id; fixture recorded".into();
            let evidence_id = record_delivery_provider_session(
                store,
                DeliverySessionRecord {
                    delivery_id,
                    member,
                    runtime,
                    message,
                    session_dir: &session_dir,
                    socket_path: &socket_path,
                    status: ProviderSessionStatus::Failed,
                    started_at,
                    stdout_ref: stdout_ref.clone(),
                    stderr_ref: stderr_ref.clone(),
                    exit_code,
                    provider_thread_id: None,
                    provider_turn_id: None,
                    terminal_source: Some(MessageTerminalSource::Failed),
                },
            )?;
            return Ok(DeliveryOutcome {
                status: ProviderSessionStatus::Failed,
                provider_thread_id: None,
                provider_turn_id: None,
                terminal_source: Some(MessageTerminalSource::Failed),
                stdout_ref,
                stderr_ref,
                request_ref: Some(session_dir.display().to_string()),
                provider_request_id: Some(thread_request_id),
                provider_session_id: Some(delivery_id.to_string()),
                evidence_ids: vec![evidence_id],
                exit_code,
                summary,
            });
        }
    }

    let thread_id = thread_id.expect("thread id checked above");
    let turn_request_id = generated_id("rpc");
    let turn_exchange = run_codex_app_server_exchange(
        &session_dir,
        &socket_path,
        "turn-start",
        &[
            build_initialize_request(),
            build_turn_start_request(member, message, &thread_id, &turn_request_id, delivery_id),
        ],
        timeout_ms,
    )?;
    let exit_code = turn_exchange.exit_code;
    let stdout_ref = Some(turn_exchange.stdout_ref.display().to_string());
    let stderr_ref = Some(turn_exchange.stderr_ref.display().to_string());

    let (status, summary) = classify_turn_exchange(&turn_exchange, &turn_request_id);
    let provider_turn_id = extract_turn_id(&turn_exchange.values, &turn_request_id);
    let terminal_source = if delivery_provider_accepted(&status) {
        terminal_source_from_values(&turn_exchange.values).or(Some(MessageTerminalSource::Unknown))
    } else {
        Some(MessageTerminalSource::Failed)
    };
    let evidence_id = record_delivery_provider_session(
        store,
        DeliverySessionRecord {
            delivery_id,
            member,
            runtime,
            message,
            session_dir: &session_dir,
            socket_path: &socket_path,
            status: status.clone(),
            started_at,
            stdout_ref: stdout_ref.clone(),
            stderr_ref: stderr_ref.clone(),
            exit_code: delivery_exit_code(&status, exit_code),
            provider_thread_id: Some(thread_id.clone()),
            provider_turn_id: provider_turn_id.clone(),
            terminal_source: terminal_source.clone(),
        },
    )?;

    Ok(DeliveryOutcome {
        provider_thread_id: delivery_provider_accepted(&status).then_some(thread_id),
        provider_turn_id,
        terminal_source,
        status: status.clone(),
        stdout_ref,
        stderr_ref,
        request_ref: Some(session_dir.display().to_string()),
        provider_request_id: Some(turn_request_id),
        provider_session_id: Some(delivery_id.to_string()),
        evidence_ids: vec![evidence_id],
        exit_code: delivery_exit_code(&status, exit_code),
        summary,
    })
}

fn classify_turn_exchange(
    exchange: &ProviderExchange,
    request_id: &str,
) -> (ProviderSessionStatus, String) {
    let turn_started = turn_exchange_confirms_turn_start(&exchange.values, request_id);
    if turn_started && exchange.only_waited_for_terminal_event() {
        if terminal_source_from_values(&exchange.values).is_some() {
            return (
                ProviderSessionStatus::Succeeded,
                "Codex turn/start was accepted and a terminal event was already captured".into(),
            );
        }
        return (
            ProviderSessionStatus::Stale,
            "Codex turn/start was accepted, but the terminal observer timed out before completion"
                .into(),
        );
    }
    if let Some(error) = exchange.failure_summary() {
        return (
            ProviderSessionStatus::Failed,
            format!("Codex turn/start failed: {error}"),
        );
    }
    if !turn_started {
        return (
            ProviderSessionStatus::Failed,
            "Codex turn/start produced provider output but no turn response or turn notification"
                .into(),
        );
    }
    (
        ProviderSessionStatus::Succeeded,
        "Codex app-server turn/start produced provider output".into(),
    )
}

#[derive(Debug)]
struct DeliverySessionRecord<'a> {
    delivery_id: &'a str,
    member: &'a AgentMember,
    runtime: &'a AgentRuntime,
    message: &'a Message,
    session_dir: &'a Path,
    socket_path: &'a Path,
    status: ProviderSessionStatus,
    started_at: String,
    stdout_ref: Option<String>,
    stderr_ref: Option<String>,
    exit_code: Option<i32>,
    provider_thread_id: Option<String>,
    provider_turn_id: Option<String>,
    terminal_source: Option<MessageTerminalSource>,
}

fn record_delivery_provider_session(
    store: &HarnessStore,
    record: DeliverySessionRecord<'_>,
) -> CliResult<String> {
    let evidence_id = generated_id("evidence");
    let evidence = Evidence {
        id: evidence_id.clone(),
        task_id: record.message.task_id.clone(),
        source_type: "codex_delivery_session".into(),
        source_ref: record.session_dir.display().to_string(),
        summary: format!(
            "Codex app-server delivery {} for message {}",
            record.delivery_id, record.message.id
        ),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;
    let ended_at = if record.status == ProviderSessionStatus::Running {
        None
    } else {
        Some(now_string())
    };
    let provider_session = ProviderSession {
        id: record.delivery_id.into(),
        provider: "codex".into(),
        agent_member_id: record.member.id.clone(),
        task_id: record.message.task_id.clone(),
        workspace_ref: None,
        provider_thread_id: record.provider_thread_id,
        provider_turn_id: record.provider_turn_id,
        terminal_source: record.terminal_source,
        status: record.status,
        command: "harness".into(),
        args: vec![
            "codex".into(),
            "app-server-ws-over-uds".into(),
            record.socket_path.display().to_string(),
        ],
        prompt_ref: record.member.prompt_ref.clone(),
        prompt_summary: Some(format!("deliver message {}", record.message.id)),
        provider_session_ref: record.runtime.control_endpoint.clone(),
        stdout_ref: record.stdout_ref,
        jsonl_ref: Some(record.session_dir.display().to_string()),
        transcript_ref: record.stderr_ref,
        last_message_ref: None,
        exit_code: record.exit_code,
        started_at: record.started_at,
        ended_at,
        evidence_ids: vec![evidence_id.clone()],
    };
    store.append_provider_session(&provider_session)?;
    Ok(evidence_id)
}

#[derive(Debug)]
struct ProviderExchange {
    values: Vec<serde_json::Value>,
    stdout_ref: PathBuf,
    stderr_ref: PathBuf,
    exit_code: Option<i32>,
    process_success: bool,
    error_messages: Vec<String>,
}

impl ProviderExchange {
    fn failure_summary(&self) -> Option<String> {
        if !self.process_success {
            return Some(format!("proxy process exited with {:?}", self.exit_code));
        }
        if !self.error_messages.is_empty() {
            return Some(self.error_messages.join("; "));
        }
        if self.values.is_empty() {
            return Some("proxy produced no JSON-RPC response or notification".into());
        }
        None
    }

    fn only_waited_for_terminal_event(&self) -> bool {
        !self.error_messages.is_empty()
            && self
                .error_messages
                .iter()
                .all(|message| message.contains("timed out waiting for turn terminal event"))
    }
}

fn run_codex_app_server_exchange(
    session_dir: &Path,
    socket_path: &Path,
    phase: &str,
    requests: &[serde_json::Value],
    timeout_ms: u64,
) -> CliResult<ProviderExchange> {
    let request_ref = session_dir.join(format!("{phase}.request.jsonl"));
    let stdout_ref = session_dir.join(format!("{phase}.stdout.jsonl"));
    let stderr_ref = session_dir.join(format!("{phase}.stderr.log"));

    let mut sent_values = Vec::new();
    let mut values = Vec::new();
    let mut stderr_lines = Vec::new();
    let process_success = match run_codex_websocket_exchange(
        socket_path,
        requests,
        timeout_ms,
        &mut sent_values,
        &mut values,
    ) {
        Ok(()) => true,
        Err(error) => {
            stderr_lines.push(error);
            false
        }
    };

    fs::write(&request_ref, jsonl_bytes(&sent_values)?)?;
    fs::write(&stdout_ref, jsonl_bytes(&values)?)?;
    fs::write(&stderr_ref, stderr_lines.join("\n"))?;
    let error_messages = provider_exchange_error_messages(&values, &stderr_lines);
    Ok(ProviderExchange {
        values,
        stdout_ref,
        stderr_ref,
        exit_code: Some(if process_success { 0 } else { 1 }),
        process_success,
        error_messages,
    })
}

fn provider_exchange_error_messages(
    values: &[serde_json::Value],
    stderr_lines: &[String],
) -> Vec<String> {
    let mut error_messages = jsonrpc_error_messages(values);
    error_messages.extend(stderr_lines.iter().cloned());
    error_messages
}

fn run_codex_websocket_exchange(
    socket_path: &Path,
    requests: &[serde_json::Value],
    timeout_ms: u64,
    sent_values: &mut Vec<serde_json::Value>,
    received_values: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
    let timeout = Duration::from_millis(timeout_ms.max(1));
    let deadline = Instant::now() + timeout;
    let stream = UnixStream::connect(socket_path)
        .map_err(|error| format!("connect {} failed: {error}", socket_path.display()))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .map_err(|error| format!("set read timeout failed: {error}"))?;
    stream
        .set_write_timeout(Some(timeout.min(Duration::from_secs(10))))
        .map_err(|error| format!("set write timeout failed: {error}"))?;

    let request = "ws://localhost/"
        .into_client_request()
        .map_err(|error| format!("build websocket request failed: {error}"))?;
    let (mut websocket, _) = tungstenite::client::client(request, stream)
        .map_err(|error| format!("websocket handshake failed: {error}"))?;

    let mut initialized = false;
    for request in requests {
        let method = request
            .get("method")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let request_id = request.get("id").cloned();
        send_ws_json(&mut websocket, request, sent_values)?;
        if let Some(request_id) = request_id.as_ref() {
            read_ws_until_response(&mut websocket, request_id, deadline, received_values)?;
        }
        if method == "initialize" && !initialized {
            let initialized_notification = serde_json::json!({"method": "initialized"});
            send_ws_json(&mut websocket, &initialized_notification, sent_values)?;
            initialized = true;
        }
        if method == "turn/start" {
            read_ws_until_turn_terminal(&mut websocket, deadline, received_values)?;
        } else {
            drain_ws_until_idle(
                &mut websocket,
                Duration::from_millis(250),
                deadline,
                received_values,
            )?;
        }
    }
    let _ = websocket.close(None);
    Ok(())
}

fn send_ws_json(
    websocket: &mut WebSocket<UnixStream>,
    value: &serde_json::Value,
    sent_values: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
    let payload = serde_json::to_string(value).map_err(|error| error.to_string())?;
    websocket
        .send(WebSocketMessage::Text(payload.into()))
        .map_err(|error| format!("websocket send failed: {error}"))?;
    sent_values.push(value.clone());
    Ok(())
}

fn read_ws_until_response(
    websocket: &mut WebSocket<UnixStream>,
    request_id: &serde_json::Value,
    deadline: Instant,
    received_values: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
    loop {
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for response id {request_id}"));
        }
        if let Some(value) = read_ws_json(websocket, deadline, received_values)? {
            if value.get("id") == Some(request_id)
                && (value.get("result").is_some() || value.get("error").is_some())
            {
                return Ok(());
            }
        }
    }
}

fn drain_ws_until_idle(
    websocket: &mut WebSocket<UnixStream>,
    idle_timeout: Duration,
    deadline: Instant,
    received_values: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
    let mut idle_deadline = Instant::now() + idle_timeout;
    while Instant::now() < deadline && Instant::now() < idle_deadline {
        if let Some(value) = read_ws_json(websocket, deadline, received_values)? {
            idle_deadline = Instant::now() + idle_timeout;
            if value.get("method").and_then(|method| method.as_str()) == Some("turn/completed") {
                break;
            }
        }
    }
    Ok(())
}

fn read_ws_until_turn_terminal(
    websocket: &mut WebSocket<UnixStream>,
    deadline: Instant,
    received_values: &mut Vec<serde_json::Value>,
) -> Result<(), String> {
    loop {
        if Instant::now() >= deadline {
            return Err("timed out waiting for turn terminal event".into());
        }
        if let Some(value) = read_ws_json(websocket, deadline, received_values)? {
            let method = value.get("method").and_then(|value| value.as_str());
            if method == Some("turn/completed") {
                return Ok(());
            }
            if method == Some("thread/status/changed")
                && value
                    .get("params")
                    .and_then(|params| params.get("status"))
                    .and_then(|status| status.get("type"))
                    .and_then(|status_type| status_type.as_str())
                    == Some("idle")
            {
                return Ok(());
            }
        }
    }
}

fn read_ws_json(
    websocket: &mut WebSocket<UnixStream>,
    deadline: Instant,
    received_values: &mut Vec<serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    if Instant::now() >= deadline {
        return Ok(None);
    }
    match websocket.read() {
        Ok(WebSocketMessage::Text(text)) => {
            let value = serde_json::from_str::<serde_json::Value>(text.as_ref())
                .map_err(|error| format!("invalid websocket JSON payload: {error}"))?;
            received_values.push(value.clone());
            Ok(Some(value))
        }
        Ok(WebSocketMessage::Ping(payload)) => {
            websocket
                .send(WebSocketMessage::Pong(payload))
                .map_err(|error| format!("websocket pong failed: {error}"))?;
            Ok(None)
        }
        Ok(WebSocketMessage::Close(_)) => Err("websocket closed before exchange completed".into()),
        Ok(WebSocketMessage::Binary(_)) => Err("unexpected binary websocket frame".into()),
        Ok(WebSocketMessage::Pong(_)) | Ok(WebSocketMessage::Frame(_)) => Ok(None),
        Err(tungstenite::Error::Io(error))
            if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
        {
            Ok(None)
        }
        Err(error) => Err(format!("websocket read failed: {error}")),
    }
}

fn jsonl_bytes(values: &[serde_json::Value]) -> CliResult<Vec<u8>> {
    let mut bytes = Vec::new();
    for value in values {
        serde_json::to_writer(&mut bytes, value)
            .map_err(|error| CliError::Usage(format!("serialize JSONL failed: {error}")))?;
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn build_initialize_request() -> serde_json::Value {
    serde_json::json!({
        "id": generated_id("rpc"),
        "method": "initialize",
        "params": {
            "clientInfo": {"name": "multi-agent-harness", "version": env!("CARGO_PKG_VERSION")},
            "capabilities": {"experimentalApi": true}
        }
    })
}

fn build_thread_start_request(member: &AgentMember, request_id: &str) -> serde_json::Value {
    let cwd = member.worktree_ref.clone().or_else(|| {
        env::current_dir()
            .ok()
            .map(|path| path.display().to_string())
    });
    let developer_instructions = provider_developer_instructions(member);
    let mut params = serde_json::Map::new();
    insert_optional_string(&mut params, "cwd", cwd);
    insert_optional_string(&mut params, "model", member.model.clone());
    params.insert(
        "developerInstructions".into(),
        serde_json::Value::String(developer_instructions),
    );
    params.insert("ephemeral".into(), serde_json::Value::Bool(false));
    if let Some(permissions) = codex_permissions_selection(member) {
        params.insert("permissions".into(), permissions);
    } else if let Some(sandbox) = member.provider_config.sandbox_policy.as_deref() {
        params.insert("sandbox".into(), serde_json::Value::String(sandbox.into()));
    }
    serde_json::json!({
        "id": request_id,
        "method": "thread/start",
        "params": params
    })
}

fn provider_developer_instructions(member: &AgentMember) -> String {
    let Some(prompt_ref) = member.prompt_ref.as_deref() else {
        return "Use harness messages as source of truth.".into();
    };
    let path = PathBuf::from(prompt_ref);
    if path.exists() {
        fs::read_to_string(path).unwrap_or_else(|_| prompt_ref.to_string())
    } else {
        prompt_ref.to_string()
    }
}

fn build_turn_start_request(
    member: &AgentMember,
    message: &Message,
    thread_id: &str,
    request_id: &str,
    delivery_attempt_id: &str,
) -> serde_json::Value {
    let mut params = serde_json::Map::new();
    params.insert(
        "threadId".into(),
        serde_json::Value::String(thread_id.into()),
    );
    params.insert(
        "input".into(),
        build_turn_input(message, delivery_attempt_id),
    );
    insert_optional_string(&mut params, "cwd", member.worktree_ref.clone());
    insert_optional_string(
        &mut params,
        "approvalPolicy",
        member.provider_config.approval_policy.clone(),
    );
    insert_optional_string(
        &mut params,
        "approvalsReviewer",
        member.provider_config.approvals_reviewer.clone(),
    );
    insert_optional_string(
        &mut params,
        "serviceTier",
        member.provider_config.service_tier.clone(),
    );
    insert_optional_string(
        &mut params,
        "collaborationMode",
        member.provider_config.collaboration_mode.clone(),
    );
    if let Some(permissions) = codex_permissions_selection(member) {
        params.insert("permissions".into(), permissions);
    } else if let Some(sandbox) = codex_sandbox_policy(member) {
        params.insert("sandboxPolicy".into(), sandbox);
    }
    serde_json::json!({
        "id": request_id,
        "method": "turn/start",
        "params": params
    })
}

fn insert_optional_string(
    params: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        params.insert(key.into(), serde_json::Value::String(value));
    }
}

fn codex_permissions_selection(member: &AgentMember) -> Option<serde_json::Value> {
    member
        .permission_profile
        .as_ref()
        .or(member.provider_config.permission_profile.as_ref())
        .map(|profile| {
            serde_json::json!({
                "type": "profile",
                "id": profile,
                "modifications": serde_json::Value::Null
            })
        })
}

fn codex_sandbox_policy(member: &AgentMember) -> Option<serde_json::Value> {
    let policy = member.provider_config.sandbox_policy.as_deref()?;
    match policy {
        "danger-full-access" | "dangerFullAccess" => {
            Some(serde_json::json!({"type": "dangerFullAccess"}))
        }
        "read-only" | "readOnly" => Some(serde_json::json!({
            "type": "readOnly",
            "networkAccess": false
        })),
        "workspace-write" | "workspaceWrite" => {
            let writable_roots = member
                .runtime_workspace_roots
                .iter()
                .chain(member.provider_config.runtime_workspace_roots.iter())
                .cloned()
                .collect::<Vec<_>>();
            Some(serde_json::json!({
                "type": "workspaceWrite",
                "networkAccess": false,
                "writableRoots": writable_roots
            }))
        }
        other => Some(serde_json::json!({"type": other})),
    }
}

fn build_turn_input(message: &Message, delivery_attempt_id: &str) -> serde_json::Value {
    serde_json::json!([{
        "type": "text",
        "text": format!(
            "Harness message envelope:\nmessage_id: {}\nkind: {}\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: {}\ndelivery_attempt: {}\ncontent:\n{}",
            message.id,
            message_kind_label(&message.kind),
            message.task_id.as_deref().unwrap_or("-"),
            message.from_agent_id,
            message.to_agent_id.as_deref().unwrap_or("-"),
            message.channel.as_deref().unwrap_or("-"),
            delivery_attempt_id,
            message.content
        )
    }])
}

#[cfg(test)]
fn frame_jsonrpc_requests(requests: &[serde_json::Value]) -> CliResult<Vec<u8>> {
    let mut framed = Vec::new();
    for request in requests {
        let body = serde_json::to_vec(request).expect("serialize json-rpc request");
        write!(framed, "Content-Length: {}\r\n\r\n", body.len())?;
        framed.extend_from_slice(&body);
    }
    Ok(framed)
}

fn socket_path_from_endpoint(endpoint: &str) -> CliResult<PathBuf> {
    endpoint
        .strip_prefix("unix://")
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage(format!("unsupported control endpoint: {endpoint}")))
}

fn ingest_provider_output(
    store: &HarnessStore,
    agent_member_id: &str,
    runtime_id: Option<&str>,
    task_id: Option<&str>,
    source_ref: &str,
) -> CliResult<()> {
    let text = fs::read_to_string(source_ref).unwrap_or_default();
    for value in extract_provider_json_values(&text) {
        let method = value
            .get("method")
            .and_then(|value| value.as_str())
            .or_else(|| value.get("type").and_then(|value| value.as_str()))
            .unwrap_or("provider_output");
        let event_type = method.replace(['/', '.'], "_");
        let summary = summarize_json_value(&value);
        let provider_context = value.get("params").unwrap_or(&value);
        let provider_thread_id = thread_id_from_container(provider_context);
        let provider_turn_id = turn_id_from_container(provider_context);
        let provider_child_thread_id = provider_child_thread_id_from_container(provider_context);
        let event = AgentEvent {
            id: generated_id("event"),
            agent_member_id: agent_member_id.into(),
            provider_runtime_id: runtime_id.map(str::to_string),
            task_id: task_id.map(str::to_string),
            provider: "codex".into(),
            provider_thread_id: provider_thread_id.clone(),
            provider_turn_id: provider_turn_id.clone(),
            provider_child_thread_id: provider_child_thread_id.clone(),
            event_type: event_type.clone(),
            summary,
            payload_ref: Some(source_ref.into()),
            created_at: now_string(),
        };
        store.append_event(&event)?;
        if let Some(child_thread) = provider_child_thread_from_event(
            agent_member_id,
            runtime_id,
            task_id,
            provider_thread_id.as_deref(),
            &value,
        ) {
            store.append_provider_child_thread(&child_thread)?;
        }
        if event_type.contains("turn_plan_updated") || event_type.contains("turn_diff_updated") {
            if let Some(task_id) = task_id {
                let proposal = Proposal {
                    id: generated_id("proposal"),
                    task_id: task_id.into(),
                    agent_member_id: agent_member_id.into(),
                    title: format!("Provider {}", event_type),
                    summary: "Proposal candidate from provider notification".into(),
                    status: ProposalStatus::Draft,
                    changed_paths: Vec::new(),
                    evidence_ids: Vec::new(),
                    created_at: now_string(),
                    updated_at: now_string(),
                };
                store.append_proposal(&proposal)?;
            }
        }
        if let Some(terminal_source) = terminal_source_from_provider_event(&value, &event_type) {
            let reconciled = reconcile_running_provider_sessions(
                store,
                agent_member_id,
                task_id,
                provider_thread_id.as_deref(),
                provider_turn_id.as_deref(),
                terminal_source,
            )?;
            if reconciled {
                continue;
            }
        }
        if event_type.contains("turn_completed") {
            let report = Message {
                id: generated_id("msg"),
                task_id: task_id.map(str::to_string),
                from_agent_id: agent_member_id.into(),
                to_agent_id: None,
                channel: Some("provider-report".into()),
                kind: MessageKind::Report,
                delivery_status: MessageDeliveryStatus::Delivered,
                content: "Provider turn completed".into(),
                evidence_ids: Vec::new(),
                created_at: now_string(),
                delivery: Some(MessageDelivery {
                    provider_session_id: None,
                    provider_request_id: None,
                    provider_thread_id,
                    provider_turn_id,
                    terminal_source: Some(MessageTerminalSource::TurnCompleted),
                    delivered_at: Some(now_string()),
                    last_error: None,
                }),
            };
            store.append_message(&report)?;
        }
    }
    Ok(())
}

fn terminal_source_from_provider_event(
    value: &serde_json::Value,
    event_type: &str,
) -> Option<MessageTerminalSource> {
    if event_type.contains("turn_completed") {
        return Some(MessageTerminalSource::TurnCompleted);
    }
    if event_type.contains("thread_status_changed")
        && value
            .get("params")
            .and_then(|params| params.get("status"))
            .and_then(|status| status.get("type"))
            .and_then(|status_type| status_type.as_str())
            == Some("idle")
    {
        return Some(MessageTerminalSource::ThreadIdle);
    }
    None
}

fn reconcile_running_provider_sessions(
    store: &HarnessStore,
    agent_member_id: &str,
    task_id: Option<&str>,
    provider_thread_id: Option<&str>,
    provider_turn_id: Option<&str>,
    terminal_source: MessageTerminalSource,
) -> CliResult<bool> {
    if provider_thread_id.is_none() && provider_turn_id.is_none() {
        return Ok(false);
    }
    let mut latest = BTreeMap::new();
    for session in store.provider_sessions()? {
        latest.insert(session.id.clone(), session);
    }
    let mut reconciled_task_ids = BTreeSet::new();
    let mut reconciled_any = false;
    for mut session in latest.into_values().filter(|session| {
        provider_session_needs_terminal_update(session)
            && session.agent_member_id == agent_member_id
            && task_id.is_none_or(|task_id| session.task_id.as_deref() == Some(task_id))
            && provider_thread_id
                .is_none_or(|thread_id| session.provider_thread_id.as_deref() == Some(thread_id))
            && provider_turn_id.is_none_or(|turn_id| {
                session
                    .provider_turn_id
                    .as_deref()
                    .is_none_or(|session_turn_id| session_turn_id == turn_id)
            })
    }) {
        session.status = ProviderSessionStatus::Succeeded;
        session.terminal_source = Some(terminal_source.clone());
        if session.provider_thread_id.is_none() {
            session.provider_thread_id = provider_thread_id.map(str::to_string);
        }
        if session.provider_turn_id.is_none() {
            session.provider_turn_id = provider_turn_id.map(str::to_string);
        }
        session.exit_code = session.exit_code.or(Some(0));
        session.ended_at = Some(now_string());
        if let Some(task_id) = session.task_id.clone() {
            reconciled_task_ids.insert(task_id);
        }
        store.append_provider_session(&session)?;
        reconciled_any = true;
        mark_delivery_messages_terminal(
            store,
            &session,
            ProviderSessionStatus::Succeeded,
            Some(terminal_source.clone()),
        )?;
    }
    if reconciled_any {
        if let Ok(mut member) = latest_member(store, agent_member_id) {
            if let Some(runtime_id) = member.provider_runtime_id.clone() {
                mark_runtime_delivery_reconciled(store, &runtime_id, &terminal_source)?;
            }
            if matches!(
                member.status,
                AgentMemberStatus::Running | AgentMemberStatus::Stale
            ) && member
                .current_task_id
                .as_ref()
                .map_or_else(|| true, |task_id| reconciled_task_ids.contains(task_id))
            {
                member.status = AgentMemberStatus::Idle;
                member.current_task_id = None;
                member.last_seen_at = Some(now_string());
                store.append_member(&member)?;
            }
        }
    }
    Ok(reconciled_any)
}

fn mark_delivery_messages_terminal(
    store: &HarnessStore,
    session: &ProviderSession,
    status: ProviderSessionStatus,
    terminal_source: Option<MessageTerminalSource>,
) -> CliResult<()> {
    let mut latest = BTreeMap::new();
    for message in store.messages()? {
        latest.insert(message.id.clone(), message);
    }
    for mut message in latest.into_values().filter(|message| {
        message.delivery_status == MessageDeliveryStatus::Acknowledged
            && message.delivery.as_ref().is_some_and(|delivery| {
                delivery.provider_session_id.as_deref() == Some(session.id.as_str())
            })
    }) {
        message.delivery_status = message_status_for_terminal(&status, terminal_source.as_ref());
        if let Some(delivery) = message.delivery.as_mut() {
            delivery.terminal_source = terminal_source.clone();
            if delivery.provider_thread_id.is_none() {
                delivery.provider_thread_id = session.provider_thread_id.clone();
            }
            if delivery.provider_turn_id.is_none() {
                delivery.provider_turn_id = session.provider_turn_id.clone();
            }
            delivery.delivered_at = Some(now_string());
            delivery.last_error = delivery_error_message(&status, "provider delivery ended");
        }
        store.append_message(&message)?;
        let report = Message {
            id: generated_id("msg"),
            task_id: message.task_id.clone(),
            from_agent_id: session.agent_member_id.clone(),
            to_agent_id: None,
            channel: Some("provider-report".into()),
            kind: MessageKind::Report,
            delivery_status: MessageDeliveryStatus::Delivered,
            content: format!(
                "Provider delivery {} ended with status {}",
                session.id,
                provider_status_label(&status)
            ),
            evidence_ids: session.evidence_ids.clone(),
            created_at: now_string(),
            delivery: message.delivery.clone(),
        };
        store.append_message(&report)?;
    }
    Ok(())
}

fn mark_runtime_delivery_reconciled(
    store: &HarnessStore,
    runtime_id: &str,
    terminal_source: &MessageTerminalSource,
) -> CliResult<()> {
    if let Some(mut runtime) = latest_runtime(store, runtime_id)? {
        runtime.health.delivery_probe =
            Some(format!("pass: {}", terminal_source_label(terminal_source)));
        runtime.health.checked_at = Some(now_string());
        runtime.last_event_at = Some(now_string());
        store.append_runtime(&runtime)?;
    }
    Ok(())
}

fn mark_runtime_delivery_terminal(
    store: &HarnessStore,
    runtime_id: &str,
    status: &ProviderSessionStatus,
    terminal_source: Option<&MessageTerminalSource>,
) -> CliResult<()> {
    if let Some(mut runtime) = latest_runtime(store, runtime_id)? {
        runtime.health.delivery_probe = Some(match status {
            ProviderSessionStatus::Succeeded => format!(
                "pass: {}",
                terminal_source
                    .map(terminal_source_label)
                    .unwrap_or_else(|| "unknown".into())
            ),
            ProviderSessionStatus::Stale => format!(
                "stale: {}",
                terminal_source
                    .map(terminal_source_label)
                    .unwrap_or_else(|| "unknown".into())
            ),
            _ => format!(
                "failed: {}",
                terminal_source
                    .map(terminal_source_label)
                    .unwrap_or_else(|| provider_status_label(status).into())
            ),
        });
        runtime.health.checked_at = Some(now_string());
        runtime.last_event_at = Some(now_string());
        store.append_runtime(&runtime)?;
    }
    Ok(())
}

fn has_unresolved_provider_session(store: &HarnessStore, agent_member_id: &str) -> CliResult<bool> {
    let mut latest = BTreeMap::new();
    for session in store.provider_sessions()? {
        latest.insert(session.id.clone(), session);
    }
    Ok(latest.into_values().any(|session| {
        session.agent_member_id == agent_member_id && provider_session_blocks_delivery(&session)
    }))
}

fn mark_running_provider_sessions_terminal(
    store: &HarnessStore,
    agent_member_id: &str,
    status: ProviderSessionStatus,
    terminal_source: Option<MessageTerminalSource>,
) -> CliResult<()> {
    let mut latest = BTreeMap::new();
    for session in store.provider_sessions()? {
        latest.insert(session.id.clone(), session);
    }
    let mut changed = false;
    for mut session in latest.into_values().filter(|session| {
        session.agent_member_id == agent_member_id
            && provider_session_needs_terminal_update(session)
    }) {
        session.status = status.clone();
        session.terminal_source = terminal_source.clone();
        session.ended_at = Some(now_string());
        store.append_provider_session(&session)?;
        mark_delivery_messages_terminal(store, &session, status.clone(), terminal_source.clone())?;
        changed = true;
    }
    if changed {
        if let Ok(mut member) = latest_member(store, agent_member_id) {
            if matches!(
                member.status,
                AgentMemberStatus::Running | AgentMemberStatus::Stale
            ) {
                if let Some(runtime_id) = member.provider_runtime_id.clone() {
                    mark_runtime_delivery_terminal(
                        store,
                        &runtime_id,
                        &status,
                        terminal_source.as_ref(),
                    )?;
                }
                member.status = AgentMemberStatus::Idle;
                member.current_task_id = None;
                member.last_seen_at = Some(now_string());
                store.append_member(&member)?;
            }
        }
    }
    Ok(())
}

fn extract_provider_json_values(text: &str) -> Vec<serde_json::Value> {
    extract_provider_json_values_from_bytes(text.as_bytes())
}

fn extract_provider_json_values_from_bytes(bytes: &[u8]) -> Vec<serde_json::Value> {
    let mut values = Vec::new();
    let mut seen = BTreeSet::new();
    let mut cursor = 0;
    while let Some(relative_header_start) = find_bytes(&bytes[cursor..], b"Content-Length:") {
        let header_start = cursor + relative_header_start;
        let Some(relative_header_end) = find_bytes(&bytes[header_start..], b"\r\n\r\n") else {
            break;
        };
        let header_end = header_start + relative_header_end;
        let header = String::from_utf8_lossy(&bytes[header_start..header_end]);
        let Some(content_length) = header.lines().find_map(parse_content_length) else {
            cursor = header_end + 4;
            continue;
        };
        let body_start = header_end + 4;
        let body_end = body_start + content_length;
        if body_end > bytes.len() {
            break;
        }
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes[body_start..body_end])
        {
            push_unique_json(&mut values, &mut seen, value);
        }
        cursor = body_end;
    }

    for line in String::from_utf8_lossy(bytes).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
                push_unique_json(&mut values, &mut seen, value);
            }
        }
    }
    values
}

fn parse_content_length(line: &str) -> Option<usize> {
    let (name, value) = line.split_once(':')?;
    if !name.eq_ignore_ascii_case("content-length") {
        return None;
    }
    value.trim().parse().ok()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn push_unique_json(
    values: &mut Vec<serde_json::Value>,
    seen: &mut BTreeSet<String>,
    value: serde_json::Value,
) {
    let key = serde_json::to_string(&value).unwrap_or_default();
    if seen.insert(key) {
        values.push(value);
    }
}

fn jsonrpc_error_messages(values: &[serde_json::Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value.get("error"))
        .map(|error| {
            error
                .get("message")
                .and_then(|message| message.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| summarize_json_value(error))
        })
        .collect()
}

fn hooks_from_list_response(
    values: &[serde_json::Value],
    request_id: &str,
) -> Vec<serde_json::Value> {
    for value in values {
        if value.get("id").and_then(|id| id.as_str()) != Some(request_id) {
            continue;
        }
        let Some(data) = value
            .get("result")
            .and_then(|result| result.get("data"))
            .and_then(|data| data.as_array())
        else {
            continue;
        };
        return data
            .iter()
            .flat_map(|entry| {
                entry
                    .get("hooks")
                    .and_then(|hooks| hooks.as_array())
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
    }
    Vec::new()
}

fn build_hooks_list_request(request_id: &str, cwd: Option<&str>) -> serde_json::Value {
    let cwds = cwd.map(|cwd| vec![cwd.to_string()]).unwrap_or_default();
    serde_json::json!({
        "id": request_id,
        "method": "hooks/list",
        "params": {
            "cwds": cwds
        }
    })
}

fn build_hooks_trust_request(
    request_id: &str,
    hooks: &[serde_json::Value],
) -> CliResult<serde_json::Value> {
    let mut hook_state = serde_json::Map::new();
    for hook in hooks {
        let Some(key) = hook.get("key").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(current_hash) = hook.get("currentHash").and_then(|value| value.as_str()) else {
            continue;
        };
        hook_state.insert(
            key.to_string(),
            serde_json::json!({
                "enabled": true,
                "trusted_hash": current_hash
            }),
        );
    }
    if hook_state.is_empty() {
        return Err(CliError::Usage(
            "hooks/list returned hooks without key/currentHash trust metadata".into(),
        ));
    }
    Ok(serde_json::json!({
        "id": request_id,
        "method": "config/batchWrite",
        "params": {
            "edits": [{
                "keyPath": "hooks.state",
                "value": serde_json::Value::Object(hook_state),
                "mergeStrategy": "upsert"
            }],
            "reloadUserConfig": true
        }
    }))
}

fn hooks_trust_edit_count(request: &serde_json::Value) -> usize {
    request
        .get("params")
        .and_then(|params| params.get("edits"))
        .and_then(|edits| edits.as_array())
        .map(|edits| edits.len())
        .unwrap_or(0)
}

fn hook_is_managed(hook: &serde_json::Value) -> bool {
    hook.get("isManaged").and_then(|value| value.as_bool()) == Some(true)
        || hook.get("trustStatus").and_then(|value| value.as_str()) == Some("managed")
        || hook_is_trusted(hook)
}

fn hook_is_trusted(hook: &serde_json::Value) -> bool {
    matches!(
        hook.get("trustStatus").and_then(|value| value.as_str()),
        Some("trusted" | "managed")
    )
}

fn turn_exchange_confirms_turn_start(values: &[serde_json::Value], request_id: &str) -> bool {
    values.iter().any(|value| {
        value.get("id").and_then(|id| id.as_str()) == Some(request_id)
            && value.get("error").is_none()
    }) || values.iter().any(|value| {
        value
            .get("method")
            .and_then(|method| method.as_str())
            .is_some_and(|method| {
                matches!(
                    method,
                    "turn/started"
                        | "turn/completed"
                        | "turn/status/changed"
                        | "turn/plan/updated"
                        | "turn/diff/updated"
                )
            })
    })
}

fn terminal_source_from_values(values: &[serde_json::Value]) -> Option<MessageTerminalSource> {
    for value in values {
        let method = value.get("method").and_then(|method| method.as_str());
        if method == Some("turn/completed") {
            return Some(MessageTerminalSource::TurnCompleted);
        }
    }
    for value in values {
        let method = value.get("method").and_then(|method| method.as_str());
        if method == Some("thread/status/changed")
            && value
                .get("params")
                .and_then(|params| params.get("status"))
                .and_then(|status| status.get("type"))
                .and_then(|status_type| status_type.as_str())
                == Some("idle")
        {
            return Some(MessageTerminalSource::ThreadIdle);
        }
    }
    None
}

fn extract_turn_id(values: &[serde_json::Value], request_id: &str) -> Option<String> {
    for value in values {
        if value.get("id").and_then(|id| id.as_str()) == Some(request_id) {
            if let Some(result) = value.get("result") {
                if let Some(turn_id) = turn_id_from_container(result) {
                    return Some(turn_id);
                }
            }
        }
    }
    for value in values {
        let method = value.get("method").and_then(|method| method.as_str());
        if method.is_some_and(|method| method.starts_with("turn/")) {
            if let Some(turn_id) = turn_id_from_container(value) {
                return Some(turn_id);
            }
        }
    }
    None
}

fn turn_id_from_container(value: &serde_json::Value) -> Option<String> {
    value
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(|id| id.as_str())
        .or_else(|| value.get("turnId").and_then(|id| id.as_str()))
        .or_else(|| value.get("turn_id").and_then(|id| id.as_str()))
        .or_else(|| value.get("id").and_then(|id| id.as_str()))
        .map(str::to_string)
}

fn provider_child_thread_id_from_container(value: &serde_json::Value) -> Option<String> {
    for path in [
        &["newThreadId"][..],
        &["new_thread_id"][..],
        &["childThreadId"][..],
        &["child_thread_id"][..],
        &["receiverThreadId"][..],
        &["receiver_thread_id"][..],
    ] {
        if let Some(thread_id) = json_path_string(value, path) {
            return Some(thread_id);
        }
    }
    None
}

fn provider_child_thread_from_event(
    agent_member_id: &str,
    runtime_id: Option<&str>,
    task_id: Option<&str>,
    parent_provider_thread_id: Option<&str>,
    value: &serde_json::Value,
) -> Option<ProviderChildThread> {
    let method = value
        .get("method")
        .and_then(|method| method.as_str())
        .or_else(|| value.get("type").and_then(|kind| kind.as_str()))
        .unwrap_or_default()
        .replace(['/', '.'], "_");
    let is_spawn_or_subagent = method.contains("subagent")
        || method.contains("collab_agent_spawn")
        || method.contains("agent_spawn");
    if !is_spawn_or_subagent {
        return None;
    }
    let params = value.get("params").unwrap_or(value);
    let provider_thread_id = provider_child_thread_id_from_container(params)
        .or_else(|| json_path_string(params, &["threadId"]))
        .or_else(|| json_path_string(params, &["thread_id"]))?;
    let status = if method.contains("stop") || method.contains("close") {
        ProviderChildThreadStatus::Closed
    } else if method.contains("end") || method.contains("completed") {
        ProviderChildThreadStatus::Completed
    } else if method.contains("start") || method.contains("spawn") {
        ProviderChildThreadStatus::Open
    } else {
        ProviderChildThreadStatus::Unknown
    };
    Some(ProviderChildThread {
        id: generated_id("provider-child"),
        provider: "codex".into(),
        agent_member_id: agent_member_id.into(),
        provider_runtime_id: runtime_id.map(str::to_string),
        task_id: task_id.map(str::to_string),
        parent_provider_thread_id: parent_provider_thread_id.map(str::to_string),
        provider_thread_id,
        provider_agent_path: json_path_string(params, &["agentPath"])
            .or_else(|| json_path_string(params, &["agent_path"])),
        provider_agent_nickname: json_path_string(params, &["agentNickname"])
            .or_else(|| json_path_string(params, &["agent_nickname"]))
            .or_else(|| json_path_string(params, &["newAgentNickname"])),
        provider_agent_role: json_path_string(params, &["agentRole"])
            .or_else(|| json_path_string(params, &["agent_role"]))
            .or_else(|| json_path_string(params, &["newAgentRole"])),
        status,
        last_message_ref: None,
        created_at: now_string(),
        updated_at: now_string(),
    })
}

fn extract_thread_id(values: &[serde_json::Value], request_id: &str) -> Option<String> {
    for value in values {
        if value.get("id").and_then(|id| id.as_str()) == Some(request_id) {
            if let Some(result) = value.get("result") {
                if let Some(thread_id) = thread_id_from_container(result) {
                    return Some(thread_id);
                }
            }
        }
    }

    for value in values {
        let method = value
            .get("method")
            .and_then(|method| method.as_str())
            .unwrap_or_default();
        if method == "thread/started" || method == "thread_started" {
            if let Some(params) = value.get("params") {
                if let Some(thread_id) = thread_id_from_container(params) {
                    return Some(thread_id);
                }
            }
        }
    }
    None
}

fn thread_id_from_container(value: &serde_json::Value) -> Option<String> {
    for path in [
        &["thread", "id"][..],
        &["thread", "threadId"][..],
        &["threadId"][..],
        &["thread_id"][..],
        &["id"][..],
    ] {
        if let Some(thread_id) = json_path_string(value, path) {
            return Some(thread_id);
        }
    }
    None
}

fn json_path_string(value: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(str::to_string)
}

fn summarize_json_value(value: &serde_json::Value) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "provider event".into());
    if raw.len() > 240 {
        format!("{}...", &raw[..240])
    } else {
        raw
    }
}

fn proposal_from_diff(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let task_id = required(args, "--task")?;
    let agent_id = required(args, "--agent")?;
    let worktree = required(args, "--worktree")?;
    let base = value(args, "--base").unwrap_or_else(|| "HEAD".into());
    let mut task = latest_task(store, &task_id)?;
    let changed_paths = git_changed_paths(&worktree, &base)?;
    if changed_paths.is_empty() && !has_flag(args, "--allow-empty") {
        return Err(CliError::Usage("git diff produced no changed paths".into()));
    }
    let violations = owned_path_violations(&changed_paths, &task.owned_paths);
    if !violations.is_empty() && !has_flag(args, "--allow-owned-path-violation") {
        return Err(CliError::Usage(format!(
            "changed paths outside owned_paths: {}",
            violations.join(",")
        )));
    }

    let proposal_id = value(args, "--id").unwrap_or_else(|| generated_id("proposal"));
    let proposal_dir = store.root().join("proposals").join(&proposal_id);
    fs::create_dir_all(&proposal_dir)?;
    let diff_ref = proposal_dir.join("diff.patch");
    fs::write(&diff_ref, git_diff_patch(&worktree, &base)?)?;
    let evidence = Evidence {
        id: generated_id("evidence"),
        task_id: Some(task_id.clone()),
        source_type: "git_diff".into(),
        source_ref: diff_ref.display().to_string(),
        summary: format!("Git diff from {base} in {worktree}"),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;
    let mut evidence_ids = vec![evidence.id.clone()];
    for command in many(args, "--check-cmd") {
        let check_evidence = run_check_command(store, &task_id, &worktree, &command)?;
        evidence_ids.push(check_evidence.id.clone());
    }
    let status = if has_flag(args, "--submit") {
        ProposalStatus::Submitted
    } else {
        ProposalStatus::Draft
    };
    let proposal = Proposal {
        id: proposal_id,
        task_id: task_id.clone(),
        agent_member_id: agent_id.clone(),
        title: required(args, "--title")?,
        summary: required(args, "--summary")?,
        status,
        changed_paths,
        evidence_ids,
        created_at: now_string(),
        updated_at: now_string(),
    };
    store.append_proposal(&proposal)?;
    task.status = TaskStatus::Review;
    task.updated_at = now_string();
    store.append_task(&task)?;
    append_agent_event(
        store,
        &agent_id,
        None,
        Some(&task_id),
        "proposal_from_diff",
        "Created proposal from git diff",
        Some(evidence.source_ref.as_str()),
    )?;
    print_json(&serde_json::json!({ "proposal": proposal, "evidence": evidence }))?;
    Ok(())
}

#[derive(Debug)]
struct GoalLearningStatus {
    goal_id: String,
    task_ids: Vec<String>,
    // Legacy representation: learning artifacts carried as Evidence rows
    // (source_type=goal_design|goal_evaluation|goal_case). Kept for back-compat.
    goal_design: Vec<Evidence>,
    goal_evaluation: Vec<Evidence>,
    goal_cases: Vec<Evidence>,
    // Graduated representation: first-class learning objects, scoped by goal_id.
    // Dual-read: a goal satisfies the design/evaluation gates via EITHER source.
    goal_design_objects: Vec<GoalDesign>,
    goal_evaluation_objects: Vec<GoalEvaluation>,
    goal_case_objects: Vec<GoalCase>,
    follow_up_tasks: Vec<Task>,
    assignment_messages: Vec<Message>,
    member_reports: Vec<Message>,
    critic_outputs: Vec<Evidence>,
    reviews: Vec<Review>,
    decisions: Vec<Decision>,
    waivers: Vec<Decision>,
    event_order: GoalLearningEventOrder,
}

#[derive(Debug)]
struct GoalLearningEventOrder {
    design_before_assignment: Option<bool>,
    assignment_before_report: Option<bool>,
    report_before_decision: Option<bool>,
    decision_before_evaluation: Option<bool>,
}

impl GoalLearningStatus {
    fn to_json(&self) -> serde_json::Value {
        let warnings = self.warnings(true);
        serde_json::json!({
            "goal_id": &self.goal_id,
            "task_ids": &self.task_ids,
            "goal_design": &self.goal_design,
            "goal_evaluation": &self.goal_evaluation,
            "goal_cases": &self.goal_cases,
            "goal_design_objects": &self.goal_design_objects,
            "goal_evaluation_objects": &self.goal_evaluation_objects,
            "goal_case_objects": &self.goal_case_objects,
            "follow_up_tasks": &self.follow_up_tasks,
            "assignment_messages": &self.assignment_messages,
            "member_reports": &self.member_reports,
            "critic_outputs": &self.critic_outputs,
            "reviews": &self.reviews,
            "decisions": &self.decisions,
            "waivers": &self.waivers,
            "event_order": {
                "design_before_assignment": self.event_order.design_before_assignment,
                "assignment_before_report": self.event_order.assignment_before_report,
                "report_before_decision": self.event_order.report_before_decision,
                "decision_before_evaluation": self.event_order.decision_before_evaluation,
            },
            "warnings": warnings,
            "ok": warnings.is_empty()
        })
    }

    fn has_goal_design(&self) -> bool {
        !self.goal_design.is_empty() || !self.goal_design_objects.is_empty()
    }

    fn has_goal_evaluation(&self) -> bool {
        !self.goal_evaluation.is_empty() || !self.goal_evaluation_objects.is_empty()
    }

    fn warnings(&self, require_evaluation: bool) -> Vec<String> {
        let mut warnings = Vec::new();
        // Dual-read: either a legacy Evidence row OR a graduated GoalDesign object
        // satisfies the gate (union by goal_id, no backfill).
        if !self.has_goal_design() {
            warnings.push("missing goal_design evidence".into());
        }
        if require_evaluation && !self.has_goal_evaluation() {
            warnings.push("missing goal_evaluation evidence".into());
        }
        if self.assignment_messages.is_empty() {
            warnings.push("missing assignment task message".into());
        }
        if self.member_reports.is_empty() {
            warnings.push("missing member report message".into());
        }
        if self.critic_outputs.is_empty() {
            warnings.push("missing critic/evaluator evidence".into());
        }
        if self.decisions.is_empty() {
            warnings.push("missing decision".into());
        }
        if self.event_order.design_before_assignment == Some(false) {
            warnings.push("goal_design evidence is post-hoc after assignment".into());
        }
        if self.event_order.assignment_before_report == Some(false) {
            warnings.push("member report appears before assignment message".into());
        }
        if self.event_order.report_before_decision == Some(false) {
            warnings.push("decision appears before member report".into());
        }
        if self.event_order.decision_before_evaluation == Some(false) {
            warnings.push("goal_evaluation appears before decision".into());
        }
        warnings
    }

    fn require_for_gate(
        &self,
        store: &HarnessStore,
        require_evaluation: bool,
        allow_waiver: bool,
        waiver_decision_id: Option<&str>,
    ) -> CliResult<()> {
        let warnings = self.warnings(require_evaluation);
        if warnings.is_empty() {
            return Ok(());
        }
        if allow_waiver {
            self.require_valid_waiver(store, waiver_decision_id)
                .map_err(|error| {
                    CliError::Usage(format!(
                        "goal {} failed learning gate: {}; waiver invalid: {}",
                        self.goal_id,
                        warnings.join("; "),
                        error
                    ))
                })?;
            return Ok(());
        }
        Err(CliError::Usage(format!(
            "goal {} failed learning gate: {}",
            self.goal_id,
            warnings.join("; ")
        )))
    }

    fn require_valid_waiver(
        &self,
        store: &HarnessStore,
        waiver_decision_id: Option<&str>,
    ) -> CliResult<()> {
        let waiver_decision_id = waiver_decision_id.ok_or_else(|| {
            CliError::Usage(
                "--waiver-decision <id> is required when using a goal-learning waiver".into(),
            )
        })?;
        let decision = self
            .waivers
            .iter()
            .find(|decision| decision.id == waiver_decision_id)
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "waiver decision {waiver_decision_id} was not found for goal {}",
                    self.goal_id
                ))
            })?;
        let evidence_by_id = latest_evidence(store)?;
        let task_by_id = latest_tasks(store)?;
        validate_goal_learning_waiver_decision(self, decision, &evidence_by_id, &task_by_id)
    }
}

fn validate_goal_learning_waiver_decision(
    status: &GoalLearningStatus,
    decision: &Decision,
    evidence_by_id: &BTreeMap<String, Evidence>,
    task_by_id: &BTreeMap<String, Task>,
) -> CliResult<()> {
    if decision.evidence_ids.is_empty() {
        return Err(CliError::Usage(format!(
            "waiver decision {} must reference evidence",
            decision.id
        )));
    }
    for evidence_id in &decision.evidence_ids {
        if !evidence_by_id.contains_key(evidence_id) {
            return Err(CliError::Usage(format!(
                "waiver decision {} references missing evidence {evidence_id}",
                decision.id
            )));
        }
    }

    let decision_task = task_by_id.get(&decision.task_id).ok_or_else(|| {
        CliError::Usage(format!(
            "waiver decision {} references missing task {}",
            decision.id, decision.task_id
        ))
    })?;
    if decision_task.goal_id.as_deref() != Some(status.goal_id.as_str()) {
        return Err(CliError::Usage(format!(
            "waiver decision {} is not attached to goal {}",
            decision.id, status.goal_id
        )));
    }
    if decision_task.owner_agent_id.trim().is_empty() {
        return Err(CliError::Usage(format!(
            "waiver decision {} must be attached to a task with an owner",
            decision.id
        )));
    }

    let text = format!("{} {}", decision.decision, decision.rationale).to_lowercase();
    let has_follow_up_word =
        text.contains("follow-up") || text.contains("follow up") || text.contains("后续");
    let has_follow_up_task = task_by_id.values().any(|task| {
        task.goal_id.as_deref() == Some(status.goal_id.as_str())
            && task.id != decision.task_id
            && text.contains(&task.id.to_lowercase())
    });
    if !has_follow_up_word || !has_follow_up_task {
        return Err(CliError::Usage(format!(
            "waiver decision {} must name a real follow-up task in goal {}",
            decision.id, status.goal_id
        )));
    }

    Ok(())
}

fn goal_learning_status(store: &HarnessStore, goal_id: &str) -> CliResult<GoalLearningStatus> {
    if !latest_goals(store)?.contains_key(goal_id) {
        return Err(CliError::Usage(format!("goal not found: {goal_id}")));
    }
    let all_tasks = latest_tasks(store)?;
    let tasks: Vec<_> = all_tasks
        .values()
        .filter(|task| task.goal_id.as_deref() == Some(goal_id))
        .cloned()
        .collect();
    let task_ids: BTreeSet<_> = tasks.iter().map(|task| task.id.clone()).collect();
    let mut task_id_vec: Vec<_> = task_ids.iter().cloned().collect();
    task_id_vec.sort();

    let evidence: Vec<_> = latest_evidence(store)?
        .into_values()
        .filter(|item| {
            item.task_id
                .as_ref()
                .is_some_and(|task_id| task_ids.contains(task_id))
        })
        .collect();
    let goal_design = evidence_by_type(&evidence, "goal_design");
    let goal_evaluation = evidence_by_type(&evidence, "goal_evaluation");
    let goal_cases = evidence_by_type(&evidence, "goal_case");

    // Dual-read: graduated learning objects scoped by goal_id (no backfill; both
    // representations coexist and union for the gate/event-order checks).
    let goal_design_objects: Vec<_> = latest_goal_designs_in_append_order(store)?
        .into_iter()
        .filter(|design| design.goal_id == goal_id)
        .collect();
    let goal_evaluation_objects: Vec<_> = latest_goal_evaluations_in_append_order(store)?
        .into_iter()
        .filter(|evaluation| evaluation.goal_id == goal_id)
        .collect();
    let goal_case_objects: Vec<_> = latest_goal_cases_in_append_order(store)?
        .into_iter()
        .filter(|case| case.source_goal_id == goal_id)
        .collect();
    let follow_up_tasks: Vec<_> = all_tasks
        .values()
        .filter(|task| {
            task.parent_task_id
                .as_ref()
                .is_some_and(|parent_id| task_ids.contains(parent_id))
                && is_follow_up_task(task)
        })
        .cloned()
        .collect();
    let critic_outputs: Vec<_> = evidence
        .iter()
        .filter(|item| {
            matches!(
                item.source_type.as_str(),
                "critic_findings" | "goal_evaluation"
            )
        })
        .cloned()
        .collect();

    let messages: Vec<_> = store
        .messages()?
        .into_iter()
        .filter(|message| {
            message
                .task_id
                .as_ref()
                .is_some_and(|task_id| task_ids.contains(task_id))
        })
        .collect();
    let assignment_messages: Vec<_> = messages
        .iter()
        .filter(|message| message.kind == MessageKind::Task)
        .cloned()
        .collect();
    let member_reports: Vec<_> = messages
        .iter()
        .filter(|message| message.kind == MessageKind::Report)
        .cloned()
        .collect();

    let decisions: Vec<_> = store
        .decisions()?
        .into_iter()
        .filter(|decision| task_ids.contains(&decision.task_id))
        .collect();
    let waivers: Vec<_> = decisions
        .iter()
        .filter(|decision| is_goal_learning_waiver_decision(decision))
        .cloned()
        .collect();
    let reviews: Vec<_> = latest_reviews_in_append_order(store)?
        .into_iter()
        .filter(|review| {
            review.goal_id.as_deref() == Some(goal_id)
                || review
                    .task_id
                    .as_ref()
                    .is_some_and(|task_id| task_ids.contains(task_id))
        })
        .collect();

    // Union the legacy-evidence times with graduated-object times so event-order
    // holds regardless of which representation a goal uses.
    let design_times = union_times(
        evidence_times(&goal_design),
        created_at_times(goal_design_objects.iter().map(|design| &design.created_at)),
    );
    let evaluation_times = union_times(
        evidence_times(&goal_evaluation),
        created_at_times(
            goal_evaluation_objects
                .iter()
                .map(|evaluation| &evaluation.created_at),
        ),
    );
    let event_order = GoalLearningEventOrder {
        design_before_assignment: compare_first(
            design_times,
            message_times(&assignment_messages),
            |left, right| left <= right,
        ),
        assignment_before_report: compare_first(
            message_times(&assignment_messages),
            message_times(&member_reports),
            |left, right| left <= right,
        ),
        report_before_decision: compare_first(
            message_times(&member_reports),
            decision_times(&decisions),
            |left, right| left <= right,
        ),
        decision_before_evaluation: compare_first(
            decision_times(&decisions),
            evaluation_times,
            |left, right| left <= right,
        ),
    };

    Ok(GoalLearningStatus {
        goal_id: goal_id.into(),
        task_ids: task_id_vec,
        goal_design,
        goal_evaluation,
        goal_cases,
        goal_design_objects,
        goal_evaluation_objects,
        goal_case_objects,
        follow_up_tasks,
        assignment_messages,
        member_reports,
        critic_outputs,
        reviews,
        decisions,
        waivers,
        event_order,
    })
}

fn evidence_by_type(evidence: &[Evidence], source_type: &str) -> Vec<Evidence> {
    evidence
        .iter()
        .filter(|item| item.source_type == source_type)
        .cloned()
        .collect()
}

fn is_goal_learning_waiver_decision(decision: &Decision) -> bool {
    let decision_text = decision.decision.to_lowercase();
    let rationale = decision.rationale.to_lowercase();
    decision_text.contains("waiver")
        || decision_text.contains("豁免")
        || rationale.contains("waiver decision")
        || rationale.contains("stage waiver")
        || rationale.contains("阶段豁免")
}

fn is_follow_up_task(task: &Task) -> bool {
    let title = task.title.to_lowercase();
    title.starts_with("follow-up:")
        || title.starts_with("follow up:")
        || title.starts_with("followup:")
        || title.starts_with("后续:")
}

fn evidence_times(evidence: &[Evidence]) -> Vec<u128> {
    evidence
        .iter()
        .filter_map(|item| parse_unix_ms(&item.created_at))
        .collect()
}

/// Parse a sequence of `created_at` strings into unix-ms times. Used to fold the
/// graduated learning objects into the same event-order check as legacy evidence.
fn created_at_times<'a>(created_at: impl Iterator<Item = &'a String>) -> Vec<u128> {
    created_at.filter_map(|value| parse_unix_ms(value)).collect()
}

/// Union two time sets so the dual-read gate treats legacy evidence and graduated
/// objects equivalently.
fn union_times(mut left: Vec<u128>, mut right: Vec<u128>) -> Vec<u128> {
    left.append(&mut right);
    left
}

fn message_times(messages: &[Message]) -> Vec<u128> {
    messages
        .iter()
        .filter_map(|item| parse_unix_ms(&item.created_at))
        .collect()
}

fn decision_times(decisions: &[Decision]) -> Vec<u128> {
    decisions
        .iter()
        .filter_map(|item| parse_unix_ms(&item.created_at))
        .collect()
}

fn compare_first(
    mut left: Vec<u128>,
    mut right: Vec<u128>,
    predicate: impl FnOnce(u128, u128) -> bool,
) -> Option<bool> {
    left.sort();
    right.sort();
    Some(predicate(*left.first()?, *right.first()?))
}

fn parse_unix_ms(value: &str) -> Option<u128> {
    value.strip_prefix("unix-ms:")?.parse().ok()
}

fn review_gate(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let task_id = required(args, "--task")?;
    let reviewer = required(args, "--reviewer")?;
    let verdict = required(args, "--decision")?;
    let rationale = required(args, "--rationale")?;
    let allow_no_check = has_flag(args, "--allow-no-check");
    let allow_no_critic = has_flag(args, "--allow-no-critic");
    let allow_no_provider_output = has_flag(args, "--allow-no-provider-output");
    let allow_no_proposal_evidence = has_flag(args, "--allow-no-proposal-evidence");
    let allow_global_evidence = has_flag(args, "--allow-global-evidence");
    let require_goal_design = has_flag(args, "--require-goal-design");
    let require_goal_evaluation = has_flag(args, "--require-goal-evaluation");
    let allow_goal_learning_waiver = has_flag(args, "--allow-goal-learning-waiver");
    let waiver_decision_id = value(args, "--waiver-decision");
    let mut task = latest_task(store, &task_id)?;
    let mut proposals: Vec<_> = latest_proposals(store)?
        .into_values()
        .filter(|proposal| proposal.task_id == task_id)
        .collect();
    proposals.sort_by(|left, right| left.updated_at.cmp(&right.updated_at));
    let mut proposal = proposals
        .pop()
        .ok_or_else(|| CliError::Usage(format!("task {task_id} has no proposal to review")))?;
    let mut evidence_ids = many(args, "--evidence");
    evidence_ids.extend(proposal.evidence_ids.clone());
    evidence_ids.sort();
    evidence_ids.dedup();
    if evidence_ids.is_empty() {
        return Err(CliError::Usage(format!(
            "task {task_id} cannot pass review without evidence"
        )));
    }
    let evidence_by_id = latest_evidence(store)?;
    let selected_evidence = resolve_review_evidence(
        &task_id,
        &evidence_ids,
        &evidence_by_id,
        allow_global_evidence,
    )?;
    validate_review_evidence_sources(&selected_evidence)?;
    let violations = owned_path_violations(&proposal.changed_paths, &task.owned_paths);
    if verdict == "accept"
        && !violations.is_empty()
        && !has_flag(args, "--allow-owned-path-violation")
    {
        return Err(CliError::Usage(format!(
            "proposal changes outside owned_paths: {}",
            violations.join(",")
        )));
    }
    if verdict == "accept" {
        if proposal.evidence_ids.is_empty() && !allow_no_proposal_evidence {
            return Err(CliError::Usage(format!(
                "task {task_id} cannot be accepted without proposal evidence"
            )));
        }
        validate_acceptance_evidence(
            store,
            &selected_evidence,
            allow_no_check,
            allow_no_critic,
            allow_no_provider_output,
        )?;
        if let Some(goal_id) = value(args, "--goal").or_else(|| task.goal_id.clone()) {
            let status = goal_learning_status(store, &goal_id)?;
            if status.goal_design.is_empty() {
                if has_flag(args, "--allow-missing-goal-design") || allow_goal_learning_waiver {
                    status.require_valid_waiver(store, waiver_decision_id.as_deref())?;
                } else {
                    return Err(CliError::Usage(format!(
                        "goal {goal_id} cannot pass review without goal_design evidence"
                    )));
                }
            }
        } else if require_goal_design || require_goal_evaluation {
            return Err(CliError::Usage(
                "--goal or task.goal_id is required for goal learning review gate".into(),
            ));
        }
        if require_goal_design || require_goal_evaluation {
            let goal_id = value(args, "--goal")
                .or_else(|| task.goal_id.clone())
                .ok_or_else(|| {
                    CliError::Usage(
                        "--goal or task.goal_id is required for goal learning review gate".into(),
                    )
                })?;
            let status = goal_learning_status(store, &goal_id)?;
            if require_goal_design && status.goal_design.is_empty() {
                if allow_goal_learning_waiver {
                    status.require_valid_waiver(store, waiver_decision_id.as_deref())?;
                } else {
                    return Err(CliError::Usage(format!(
                        "goal {goal_id} cannot pass review without goal_design evidence"
                    )));
                }
            }
            status.require_for_gate(
                store,
                require_goal_evaluation,
                allow_goal_learning_waiver,
                waiver_decision_id.as_deref(),
            )?;
        }
    }

    let decision_text = match verdict.as_str() {
        "accept" => {
            proposal.status = ProposalStatus::Accepted;
            task.status = TaskStatus::Done;
            "accepted"
        }
        "reject" => {
            proposal.status = ProposalStatus::Rejected;
            task.status = TaskStatus::Blocked;
            "rejected"
        }
        other => return Err(CliError::Usage(format!("unknown review decision: {other}"))),
    };
    proposal.updated_at = now_string();
    task.updated_at = now_string();
    store.append_proposal(&proposal)?;
    store.append_task(&task)?;
    let decision = Decision {
        id: value(args, "--id").unwrap_or_else(|| generated_id("decision")),
        task_id: task_id.clone(),
        decision: format!("{decision_text} by {reviewer}"),
        rationale,
        evidence_ids,
        created_at: now_string(),
        decision_kind: Some("verdict".to_string()),
        goal_id: None,
        is_waiver: false,
        follow_up_task_id: None,
    };
    store.append_decision(&decision)?;
    print_json(&serde_json::json!({
        "task": task,
        "proposal": proposal,
        "decision": decision
    }))?;
    Ok(())
}

fn resolve_review_evidence(
    task_id: &str,
    evidence_ids: &[String],
    evidence_by_id: &BTreeMap<String, Evidence>,
    allow_global_evidence: bool,
) -> CliResult<Vec<Evidence>> {
    let mut selected = Vec::new();
    for evidence_id in evidence_ids {
        let evidence = evidence_by_id.get(evidence_id).ok_or_else(|| {
            CliError::Usage(format!("review evidence id not found: {evidence_id}"))
        })?;
        match evidence.task_id.as_deref() {
            Some(evidence_task_id) if evidence_task_id != task_id => {
                return Err(CliError::Usage(format!(
                    "evidence {evidence_id} belongs to task {evidence_task_id}, not {task_id}"
                )));
            }
            None if !allow_global_evidence => {
                return Err(CliError::Usage(format!(
                    "evidence {evidence_id} is not attached to task {task_id}"
                )));
            }
            _ => selected.push(evidence.clone()),
        }
    }
    Ok(selected)
}

fn validate_review_evidence_sources(evidence: &[Evidence]) -> CliResult<()> {
    for item in evidence {
        if item.source_ref.trim().is_empty() {
            return Err(CliError::Usage(format!(
                "evidence {} has empty source_ref",
                item.id
            )));
        }
        if source_type_requires_existing_ref(&item.source_type)
            && !source_ref_exists(&item.source_ref)
        {
            return Err(CliError::Usage(format!(
                "evidence {} source_ref does not exist: {}",
                item.id, item.source_ref
            )));
        }
    }
    Ok(())
}

fn validate_acceptance_evidence(
    store: &HarnessStore,
    evidence: &[Evidence],
    allow_no_check: bool,
    allow_no_critic: bool,
    allow_no_provider_output: bool,
) -> CliResult<()> {
    if evidence
        .iter()
        .any(|item| item.source_type == "check_failed")
    {
        return Err(CliError::Usage(
            "acceptance cannot use failed check evidence".into(),
        ));
    }
    if !allow_no_check
        && !evidence
            .iter()
            .any(|item| item.source_type == "check_passed")
    {
        return Err(CliError::Usage(
            "acceptance requires check_passed evidence; use --allow-no-check only for explicit exceptions"
                .into(),
        ));
    }
    if !allow_no_critic
        && !evidence
            .iter()
            .any(|item| item.source_type == "critic_findings")
    {
        return Err(CliError::Usage(
            "acceptance requires critic_findings evidence; use --allow-no-critic only for explicit exceptions"
                .into(),
        ));
    }
    if !allow_no_provider_output
        && !evidence
            .iter()
            .any(|item| provider_output_source_type(&item.source_type))
    {
        return Err(CliError::Usage(
            "acceptance requires provider or worker output evidence; use --allow-no-provider-output only for explicit exceptions"
                .into(),
        ));
    }
    validate_provider_session_evidence(store, evidence)?;
    Ok(())
}

fn validate_provider_session_evidence(
    store: &HarnessStore,
    evidence: &[Evidence],
) -> CliResult<()> {
    let mut sessions = BTreeMap::new();
    for session in store.provider_sessions()? {
        sessions.insert(session.id.clone(), session);
    }
    for item in evidence
        .iter()
        .filter(|item| codex_session_source_type(&item.source_type))
    {
        let session = sessions
            .values()
            .find(|session| session.evidence_ids.iter().any(|id| id == &item.id))
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "evidence {} has source_type {} but no provider session references it",
                    item.id, item.source_type
                ))
            })?;
        if session.status != ProviderSessionStatus::Succeeded {
            return Err(CliError::Usage(format!(
                "provider session {} for evidence {} is {:?}, not succeeded",
                session.id, item.id, session.status
            )));
        }
    }
    Ok(())
}

fn source_type_requires_existing_ref(source_type: &str) -> bool {
    matches!(
        source_type,
        "git_diff"
            | "check_passed"
            | "check_failed"
            | "codex_delivery_session"
            | "codex_provider_session"
            | "codex_review_session"
            | "critic_findings"
            | "worker_report"
            | "provider_output"
            | "dashboard_snapshot"
            | "goal_design"
            | "goal_evaluation"
            | "goal_proposal"
            | "graph_change_proposal"
            | "blocker"
            | "follow_up"
            | "next_round_plan"
            | "protocol_fixture"
    )
}

fn source_ref_exists(source_ref: &str) -> bool {
    source_ref.starts_with("http://")
        || source_ref.starts_with("https://")
        || PathBuf::from(source_ref).exists()
}

fn provider_output_source_type(source_type: &str) -> bool {
    codex_session_source_type(source_type)
        || matches!(source_type, "worker_report" | "provider_output")
}

fn codex_session_source_type(source_type: &str) -> bool {
    matches!(
        source_type,
        "codex_delivery_session" | "codex_provider_session" | "codex_review_session"
    )
}

fn git_changed_paths(worktree: &str, base: &str) -> CliResult<Vec<String>> {
    let mut paths = BTreeSet::new();
    for args in [
        vec!["diff", "--name-only", &format!("{base}...HEAD")],
        vec!["diff", "--name-only", "HEAD"],
        vec!["diff", "--cached", "--name-only"],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(worktree)
            .args(args)
            .output()?;
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let path = line.trim();
                if !path.is_empty() {
                    paths.insert(path.to_string());
                }
            }
        }
    }
    for path in git_untracked_paths(worktree)? {
        paths.insert(path);
    }
    Ok(paths.into_iter().collect())
}

fn git_status_snapshot(
    worktree: &str,
    base: &str,
    owned_paths: &[String],
) -> CliResult<serde_json::Value> {
    let changed_paths = git_changed_paths(worktree, base)?;
    let branch = command_stdout("git", &["-C", worktree, "branch", "--show-current"])
        .unwrap_or_else(|_| "-".into());
    let status_short =
        command_stdout("git", &["-C", worktree, "status", "--short"]).unwrap_or_default();
    let violations = owned_path_violations(&changed_paths, owned_paths);
    Ok(serde_json::json!({
        "worktree": worktree,
        "base": base,
        "branch": branch.trim(),
        "dirty": !status_short.trim().is_empty(),
        "status_short": status_short.lines().collect::<Vec<_>>(),
        "changed_paths": changed_paths,
        "owned_paths": owned_paths,
        "owned_path_violations": violations
    }))
}

fn run_check_command(
    store: &HarnessStore,
    task_id: &str,
    worktree: &str,
    command: &str,
) -> CliResult<Evidence> {
    let check_id = generated_id("check");
    let check_dir = store.root().join("checks").join(&check_id);
    fs::create_dir_all(&check_dir)?;
    let stdout_ref = check_dir.join("stdout.log");
    let stderr_ref = check_dir.join("stderr.log");
    let output = Command::new("sh")
        .arg("-lc")
        .arg(command)
        .current_dir(worktree)
        .output()?;
    fs::write(&stdout_ref, &output.stdout)?;
    fs::write(&stderr_ref, &output.stderr)?;
    let evidence = Evidence {
        id: generated_id("evidence"),
        task_id: Some(task_id.into()),
        source_type: if output.status.success() {
            "check_passed".into()
        } else {
            "check_failed".into()
        },
        source_ref: check_dir.display().to_string(),
        summary: format!("check `{command}` exited with {:?}", output.status.code()),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;
    if !output.status.success() {
        return Err(CliError::Usage(format!(
            "check command failed for task {task_id}: {command}"
        )));
    }
    Ok(evidence)
}

fn command_stdout(command: &str, args: &[&str]) -> CliResult<String> {
    let output = Command::new(command).args(args).output()?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(CliError::Usage(format!("command failed: {command}")))
    }
}

fn git_diff_patch(worktree: &str, base: &str) -> CliResult<Vec<u8>> {
    for args in [
        vec!["diff", &format!("{base}...HEAD")],
        vec!["diff", "HEAD"],
        vec!["diff", "--cached"],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(worktree)
            .args(args)
            .output()?;
        if output.status.success() && !output.stdout.is_empty() {
            return Ok(output.stdout);
        }
    }
    let untracked = git_untracked_paths(worktree)?;
    if !untracked.is_empty() {
        let mut patch = Vec::new();
        for path in untracked {
            let file_path = PathBuf::from(worktree).join(&path);
            writeln!(patch, "diff --git a/{path} b/{path}")?;
            writeln!(patch, "new file mode 100644")?;
            writeln!(patch, "--- /dev/null")?;
            writeln!(patch, "+++ b/{path}")?;
            writeln!(patch, "@@")?;
            let content = fs::read_to_string(file_path).unwrap_or_default();
            for line in content.lines() {
                writeln!(patch, "+{line}")?;
            }
        }
        return Ok(patch);
    }
    Ok(Vec::new())
}

fn git_untracked_paths(worktree: &str) -> CliResult<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["ls-files", "--others", "--exclude-standard"])
        .output()?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn owned_path_violations(changed_paths: &[String], owned_paths: &[String]) -> Vec<String> {
    if owned_paths.is_empty() {
        return Vec::new();
    }
    changed_paths
        .iter()
        .filter(|path| {
            !owned_paths.iter().any(|owned| {
                let owned = owned.trim_end_matches('/');
                path.as_str() == owned || path.starts_with(&format!("{owned}/"))
            })
        })
        .cloned()
        .collect()
}

fn dashboard_snapshot(store: &HarnessStore) -> CliResult<serde_json::Value> {
    let goals = latest_goals(store)?;
    let tasks = latest_tasks(store)?;
    let members = latest_members(store)?;
    let teams = latest_teams(store)?;
    let runtimes = latest_runtimes(store)?;
    let proposals = latest_proposals(store)?;
    let messages = latest_messages_in_append_order(store)?;
    let events = store.events()?;
    let evidence = store.evidence()?;
    let decisions = store.decisions()?;
    let reviews = latest_reviews_in_append_order(store)?;
    let gaps = latest_gaps_in_append_order(store)?;
    let goal_designs = latest_goal_designs_in_append_order(store)?;
    let goal_evaluations = latest_goal_evaluations_in_append_order(store)?;
    let goal_cases = latest_goal_cases_in_append_order(store)?;
    let visions = latest_visions_in_append_order(store)?;
    let sessions = latest_provider_sessions_in_append_order(store)?;
    let provider_child_threads = store.provider_child_threads()?;
    let autonomous_proposals =
        autonomous_proposals_snapshot(&tasks, &messages, &evidence, &decisions);
    let goal_learning_status: Vec<_> = goals
        .keys()
        .filter_map(|goal_id| goal_learning_status(store, goal_id).ok())
        .map(|status| status.to_json())
        .collect();
    let mut kanban = BTreeMap::new();
    for status in [
        TaskStatus::Planned,
        TaskStatus::Assigned,
        TaskStatus::Running,
        TaskStatus::Blocked,
        TaskStatus::Review,
        TaskStatus::Done,
        TaskStatus::Archived,
    ] {
        let label = status_label(&status).to_string();
        let task_ids: Vec<_> = tasks
            .values()
            .filter(|task| task.status == status)
            .map(|task| task.id.clone())
            .collect();
        kanban.insert(label, task_ids);
    }
    let member_cards: Vec<_> = members
        .values()
        .map(|member| {
            let runtime = member
                .provider_runtime_id
                .as_ref()
                .and_then(|runtime_id| runtimes.get(runtime_id));
            let inbox_count = messages
                .iter()
                .filter(|message| message.to_agent_id.as_ref() == Some(&member.id))
                .count();
            let queued_count = messages
                .iter()
                .filter(|message| message.to_agent_id.as_ref() == Some(&member.id))
                .filter(|message| message.delivery_status == MessageDeliveryStatus::Queued)
                .count();
            let child_thread_count = provider_child_threads
                .iter()
                .filter(|thread| thread.agent_member_id == member.id)
                .count();
            serde_json::json!({
                "id": member.id,
                "name": member.name,
                "description": member.description,
                "role": member.role,
                "provider": member.provider,
                "status": member.status,
                "runtime_status": runtime.map(|runtime| &runtime.status),
                "runtime_id": runtime.map(|runtime| runtime.id.clone()),
                "runtime_pid": runtime.and_then(|runtime| runtime.pid),
                "runtime_alive": runtime.is_some_and(runtime_is_alive),
                "runtime_health": runtime.map(|runtime| runtime.health.clone()),
                "control_endpoint": member.control_endpoint.clone(),
                "provider_thread_id": member.provider_thread_id.clone(),
                "provider_agent_path": member.provider_agent_path.clone(),
                "provider_agent_nickname": member.provider_agent_nickname.clone(),
                "provider_agent_role": member.provider_agent_role.clone(),
                "current_task_id": member.current_task_id,
                "current_proposal_id": member.current_proposal_id,
                "prompt_ref": member.prompt_ref,
                "skill_refs": member.skill_refs,
                "inbox_count": inbox_count,
                "queued_count": queued_count,
                "provider_child_thread_count": child_thread_count
            })
        })
        .collect();
    Ok(serde_json::json!({
        "generated_at": now_string(),
        "goals": goals.into_values().collect::<Vec<_>>(),
        "goal_learning_status": goal_learning_status,
        "teams": teams.into_values().filter(|team| team.status == AgentTeamStatus::Active).collect::<Vec<_>>(),
        "members": member_cards,
        "kanban": kanban,
        "tasks": tasks.into_values().collect::<Vec<_>>(),
        "messages": messages,
        "events": events,
        "proposals": proposals.into_values().collect::<Vec<_>>(),
        "autonomous_proposals": autonomous_proposals,
        "evidence": evidence,
        "decisions": decisions,
        "reviews": reviews,
        "gaps": gaps,
        "goal_designs": goal_designs,
        "goal_evaluations": goal_evaluations,
        "goal_cases": goal_cases,
        "visions": visions,
        "provider_sessions": sessions,
        "provider_child_threads": provider_child_threads
    }))
}

fn autonomous_proposals_snapshot(
    tasks: &BTreeMap<String, Task>,
    messages: &[Message],
    evidence: &[Evidence],
    decisions: &[Decision],
) -> Vec<serde_json::Value> {
    evidence
        .iter()
        .filter(|item| autonomy_proposal_source_type(&item.source_type))
        .map(|proposal| {
            let task = proposal
                .task_id
                .as_ref()
                .and_then(|task_id| tasks.get(task_id));
            let message = messages
                .iter()
                .rev()
                .find(|message| message.evidence_ids.iter().any(|id| id == &proposal.id));
            let decision = decisions
                .iter()
                .rev()
                .find(|decision| decision.evidence_ids.iter().any(|id| id == &proposal.id));
            let follow_up_tasks: Vec<_> = tasks
                .values()
                .filter(|candidate| candidate.parent_task_id == proposal.task_id)
                .map(|task| task.id.clone())
                .collect();
            let follow_up_goals: Vec<_> = tasks
                .values()
                .filter(|candidate| candidate.parent_task_id == proposal.task_id)
                .filter_map(|task| task.goal_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            serde_json::json!({
                "id": proposal.id,
                "kind": proposal.source_type,
                "source_type": proposal.source_type,
                "source_ref": proposal.source_ref,
                "summary": proposal.summary,
                "task_id": proposal.task_id,
                "goal_id": task.and_then(|task| task.goal_id.clone()),
                "created_at": proposal.created_at,
                "message_id": message.map(|message| message.id.clone()),
                "from_agent_id": message.map(|message| message.from_agent_id.clone()),
                "to_agent_id": message.and_then(|message| message.to_agent_id.clone()),
                "linked_evidence_ids": message
                    .map(|message| message.evidence_ids.clone())
                    .unwrap_or_else(|| vec![proposal.id.clone()]),
                "disposition": decision
                    .map(|decision| autonomy_decision_disposition(&decision.decision))
                    .unwrap_or("pending"),
                "decision_id": decision.map(|decision| decision.id.clone()),
                "decision_rationale": decision.map(|decision| decision.rationale.clone()),
                "follow_up_task_ids": follow_up_tasks,
                "follow_up_goal_ids": follow_up_goals
            })
        })
        .collect()
}

fn autonomy_decision_disposition(decision: &str) -> &'static str {
    let text = decision.to_lowercase();
    if text.contains("request_evidence") || text.contains("request evidence") {
        "request_evidence"
    } else if text.contains("accept") {
        "accepted"
    } else if text.contains("reject") {
        "rejected"
    } else if text.contains("defer") {
        "deferred"
    } else {
        "decided"
    }
}

fn latest_task(store: &HarnessStore, task_id: &str) -> CliResult<Task> {
    latest_tasks(store)?
        .remove(task_id)
        .ok_or_else(|| CliError::Usage(format!("task not found: {task_id}")))
}

fn latest_member(store: &HarnessStore, member_id: &str) -> CliResult<AgentMember> {
    latest_members(store)?
        .remove(member_id)
        .ok_or_else(|| CliError::Usage(format!("agent member not found: {member_id}")))
}

fn latest_message(store: &HarnessStore, message_id: &str) -> CliResult<Message> {
    latest_messages(store)?
        .remove(message_id)
        .ok_or_else(|| CliError::Usage(format!("message not found: {message_id}")))
}

fn latest_messages(store: &HarnessStore) -> CliResult<BTreeMap<String, Message>> {
    let mut messages = BTreeMap::new();
    for message in store.messages()? {
        messages.insert(message.id.clone(), message);
    }
    Ok(messages)
}

fn latest_goals(store: &HarnessStore) -> CliResult<BTreeMap<String, Goal>> {
    let mut goals = BTreeMap::new();
    for goal in store.goals()? {
        goals.insert(goal.id.clone(), goal);
    }
    Ok(goals)
}

fn latest_runtime(store: &HarnessStore, runtime_id: &str) -> CliResult<Option<AgentRuntime>> {
    let mut runtimes = BTreeMap::new();
    for runtime in store.runtimes()? {
        runtimes.insert(runtime.id.clone(), runtime);
    }
    Ok(runtimes.remove(runtime_id))
}

fn latest_provider_session(
    store: &HarnessStore,
    session_id: &str,
) -> CliResult<Option<ProviderSession>> {
    let mut sessions = BTreeMap::new();
    for session in store.provider_sessions()? {
        sessions.insert(session.id.clone(), session);
    }
    Ok(sessions.remove(session_id))
}

fn latest_provider_sessions_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<ProviderSession>> {
    let mut session_ids = Vec::new();
    let mut sessions_by_id = BTreeMap::new();
    for session in store.provider_sessions()? {
        session_ids.retain(|id| id != &session.id);
        session_ids.push(session.id.clone());
        sessions_by_id.insert(session.id.clone(), session);
    }
    Ok(session_ids
        .into_iter()
        .filter_map(|id| sessions_by_id.remove(&id))
        .collect())
}

fn latest_messages_in_append_order(store: &HarnessStore) -> CliResult<Vec<Message>> {
    let mut message_ids = Vec::new();
    let mut messages_by_id = BTreeMap::new();
    for message in store.messages()? {
        message_ids.retain(|id| id != &message.id);
        message_ids.push(message.id.clone());
        messages_by_id.insert(message.id.clone(), message);
    }
    Ok(message_ids
        .into_iter()
        .filter_map(|id| messages_by_id.remove(&id))
        .collect())
}

fn latest_reviews_in_append_order(store: &HarnessStore) -> CliResult<Vec<Review>> {
    let mut review_ids = Vec::new();
    let mut reviews_by_id = BTreeMap::new();
    for review in store.reviews()? {
        review_ids.retain(|id| id != &review.id);
        review_ids.push(review.id.clone());
        reviews_by_id.insert(review.id.clone(), review);
    }
    Ok(review_ids
        .into_iter()
        .filter_map(|id| reviews_by_id.remove(&id))
        .collect())
}

fn latest_gaps_in_append_order(store: &HarnessStore) -> CliResult<Vec<Gap>> {
    let mut gap_ids = Vec::new();
    let mut gaps_by_id = BTreeMap::new();
    for gap in store.gaps()? {
        gap_ids.retain(|id| id != &gap.id);
        gap_ids.push(gap.id.clone());
        gaps_by_id.insert(gap.id.clone(), gap);
    }
    Ok(gap_ids
        .into_iter()
        .filter_map(|id| gaps_by_id.remove(&id))
        .collect())
}

fn latest_goal_designs_in_append_order(store: &HarnessStore) -> CliResult<Vec<GoalDesign>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for design in store.goal_designs()? {
        ids.retain(|id| id != &design.id);
        ids.push(design.id.clone());
        by_id.insert(design.id.clone(), design);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

fn latest_goal_evaluations_in_append_order(
    store: &HarnessStore,
) -> CliResult<Vec<GoalEvaluation>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for evaluation in store.goal_evaluations()? {
        ids.retain(|id| id != &evaluation.id);
        ids.push(evaluation.id.clone());
        by_id.insert(evaluation.id.clone(), evaluation);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

fn latest_goal_cases_in_append_order(store: &HarnessStore) -> CliResult<Vec<GoalCase>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for case in store.goal_cases()? {
        ids.retain(|id| id != &case.case_id);
        ids.push(case.case_id.clone());
        by_id.insert(case.case_id.clone(), case);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

fn latest_visions_in_append_order(store: &HarnessStore) -> CliResult<Vec<Vision>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for vision in store.visions()? {
        ids.retain(|id| id != &vision.id);
        ids.push(vision.id.clone());
        by_id.insert(vision.id.clone(), vision);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

fn latest_runtimes(store: &HarnessStore) -> CliResult<BTreeMap<String, AgentRuntime>> {
    let mut runtimes = BTreeMap::new();
    for runtime in store.runtimes()? {
        runtimes.insert(runtime.id.clone(), runtime);
    }
    Ok(runtimes)
}

fn latest_proposal(store: &HarnessStore, proposal_id: &str) -> CliResult<Proposal> {
    let mut proposals = BTreeMap::new();
    for proposal in store.proposals()? {
        proposals.insert(proposal.id.clone(), proposal);
    }
    proposals
        .remove(proposal_id)
        .ok_or_else(|| CliError::Usage(format!("proposal not found: {proposal_id}")))
}

fn latest_proposals(store: &HarnessStore) -> CliResult<BTreeMap<String, Proposal>> {
    let mut proposals = BTreeMap::new();
    for proposal in store.proposals()? {
        proposals.insert(proposal.id.clone(), proposal);
    }
    Ok(proposals)
}

fn latest_evidence(store: &HarnessStore) -> CliResult<BTreeMap<String, Evidence>> {
    let mut evidence = BTreeMap::new();
    for item in store.evidence()? {
        evidence.insert(item.id.clone(), item);
    }
    Ok(evidence)
}

fn latest_tasks(store: &HarnessStore) -> CliResult<BTreeMap<String, Task>> {
    let mut tasks = BTreeMap::new();
    for task in store.tasks()? {
        tasks.insert(task.id.clone(), task);
    }
    Ok(tasks)
}

fn latest_members(store: &HarnessStore) -> CliResult<BTreeMap<String, AgentMember>> {
    let mut members = BTreeMap::new();
    for member in store.members()? {
        members.insert(member.id.clone(), member);
    }
    Ok(members)
}

fn latest_teams(store: &HarnessStore) -> CliResult<BTreeMap<String, AgentTeam>> {
    let mut teams = BTreeMap::new();
    for team in store.teams()? {
        teams.insert(team.id.clone(), team);
    }
    Ok(teams)
}

fn build_member_from_args(args: &[String], status: AgentMemberStatus) -> CliResult<AgentMember> {
    Ok(AgentMember {
        id: value(args, "--id").unwrap_or_else(|| generated_id("agent")),
        name: required(args, "--name")?,
        description: value(args, "--description")
            .unwrap_or_else(|| "Codex-backed Agent Member".into()),
        role: required(args, "--role")?,
        provider: value(args, "--provider").unwrap_or_else(|| "codex".into()),
        model: value(args, "--model"),
        profile: value(args, "--profile"),
        provider_config: AgentProviderConfig {
            service_tier: value(args, "--service-tier"),
            collaboration_mode: value(args, "--collaboration-mode"),
            approval_policy: value(args, "--approval-policy"),
            approvals_reviewer: value(args, "--approvals-reviewer"),
            sandbox_policy: value(args, "--sandbox-policy"),
            permission_profile: value(args, "--permission-profile"),
            runtime_workspace_roots: many(args, "--runtime-workspace-root"),
            environment_id: value(args, "--environment"),
        },
        capabilities: many(args, "--capability"),
        team_ids: many(args, "--team"),
        prompt_ref: value(args, "--prompt-ref"),
        skill_refs: many(args, "--skill"),
        workspace_policy: value(args, "--workspace-policy"),
        worktree_ref: value(args, "--worktree"),
        permission_profile: value(args, "--permission-profile"),
        runtime_workspace_roots: many(args, "--runtime-workspace-root"),
        status,
        current_task_id: None,
        current_proposal_id: None,
        provider_runtime_id: None,
        provider_thread_id: None,
        provider_agent_path: value(args, "--provider-agent-path"),
        provider_agent_nickname: value(args, "--provider-agent-nickname"),
        provider_agent_role: value(args, "--provider-agent-role"),
        control_endpoint: None,
        created_at: now_string(),
        last_seen_at: None,
    })
}

fn ensure_agent_prompt(
    store: &HarnessStore,
    member: &AgentMember,
    args: &[String],
) -> CliResult<String> {
    if let Some(prompt_ref) = member.prompt_ref.clone() {
        return Ok(prompt_ref);
    }

    store.init()?;
    let prompt_path = store
        .root()
        .join("prompts")
        .join(format!("{}.md", member.id));
    let prompt = value(args, "--prompt").unwrap_or_else(|| build_bootstrap_prompt(member));
    fs::write(&prompt_path, prompt)?;
    Ok(prompt_path.display().to_string())
}

fn build_bootstrap_prompt(member: &AgentMember) -> String {
    format!(
        "# Agent Bootstrap\n\nid: {}\nname: {}\ndescription: {}\nrole: {}\nprovider: {}\n\nUse harness messages as the source of truth. Report task progress with evidence refs. Respect worktree, branch, PR, and owned-path boundaries.\n",
        member.id, member.name, member.description, member.role, member.provider
    )
}

fn add_codex_hook_config(
    args: &mut Vec<String>,
    agent_id: &str,
    runtime_id: &str,
) -> CliResult<()> {
    let harness_bin = env::current_exe()?;
    let hook_command = format!(
        "{} hook record --agent {} --runtime {}",
        shell_quote(&harness_bin.display().to_string()),
        shell_quote(agent_id),
        shell_quote(runtime_id)
    );
    let command_value = serde_json::to_string(&hook_command).expect("serialize hook command");
    let hook_specs = [
        ("SessionStart", "startup|resume|clear|compact"),
        ("UserPromptSubmit", "*"),
        ("PermissionRequest", "*"),
        ("PreToolUse", "*"),
        ("PostToolUse", "*"),
        ("Stop", "*"),
    ];
    for (event_name, matcher) in hook_specs {
        args.push("--config".into());
        args.push(format!(
            "hooks.{event_name}=[{{matcher={matcher:?},hooks=[{{type=\"command\",command={command_value},timeout=30,statusMessage=\"Harness telemetry\"}}]}}]"
        ));
    }
    Ok(())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn absolute_store_root(store: &HarnessStore) -> CliResult<PathBuf> {
    match fs::canonicalize(store.root()) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            if store.root().is_absolute() {
                Ok(store.root().to_path_buf())
            } else {
                Ok(env::current_dir()?.join(store.root()))
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn start_codex_runtime(store: &HarnessStore, member: &AgentMember) -> CliResult<AgentRuntime> {
    let runtime_id = generated_id("runtime");
    let runtime_dir = store.root().join("runtimes").join(&member.id);
    fs::create_dir_all(&runtime_dir)?;
    let socket_path = runtime_dir.join("codex.sock");
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }
    let stdout = File::create(runtime_dir.join("stdout.log"))?;
    let stderr = File::create(runtime_dir.join("stderr.log"))?;
    let endpoint = format!("unix://{}", socket_path.display());
    let mut args = vec!["app-server".to_string(), format!("--listen={endpoint}")];
    args.push("--enable".into());
    args.push("hooks".into());
    if env::var("HARNESS_CODEX_ENABLE_PLUGIN_HOOKS")
        .ok()
        .as_deref()
        != Some("1")
    {
        args.push("--disable".into());
        args.push("plugin_hooks".into());
    }
    if env::var("HARNESS_CODEX_DISABLE_SESSION_HOOK_CONFIG")
        .ok()
        .as_deref()
        != Some("1")
    {
        add_codex_hook_config(&mut args, &member.id, &runtime_id)?;
    }
    if let Some(model) = &member.model {
        args.push("--config".into());
        args.push(format!("model={model:?}"));
    }
    let mut command = Command::new("codex");
    configure_child_session(&mut command);
    let harness_root = absolute_store_root(store)?;
    command
        .args(&args)
        .env("HARNESS_ROOT", harness_root)
        .env("HARNESS_AGENT_MEMBER_ID", &member.id)
        .env("HARNESS_AGENT_RUNTIME_ID", &runtime_id)
        .env("HARNESS_CODEX_RUNTIME_DIR", &runtime_dir)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let mut child = command.spawn()?;
    let pid = child.id();
    let timeout_ms = env::var("HARNESS_AGENT_START_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(30_000);
    wait_for_runtime_socket(&mut child, &socket_path, timeout_ms)?;

    Ok(AgentRuntime {
        id: runtime_id,
        agent_member_id: member.id.clone(),
        provider: member.provider.clone(),
        status: AgentRuntimeStatus::Running,
        pid: Some(pid),
        control_endpoint: Some(endpoint),
        command: "codex".into(),
        args,
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: AgentRuntimeHealth {
            process_alive: true,
            socket_exists: true,
            protocol_probe: Some("unknown".into()),
            delivery_probe: Some("unknown".into()),
            checked_at: Some(now_string()),
        },
    })
}

fn configure_child_session(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            let result = setsid();
            if result == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
}

fn wait_for_runtime_socket(
    child: &mut Child,
    socket_path: &Path,
    timeout_ms: u64,
) -> CliResult<()> {
    let attempts = (timeout_ms / 50).max(1);
    for _ in 0..attempts {
        if socket_path.exists() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(CliError::Usage(format!(
                "codex app-server exited with {status:?} before creating socket {}",
                socket_path.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    Err(CliError::Usage(format!(
        "codex app-server did not create socket {} within {}ms",
        socket_path.display(),
        timeout_ms
    )))
}

fn parse_hook_payload(input: &str) -> serde_json::Value {
    if input.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(input).unwrap_or_else(|error| {
            serde_json::json!({
                "parse_error": error.to_string(),
                "raw": input
            })
        })
    }
}

fn persist_hook_payload(
    store: &HarnessStore,
    event_id: &str,
    payload: &serde_json::Value,
) -> CliResult<String> {
    let payload_dir = store.root().join("hook-payloads");
    fs::create_dir_all(&payload_dir)?;
    let path = payload_dir.join(format!("{event_id}.json"));
    fs::write(
        &path,
        serde_json::to_vec_pretty(payload).expect("serialize hook payload"),
    )?;
    Ok(path.display().to_string())
}

fn json_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn codex_hook_summary(hook_event_name: &str, payload: &serde_json::Value) -> String {
    match hook_event_name {
        "SessionStart" | "sessionStart" => format!(
            "Codex SessionStart hook source={}",
            json_str(payload, "source").unwrap_or_else(|| "unknown".into())
        ),
        "PreToolUse" | "PostToolUse" | "PermissionRequest" | "preToolUse" | "postToolUse"
        | "permissionRequest" => format!(
            "Codex {hook_event_name} hook tool={}",
            json_str(payload, "tool_name").unwrap_or_else(|| "unknown".into())
        ),
        "SubagentStart" | "SubagentStop" | "subagentStart" | "subagentStop" => format!(
            "Codex {hook_event_name} hook child={} type={}",
            json_str(payload, "agent_id").unwrap_or_else(|| "unknown".into()),
            json_str(payload, "agent_type").unwrap_or_else(|| "unknown".into())
        ),
        "Stop" | "stop" => format!(
            "Codex Stop hook turn={}",
            json_str(payload, "turn_id").unwrap_or_else(|| "unknown".into())
        ),
        other => format!("Codex hook {other}"),
    }
}

fn append_agent_event(
    store: &HarnessStore,
    agent_member_id: &str,
    runtime_id: Option<&str>,
    task_id: Option<&str>,
    event_type: &str,
    summary: &str,
    payload_ref: Option<&str>,
) -> CliResult<()> {
    let event = AgentEvent {
        id: generated_id("event"),
        agent_member_id: agent_member_id.into(),
        provider_runtime_id: runtime_id.map(str::to_string),
        task_id: task_id.map(str::to_string),
        provider: "codex".into(),
        provider_thread_id: None,
        provider_turn_id: None,
        provider_child_thread_id: None,
        event_type: event_type.into(),
        summary: summary.into(),
        payload_ref: payload_ref.map(str::to_string),
        created_at: now_string(),
    };
    store.append_event(&event)?;
    Ok(())
}

fn stop_pid(pid: u32) -> CliResult<()> {
    let status = Command::new("kill").arg(pid.to_string()).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::Usage(format!("failed to stop pid {pid}")))
    }
}

fn require_subcommand(args: &[String], usage: &str) -> CliResult<()> {
    if args.is_empty() {
        Err(CliError::Usage(format!("usage: harness {usage}")))
    } else {
        Ok(())
    }
}

fn required(args: &[String], name: &str) -> CliResult<String> {
    value(args, name).ok_or_else(|| CliError::Usage(format!("{name} is required")))
}

fn value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find_map(|window| (window[0] == name).then(|| window[1].clone()))
}

fn many(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|window| window[0] == name)
        .map(|window| window[1].clone())
        .collect()
}

fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn parse_message_kind(value: &str) -> CliResult<MessageKind> {
    match value {
        "message" => Ok(MessageKind::Message),
        "task" => Ok(MessageKind::Task),
        "report" => Ok(MessageKind::Report),
        other => Err(CliError::Usage(format!("unknown message kind: {other}"))),
    }
}

fn message_kind_label(kind: &MessageKind) -> &'static str {
    match kind {
        MessageKind::Message => "message",
        MessageKind::Task => "task",
        MessageKind::Report => "report",
    }
}

fn parse_delivery_status(value: &str) -> CliResult<MessageDeliveryStatus> {
    match value {
        "queued" => Ok(MessageDeliveryStatus::Queued),
        "delivered" => Ok(MessageDeliveryStatus::Delivered),
        "acknowledged" => Ok(MessageDeliveryStatus::Acknowledged),
        "failed" => Ok(MessageDeliveryStatus::Failed),
        other => Err(CliError::Usage(format!(
            "unknown message delivery status: {other}"
        ))),
    }
}

fn parse_provider_session_status(value: &str) -> CliResult<ProviderSessionStatus> {
    match value {
        "queued" => Ok(ProviderSessionStatus::Queued),
        "running" => Ok(ProviderSessionStatus::Running),
        "succeeded" => Ok(ProviderSessionStatus::Succeeded),
        "failed" => Ok(ProviderSessionStatus::Failed),
        "canceled" => Ok(ProviderSessionStatus::Canceled),
        "stale" => Ok(ProviderSessionStatus::Stale),
        other => Err(CliError::Usage(format!(
            "unknown provider session status: {other}"
        ))),
    }
}

fn parse_terminal_source(value: &str) -> CliResult<MessageTerminalSource> {
    match value {
        "turn_completed" => Ok(MessageTerminalSource::TurnCompleted),
        "thread_idle" => Ok(MessageTerminalSource::ThreadIdle),
        "thread_read" => Ok(MessageTerminalSource::ThreadRead),
        "hook_stop" => Ok(MessageTerminalSource::HookStop),
        "dry_run" => Ok(MessageTerminalSource::DryRun),
        "failed" => Ok(MessageTerminalSource::Failed),
        "unknown" => Ok(MessageTerminalSource::Unknown),
        other => Err(CliError::Usage(format!(
            "unknown message terminal source: {other}"
        ))),
    }
}

fn parse_proposal_status(value: &str) -> CliResult<ProposalStatus> {
    match value {
        "draft" => Ok(ProposalStatus::Draft),
        "submitted" => Ok(ProposalStatus::Submitted),
        "accepted" => Ok(ProposalStatus::Accepted),
        "rejected" => Ok(ProposalStatus::Rejected),
        "superseded" => Ok(ProposalStatus::Superseded),
        other => Err(CliError::Usage(format!("unknown proposal status: {other}"))),
    }
}

fn parse_task_status(value: &str) -> CliResult<TaskStatus> {
    match value {
        "planned" => Ok(TaskStatus::Planned),
        "assigned" => Ok(TaskStatus::Assigned),
        "running" => Ok(TaskStatus::Running),
        "blocked" => Ok(TaskStatus::Blocked),
        "review" => Ok(TaskStatus::Review),
        "done" => Ok(TaskStatus::Done),
        "archived" => Ok(TaskStatus::Archived),
        other => Err(CliError::Usage(format!("unknown task status: {other}"))),
    }
}

fn terminal_source_label(source: &MessageTerminalSource) -> String {
    match source {
        MessageTerminalSource::TurnCompleted => "turn_completed",
        MessageTerminalSource::ThreadIdle => "thread_idle",
        MessageTerminalSource::ThreadRead => "thread_read",
        MessageTerminalSource::HookStop => "hook_stop",
        MessageTerminalSource::DryRun => "dry_run",
        MessageTerminalSource::Failed => "failed",
        MessageTerminalSource::Unknown => "unknown",
    }
    .into()
}

fn provider_status_label(status: &ProviderSessionStatus) -> &'static str {
    match status {
        ProviderSessionStatus::Queued => "queued",
        ProviderSessionStatus::Running => "running",
        ProviderSessionStatus::Succeeded => "succeeded",
        ProviderSessionStatus::Failed => "failed",
        ProviderSessionStatus::Canceled => "canceled",
        ProviderSessionStatus::Stale => "stale",
    }
}

fn status_label(status: &TaskStatus) -> &'static str {
    match status {
        TaskStatus::Planned => "planned",
        TaskStatus::Assigned => "assigned",
        TaskStatus::Running => "running",
        TaskStatus::Blocked => "blocked",
        TaskStatus::Review => "review",
        TaskStatus::Done => "done",
        TaskStatus::Archived => "archived",
    }
}

fn now_string() -> String {
    let millis = current_unix_ms();
    format!("unix-ms:{millis}")
}

fn current_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn generated_id(prefix: &str) -> String {
    let millis = current_unix_ms();
    let counter = GENERATED_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{millis}-{counter}")
}

static GENERATED_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn print_json<T: serde::Serialize>(value: &T) -> CliResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serialize cli output")
    );
    Ok(())
}

fn print_help() {
    println!(
        "harness commands:
  init
  agent create --name <name> --role <role> [--description <text>] [--provider codex] [--team <team>] [--skill <skill>] [--prompt <text>] [--prompt-ref <path>] [--worktree <path>] [--permission-profile <profile>] [--runtime-workspace-root <path>] [--approval-policy <policy>] [--sandbox-policy <policy>] [--service-tier <tier>] [--collaboration-mode <mode>] [--provider-agent-path <path>] [--provider-agent-nickname <name>] [--provider-agent-role <role>] [--start]
  agent list
  agent start --id <agent>
  agent health --id <agent>
  agent show --id <agent>
  agent send --from <agent> --to <agent> --content <text> [--task <task>] [--channel <channel>] [--kind message|task|report]
  agent deliver --agent <agent> [--message <message>] [--dry-run] [--start-runtime] [--timeout-ms <ms>]
  agent retry-delivery --agent <agent> --message <message> [--session <session>] [--reason <text>] [--force]
  agent reconcile-session --agent <agent> --session <session> --status <succeeded|failed|canceled|stale> [--terminal-source <source>] [--reason <text>]
  agent gateway [--once] [--dry-run] [--start-runtime] [--interval-ms <ms>] [--timeout-ms <ms>] [--claim-ttl-ms <ms>]
  agent ingest --agent <agent> --source <provider-output> [--runtime <runtime>] [--task <task>]
  agent close --id <agent>
  team create --name <name> --description <text> --owner <agent> [--member <agent>]
  team list [--all]
  team show --id <team>
  team close --id <team>
  member register --name <name> --role <role> [--provider codex] [--capability <cap>] [--worktree <path>] [--permission-profile <profile>] [--runtime-workspace-root <path>]
  member list
  goal create --title <title> --objective <text> --owner <agent> [--success <text>]
  goal learning-status --id <goal> [--strict] [--require-evaluation] [--allow-waiver] [--waiver-decision <decision>]
  goal list
  task create --title <title> --objective <text> --owner <agent> [--goal <goal>] [--assignee <agent>] [--reviewer <agent>] [--workspace <path>] [--branch <ref>] [--pr <ref>] [--owned-path <path>] [--acceptance <text>]
  task assign --id <task> --assignee <agent> [--channel <channel>] [--allow-missing-goal-design --waiver-decision <decision>]
  task status --id <task> --status <planned|assigned|running|blocked|review|done|archived>
  task list
  message send --from <agent> --content <text> [--to <agent>] [--task <task>] [--channel <channel>] [--kind message|task|report]
  message list [--channel <channel>] [--task <task>]
  message status --id <message> --status <queued|delivered|acknowledged|failed>
  event add --agent <agent> --type <event_type> --summary <text> [--runtime <runtime>] [--task <task>] [--provider-thread <id>] [--provider-turn <id>] [--provider-child-thread <id>] [--payload-ref <ref>]
  event list [--agent <agent>] [--task <task>]
  proposal create --task <task> --agent <agent> --title <title> --summary <text> [--changed-path <path>] [--evidence <id>]
  proposal from-diff --task <task> --agent <agent> --worktree <path> --title <title> --summary <text> [--base <ref>] [--submit] [--check-cmd <cmd>]
  proposal list [--agent <agent>] [--task <task>]
  proposal status --id <proposal> --status <draft|submitted|accepted|rejected|superseded>
  git worktree-create --task <task> --repo <path> --path <worktree> --branch <branch> [--base <ref>] [--no-create]
  git attach --task <task> --workspace <path> --branch <branch> [--pr <ref>] [--owned-path <path>]
  git status [--task <task>] [--worktree <path>] [--base <ref>]
  git changed-paths --worktree <path> [--base <ref>]
  review gate --task <task> --reviewer <agent> --decision <accept|reject> --rationale <text> [--evidence <id>] [--allow-no-check] [--allow-no-critic] [--allow-no-provider-output] [--allow-missing-goal-design --waiver-decision <decision>] [--require-goal-design] [--require-goal-evaluation] [--allow-goal-learning-waiver --waiver-decision <decision>]
  evidence add --source-type <type> --source-ref <ref> --summary <text> [--task <task>]
  decision record --task <task> --decision <text> --rationale <text> [--evidence <id>]
  autonomy observe --goal <goal> --task <task> --observer <agent> --lead <agent> [--kind goal_proposal|graph_change_proposal|blocker|follow_up] [--summary <text>]
  autonomy plan-next --goal <goal> --task <task> --observer <agent> --lead <agent> [--summary <text>] [--proposal-summary <text>]
  autonomy decide --task <task> --lead <agent> --proposal <evidence> --decision <accept|reject|defer|request_evidence> --rationale <text> [--create-goal <goal> --goal-title <title> --goal-objective <text>] [--create-task <task> --task-title <title> --task-objective <text> --assignee <agent> --reviewer <agent>]
  autonomy tick --observer <agent> --lead <agent> --vision-ref <path>|--vision-summary <text> [--goal <goal>] [--auto-accept --assignee <agent> --reviewer <agent>] [--dry-run] [--max-new-goals <n>]
  autonomy loop --observer <agent> --lead <agent> --vision-ref <path>|--vision-summary <text> [--iterations <n>|--forever] [--interval-ms <ms>] [--auto-accept --assignee <agent> --reviewer <agent>] [--dry-run]
  dashboard snapshot
  board
  hook record --agent <agent> [--runtime <runtime>] [--task <task>]
  codex run --task <task> --agent <agent> --worktree <path> --prompt <text>
  codex review --task <task> --agent <agent> --worktree <path> [--base <branch>] [--uncommitted] [--prompt <text>]
  serve [--addr 127.0.0.1:8787] [--once]"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_content_length_frames_and_json_lines_without_duplicates() {
        let first = serde_json::json!({"jsonrpc": "2.0", "id": "a", "result": {"ok": true}});
        let second = serde_json::json!({"method": "turn/completed", "params": {"ok": true}});
        let mut framed = frame_jsonrpc_requests(&[first.clone(), second.clone()]).unwrap();
        framed.extend_from_slice(
            br#"
{"method":"item/agentMessage/delta","params":{"text":"done"}}
"#,
        );

        let values = extract_provider_json_values_from_bytes(&framed);
        assert_eq!(values.len(), 3);
        assert!(values.contains(&first));
        assert!(values.contains(&second));
    }

    #[test]
    fn extracts_thread_id_from_thread_start_response_before_turn_start() {
        let values = vec![serde_json::json!({
            "jsonrpc": "2.0",
            "id": "thread-rpc",
            "result": {"thread": {"id": "real-thread-1"}}
        })];

        assert_eq!(
            extract_thread_id(&values, "thread-rpc").as_deref(),
            Some("real-thread-1")
        );
        assert_eq!(extract_thread_id(&values, "other-rpc"), None);
    }

    #[test]
    fn detects_jsonrpc_error_messages() {
        let values = vec![serde_json::json!({
            "jsonrpc": "2.0",
            "id": "turn-rpc",
            "error": {"code": -32602, "message": "bad thread"}
        })];

        assert_eq!(jsonrpc_error_messages(&values), vec!["bad thread"]);
    }

    #[test]
    fn provider_exchange_errors_include_websocket_failures() {
        let errors = provider_exchange_error_messages(
            &[],
            &["timed out waiting for turn terminal event".into()],
        );

        assert_eq!(
            errors,
            vec!["timed out waiting for turn terminal event".to_string()]
        );
    }

    #[test]
    fn generated_ids_are_unique_inside_one_exchange() {
        let ids: BTreeSet<_> = (0..64).map(|_| generated_id("rpc")).collect();
        assert_eq!(ids.len(), 64);
    }

    #[test]
    fn turn_delivery_requires_turn_response_or_notification() {
        let initialize_only = vec![serde_json::json!({
            "jsonrpc": "2.0",
            "id": "initialize-rpc",
            "result": {"ok": true}
        })];
        assert!(!turn_exchange_confirms_turn_start(
            &initialize_only,
            "turn-rpc"
        ));

        let turn_response = vec![serde_json::json!({
            "jsonrpc": "2.0",
            "id": "turn-rpc",
            "result": {"ok": true}
        })];
        assert!(turn_exchange_confirms_turn_start(
            &turn_response,
            "turn-rpc"
        ));

        let turn_notification = vec![serde_json::json!({
            "method": "turn/started",
            "params": {"turnId": "turn-1"}
        })];
        assert!(turn_exchange_confirms_turn_start(
            &turn_notification,
            "turn-rpc"
        ));
    }

    #[test]
    fn accepted_turn_timeout_without_watcher_is_reported_stale() {
        let exchange = ProviderExchange {
            values: vec![serde_json::json!({
                "jsonrpc": "2.0",
                "id": "turn-rpc",
                "result": {"turn": {"id": "turn-1"}}
            })],
            stdout_ref: PathBuf::from("turn-start.stdout.jsonl"),
            stderr_ref: PathBuf::from("turn-start.stderr.log"),
            exit_code: Some(1),
            process_success: false,
            error_messages: vec!["timed out waiting for turn terminal event".into()],
        };

        let (status, summary) = classify_turn_exchange(&exchange, "turn-rpc");
        assert_eq!(status, ProviderSessionStatus::Stale);
        assert!(summary.contains("accepted"));
        assert!(summary.contains("timed out"));
    }

    #[test]
    fn accepted_turn_timeout_with_captured_terminal_is_succeeded() {
        let exchange = ProviderExchange {
            values: vec![
                serde_json::json!({
                    "method": "turn/completed",
                    "params": {"threadId": "thread-1", "turnId": "turn-1"}
                }),
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": "turn-rpc",
                    "result": {"turn": {"id": "turn-1"}}
                }),
            ],
            stdout_ref: PathBuf::from("turn-start.stdout.jsonl"),
            stderr_ref: PathBuf::from("turn-start.stderr.log"),
            exit_code: Some(1),
            process_success: false,
            error_messages: vec!["timed out waiting for turn terminal event".into()],
        };

        let (status, summary) = classify_turn_exchange(&exchange, "turn-rpc");
        assert_eq!(status, ProviderSessionStatus::Succeeded);
        assert!(summary.contains("terminal event"));
    }

    #[test]
    fn unconfirmed_turn_timeout_is_reported_failed() {
        let exchange = ProviderExchange {
            values: vec![serde_json::json!({
                "jsonrpc": "2.0",
                "id": "initialize-rpc",
                "result": {"ok": true}
            })],
            stdout_ref: PathBuf::from("turn-start.stdout.jsonl"),
            stderr_ref: PathBuf::from("turn-start.stderr.log"),
            exit_code: Some(1),
            process_success: false,
            error_messages: vec!["timed out waiting for turn terminal event".into()],
        };

        let (status, summary) = classify_turn_exchange(&exchange, "turn-rpc");
        assert_eq!(status, ProviderSessionStatus::Failed);
        assert!(summary.contains("failed"));
    }

    #[test]
    fn running_delivery_is_acknowledged_not_delivered() {
        assert_eq!(
            message_status_for_delivery(&ProviderSessionStatus::Running),
            MessageDeliveryStatus::Acknowledged
        );
        assert_eq!(
            message_status_for_delivery(&ProviderSessionStatus::Succeeded),
            MessageDeliveryStatus::Delivered
        );
        assert_eq!(
            message_status_for_delivery(&ProviderSessionStatus::Failed),
            MessageDeliveryStatus::Failed
        );
    }

    #[test]
    fn ingest_turn_completed_reconciles_running_delivery_session() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("reconcile")));
        let store = HarnessStore::new(&root);
        let mut member = make_member("agent-1");
        member.status = AgentMemberStatus::Running;
        member.current_task_id = Some("task-1".into());
        member.provider_runtime_id = Some("runtime-1".into());
        store.append_member(&member).expect("append member");
        store
            .append_runtime(&AgentRuntime {
                id: "runtime-1".into(),
                agent_member_id: "agent-1".into(),
                provider: "codex".into(),
                status: AgentRuntimeStatus::Running,
                pid: None,
                control_endpoint: Some("unix://test.sock".into()),
                command: "codex".into(),
                args: Vec::new(),
                started_at: "unix-ms:1".into(),
                ended_at: None,
                last_event_at: Some("unix-ms:1".into()),
                health: AgentRuntimeHealth {
                    process_alive: true,
                    socket_exists: true,
                    protocol_probe: Some("pass".into()),
                    delivery_probe: Some("pending: delivery accepted".into()),
                    checked_at: Some("unix-ms:1".into()),
                },
            })
            .expect("append runtime");
        store
            .append_message(&Message {
                id: "message-1".into(),
                task_id: Some("task-1".into()),
                from_agent_id: "lead-1".into(),
                to_agent_id: Some("agent-1".into()),
                channel: Some("assignment".into()),
                kind: MessageKind::Task,
                delivery_status: MessageDeliveryStatus::Acknowledged,
                content: "Do the task".into(),
                evidence_ids: Vec::new(),
                created_at: "unix-ms:1".into(),
                delivery: Some(MessageDelivery {
                    provider_session_id: Some("delivery-1".into()),
                    provider_request_id: None,
                    provider_thread_id: Some("thread-1".into()),
                    provider_turn_id: Some("turn-1".into()),
                    terminal_source: Some(MessageTerminalSource::Unknown),
                    delivered_at: Some("unix-ms:1".into()),
                    last_error: None,
                }),
            })
            .expect("append acknowledged assignment");
        let evidence = Evidence {
            id: "evidence-1".into(),
            task_id: Some("task-1".into()),
            source_type: "codex_delivery_session".into(),
            source_ref: root.display().to_string(),
            summary: "running delivery evidence".into(),
            created_at: "unix-ms:1".into(),
            evidence_kind: None,
            goal_id: None,
        };
        store.append_evidence(&evidence).expect("append evidence");
        store
            .append_provider_session(&ProviderSession {
                id: "delivery-1".into(),
                provider: "codex".into(),
                agent_member_id: "agent-1".into(),
                task_id: Some("task-1".into()),
                workspace_ref: None,
                provider_thread_id: Some("thread-1".into()),
                provider_turn_id: Some("turn-1".into()),
                terminal_source: Some(MessageTerminalSource::Unknown),
                status: ProviderSessionStatus::Running,
                command: "harness".into(),
                args: Vec::new(),
                prompt_ref: None,
                prompt_summary: None,
                provider_session_ref: None,
                stdout_ref: None,
                jsonl_ref: None,
                transcript_ref: None,
                last_message_ref: None,
                exit_code: None,
                started_at: "unix-ms:1".into(),
                ended_at: None,
                evidence_ids: vec!["evidence-1".into()],
            })
            .expect("append running provider session");
        let source = root.join("turn-completed.jsonl");
        std::fs::write(
            &source,
            r#"{"method":"turn/completed","params":{"threadId":"thread-1","turnId":"turn-1"}}"#,
        )
        .expect("write provider event");

        ingest_provider_output(
            &store,
            "agent-1",
            Some("runtime-1"),
            Some("task-1"),
            &source.display().to_string(),
        )
        .expect("ingest provider output");

        let sessions = store.provider_sessions().expect("provider sessions");
        let latest = sessions
            .iter()
            .rev()
            .find(|session| session.id == "delivery-1")
            .expect("reconciled session");
        assert_eq!(latest.status, ProviderSessionStatus::Succeeded);
        assert_eq!(
            latest.terminal_source,
            Some(MessageTerminalSource::TurnCompleted)
        );
        assert_eq!(latest.exit_code, Some(0));
        assert!(latest.ended_at.is_some());
        validate_provider_session_evidence(&store, &[evidence])
            .expect("gate should use latest reconciled session row");
        let latest_member = latest_member(&store, "agent-1").expect("latest member");
        assert_eq!(latest_member.status, AgentMemberStatus::Idle);
        assert_eq!(latest_member.current_task_id, None);
        let latest_runtime = latest_runtime(&store, "runtime-1")
            .expect("runtime lookup")
            .expect("latest runtime");
        assert_eq!(
            latest_runtime.health.delivery_probe.as_deref(),
            Some("pass: turn_completed")
        );
        let latest_message = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            MessageDeliveryStatus::Delivered
        );
        let delivery = latest_message.delivery.expect("message delivery");
        assert_eq!(
            delivery.terminal_source,
            Some(MessageTerminalSource::TurnCompleted)
        );
        assert_eq!(delivery.last_error, None);
        let reports: Vec<_> = store
            .messages()
            .expect("messages")
            .into_iter()
            .filter(|message| message.kind == MessageKind::Report)
            .filter(|message| message.channel.as_deref() == Some("provider-report"))
            .collect();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].delivery.as_ref().is_some_and(|delivery| {
            delivery.provider_session_id.as_deref() == Some("delivery-1")
        }));
        assert!(reports
            .iter()
            .any(|message| message.evidence_ids == vec!["evidence-1".to_string()]));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn taskless_running_delivery_reconciliation_clears_member_and_reports() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("direct")));
        let store = HarnessStore::new(&root);
        let mut member = make_member("agent-1");
        member.status = AgentMemberStatus::Running;
        member.current_task_id = None;
        store.append_member(&member).expect("append member");
        store
            .append_message(&Message {
                id: "message-1".into(),
                task_id: None,
                from_agent_id: "lead-1".into(),
                to_agent_id: Some("agent-1".into()),
                channel: Some("direct".into()),
                kind: MessageKind::Message,
                delivery_status: MessageDeliveryStatus::Acknowledged,
                content: "Direct message".into(),
                evidence_ids: Vec::new(),
                created_at: "unix-ms:1".into(),
                delivery: Some(MessageDelivery {
                    provider_session_id: Some("delivery-1".into()),
                    provider_request_id: None,
                    provider_thread_id: Some("thread-1".into()),
                    provider_turn_id: Some("turn-1".into()),
                    terminal_source: Some(MessageTerminalSource::Unknown),
                    delivered_at: Some("unix-ms:1".into()),
                    last_error: None,
                }),
            })
            .expect("append acknowledged message");
        store
            .append_provider_session(&ProviderSession {
                id: "delivery-1".into(),
                provider: "codex".into(),
                agent_member_id: "agent-1".into(),
                task_id: None,
                workspace_ref: None,
                provider_thread_id: Some("thread-1".into()),
                provider_turn_id: Some("turn-1".into()),
                terminal_source: Some(MessageTerminalSource::Unknown),
                status: ProviderSessionStatus::Running,
                command: "harness".into(),
                args: Vec::new(),
                prompt_ref: None,
                prompt_summary: None,
                provider_session_ref: None,
                stdout_ref: None,
                jsonl_ref: None,
                transcript_ref: None,
                last_message_ref: None,
                exit_code: None,
                started_at: "unix-ms:1".into(),
                ended_at: None,
                evidence_ids: vec!["evidence-1".into()],
            })
            .expect("append running provider session");

        reconcile_running_provider_sessions(
            &store,
            "agent-1",
            None,
            Some("thread-1"),
            Some("turn-1"),
            MessageTerminalSource::TurnCompleted,
        )
        .expect("reconcile taskless delivery");

        let latest_member = latest_member(&store, "agent-1").expect("latest member");
        assert_eq!(latest_member.status, AgentMemberStatus::Idle);
        assert_eq!(latest_member.current_task_id, None);
        let latest_message = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            MessageDeliveryStatus::Delivered
        );
        let report = store
            .messages()
            .expect("messages")
            .into_iter()
            .find(|message| {
                message.kind == MessageKind::Report
                    && message.channel.as_deref() == Some("provider-report")
                    && message.delivery.as_ref().is_some_and(|delivery| {
                        delivery.provider_session_id.as_deref() == Some("delivery-1")
                    })
            })
            .expect("provider report");
        assert_eq!(report.task_id, None);
        assert_eq!(report.evidence_ids, vec!["evidence-1".to_string()]);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn running_provider_session_blocks_more_delivery() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("block")));
        let store = HarnessStore::new(&root);
        store
            .append_provider_session(&ProviderSession {
                id: "delivery-1".into(),
                provider: "codex".into(),
                agent_member_id: "agent-1".into(),
                task_id: Some("task-1".into()),
                workspace_ref: None,
                provider_thread_id: Some("thread-1".into()),
                provider_turn_id: Some("turn-1".into()),
                terminal_source: Some(MessageTerminalSource::Unknown),
                status: ProviderSessionStatus::Running,
                command: "harness".into(),
                args: Vec::new(),
                prompt_ref: None,
                prompt_summary: None,
                provider_session_ref: None,
                stdout_ref: None,
                jsonl_ref: None,
                transcript_ref: None,
                last_message_ref: None,
                exit_code: None,
                started_at: "unix-ms:1".into(),
                ended_at: None,
                evidence_ids: vec!["evidence-1".into()],
            })
            .expect("append running provider session");

        assert!(has_unresolved_provider_session(&store, "agent-1").expect("running check"));

        mark_running_provider_sessions_terminal(
            &store,
            "agent-1",
            ProviderSessionStatus::Stale,
            Some(MessageTerminalSource::Failed),
        )
        .expect("mark stale");
        assert!(!has_unresolved_provider_session(&store, "agent-1").expect("running check"));
        let sessions = store.provider_sessions().expect("provider sessions");
        let latest = sessions
            .iter()
            .rev()
            .find(|session| session.id == "delivery-1")
            .expect("latest session");
        assert_eq!(latest.status, ProviderSessionStatus::Stale);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stale_unknown_provider_session_blocks_more_delivery() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("stale")));
        let store = HarnessStore::new(&root);
        store
            .append_provider_session(&ProviderSession {
                id: "delivery-1".into(),
                provider: "codex".into(),
                agent_member_id: "agent-1".into(),
                task_id: Some("task-1".into()),
                workspace_ref: None,
                provider_thread_id: Some("thread-1".into()),
                provider_turn_id: Some("turn-1".into()),
                terminal_source: Some(MessageTerminalSource::Unknown),
                status: ProviderSessionStatus::Stale,
                command: "harness".into(),
                args: Vec::new(),
                prompt_ref: None,
                prompt_summary: None,
                provider_session_ref: None,
                stdout_ref: None,
                jsonl_ref: None,
                transcript_ref: None,
                last_message_ref: None,
                exit_code: None,
                started_at: "unix-ms:1".into(),
                ended_at: None,
                evidence_ids: Vec::new(),
            })
            .expect("append stale provider session");

        assert!(has_unresolved_provider_session(&store, "agent-1").expect("running check"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stale_failed_provider_session_marks_message_failed_and_clears_member() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("stale-failed")));
        let store = HarnessStore::new(&root);
        let mut member = make_member("agent-1");
        member.status = AgentMemberStatus::Stale;
        member.current_task_id = Some("task-1".into());
        store.append_member(&member).expect("append member");
        store
            .append_message(&Message {
                id: "message-1".into(),
                task_id: Some("task-1".into()),
                from_agent_id: "lead-1".into(),
                to_agent_id: Some("agent-1".into()),
                channel: Some("assignment".into()),
                kind: MessageKind::Task,
                delivery_status: MessageDeliveryStatus::Acknowledged,
                content: "Do the task".into(),
                evidence_ids: Vec::new(),
                created_at: "unix-ms:1".into(),
                delivery: Some(MessageDelivery {
                    provider_session_id: Some("delivery-1".into()),
                    provider_request_id: None,
                    provider_thread_id: Some("thread-1".into()),
                    provider_turn_id: Some("turn-1".into()),
                    terminal_source: Some(MessageTerminalSource::Unknown),
                    delivered_at: Some("unix-ms:1".into()),
                    last_error: None,
                }),
            })
            .expect("append acknowledged message");
        store
            .append_provider_session(&ProviderSession {
                id: "delivery-1".into(),
                provider: "codex".into(),
                agent_member_id: "agent-1".into(),
                task_id: Some("task-1".into()),
                workspace_ref: None,
                provider_thread_id: Some("thread-1".into()),
                provider_turn_id: Some("turn-1".into()),
                terminal_source: Some(MessageTerminalSource::Unknown),
                status: ProviderSessionStatus::Stale,
                command: "harness".into(),
                args: Vec::new(),
                prompt_ref: None,
                prompt_summary: None,
                provider_session_ref: None,
                stdout_ref: None,
                jsonl_ref: None,
                transcript_ref: None,
                last_message_ref: None,
                exit_code: None,
                started_at: "unix-ms:1".into(),
                ended_at: None,
                evidence_ids: Vec::new(),
            })
            .expect("append stale provider session");

        mark_running_provider_sessions_terminal(
            &store,
            "agent-1",
            ProviderSessionStatus::Stale,
            Some(MessageTerminalSource::Failed),
        )
        .expect("mark stale failed");

        assert!(!has_unresolved_provider_session(&store, "agent-1").expect("running check"));
        let latest_message = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            MessageDeliveryStatus::Failed
        );
        let latest_member = latest_member(&store, "agent-1").expect("latest member");
        assert_eq!(latest_member.status, AgentMemberStatus::Idle);
        assert_eq!(latest_member.current_task_id, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn start_runtime_delivery_checks_running_session_before_spawning_runtime() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("guard")));
        let store = HarnessStore::new(&root);
        store
            .append_member(&make_member("agent-1"))
            .expect("append member");
        store
            .append_provider_session(&ProviderSession {
                id: "delivery-1".into(),
                provider: "codex".into(),
                agent_member_id: "agent-1".into(),
                task_id: Some("task-1".into()),
                workspace_ref: None,
                provider_thread_id: Some("thread-1".into()),
                provider_turn_id: Some("turn-1".into()),
                terminal_source: Some(MessageTerminalSource::Unknown),
                status: ProviderSessionStatus::Running,
                command: "harness".into(),
                args: Vec::new(),
                prompt_ref: None,
                prompt_summary: None,
                provider_session_ref: None,
                stdout_ref: None,
                jsonl_ref: None,
                transcript_ref: None,
                last_message_ref: None,
                exit_code: None,
                started_at: "unix-ms:1".into(),
                ended_at: None,
                evidence_ids: Vec::new(),
            })
            .expect("append running provider session");

        let result = deliver_agent_messages(
            &store,
            &["--agent".into(), "agent-1".into(), "--start-runtime".into()],
        );

        assert!(result.is_err());
        assert!(store.runtimes().expect("runtimes").is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thread_idle_without_turn_id_reconciles_single_running_session() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("idle")));
        let store = HarnessStore::new(&root);
        store
            .append_provider_session(&ProviderSession {
                id: "delivery-1".into(),
                provider: "codex".into(),
                agent_member_id: "agent-1".into(),
                task_id: Some("task-1".into()),
                workspace_ref: None,
                provider_thread_id: Some("thread-1".into()),
                provider_turn_id: Some("turn-1".into()),
                terminal_source: Some(MessageTerminalSource::Unknown),
                status: ProviderSessionStatus::Running,
                command: "harness".into(),
                args: Vec::new(),
                prompt_ref: None,
                prompt_summary: None,
                provider_session_ref: None,
                stdout_ref: None,
                jsonl_ref: None,
                transcript_ref: None,
                last_message_ref: None,
                exit_code: None,
                started_at: "unix-ms:1".into(),
                ended_at: None,
                evidence_ids: Vec::new(),
            })
            .expect("append running provider session");

        reconcile_running_provider_sessions(
            &store,
            "agent-1",
            Some("task-1"),
            Some("thread-1"),
            None,
            MessageTerminalSource::ThreadIdle,
        )
        .expect("thread idle should reconcile the active session");

        let latest = store
            .provider_sessions()
            .expect("provider sessions")
            .into_iter()
            .rev()
            .find(|session| session.id == "delivery-1")
            .expect("latest session");
        assert_eq!(latest.status, ProviderSessionStatus::Succeeded);
        assert_eq!(latest.provider_turn_id.as_deref(), Some("turn-1"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thread_idle_without_turn_id_is_terminal_source_for_active_stream() {
        let idle = serde_json::json!({
            "method": "thread/status/changed",
            "params": {
                "threadId": "thread-1",
                "status": {"type": "idle"}
            }
        });
        let idle_with_turn = serde_json::json!({
            "method": "thread/status/changed",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "status": {"type": "idle"}
            }
        });

        assert_eq!(
            terminal_source_from_values(&[idle]),
            Some(MessageTerminalSource::ThreadIdle)
        );
        assert_eq!(
            terminal_source_from_values(&[idle_with_turn]),
            Some(MessageTerminalSource::ThreadIdle)
        );
    }

    #[test]
    fn reconciliation_matches_when_stored_turn_id_is_missing() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("turnless")));
        let store = HarnessStore::new(&root);
        store
            .append_provider_session(&ProviderSession {
                id: "delivery-1".into(),
                provider: "codex".into(),
                agent_member_id: "agent-1".into(),
                task_id: Some("task-1".into()),
                workspace_ref: None,
                provider_thread_id: Some("thread-1".into()),
                provider_turn_id: None,
                terminal_source: Some(MessageTerminalSource::Unknown),
                status: ProviderSessionStatus::Running,
                command: "harness".into(),
                args: Vec::new(),
                prompt_ref: None,
                prompt_summary: None,
                provider_session_ref: None,
                stdout_ref: None,
                jsonl_ref: None,
                transcript_ref: None,
                last_message_ref: None,
                exit_code: None,
                started_at: "unix-ms:1".into(),
                ended_at: None,
                evidence_ids: Vec::new(),
            })
            .expect("append running provider session");

        reconcile_running_provider_sessions(
            &store,
            "agent-1",
            Some("task-1"),
            Some("thread-1"),
            Some("turn-1"),
            MessageTerminalSource::TurnCompleted,
        )
        .expect("reconcile session with missing stored turn id");

        let latest = store
            .provider_sessions()
            .expect("provider sessions")
            .into_iter()
            .rev()
            .find(|session| session.id == "delivery-1")
            .expect("latest session");
        assert_eq!(latest.status, ProviderSessionStatus::Succeeded);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dashboard_snapshot_uses_latest_provider_session_per_id() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("snapshot")));
        let store = HarnessStore::new(&root);
        let mut session = ProviderSession {
            id: "delivery-1".into(),
            provider: "codex".into(),
            agent_member_id: "agent-1".into(),
            task_id: Some("task-1".into()),
            workspace_ref: None,
            provider_thread_id: Some("thread-1".into()),
            provider_turn_id: Some("turn-1".into()),
            terminal_source: Some(MessageTerminalSource::Unknown),
            status: ProviderSessionStatus::Running,
            command: "harness".into(),
            args: Vec::new(),
            prompt_ref: None,
            prompt_summary: None,
            provider_session_ref: None,
            stdout_ref: None,
            jsonl_ref: None,
            transcript_ref: None,
            last_message_ref: None,
            exit_code: None,
            started_at: "unix-ms:1".into(),
            ended_at: None,
            evidence_ids: Vec::new(),
        };
        store
            .append_provider_session(&session)
            .expect("append running session");
        session.status = ProviderSessionStatus::Succeeded;
        session.terminal_source = Some(MessageTerminalSource::TurnCompleted);
        session.ended_at = Some("unix-ms:2".into());
        store
            .append_provider_session(&session)
            .expect("append succeeded session");

        let snapshot = dashboard_snapshot(&store).expect("dashboard snapshot");
        let sessions = snapshot
            .get("provider_sessions")
            .and_then(|value| value.as_array())
            .expect("provider sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            sessions[0].get("status").and_then(|value| value.as_str()),
            Some("succeeded")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dashboard_snapshot_uses_latest_message_per_id() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("messages")));
        let store = HarnessStore::new(&root);
        let mut message = Message {
            id: "message-1".into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some("agent-1".into()),
            channel: Some("assignment".into()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Assign task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
        };
        store
            .append_message(&message)
            .expect("append queued message");
        message.delivery_status = MessageDeliveryStatus::Acknowledged;
        store
            .append_message(&message)
            .expect("append acknowledged message");

        let snapshot = dashboard_snapshot(&store).expect("dashboard snapshot");
        let messages = snapshot
            .get("messages")
            .and_then(|value| value.as_array())
            .expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(
            messages[0]
                .get("delivery_status")
                .and_then(|value| value.as_str()),
            Some("acknowledged")
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delivery_queue_uses_latest_message_status_per_id() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("queue")));
        let store = HarnessStore::new(&root);
        store
            .append_member(&make_member("agent-1"))
            .expect("append member");
        let mut message = Message {
            id: "message-1".into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some("agent-1".into()),
            channel: Some("assignment".into()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Assign task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
        };
        store.append_message(&message).expect("append queued");
        message.delivery_status = MessageDeliveryStatus::Acknowledged;
        store.append_message(&message).expect("append acknowledged");

        deliver_agent_messages(
            &store,
            &["--agent".into(), "agent-1".into(), "--dry-run".into()],
        )
        .expect("deliver should not redeliver stale queued row");

        let latest = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(latest.delivery_status, MessageDeliveryStatus::Acknowledged);
        assert!(store
            .messages()
            .expect("messages")
            .iter()
            .all(|message| message.kind != MessageKind::Report));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dry_run_delivery_claims_and_finishes_provider_session() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("dry-claim")));
        let store = HarnessStore::new(&root);
        store
            .append_member(&make_member("agent-1"))
            .expect("append member");
        store
            .append_message(&Message {
                id: "message-1".into(),
                task_id: Some("task-1".into()),
                from_agent_id: "leader".into(),
                to_agent_id: Some("agent-1".into()),
                channel: Some("assignment".into()),
                kind: MessageKind::Task,
                delivery_status: MessageDeliveryStatus::Queued,
                content: "Assign task".into(),
                evidence_ids: Vec::new(),
                created_at: "unix-ms:1".into(),
                delivery: None,
            })
            .expect("append queued");

        deliver_agent_messages(
            &store,
            &["--agent".into(), "agent-1".into(), "--dry-run".into()],
        )
        .expect("dry-run delivery");

        let latest = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(latest.delivery_status, MessageDeliveryStatus::Delivered);
        let delivery = latest.delivery.expect("delivery");
        let session_id = delivery
            .provider_session_id
            .expect("claimed provider session id");
        assert_eq!(
            delivery.terminal_source,
            Some(MessageTerminalSource::DryRun)
        );

        let session = latest_provider_session(&store, &session_id)
            .expect("session lookup")
            .expect("provider session");
        assert_eq!(session.status, ProviderSessionStatus::Succeeded);
        assert_eq!(session.terminal_source, Some(MessageTerminalSource::DryRun));
        assert!(!session.evidence_ids.is_empty());

        let reports: Vec<_> = store
            .messages()
            .expect("messages")
            .into_iter()
            .filter(|message| message.kind == MessageKind::Report)
            .collect();
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].evidence_ids, session.evidence_ids);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn retry_delivery_requeues_safe_claim_without_provider_request() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("retry")));
        let store = HarnessStore::new(&root);
        let member = make_member("agent-1");
        store.append_member(&member).expect("append member");
        let message = Message {
            id: "message-1".into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some("agent-1".into()),
            channel: Some("assignment".into()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Assign task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
        };
        store.append_message(&message).expect("append queued");
        claim_message_for_delivery(&store, &member, None, &message, "delivery-1")
            .expect("claim")
            .expect("claimed message");

        retry_delivery_value(
            &store,
            "agent-1",
            "message-1",
            Some("delivery-1"),
            "safe retry test",
            false,
        )
        .expect("retry delivery");

        let latest_message = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            MessageDeliveryStatus::Queued
        );
        assert!(latest_message.delivery.is_none());
        let latest_session = latest_provider_session(&store, "delivery-1")
            .expect("session lookup")
            .expect("session");
        assert_eq!(latest_session.status, ProviderSessionStatus::Canceled);
        assert_eq!(
            latest_session.terminal_source,
            Some(MessageTerminalSource::Failed)
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn gateway_expires_safe_pre_provider_claims() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("expire")));
        let store = HarnessStore::new(&root);
        let member = make_member("agent-1");
        store.append_member(&member).expect("append member");
        let message = Message {
            id: "message-1".into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some("agent-1".into()),
            channel: Some("assignment".into()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Assign task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
        };
        store.append_message(&message).expect("append queued");
        claim_message_for_delivery(&store, &member, None, &message, "delivery-1")
            .expect("claim")
            .expect("claimed message");
        let mut old_session = latest_provider_session(&store, "delivery-1")
            .expect("session lookup")
            .expect("session");
        old_session.started_at = "unix-ms:1".into();
        store
            .append_provider_session(&old_session)
            .expect("append old session");

        let result = provider_gateway_tick_value(
            &store,
            GatewayOptions {
                dry_run: false,
                start_runtime: false,
                timeout_ms: 100,
                claim_ttl_ms: 1,
            },
        )
        .expect("gateway tick");

        assert_eq!(result["expired_claims"].as_array().map(Vec::len), Some(1));
        let latest_message = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(
            latest_message.delivery_status,
            MessageDeliveryStatus::Failed
        );
        let sessions = latest_provider_sessions_in_append_order(&store).expect("sessions");
        assert!(sessions.iter().any(|session| {
            session.id == "delivery-1" && session.status == ProviderSessionStatus::Canceled
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn gateway_tick_delivers_queued_messages_with_same_delivery_path() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("gateway")));
        let store = HarnessStore::new(&root);
        store
            .append_member(&make_member("agent-1"))
            .expect("append member 1");
        store
            .append_member(&make_member("agent-2"))
            .expect("append member 2");
        for agent_id in ["agent-1", "agent-2"] {
            store
                .append_message(&Message {
                    id: format!("message-{agent_id}"),
                    task_id: Some(format!("task-{agent_id}")),
                    from_agent_id: "leader".into(),
                    to_agent_id: Some(agent_id.into()),
                    channel: Some("assignment".into()),
                    kind: MessageKind::Task,
                    delivery_status: MessageDeliveryStatus::Queued,
                    content: "Assign task".into(),
                    evidence_ids: Vec::new(),
                    created_at: "unix-ms:1".into(),
                    delivery: None,
                })
                .expect("append queued");
        }

        let result = provider_gateway_tick_value(
            &store,
            GatewayOptions {
                dry_run: true,
                start_runtime: false,
                timeout_ms: 100,
                claim_ttl_ms: 300_000,
            },
        )
        .expect("gateway tick");

        assert_eq!(result["agent_count"].as_u64(), Some(2));
        for agent_id in ["agent-1", "agent-2"] {
            let latest =
                latest_message(&store, &format!("message-{agent_id}")).expect("latest message");
            assert_eq!(latest.delivery_status, MessageDeliveryStatus::Delivered);
            assert!(latest
                .delivery
                .and_then(|delivery| delivery.provider_session_id)
                .is_some());
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn http_action_dispatches_control_plane_safe_actions() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("http-action")));
        let store = HarnessStore::new(&root);
        store
            .append_goal(&make_goal("goal-1"))
            .expect("append goal");
        store
            .append_member(&make_member("worker"))
            .expect("append worker");
        store
            .append_member(&make_member("critic"))
            .expect("append critic");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("append task");

        let message = handle_http_action(
            &store,
            "/v1/messages",
            &serde_json::json!({
                "from_agent_id": "leader",
                "to_agent_id": "worker",
                "task_id": "task-1",
                "kind": "message",
                "content": "please inspect"
            }),
        )
        .expect("message action");
        let message_id = message
            .get("id")
            .and_then(|value| value.as_str())
            .expect("message id");
        let latest = latest_message(&store, message_id).expect("latest message");
        assert_eq!(latest.to_agent_id.as_deref(), Some("worker"));
        assert_eq!(latest.delivery_status, MessageDeliveryStatus::Queued);

        let review = handle_http_action(
            &store,
            "/v1/tasks/task-1/request-review",
            &serde_json::json!({
                "from_agent_id": "leader",
                "content": "please review"
            }),
        )
        .expect("request review action");
        assert_eq!(
            latest_task(&store, "task-1").expect("latest task").status,
            TaskStatus::Review
        );
        assert_eq!(
            review
                .get("message")
                .and_then(|value| value.get("to_agent_id"))
                .and_then(|value| value.as_str()),
            Some("critic")
        );

        handle_http_action(&store, "/v1/agents/worker/close", &serde_json::json!({}))
            .expect("close worker");
        assert_eq!(
            latest_member(&store, "worker")
                .expect("latest worker")
                .status,
            AgentMemberStatus::Closed
        );
        let closed_send = handle_http_action(
            &store,
            "/v1/messages",
            &serde_json::json!({
                "from_agent_id": "leader",
                "to_agent_id": "worker",
                "content": "should fail"
            }),
        );
        assert!(closed_send.is_err());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closed_member_rejects_delivery_without_claiming_message() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("closed")));
        let store = HarnessStore::new(&root);
        let mut member = make_member("agent-1");
        member.status = AgentMemberStatus::Closed;
        store.append_member(&member).expect("append member");
        store
            .append_message(&Message {
                id: "message-1".into(),
                task_id: Some("task-1".into()),
                from_agent_id: "leader".into(),
                to_agent_id: Some("agent-1".into()),
                channel: Some("assignment".into()),
                kind: MessageKind::Task,
                delivery_status: MessageDeliveryStatus::Queued,
                content: "Assign task".into(),
                evidence_ids: Vec::new(),
                created_at: "unix-ms:1".into(),
                delivery: None,
            })
            .expect("append queued");

        let result = deliver_agent_messages(
            &store,
            &["--agent".into(), "agent-1".into(), "--dry-run".into()],
        );

        assert!(result.is_err());
        let latest = latest_message(&store, "message-1").expect("latest message");
        assert_eq!(latest.delivery_status, MessageDeliveryStatus::Queued);
        assert!(latest.delivery.is_none());
        assert!(store.provider_sessions().expect("sessions").is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thread_start_uses_prompt_file_contents() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("prompt")));
        std::fs::create_dir_all(&root).expect("create temp prompt dir");
        let prompt_path = root.join("agent.md");
        std::fs::write(&prompt_path, "Prompt file contents").expect("write prompt");
        let mut member = make_member("agent-1");
        member.prompt_ref = Some(prompt_path.display().to_string());

        let request = build_thread_start_request(&member, "rpc-1");
        assert_eq!(
            request
                .get("params")
                .and_then(|params| params.get("developerInstructions"))
                .and_then(|value| value.as_str()),
            Some("Prompt file contents")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn turn_input_uses_stable_harness_envelope() {
        let message = Message {
            id: "message-1".into(),
            task_id: Some("task-1".into()),
            from_agent_id: "leader".into(),
            to_agent_id: Some("agent-1".into()),
            channel: Some("assignment".into()),
            kind: MessageKind::Task,
            delivery_status: MessageDeliveryStatus::Acknowledged,
            content: "Do the task".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
        };

        let input = build_turn_input(&message, "delivery-1");
        let text = input[0]["text"].as_str().expect("turn text");

        assert!(text.contains("message_id: message-1"));
        assert!(text.contains("kind: task"));
        assert!(text.contains("task_id: task-1"));
        assert!(text.contains("from_agent_id: leader"));
        assert!(text.contains("to_agent_id: agent-1"));
        assert!(text.contains("channel: assignment"));
        assert!(text.contains("delivery_attempt: delivery-1"));
        assert!(text.contains("content:\nDo the task"));
        assert!(!text.contains("kind: Task"));
    }

    #[test]
    fn acceptance_evidence_rejects_failed_checks() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let items = vec![
            make_evidence("check_failed", Some("task-1")),
            make_evidence("critic_findings", Some("task-1")),
            make_evidence("worker_report", Some("task-1")),
        ];

        let error = validate_acceptance_evidence(&store, &items, false, false, false)
            .expect_err("failed checks must block acceptance");
        assert!(error.to_string().contains("failed check"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn acceptance_evidence_requires_check_critic_and_provider_output() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let items = vec![make_evidence("check_passed", Some("task-1"))];

        let error = validate_acceptance_evidence(&store, &items, false, false, false)
            .expect_err("critic findings are required");
        assert!(error.to_string().contains("critic_findings"));

        let items = vec![
            make_evidence("check_passed", Some("task-1")),
            make_evidence("critic_findings", Some("task-1")),
            make_evidence("worker_report", Some("task-1")),
        ];
        validate_acceptance_evidence(&store, &items, false, false, false)
            .expect("complete evidence bundle should pass");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn source_refs_for_required_types_must_exist() {
        let mut item = make_evidence("critic_findings", Some("task-1"));
        item.source_ref = "/definitely/missing/harness/source/ref".into();
        let error = validate_review_evidence_sources(&[item])
            .expect_err("missing source ref must be rejected");
        assert!(error.to_string().contains("source_ref does not exist"));
    }

    #[test]
    fn goal_learning_status_reports_complete_chain() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let goal = make_goal("goal-1");
        let task = make_task("task-1", "goal-1");
        store.append_goal(&goal).expect("append goal");
        store.append_task(&task).expect("append task");
        let mut follow_up = make_task("follow-up-task", "goal-2");
        follow_up.parent_task_id = Some("task-1".into());
        follow_up.title = "Follow-up: add goal commands".into();
        store
            .append_task(&follow_up)
            .expect("append follow-up task");
        store
            .append_evidence(&make_timed_evidence(
                "design",
                "goal_design",
                Some("task-1"),
                "unix-ms:100",
            ))
            .expect("append design");
        store
            .append_message(&make_timed_message(
                "assign",
                MessageKind::Task,
                "leader",
                Some("worker"),
                "task-1",
                "unix-ms:110",
            ))
            .expect("append assignment");
        store
            .append_message(&make_timed_message(
                "report",
                MessageKind::Report,
                "worker",
                Some("leader"),
                "task-1",
                "unix-ms:120",
            ))
            .expect("append report");
        store
            .append_decision(&make_timed_decision("decision", "task-1", "unix-ms:130"))
            .expect("append decision");
        store
            .append_evidence(&make_timed_evidence(
                "evaluation",
                "goal_evaluation",
                Some("task-1"),
                "unix-ms:140",
            ))
            .expect("append evaluation");
        store
            .append_evidence(&make_timed_evidence(
                "case",
                "goal_case",
                Some("task-1"),
                "unix-ms:150",
            ))
            .expect("append case");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(status.warnings(true).is_empty());
        assert_eq!(status.goal_cases.len(), 1);
        assert_eq!(status.follow_up_tasks.len(), 1);
        status
            .require_for_gate(&store, true, false, None)
            .expect("complete chain should pass");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn goal_learning_status_dual_reads_graduated_objects() {
        // The design/evaluation gates must pass when the artifacts are graduated
        // first-class objects (GoalDesign/GoalEvaluation) instead of legacy
        // Evidence rows — proving the union-by-goal_id dual-read with no backfill.
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let goal = make_goal("goal-1");
        let task = make_task("task-1", "goal-1");
        store.append_goal(&goal).expect("append goal");
        store.append_task(&task).expect("append task");
        store
            .append_goal_design(&GoalDesign {
                id: "design-1".into(),
                goal_id: "goal-1".into(),
                scenario_summary: "Render learning layer.".into(),
                non_goals: vec![],
                risk_and_permission_boundaries: "Read-only.".into(),
                required_infra: vec![],
                agent_team: None,
                task_graph: vec!["task-1".into()],
                evidence_plan: vec![],
                acceptance_gates: vec!["cargo test".into()],
                created_at: "unix-ms:100".into(),
            })
            .expect("append goal design object");
        store
            .append_message(&make_timed_message(
                "assign",
                MessageKind::Task,
                "leader",
                Some("worker"),
                "task-1",
                "unix-ms:110",
            ))
            .expect("append assignment");
        store
            .append_message(&make_timed_message(
                "report",
                MessageKind::Report,
                "worker",
                Some("leader"),
                "task-1",
                "unix-ms:120",
            ))
            .expect("append report");
        store
            .append_decision(&make_timed_decision("decision", "task-1", "unix-ms:130"))
            .expect("append decision");
        // The critic/evaluator-evidence warning still needs a critic row; supply it.
        store
            .append_evidence(&make_timed_evidence(
                "critic",
                "critic_findings",
                Some("task-1"),
                "unix-ms:135",
            ))
            .expect("append critic evidence");
        store
            .append_goal_evaluation(&GoalEvaluation {
                id: "eval-1".into(),
                goal_id: "goal-1".into(),
                evaluator_agent_id: "evaluator".into(),
                outcome: EvaluationOutcome::Success,
                what_worked: "Dual-read.".into(),
                what_failed: "None.".into(),
                missing_infra: vec![],
                missing_evidence: vec![],
                team_design_feedback: "ok".into(),
                task_graph_feedback: "ok".into(),
                dashboard_feedback: "ok".into(),
                reusable_patterns: vec![],
                anti_patterns: vec![],
                follow_up_task_ids: vec![],
                proposed_goal_ids: vec![],
                created_at: "unix-ms:140".into(),
            })
            .expect("append goal evaluation object");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        // No legacy evidence rows for design/evaluation.
        assert!(status.goal_design.is_empty());
        assert!(status.goal_evaluation.is_empty());
        // Graduated objects are surfaced and satisfy the gate.
        assert_eq!(status.goal_design_objects.len(), 1);
        assert_eq!(status.goal_evaluation_objects.len(), 1);
        assert!(status.has_goal_design());
        assert!(status.has_goal_evaluation());
        assert!(
            status.warnings(true).is_empty(),
            "graduated objects must satisfy the gate, got: {:?}",
            status.warnings(true)
        );
        status
            .require_for_gate(&store, true, false, None)
            .expect("dual-read chain should pass the strict gate");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn goal_learning_status_rejects_missing_evaluation_when_required() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let goal = make_goal("goal-1");
        let task = make_task("task-1", "goal-1");
        store.append_goal(&goal).expect("append goal");
        store.append_task(&task).expect("append task");
        store
            .append_evidence(&make_timed_evidence(
                "design",
                "goal_design",
                Some("task-1"),
                "unix-ms:100",
            ))
            .expect("append design");
        store
            .append_message(&make_timed_message(
                "assign",
                MessageKind::Task,
                "leader",
                Some("worker"),
                "task-1",
                "unix-ms:110",
            ))
            .expect("append assignment");
        store
            .append_message(&make_timed_message(
                "report",
                MessageKind::Report,
                "worker",
                Some("leader"),
                "task-1",
                "unix-ms:120",
            ))
            .expect("append report");
        store
            .append_decision(&make_timed_decision("decision", "task-1", "unix-ms:130"))
            .expect("append decision");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        let error = status
            .require_for_gate(&store, true, false, None)
            .expect_err("missing evaluation must fail strict gate");
        assert!(error.to_string().contains("goal_evaluation"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn goal_learning_waiver_requires_evidence_owner_and_follow_up_task() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let goal = make_goal("goal-1");
        let task = make_task("task-1", "goal-1");
        let follow_up = make_task("follow-up-task", "goal-1");
        store.append_goal(&goal).expect("append goal");
        store.append_task(&task).expect("append task");
        store
            .append_task(&follow_up)
            .expect("append follow-up task");
        store
            .append_evidence(&Evidence {
                id: "waiver-evidence".into(),
                task_id: Some("task-1".into()),
                source_type: "worker_report".into(),
                source_ref: std::env::temp_dir().display().to_string(),
                summary: "waiver evidence".into(),
                created_at: "unix-ms:100".into(),
                evidence_kind: None,
                goal_id: None,
            })
            .expect("append evidence");
        store
            .append_decision(&Decision {
                id: "bad-waiver".into(),
                task_id: "task-1".into(),
                decision: "waiver".into(),
                rationale: "skip design for now".into(),
                evidence_ids: vec!["waiver-evidence".into()],
                created_at: "unix-ms:110".into(),
                decision_kind: Some("waiver".into()),
                goal_id: None,
                is_waiver: true,
                follow_up_task_id: None,
            })
            .expect("append bad waiver");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        let error = status
            .require_for_gate(&store, true, true, Some("bad-waiver"))
            .expect_err("waiver without follow-up task must fail");
        assert!(error.to_string().contains("follow-up task"));

        store
            .append_decision(&Decision {
                id: "good-waiver".into(),
                task_id: "task-1".into(),
                decision: "waiver".into(),
                rationale: "temporary waiver; follow-up task follow-up-task will produce GoalDesign/GoalEvaluation evidence".into(),
                evidence_ids: vec!["waiver-evidence".into()],
                created_at: "unix-ms:120".into(),
                decision_kind: Some("waiver".into()),
                goal_id: None,
                is_waiver: true,
                follow_up_task_id: Some("follow-up-task".into()),
            })
            .expect("append good waiver");
        let status = goal_learning_status(&store, "goal-1").expect("status");
        status
            .require_for_gate(&store, true, true, Some("good-waiver"))
            .expect("valid waiver should pass when explicitly selected");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn review_gate_rejects_missing_goal_evaluation_when_required() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let goal = make_goal("goal-1");
        let task = make_task("task-1", "goal-1");
        store.append_goal(&goal).expect("append goal");
        store.append_task(&task).expect("append task");
        store
            .append_evidence(&make_timed_evidence(
                "design",
                "goal_design",
                Some("task-1"),
                "unix-ms:100",
            ))
            .expect("append design");
        store
            .append_message(&make_timed_message(
                "assign",
                MessageKind::Task,
                "leader",
                Some("worker"),
                "task-1",
                "unix-ms:110",
            ))
            .expect("append assignment");
        store
            .append_message(&make_timed_message(
                "report",
                MessageKind::Report,
                "worker",
                Some("leader"),
                "task-1",
                "unix-ms:120",
            ))
            .expect("append report");
        for evidence in [
            make_timed_evidence("check", "check_passed", Some("task-1"), "unix-ms:121"),
            make_timed_evidence("critic", "critic_findings", Some("task-1"), "unix-ms:122"),
            make_timed_evidence("worker", "worker_report", Some("task-1"), "unix-ms:123"),
        ] {
            store.append_evidence(&evidence).expect("append evidence");
        }
        store
            .append_proposal(&make_proposal("proposal-1", "task-1"))
            .expect("append proposal");

        let args = strings(&[
            "gate",
            "--task",
            "task-1",
            "--reviewer",
            "critic",
            "--decision",
            "accept",
            "--rationale",
            "test",
            "--evidence",
            "check",
            "--evidence",
            "critic",
            "--evidence",
            "worker",
            "--require-goal-design",
            "--require-goal-evaluation",
        ]);
        let error = review_gate(&store, &args).expect_err("missing evaluation must block");
        assert!(error.to_string().contains("goal_evaluation"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn review_create_persists_structured_verdict() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);

        let args = strings(&[
            "create",
            "--id",
            "review-1",
            "--task",
            "task-1",
            "--goal",
            "goal-1",
            "--reviewer",
            "critic",
            "--kind",
            "acceptance",
            "--verdict",
            "pass",
            "--summary",
            "Acceptance gates met.",
            "--blocker",
            "none",
            "--missing-validation",
            "load test deferred",
            "--evidence",
            "ev-1",
        ]);
        review_command(&store, &args).expect("create review");

        let reviews = store.reviews().expect("read reviews");
        assert_eq!(reviews.len(), 1);
        let review = &reviews[0];
        assert_eq!(review.id, "review-1");
        assert_eq!(review.verdict, ReviewVerdict::Pass);
        assert_eq!(review.task_id.as_deref(), Some("task-1"));
        assert_eq!(review.goal_id.as_deref(), Some("goal-1"));
        assert_eq!(review.evidence_ids, vec!["ev-1".to_string()]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn review_create_requires_task_or_goal() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);

        let args = strings(&[
            "create",
            "--reviewer",
            "critic",
            "--kind",
            "acceptance",
            "--verdict",
            "pass",
            "--summary",
            "Detached review.",
        ]);
        let error = review_command(&store, &args).expect_err("review without scope must fail");
        assert!(error.to_string().contains("--task or --goal"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn task_assign_rejects_missing_goal_design_by_default() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let goal = make_goal("goal-1");
        let task = make_task("task-1", "goal-1");
        store.append_goal(&goal).expect("append goal");
        store.append_task(&task).expect("append task");

        let args = strings(&["assign", "--id", "task-1", "--assignee", "worker"]);
        let error = task_command(&store, &args).expect_err("missing design must block assignment");
        assert!(error.to_string().contains("goal_design"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn task_assign_requires_explicit_waiver_decision_for_missing_goal_design() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        let goal = make_goal("goal-1");
        let task = make_task("task-1", "goal-1");
        store.append_goal(&goal).expect("append goal");
        store.append_task(&task).expect("append task");

        let args = strings(&[
            "assign",
            "--id",
            "task-1",
            "--assignee",
            "worker",
            "--allow-missing-goal-design",
        ]);
        let error = task_command(&store, &args).expect_err("bare waiver flag must fail");
        assert!(error.to_string().contains("--waiver-decision"));
        let _ = std::fs::remove_dir_all(root);
    }

    fn make_evidence(source_type: &str, task_id: Option<&str>) -> Evidence {
        Evidence {
            id: generated_id("evidence"),
            task_id: task_id.map(str::to_string),
            source_type: source_type.into(),
            source_ref: std::env::temp_dir().display().to_string(),
            summary: "test evidence".into(),
            created_at: now_string(),
            evidence_kind: None,
            goal_id: None,
        }
    }

    fn make_timed_evidence(
        id: &str,
        source_type: &str,
        task_id: Option<&str>,
        created_at: &str,
    ) -> Evidence {
        Evidence {
            id: id.into(),
            task_id: task_id.map(str::to_string),
            source_type: source_type.into(),
            source_ref: std::env::temp_dir().display().to_string(),
            summary: format!("{source_type} evidence"),
            created_at: created_at.into(),
            evidence_kind: None,
            goal_id: None,
        }
    }

    fn make_goal(id: &str) -> Goal {
        Goal {
            id: id.into(),
            title: "Goal".into(),
            objective: "Test goal".into(),
            owner_agent_id: "leader".into(),
            status: GoalStatus::Active,
            success_criteria: vec!["pass".into()],
            priority: "p0".into(),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            vision_id: None,
            goal_design_id: None,
            closed_by_decision_id: None,
        }
    }

    fn make_task(id: &str, goal_id: &str) -> Task {
        Task {
            id: id.into(),
            goal_id: Some(goal_id.into()),
            parent_task_id: None,
            title: "Task".into(),
            objective: "Test task".into(),
            owner_agent_id: "leader".into(),
            assignee_agent_id: Some("worker".into()),
            reviewer_agent_id: Some("critic".into()),
            status: TaskStatus::Assigned,
            depends_on_task_ids: Vec::new(),
            workspace_ref: None,
            branch_ref: None,
            pr_ref: None,
            owned_paths: Vec::new(),
            acceptance_criteria: Vec::new(),
            created_at: "unix-ms:10".into(),
            updated_at: "unix-ms:10".into(),
            phase: None,
            scope_refs: Vec::new(),
            requires_human_approval: false,
            verdict_decision_id: None,
        }
    }

    fn make_member(id: &str) -> AgentMember {
        AgentMember {
            id: id.into(),
            name: "Member".into(),
            description: "Test member".into(),
            role: "worker".into(),
            provider: "codex".into(),
            model: None,
            profile: None,
            provider_config: AgentProviderConfig::default(),
            capabilities: Vec::new(),
            team_ids: Vec::new(),
            prompt_ref: None,
            skill_refs: Vec::new(),
            workspace_policy: None,
            worktree_ref: None,
            permission_profile: None,
            runtime_workspace_roots: Vec::new(),
            status: AgentMemberStatus::Idle,
            current_task_id: None,
            current_proposal_id: None,
            provider_runtime_id: None,
            provider_thread_id: None,
            provider_agent_path: None,
            provider_agent_nickname: None,
            provider_agent_role: None,
            control_endpoint: None,
            created_at: "unix-ms:1".into(),
            last_seen_at: None,
        }
    }

    fn make_timed_message(
        id: &str,
        kind: MessageKind,
        from: &str,
        to: Option<&str>,
        task_id: &str,
        created_at: &str,
    ) -> Message {
        Message {
            id: id.into(),
            task_id: Some(task_id.into()),
            from_agent_id: from.into(),
            to_agent_id: to.map(str::to_string),
            channel: Some("test".into()),
            kind,
            delivery_status: MessageDeliveryStatus::Delivered,
            content: "test message".into(),
            evidence_ids: Vec::new(),
            created_at: created_at.into(),
            delivery: None,
        }
    }

    fn make_timed_decision(id: &str, task_id: &str, created_at: &str) -> Decision {
        Decision {
            id: id.into(),
            task_id: task_id.into(),
            decision: "accepted".into(),
            rationale: "test".into(),
            evidence_ids: Vec::new(),
            created_at: created_at.into(),
            decision_kind: None,
            goal_id: None,
            is_waiver: false,
            follow_up_task_id: None,
        }
    }

    fn make_proposal(id: &str, task_id: &str) -> Proposal {
        Proposal {
            id: id.into(),
            task_id: task_id.into(),
            agent_member_id: "worker".into(),
            title: "Proposal".into(),
            summary: "Test proposal".into(),
            status: ProposalStatus::Submitted,
            changed_paths: Vec::new(),
            evidence_ids: vec!["check".into()],
            created_at: "unix-ms:124".into(),
            updated_at: "unix-ms:124".into(),
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }
}

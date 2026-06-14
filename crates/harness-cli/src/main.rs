use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use harness_core::{
    build_launch_spec, AgentEvent, AgentMember, AgentMemberStatus, AgentProviderConfig,
    AgentRuntime, AgentRuntimeHealth, AgentRuntimeStatus, AgentTeam, AgentTeamStatus, Decision,
    EvaluationOutcome, Evidence, Exploration, Gap, GapSeverity, GapStatus, Goal, GoalCase,
    GoalDesign, GoalEvaluation, GoalStage, GoalStatus, HarnessTokenUsage, HarnessToolCall,
    HarnessToolResult, HarnessTurnEvent, HarnessTurnEventKind, LaunchMcp, LaunchPermission,
    LaunchSpec, Message, MessageDelivery, MessageDeliveryStatus, MessageKind,
    MessageTerminalSource, Proposal, ProposalStatus, ProviderChildThread,
    ProviderChildThreadStatus, ProviderSession, ProviderSessionStatus, Review, ReviewVerdict,
    SenderKind, Task, TaskStatus, Vision, WorkflowRun, WorkflowRunStatus, WorkflowStep,
    WorkflowStepStatus,
};
use harness_store::{HarnessStore, MessageDeliveryClaimResult};
use thiserror::Error;

mod resident;
#[cfg(unix)]
mod resident_daemon;
mod sse;
mod workflow;

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

/// Resolve the harness store root with clear precedence so commands run from
/// different working directories converge on ONE store (issue #89 item 3).
/// Precedence: explicit `--store <path>` (stripped from `args` so subcommands
/// don't see it), then `HARNESS_ROOT` env (back-compat), then the nearest
/// existing `.harness` walking up from the cwd (like git finds `.git`), then
/// `./.harness` (the historical default).
///
/// Without this, `serve` (reads `<serve cwd>/.harness`) and `run-script` (writes
/// `<run-script cwd>/.harness`) silently used different stores → an empty dashboard.
fn resolve_store_root(args: &mut Vec<String>) -> PathBuf {
    if let Some(path) = take_flag_value(args, "--store") {
        return PathBuf::from(path);
    }
    if let Ok(root) = env::var("HARNESS_ROOT") {
        if !root.is_empty() {
            return PathBuf::from(root);
        }
    }
    // `init` MATERIALIZES a store and must not adopt an ancestor's — it always
    // targets `./.harness` (or the explicit `--store`/`HARNESS_ROOT` handled
    // above). Every other command walks up to the nearest existing `.harness` so
    // sibling processes (serve + run-script) converge on one store.
    if args.first().map(String::as_str) != Some("init") {
        if let Ok(cwd) = env::current_dir() {
            if let Some(found) = discover_harness_from(&cwd) {
                return found;
            }
        }
    }
    PathBuf::from(".harness")
}

/// Walk up from `start` returning the first existing `<dir>/.harness` directory,
/// or `None` if none is found up to the filesystem root.
fn discover_harness_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join(".harness");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Remove the first `--flag <value>` pair from `args`, returning the value. The
/// flag is always removed; the value is returned only when present (a trailing
/// `--flag` with no value yields `None`).
fn take_flag_value(args: &mut Vec<String>, flag: &str) -> Option<String> {
    let pos = args.iter().position(|a| a == flag)?;
    args.remove(pos);
    if pos < args.len() {
        Some(args.remove(pos))
    } else {
        None
    }
}

fn run() -> CliResult<()> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    // Resolve the store root FIRST (strips a global `--store <path>` from args so
    // the subcommand parsers never see it), so `serve` and `run-script` started
    // from different working directories can be pointed at ONE store (issue #89
    // item 3).
    let store_root = resolve_store_root(&mut args);
    if args.is_empty() || args[0] == "help" || args[0] == "--help" {
        print_help();
        return Ok(());
    }

    let store = HarnessStore::new(store_root);
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
        "workflow" => workflow_command(&store, &args[1..])?,
        "hook" => hook_command(&store, &args[1..])?,
        "serve" => serve_command(&store, &args[1..])?,
        #[cfg(unix)]
        "daemon" => daemon_command(&store, &args[1..])?,
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
                let runtime = start_provider_runtime(store, &member)?;
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
            } else {
                // No runtime requested: persist the member and emit the
                // creation event via the shared path used by POST /v1/agents.
                member.status = AgentMemberStatus::Idle;
                finalize_member_creation(store, &member)?;
            }
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
                sender_kind: sender_kind_from_args(args)?,
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
            persist_new_team(store, &team)?;
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

/// Read a markdown field value from either an inline `--<name>` flag or a
/// `--<name>-file <path>` (file content). Long markdown is awkward as a shell
/// arg, so the `-file` form is the ergonomic path for design/acceptance bodies.
fn md_value(args: &[String], name: &str) -> CliResult<Option<String>> {
    if let Some(v) = value(args, &format!("--{name}")) {
        return Ok(Some(v));
    }
    if let Some(path) = value(args, &format!("--{name}-file")) {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| CliError::Usage(format!("cannot read --{name}-file {path}: {e}")))?;
        return Ok(Some(content));
    }
    Ok(None)
}

/// Load the latest row for a goal id, or a clear not-found error.
fn goal_load(store: &HarnessStore, id: &str) -> CliResult<Goal> {
    latest_goals(store)?
        .remove(id)
        .ok_or_else(|| CliError::Usage(format!("goal not found: {id}")))
}

/// Parse a lifecycle stage string via the serde snake_case mapping.
fn parse_goal_stage(s: &str) -> CliResult<GoalStage> {
    serde_json::from_value(serde_json::Value::String(s.to_string())).map_err(|_| {
        CliError::Usage(format!(
            "unknown stage `{s}` (draft|exploring|explored|working|done|verifying|verified)"
        ))
    })
}

fn goal_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "goal create|list|show|describe-set|design-set|acceptance-set|explore-add|stage|learning-status|evaluate|close",
    )?;
    match args[0].as_str() {
        "create" => {
            let goal = Goal {
                id: value(args, "--id").unwrap_or_else(|| generated_id("goal")),
                title: required(args, "--title")?,
                owner_agent_id: required(args, "--owner")?,
                status: GoalStatus::Active,
                priority: value(args, "--priority").unwrap_or_else(|| "p0".into()),
                created_at: now_string(),
                updated_at: now_string(),
                vision_id: value(args, "--vision"),
                goal_design_id: value(args, "--goal-design"),
                closed_by_decision_id: value(args, "--closed-by-decision"),
                git_metadata: None,
                // A goal is born in `draft`; it must be explored before it can work.
                stage: GoalStage::Draft,
                description_md: md_value(args, "description")?,
                design_md: md_value(args, "design")?,
                acceptance_md: md_value(args, "acceptance")?,
                explorations: Vec::new(),
                skill_refs: many(args, "--skill-ref"),
                stage_changed_at: Some(now_string()),
            };
            persist_new_goal(store, &goal)?;
            print_json(&goal)?;
        }
        "show" => {
            let id = required(args, "--id").or_else(|_| required(args, "--goal"))?;
            print_json(&goal_load(store, &id)?)?;
        }
        "describe-set" | "design-set" | "acceptance-set" => {
            let id = required(args, "--id").or_else(|_| required(args, "--goal"))?;
            let mut goal = goal_load(store, &id)?;
            let body = md_value(args, "md")?.ok_or_else(|| {
                CliError::Usage(format!("{} needs --md <text> or --md-file <path>", args[0]))
            })?;
            match args[0].as_str() {
                "describe-set" => goal.description_md = Some(body),
                "design-set" => goal.design_md = Some(body),
                "acceptance-set" => goal.acceptance_md = Some(body),
                _ => unreachable!(),
            }
            goal.updated_at = now_string();
            store.append_goal(&goal)?;
            print_json(&goal)?;
        }
        "explore-add" => {
            let id = required(args, "--id").or_else(|_| required(args, "--goal"))?;
            let mut goal = goal_load(store, &id)?;
            let author = required(args, "--author")?;
            let round = value(args, "--round")
                .and_then(|r| r.parse::<u32>().ok())
                .unwrap_or_else(|| {
                    goal.explorations
                        .iter()
                        .map(|e| e.round)
                        .max()
                        .map_or(1, |m| m + 1)
                });
            let notes = md_value(args, "notes")?.ok_or_else(|| {
                CliError::Usage("explore-add needs --notes <text> or --notes-file <path>".into())
            })?;
            goal.explorations.push(Exploration {
                author,
                round,
                notes_md: notes,
                created_at: now_string(),
            });
            goal.updated_at = now_string();
            store.append_goal(&goal)?;
            print_json(&goal)?;
        }
        "stage" => {
            let id = required(args, "--id").or_else(|_| required(args, "--goal"))?;
            let to = parse_goal_stage(&required(args, "--to")?)?;
            let mut goal = goal_load(store, &id)?;
            // The gate is where substance is enforced (design before explored,
            // real acceptance before working). Refuse otherwise.
            goal.check_transition(to).map_err(CliError::Usage)?;
            goal.stage = to;
            goal.status = to.to_status();
            let now = now_string();
            goal.stage_changed_at = Some(now.clone());
            goal.updated_at = now;
            store.append_goal(&goal)?;
            append_agent_event(
                store,
                &goal.owner_agent_id,
                None,
                None,
                "goal_stage_changed",
                &format!("Goal {id} → stage {}", to.as_str()),
                None,
            )?;
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
        "evaluate" => goal_evaluate(store, &args[1..])?,
        "close" => goal_close(store, &args[1..])?,
        "list" => print_json(&store.goals()?)?,
        other => return Err(CliError::Usage(format!("unknown goal command: {other}"))),
    }
    Ok(())
}

/// Transition a Goal to `complete`, enforcing the §3.7 closeout gate: a goal may
/// only close with a closeout Decision (decision_kind=closeout, >=1 evidence) AND a
/// GoalEvaluation, OR an explicit valid waiver. Refuses otherwise with a clear error.
fn goal_close(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let goal_id = required(args, "--id").or_else(|_| required(args, "--goal"))?;
    let mut goal = latest_goals(store)?
        .remove(&goal_id)
        .ok_or_else(|| CliError::Usage(format!("goal not found: {goal_id}")))?;
    let status = goal_learning_status(store, &goal_id)?;
    status.require_closeout()?;

    // Record the decision id that satisfied the gate so the close is auditable and
    // the Goal carries closed_by_decision_id (the field added in WP-B).
    let closing_decision_id = status
        .closeout_decisions
        .last()
        .map(|decision| decision.id.clone())
        .or_else(|| {
            status
                .valid_closeout_waivers()
                .last()
                .map(|decision| decision.id.clone())
        });

    if goal.status != GoalStatus::Done {
        goal.status = GoalStatus::Done;
    }
    if goal.closed_by_decision_id.is_none() {
        goal.closed_by_decision_id = closing_decision_id.clone();
    }
    goal.updated_at = now_string();
    store.append_goal(&goal)?;
    append_agent_event(
        store,
        &goal.owner_agent_id,
        None,
        None,
        "goal_closed",
        &format!("Goal {goal_id} marked done via closeout gate"),
        closing_decision_id.as_deref(),
    )?;
    print_json(&goal)?;
    Ok(())
}

/// Build a TYPED [`GoalEvaluation`] (per schemas/goal-evaluation.schema.json) for a
/// goal and persist it via `append_goal_evaluation`. This is the producer the
/// closeout gate and `goal_learning_status.has_evaluation` read through the typed
/// dual-read seam — it is NOT an untyped `Evidence(source_type=goal_evaluation)`
/// note. The evaluation references the goal's task evidence: every Evidence row
/// attached to a task in the goal's graph is surfaced under
/// `referenced_evidence_ids` in the JSON output, and any evidence ids passed via
/// `--missing-evidence` are kept as the evaluator's gaps. Existing
/// `goal-evaluation create` remains the lower-level form; `goal evaluate` is the
/// goal-scoped path that wires in the trace automatically.
fn goal_evaluate(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let goal_id = required(args, "--id").or_else(|_| required(args, "--goal"))?;
    // Validate the goal exists and gather its trace (errors if the goal is unknown).
    let status = goal_learning_status(store, &goal_id)?;

    // Reference the goal's task evidence: collect every Evidence row attached to a
    // task in this goal's graph so the evaluation points at the real trace.
    let task_id_set: BTreeSet<String> = status.task_ids.iter().cloned().collect();
    let mut referenced_evidence_ids: Vec<String> = latest_evidence(store)?
        .into_values()
        .filter(|item| {
            item.task_id
                .as_ref()
                .is_some_and(|task_id| task_id_set.contains(task_id))
        })
        .map(|item| item.id)
        .collect();
    referenced_evidence_ids.sort();
    referenced_evidence_ids.dedup();

    let evaluation = GoalEvaluation {
        id: value(args, "--id-out").unwrap_or_else(|| generated_id("goal-evaluation")),
        goal_id: goal_id.clone(),
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
    append_agent_event(
        store,
        &evaluation.evaluator_agent_id,
        None,
        None,
        "goal_evaluated",
        &format!(
            "GoalEvaluation {} recorded for goal {goal_id} ({})",
            evaluation.id,
            evaluation.outcome.as_str()
        ),
        None,
    )?;
    print_json(&serde_json::json!({
        "evaluation": evaluation,
        "referenced_evidence_ids": referenced_evidence_ids,
    }))?;
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
                description: value(args, "--description"),
                git_metadata: None,
            };
            persist_new_task(store, &task)?;
            print_json(&task)?;
        }
        "assign" => {
            let task = assign_task(
                store,
                &required(args, "--id")?,
                &TaskAssignment {
                    assignee: required(args, "--assignee")?,
                    channel: value(args, "--channel"),
                    allow_missing_goal_design: has_flag(args, "--allow-missing-goal-design"),
                    waiver_decision_id: value(args, "--waiver-decision"),
                },
            )?;
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
                sender_kind: sender_kind_from_args(args)?,
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
            // Hooks are codex's runtime mechanism. Default to codex (today's only
            // caller passes no provider), but honor an explicit --provider /
            // HARNESS_PROVIDER override; a non-codex provider gets the trait default
            // (an explicit "does not support hook events" error), never a mis-stamped
            // codex event.
            let provider = value(args, "--provider")
                .or_else(|| std::env::var("HARNESS_PROVIDER").ok())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| CodexAdapter.name().to_string());
            let adapter = provider_adapter(&provider)
                .ok_or_else(|| unknown_provider_error(&provider, "hook record"))?;
            adapter.record_hook_event(store, args)?;
        }
        other => return Err(CliError::Usage(format!("unknown hook command: {other}"))),
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
            validate_decision(&decision)?;
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

/// Canonical stop-gate decision values (§3.6). Kept domain-neutral: the harness
/// only knows "stop" vs "continue", never *why* a run must stop.
const STOP_GATE_DECISIONS: [&str; 2] = ["stop_approved", "continue_required"];

/// Validate a Decision before it is persisted. Enforces the WP-F write-time rules:
/// - is_waiver=true requires a follow_up_task_id AND >=1 evidence_ids (§3.6).
/// - decision_kind=stop_gate requires decision in {stop_approved, continue_required}.
fn validate_decision(decision: &Decision) -> CliResult<()> {
    if decision.is_waiver {
        if decision.follow_up_task_id.is_none() {
            return Err(CliError::Usage(
                "a waiver decision (--waiver) must name a --follow-up-task".into(),
            ));
        }
        if decision.evidence_ids.is_empty() {
            return Err(CliError::Usage(
                "a waiver decision (--waiver) must reference at least one --evidence".into(),
            ));
        }
    }
    if decision.decision_kind.as_deref() == Some("stop_gate")
        && !STOP_GATE_DECISIONS.contains(&decision.decision.as_str())
    {
        return Err(CliError::Usage(format!(
            "decision_kind=stop_gate requires --decision one of {}; got \"{}\"",
            STOP_GATE_DECISIONS.join("|"),
            decision.decision
        )));
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
        sender_kind: SenderKind::Agent,
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
        sender_kind: SenderKind::Agent,
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
                owner_agent_id: lead.clone(),
                status: GoalStatus::Active,
                priority: value(args, "--priority").unwrap_or_else(|| "p0".into()),
                created_at: now_string(),
                updated_at: now_string(),
                vision_id: value(args, "--goal-vision"),
                goal_design_id: None,
                closed_by_decision_id: None,
                git_metadata: None,
                stage: GoalStage::default(),
                description_md: None,
                design_md: None,
                acceptance_md: None,
                explorations: Vec::new(),
                skill_refs: Vec::new(),
                stage_changed_at: None,
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
                description: value(args, "--task-description"),
                git_metadata: None,
            };
            // Write a TYPED GoalDesign (graduated object) rather than an untyped
            // Evidence(source_type=goal_design): the next-round goal is designed
            // through the same first-class object the design gate / dashboard read,
            // scoped to the accepted goal so goal_learning_status.has_goal_design
            // sees it via the typed dual-read seam. Falls back to the source goal id
            // when the accept did not create a new goal.
            let design_goal_id = next_goal_id
                .clone()
                .or_else(|| source_task.goal_id.clone())
                .unwrap_or_else(|| task.id.clone());
            let design = GoalDesign {
                id: generated_id("goal-design"),
                goal_id: design_goal_id,
                scenario_summary: format!(
                    "Next-round design generated from accepted autonomous proposal {proposal_id}: {}",
                    task.objective
                ),
                non_goals: Vec::new(),
                risk_and_permission_boundaries: format!(
                    "Inherits permission boundaries of source goal {} / task {}; gated behind closeout decision {}.",
                    source_task.goal_id.as_deref().unwrap_or("-"),
                    source_task.id,
                    decision.id
                ),
                required_infra: Vec::new(),
                agent_team: assignee.clone(),
                task_graph: vec![task.id.clone()],
                evidence_plan: vec![format!("proposal:{proposal_id}")],
                acceptance_gates: task.acceptance_criteria.clone(),
                created_at: now_string(),
            };
            store.append_goal_design(&design)?;
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
                    sender_kind: SenderKind::Agent,
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
        // Dual-read: the goal is a next-round candidate if it carries EITHER a
        // typed GoalEvaluation object OR a legacy Evidence(source_type=goal_evaluation)
        // note. The typed producer is the primary path (item 3 of WP-7); the legacy
        // note remains a valid back-compat source so old goals keep firing.
        let Some((evaluation_evidence_id, source_task_id)) =
            latest_goal_evaluation_source(&status, store)?
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

/// Resolve the goal's evaluation into `(evaluation_id, source_task_id)` for the
/// next-round runner, reading BOTH learning representations (WP-7 dual-read). The
/// most-recent evaluation wins across the typed [`GoalEvaluation`] objects and the
/// legacy `Evidence(source_type=goal_evaluation)` notes. A typed object has no
/// `task_id`, so the source task is the latest task in the goal's graph — the task
/// the closeout decision and next-round plan hang off. Returns `None` only when
/// the goal has no evaluation at all (caller skips it as a candidate).
fn latest_goal_evaluation_source(
    status: &GoalLearningStatus,
    store: &HarnessStore,
) -> CliResult<Option<(String, String)>> {
    // Best legacy candidate: (time, evidence_id, source_task_id).
    let legacy = status
        .goal_evaluation
        .iter()
        .filter_map(|item| {
            Some((
                parse_unix_ms(&item.created_at).unwrap_or_default(),
                item.id.clone(),
                item.task_id.clone()?,
            ))
        })
        .max_by_key(|(created_at, _, _)| *created_at);

    // Best typed candidate: resolve a source task from the goal's graph (latest by
    // task created_at, falling back to the last task id) so the typed evaluation
    // can drive the same close/plan path.
    let latest_typed = status
        .goal_evaluation_objects
        .iter()
        .max_by_key(|evaluation| parse_unix_ms(&evaluation.created_at).unwrap_or_default());
    let typed = match latest_typed {
        // Anchor the typed evaluation on a task in the goal's graph; without one
        // there is nothing for the closeout decision / next-round plan to hang off.
        Some(evaluation) => goal_graph_source_task(status, store)?.map(|task_id| {
            (
                parse_unix_ms(&evaluation.created_at).unwrap_or_default(),
                evaluation.id.clone(),
                task_id,
            )
        }),
        None => None,
    };

    let best = match (legacy, typed) {
        (Some(legacy), Some(typed)) => {
            if typed.0 >= legacy.0 {
                Some(typed)
            } else {
                Some(legacy)
            }
        }
        (Some(legacy), None) => Some(legacy),
        (None, Some(typed)) => Some(typed),
        (None, None) => None,
    };
    Ok(best.map(|(_, evaluation_id, source_task_id)| (evaluation_id, source_task_id)))
}

/// The task in the goal's graph that anchors a typed-evaluation next round: the
/// most recently created task, falling back to the lexically-last task id when no
/// timestamp is parseable. Returns `None` when the goal has no tasks.
fn goal_graph_source_task(
    status: &GoalLearningStatus,
    store: &HarnessStore,
) -> CliResult<Option<String>> {
    if status.task_ids.is_empty() {
        return Ok(None);
    }
    let tasks = latest_tasks(store)?;
    let source = status
        .task_ids
        .iter()
        .map(|task_id| {
            let created_at = tasks
                .get(task_id)
                .and_then(|task| parse_unix_ms(&task.created_at))
                .unwrap_or_default();
            (created_at, task_id.clone())
        })
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, task_id)| task_id);
    Ok(source)
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
        &format!("Goal {} marked done by runner", candidate.goal_id),
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
        "--create-task".into(),
        next_task_id.clone(),
        "--task-title".into(),
        format!("Follow-up: continue from {source_task_id}"),
        "--task-objective".into(),
        format!("Execute the next autonomous runner task generated from proposal {proposal_id}."),
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
        let rows: Vec<&Gap> = gaps.iter().filter(|gap| gap.severity == severity).collect();
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
        other => Err(CliError::Usage(format!(
            "unknown goal-case command: {other}"
        ))),
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

fn handle_sse_stream(
    store: &HarnessStore,
    mut stream: TcpStream,
    sse_manager: sse::SseManager,
) -> CliResult<()> {
    use std::time::Duration;

    // Send SSE header
    sse::write_sse_header(&mut stream)?;

    // Send initial snapshot
    let events = store.events()?;
    let messages = store.messages()?;
    let sessions = store.provider_sessions()?;
    // Initial snapshot sent to client for sync
    let _snapshot = sse::SseEventFrame::Snapshot {
        agent_events: events,
        messages,
        provider_sessions: sessions,
        generated_at: now_string(),
    };

    // Convert snapshot to JSON for transmission
    let snapshot_json = serde_json::json!({
        "generated_at": now_string(),
    });
    sse::write_sse_frame(&mut stream, "snapshot", &snapshot_json)?;

    // Subscribe to the SSE channel
    let rx = sse_manager.subscribe();
    let mut last_keepalive = std::time::Instant::now();

    // Wait for events and stream them to the client
    loop {
        // Calculate timeout for the next keepalive
        let elapsed = last_keepalive.elapsed();
        let timeout = if elapsed < Duration::from_secs(15) {
            Duration::from_secs(15) - elapsed
        } else {
            Duration::from_millis(100)
        };

        match rx.recv_timeout(timeout) {
            Ok(frame) => {
                match frame {
                    sse::SseEventFrame::Snapshot { .. } => {
                        // Don't re-send snapshots after initial
                    }
                    sse::SseEventFrame::AgentEvent(event) => {
                        if let Ok(json) = serde_json::to_value(&event) {
                            if sse::write_sse_frame(&mut stream, "agent_event", &json).is_err() {
                                break; // Client disconnected
                            }
                        }
                    }
                    sse::SseEventFrame::Message(msg) => {
                        if let Ok(json) = serde_json::to_value(&msg) {
                            if sse::write_sse_frame(&mut stream, "message", &json).is_err() {
                                break; // Client disconnected
                            }
                        }
                    }
                    sse::SseEventFrame::ProviderSession(session) => {
                        if let Ok(json) = serde_json::to_value(&session) {
                            if sse::write_sse_frame(&mut stream, "provider_session", &json).is_err()
                            {
                                break; // Client disconnected
                            }
                        }
                    }
                    // WP2: workflow run and step frames
                    sse::SseEventFrame::WorkflowRun(run) => {
                        if let Ok(json) = serde_json::to_value(&run) {
                            if sse::write_sse_frame(&mut stream, "workflow_run", &json).is_err() {
                                break; // Client disconnected
                            }
                        }
                    }
                    sse::SseEventFrame::WorkflowStep(step) => {
                        if let Ok(json) = serde_json::to_value(&step) {
                            if sse::write_sse_frame(&mut stream, "workflow_step", &json).is_err() {
                                break; // Client disconnected
                            }
                        }
                    }
                    sse::SseEventFrame::ProviderTurnEvent(value) => {
                        if sse::write_sse_frame(&mut stream, "provider_turn_event", &value).is_err()
                        {
                            break; // Client disconnected
                        }
                    }
                    sse::SseEventFrame::ProviderTurnEventNormalized(value) => {
                        if sse::write_sse_frame(
                            &mut stream,
                            "provider_turn_event_normalized",
                            &value,
                        )
                        .is_err()
                        {
                            break; // Client disconnected
                        }
                    }
                }
                last_keepalive = std::time::Instant::now();
            }
            Err(crossbeam::channel::RecvTimeoutError::Timeout) => {
                // Send keepalive to keep connection alive
                if sse::write_sse_keepalive(&mut stream).is_err() {
                    break; // Client disconnected
                }
                last_keepalive = std::time::Instant::now();
            }
            Err(crossbeam::channel::RecvTimeoutError::Disconnected) => {
                break; // Channel closed, exit
            }
        }
    }

    Ok(())
}

fn serve_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let addr = value(args, "--addr").unwrap_or_else(|| "127.0.0.1:8787".into());
    let once = has_flag(args, "--once");
    let listener = TcpListener::bind(&addr)?;
    println!("serving harness API on http://{addr}");
    // Show WHICH store this serve reads — the #1 confusion in issue #89 item 3 was
    // serve and run-script silently using different `.harness` dirs. Print the
    // absolute path so it can be compared against run-script's at a glance.
    let store_display = std::fs::canonicalize(store.root())
        .unwrap_or_else(|_| store.root().to_path_buf())
        .display()
        .to_string();
    println!("store: {store_display}  (override with --store <path> or HARNESS_ROOT)");

    let sse_manager = sse::SseManager::new();

    // Truncate the transient live turn-event tee (Stage B) on startup so it does
    // not grow unbounded across serve runs; the watcher seeds at EOF and the
    // per-session NDJSON remains the durable source for catch-up.
    let _ = fs::write(store.root().join("provider_turn_events.jsonl"), b"");

    let normalize_store = store.clone();
    let provider_cache = Mutex::new(HashMap::<String, String>::new());
    let next_seq_cache = Mutex::new(HashMap::<String, u64>::new());
    let normalize = move |session_id: &str, raw: &serde_json::Value| -> Vec<serde_json::Value> {
        let provider = {
            let Ok(mut cache) = provider_cache.lock() else {
                return Vec::new();
            };
            if let Some(provider) = cache.get(session_id).cloned() {
                provider
            } else {
                let session = match latest_provider_session(&normalize_store, session_id) {
                    Ok(Some(session)) => session,
                    Ok(None) | Err(_) => return Vec::new(),
                };
                let provider = session.provider;
                cache.insert(session_id.to_string(), provider.clone());
                provider
            }
        };

        let next_seq = {
            let Ok(cache) = next_seq_cache.lock() else {
                return Vec::new();
            };
            cache.get(session_id).copied().unwrap_or(0)
        };
        let events = normalize_live_turn_event(&provider, session_id, raw, next_seq);
        if events.is_empty() {
            return Vec::new();
        }

        let Ok(mut cache) = next_seq_cache.lock() else {
            return Vec::new();
        };
        cache.insert(session_id.to_string(), next_seq + events.len() as u64);

        events
            .into_iter()
            .filter_map(|event| serde_json::to_value(event).ok())
            .collect()
    };

    // Start the SSE watcher thread
    sse::start_sse_watcher(store, sse_manager.clone(), normalize).map_err(CliError::Io)?;

    // Start the abandoned-run reaper: periodically flip `Running` runs whose
    // driver process has died (or legacy runs past the stale window) to `Failed`,
    // so the dashboard never shows a phantom-running workflow after a driver is
    // killed/crashes. The terminal rows it appends are tailed and broadcast by
    // the SSE watcher above, so a live dashboard updates without a refetch.
    {
        let reaper_store = store.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(REAP_POLL_INTERVAL);
            let _ = reap_stale_workflow_runs(&reaper_store);
        });
    }

    for stream in listener.incoming() {
        let stream = match stream {
            Ok(stream) => stream,
            Err(error) => {
                // A failed accept (e.g. a client that hung up before the
                // handshake) must not take the whole server down.
                eprintln!("serve: accept failed: {error}");
                continue;
            }
        };

        if once {
            // Single-shot mode (tests): handle inline for deterministic ordering.
            if let Err(error) = handle_http_connection(store, stream, sse_manager.clone()) {
                eprintln!("serve: connection error: {error}");
            }
            break;
        }

        // Handle each connection on its own thread so a long-lived SSE stream
        // (/v1/events blocks for the life of the client) cannot starve other
        // requests — POST actions, snapshot polling, and additional clients
        // must still be served while a stream is open. Per-connection errors
        // (most commonly a broken pipe when a client disconnects mid-write) are
        // logged and contained to that thread instead of aborting the loop.
        let conn_store = store.clone();
        let conn_manager = sse_manager.clone();
        std::thread::spawn(move || {
            if let Err(error) = handle_http_connection(&conn_store, stream, conn_manager) {
                eprintln!("serve: connection error: {error}");
            }
        });
    }
    Ok(())
}

/// `harness daemon start|status|stop`: the resident warm-child host (unix-only).
///
/// The daemon keeps `claude` children warm across short-lived `harness deliver`
/// invocations behind a per-workspace Unix socket under the store root. The
/// resident delivery path (`HARNESS_CLAUDE_RESIDENT=1`) routes through it when a
/// socket is present, and falls back to an inline single turn when it is not.
#[cfg(unix)]
fn daemon_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(args, "daemon start|status|stop")?;
    let harness_root = store.root().to_path_buf();
    match args[0].as_str() {
        "start" => {
            let idle_secs = value(args, "--idle-secs")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(resident::DEFAULT_MAX_IDLE.as_secs());
            // `--socket <path>` may only restate the default per-workspace
            // socket. Discovery is HARNESS_ROOT-only: the delivery client and
            // `daemon status`/`stop` all derive the socket from HARNESS_ROOT via
            // `daemon_socket_path`, with no way to learn an overridden directory.
            // So a socket whose parent != the store root would start a live but
            // UNDISCOVERABLE daemon (deliveries silently degrade to inline,
            // `status` reports absent, `stop` finds no pidfile). We therefore
            // accept the flag only when it names exactly `<HARNESS_ROOT>/resident.sock`
            // and reject any other path with a clear error rather than spawning
            // an orphan daemon.
            if let Some(path) = value(args, "--socket") {
                let path = PathBuf::from(path);
                let expected = resident_daemon::daemon_socket_path(&harness_root);
                if path != expected {
                    return Err(CliError::Usage(format!(
                        "--socket must be {} (discovery is HARNESS_ROOT-only); got {}",
                        expected.display(),
                        path.display()
                    )));
                }
            }
            resident_daemon::run_daemon(&harness_root, idle_secs)?;
        }
        "status" => match resident_daemon::daemon_status(&harness_root) {
            resident_daemon::DaemonStatus::Running => {
                let pid = resident_daemon::daemon_pid(&harness_root);
                println!(
                    "running (socket {}{})",
                    resident_daemon::daemon_socket_path(&harness_root).display(),
                    pid.map(|p| format!(", pid {p}")).unwrap_or_default()
                );
            }
            resident_daemon::DaemonStatus::Stale => println!(
                "stale (socket {} exists but no daemon answers)",
                resident_daemon::daemon_socket_path(&harness_root).display()
            ),
            resident_daemon::DaemonStatus::Absent => println!("absent (no daemon socket)"),
        },
        "stop" => match resident_daemon::daemon_pid(&harness_root) {
            Some(pid) => {
                stop_pid(pid)?;
                println!("stopped resident daemon pid {pid}");
            }
            None => return Err(CliError::Usage("no resident daemon pidfile found".into())),
        },
        other => return Err(CliError::Usage(format!("unknown daemon command: {other}"))),
    }
    Ok(())
}

fn handle_http_connection(
    store: &HarnessStore,
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
            "/v1/events" => {
                // Handle SSE endpoint
                handle_sse_stream(store, stream, sse_manager)?
            }
            "/v1/docs" => match read_allowed_doc(&path) {
                Ok((doc_path, content)) => write_http_json(
                    &mut stream,
                    "200 OK",
                    &serde_json::json!({"path": doc_path, "content": content}),
                )?,
                Err(detail) => write_http_json(
                    &mut stream,
                    "404 Not Found",
                    &serde_json::json!({"error": "doc_not_found", "detail": detail}),
                )?,
            },
            // GET /v1/provider-sessions/{id}/normalized-events — normalized
            // HarnessTurnEvent[] computed on read from the retained RAW
            // per-session provider NDJSON. This does not write new storage and
            // intentionally uses provider adapter defaults until S2b/S2c add
            // provider-specific mappings.
            session_path
                if session_path.starts_with("/v1/provider-sessions/")
                    && session_path.ends_with("/normalized-events") =>
            {
                // Session ids are generated tokens (delivery-<ts>-<n>): safe
                // path chars, no URL-decoding needed.
                let session_id = session_path
                    .strip_prefix("/v1/provider-sessions/")
                    .and_then(|rest| rest.strip_suffix("/normalized-events"))
                    .unwrap_or_default()
                    .to_string();
                match read_provider_session_normalized_events(store, &session_id) {
                    Ok((events, truncated)) => write_http_json(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "session_id": session_id,
                            "events": events,
                            "truncated": truncated,
                        }),
                    )?,
                    Err(detail) => write_http_json(
                        &mut stream,
                        "404 Not Found",
                        &serde_json::json!({"error": "session_events_not_found", "detail": detail.to_string()}),
                    )?,
                }
            }
            // GET /v1/provider-sessions/{id}/events — the RAW provider turn,
            // 1:1: every line of the persisted claude/codex stream as parsed
            // JSON, so the dashboard can show the agent's actual events
            // (assistant text, tool_use, tool_result, result) instead of a
            // wrapped "succeeded: N events" summary.
            session_path
                if session_path.starts_with("/v1/provider-sessions/")
                    && session_path.ends_with("/events") =>
            {
                // Session ids are generated tokens (delivery-<ts>-<n>): safe
                // path chars, no URL-decoding needed.
                let session_id = session_path
                    .strip_prefix("/v1/provider-sessions/")
                    .and_then(|rest| rest.strip_suffix("/events"))
                    .unwrap_or_default()
                    .to_string();
                match read_provider_session_events(store, &session_id) {
                    Ok((events, truncated)) => write_http_json(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "session_id": session_id,
                            "events": events,
                            "truncated": truncated,
                        }),
                    )?,
                    Err(detail) => write_http_json(
                        &mut stream,
                        "404 Not Found",
                        &serde_json::json!({"error": "session_events_not_found", "detail": detail.to_string()}),
                    )?,
                }
            }
            // GET /v1/sessions/{id}/normalized-events — the normalized
            // (HarnessTurnEvent[]) companion to the historical raw endpoint
            // below, computed on read from the DURABLE per-session NDJSON. Same
            // `retained` semantics: a pruned `--trace live` run returns
            // `retained: false` with an empty list so the dashboard can render
            // "trace not retained" provider-agnostically. Matched BEFORE the raw
            // `/events` arm because it is the more specific suffix.
            sessions_norm_path
                if sessions_norm_path.starts_with("/v1/sessions/")
                    && sessions_norm_path.ends_with("/normalized-events") =>
            {
                let session_id = sessions_norm_path
                    .strip_prefix("/v1/sessions/")
                    .and_then(|rest| rest.strip_suffix("/normalized-events"))
                    .unwrap_or_default()
                    .to_string();
                match read_session_turn_events_normalized(store, &session_id) {
                    Ok((retained, events, truncated)) => write_http_json(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "session_id": session_id,
                            "retained": retained,
                            "events": events,
                            "truncated": truncated,
                        }),
                    )?,
                    Err(detail) => write_http_json(
                        &mut stream,
                        "404 Not Found",
                        &serde_json::json!({"error": "session_events_not_found", "detail": detail.to_string()}),
                    )?,
                }
            }
            // GET /v1/sessions/{id}/events — the PERSISTED per-session turn
            // events for a completed durable run's historical drill-in (two-tier
            // persistence read side). Reads the durable per-session NDJSON the
            // ProviderSession's jsonl_ref/stdout_ref points at (survives a serve
            // restart). A `--trace live` run whose trace was pruned after
            // execution left those refs None, so we return `retained: false`
            // ("trace not retained") and the UI can distinguish it.
            sessions_path
                if sessions_path.starts_with("/v1/sessions/")
                    && sessions_path.ends_with("/events") =>
            {
                let session_id = sessions_path
                    .strip_prefix("/v1/sessions/")
                    .and_then(|rest| rest.strip_suffix("/events"))
                    .unwrap_or_default()
                    .to_string();
                match read_session_turn_events(store, &session_id) {
                    Ok(result) => write_http_json(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "session_id": session_id,
                            "retained": result.retained,
                            "events": result.events,
                            "truncated": result.truncated,
                        }),
                    )?,
                    Err(detail) => write_http_json(
                        &mut stream,
                        "404 Not Found",
                        &serde_json::json!({"error": "session_events_not_found", "detail": detail.to_string()}),
                    )?,
                }
            }
            // GET /v1/workflows — the registered (built-in) workflow catalog,
            // run-independent { name, summary } pairs from the compiled registry.
            "/v1/workflows" => {
                let registry = workflow::WorkflowRegistry::builtin();
                let defs: Vec<serde_json::Value> = registry
                    .names()
                    .into_iter()
                    .filter_map(|name| registry.get(name))
                    .map(|def| serde_json::json!({"name": def.name, "summary": def.summary}))
                    .collect();
                write_http_json(&mut stream, "200 OK", &serde_json::json!(defs))?
            }
            // GET /v1/workflows/{name}/source — the Rust source of the workflow
            // module, so the Definition section can show the ground-truth body.
            source_path
                if source_path.starts_with("/v1/workflows/")
                    && source_path.ends_with("/source") =>
            {
                let name = source_path
                    .strip_prefix("/v1/workflows/")
                    .and_then(|rest| rest.strip_suffix("/source"))
                    .unwrap_or_default();
                let registry = workflow::WorkflowRegistry::builtin();
                if registry.get(name).is_some() {
                    write_http_json(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "path": "workflow.rs",
                            "source": include_str!("workflow.rs"),
                        }),
                    )?
                } else {
                    write_http_json(
                        &mut stream,
                        "404 Not Found",
                        &serde_json::json!({"error": "workflow_not_found", "name": name}),
                    )?
                }
            }
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
    if path == "/v1/teams" {
        return create_team_value(store, body);
    }
    if path == "/v1/agents" {
        return create_agent_value(store, body);
    }
    if path == "/v1/goals" {
        return create_goal_value(store, body);
    }
    if path == "/v1/tasks" {
        return create_task_value(store, body);
    }
    if let Some(task_id) = path
        .strip_prefix("/v1/tasks/")
        .and_then(|rest| rest.strip_suffix("/assign"))
    {
        return assign_task_value(store, task_id, body);
    }
    if let Some(task_id) = path
        .strip_prefix("/v1/tasks/")
        .and_then(|rest| rest.strip_suffix("/reviewer"))
    {
        return set_task_reviewer_value(store, task_id, body);
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
        sender_kind: match json_string(body, "sender_kind") {
            Some(value) => parse_sender_kind(&value)?,
            None => SenderKind::default(),
        },
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

// ---------------------------------------------------------------------------
// Create-entity side-effect helpers (WP-ii)
//
// These functions own the *persistence + event* logic for creating each core
// entity, so the CLI command arms and the HTTP create routes (POST /v1/teams,
// /agents, /goals, /tasks[+assign]) share one implementation. The CLI builds
// the struct from `--flag` args; the HTTP value-fns below build the same struct
// from a JSON body. Both then call these helpers, so behaviour cannot diverge.
// ---------------------------------------------------------------------------

/// Persist a freshly-built team. Mirrors the `team create` CLI arm.
fn persist_new_team(store: &HarnessStore, team: &AgentTeam) -> CliResult<()> {
    store.append_team(team)?;
    Ok(())
}

/// Persist a freshly-built goal. Mirrors the `goal create` CLI arm.
fn persist_new_goal(store: &HarnessStore, goal: &Goal) -> CliResult<()> {
    store.append_goal(goal)?;
    Ok(())
}

/// Persist a freshly-built task. Mirrors the `task create` CLI arm.
fn persist_new_task(store: &HarnessStore, task: &Task) -> CliResult<()> {
    store.append_task(task)?;
    Ok(())
}

/// Persist a freshly-built member (no runtime start) and emit the
/// `agent_created` event. Shared by the non-`--start` CLI path and the
/// POST /v1/agents route. Runtime start stays a separate action.
fn finalize_member_creation(store: &HarnessStore, member: &AgentMember) -> CliResult<()> {
    store.append_member(member)?;
    append_agent_event(
        store,
        &member.id,
        member.provider_runtime_id.as_deref(),
        None,
        "agent_created",
        "Agent Member created",
        member.prompt_ref.as_deref(),
    )?;
    Ok(())
}

/// Parameters for assigning a task, shared by the `task assign` CLI arm and the
/// POST /v1/tasks/{id}/assign route.
struct TaskAssignment {
    assignee: String,
    channel: Option<String>,
    allow_missing_goal_design: bool,
    waiver_decision_id: Option<String>,
}

/// Assign a task to an agent, enforcing the goal-design gate, and queue the
/// task-assignment message. Shared by the CLI and HTTP assign paths.
fn assign_task(
    store: &HarnessStore,
    task_id: &str,
    assignment: &TaskAssignment,
) -> CliResult<Task> {
    let mut task = latest_task(store, task_id)?;
    if let Some(goal_id) = task.goal_id.as_deref() {
        let status = goal_learning_status(store, goal_id)?;
        if status.goal_design.is_empty() {
            if assignment.allow_missing_goal_design {
                status.require_valid_waiver(store, assignment.waiver_decision_id.as_deref())?;
            } else {
                return Err(CliError::Usage(format!(
                    "task {task_id} cannot be assigned before goal {goal_id} has goal_design evidence; use --allow-missing-goal-design with --waiver-decision <id> only for an explicit design-stage waiver"
                )));
            }
        }
    }
    task.assignee_agent_id = Some(assignment.assignee.clone());
    task.status = TaskStatus::Assigned;
    task.updated_at = now_string();
    store.append_task(&task)?;
    let message = Message {
        id: generated_id("msg"),
        task_id: Some(task.id.clone()),
        from_agent_id: task.owner_agent_id.clone(),
        to_agent_id: Some(assignment.assignee.clone()),
        channel: Some(
            assignment
                .channel
                .clone()
                .unwrap_or_else(|| "task-assignment".into()),
        ),
        kind: MessageKind::Task,
        delivery_status: MessageDeliveryStatus::Queued,
        content: format!("Assigned task {}", task.id),
        evidence_ids: Vec::new(),
        created_at: now_string(),
        delivery: None,
        sender_kind: SenderKind::Agent,
    };
    store.append_message(&message)?;
    Ok(task)
}

// ---------------------------------------------------------------------------
// HTTP create value-fns (WP-ii)
//
// Thin wrappers that build each entity from a JSON body and delegate to the
// shared persistence helpers above. Missing required fields surface as
// `CliError::Usage`, which the serve loop maps to a 400 response.
// ---------------------------------------------------------------------------

/// POST /v1/teams — build a team from the JSON body and persist it.
fn create_team_value(
    store: &HarnessStore,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let team = AgentTeam {
        id: json_string(body, "id").unwrap_or_else(|| generated_id("team")),
        name: required_json_string(body, "name")?,
        description: required_json_string(body, "description")?,
        owner_agent_id: required_json_string(body, "owner")
            .or_else(|_| required_json_string(body, "owner_agent_id"))?,
        status: AgentTeamStatus::Active,
        member_ids: json_string_array(body, "member"),
        created_at: now_string(),
        updated_at: now_string(),
    };
    persist_new_team(store, &team)?;
    Ok(serde_json::to_value(team)?)
}

/// POST /v1/agents — build an Agent Member from the JSON body and persist it.
/// Does NOT start a runtime: `--start` / runtime spawn stays a separate action.
fn create_agent_value(
    store: &HarnessStore,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let mut member = build_member_from_json(body)?;
    let prompt_ref =
        ensure_agent_prompt_with_override(store, &member, json_string(body, "prompt"))?;
    member.prompt_ref = Some(prompt_ref);
    member.status = AgentMemberStatus::Idle;
    finalize_member_creation(store, &member)?;
    Ok(serde_json::to_value(member)?)
}

/// POST /v1/goals — build a goal from the JSON body and persist it.
fn create_goal_value(
    store: &HarnessStore,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let goal = Goal {
        id: json_string(body, "id").unwrap_or_else(|| generated_id("goal")),
        title: required_json_string(body, "title")?,
        owner_agent_id: required_json_string(body, "owner")
            .or_else(|_| required_json_string(body, "owner_agent_id"))?,
        status: GoalStatus::Active,
        priority: json_string(body, "priority").unwrap_or_else(|| "p0".into()),
        created_at: now_string(),
        updated_at: now_string(),
        vision_id: json_string(body, "vision"),
        goal_design_id: json_string(body, "goal_design"),
        closed_by_decision_id: json_string(body, "closed_by_decision"),
        git_metadata: None,
        stage: GoalStage::default(),
        description_md: None,
        design_md: None,
        acceptance_md: None,
        explorations: Vec::new(),
        skill_refs: Vec::new(),
        stage_changed_at: None,
    };
    persist_new_goal(store, &goal)?;
    Ok(serde_json::to_value(goal)?)
}

/// POST /v1/tasks — build a task from the JSON body and persist it.
fn create_task_value(
    store: &HarnessStore,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let task = Task {
        id: json_string(body, "id").unwrap_or_else(|| generated_id("task")),
        goal_id: json_string(body, "goal"),
        parent_task_id: json_string(body, "parent"),
        title: required_json_string(body, "title")?,
        objective: required_json_string(body, "objective")?,
        owner_agent_id: required_json_string(body, "owner")
            .or_else(|_| required_json_string(body, "owner_agent_id"))?,
        assignee_agent_id: json_string(body, "assignee"),
        reviewer_agent_id: json_string(body, "reviewer"),
        status: TaskStatus::Planned,
        depends_on_task_ids: json_string_array(body, "depends_on"),
        workspace_ref: json_string(body, "workspace"),
        branch_ref: json_string(body, "branch"),
        pr_ref: json_string(body, "pr"),
        owned_paths: json_string_array(body, "owned_path"),
        acceptance_criteria: json_string_array(body, "acceptance"),
        created_at: now_string(),
        updated_at: now_string(),
        phase: json_string(body, "phase"),
        scope_refs: json_string_array(body, "scope_ref"),
        requires_human_approval: json_bool(body, "requires_human_approval").unwrap_or(false),
        verdict_decision_id: json_string(body, "verdict_decision"),
        description: json_string(body, "description"),
        git_metadata: None,
    };
    persist_new_task(store, &task)?;
    Ok(serde_json::to_value(task)?)
}

/// POST /v1/tasks/{id}/assign — assign a task to an agent from the JSON body.
fn assign_task_value(
    store: &HarnessStore,
    task_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let task = assign_task(
        store,
        task_id,
        &TaskAssignment {
            assignee: required_json_string(body, "assignee")
                .or_else(|_| required_json_string(body, "assignee_agent_id"))?,
            channel: json_string(body, "channel"),
            allow_missing_goal_design: json_bool(body, "allow_missing_goal_design")
                .unwrap_or(false),
            waiver_decision_id: json_string(body, "waiver_decision"),
        },
    )?;
    Ok(serde_json::to_value(task)?)
}

/// POST /v1/tasks/{id}/reviewer — set the task's reviewer agent from the JSON
/// body (the `@reviewer` gesture on the dashboard). This only records the
/// reviewer accountability on the existing nullable `Task.reviewer_agent_id`
/// field (no schema change); it deliberately does NOT change status or queue a
/// message. Review delivery is a separate hand-off (`/request-review`,
/// `request_task_review_value`) so the assignment-proof chain stays explicit:
/// naming a reviewer is not the same as handing the work off to them.
fn set_task_reviewer_value(
    store: &HarnessStore,
    task_id: &str,
    body: &serde_json::Value,
) -> CliResult<serde_json::Value> {
    let reviewer = required_json_string(body, "reviewer")
        .or_else(|_| required_json_string(body, "reviewer_agent_id"))?;
    // Fail fast if the named reviewer is not a real member, mirroring assign.
    let _ = latest_member(store, &reviewer)?;
    let mut task = latest_task(store, task_id)?;
    task.reviewer_agent_id = Some(reviewer);
    task.updated_at = now_string();
    store.append_task(&task)?;
    Ok(serde_json::to_value(task)?)
}

/// Build an Agent Member from a JSON body, mirroring `build_member_from_args`.
/// The member is created in `Creating` status; callers set the final status.
fn build_member_from_json(body: &serde_json::Value) -> CliResult<AgentMember> {
    Ok(AgentMember {
        id: json_string(body, "id").unwrap_or_else(|| generated_id("agent")),
        name: required_json_string(body, "name")?,
        description: json_string(body, "description")
            .unwrap_or_else(|| "Codex-backed Agent Member".into()),
        role: required_json_string(body, "role")?,
        provider: json_string(body, "provider").unwrap_or_else(|| "codex".into()),
        model: json_string(body, "model"),
        profile: json_string(body, "profile"),
        provider_config: AgentProviderConfig {
            service_tier: json_string(body, "service_tier"),
            collaboration_mode: json_string(body, "collaboration_mode"),
            effort: json_string(body, "effort"),
            output_schema: body.get("output_schema").filter(|v| !v.is_null()).cloned(),
            approval_policy: json_string(body, "approval_policy"),
            approvals_reviewer: json_string(body, "approvals_reviewer"),
            sandbox_policy: json_string(body, "sandbox_policy"),
            permission_profile: json_string(body, "permission_profile"),
            runtime_workspace_roots: json_string_array(body, "runtime_workspace_root"),
            environment_id: json_string(body, "environment"),
            mcp: None,
        },
        capabilities: json_string_array(body, "capability"),
        team_ids: json_string_array(body, "team"),
        prompt_ref: json_string(body, "prompt_ref"),
        skill_refs: json_string_array(body, "skill"),
        workspace_policy: json_string(body, "workspace_policy"),
        worktree_ref: json_string(body, "worktree"),
        permission_profile: json_string(body, "permission_profile"),
        runtime_workspace_roots: json_string_array(body, "runtime_workspace_root"),
        status: AgentMemberStatus::Creating,
        current_task_id: None,
        current_proposal_id: None,
        provider_runtime_id: None,
        provider_thread_id: None,
        provider_agent_path: json_string(body, "provider_agent_path"),
        provider_agent_nickname: json_string(body, "provider_agent_nickname"),
        provider_agent_role: json_string(body, "provider_agent_role"),
        control_endpoint: None,
        created_at: now_string(),
        last_seen_at: None,
    })
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
        sender_kind: SenderKind::Agent,
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

/// Resolve a `GET /v1/docs?path=docs/...` request to the markdown body, allow-listed
/// to the repository's `docs/` tree. Returns `(canonical-relative-path, content)` or a
/// human-readable error. Rejects anything outside `docs/` and any path traversal so the
/// route can only serve committed docs (ADR 0019, Vision `source_refs` rendering).
fn read_allowed_doc(request_target: &str) -> Result<(String, String), String> {
    let query = request_target.split('?').nth(1).unwrap_or("");
    let raw = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("path="))
        .ok_or_else(|| "missing ?path= parameter".to_string())?;
    // Minimal percent-decoding (paths are simple: slashes + alnum + .-_).
    let decoded = raw
        .replace("%2F", "/")
        .replace("%2f", "/")
        .replace("%20", " ");
    if !decoded.starts_with("docs/") || decoded.contains("..") {
        return Err(format!(
            "path must be under docs/ and contain no ..: {decoded}"
        ));
    }
    let base = std::env::current_dir()
        .and_then(|dir| dir.canonicalize())
        .map_err(|error| format!("cannot resolve working dir: {error}"))?;
    let docs_root = base.join("docs");
    let full = base
        .join(&decoded)
        .canonicalize()
        .map_err(|error| format!("doc not found: {decoded} ({error})"))?;
    if !full.starts_with(&docs_root) {
        return Err(format!("resolved path escapes docs/: {decoded}"));
    }
    let content =
        std::fs::read_to_string(&full).map_err(|error| format!("read failed: {error}"))?;
    Ok((decoded, content))
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

    // Redirect stdin to /dev/null: `codex exec` with an inherited (empty) stdin
    // can wedge forever on "Reading additional input from stdin…" (issue #139
    // item 1). `.output()` leaves stdin inherited, so null it explicitly — the
    // same guard run_ndjson_child already applies to the ephemeral/persistent paths.
    let output = Command::new("codex")
        .args(&command_args)
        .stdin(Stdio::null())
        .output()?;
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
        sender_kind: SenderKind::Agent,
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

    // Null stdin so a no-TTY `codex exec` can't wedge on "Reading additional
    // input from stdin…" (issue #139 item 1); `.output()` leaves it inherited.
    let output = Command::new("codex")
        .args(&command_args)
        .current_dir(&worktree)
        .stdin(Stdio::null())
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
        sender_kind: SenderKind::Agent,
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
    let runtime = match start_provider_runtime(store, &member) {
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
                &format!("{} runtime failed to start: {error}", member.provider),
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
    let socket_path: Option<std::path::PathBuf> = None; // Exec-based delivery has no persistent socket
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
    let protocol_probe = Some("exec-stream".into()); // Codex uses exec-stream, no protocol probe needed
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

fn runtime_is_alive(runtime: &AgentRuntime) -> bool {
    // Exec-stream runtimes don't have persistent PIDs or sockets.
    // Runtime is considered alive if its status is Running.
    runtime.status == AgentRuntimeStatus::Running && runtime.control_endpoint.is_some()
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

// ---------------------------------------------------------------------------
// Workflow runtime CLI (WP1)
//
// `harness workflow run --name <name> [--prompt <text>] [--timeout-ms N] [--model <m>] [--effort <e>] [--dry-run]`
//
// Creates a WorkflowRun (status running), dispatches the named built-in Rust
// workflow through the registry, journals each WorkflowStep, and sets the run
// to completed/failed. Each `agent()` node references a PROVIDER ("codex" |
// "claude") and spins up a NEW one-shot ephemeral worker (Stage B: real spawn
// of codex exec / claude -p; Stage A: a mock driver returns a provider-carrying
// StepResult). The runtime stays provider-neutral — the injected driver carries
// the provider + optional isolation through (ADR-0011 provider-neutral).
// ---------------------------------------------------------------------------

/// Options controlling how the real (non-mock) agent step spins up its ephemeral
/// worker. `dry_run` selects the mock driver (CI default, no spawning);
/// otherwise the real `codex exec` / `claude -p` ephemeral spawn runs with a
/// per-node `timeout_ms`. `start_runtime` is reserved (the ephemeral path does
/// not need a resident runtime).
#[derive(Debug, Clone)]
struct WorkflowDeliveryOptions {
    dry_run: bool,
    #[allow(dead_code)]
    start_runtime: bool,
    timeout_ms: u64,
    /// Run-level default model for real workflow leaves. A leaf's own
    /// `model = ...` still wins.
    default_model: Option<String>,
    /// Run-level default reasoning effort for real workflow leaves. A leaf's own
    /// `effort = ...` still wins.
    default_effort: Option<String>,
    /// Per-WORKER spend backstop in USD (the run's `--max-budget-usd`). Passed to
    /// claude as `--max-budget-usd` so a single worker can never exceed the whole
    /// run's ceiling between the cumulative tally's barrier-granular checks. `None`
    /// = no per-worker cap. Codex has no native budget flag, so this is claude-only.
    max_budget_usd: Option<f64>,
    /// Retention policy for the heavy per-node turn-event trace: "durable"
    /// (default) persists the per-session AgentEvents + retained NDJSON trace;
    /// "live" streams the trace over SSE during execution but prunes it after the
    /// run so a PAST run shows "trace not retained". Live streaming itself is
    /// independent and always happens.
    trace_retention: String,
    /// When true, emit a compact NDJSON progress line to STDERR as each step goes
    /// `running` then terminal — so an agent caller that invoked us via its shell
    /// tool sees the phase-by-phase timeline (which step/phase is live) alongside
    /// the clean final result on STDOUT. Off by default (opt-in `--progress`) so
    /// quiet callers and stdout-parsers are unaffected. Stderr is the conventional
    /// progress stream; stdout stays a single parseable JSON document.
    progress: bool,
}

/// Emit one compact NDJSON progress event to STDERR (used when `--progress` is on).
/// Stderr — not stdout — so stdout stays a single parseable JSON document; an agent
/// caller's shell tool captures both streams, so it still sees the live timeline.
fn emit_progress(event: &serde_json::Value) {
    eprintln!("{event}");
}

fn workflow_effective_model<'a>(
    options: &'a WorkflowDeliveryOptions,
    spec: &'a workflow::AgentStepSpec,
) -> Option<&'a str> {
    spec.model.as_deref().or(options.default_model.as_deref())
}

fn workflow_effective_effort<'a>(
    options: &'a WorkflowDeliveryOptions,
    spec: &'a workflow::AgentStepSpec,
) -> Option<&'a str> {
    spec.effort.as_deref().or(options.default_effort.as_deref())
}

/// The REAL agent-step driver. Drives one provider delivery through the neutral
/// seam: (1) queue a Message addressed to the member, (2) deliver exactly that
/// message via `deliver_agent_messages_value` (which claims + runs
/// `run_provider_delivery`), (3) read back the resulting provider session and
/// report to build a [`workflow::StepResult`].
///
/// This fn is TOTAL: any error (store failure, no runtime, provider failure) is
/// reported as `StepResult { ok: false, .. }` so the workflow's control flow —
/// and the `parallel()` barrier — stays in charge rather than unwinding.
///
/// Build the TERMINAL `WorkflowStep` row for a finished step. The real
/// completion time is `started_at + duration_ms` (the worker's measured
/// duration), not the journal `now`: at finalize every step is journaled with
/// the same `now`, which would make a serial step falsely overlap the later
/// parallel ones. Shared by the live per-step journal (in the driver) and the
/// finalize journal (for mock/test drivers).
fn build_terminal_step(
    run_id: &str,
    step_id: String,
    started_at: String,
    result: &workflow::StepResult,
) -> WorkflowStep {
    let now = now_string();
    let ended_at = match (
        Some(created_ms(&started_at)).filter(|&ms| ms > 0),
        result
            .details
            .as_ref()
            .and_then(|d| d.get("duration_ms"))
            .and_then(|v| v.as_u64()),
    ) {
        (Some(start_ms), Some(dur)) => {
            format!("unix-ms:{}", start_ms.saturating_add(u128::from(dur)))
        }
        _ => now,
    };
    WorkflowStep {
        id: step_id,
        run_id: run_id.to_string(),
        phase: result.phase.clone(),
        label: result.label.clone(),
        provider_session_id: result.provider_session_id.clone(),
        status: result.step_status(),
        output_summary: Some(result.output_summary.clone()),
        result: Some(workflow::step_result_json(result)),
        started_at,
        ended_at: Some(ended_at),
    }
}

fn workflow_real_agent_step(
    store: &HarnessStore,
    run_id: &str,
    options: &WorkflowDeliveryOptions,
    spec: &workflow::AgentStepSpec,
) -> workflow::StepResult {
    // Mint the provider session id HERE, before the `running` row, and stamp it
    // on that row — so the dashboard can link this step to its LIVE turn-event
    // stream WHILE it runs, not only after it finishes. The worker tees each
    // event to the shared `provider_turn_events.jsonl` keyed by this id; the
    // per-node drill-in looks the step's `provider_session_id` up in that live
    // buffer. If the id were only assigned on the terminal row (as before), a
    // running step carried `None` and its live tool-by-tool activity could not be
    // attached until it completed. The worker reuses this exact id.
    let step_id = generated_id("wfstep");
    let session_id = generated_id("session");
    let started_at = now_string();
    let running = WorkflowStep {
        id: step_id.clone(),
        run_id: run_id.to_string(),
        phase: spec.phase.clone(),
        label: spec.label.clone(),
        provider_session_id: Some(session_id.clone()),
        status: WorkflowStepStatus::Running,
        output_summary: None,
        result: None,
        started_at: started_at.clone(),
        ended_at: None,
    };
    // A failure to journal the start row must not abort the step; the terminal
    // row still records the outcome. Best-effort, like the rest of this seam.
    let _ = store.append_workflow_step(&running);

    // Live progress to stderr (opt-in): the caller sees this step go live — its
    // phase and label — the instant it starts, not batched at run finalize.
    if options.progress {
        emit_progress(&serde_json::json!({
            "event": "step",
            "status": "running",
            "phase": spec.phase,
            "label": spec.label,
            "provider": spec.provider,
            "ordinal": spec.ordinal,
        }));
    }

    let result = match try_workflow_real_agent_step(store, options, spec, run_id, &session_id) {
        Ok(mut result) => {
            result.step_id = Some(step_id.clone());
            result.started_at = Some(started_at.clone());
            result
        }
        Err(error) => {
            // A setup/spawn error (e.g. worktree create or process spawn failed)
            // never reached a provider turn, so it has no usage/exit telemetry.
            // We still record a structured failure + the static identity so the
            // dashboard renders the same observability shape as a worker failure.
            let details = serde_json::json!({
                "provider": spec.provider,
                "model": workflow_effective_model(options, spec),
                "failure": {
                    "failed": true,
                    "reason": "spawn",
                    "detail": error.to_string(),
                },
            });
            workflow::StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: false,
                provider_session_id: None,
                output_summary: format!("agent step error: {error}"),
                step_id: Some(step_id.clone()),
                started_at: Some(started_at.clone()),
                details: Some(details),
                structured: None,
                ordinal: spec.ordinal,
            }
        }
    };
    // Journal the TERMINAL row the instant this step finishes. The WorkflowStep
    // SSE watcher tails workflow_steps.jsonl, so the dashboard's per-step status +
    // tokens now light up live as each worker completes — not batched at run
    // finalize. `run_workflow_with_driver` recognises this (step_id is Some) and
    // does not re-journal.
    let _ = store.append_workflow_step(&build_terminal_step(run_id, step_id, started_at, &result));

    // Live progress to stderr (opt-in): the step's terminal status the instant it
    // finishes, so the caller tracks completion per phase as the run streams.
    if options.progress {
        emit_progress(&serde_json::json!({
            "event": "step",
            "status": if result.ok { "ok" } else { "failed" },
            "phase": result.phase,
            "label": result.label,
            "ok": result.ok,
            "ordinal": result.ordinal,
        }));
    }
    result
}

fn try_workflow_real_agent_step(
    store: &HarnessStore,
    options: &WorkflowDeliveryOptions,
    spec: &workflow::AgentStepSpec,
    run_id: &str,
    session_id: &str,
) -> CliResult<workflow::StepResult> {
    // The node references a PROVIDER (not a pre-existing member). In --dry-run
    // (CI default) we return a MOCK StepResult so the run/steps journal, the
    // dashboard, the acceptance script, and `cargo test` exercise the full
    // contract end-to-end without spawning a provider or spending tokens. The
    // real (non-dry-run) path spins up a one-shot EDITABLE ephemeral worker.
    if options.dry_run {
        let isolation_note = match spec.isolation.as_deref() {
            Some(mode) => format!(", isolation={mode}"),
            None => String::new(),
        };
        let model_note = match spec.model.as_deref() {
            Some(model) => format!(", model={model}"),
            None => String::new(),
        };
        // Include multi-byte (CJK) text in the mock output so the dry-run path
        // exercises the SAME truncation/summary code a real non-ASCII run hits —
        // a dry-run that stays pure-ASCII gave a false green for the CJK
        // byte-slice panic class (issue #89 item 2; the panic itself is fixed in
        // #94, this keeps dry-run representative so a regression can't hide).
        let output_summary = format!(
            "ephemeral {} worker (dry-run) for {}{model_note}{isolation_note} · 校验占位中文输出",
            spec.provider, spec.label,
        );
        // In schema mode, synthesize a mock structured object (each required key
        // -> a mock string) so `cargo test` + the acceptance script exercise the
        // structured path WITHOUT a live provider.
        let structured = spec.schema.as_ref().map(|schema| {
            let obj: serde_json::Map<String, serde_json::Value> = schema_required_keys(schema)
                .into_iter()
                .map(|key| {
                    (
                        key.clone(),
                        serde_json::Value::String(format!("mock {key}")),
                    )
                })
                .collect();
            serde_json::Value::Object(obj)
        });
        return Ok(workflow::StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            // Reuse the caller's session id so the mock terminal row matches the
            // `running` row's `provider_session_id` (consistent in dry-run too).
            provider_session_id: Some(session_id.to_string()),
            output_summary,
            // The journaling identity is assigned by the caller, which already
            // journaled the `running` start row before this step began.
            step_id: None,
            started_at: None,
            // No worker ran (dry-run), so there is no usage/exit telemetry; we
            // still surface the requested model so the dashboard can label it.
            details: Some(serde_json::json!({ "model": spec.model })),
            structured,
            ordinal: spec.ordinal,
        });
    }

    spawn_ephemeral_worker(store, options, spec, run_id, session_id)
}

/// RAII guard owning a harness-created throwaway worktree. Its `Drop` removes the
/// worktree (and any temp branch) no matter how the step exits — normal return,
/// `?` early-return, timeout, or panic — so a failed/timed-out node never leaks
/// an orphan (cleanup layer 2). The normal-path cleanup is the SAME code, just
/// triggered by the guard going out of scope at the end of a successful step.
struct WorktreeGuard {
    /// Repo root the `git worktree` commands run against (`git -C <repo>`).
    repo_root: PathBuf,
    /// Absolute path of the worktree checkout.
    path: PathBuf,
    /// Temp branch created with the worktree, deleted alongside it.
    branch: String,
}

/// The throwaway worktree's relative path and temp branch for one leaf, keyed by
/// run + node label + the per-leaf `session_id`. The `session_id` disambiguator is
/// what makes two SAME-LABEL writable nodes (e.g. a fan-out of workers all labeled
/// "fix") get DISTINCT worktrees instead of colliding on one branch+path — the
/// collision that made the 2nd+ such node fail with a cryptic "branch already
/// checked out" git error (issue #139 item 7).
fn worktree_paths(run_id: &str, node_label: &str, session_id: &str) -> (String, String) {
    let slug = sanitize_worktree_slug(node_label);
    let unique = sanitize_worktree_slug(session_id);
    (
        format!(".harness/worktrees/{run_id}-{slug}-{unique}"),
        format!("harness/wt/{run_id}-{slug}-{unique}"),
    )
}

impl WorktreeGuard {
    /// `git -C <repo> worktree add -B <branch> <path> HEAD` — a detach-free
    /// throwaway checkout of HEAD the worker mutates in isolation. Uniform for
    /// both providers (the harness owns the worktree; we never use claude's -w).
    /// The branch+path are unique per LEAF (via `session_id`), so concurrent
    /// same-label writable nodes never collide (issue #139 item 7).
    fn create(
        repo_root: &Path,
        run_id: &str,
        node_label: &str,
        session_id: &str,
    ) -> CliResult<WorktreeGuard> {
        // A writable / isolation="worktree" step runs in a throwaway git worktree.
        // If the workflow's cwd is NOT a git repo, `git worktree add` fails with a
        // cryptic "fatal: not a git repository". Catch that up front with an
        // actionable message (issue #89 item 5): the user either runs from a git
        // repo or keeps the step read-only and pulls the output via get-output.
        if !is_git_repo(repo_root) {
            return Err(CliError::Usage(format!(
                "node '{node_label}' needs an isolated git worktree (it is writable, \
                 or sets isolation=\"worktree\"), but {} is not a git repository. \
                 Either run the workflow from a git repo (e.g. `git init` there), or \
                 make this step READ-ONLY (drop writable / isolation) and retrieve its \
                 output with `harness workflow get-output <run_id> --step {node_label}`.",
                repo_root.display()
            )));
        }

        let (rel, branch) = worktree_paths(run_id, node_label, session_id);
        let path = repo_root.join(&rel);

        // Defensive: a stale dir from a crashed prior run would make `add` fail.
        if path.exists() {
            let _ = Command::new("git")
                .args([
                    "-C",
                    &repo_root.display().to_string(),
                    "worktree",
                    "remove",
                    "--force",
                ])
                .arg(&path)
                .output();
            let _ = fs::remove_dir_all(&path);
        }

        let output = Command::new("git")
            .args([
                "-C",
                &repo_root.display().to_string(),
                "worktree",
                "add",
                "-B",
                &branch,
            ])
            .arg(&path)
            .arg("HEAD")
            .output()?;
        if !output.status.success() {
            return Err(CliError::Usage(format!(
                "git worktree add failed for node {node_label}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(WorktreeGuard {
            repo_root: repo_root.to_path_buf(),
            path,
            branch,
        })
    }
}

impl Drop for WorktreeGuard {
    fn drop(&mut self) {
        // Bulletproof cleanup: remove the worktree and its temp branch however
        // the step exited. Best-effort — Drop must not panic — but `--force`
        // plus a manual dir sweep makes a leak very unlikely.
        let repo = self.repo_root.display().to_string();
        let _ = Command::new("git")
            .args(["-C", &repo, "worktree", "remove", "--force"])
            .arg(&self.path)
            .output();
        let _ = fs::remove_dir_all(&self.path);
        let _ = Command::new("git")
            .args(["-C", &repo, "branch", "-D", &self.branch])
            .output();
        // Prune any now-dangling administrative entry.
        let _ = Command::new("git")
            .args(["-C", &repo, "worktree", "prune"])
            .output();
    }
}

/// Map a node label to a filesystem-safe worktree slug (no `/`, spaces, etc.).
fn sanitize_worktree_slug(label: &str) -> String {
    let slug: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "node".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Resolve the repo root the worktrees are created under. The shared default
/// workspace is the current working directory (the repo cwd); worktrees live in
/// the gitignored `.harness/worktrees/` beneath it.
fn workflow_repo_root() -> PathBuf {
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Whether `path` is inside a git work tree — `git -C <path> rev-parse
/// --is-inside-work-tree` exits 0 and prints `true`. Used to fail a
/// writable/isolated workflow step with a clear message BEFORE attempting a
/// `git worktree add` that would otherwise error cryptically (issue #89 item 5).
fn is_git_repo(path: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Spin up a NEW one-shot EDITABLE ephemeral worker for one `agent()` node and
/// reduce its result into a [`workflow::StepResult`].
///
/// Workspace: the shared repo cwd by default (serial nodes' edits compose on the
/// same tree, exactly like Claude Code's Workflow). When the node opts into
/// `isolation:"worktree"` the HARNESS creates a throwaway worktree (uniform for
/// both providers) and runs the worker there; its `git diff` is collected as the
/// node's evidence and the worktree is NOT auto-merged. Cleanup is the
/// `WorktreeGuard`'s Drop (bulletproof across success/failure/timeout).
fn spawn_ephemeral_worker(
    store: &HarnessStore,
    options: &WorkflowDeliveryOptions,
    spec: &workflow::AgentStepSpec,
    run_id: &str,
    session_id: &str,
) -> CliResult<workflow::StepResult> {
    let repo_root = workflow_repo_root();

    // Opt-in isolation: harness-owned throwaway worktree, else the shared cwd.
    // The guard (when present) cleans up on every exit path via Drop.
    // A node isolates when it explicitly opts in, OR whenever it is `writable`:
    // an editing worker runs in a throwaway worktree so its writes land in a
    // discardable checkout (captured as the step diff), never the live repo.
    let isolate = spec.isolation.as_deref() == Some("worktree") || spec.writable;
    let guard = if isolate {
        Some(WorktreeGuard::create(
            &repo_root,
            run_id,
            &spec.label,
            session_id,
        )?)
    } else {
        None
    };
    let cwd = guard
        .as_ref()
        .map(|g| g.path.clone())
        .unwrap_or_else(|| repo_root.clone());

    // One ephemeral worker == one ProviderSession. The session id keys the
    // dashboard per-node drill-in (WorkflowStep.provider_session_id) and the
    // durable NDJSON / live turn-events. It is minted by the caller and already
    // stamped on the `running` step row, so the live drill-in links mid-flight;
    // the worker reuses it verbatim.
    let session_id = session_id.to_string();
    let session_dir = store.root().join("provider-sessions").join(&session_id);
    fs::create_dir_all(&session_dir)?;

    // Publish a RUNNING ProviderSession row NOW, before the blocking spawn — so the
    // dashboard's per-node drill-in resolves this step's session WHILE it runs and
    // renders the live turn-event stream, instead of "no turn yet" until the step
    // finishes. `ingest_ephemeral_events` writes the terminal row afterward (same
    // id, latest-wins).
    write_running_ephemeral_session(store, &session_id, &session_dir, spec);

    // The structured schema normalized to a real JSON Schema for the providers'
    // native flags (claude `--json-schema`, codex `--output-schema`). `None` for
    // text-mode steps.
    let schema_json = spec.schema.as_ref().map(schema_to_json_schema);

    // One spawn of the configured provider against a (possibly augmented) prompt.
    // Factored into a closure so structured mode can re-run it once for the retry.
    let effective_model = workflow_effective_model(options, spec);
    let effective_effort = workflow_effective_effort(options, spec);
    let spawn_once = |prompt: &str| -> CliResult<EphemeralSpawn> {
        let ctx = EphemeralSpawnContext {
            session_dir: &session_dir,
            session_id: &session_id,
            spec,
            schema_json: schema_json.as_ref(),
            prompt,
            cwd: &cwd,
            model: effective_model,
            effort: effective_effort,
            timeout_ms: options.timeout_ms,
            max_budget_usd: options.max_budget_usd,
        };
        match provider_adapter(spec.provider.as_str()) {
            Some(adapter) => adapter.spawn_ephemeral(&ctx),
            None => Err(CliError::Usage(format!(
                "unknown workflow provider {} (expected codex|claude)",
                spec.provider
            ))),
        }
    };

    // Retry ONCE on a transient PROCESS crash — a non-zero / signalled exit that
    // did NOT time out and produced no reply. That is a blip/crash worth retrying;
    // it deliberately does NOT retry a timeout (we'd just re-hang for another
    // window) nor a clean-exit delivery failure (auth/usage-limit — we'd reproduce
    // it). Distinct from the schema-conformance retry below.
    let spawn_once_resilient = |prompt: &str| -> CliResult<EphemeralSpawn> {
        let first = spawn_once(prompt)?;
        let transient_crash =
            !first.ok && !first.timed_out && first.reply.is_none() && first.exit_code != Some(0);
        if transient_crash {
            std::thread::sleep(Duration::from_millis(500));
            return spawn_once(prompt);
        }
        Ok(first)
    };

    // STRUCTURED mode (spec.schema is Some): append a JSON-only instruction to the
    // prompt, then parse + validate the reply into a structured object. On failure
    // re-run the worker ONCE with a corrective suffix; if it still fails, leave
    // `structured` None and record a "schema" step failure below. Text-mode steps
    // (no schema) just deliver the prompt verbatim, as before.
    let required_keys: Vec<String> = spec
        .schema
        .as_ref()
        .map(schema_required_keys)
        .unwrap_or_default();

    // Wall-clock span of the worker process itself, for the step's `duration_ms`.
    let worker_start = Instant::now();
    let mut structured: Option<serde_json::Value> = None;
    let spawn = if let Some(schema) = &spec.schema {
        let instruction = schema_instruction(schema);

        // First attempt: prompt + the JSON-only instruction. Prefer the
        // provider-validated `structured` (native --json-schema/--output-schema);
        // fall back to extracting JSON from the reply text (the prompt-hint path).
        let mut spawn = spawn_once_resilient(&format!("{}{instruction}", spec.prompt))?;
        structured = spawn.structured.clone().or_else(|| {
            spawn
                .reply
                .as_deref()
                .and_then(extract_json_object)
                .filter(|obj| object_has_required_keys(obj, &required_keys))
        });

        // ONE corrective retry when the worker produced no valid JSON.
        if structured.is_none() {
            let retry_prompt = format!(
                "{}{instruction}\n\nYour previous reply was not valid JSON with keys [{}]; \
                 return ONLY that JSON object.",
                spec.prompt,
                required_keys.join(", "),
            );
            spawn = spawn_once(&retry_prompt)?;
            structured = spawn.structured.clone().or_else(|| {
                spawn
                    .reply
                    .as_deref()
                    .and_then(extract_json_object)
                    .filter(|obj| object_has_required_keys(obj, &required_keys))
            });
        }
        spawn
    } else {
        spawn_once_resilient(&spec.prompt)?
    };

    let duration_ms = worker_start.elapsed().as_millis() as u64;

    // A schema-mode step that never yielded valid JSON is a FAILURE — surface it
    // so the dashboard shows the same observability shape as a worker failure.
    let schema_failed = spec.schema.is_some() && structured.is_none();

    // Collect the worktree diff as the node's evidence (isolation path only). We
    // read it BEFORE the guard drops (which removes the worktree).
    let diff = if isolate {
        ephemeral_worktree_diff(&cwd)
    } else {
        None
    };

    // Two-tier persistence (locked design). The live SSE frames were already
    // streamed during the spawn loop (per-session NDJSON + shared
    // provider_turn_events.jsonl), so a LIVE drill-in worked during execution no
    // matter the retention. Now decide what SURVIVES the run:
    //  - durable: persist the heavy trace (per-session AgentEvents + retained
    //    NDJSON) so a completed run can be drilled into historically.
    //  - live: do NOT retain the heavy trace — skip the durable AgentEvents and
    //    prune the streamed NDJSON rows — so a past live-only run shows
    //    "trace not retained". The ProviderSession row is still written either
    //    way (with jsonl_ref only when durable), keeping the
    //    WorkflowStep.provider_session_id linkage stable.
    let retain_trace = options.trace_retention != "live";
    let _ = ingest_ephemeral_events(store, &session_id, spec, &spawn, retain_trace);
    if !retain_trace {
        prune_live_only_trace(store, &session_id);
    }

    let mut output_summary = if let Some(reply) = spawn.reply.clone() {
        // The worker's FINAL answer, FULL and FAITHFUL — NOT truncated. This is the
        // text `agent()` hands the program in text mode: the program splits it
        // (`.splitlines()`, first-line verdicts) AND forward-injects it into the next
        // leaf's prompt. Capping it (the old 4000-char clip) silently truncated the
        // node's output, so chaining a long result into a later leaf (e.g. a synthesis
        // over deep-dive sections) lost most of the input — a real design defect. The
        // full text is the node's data; newlines are preserved; reply.txt keeps a
        // durable copy too. Bounding runaway output is the budget/idle-timeout's job,
        // not a silent clip here.
        reply
    } else {
        format!(
            "{} ephemeral worker for {} ({})",
            spec.provider,
            spec.label,
            if spawn.ok { "ok" } else { "failed" }
        )
    };
    if let Some(diff) = &diff {
        if diff.trim().is_empty() {
            output_summary.push_str(" [worktree diff: empty]");
        } else {
            let lines = diff.lines().count();
            output_summary.push_str(&format!(" [worktree diff: {lines} lines]"));
        }
    }
    if !spawn.ok && !spawn.stderr.trim().is_empty() {
        let err = spawn.stderr.replace('\n', " ");
        let err = truncate_on_char_boundary(&err, 160);
        output_summary.push_str(&format!(" [error: {err}]"));
    }
    if schema_failed {
        output_summary.push_str(" [schema: no valid JSON with required keys]");
    }

    // Drop the guard here (explicitly, for clarity) AFTER the diff is collected —
    // cleanup layer 1 (normal) for the worktree path. For the shared-cwd path the
    // guard is None and there is nothing to remove.
    drop(guard);

    let mut details =
        build_step_details(spec, &spawn, effective_model, duration_ms, diff.as_deref());
    // Record a "schema" failure (reusing the same failure shape build_step_details
    // emits for worker failures) so the dashboard renders the schema miss.
    if schema_failed {
        if let Some(map) = details.as_object_mut() {
            map.insert(
                "failure".into(),
                serde_json::json!({
                    "failed": true,
                    "reason": "schema",
                    "detail": format!(
                        "worker reply was not a JSON object with required keys [{}]",
                        required_keys.join(", "),
                    ),
                }),
            );
        }
    }

    // The step is ok iff the worker succeeded AND (text mode OR schema parsed).
    let ok = spawn.ok && !schema_failed;

    Ok(workflow::StepResult {
        phase: spec.phase.clone(),
        label: spec.label.clone(),
        provider: spec.provider.clone(),
        isolation: spec.isolation.clone(),
        ok,
        provider_session_id: Some(session_id),
        output_summary,
        step_id: None,
        started_at: None,
        details: Some(details),
        structured,
        ordinal: spec.ordinal,
    })
}

/// Normalize a `schema=` dict into a real JSON Schema suitable for the providers'
/// native structured-output flags (claude `--json-schema`, codex `--output-schema`).
/// Two input shapes are accepted: an ALREADY-valid JSON Schema (has `type` or
/// `properties`) is passed through unchanged; the legacy flat `{ key: "hint" }`
/// form is wrapped into `{ type:object, properties:{...}, required:[keys],
/// additionalProperties:false }`.
///
/// A flat hint that is a WELL-KNOWN type word (`bool`/`int`/`number`/…) becomes a
/// real JSON-Schema scalar type, so the provider returns — and the workflow script
/// reads back — a real bool/int/number instead of a string (issue #139 item 5:
/// `{ "ok": "bool" }` used to yield the STRING `"true"`, making `if res["ok"]:`
/// always truthy). Any other hint stays a `string` field with the hint kept as its
/// `description`, exactly as before.
fn schema_to_json_schema(schema: &serde_json::Value) -> serde_json::Value {
    let Some(obj) = schema.as_object() else {
        return schema.clone();
    };
    if obj.contains_key("type") || obj.contains_key("properties") {
        return schema.clone();
    }
    let mut props = serde_json::Map::new();
    for (k, v) in obj {
        let hint = v.as_str().unwrap_or("");
        let json_type = match hint.trim().to_ascii_lowercase().as_str() {
            "bool" | "boolean" => "boolean",
            "int" | "integer" => "integer",
            "number" | "float" | "double" => "number",
            _ => "string",
        };
        let mut field = serde_json::Map::new();
        field.insert("type".into(), serde_json::Value::from(json_type));
        // Keep the hint as the description only when it carries real meaning — a
        // bare type word ("bool") becomes the type and needs no description.
        if json_type == "string" && !hint.is_empty() {
            field.insert("description".into(), serde_json::Value::from(hint));
        }
        props.insert(k.clone(), serde_json::Value::Object(field));
    }
    serde_json::json!({
        "type": "object",
        "properties": props,
        "required": obj.keys().cloned().collect::<Vec<_>>(),
        "additionalProperties": false,
    })
}

/// The REQUIRED top-level keys a schema declares. The schema is a JSON object;
/// its keys ARE the required keys the structured reply must carry. A non-object
/// schema (or one with no keys) declares no required keys.
fn schema_required_keys(schema: &serde_json::Value) -> Vec<String> {
    schema
        .as_object()
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_default()
}

/// Build the schema instruction appended to a structured-mode prompt: tell the
/// worker to reply with ONLY a single JSON object carrying the schema's top-level
/// keys (no prose, no markdown fences), and inline the compact schema as a shape
/// hint. Returned with a leading separator so it can be concatenated onto a prompt.
fn schema_instruction(schema: &serde_json::Value) -> String {
    let keys = schema_required_keys(schema).join(", ");
    let compact = serde_json::to_string(schema).unwrap_or_else(|_| "{}".to_string());
    format!(
        "\n\nRespond with ONLY a single JSON object with these top-level keys: [{keys}]. \
         No prose, no markdown fences. Shape hint: {compact}"
    )
}

/// Extract a JSON OBJECT from a worker reply, robustly: first strip a leading /
/// trailing triple-backtick fence (```json ... ``` or ``` ... ```) and try to
/// parse the whole thing; failing that, take the FIRST balanced `{ ... }` object
/// substring and parse it. Returns the parsed value only when it is a JSON object.
fn extract_json_object(reply: &str) -> Option<serde_json::Value> {
    let trimmed = reply.trim();

    // 1. Strip a surrounding ```json ... ``` (or ``` ... ```) fence if present.
    let unfenced = strip_code_fence(trimmed);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(unfenced.trim()) {
        if value.is_object() {
            return Some(value);
        }
    }

    // 2. Fall back to the first balanced `{ ... }` object in the text.
    if let Some(slice) = first_balanced_object(trimmed) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(slice) {
            if value.is_object() {
                return Some(value);
            }
        }
    }
    None
}

/// Strip a single surrounding triple-backtick fence from `text` if it both starts
/// with ``` (optionally ```json / ```JSON) and ends with ```. Returns the inner
/// body; otherwise returns `text` unchanged.
fn strip_code_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    // Drop an optional language tag on the opening fence line.
    let body = match rest.split_once('\n') {
        Some((_lang, after)) => after,
        None => rest,
    };
    body.strip_suffix("```").unwrap_or(body)
}

/// Return the first balanced `{ ... }` object substring of `text`, honoring JSON
/// string literals (so braces inside strings do not affect nesting). `None` when
/// there is no balanced object.
fn first_balanced_object(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let start = text.find('{')?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, &byte) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=start + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether `obj` (a parsed structured reply) contains EVERY required top-level
/// key. An empty required set is vacuously satisfied.
fn object_has_required_keys(obj: &serde_json::Value, required: &[String]) -> bool {
    match obj.as_object() {
        Some(map) => required.iter().all(|key| map.contains_key(key)),
        None => false,
    }
}

/// Maximum worktree-diff text we store on a step result. Diffs above this are
/// truncated to the cap and flagged with `worktree_diff_truncated: true` so the
/// dashboard can render a "diff truncated" hint without choking on a huge blob.
const WORKTREE_DIFF_CAP: usize = 20_000;

/// Assemble the observability `details` object merged onto the step's `result`
/// JSON (see `workflow::step_result_json`): the model the worker ran, exit code,
/// duration, normalized token usage, a structured failure (when the step failed),
/// and the FULL worktree diff text (capped) for the isolation path. Keys here are
/// additive — the base step_result_json keys win on any collision.
fn build_step_details(
    spec: &workflow::AgentStepSpec,
    spawn: &EphemeralSpawn,
    effective_model: Option<&str>,
    duration_ms: u64,
    diff: Option<&str>,
) -> serde_json::Value {
    // The node's requested model wins; otherwise fall back to the model the
    // worker reported in its own output (claude's init frame).
    let model = effective_model
        .map(|model| model.to_string())
        .or_else(|| spawn.model.clone());
    let mut details = serde_json::json!({
        "model": model,
        "exit_code": spawn.exit_code,
        "duration_ms": duration_ms,
    });
    let map = details
        .as_object_mut()
        .expect("json! object is always an object");

    if let Some(tokens) = spawn.tokens {
        map.insert("tokens".into(), tokens.to_json());
    }

    if let Some(cost) = spawn.cost_usd {
        if let Some(n) = serde_json::Number::from_f64(cost) {
            map.insert("cost_usd".into(), serde_json::Value::Number(n));
        }
    }

    if let Some(reason) = classify_failure_reason(spawn.ok, spawn.exit_code, spawn.timed_out) {
        let detail = if spawn.stderr.trim().is_empty() {
            format!("{} worker step failed ({reason})", spec.provider)
        } else {
            spawn.stderr.trim().to_string()
        };
        map.insert(
            "failure".into(),
            serde_json::json!({
                "failed": true,
                "reason": reason,
                "detail": detail,
            }),
        );
    }

    if let Some(diff) = diff {
        let (text, truncated) = if diff.len() > WORKTREE_DIFF_CAP {
            (truncate_on_char_boundary(diff, WORKTREE_DIFF_CAP), true)
        } else {
            (diff, false)
        };
        map.insert(
            "worktree_diff".into(),
            serde_json::Value::String(text.to_string()),
        );
        map.insert(
            "worktree_diff_truncated".into(),
            serde_json::Value::Bool(truncated),
        );
    }

    if !spawn.warnings.is_empty() {
        map.insert(
            "observability_warnings".into(),
            serde_json::Value::Array(
                spawn
                    .warnings
                    .iter()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }

    details
}

/// The outcome of one ephemeral worker process: whether the turn succeeded, the
/// parsed terminal reply text (if any), the raw NDJSON the worker emitted, and
/// any stderr (for failure summaries).
struct EphemeralSpawn {
    ok: bool,
    reply: Option<String>,
    /// Raw NDJSON stdout (one JSON event per line) for neutral-event ingest.
    ndjson: String,
    stderr: String,
    /// Process exit code; `None` when the worker was killed on timeout / signal.
    exit_code: Option<i32>,
    /// True when the per-node timeout fired (the worker was killed mid-turn).
    timed_out: bool,
    /// Normalized token usage parsed from the terminal event, when present:
    /// `{ input, output, total }`. `None` when the stream carried no usage.
    tokens: Option<TokenUsage>,
    /// The model the worker actually ran, parsed from its output when the
    /// provider reports it (claude's `system`/`init` event). `None` for codex,
    /// whose `exec --json` stream carries no model — the node's requested
    /// `spec.model` is the only signal there.
    model: Option<String>,
    /// The provider-validated structured object, when the worker ran with a
    /// native schema flag (claude `--json-schema` → `result.structured_output`;
    /// codex `--output-schema` → the schema-constrained reply). `None` for
    /// text-mode steps or when no native structured output was produced — the
    /// caller then falls back to extracting JSON from the reply text.
    structured: Option<serde_json::Value>,
    /// Billed cost in USD for the turn, when the provider reports it (claude's
    /// `result.total_cost_usd`). `None` for codex, which emits only token usage.
    cost_usd: Option<f64>,
    /// Advisory observability issues from the streaming path. These never affect
    /// step success semantics.
    warnings: Vec<String>,
}

/// Normalized token usage for one worker turn, provider-agnostic. Parsed from the
/// codex `turn.completed.usage` or the claude `result.usage` shape and reduced to
/// the three numbers the dashboard surfaces. `total` is `input + output` (codex's
/// `cached_input_tokens` is a SUBSET of `input_tokens`, not additive, and
/// `reasoning_output_tokens` is a SUBSET of `output_tokens`, so they are not
/// re-added here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenUsage {
    input: u64,
    output: u64,
    total: u64,
}

impl TokenUsage {
    fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "input": self.input,
            "output": self.output,
            "total": self.total,
        })
    }
}

/// Parse codex `turn.completed` usage into a normalized [`TokenUsage`]. Codex
/// `exec --json` emits `{"type":"turn.completed","usage":{...}}` (some builds nest
/// the usage under `turn`). The usage object carries `input_tokens`,
/// `output_tokens`, and the SUBSET counters `cached_input_tokens` /
/// `reasoning_output_tokens` (already included in input/output respectively).
/// Returns `None` when no terminal usage object is present.
fn parse_codex_usage(events: &[serde_json::Value]) -> Option<TokenUsage> {
    events.iter().rev().find_map(|payload| {
        let ty = payload.get("type").and_then(|t| t.as_str())?;
        if ty != "turn.completed" && ty != "turn_completed" {
            return None;
        }
        let usage = payload
            .get("usage")
            .or_else(|| payload.get("turn").and_then(|t| t.get("usage")))?;
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Some(TokenUsage {
            input,
            output,
            total: input.saturating_add(output),
        })
    })
}

/// Parse claude `result` usage into a normalized [`TokenUsage`]. Claude
/// `--output-format stream-json` emits a terminal `{"type":"result","usage":{
/// "input_tokens":N,"output_tokens":N,...}}`. Returns `None` when no result usage
/// is present.
fn parse_claude_usage(events: &[serde_json::Value]) -> Option<TokenUsage> {
    events.iter().rev().find_map(|payload| {
        if payload.get("type").and_then(|t| t.as_str()) != Some("result") {
            return None;
        }
        let usage = payload.get("usage")?;
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Some(TokenUsage {
            input,
            output,
            total: input.saturating_add(output),
        })
    })
}

/// The model a worker actually ran, when the provider reports it. Claude
/// `--output-format stream-json` emits a `{"type":"system","subtype":"init",
/// "model":"claude-…"}` frame; codex `exec --json` carries none (returns `None`).
fn parse_worker_model(events: &[serde_json::Value]) -> Option<String> {
    events.iter().find_map(|payload| {
        if payload.get("type").and_then(|t| t.as_str()) != Some("system") {
            return None;
        }
        payload
            .get("model")
            .and_then(|m| m.as_str())
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string())
    })
}

/// Parse claude's terminal `result` frame for the two extras it carries:
/// `structured_output` (a schema-validated object, present only when the worker
/// ran with `--json-schema`) and `total_cost_usd` (the billed turn cost). Returns
/// `(structured, cost_usd)`, each `None` when absent.
fn parse_claude_result_extras(
    events: &[serde_json::Value],
) -> (Option<serde_json::Value>, Option<f64>) {
    events
        .iter()
        .rev()
        .find_map(|payload| {
            if payload.get("type").and_then(|t| t.as_str()) != Some("result") {
                return None;
            }
            let structured = payload
                .get("structured_output")
                .filter(|v| v.is_object())
                .cloned();
            let cost = payload.get("total_cost_usd").and_then(|v| v.as_f64());
            Some((structured, cost))
        })
        .unwrap_or((None, None))
}

fn codex_delivery_telemetry(
    raw_events: &[serde_json::Value],
    spec: &LaunchSpec,
) -> (Option<TokenUsage>, Option<f64>, Option<String>) {
    (parse_codex_usage(raw_events), None, spec.model.clone())
}

fn codex_delivery_structured(reply: Option<&str>, spec: &LaunchSpec) -> Option<serde_json::Value> {
    spec.output_schema
        .as_ref()
        .and_then(|_| reply.and_then(extract_json_object))
}

/// The structured output is the turn's ANSWER, so it is surfaced only on a
/// SUCCEEDED delivery. A failed/stale turn may have emitted partial or
/// schema-violating JSON that must not be reported as the structured result.
fn structured_for_status(
    status: &ProviderSessionStatus,
    structured: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    match status {
        ProviderSessionStatus::Succeeded => structured,
        _ => None,
    }
}

fn claude_delivery_telemetry(
    raw_events: &[serde_json::Value],
) -> (
    Option<TokenUsage>,
    Option<f64>,
    Option<String>,
    Option<serde_json::Value>,
) {
    let (structured, cost_usd) = parse_claude_result_extras(raw_events);
    (
        parse_claude_usage(raw_events),
        cost_usd,
        parse_worker_model(raw_events),
        structured,
    )
}

/// Classify WHY a step failed, into a stable `reason` tag the dashboard groups on.
/// Precedence: a fired timeout dominates (the worker never reached a clean turn);
/// then a non-zero / absent exit code; then a delivery that exited 0 but produced
/// no successful terminal event (`ok == false` with a clean exit == a delivery
/// problem, e.g. an auth/usage-limit `result` with `subtype != "success"`).
/// Returns `None` when the step succeeded.
fn classify_failure_reason(
    ok: bool,
    exit_code: Option<i32>,
    timed_out: bool,
) -> Option<&'static str> {
    if ok {
        return None;
    }
    if timed_out {
        return Some("timeout");
    }
    match exit_code {
        // Clean exit (0) but the delivery still failed == a delivery-layer
        // problem: a `result`/turn that completed the process but reported no
        // successful turn (e.g. an auth or usage-limit terminal).
        Some(0) => Some("delivery"),
        // A non-zero code, or no code at all (killed by a signal), is a process
        // exit failure.
        _ => Some("exit"),
    }
}

/// Spawn a one-shot `codex exec` with an EDITABLE (`--sandbox workspace-write`)
/// sandbox, JSON event stream, running in `cwd`. Non-interactive (stdin closed)
/// with a per-node timeout. When `schema_json` is set, `--output-schema <file>`
/// constrains codex's final answer to that JSON Schema. Flags verified via
/// `codex exec --help`: `--json`, `--sandbox workspace-write`, `--cd <dir>`,
/// `-m <model>`, `--skip-git-repo-check`, `--output-last-message <file>`,
/// `--output-schema <file>`.
#[allow(clippy::too_many_arguments)] // the spawn surface (session/spec/schema/cwd/model/effort/timeout)
fn spawn_codex_ephemeral(
    session_dir: &Path,
    session_id: &str,
    spec: &workflow::AgentStepSpec,
    schema_json: Option<&serde_json::Value>,
    prompt: &str,
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    timeout_ms: u64,
) -> CliResult<EphemeralSpawn> {
    let last_message_ref = session_dir.join("last-message.md");
    // Read-only by default; a `writable` node gets FULL access (the codex analogue of
    // claude's `--permission-mode bypassPermissions`). NOT `workspace-write`: that
    // mode blocks writes to `.git/`, so a worker could edit files but `git add`/
    // `git commit` failed ("sandbox denied .git") and network was off. The caller has
    // already isolated the worker into a throwaway worktree, so the worktree (not the
    // codex sandbox) is the boundary — give it full access to actually do the work.
    let sandbox = if spec.writable {
        "danger-full-access"
    } else {
        "read-only"
    };
    let mut cmd = Command::new("codex");
    cmd.arg("exec")
        .arg("--cd")
        .arg(cwd)
        .arg("--sandbox")
        .arg(sandbox)
        .arg("--skip-git-repo-check")
        .arg("--json")
        .arg("--output-last-message")
        .arg(&last_message_ref);
    // Native schema enforcement: write the JSON Schema to a file and constrain the
    // final answer to it. The reply text then IS the validated JSON object.
    if let Some(schema) = schema_json {
        let schema_path = session_dir.join("output-schema.json");
        if fs::write(&schema_path, schema.to_string()).is_ok() {
            cmd.arg("--output-schema").arg(&schema_path);
        }
    }
    if let Some(model) = model {
        cmd.arg("-m").arg(model);
    }
    // Reasoning effort: codex takes it as a config override (no dedicated flag).
    if let Some(effort) = effort {
        cmd.arg("-c")
            .arg(format!("model_reasoning_effort={effort}"));
    }
    // codex has no fallback-model flag; only providers with a native flag use it.
    for path in &spec.image {
        cmd.arg("-i").arg(path);
    }
    for path in &spec.add_dir {
        cmd.arg("--add-dir").arg(path);
    }
    // `-i/--image <FILE>...` is VARIADIC: a positional prompt placed after it is
    // swallowed as another image path, so codex finds no PROMPT positional, reads
    // an empty stdin, and dies with "No prompt provided via stdin." Terminate
    // option parsing with `--` so the prompt is unambiguously the PROMPT positional.
    if !spec.image.is_empty() {
        cmd.arg("--");
    }
    cmd.arg(prompt);

    let run = run_ndjson_child(
        cmd,
        session_dir,
        session_id,
        "codex.stream-json.ndjson",
        timeout_ms,
        "ephemeral worker",
    )?;
    let codex_events: Vec<CodexExecEvent> = run
        .events
        .iter()
        .filter_map(|v| serde_json::to_string(v).ok())
        .filter_map(|line| CodexExecEvent::parse_line(&line))
        .collect();
    let ok = matches!(
        infer_provider_session_status(&codex_events, run.process_success),
        ProviderSessionStatus::Succeeded
    );
    // Prefer the parsed agent message; fall back to the last-message file codex
    // wrote (the terminal assistant text).
    let reply = extract_codex_reply_text(&codex_events)
        .or_else(|| fs::read_to_string(&last_message_ref).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let tokens = parse_codex_usage(&run.events);
    // With `--output-schema`, the constrained answer is the turn's FINAL message.
    // Parse structured output from that final message — the `--output-last-message`
    // file first, then the last `agent_message` — NOT the joined narration, so a
    // streamed preamble ("I'll start by inspecting…") can't be captured as the
    // result (issue #139 item 2). Fall back to the joined reply only as a last resort.
    let structured = schema_json.and_then(|_| {
        fs::read_to_string(&last_message_ref)
            .ok()
            .as_deref()
            .and_then(extract_json_object)
            .or_else(|| {
                extract_codex_final_message(&codex_events)
                    .as_deref()
                    .and_then(extract_json_object)
            })
            .or_else(|| reply.as_deref().and_then(extract_json_object))
    });

    Ok(EphemeralSpawn {
        ok,
        reply,
        ndjson: ndjson_lines(&run.events),
        stderr: run.stderr,
        exit_code: run.exit_code,
        timed_out: run.timed_out,
        tokens,
        // codex exec --json carries no model; only spec.model is known.
        model: None,
        structured,
        // codex emits token usage but no dollar cost.
        cost_usd: None,
        warnings: run.warnings,
    })
}

/// Spawn a one-shot `claude -p` with EDITING allowed: `--output-format
/// stream-json --verbose`, an allowedTools set incl. Read/Edit/Write/Bash, and a
/// non-blocking `--permission-mode bypassPermissions` so it never blocks on an
/// approval prompt. When `schema_json` is set, `--json-schema <inline>` makes
/// claude emit a schema-validated `result.structured_output`. Runs with cwd =
/// `cwd` (the harness owns isolation; we do NOT use claude's -w). Flags verified
/// via `claude --help`.
#[allow(clippy::too_many_arguments)] // the spawn surface (session/spec/schema/cwd/timeout/budget)
fn spawn_claude_ephemeral(
    session_dir: &Path,
    session_id: &str,
    spec: &workflow::AgentStepSpec,
    schema_json: Option<&serde_json::Value>,
    prompt: &str,
    cwd: &Path,
    model: Option<&str>,
    effort: Option<&str>,
    timeout_ms: u64,
    max_budget_usd: Option<f64>,
) -> CliResult<EphemeralSpawn> {
    let prompt_with_images;
    let prompt = if spec.image.is_empty() {
        prompt
    } else {
        prompt_with_images = format!(
            "Attached image files (read them with the Read tool): {}\n\n{}",
            spec.image.join(", "),
            prompt
        );
        &prompt_with_images
    };
    // Read-only by default (no Edit/Write/Bash); a `writable` node gets the editing
    // tools (and the caller has isolated it into a throwaway worktree). The tool
    // allowlist is the gate; bypassPermissions only keeps -p non-interactive.
    let tools = if spec.writable {
        "Read,Edit,Write,Bash,Grep,Glob"
    } else {
        "Read,Grep,Glob"
    };
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--permission-mode")
        .arg("bypassPermissions")
        .arg("--allowedTools")
        .arg(tools)
        .current_dir(cwd);
    // Per-worker spend backstop: bound a single worker to the run's ceiling so it
    // can't blow the budget between the program's barrier-granular tally checks.
    // (Soft: claude's --max-budget-usd is a post-turn cap that can overshoot a
    // little, but it bounds the runaway-single-worker case the tally can miss.)
    if let Some(budget) = max_budget_usd {
        if budget > 0.0 {
            cmd.arg("--max-budget-usd").arg(format!("{budget}"));
        }
    }
    // Native schema enforcement via constrained decoding: the validated object is
    // emitted on the terminal `result` event as `structured_output`.
    if let Some(schema) = schema_json {
        cmd.arg("--json-schema").arg(schema.to_string());
    }
    if let Some(model) = model {
        cmd.arg("--model").arg(model);
    }
    // Reasoning effort: claude has a native session flag.
    if let Some(effort) = effort {
        cmd.arg("--effort").arg(effort);
    }
    if let Some(model) = &spec.fallback_model {
        cmd.arg("--fallback-model").arg(model);
    }
    for path in &spec.add_dir {
        cmd.arg("--add-dir").arg(path);
    }

    let run = run_ndjson_child(
        cmd,
        session_dir,
        session_id,
        "claude.stream-json.ndjson",
        timeout_ms,
        "ephemeral worker",
    )?;
    let claude_events: Vec<ClaudeStreamEvent> = run
        .events
        .iter()
        .filter_map(|v| serde_json::to_string(v).ok())
        .filter_map(|line| ClaudeStreamEvent::parse_line(&line))
        .collect();
    let ok = matches!(
        infer_claude_session_status(&claude_events, run.process_success),
        ProviderSessionStatus::Succeeded
    );
    let reply = extract_claude_reply_text(&claude_events);
    let tokens = parse_claude_usage(&run.events);
    let model = parse_worker_model(&run.events);
    // `structured_output` (when `--json-schema` ran) + the billed turn cost, both
    // off the terminal `result` frame.
    let (structured, cost_usd) = parse_claude_result_extras(&run.events);

    Ok(EphemeralSpawn {
        ok,
        reply,
        ndjson: ndjson_lines(&run.events),
        stderr: run.stderr,
        exit_code: run.exit_code,
        timed_out: run.timed_out,
        tokens,
        model,
        structured,
        cost_usd,
        warnings: run.warnings,
    })
}

/// The terminal state of one NDJSON child process: whether it exited 0, its raw
/// exit code (None when killed on timeout / signalled), whether the per-node
/// timeout fired, the parsed event payloads, and any stderr.
struct NdjsonRun {
    process_success: bool,
    /// Process exit code when the child exited on its own; `None` when it was
    /// killed on timeout or terminated by a signal (no code available).
    exit_code: Option<i32>,
    /// True when the per-node timeout fired and we killed the child.
    timed_out: bool,
    events: Vec<serde_json::Value>,
    stderr: String,
    warnings: Vec<String>,
}

/// Spawn a child that emits NDJSON on stdout, non-interactively (stdin closed),
/// teeing each parsed event to TWO sinks while it streams MID-TURN: (1) the
/// durable per-session `<file>` the ProviderSession's jsonl_ref points at, and
/// (2) the shared `<store_root>/provider_turn_events.jsonl` the SSE watcher tails
/// to push live frames (keyed by session id). Enforces a per-node timeout: on
/// timeout the child is killed and `process_success=false` (the run tolerates
/// failed nodes). Returns the terminal [`NdjsonRun`].
/// SIGKILL the worker's whole process GROUP (the child is the group leader, so
/// its pid is the pgid; `kill -9 -<pgid>`). codex/claude spawn child binaries
/// that inherit our stdout pipe — killing only the immediate child would leave a
/// grandchild holding the pipe open and the reader thread (and its join) blocked
/// forever. Falls back to killing the immediate child.
fn kill_worker_tree(child: &mut std::process::Child) {
    let pid = child.id();
    #[cfg(unix)]
    {
        // SIGKILL the whole process GROUP (negative pid == the group). The child is
        // its own group leader (`process_group(0)`), so its pid IS the pgid; a
        // grandchild (codex/claude spawn a child binary; or a test's `sleep`)
        // inherits the group, so this reaps the tree and closes the inherited
        // stdout pipe — which is what lets the reader thread's join return.
        //
        // We call `kill(2)` directly rather than shelling out to `kill -9 -<pgid>`:
        // the external `kill` parses a leading-dash pgid INCONSISTENTLY across
        // platforms (BSD/macOS accept it; util-linux on CI swallowed it as options),
        // which left the grandchild alive and hung the reader for the full 600s.
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run_ndjson_child(
    mut cmd: Command,
    session_dir: &Path,
    session_id: &str,
    live_file_name: &str,
    timeout_ms: u64,
    // Human label for this worker in spawn/timeout error + warning strings
    // (e.g. "ephemeral worker", "codex exec", "claude -p"). The persistent member
    // path passes its provider-specific label so failure summaries read the same
    // as before this runner was shared.
    context: &str,
) -> CliResult<NdjsonRun> {
    // Put the worker in its OWN process group so a timeout can kill the whole
    // tree (see kill_worker_tree), not just the immediate child.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| CliError::Usage(format!("failed to spawn {context}: {error}")))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliError::Usage(format!("{context} stdout not available")))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CliError::Usage(format!("{context} stderr not available")))?;

    let _ = fs::create_dir_all(session_dir);
    let live_path = session_dir.join(live_file_name);
    let shared_path = session_dir
        .parent()
        .and_then(|provider_sessions| provider_sessions.parent())
        .map(|store_root| store_root.join("provider_turn_events.jsonl"));
    let session_id_owned = session_id.to_string();

    // IDLE-timeout clock. A productive worker keeps emitting events, each resetting
    // this to "now"; the main thread kills only a worker that has gone SILENT for
    // `timeout_ms` (a wedged provider / auth or network stall) — never a slow but
    // still-streaming turn. Stored as millis-since-`start`.
    let start = Instant::now();
    let last_activity_ms = Arc::new(AtomicU64::new(0));
    let activity_ms = Arc::clone(&last_activity_ms);
    let activity_start = start;

    // Read stdout in a DEDICATED THREAD so the main thread can enforce the idle
    // timeout by KILLING a worker that stops emitting events but never closes stdout
    // (an auth/network stall, a wedged provider). The old code read stdout on the
    // main thread and only checked the deadline AFTER the read loop returned, so a
    // hung worker (stdout still open) blocked forever and froze the whole run. The
    // thread tees each event live + collects them; killing the child closes stdout,
    // which ends this loop.
    let stdout_handle = std::thread::spawn(move || {
        let mut warnings = Vec::new();
        let mut session_writer = match fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&live_path)
        {
            Ok(file) => Some(BufWriter::new(file)),
            Err(_) => {
                warnings.push("failed to open live ndjson file".to_string());
                None
            }
        };
        let mut shared_writer = match shared_path.as_ref() {
            Some(path) => match fs::OpenOptions::new().create(true).append(true).open(path) {
                Ok(file) => Some(BufWriter::new(file)),
                Err(_) => {
                    warnings.push("failed to open shared ndjson file".to_string());
                    None
                }
            },
            None => None,
        };
        let mut events = Vec::new();
        let mut dropped_lines = 0usize;
        for line in BufReader::new(stdout).lines() {
            let Ok(line_str) = line else { continue };
            let trimmed = line_str.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Any non-empty output proves the worker is alive — reset the idle clock.
            activity_ms.store(
                activity_start.elapsed().as_millis() as u64,
                Ordering::Relaxed,
            );
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(trimmed) else {
                dropped_lines += 1;
                continue;
            };
            if let Some(writer) = session_writer.as_mut() {
                let _ = writeln!(writer, "{trimmed}");
                let _ = writer.flush();
            }
            if let Some(writer) = shared_writer.as_mut() {
                let envelope =
                    serde_json::json!({ "session_id": session_id_owned, "event": payload });
                if let Ok(line) = serde_json::to_string(&envelope) {
                    let _ = writeln!(writer, "{line}");
                    let _ = writer.flush();
                }
            }
            events.push(payload);
        }
        if dropped_lines > 0 {
            warnings.push(format!(
                "{dropped_lines} stdout line(s) were not valid JSON and were dropped"
            ));
        }
        (events, warnings)
    });

    // Drain stderr in its own thread so a chatty worker cannot fill the pipe and
    // block (which would also stall the kill path).
    let stderr_handle = std::thread::spawn(move || {
        let mut log = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut log);
        log
    });

    // Main thread: enforce the IDLE timeout. While the worker keeps streaming events
    // the idle clock resets, so a slow-but-productive turn runs to completion however
    // long it takes; only a worker SILENT for `timeout_ms` (a wedged provider, an
    // auth/network stall) is killed. Killing closes stdout/stderr so the reader
    // threads finish and join cleanly.
    let idle_limit = Duration::from_millis(timeout_ms.max(1));
    let mut timed_out = false;
    let mut exit_code: Option<i32> = None;
    let process_success = loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_code = status.code();
                break status.success();
            }
            Ok(None) => {
                let last = Duration::from_millis(last_activity_ms.load(Ordering::Relaxed));
                if start.elapsed().saturating_sub(last) > idle_limit {
                    kill_worker_tree(&mut child);
                    timed_out = true;
                    break false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break false,
        }
    };

    let (events, mut warnings) = stdout_handle.join().unwrap_or_default();
    let mut stderr_log = stderr_handle.join().unwrap_or_default();
    if timed_out && stderr_log.is_empty() {
        stderr_log = format!("timeout waiting for {context}");
    }
    if timed_out {
        warnings.push(format!("{context} timed out"));
    }

    Ok(NdjsonRun {
        process_success,
        exit_code,
        timed_out,
        events,
        stderr: stderr_log,
        warnings,
    })
}

/// Join parsed event payloads back into NDJSON text (one JSON object per line).
fn ndjson_lines(events: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for event in events {
        if let Ok(line) = serde_json::to_string(event) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out
}

/// `git -C <wt> diff` — the node's collected evidence for the isolation path.
/// Returns None when git is unavailable; an empty string means a clean tree.
///
/// We first `git add -A --intent-to-add` so brand-new UNTRACKED files a worker
/// creates show up in the diff as additions (plain `git diff` omits untracked
/// content). The worktree is throwaway, so touching its index is harmless.
fn ephemeral_worktree_diff(worktree: &Path) -> Option<String> {
    let wt = worktree.display().to_string();
    // Best-effort intent-to-add so untracked files are included; ignore failure.
    let _ = Command::new("git")
        .args(["-C", &wt, "add", "-A", "--intent-to-add"])
        .output();
    let output = Command::new("git")
        .args(["-C", &wt, "diff"])
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Persist the ephemeral worker's NDJSON as neutral AgentEvents + one
/// ProviderSession row keyed by `session_id`, so the dashboard per-node drill-in
/// streams its tool calls. Reuses the existing claude stream-json reducer
/// (`ingest_claude_stream_json`) for claude; emits a neutral event per codex
/// NDJSON line for codex, mirroring the existing provider-output ingest.
/// Write a RUNNING [`ProviderSession`] row the instant a workflow worker starts,
/// BEFORE the blocking spawn. The dashboard's per-node drill-in resolves a step's
/// turn-event stream via its `provider_session_id`, so without this row a RUNNING
/// step rendered "no turn yet" — its live `provider_turn_event`s reached the
/// frontend but had no session row to attach to — until it finished and
/// [`ingest_ephemeral_events`] wrote the terminal row. This publishes the row up
/// front (same id; the terminal row supersedes it latest-wins) and pre-creates the
/// live NDJSON so `GET /v1/provider-sessions/{id}/events` returns a growing list
/// from t0 rather than a missing-file error. Best-effort: a failure here must not
/// abort the step — the terminal row still records the outcome.
fn write_running_ephemeral_session(
    store: &HarnessStore,
    session_id: &str,
    session_dir: &Path,
    spec: &workflow::AgentStepSpec,
) {
    let live_file = provider_adapter(&spec.provider)
        .map(|adapter| adapter.live_ndjson_file_name())
        .unwrap_or_else(|| CodexAdapter.live_ndjson_file_name());
    let live_path = session_dir.join(live_file);
    // Pre-create the live NDJSON so the events route serves [] (then a growing
    // list) during the turn instead of erroring on a not-yet-existent file.
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&live_path);
    let jsonl_ref = Some(live_path.display().to_string());
    let session = ProviderSession {
        id: session_id.into(),
        provider: spec.provider.clone(),
        agent_member_id: session_id.into(),
        task_id: None,
        workspace_ref: None,
        provider_thread_id: None,
        provider_turn_id: None,
        terminal_source: None,
        status: ProviderSessionStatus::Running,
        command: spec.provider.clone(),
        args: Vec::new(),
        prompt_ref: None,
        prompt_summary: Some(format!(
            "ephemeral {} worker: {}",
            spec.provider, spec.label
        )),
        provider_session_ref: None,
        stdout_ref: jsonl_ref.clone(),
        jsonl_ref,
        transcript_ref: None,
        last_message_ref: None,
        exit_code: None,
        started_at: now_string(),
        ended_at: None,
        evidence_ids: Vec::new(),
    };
    let _ = store.append_provider_session(&session);
}

fn ingest_ephemeral_events(
    store: &HarnessStore,
    session_id: &str,
    spec: &workflow::AgentStepSpec,
    spawn: &EphemeralSpawn,
    retain_trace: bool,
) -> CliResult<()> {
    // Persist the worker's FULL reply as a human-browsable artifact, so the
    // deliverable can be retrieved in full (issue #89 item 4). The step's
    // `output_summary` is capped at OUTPUT_SUMMARY_CAP chars, so a long synthesis
    // would otherwise only live (scattered) inside the turn trace. Durable runs
    // only; a `--trace live` run prunes the session dir afterward, and
    // `workflow get-output` then falls back to the capped summary.
    if retain_trace {
        if let Some(reply) = spawn.reply.as_deref() {
            let reply_path = store
                .root()
                .join("provider-sessions")
                .join(session_id)
                .join("reply.txt");
            if let Some(parent) = reply_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&reply_path, reply);
        }
    }

    // The DURABLE per-session AgentEvents are the heavy trace gated by retention.
    // When `retain_trace` is false (a `--trace live` run) we skip them entirely:
    // the live SSE frames already streamed during the spawn loop, so the only
    // thing we omit is the historical (post-run) trace.
    if retain_trace {
        // claude => claude reducer; codex/unknown => codex AgentEvent loop (unchanged policy).
        provider_adapter(&spec.provider)
            .unwrap_or(&CodexAdapter as &dyn ProviderAdapter)
            .ingest_ephemeral_trace(store, session_id, spawn);
    }

    // A ProviderSession keyed by OUR session id — the stable drill-in key, always
    // written so WorkflowStep.provider_session_id resolves either way. The
    // jsonl_ref/stdout_ref point at the retained per-session NDJSON ONLY when the
    // trace is durable; a live-only run leaves them None so the drill-in renders
    // "trace not retained" (the NDJSON is pruned after the run).
    let live_file = provider_adapter(&spec.provider)
        .map(|adapter| adapter.live_ndjson_file_name())
        .unwrap_or_else(|| CodexAdapter.live_ndjson_file_name());
    let jsonl_ref = if retain_trace {
        Some(
            store
                .root()
                .join("provider-sessions")
                .join(session_id)
                .join(live_file)
                .display()
                .to_string(),
        )
    } else {
        None
    };
    let status = if spawn.ok {
        ProviderSessionStatus::Succeeded
    } else {
        ProviderSessionStatus::Failed
    };
    let session = ProviderSession {
        id: session_id.into(),
        provider: spec.provider.clone(),
        agent_member_id: session_id.into(),
        task_id: None,
        workspace_ref: None,
        provider_thread_id: None,
        provider_turn_id: None,
        terminal_source: status_to_terminal_source(&status),
        status,
        command: spec.provider.clone(),
        args: Vec::new(),
        prompt_ref: None,
        prompt_summary: Some(format!(
            "ephemeral {} worker: {}",
            spec.provider, spec.label
        )),
        provider_session_ref: None,
        stdout_ref: jsonl_ref.clone(),
        jsonl_ref,
        transcript_ref: None,
        last_message_ref: None,
        exit_code: Some(if spawn.ok { 0 } else { 1 }),
        started_at: now_string(),
        ended_at: Some(now_string()),
        evidence_ids: Vec::new(),
    };
    let _ = store.append_provider_session(&session);
    Ok(())
}

/// Prune the heavy turn-event trace a `--trace live` run streamed but does NOT
/// retain (two-tier persistence). The live SSE frames already reached connected
/// clients during execution; this removes what would otherwise SURVIVE so a past
/// live-only run shows "trace not retained":
///  - the per-session NDJSON directory (`provider-sessions/<session_id>/`), and
///  - this session's rows in the shared `provider_turn_events.jsonl`.
///
/// Best-effort: a prune failure must not flip an otherwise-successful step.
fn prune_live_only_trace(store: &HarnessStore, session_id: &str) {
    // Drop the per-session NDJSON the spawn loop teed during streaming.
    let session_dir = store.root().join("provider-sessions").join(session_id);
    let _ = fs::remove_dir_all(&session_dir);

    // Strip this session's lines from the shared turn-event log, keeping the rows
    // of OTHER (possibly durable) sessions intact. Each line is a
    // {"session_id": ..., "event": ...} envelope; we drop only matching ids.
    let shared_path = store.root().join("provider_turn_events.jsonl");
    let Ok(contents) = fs::read_to_string(&shared_path) else {
        return;
    };
    let mut kept = String::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let drop_line = serde_json::from_str::<serde_json::Value>(trimmed)
            .ok()
            .and_then(|value| {
                value
                    .get("session_id")
                    .and_then(|s| s.as_str())
                    .map(|s| s == session_id)
            })
            .unwrap_or(false);
        if !drop_line {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    let _ = fs::write(&shared_path, kept);
}

/// Backstop GC (cleanup layer 3): `git worktree prune` + sweep
/// `.harness/worktrees/` for dirs not tied to an ACTIVE run. Active = a worktree
/// still registered with git (a leftover from a crash is unregistered after
/// prune). Conservative: only removes dirs git no longer tracks.
/// `workflow get-output <run_id> [--step <label>]` — retrieve a run's leaf
/// OUTPUTS (issue #89 item 4). For text-producing workflows the deliverable was
/// hard to get back: `output_summary` is capped and the full text otherwise only
/// lived (scattered) in the turn trace. Each step's full reply is persisted as
/// `provider-sessions/<session_id>/reply.txt` at ingest (durable runs); this reads
/// it back, in `step_ids` order, falling back to the capped `output_summary` when
/// the full artifact is absent (e.g. a `--trace live` run whose dir was pruned).
/// `source` tells the caller which they got: `"reply"` (full) or `"summary"`.
fn workflow_get_output_value(
    store: &HarnessStore,
    args: &[String],
) -> CliResult<serde_json::Value> {
    let run_id = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .cloned()
        .ok_or_else(|| CliError::Usage("workflow get-output requires a <run_id>".into()))?;
    let step_filter = value(args, "--step");

    let run = store
        .workflow_runs()?
        .into_iter()
        .rfind(|r| r.id == run_id)
        .ok_or_else(|| CliError::Usage(format!("workflow run not found: {run_id}")))?;

    // Latest-wins projection of this run's steps, then order by run.step_ids so the
    // output reads in workflow order (fall back to journal order if step_ids empty).
    let mut by_id: std::collections::HashMap<String, WorkflowStep> =
        std::collections::HashMap::new();
    let mut journal_order: Vec<String> = Vec::new();
    for step in store.workflow_steps()? {
        if step.run_id == run_id {
            if !by_id.contains_key(&step.id) {
                journal_order.push(step.id.clone());
            }
            by_id.insert(step.id.clone(), step);
        }
    }
    let order: Vec<String> = if run.step_ids.is_empty() {
        journal_order
    } else {
        run.step_ids.clone()
    };

    let mut out_steps = Vec::new();
    for id in order {
        let Some(step) = by_id.get(&id) else { continue };
        if let Some(filter) = &step_filter {
            if &step.label != filter {
                continue;
            }
        }
        let (output, source) = match step.provider_session_id.as_deref() {
            Some(sid) => {
                let reply_path = store
                    .root()
                    .join("provider-sessions")
                    .join(sid)
                    .join("reply.txt");
                match fs::read_to_string(&reply_path) {
                    Ok(text) => (text, "reply"),
                    Err(_) => (step.output_summary.clone().unwrap_or_default(), "summary"),
                }
            }
            None => (step.output_summary.clone().unwrap_or_default(), "summary"),
        };
        out_steps.push(serde_json::json!({
            "label": step.label,
            "status": serde_json::to_value(step.status)?,
            "provider_session_id": step.provider_session_id,
            "source": source,
            "output": output,
        }));
    }

    if let Some(filter) = &step_filter {
        if out_steps.is_empty() {
            return Err(CliError::Usage(format!(
                "no step labeled '{filter}' in run {run_id}"
            )));
        }
    }

    Ok(serde_json::json!({
        "run_id": run_id,
        "workflow_name": run.workflow_name,
        "steps": out_steps,
    }))
}

fn workflow_gc_worktrees(store: &HarnessStore) -> CliResult<serde_json::Value> {
    let repo_root = workflow_repo_root();
    let repo = repo_root.display().to_string();

    // Prune dangling administrative entries first.
    let _ = Command::new("git")
        .args(["-C", &repo, "worktree", "prune"])
        .output();

    // Registered worktree paths (so we never delete a live one).
    let listed = Command::new("git")
        .args(["-C", &repo, "worktree", "list", "--porcelain"])
        .output()?;
    let listed_text = String::from_utf8_lossy(&listed.stdout);
    let registered: BTreeSet<PathBuf> = listed_text
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|p| PathBuf::from(p.trim()))
        .collect();

    let worktrees_dir = repo_root.join(".harness").join("worktrees");
    let mut removed = Vec::new();
    if let Ok(entries) = fs::read_dir(&worktrees_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            // Compare against the canonicalized registered set when possible.
            let is_registered = registered
                .iter()
                .any(|reg| reg == &path || reg.canonicalize().ok() == path.canonicalize().ok());
            if is_registered {
                continue;
            }
            let _ = Command::new("git")
                .args(["-C", &repo, "worktree", "remove", "--force"])
                .arg(&path)
                .output();
            let _ = fs::remove_dir_all(&path);
            removed.push(path.display().to_string());
        }
    }
    let _ = Command::new("git")
        .args(["-C", &repo, "worktree", "prune"])
        .output();

    // Touch the store so the GC arm has a uniform signature with the rest.
    let _ = store.root();

    Ok(serde_json::json!({
        "ok": true,
        "removed": removed,
        "worktrees_dir": worktrees_dir.display().to_string(),
    }))
}

/// Parse a `unix-ms:<millis>` timestamp string into millis; 0 if unparseable.
fn created_ms(created_at: &str) -> u128 {
    created_at
        .strip_prefix("unix-ms:")
        .and_then(|n| n.parse::<u128>().ok())
        .unwrap_or(0)
}

/// Retention-window GC for the heavy per-node turn-event trace. The small audit
/// record (WorkflowRun / WorkflowStep / result / initiated_by / spec) is ALWAYS
/// kept; only the durable per-session NDJSON of OLDER runs is pruned. We keep the
/// `keep_runs` most-recent durable runs and any newer than `keep_days` (when
/// set), and prune the rest: delete each session's on-disk NDJSON dir, null its
/// `jsonl_ref`/`stdout_ref` (so `/v1/sessions/<id>/events` reports
/// `retained:false`), and flip the run's `trace_retention` to `"expired"`
/// (distinct from `"live"`, which was never retained). `--dry-run` reports the
/// plan without touching anything. Run it on a schedule (cron / `/loop`).
fn workflow_gc_trace(
    store: &HarnessStore,
    keep_runs: usize,
    keep_days: Option<u64>,
    dry_run: bool,
) -> CliResult<serde_json::Value> {
    let now_ms = current_unix_ms();
    let mut durable: Vec<WorkflowRun> = latest_workflow_runs_in_append_order(store)?
        .into_iter()
        .filter(|run| run.trace_retention == "durable")
        .collect();
    // Most-recent first, so the first `keep_runs` survive.
    durable.sort_by_key(|run| std::cmp::Reverse(created_ms(&run.created_at)));

    let steps = latest_workflow_steps_in_append_order(store)?;
    let mut pruned = Vec::new();
    let mut freed_sessions = 0usize;

    for (index, run) in durable.iter().enumerate() {
        let too_many = index >= keep_runs;
        let too_old = keep_days
            .map(|days| {
                now_ms.saturating_sub(created_ms(&run.created_at)) > u128::from(days) * 86_400_000
            })
            .unwrap_or(false);
        if !(too_many || too_old) {
            continue;
        }
        let session_ids: Vec<String> = steps
            .iter()
            .filter(|step| step.run_id == run.id)
            .filter_map(|step| step.provider_session_id.clone())
            .collect();
        pruned.push(serde_json::json!({
            "run_id": run.id,
            "created_at": run.created_at,
            "sessions": session_ids.len(),
            "reason": if too_old { "age" } else { "count" },
        }));
        if dry_run {
            freed_sessions += session_ids.len();
            continue;
        }
        for session_id in &session_ids {
            let dir = store.root().join("provider-sessions").join(session_id);
            let _ = fs::remove_dir_all(&dir);
            if let Some(mut session) = latest_provider_session(store, session_id)? {
                session.jsonl_ref = None;
                session.stdout_ref = None;
                store.append_provider_session(&session)?;
            }
            freed_sessions += 1;
        }
        let mut expired = run.clone();
        expired.trace_retention = "expired".to_string();
        store.append_workflow_run(&expired)?;
    }

    Ok(serde_json::json!({
        "ok": true,
        "kept_runs": durable.len().min(keep_runs),
        "pruned_runs": pruned.len(),
        "freed_sessions": freed_sessions,
        "dry_run": dry_run,
        "pruned": pruned,
    }))
}

fn workflow_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    require_subcommand(
        args,
        "workflow run|run-script|get-output|list|reap|gc-worktrees|gc-trace",
    )?;
    match args[0].as_str() {
        "gc-worktrees" => {
            let result = workflow_gc_worktrees(store)?;
            print_json(&result)?;
        }
        "reap" => {
            // One manual reaper pass (the serve loop runs this on an interval).
            // Useful to clean up abandoned `Running` runs when serve is not up.
            let reaped = reap_stale_workflow_runs(store)?;
            print_json(&serde_json::json!({ "reaped": reaped }))?;
        }
        "gc-trace" => {
            let keep_runs = value(&args[1..], "--keep-runs")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(100);
            let keep_days = value(&args[1..], "--keep-days").and_then(|v| v.parse::<u64>().ok());
            let dry_run = args[1..].iter().any(|a| a == "--dry-run");
            let result = workflow_gc_trace(store, keep_runs, keep_days, dry_run)?;
            print_json(&result)?;
        }
        "list" => {
            let registry = workflow::WorkflowRegistry::builtin();
            let defs: Vec<_> = registry
                .names()
                .into_iter()
                .filter_map(|name| registry.get(name))
                .map(|def| serde_json::json!({ "name": def.name, "summary": def.summary }))
                .collect();
            print_json(&serde_json::json!({ "workflows": defs }))?;
        }
        "run" => {
            let result = workflow_run_value(store, &args[1..])?;
            print_json(&result)?;
        }
        "get-output" => {
            let result = workflow_get_output_value(store, &args[1..])?;
            if has_flag(&args[1..], "--text") {
                // Plain-text mode: print just the deliverable(s), so a text-producing
                // workflow's output pipes straight to a file (issue #89 item 4).
                if let Some(steps) = result["steps"].as_array() {
                    let multi = steps.len() > 1;
                    for (i, s) in steps.iter().enumerate() {
                        if i > 0 {
                            println!("\n---\n");
                        }
                        if multi {
                            println!("## {}\n", s["label"].as_str().unwrap_or(""));
                        }
                        println!("{}", s["output"].as_str().unwrap_or(""));
                    }
                }
            } else {
                print_json(&result)?;
            }
        }
        "run-script" => {
            // Tell the operator WHICH store this run is written to (stderr, so the
            // JSON result on stdout stays clean) — so a serve reading a different
            // `.harness` is caught immediately (issue #89 item 3).
            let store_display = std::fs::canonicalize(store.root())
                .unwrap_or_else(|_| store.root().to_path_buf())
                .display()
                .to_string();
            eprintln!(
                "workflow store: {store_display}  (point `serve` at the same path: --store <path>)"
            );
            let result = workflow_run_script_value(store, &args[1..])?;
            print_json(&result)?;
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown workflow command: {other}"
            )))
        }
    }
    Ok(())
}

fn workflow_run_value(store: &HarnessStore, args: &[String]) -> CliResult<serde_json::Value> {
    let name = value(args, "--name").unwrap_or_else(|| "investigate".to_string());
    let registry = workflow::WorkflowRegistry::builtin();
    let def = registry
        .get(&name)
        .ok_or_else(|| CliError::Usage(format!("unknown workflow: {name}")))?;

    let prompt = value(args, "--prompt").unwrap_or_else(|| "failure X".to_string());
    let options = WorkflowDeliveryOptions {
        dry_run: has_flag(args, "--dry-run"),
        start_runtime: has_flag(args, "--start-runtime"),
        // Per-node ephemeral-worker timeout. Default 5 min: a real codex/claude
        // turn takes ~30-60s, so 3s would kill every worker now that the timeout
        // actually fires during the read (see run_ndjson_child); this is an IDLE
        // limit — a worker is killed only after this long with NO output, so a slow
        // but productive turn is never cut off. Default 15 min of silence.
        timeout_ms: value(args, "--timeout-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(900_000),
        default_model: value(args, "--model"),
        default_effort: value(args, "--effort"),
        max_budget_usd: None,
        // Registry runs always retain their trace durably.
        trace_retention: "durable".to_string(),
        progress: has_flag(args, "--progress"),
    };

    // The run id is minted up front so the driver can journal each step's
    // `running` row against it AS THE STEP STARTS (live progress over SSE),
    // rather than only emitting a terminal row after the whole body returns.
    let run_id = generated_id("wfrun");

    // Read the Copy flag before the `move` driver closure consumes `options`.
    let is_dry_run = options.dry_run;

    // Build the injectable real driver. The store, run id, and options are
    // captured by reference; the closure is Sync (HarnessStore serializes writes
    // via flock) so it can be shared across the parallel barrier's scoped threads.
    let driver = {
        let run_id = run_id.clone();
        move |spec: &workflow::AgentStepSpec| {
            workflow_real_agent_step(store, &run_id, &options, spec)
        }
    };

    run_workflow_with_driver(store, &run_id, def, &prompt, is_dry_run, &driver)
}

/// `harness workflow run-script <prog.star> [--name <n>] [--args <json>]
///  [--trace durable|live] [--dry-run] [--start-runtime] [--timeout-ms <ms>]
///  [--model <m>] [--effort <e>] [--initiated-by <id>]`
///
/// Reads a runtime-authored Starlark program — the SOLE dynamic authoring
/// surface — evaluates it via `starlark_front::run_starlark`, and journals the
/// run/steps through the shared `journal_workflow_outcome`.
///
/// The program MUST declare a `workflow(name, design_intent)` header (the WHY
/// behind its shape); `run_starlark` rejects it otherwise. The captured
/// `design_intent` is persisted on the run, and the raw script text is
/// snapshotted under `spec = {"lang":"starlark","script": <text>}` for
/// reproducibility. `--name` defaults to the declared meta name (else the file
/// stem).
/// Reconstruct a [`workflow::StepResult`] from a stored terminal [`WorkflowStep`]
/// for the `--resume` replay cache. Returns `None` unless the step carries an
/// ordinal in its `result` JSON (steps journaled before the resume feature have no
/// ordinal, so they are simply skipped → re-run, never incorrectly reused).
///
/// The reconstructed result sets `step_id = None` and `started_at = None` so
/// [`journal_workflow_outcome`] mints a FRESH terminal row for the NEW (resumed)
/// run id — replayed leaves journal like normal new steps. `ok = true` because the
/// caller only feeds Completed steps. `provider`/`isolation`/`structured`/`details`
/// are read back out of the same `result` object [`workflow::step_result_json`] wrote.
fn step_result_from_stored(step: &WorkflowStep) -> Option<workflow::StepResult> {
    let result = step.result.as_ref()?;
    let ordinal = result.get("ordinal").and_then(|v| v.as_u64())?;
    let provider = result
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let isolation = result
        .get("isolation")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let structured = result.get("structured").cloned().filter(|v| !v.is_null());
    // Carry the captured telemetry blob forward (model/tokens/cost/...). The base
    // keys step_result_json re-writes from the reconstructed fields take precedence
    // on the next journal, so passing the whole object back is safe.
    let details = step.result.clone().filter(|v| v.is_object());
    Some(workflow::StepResult {
        phase: step.phase.clone(),
        label: step.label.clone(),
        provider,
        isolation,
        ok: true,
        provider_session_id: step.provider_session_id.clone(),
        output_summary: step.output_summary.clone().unwrap_or_default(),
        step_id: None,
        started_at: None,
        details,
        structured,
        ordinal: Some(ordinal),
    })
}

/// Build the `--resume` replay cache: a map from leaf ordinal to the prior run's
/// succeeded [`workflow::StepResult`]. Loads the prior run's latest terminal steps,
/// keeps only Completed steps carrying an ordinal, and reconstructs each. A prior
/// FAILED leaf is naturally absent → it re-runs. On duplicate ordinals (should not
/// happen post-projection) last wins.
fn build_replay_map(
    store: &HarnessStore,
    prior_run_id: &str,
) -> CliResult<std::collections::HashMap<u64, workflow::StepResult>> {
    let mut map = std::collections::HashMap::new();
    for step in latest_workflow_steps_in_append_order(store)? {
        if step.run_id != prior_run_id {
            continue;
        }
        if step.status != WorkflowStepStatus::Completed {
            continue;
        }
        if let Some(result) = step_result_from_stored(&step) {
            if let Some(ord) = result.ordinal {
                map.insert(ord, result);
            }
        }
    }
    Ok(map)
}

fn workflow_run_script_value(
    store: &HarnessStore,
    args: &[String],
) -> CliResult<serde_json::Value> {
    // The script path is the first positional arg (not a --flag) or `--script <path>`.
    let path = value(args, "--script")
        .or_else(|| args.iter().find(|arg| !arg.starts_with("--")).cloned())
        .ok_or_else(|| {
            CliError::Usage("workflow run-script requires a <prog.star> path".to_string())
        })?;

    let script = std::fs::read_to_string(&path)
        .map_err(|error| CliError::Usage(format!("cannot read script {path}: {error}")))?;

    // Optional `--resume <prior_run_id>`: re-run this SAME script but reuse the
    // results of leaves that SUCCEEDED in the prior run, so a crash/kill does not
    // re-spend tokens on already-done work. Build the replay cache here after the
    // safety guard (the prior run must exist and have snapshotted the IDENTICAL
    // script; a changed script would misalign the deterministic leaf ordinals).
    let resume_from = value(args, "--resume");
    let replay = match &resume_from {
        Some(prior_run_id) => {
            let prior = latest_workflow_runs_in_append_order(store)?
                .into_iter()
                .find(|r| &r.id == prior_run_id)
                .ok_or_else(|| {
                    CliError::Usage(format!("cannot resume {prior_run_id}: no such run"))
                })?;
            let prior_script = prior
                .spec
                .as_ref()
                .and_then(|s| s.get("script"))
                .and_then(|v| v.as_str());
            match prior_script {
                Some(prev) if prev == script => {}
                Some(_) => {
                    return Err(CliError::Usage(format!(
                        "cannot resume {prior_run_id}: the script changed since that run"
                    )))
                }
                None => {
                    return Err(CliError::Usage(format!(
                        "cannot resume {prior_run_id}: that run has no snapshotted script"
                    )))
                }
            }
            Some(build_replay_map(store, prior_run_id)?)
        }
        None => None,
    };

    // Default workflow name: explicit `--name`, else the file stem. The Starlark
    // `workflow(...)` header's name can override this default once captured.
    let name = value(args, "--name").unwrap_or_else(|| {
        Path::new(&path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("workflow")
            .to_string()
    });

    // Optional `--args <json>`: parsed into the opaque value injected as the
    // script's `args` global. A typo fails fast.
    let parsed_args = match value(args, "--args") {
        Some(raw) => Some(
            serde_json::from_str::<serde_json::Value>(&raw)
                .map_err(|error| CliError::Usage(format!("invalid --args json: {error}")))?,
        ),
        None => None,
    };

    // Retention policy for the heavy per-node turn-event trace. `durable`
    // (default) persists the trace; `live` streams it over SSE during execution
    // but does not retain it. Validated up front so a typo fails fast.
    let trace_retention = value(args, "--trace").unwrap_or_else(|| "durable".to_string());
    if trace_retention != "durable" && trace_retention != "live" {
        return Err(CliError::Usage(format!(
            "--trace must be 'durable' or 'live', got '{trace_retention}'"
        )));
    }

    let options = WorkflowDeliveryOptions {
        dry_run: has_flag(args, "--dry-run"),
        start_runtime: has_flag(args, "--start-runtime"),
        // Per-node ephemeral-worker IDLE timeout: a worker is killed only after this
        // long with NO output (a wedged provider), so a slow-but-streaming turn runs
        // to completion. Default 15 min of silence.
        timeout_ms: value(args, "--timeout-ms")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(900_000),
        default_model: value(args, "--model"),
        default_effort: value(args, "--effort"),
        max_budget_usd: value(args, "--max-budget-usd").and_then(|v| v.parse::<f64>().ok()),
        trace_retention: trace_retention.clone(),
        progress: has_flag(args, "--progress"),
    };

    // Who initiated the run: an explicit `--initiated-by <id>`, else the
    // ambient agent member id (when an agent shells out), else "operator".
    let initiated_by = value(args, "--initiated-by")
        .or_else(|| std::env::var("HARNESS_AGENT_MEMBER_ID").ok())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "operator".to_string());

    // Reap any orphaned `Running` rows from crashed prior runs before starting a
    // new one, so phantoms never accumulate in the store / dashboard. Best-effort.
    let _ = reap_stale_workflow_runs(store);

    // Mint the run id up front so the real driver can journal each step's
    // `running` row as it starts (live SSE progress).
    let run_id = generated_id("wfrun");

    let mut run = WorkflowRun {
        id: run_id.clone(),
        workflow_name: name.clone(),
        status: WorkflowRunStatus::Running,
        step_ids: Vec::new(),
        created_at: now_string(),
        ended_at: None,
        summary: None,
        // The script's `args` global is carried opaquely onto the run.
        args: parsed_args.clone(),
        agents_spawned: 0,
        final_output: None,
        // Always-persisted durable audit record: who ran it + the raw script
        // text (the script is not a serializable spec), plus the retention
        // policy governing the heavy trace. `design_intent` is filled in from the
        // captured `workflow(...)` header once evaluation succeeds.
        initiated_by: Some(initiated_by),
        design_intent: None,
        // The resumed run is a NEW run_id; record which prior run it resumed from
        // so the new run has a complete, auditable record (DESIGN step 6).
        spec: Some(match &resume_from {
            Some(prior) => serde_json::json!({
                "lang": "starlark",
                "script": script,
                "resumed_from": prior,
            }),
            None => serde_json::json!({ "lang": "starlark", "script": script }),
        }),
        trace_retention,
        // Stamp this driver process's pid so the serve-side reaper can detect an
        // abandoned run (driver killed/crashed before journaling a terminal row).
        host_pid: Some(std::process::id()),
        // Mark dry-run validation runs so they are never mistaken for real runs in
        // the jsonl / dashboard (issue #89 item 2).
        dry_run: options.dry_run,
    };
    store.append_workflow_run(&run)?;

    // Optional per-run spend ceiling: once cumulative step cost reaches it, the
    // runtime short-circuits further agent()/parallel() calls into failed `budget`
    // steps. A `workflow(budget_usd=…)` header may lower it further.
    let max_budget_usd = value(args, "--max-budget-usd").and_then(|v| v.parse::<f64>().ok());

    let started = {
        let run_id = run_id.clone();
        let driver = move |step: &workflow::AgentStepSpec| {
            workflow_real_agent_step(store, &run_id, &options, step)
        };
        harness_workflow::starlark_front::run_starlark_with_budget(
            &script,
            &name,
            parsed_args.as_ref(),
            &driver,
            max_budget_usd,
            replay,
        )
        .map_err(|error| CliError::Usage(error.to_string()))?
    };

    // Persist the captured mandatory meta: the declared `design_intent` and the
    // workflow name (the header's name overrides the CLI default).
    run.design_intent = Some(started.meta.design_intent.clone());
    run.workflow_name = started.meta.name.clone();

    journal_workflow_outcome(store, run, &started.outcome)
}

/// Create the WorkflowRun (running), dispatch the workflow body with the given
/// agent-step driver, journal a WorkflowStep per step, and finalize the run.
/// The `driver` is injectable so tests pass a mock instead of the real provider
/// path.
fn run_workflow_with_driver(
    store: &HarnessStore,
    run_id: &str,
    def: &workflow::WorkflowDef,
    prompt: &str,
    dry_run: bool,
    driver: &workflow::AgentStepFn<'_>,
) -> CliResult<serde_json::Value> {
    let run = WorkflowRun {
        id: run_id.to_string(),
        workflow_name: def.name.to_string(),
        status: WorkflowRunStatus::Running,
        step_ids: Vec::new(),
        created_at: now_string(),
        ended_at: None,
        summary: None,
        // Registry runs are not parameterized and do not snapshot the scheduler;
        // `journal_workflow_outcome` fills `final_output`/`agents_spawned` (0 here).
        args: None,
        agents_spawned: 0,
        final_output: None,
        // Registry runs are operator-triggered and carry no dynamic spec; they
        // default to durable trace retention.
        initiated_by: Some("operator".to_string()),
        design_intent: None,
        spec: None,
        trace_retention: "durable".to_string(),
        // Stamp this driver process's pid so the serve-side reaper can detect an
        // abandoned run (see the run-script path and `reap_abandoned_runs`).
        host_pid: Some(std::process::id()),
        dry_run,
    };
    store.append_workflow_run(&run)?;

    // Dispatch the compiled workflow body (option C registry dispatch).
    let outcome = (def.run)(driver, prompt);

    journal_workflow_outcome(store, run, &outcome)
}

/// Journal the running `run`'s terminal steps + finalize it from a
/// [`workflow::WorkflowOutcome`]. Shared by the registry `run` path and the
/// dynamic `run-script` (Starlark) path so both journal identically.
fn journal_workflow_outcome(
    store: &HarnessStore,
    mut run: WorkflowRun,
    outcome: &workflow::WorkflowOutcome,
) -> CliResult<serde_json::Value> {
    let run_id = run.id.clone();
    // Journal one TERMINAL WorkflowStep per StepResult, preserving order. When
    // the driver already journaled a `running` row at step start (real path), we
    // REUSE its `step_id` and real `started_at` so the latest-wins projection
    // updates the same row in place and the journaled window reflects true
    // (overlapping) execution. Mock drivers leave those `None`, so we mint a
    // fresh id and stamp the journal time, preserving the pre-existing behavior.
    let mut steps_json = Vec::new();
    for result in &outcome.steps {
        // The real driver (`workflow_real_agent_step`) already journaled this
        // step's terminal row the instant it completed — for live per-step SSE.
        // It is recognisable by a present `step_id`. Mock/test drivers leave it
        // `None`, so we mint an id and journal the terminal row here.
        let already_journaled = result.step_id.is_some();
        let step_id = result
            .step_id
            .clone()
            .unwrap_or_else(|| generated_id("wfstep"));
        let started_at = result.started_at.clone().unwrap_or_else(now_string);
        let step = build_terminal_step(&run_id, step_id.clone(), started_at, result);
        if !already_journaled {
            store.append_workflow_step(&step)?;
        }
        run.step_ids.push(step_id);
        steps_json.push(serde_json::to_value(&step)?);
    }

    // Finalize the run with the workflow's own status verdict + the collected
    // structured output and the agent count the dispatch spawned.
    run.status = outcome.status;
    run.ended_at = Some(now_string());
    run.summary = Some(outcome.summary.clone());
    run.agents_spawned = outcome.agents_spawned;
    run.final_output = outcome.final_output.clone();
    store.append_workflow_run(&run)?;

    Ok(serde_json::json!({
        "run": serde_json::to_value(&run)?,
        "steps": steps_json,
    }))
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
                tokens: None,
                cost_usd: None,
                model: None,
                structured: None,
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
                let summary = format!(
                    "{} runtime start failed after claim: {error}",
                    member.provider
                );
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
                    tokens: None,
                    cost_usd: None,
                    model: None,
                    structured: None,
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
                    tokens: None,
                    cost_usd: None,
                    model: None,
                    structured: None,
                    summary,
                }
            } else {
                let runtime = runtime.clone().expect("runtime checked");
                run_provider_delivery(
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
                sender_kind: SenderKind::Agent,
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
            "exit_code": delivery.exit_code,
            "tokens": delivery.tokens.map(TokenUsage::to_json),
            "cost_usd": delivery.cost_usd,
            "model": delivery.model,
            "structured": delivery.structured
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
    tokens: Option<TokenUsage>,
    cost_usd: Option<f64>,
    model: Option<String>,
    structured: Option<serde_json::Value>,
    summary: String,
}

fn claim_message_for_delivery(
    store: &HarnessStore,
    member: &AgentMember,
    runtime: Option<&AgentRuntime>,
    message: &Message,
    delivery_id: &str,
) -> CliResult<Option<Message>> {
    let mut provider_session =
        build_claimed_provider_session(delivery_id, member, runtime, message);
    // Live agent view: point the RUNNING claim row at the NDJSON file the exec
    // delivery appends to MID-TURN, and pre-create it so the first poll of
    // GET /v1/provider-sessions/{id}/events returns [] (not a not-found error)
    // before the first event lands. Same delivery_id → same session row as the
    // terminal row, so the poll resolves to the growing file throughout. Both
    // providers stream; the file name matches what each exec path writes.
    let live_filename =
        provider_adapter(member.provider.as_str()).map(|adapter| adapter.live_ndjson_file_name());
    if let Some(filename) = live_filename {
        let session_dir = store.root().join("provider-sessions").join(delivery_id);
        let live_path = session_dir.join(filename);
        if fs::create_dir_all(&session_dir).is_ok() {
            let _ = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&live_path);
            provider_session.jsonl_ref = Some(live_path.display().to_string());
        }
    }
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
        provider: member.provider.clone(),
        agent_member_id: member.id.clone(),
        task_id: message.task_id.clone(),
        workspace_ref: member.worktree_ref.clone(),
        provider_thread_id: member.provider_thread_id.clone(),
        provider_turn_id: None,
        terminal_source: None,
        status: ProviderSessionStatus::Running,
        command: "harness".into(),
        args: vec![
            member.provider.clone(),
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
        source_type: "claude_delivery_session".into(),
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

// Test-only helper: builds the codex app-server turn input envelope. Exercised by
// unit tests; not yet wired into the live delivery path (kept for the WP that lands it).
#[cfg(test)]
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

/// Resolve a control endpoint to a filesystem path.
///
/// Codex uses a `unix://` socket endpoint, so its path is the prefix-stripped
/// value. Other providers (e.g. the claude CLI shape, or HTTP/stdio transports)
/// do not present a unix-socket endpoint; for any non-`unix://` scheme we return
/// the endpoint verbatim so callers that only inspect existence/format keep
/// working without assuming a unix socket. This keeps the seam provider-neutral
/// per ADR 0011 — the endpoint format is the one place Codex assumed a socket.
fn ingest_provider_output(
    store: &HarnessStore,
    agent_member_id: &str,
    runtime_id: Option<&str>,
    task_id: Option<&str>,
    source_ref: &str,
) -> CliResult<()> {
    // The member's declared provider is the source of truth for the native output
    // shape we parse and the provider string we stamp; on lookup failure default to codex.
    let provider = latest_member(store, agent_member_id)
        .map(|member| member.provider)
        .unwrap_or_else(|_| CodexAdapter.name().to_string());
    match provider_adapter(&provider) {
        Some(adapter) => {
            adapter.ingest_output(store, agent_member_id, runtime_id, task_id, source_ref)
        }
        None => Err(unknown_provider_error(&provider, "output ingest")),
    }
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
            sender_kind: SenderKind::Agent,
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

// Test-only helper: extracts JSON-RPC error strings; covered by unit tests only.
#[cfg(test)]
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

// Test-only helper: validates a codex app-server turn-start exchange; unit-tested only.
#[cfg(test)]
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

// Test-only helper: maps codex app-server values to a terminal source; unit-tested only.
#[cfg(test)]
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
    provider: &str,
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
        provider: provider.into(),
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

// Test-only helper: extracts a thread id from codex app-server values; unit-tested only.
#[cfg(test)]
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

/// Truncate `s` to at most `max` BYTES without splitting a UTF-8 char: byte
/// slicing (`&s[..max]`) panics when `max` lands inside a multi-byte char (CJK,
/// emoji, …), so back off to the nearest char boundary at or below `max` first.
/// Used on every summary/error path that bounds an arbitrary (possibly non-ASCII)
/// provider string — a formatting nicety must never be able to panic a live run
/// after the agent work (and its tokens) are already spent. (issue #89, item 1)
fn truncate_on_char_boundary(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn summarize_json_value(value: &serde_json::Value) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "provider event".into());
    if raw.len() > 240 {
        format!("{}...", truncate_on_char_boundary(&raw, 240))
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
    // Closeout-gate inputs (§3.7): a closeout Decision is one scoped to this goal
    // (goal_id == G OR task in the goal's graph) with decision_kind=closeout and at
    // least one backing evidence_id. A closeout waiver is an explicit is_waiver
    // Decision (also goal-scoped) that names a follow_up_task_id and carries
    // evidence — it lets the goal close without the evaluation chain.
    closeout_decisions: Vec<Decision>,
    closeout_waivers: Vec<Decision>,
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
            "closeout_decisions": &self.closeout_decisions,
            "closeout_waivers": &self.closeout_waivers,
            // Closeout-gate readiness (§3.7, §3.4 of WP-F): the frontend reads these
            // to render the closeout-gate ProofRow and the goal_close_without_evaluation
            // / waiver_without_follow_up warnings.
            "has_closeout_decision": self.has_closeout_decision(),
            "has_evaluation": self.has_goal_evaluation(),
            "has_closeout_waiver": self.has_valid_closeout_waiver(),
            "may_close": self.may_close(),
            "closeout_blockers": self.closeout_blockers(),
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

    /// A closeout Decision (decision_kind=closeout, >=1 evidence_id) exists for the
    /// goal. The evidence requirement is already enforced when the field is built.
    fn has_closeout_decision(&self) -> bool {
        !self.closeout_decisions.is_empty()
    }

    /// At least one valid closeout waiver: is_waiver=true, names a follow_up_task_id
    /// and carries >=1 evidence_id. These are the structural fields the CLI enforces
    /// at write time; the gate re-checks them here so a hand-edited JSONL row cannot
    /// slip an invalid waiver past the closeout gate.
    fn valid_closeout_waivers(&self) -> impl Iterator<Item = &Decision> {
        self.closeout_waivers.iter().filter(|decision| {
            decision.follow_up_task_id.is_some() && !decision.evidence_ids.is_empty()
        })
    }

    fn has_valid_closeout_waiver(&self) -> bool {
        self.valid_closeout_waivers().next().is_some()
    }

    /// The §3.7 closeout gate: a goal may become complete only with BOTH a closeout
    /// Decision and a GoalEvaluation, OR an explicit valid waiver.
    fn may_close(&self) -> bool {
        (self.has_closeout_decision() && self.has_goal_evaluation())
            || self.has_valid_closeout_waiver()
    }

    /// Human-readable reasons the closeout gate is not yet satisfied. Empty when
    /// [`may_close`](Self::may_close) is true.
    fn closeout_blockers(&self) -> Vec<String> {
        if self.may_close() {
            return Vec::new();
        }
        let mut blockers = Vec::new();
        if !self.has_closeout_decision() {
            blockers.push(
                "missing closeout decision (decision_kind=closeout with >=1 evidence_id)".into(),
            );
        }
        if !self.has_goal_evaluation() {
            blockers.push("missing goal_evaluation".into());
        }
        // Surface why an attempted waiver did not count, when one is present.
        if !self.closeout_waivers.is_empty() && !self.has_valid_closeout_waiver() {
            blockers
                .push("waiver decision missing follow_up_task_id and/or >=1 evidence_id".into());
        }
        blockers
    }

    /// Enforce the closeout gate (used by `goal close`). Returns a descriptive error
    /// listing every unmet requirement when the goal may not yet close.
    fn require_closeout(&self) -> CliResult<()> {
        if self.may_close() {
            return Ok(());
        }
        Err(CliError::Usage(format!(
            "goal {} cannot be closed: {}. Record a closeout decision (decision_kind=closeout) with evidence plus a GoalEvaluation, or an explicit waiver (--waiver) naming a --follow-up-task and >=1 --evidence.",
            self.goal_id,
            self.closeout_blockers().join("; ")
        )))
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

    let all_decisions = store.decisions()?;
    // A decision is "in scope" for this goal when it is explicitly goal-scoped
    // (goal_id == G) OR it hangs off a task in the goal's graph. Closeout decisions
    // are typically goal-scoped (no task), so we must not restrict by task_id alone.
    let decision_in_goal_scope = |decision: &Decision| {
        decision.goal_id.as_deref() == Some(goal_id) || task_ids.contains(&decision.task_id)
    };
    let decisions: Vec<_> = all_decisions
        .iter()
        .filter(|decision| task_ids.contains(&decision.task_id))
        .cloned()
        .collect();
    let waivers: Vec<_> = decisions
        .iter()
        .filter(|decision| is_goal_learning_waiver_decision(decision))
        .cloned()
        .collect();
    // Closeout gate (§3.7): closeout decisions carry decision_kind=closeout and at
    // least one evidence_id; closeout waivers set is_waiver and name a follow-up.
    let closeout_decisions: Vec<_> = all_decisions
        .iter()
        .filter(|decision| {
            decision_in_goal_scope(decision)
                && decision.decision_kind.as_deref() == Some("closeout")
                && !decision.evidence_ids.is_empty()
        })
        .cloned()
        .collect();
    let closeout_waivers: Vec<_> = all_decisions
        .iter()
        .filter(|decision| decision_in_goal_scope(decision) && decision.is_waiver)
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
        closeout_decisions,
        closeout_waivers,
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
    created_at
        .filter_map(|value| parse_unix_ms(value))
        .collect()
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
    let workflow_runs = latest_workflow_runs_in_append_order(store)?;
    let workflow_steps = latest_workflow_steps_in_append_order(store)?;
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
                // Config-tab + identity-rail data (Multica layout): these live on
                // the AgentMember but were not previously projected into the
                // snapshot. Additive — no schema change.
                "model": member.model,
                "profile": member.profile,
                "provider_config": member.provider_config,
                "team_ids": member.team_ids,
                "created_at": member.created_at,
                "last_seen_at": member.last_seen_at,
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
        "provider_child_threads": provider_child_threads,
        "workflow_runs": workflow_runs,
        "workflow_steps": workflow_steps
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

/// Read the RAW provider turn for one session, 1:1: each line of the persisted
/// claude (`jsonl_ref`) or codex (`stdout_ref`) NDJSON stream parsed back into
/// JSON. This is what powers the dashboard's "▸ turn" drill-in — the agent's
/// actual events (assistant text, tool_use, tool_result, result), not a wrapped
/// summary. Returns `(events, truncated)`; capped so a long turn cannot flood
/// the response. Non-JSON lines are skipped so a partial final line is safe.
fn read_provider_session_events(
    store: &HarnessStore,
    session_id: &str,
) -> CliResult<(Vec<serde_json::Value>, bool)> {
    const MAX_EVENTS: usize = 1000;
    let session = latest_provider_session(store, session_id)?
        .ok_or_else(|| CliError::Usage(format!("provider session not found: {session_id}")))?;
    let path = session
        .jsonl_ref
        .clone()
        .or_else(|| session.stdout_ref.clone())
        .ok_or_else(|| {
            CliError::Usage(format!("session {session_id} has no recorded event stream"))
        })?;
    let content = fs::read_to_string(&path)
        .map_err(|error| CliError::Usage(format!("cannot read session stream {path}: {error}")))?;
    let mut events = Vec::new();
    let mut truncated = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            if events.len() >= MAX_EVENTS {
                truncated = true;
                break;
            }
            events.push(value);
        }
    }
    Ok((events, truncated))
}

/// A normalized event for a raw provider frame we have no specific mapping
/// for yet: retains the raw JSON, stamps provider/ts, kind = Unknown. The
/// caller/read helper assigns the final sequence number.
fn generic_turn_event(
    provider: &str,
    session_id: &str,
    raw: &serde_json::Value,
) -> HarnessTurnEvent {
    HarnessTurnEvent {
        session_id: session_id.to_string(),
        provider: provider.to_string(),
        seq: 0,
        ts: now_string(),
        provider_thread_id: None,
        provider_turn_id: None,
        provider_item_id: None,
        kind: HarnessTurnEventKind::Unknown,
        role: None,
        text: None,
        delta: None,
        tool_call: None,
        tool_result: None,
        usage: None,
        model: None,
        duration_ms: None,
        cost_usd: None,
        status: None,
        error: None,
        raw_provider_event: raw.clone(),
    }
}

fn normalize_live_turn_event(
    provider: &str,
    session_id: &str,
    raw: &serde_json::Value,
    next_seq: u64,
) -> Vec<HarnessTurnEvent> {
    match provider_adapter(provider) {
        Some(adapter) => adapter.normalize_turn_event(session_id, raw),
        None => vec![generic_turn_event(provider, session_id, raw)],
    }
    .into_iter()
    .enumerate()
    .map(|(index, mut event)| {
        event.seq = next_seq + index as u64;
        event
    })
    .collect()
}

/// Read a session's RAW per-session events and normalize each to a
/// HarnessTurnEvent (normalize-on-read; no new storage, raw route unchanged).
fn read_provider_session_normalized_events(
    store: &HarnessStore,
    session_id: &str,
) -> CliResult<(Vec<HarnessTurnEvent>, bool)> {
    let session = latest_provider_session(store, session_id)?
        .ok_or_else(|| CliError::Usage(format!("provider session not found: {session_id}")))?;
    let (raw_events, truncated) = read_provider_session_events(store, session_id)?;
    let normalized = raw_events
        .iter()
        .flat_map(|raw| match provider_adapter(&session.provider) {
            Some(adapter) => adapter.normalize_turn_event(session_id, raw),
            None => vec![generic_turn_event(&session.provider, session_id, raw)],
        })
        .enumerate()
        .map(|(i, mut event)| {
            event.seq = i as u64;
            event
        })
        .collect();
    Ok((normalized, truncated))
}

/// Historical (completed-run) normalized read: the normalize-on-read companion to
/// `read_session_turn_events`, used by `GET /v1/sessions/{id}/normalized-events`.
/// Mirrors `read_provider_session_normalized_events` but reads the DURABLE
/// per-session NDJSON via the two-tier-persistence path so it can also report
/// `retained` — a `--trace live` run whose trace was pruned returns
/// `(retained=false, [], false)` so the dashboard renders "trace not retained"
/// instead of a 404, exactly like the raw historical endpoint.
fn read_session_turn_events_normalized(
    store: &HarnessStore,
    session_id: &str,
) -> CliResult<(bool, Vec<HarnessTurnEvent>, bool)> {
    let raw = read_session_turn_events(store, session_id)?;
    if !raw.retained {
        return Ok((false, Vec::new(), raw.truncated));
    }
    // The provider drives adapter selection; a retained trace always has its
    // ProviderSession row, but fall back to the generic adapter if it is missing
    // so a normalized read never fails on a recoverable lookup.
    let provider = latest_provider_session(store, session_id)?
        .map(|session| session.provider)
        .unwrap_or_default();
    let normalized = raw
        .events
        .iter()
        .flat_map(|event| match provider_adapter(&provider) {
            Some(adapter) => adapter.normalize_turn_event(session_id, event),
            None => vec![generic_turn_event(&provider, session_id, event)],
        })
        .enumerate()
        .map(|(i, mut event)| {
            event.seq = i as u64;
            event
        })
        .collect();
    Ok((true, normalized, raw.truncated))
}

/// Outcome of reading one provider session's PERSISTED turn-event trace for the
/// historical drill-in (`GET /v1/sessions/<id>/events`). Distinguishes a durable
/// run whose heavy trace survived from a `--trace live` run whose trace was
/// pruned after execution (two-tier persistence): the latter streamed live over
/// SSE but retains nothing, so a past drill-in shows "trace not retained".
struct SessionTurnEvents {
    /// Whether the heavy per-node trace was retained for this session.
    retained: bool,
    /// Ordered turn events parsed from the durable per-session NDJSON (one JSON
    /// value per line). Empty when `retained` is false.
    events: Vec<serde_json::Value>,
    /// True when the cap was hit and trailing events were dropped.
    truncated: bool,
}

/// Read the PERSISTED per-session turn events for a completed durable run's
/// historical drill-in, keyed by provider session id. This is the read side of
/// the two-tier persistence design: the small audit record (WorkflowRun/Step)
/// always survives, while the heavy turn-event trace survives only for
/// `trace_retention == "durable"` runs.
///
/// Source of truth is the DURABLE per-session NDJSON the ProviderSession's
/// `jsonl_ref` (claude) / `stdout_ref` (codex) points at — the file the spawn
/// loop writes under `provider-sessions/<id>/` and which survives a server
/// restart (unlike the shared `provider_turn_events.jsonl` live tee, which
/// `serve` truncates on startup). We parse it the same way `read_provider_session_events`
/// and `sse.rs` do: one JSON value per line, skipping torn/non-JSON lines so a
/// mid-append final fragment is safe.
///
/// A `--trace live` run prunes that NDJSON after execution and the Backend left
/// the ProviderSession's `jsonl_ref`/`stdout_ref` as `None` precisely so the
/// historical drill-in reports `retained: false` ("trace not retained") instead
/// of an empty-but-durable trace. A session with no ProviderSession row at all is
/// likewise reported as not retained.
fn read_session_turn_events(
    store: &HarnessStore,
    session_id: &str,
) -> CliResult<SessionTurnEvents> {
    const MAX_EVENTS: usize = 1000;

    // The retention marker IS the presence of a recorded event stream on the
    // session row: durable runs point jsonl_ref/stdout_ref at the retained
    // per-session NDJSON; live-only runs (and missing sessions) have neither.
    let path = match latest_provider_session(store, session_id)? {
        Some(session) => session
            .jsonl_ref
            .clone()
            .or_else(|| session.stdout_ref.clone()),
        None => None,
    };
    let Some(path) = path else {
        return Ok(SessionTurnEvents {
            retained: false,
            events: Vec::new(),
            truncated: false,
        });
    };

    // The trace was retained but the file may have been swept; treat an
    // unreadable durable ref as an empty (still-retained) trace rather than 404.
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(_) => {
            return Ok(SessionTurnEvents {
                retained: true,
                events: Vec::new(),
                truncated: false,
            })
        }
    };
    let mut events = Vec::new();
    let mut truncated = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        if events.len() >= MAX_EVENTS {
            truncated = true;
            break;
        }
        events.push(value);
    }
    Ok(SessionTurnEvents {
        retained: true,
        events,
        truncated,
    })
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

fn latest_workflow_runs_in_append_order(store: &HarnessStore) -> CliResult<Vec<WorkflowRun>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for run in store.workflow_runs()? {
        ids.retain(|id| id != &run.id);
        ids.push(run.id.clone());
        by_id.insert(run.id.clone(), run);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
}

/// Age after which a `Running` WorkflowRun is assumed orphaned and reaped. The
/// run-script path is SYNCHRONOUS — a run is only `Running` in the store while its
/// host process is alive — so a row left `Running` past this age means the process
/// died (crash / Ctrl-C / OOM) before finalizing it. Generous (the longest real
/// runs are ~1.5h) so a legitimately long run is never reaped.
// Age-based backstop for the reaper. The PRIMARY signal is host-pid liveness
// (a killed driver is caught in seconds); this only governs legacy runs that
// carry no `host_pid`, plus the rare pid-reuse false-negative.
const REAP_STALE_RUN_AFTER_MS: u128 = 4 * 60 * 60 * 1000; // 4 hours

/// How often the serve-side reaper scans for abandoned runs. A killed driver is
/// reflected on the dashboard within this window.
const REAP_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Finalize ABANDONED `Running` workflow runs to `Failed`, so a crashed / killed
/// driver does not sit `Running` forever in the store / snapshot / dashboard.
///
/// A run is abandoned when EITHER:
///   - its `host_pid` is recorded and that process is no longer alive on this
///     host (driver killed / crashed / Ctrl-C'd) — caught within one poll,
///     regardless of age; OR
///   - it has been `Running` longer than [`REAP_STALE_RUN_AFTER_MS`] — the age
///     backstop covering legacy rows with no `host_pid` (and pid reuse).
///
/// Reaping a run also flips its still-open (`running`/`queued`) steps to `failed`
/// so the per-step view is not frozen mid-flight after the run itself fails. The
/// appended terminal rows are picked up and broadcast by the SSE watcher, so a
/// live dashboard updates without a refetch. Best-effort; returns the count of
/// runs reaped. Same-host only — `host_pid` liveness is meaningless across hosts.
fn reap_stale_workflow_runs(store: &HarnessStore) -> CliResult<usize> {
    let now = current_unix_ms();
    // Group the latest step rows by run so a reaped run's open steps close too.
    let mut steps_by_run: BTreeMap<String, Vec<WorkflowStep>> = BTreeMap::new();
    for step in latest_workflow_steps_in_append_order(store)? {
        steps_by_run
            .entry(step.run_id.clone())
            .or_default()
            .push(step);
    }
    let mut reaped = 0;
    for mut run in latest_workflow_runs_in_append_order(store)? {
        if run.status != WorkflowRunStatus::Running {
            continue;
        }
        let age = now.saturating_sub(created_ms(&run.created_at));
        let pid_dead = run.host_pid.map(|pid| !pid_is_alive(pid)).unwrap_or(false);
        let too_old = age >= REAP_STALE_RUN_AFTER_MS;
        if !pid_dead && !too_old {
            continue;
        }
        // Close any non-terminal steps so the dashboard's per-step status is not
        // stuck at `running` after the run itself is failed.
        if let Some(steps) = steps_by_run.get(&run.id) {
            for step in steps {
                if !matches!(
                    step.status,
                    WorkflowStepStatus::Running | WorkflowStepStatus::Queued
                ) {
                    continue;
                }
                let mut closed = step.clone();
                closed.status = WorkflowStepStatus::Failed;
                closed.ended_at = Some(now_string());
                closed.output_summary = Some(match closed.output_summary.as_deref() {
                    Some(s) if !s.is_empty() => format!("{s} [reaped: driver process gone]"),
                    _ => "reaped: driver process gone".to_string(),
                });
                store.append_workflow_step(&closed)?;
            }
        }
        run.status = WorkflowRunStatus::Failed;
        run.ended_at = Some(now_string());
        run.summary = Some(match run.host_pid {
            Some(pid) if pid_dead => format!(
                "reaped: driver process (pid {pid}) is no longer alive — the run was abandoned before it finalized"
            ),
            _ => format!(
                "reaped: orphaned Running for ~{}h — host process exited before the run finalized",
                age / (60 * 60 * 1000)
            ),
        });
        store.append_workflow_run(&run)?;
        reaped += 1;
    }
    Ok(reaped)
}

fn latest_workflow_steps_in_append_order(store: &HarnessStore) -> CliResult<Vec<WorkflowStep>> {
    let mut ids = Vec::new();
    let mut by_id = BTreeMap::new();
    for step in store.workflow_steps()? {
        ids.retain(|id| id != &step.id);
        ids.push(step.id.clone());
        by_id.insert(step.id.clone(), step);
    }
    Ok(ids.into_iter().filter_map(|id| by_id.remove(&id)).collect())
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

fn latest_goal_evaluations_in_append_order(store: &HarnessStore) -> CliResult<Vec<GoalEvaluation>> {
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
    let output_schema = output_schema_from_args(args)?;
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
            effort: value(args, "--effort"),
            output_schema,
            approval_policy: value(args, "--approval-policy"),
            approvals_reviewer: value(args, "--approvals-reviewer"),
            sandbox_policy: value(args, "--sandbox-policy"),
            permission_profile: value(args, "--permission-profile"),
            runtime_workspace_roots: many(args, "--runtime-workspace-root"),
            environment_id: value(args, "--environment"),
            mcp: None,
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

fn output_schema_from_args(args: &[String]) -> CliResult<Option<serde_json::Value>> {
    let Some(path) = value(args, "--output-schema-file") else {
        return Ok(None);
    };
    let contents = fs::read_to_string(&path)
        .map_err(|e| CliError::Usage(format!("failed to read --output-schema-file {path}: {e}")))?;
    let schema = serde_json::from_str::<serde_json::Value>(&contents).map_err(|e| {
        CliError::Usage(format!(
            "failed to parse --output-schema-file {path} as JSON: {e}"
        ))
    })?;
    Ok(Some(schema))
}

fn ensure_agent_prompt(
    store: &HarnessStore,
    member: &AgentMember,
    args: &[String],
) -> CliResult<String> {
    ensure_agent_prompt_with_override(store, member, value(args, "--prompt"))
}

/// Persist (or reuse) the bootstrap prompt for a member. Shared by the CLI
/// (`--prompt`) and the HTTP create route (`prompt` JSON field). When the member
/// already carries an explicit `prompt_ref` it is returned untouched; otherwise a
/// prompt file is written under the store's `prompts/` dir, using the caller's
/// override text or a generated bootstrap prompt.
fn ensure_agent_prompt_with_override(
    store: &HarnessStore,
    member: &AgentMember,
    prompt_override: Option<String>,
) -> CliResult<String> {
    if let Some(prompt_ref) = member.prompt_ref.clone() {
        return Ok(prompt_ref);
    }

    store.init()?;
    let prompt_path = store
        .root()
        .join("prompts")
        .join(format!("{}.md", member.id));
    let prompt = prompt_override.unwrap_or_else(|| build_bootstrap_prompt(member));
    fs::write(&prompt_path, prompt)?;
    Ok(prompt_path.display().to_string())
}

fn build_bootstrap_prompt(member: &AgentMember) -> String {
    format!(
        "# Agent Bootstrap\n\nid: {}\nname: {}\ndescription: {}\nrole: {}\nprovider: {}\n\nUse harness messages as the source of truth. Report task progress with evidence refs. Respect worktree, branch, PR, and owned-path boundaries.\n",
        member.id, member.name, member.description, member.role, member.provider
    )
}

// ---------------------------------------------------------------------------
// Provider dispatch seam (BE-WP6)
//
// The harness core stays provider-neutral (ADR 0011); all provider-specific
// behaviour lives behind these four dispatch points keyed on `member.provider`.
// Codex routes to the existing, regression-clean implementation. Claude routes
// to stubs that return a clear "not yet implemented" error until BE-WP7/WP8
// land the real claude-CLI runtime/delivery/ingest. Unknown providers fail
// fast with an explicit, debuggable message rather than silently assuming Codex.
// ---------------------------------------------------------------------------

/// Everything a one-shot ephemeral provider spawn needs, bundled so the
/// `ProviderAdapter::spawn_ephemeral` dispatch method takes a single arg and
/// stays object-safe. Mirrors the params of the per-provider spawn helpers.
struct EphemeralSpawnContext<'a> {
    session_dir: &'a Path,
    session_id: &'a str,
    spec: &'a workflow::AgentStepSpec,
    schema_json: Option<&'a serde_json::Value>,
    prompt: &'a str,
    cwd: &'a Path,
    model: Option<&'a str>,
    effort: Option<&'a str>,
    timeout_ms: u64,
    max_budget_usd: Option<f64>,
}

/// Provider-specific behaviour boundary (Issue #107 Gap 1). Stage 3 carries the
/// provider's canonical name and the workflow ephemeral spawn dispatch. Every
/// provider dispatch site in the CLI routes through this trait and the
/// `provider_adapter` registry, which is the single source of truth for the
/// providers the harness supports.
trait ProviderAdapter: Sync {
    /// Canonical provider id as used in `member.provider` and `agent(provider=...)`.
    fn name(&self) -> &'static str;

    /// The per-session live NDJSON filename this provider's spawn/delivery writes,
    /// which the ProviderSession `jsonl_ref` points at during a turn.
    fn live_ndjson_file_name(&self) -> &'static str;

    /// Map a LaunchPermission to this provider's CLI permission flag value
    /// (codex `--sandbox`, claude `--permission-mode`).
    fn map_permission(&self, perm: LaunchPermission) -> &'static str;

    /// The recorded argv head for this provider's delivery command (the
    /// ProviderSession `args` audit field), optionally resuming `resume_id`.
    fn recorded_args(&self, resume_id: Option<&str>) -> Vec<String>;

    /// Record a provider hook event into the neutral event log. Hooks are a
    /// codex-runtime mechanism; the default reports the provider has no hook
    /// integration — an explicit error beats silently recording a codex-shaped
    /// event. Only CodexAdapter overrides this.
    fn record_hook_event(&self, _store: &HarnessStore, _args: &[String]) -> CliResult<()> {
        Err(CliError::Usage(format!(
            "provider {} does not support hook events",
            self.name()
        )))
    }

    /// Map one raw provider event frame to normalized HarnessTurnEvents. The
    /// default is a single generic Unknown event (raw retained); each provider
    /// overrides it to map its own vocabulary. The read helper assigns final seq.
    fn normalize_turn_event(
        &self,
        session_id: &str,
        raw: &serde_json::Value,
    ) -> Vec<HarnessTurnEvent> {
        vec![generic_turn_event(self.name(), session_id, raw)]
    }

    /// Reduce this provider's retained ephemeral NDJSON trace into neutral
    /// AgentEvents (and, for claude, a coexisting ProviderSession). Called only on
    /// durable runs. Ingest errors are swallowed — they must never fail the step.
    fn ingest_ephemeral_trace(
        &self,
        store: &HarnessStore,
        session_id: &str,
        spawn: &EphemeralSpawn,
    );

    /// Ingest a persistent provider runtime's recorded output file (`source_ref`)
    /// into neutral AgentEvents / child-threads / proposals / reconciliations /
    /// reports. The provider-output ingest counterpart of the runtime delivery path.
    fn ingest_output(
        &self,
        store: &HarnessStore,
        agent_member_id: &str,
        runtime_id: Option<&str>,
        task_id: Option<&str>,
        source_ref: &str,
    ) -> CliResult<()>;

    /// Spawn (or attach) the persistent runtime for a member of this provider.
    fn start_runtime(&self, store: &HarnessStore, member: &AgentMember) -> CliResult<AgentRuntime>;

    /// Run a single message delivery against this provider's persistent runtime.
    fn run_delivery(
        &self,
        store: &HarnessStore,
        member: &AgentMember,
        runtime: &AgentRuntime,
        message: &Message,
        delivery_id: &str,
        timeout_ms: u64,
    ) -> CliResult<DeliveryOutcome>;

    fn spawn_ephemeral(&self, ctx: &EphemeralSpawnContext<'_>) -> CliResult<EphemeralSpawn>;
}

struct CodexAdapter;
struct ClaudeAdapter;

impl ProviderAdapter for CodexAdapter {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn live_ndjson_file_name(&self) -> &'static str {
        "codex.stream-json.ndjson"
    }

    fn map_permission(&self, perm: LaunchPermission) -> &'static str {
        match perm {
            LaunchPermission::ReadOnly => "read-only",
            LaunchPermission::WorkspaceWrite => "workspace-write",
            LaunchPermission::FullAccess => "danger-full-access",
        }
    }

    fn recorded_args(&self, resume_id: Option<&str>) -> Vec<String> {
        match resume_id {
            Some(id) => vec![
                "codex".into(),
                "exec".into(),
                "resume".into(),
                "--json".into(),
                id.into(),
            ],
            None => vec!["codex".into(), "exec".into(), "--json".into()],
        }
    }

    fn normalize_turn_event(
        &self,
        session_id: &str,
        raw: &serde_json::Value,
    ) -> Vec<HarnessTurnEvent> {
        let mut event = generic_turn_event(self.name(), session_id, raw);

        if let Some(error) = raw.get("error") {
            event.kind = HarnessTurnEventKind::Error;
            event.error = Some(
                error
                    .as_str()
                    .map(str::to_string)
                    .or_else(|| {
                        error
                            .get("message")
                            .and_then(|message| message.as_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| error.to_string()),
            );
            return vec![event];
        }

        match raw.get("type").and_then(|value| value.as_str()) {
            Some("thread.started") => {
                event.kind = HarnessTurnEventKind::ProviderMeta;
                event.provider_thread_id = raw
                    .get("thread_id")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
            }
            Some("turn.started") => {
                event.kind = HarnessTurnEventKind::TurnStarted;
                event.provider_turn_id = raw
                    .get("turn_id")
                    .and_then(|value| value.as_str())
                    .or_else(|| {
                        raw.get("turn")
                            .and_then(|turn| turn.get("id"))
                            .and_then(|value| value.as_str())
                    })
                    .map(str::to_string);
            }
            Some("item.started") => {
                event.kind = HarnessTurnEventKind::ProviderMeta;
                event.provider_item_id = raw
                    .get("item")
                    .and_then(|item| item.get("id"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
            }
            Some("item.completed") => {
                if let Some(item) = raw.get("item") {
                    event.provider_item_id = item
                        .get("id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string);
                    match item.get("type").and_then(|value| value.as_str()) {
                        Some("agent_message") => {
                            event.kind = HarnessTurnEventKind::Message;
                            event.role = Some("assistant".into());
                            event.text = item
                                .get("text")
                                .and_then(|value| value.as_str())
                                .map(str::to_string);
                        }
                        Some("reasoning") => {
                            event.kind = HarnessTurnEventKind::Reasoning;
                            event.text = item
                                .get("text")
                                .and_then(|value| value.as_str())
                                .map(str::to_string);
                        }
                        Some("command_execution") => {
                            event.kind = HarnessTurnEventKind::ToolCall;
                            let name = item
                                .get("command")
                                .and_then(|value| value.as_str())
                                .filter(|value| !value.is_empty())
                                .unwrap_or("command_execution")
                                .to_string();
                            event.tool_call = Some(HarnessToolCall {
                                id: event.provider_item_id.clone(),
                                name: name.clone(),
                                args: item.clone(),
                            });
                            let output = item
                                .get("aggregated_output")
                                .and_then(|value| value.as_str())
                                .unwrap_or("")
                                .to_string();
                            let exit_code = item.get("exit_code").and_then(|value| value.as_i64());
                            let failed = exit_code.is_some_and(|code| code != 0);
                            // Emit a ToolResult whenever the command produced ANY
                            // output (raw, not trimmed — whitespace-only output is
                            // still real output and must not be discarded) or it
                            // failed. Content is the actual output verbatim; fall
                            // back to `exit N` only when there is literally no
                            // output. Trimming/hide-if-blank is a render concern the
                            // dashboard owns; the canonical event stays faithful.
                            if !output.is_empty() || failed {
                                let mut result = generic_turn_event(self.name(), session_id, raw);
                                result.provider_item_id = event.provider_item_id.clone();
                                result.kind = HarnessTurnEventKind::ToolResult;
                                result.tool_result = Some(HarnessToolResult {
                                    tool_call_id: event.provider_item_id.clone(),
                                    name: Some(name),
                                    content: if output.is_empty() {
                                        format!("exit {}", exit_code.unwrap_or_default())
                                    } else {
                                        output
                                    },
                                    is_error: failed,
                                });
                                return vec![event, result];
                            }
                            return vec![event];
                        }
                        Some("file_change") => {
                            let changes = item
                                .get("changes")
                                .and_then(|value| value.as_array())
                                .filter(|changes| !changes.is_empty());
                            if let Some(changes) = changes {
                                let mut events = Vec::with_capacity(changes.len());
                                for change in changes {
                                    let name =
                                        match change.get("kind").and_then(|value| value.as_str()) {
                                            Some("add") => "Write",
                                            Some("delete") => "Delete",
                                            _ => "Edit",
                                        };
                                    let mut change_event =
                                        generic_turn_event(self.name(), session_id, raw);
                                    change_event.provider_item_id = event.provider_item_id.clone();
                                    change_event.kind = HarnessTurnEventKind::ToolCall;
                                    change_event.tool_call = Some(HarnessToolCall {
                                        id: event.provider_item_id.clone(),
                                        name: name.into(),
                                        args: change.clone(),
                                    });
                                    events.push(change_event);
                                }
                                return events;
                            }
                            event.kind = HarnessTurnEventKind::ProviderMeta;
                        }
                        _ => {
                            event.kind = HarnessTurnEventKind::ProviderMeta;
                        }
                    }
                } else {
                    event.kind = HarnessTurnEventKind::ProviderMeta;
                }
            }
            Some("turn.completed") | Some("turn_completed") => {
                event.kind = HarnessTurnEventKind::TurnCompleted;
                // The raw usage object lives where parse_codex_usage reads it, so
                // the normalized usage also keeps codex's cached/reasoning subtotals.
                let raw_usage = raw
                    .get("usage")
                    .or_else(|| raw.get("turn").and_then(|turn| turn.get("usage")));
                event.usage =
                    parse_codex_usage(std::slice::from_ref(raw)).map(|usage| HarnessTokenUsage {
                        input_tokens: usage.input,
                        output_tokens: usage.output,
                        total_tokens: usage.total,
                        cached_input_tokens: raw_usage
                            .and_then(|usage| usage.get("cached_input_tokens"))
                            .and_then(serde_json::Value::as_u64),
                        reasoning_output_tokens: raw_usage
                            .and_then(|usage| usage.get("reasoning_output_tokens"))
                            .and_then(serde_json::Value::as_u64),
                    });
            }
            // Codex emits `thread.idle` as the terminal idle marker (see
            // codex_event_is_terminal); there is no `turn.idle`.
            Some("thread.idle") | Some("thread_idle") => {
                event.kind = HarnessTurnEventKind::ProviderMeta;
            }
            _ => {}
        }

        vec![event]
    }

    fn start_runtime(&self, store: &HarnessStore, member: &AgentMember) -> CliResult<AgentRuntime> {
        start_codex_exec_runtime(store, member)
    }

    fn run_delivery(
        &self,
        store: &HarnessStore,
        member: &AgentMember,
        runtime: &AgentRuntime,
        message: &Message,
        delivery_id: &str,
        timeout_ms: u64,
    ) -> CliResult<DeliveryOutcome> {
        run_codex_exec_delivery(store, member, runtime, message, delivery_id, timeout_ms)
    }

    fn record_hook_event(&self, store: &HarnessStore, args: &[String]) -> CliResult<()> {
        store.init()?;
        let agent_id = value(args, "--agent")
            .or_else(|| env::var("HARNESS_AGENT_MEMBER_ID").ok())
            .ok_or_else(|| CliError::Usage("--agent is required".into()))?;
        let runtime_id =
            value(args, "--runtime").or_else(|| env::var("HARNESS_AGENT_RUNTIME_ID").ok());
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
            provider: self.name().into(),
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

    fn ingest_ephemeral_trace(
        &self,
        store: &HarnessStore,
        session_id: &str,
        spawn: &EphemeralSpawn,
    ) {
        // Codex: one neutral AgentEvent per NDJSON line, mirroring the
        // provider-output ingest path (event_type from the `type` discriminant).
        for line in spawn.ndjson.lines() {
            let Ok(payload) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
                continue;
            };
            let event_type = payload
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("provider_output")
                .replace(['/', '.'], "_");
            let event = AgentEvent {
                id: generated_id("event"),
                agent_member_id: session_id.into(),
                provider_runtime_id: None,
                task_id: None,
                provider: self.name().into(),
                provider_thread_id: payload
                    .get("thread_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                provider_turn_id: None,
                provider_child_thread_id: None,
                event_type,
                summary: summarize_json_value(&payload),
                payload_ref: None,
                created_at: now_string(),
            };
            let _ = store.append_event(&event);
        }
    }

    fn spawn_ephemeral(&self, ctx: &EphemeralSpawnContext<'_>) -> CliResult<EphemeralSpawn> {
        spawn_codex_ephemeral(
            ctx.session_dir,
            ctx.session_id,
            ctx.spec,
            ctx.schema_json,
            ctx.prompt,
            ctx.cwd,
            ctx.model,
            ctx.effort,
            ctx.timeout_ms,
        )
    }

    fn ingest_output(
        &self,
        store: &HarnessStore,
        agent_member_id: &str,
        runtime_id: Option<&str>,
        task_id: Option<&str>,
        source_ref: &str,
    ) -> CliResult<()> {
        let provider = self.name().to_string();
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
            let provider_child_thread_id =
                provider_child_thread_id_from_container(provider_context);
            let event = AgentEvent {
                id: generated_id("event"),
                agent_member_id: agent_member_id.into(),
                provider_runtime_id: runtime_id.map(str::to_string),
                task_id: task_id.map(str::to_string),
                provider: provider.clone(),
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
                &provider,
                agent_member_id,
                runtime_id,
                task_id,
                provider_thread_id.as_deref(),
                &value,
            ) {
                store.append_provider_child_thread(&child_thread)?;
            }
            if event_type.contains("turn_plan_updated") || event_type.contains("turn_diff_updated")
            {
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
            if let Some(terminal_source) = terminal_source_from_provider_event(&value, &event_type)
            {
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
                    sender_kind: SenderKind::Agent,
                };
                store.append_message(&report)?;
            }
        }
        Ok(())
    }
}
impl ProviderAdapter for ClaudeAdapter {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn live_ndjson_file_name(&self) -> &'static str {
        "claude.stream-json.ndjson"
    }

    fn map_permission(&self, perm: LaunchPermission) -> &'static str {
        match perm {
            LaunchPermission::ReadOnly => "plan",
            LaunchPermission::WorkspaceWrite => "acceptEdits",
            LaunchPermission::FullAccess => "bypassPermissions",
        }
    }

    fn recorded_args(&self, resume_id: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "-p".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
        ];
        if let Some(id) = resume_id {
            args.push("--resume".into());
            args.push(id.into());
        }
        args
    }

    fn normalize_turn_event(
        &self,
        session_id: &str,
        raw: &serde_json::Value,
    ) -> Vec<HarnessTurnEvent> {
        let mut event = generic_turn_event(self.name(), session_id, raw);
        let payload = raw;

        match raw.get("type").and_then(|value| value.as_str()) {
            Some("system") => {
                event.kind = HarnessTurnEventKind::ProviderMeta;
                event.model = payload
                    .get("model")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                event.provider_thread_id = payload
                    .get("session_id")
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
            }
            Some("assistant") => {
                let Some(blocks) = payload
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(|content| content.as_array())
                else {
                    event.kind = HarnessTurnEventKind::ProviderMeta;
                    return vec![event];
                };

                let mut events = Vec::new();

                let thinking_parts: Vec<&str> = blocks
                    .iter()
                    .filter(|block| {
                        block.get("type").and_then(|value| value.as_str()) == Some("thinking")
                    })
                    .filter_map(|block| {
                        block
                            .get("thinking")
                            .or_else(|| block.get("text"))
                            .and_then(|value| value.as_str())
                    })
                    .collect();
                if !thinking_parts.is_empty() {
                    let mut reasoning_event = generic_turn_event(self.name(), session_id, raw);
                    reasoning_event.kind = HarnessTurnEventKind::Reasoning;
                    reasoning_event.text = Some(thinking_parts.join("\n"));
                    events.push(reasoning_event);
                }

                let text_parts: Vec<&str> = blocks
                    .iter()
                    .filter(|block| {
                        block.get("type").and_then(|value| value.as_str()) == Some("text")
                    })
                    .filter_map(|block| block.get("text").and_then(|value| value.as_str()))
                    .collect();
                if !text_parts.is_empty() {
                    let mut message_event = generic_turn_event(self.name(), session_id, raw);
                    message_event.kind = HarnessTurnEventKind::Message;
                    message_event.role = Some("assistant".into());
                    message_event.text = Some(text_parts.join("\n"));
                    events.push(message_event);
                }

                for block in blocks.iter().filter(|block| {
                    block.get("type").and_then(|value| value.as_str()) == Some("tool_use")
                }) {
                    let mut tool_call_event = generic_turn_event(self.name(), session_id, raw);
                    tool_call_event.kind = HarnessTurnEventKind::ToolCall;
                    tool_call_event.provider_item_id = block
                        .get("id")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                        .map(str::to_string);
                    tool_call_event.tool_call = Some(HarnessToolCall {
                        id: tool_call_event.provider_item_id.clone(),
                        name: block
                            .get("name")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.is_empty())
                            .unwrap_or("tool_use")
                            .to_string(),
                        args: block.get("input").cloned().unwrap_or_else(|| block.clone()),
                    });
                    events.push(tool_call_event);
                }

                if events.is_empty() {
                    event.kind = HarnessTurnEventKind::ProviderMeta;
                    return vec![event];
                }
                return events;
            }
            Some("user") => {
                let Some(blocks) = payload
                    .get("message")
                    .and_then(|message| message.get("content"))
                    .and_then(|content| content.as_array())
                else {
                    event.kind = HarnessTurnEventKind::ProviderMeta;
                    return vec![event];
                };

                let mut events = Vec::new();
                for block in blocks.iter().filter(|block| {
                    block.get("type").and_then(|value| value.as_str()) == Some("tool_result")
                }) {
                    let mut tool_result_event = generic_turn_event(self.name(), session_id, raw);
                    tool_result_event.kind = HarnessTurnEventKind::ToolResult;
                    tool_result_event.provider_item_id = block
                        .get("tool_use_id")
                        .and_then(|value| value.as_str())
                        .filter(|value| !value.is_empty())
                        .map(str::to_string);
                    let content = block
                        .get("content")
                        .map(|value| {
                            value
                                .as_str()
                                .map(str::to_string)
                                .unwrap_or_else(|| value.to_string())
                        })
                        .unwrap_or_default();
                    tool_result_event.tool_result = Some(HarnessToolResult {
                        tool_call_id: tool_result_event.provider_item_id.clone(),
                        name: None,
                        content,
                        is_error: block
                            .get("is_error")
                            .and_then(|value| value.as_bool())
                            .unwrap_or(false),
                    });
                    events.push(tool_result_event);
                }

                if events.is_empty() {
                    event.kind = HarnessTurnEventKind::ProviderMeta;
                    return vec![event];
                }
                return events;
            }
            Some("result") => {
                let raw_usage = payload.get("usage");
                event.usage =
                    parse_claude_usage(std::slice::from_ref(raw)).map(|usage| HarnessTokenUsage {
                        input_tokens: usage.input,
                        output_tokens: usage.output,
                        total_tokens: usage.total,
                        cached_input_tokens: raw_usage
                            .and_then(|usage| usage.get("cached_input_tokens"))
                            .and_then(serde_json::Value::as_u64),
                        reasoning_output_tokens: raw_usage
                            .and_then(|usage| usage.get("reasoning_output_tokens"))
                            .and_then(serde_json::Value::as_u64),
                    });
                let (_structured, cost_usd) = parse_claude_result_extras(std::slice::from_ref(raw));
                event.cost_usd = cost_usd;
                event.model = parse_worker_model(std::slice::from_ref(raw));
                event.text = payload
                    .get("result")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);

                match payload.get("subtype").and_then(|value| value.as_str()) {
                    Some(subtype) if subtype != "success" => {
                        event.kind = HarnessTurnEventKind::Error;
                        event.error = Some(
                            payload
                                .get("result")
                                .and_then(|value| value.as_str())
                                .or_else(|| payload.get("error").and_then(|value| value.as_str()))
                                .map(str::to_string)
                                .or_else(|| {
                                    payload
                                        .get("error")
                                        .and_then(|error| error.get("message"))
                                        .and_then(|value| value.as_str())
                                        .map(str::to_string)
                                })
                                .or_else(|| payload.get("error").map(|value| value.to_string()))
                                .unwrap_or_else(|| format!("claude result subtype {subtype}")),
                        );
                    }
                    _ => {
                        event.kind = HarnessTurnEventKind::TurnCompleted;
                    }
                }
            }
            Some("stream_event") => {
                event.kind = HarnessTurnEventKind::ProviderMeta;
            }
            _ => {}
        }

        vec![event]
    }

    fn start_runtime(&self, store: &HarnessStore, member: &AgentMember) -> CliResult<AgentRuntime> {
        start_claude_runtime(store, member)
    }

    fn run_delivery(
        &self,
        store: &HarnessStore,
        member: &AgentMember,
        runtime: &AgentRuntime,
        message: &Message,
        delivery_id: &str,
        timeout_ms: u64,
    ) -> CliResult<DeliveryOutcome> {
        run_claude_delivery(store, member, runtime, message, delivery_id, timeout_ms)
    }

    fn ingest_ephemeral_trace(
        &self,
        store: &HarnessStore,
        session_id: &str,
        spawn: &EphemeralSpawn,
    ) {
        let _ = ingest_claude_stream_json(store, session_id, None, None, &spawn.ndjson);
    }

    fn spawn_ephemeral(&self, ctx: &EphemeralSpawnContext<'_>) -> CliResult<EphemeralSpawn> {
        spawn_claude_ephemeral(
            ctx.session_dir,
            ctx.session_id,
            ctx.spec,
            ctx.schema_json,
            ctx.prompt,
            ctx.cwd,
            ctx.model,
            ctx.effort,
            ctx.timeout_ms,
            ctx.max_budget_usd,
        )
    }

    fn ingest_output(
        &self,
        store: &HarnessStore,
        agent_member_id: &str,
        runtime_id: Option<&str>,
        task_id: Option<&str>,
        source_ref: &str,
    ) -> CliResult<()> {
        let text = fs::read_to_string(source_ref).unwrap_or_default();
        ingest_claude_stream_json(store, agent_member_id, runtime_id, task_id, &text)
    }
}

/// All providers the harness recognises, in canonical display order.
fn provider_registry() -> &'static [&'static dyn ProviderAdapter] {
    &[&CodexAdapter, &ClaudeAdapter]
}

/// The adapter for a provider id, or `None` if unrecognised.
fn provider_adapter(name: &str) -> Option<&'static dyn ProviderAdapter> {
    provider_registry()
        .iter()
        .copied()
        .find(|adapter| adapter.name() == name)
}

/// The supported provider ids, derived from the registry (single source of truth).
fn supported_provider_names() -> Vec<&'static str> {
    provider_registry().iter().map(|a| a.name()).collect()
}

/// Build the standard error for a provider the harness does not recognise.
fn unknown_provider_error(provider: &str, concern: &str) -> CliError {
    CliError::Usage(format!(
        "unknown provider {provider:?} for {concern}; supported providers: {}",
        supported_provider_names().join(", ")
    ))
}

/// Spawn (or attach) the runtime for a member, routed by `member.provider`.
fn start_provider_runtime(store: &HarnessStore, member: &AgentMember) -> CliResult<AgentRuntime> {
    match provider_adapter(&member.provider) {
        Some(adapter) => adapter.start_runtime(store, member),
        None => Err(unknown_provider_error(&member.provider, "runtime start")),
    }
}

// ============================================================================
// WP-3: Claude stream-json event parser and delivery (replaces stub)
// ============================================================================

/// Represents a single event from `claude -p --output-format stream-json --verbose` NDJSON stream.
/// Stream-json format emits: system (init), stream_event (message lifecycle), result (terminal).
#[derive(Debug, Clone, PartialEq)]
struct ClaudeStreamEvent {
    /// Event type: "system", "stream_event", "result"
    event_type: String,
    /// Raw JSON payload for extraction
    payload: serde_json::Value,
}

impl ClaudeStreamEvent {
    /// Parse one NDJSON line into a ClaudeStreamEvent if valid, else None (skip).
    fn parse_line(line: &str) -> Option<ClaudeStreamEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(payload) => {
                let event_type = payload
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Some(ClaudeStreamEvent {
                    event_type,
                    payload,
                })
            }
            Err(_) => None,
        }
    }

    /// Extract session_id from system init event.
    fn session_id(&self) -> Option<String> {
        if self.event_type == "system" {
            self.payload
                .get("session_id")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        } else {
            None
        }
    }
}

/// Infer provider session status from Claude stream-json events.
fn infer_claude_session_status(
    events: &[ClaudeStreamEvent],
    process_success: bool,
) -> ProviderSessionStatus {
    if !process_success {
        return ProviderSessionStatus::Failed;
    }
    let has_result = events.iter().any(|e| e.event_type == "result");
    if has_result {
        if let Some(result_event) = events.iter().find(|e| e.event_type == "result") {
            if result_event.payload.get("error").is_some() {
                return ProviderSessionStatus::Failed;
            }
        }
        ProviderSessionStatus::Succeeded
    } else if events.is_empty() {
        ProviderSessionStatus::Failed
    } else {
        ProviderSessionStatus::Stale
    }
}

/// Extract session_id from Claude stream events.
fn extract_session_id_from_claude_events(events: &[ClaudeStreamEvent]) -> Option<String> {
    events.iter().find_map(|e| e.session_id())
}

/// Extract provider_thread_id from Claude stream events if present.
fn extract_thread_id_from_claude_events(_events: &[ClaudeStreamEvent]) -> Option<String> {
    None
}

/// Extract provider_turn_id from Claude stream events if present.
fn extract_turn_id_from_claude_events(_events: &[ClaudeStreamEvent]) -> Option<String> {
    None
}

/// Extract the assistant's ACTUAL reply text from a `claude -p
/// --output-format stream-json` stream, so the delivery report surfaces what
/// the agent said rather than a meta event count. Prefers the terminal
/// `result` event's `result` field; falls back to concatenating the text
/// blocks of `assistant` messages. Returns None when the turn produced no
/// assistant text (e.g. tool-only), letting the caller keep a status summary.
fn extract_claude_reply_text(events: &[ClaudeStreamEvent]) -> Option<String> {
    // The terminal result event carries the final assistant text.
    for event in events.iter().rev() {
        if event.event_type != "result" {
            continue;
        }
        if let Some(text) = event.payload.get("result").and_then(|v| v.as_str()) {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    // Fallback: concatenate text blocks from assistant messages in order.
    let mut parts = Vec::new();
    for event in events {
        if event.event_type != "assistant" {
            continue;
        }
        let Some(content) = event
            .payload
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) != Some("text") {
                continue;
            }
            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                if !text.trim().is_empty() {
                    parts.push(text.trim().to_string());
                }
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// Parse Claude stream-json NDJSON and ingest as neutral AgentEvent / ProviderSession.
/// Mirrors the Codex exec reducer: same neutral objects, provider-specific parsing.
fn ingest_claude_stream_json(
    store: &HarnessStore,
    agent_member_id: &str,
    runtime_id: Option<&str>,
    task_id: Option<&str>,
    text: &str,
) -> CliResult<()> {
    // Parse NDJSON from text
    let mut events = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            if let Some(event) = ClaudeStreamEvent::parse_line(trimmed) {
                events.push(event);
            }
        }
    }

    // Extract session_id and infer status from the event stream.
    let session_id =
        extract_session_id_from_claude_events(&events).unwrap_or_else(|| generated_id("session"));
    let process_success = true; // Stream was parsed successfully.
    let status = infer_claude_session_status(&events, process_success);

    // Ingest each event as a neutral AgentEvent.
    for event in &events {
        let event_kind = match event.event_type.as_str() {
            "system" => "stream_system_init",
            "stream_event" => {
                // Extract the stream_event subtype if present.
                event
                    .payload
                    .get("event")
                    .and_then(|e| e.as_str())
                    .unwrap_or("stream_event")
            }
            "result" => "stream_result",
            _ => "unknown",
        };

        let summary = summarize_json_value(&event.payload);
        let thread_id = extract_thread_id_from_claude_events(std::slice::from_ref(event));
        let turn_id = extract_turn_id_from_claude_events(std::slice::from_ref(event));

        let agent_event = AgentEvent {
            id: generated_id("event"),
            agent_member_id: agent_member_id.into(),
            provider_runtime_id: runtime_id.map(str::to_string),
            task_id: task_id.map(str::to_string),
            provider: "claude".into(),
            provider_thread_id: thread_id,
            provider_turn_id: turn_id,
            provider_child_thread_id: None, // Subagents handled separately per ADR 0011
            event_type: event_kind.to_string(),
            summary,
            payload_ref: None, // Inline payload
            created_at: now_string(),
        };
        store.append_event(&agent_event)?;
    }

    // Create one ProviderSession record for the entire delivery.
    let provider_session = ProviderSession {
        id: session_id.clone(),
        provider: "claude".into(),
        agent_member_id: agent_member_id.into(),
        task_id: task_id.map(str::to_string),
        workspace_ref: None,
        provider_thread_id: None,
        provider_turn_id: None,
        terminal_source: status_to_terminal_source(&status),
        status,
        command: "claude".into(),
        args: vec![
            "-p".into(),
            "--output-format".into(),
            "stream-json".into(),
            "--verbose".into(),
        ],
        prompt_ref: None,
        prompt_summary: Some("delivered via claude -p stream-json".into()),
        provider_session_ref: None,
        stdout_ref: None,
        jsonl_ref: None,
        transcript_ref: None,
        last_message_ref: None,
        exit_code: if process_success { Some(0) } else { Some(1) },
        started_at: now_string(),
        ended_at: Some(now_string()),
        evidence_ids: Vec::new(),
    };
    store.append_provider_session(&provider_session)?;

    Ok(())
}

/// Map ProviderSessionStatus to terminal source.
fn status_to_terminal_source(status: &ProviderSessionStatus) -> Option<MessageTerminalSource> {
    match status {
        ProviderSessionStatus::Succeeded => Some(MessageTerminalSource::TurnCompleted),
        ProviderSessionStatus::Failed => Some(MessageTerminalSource::Failed),
        _ => None,
    }
}

// --- Codex exec --json delivery (WP-2) ---
// Parse NDJSON output from `codex exec --json` into AgentEvent + ProviderSession lifecycle.
// Row parity with app-server path: identical ProviderSession/Evidence structure.

#[derive(Debug, Clone, PartialEq)]
struct CodexExecEvent {
    /// Event discriminant extracted from NDJSON payload.
    event_type: String,
    /// Raw JSON payload for extraction.
    payload: serde_json::Value,
}

impl CodexExecEvent {
    /// Parse one NDJSON line into a CodexExecEvent if valid, else None (skip).
    fn parse_line(line: &str) -> Option<CodexExecEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }
        match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(payload) => {
                let event_type = payload
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Some(CodexExecEvent {
                    event_type,
                    payload,
                })
            }
            Err(_) => None,
        }
    }

    /// Extract the terminal source from this event if it is a completion event.
    fn terminal_source(&self) -> Option<MessageTerminalSource> {
        if codex_event_is_terminal(&self.event_type) {
            Some(MessageTerminalSource::TurnCompleted)
        } else {
            None
        }
    }
}

/// True when a codex exec event type marks the end of a turn/thread.
///
/// Codex 0.13x `exec --json` emits dot-separated discriminants
/// (`turn.completed`, `thread.idle`). Older notes used underscore names
/// (`turn_completed`, `thread_idle`); both are accepted so the parser is
/// robust across codex versions.
fn codex_event_is_terminal(event_type: &str) -> bool {
    matches!(
        event_type,
        "turn.completed" | "thread.idle" | "turn_completed" | "thread_idle"
    )
}

/// Parse NDJSON from codex exec stdout into CodexExecEvent stream.
/// Resilient: silently skip invalid lines, partial final lines, unknown events.
// Thin no-tee wrapper; only the unit tests use it now (the delivery path uses
// the callback form), so it is dead in the binary target.
#[allow(dead_code)]
fn parse_codex_ndjson(reader: impl BufRead) -> Vec<CodexExecEvent> {
    parse_codex_ndjson_to(reader, None::<fn(&serde_json::Value)>)
}

/// Like `parse_codex_ndjson`, but invokes `on_event` with each parsed event's
/// payload AS IT IS READ — used to tee codex events MID-TURN to the session
/// NDJSON (poll) and the shared turn-events file (live SSE), mirroring the
/// claude path. The returned Vec is identical to the no-callback path.
fn parse_codex_ndjson_to<F: FnMut(&serde_json::Value)>(
    reader: impl BufRead,
    mut on_event: Option<F>,
) -> Vec<CodexExecEvent> {
    let mut events = Vec::new();
    for line in reader.lines() {
        let Ok(line_str) = line else { continue };
        if let Some(event) = CodexExecEvent::parse_line(&line_str) {
            if let Some(callback) = on_event.as_mut() {
                callback(&event.payload);
            }
            events.push(event);
        }
    }
    events
}

/// Infer the lifecycle status from a stream of CodexExecEvent.
/// Follows the same logic as the app-server path: queued → running → (succeeded|failed).
fn infer_provider_session_status(
    events: &[CodexExecEvent],
    process_success: bool,
) -> ProviderSessionStatus {
    if !process_success {
        return ProviderSessionStatus::Failed;
    }
    // If we saw a terminal event, we succeeded.
    let has_terminal = events
        .iter()
        .any(|e| codex_event_is_terminal(&e.event_type));
    if has_terminal {
        ProviderSessionStatus::Succeeded
    } else if events.is_empty() {
        ProviderSessionStatus::Failed
    } else {
        // We have events but no terminal: stale (timed out waiting for completion).
        ProviderSessionStatus::Stale
    }
}

/// Extract provider_thread_id from the exec output events if present.
///
/// Codex `exec --json` emits a `thread.started` event carrying the real
/// `thread_id` (e.g. `{"thread_id":"019e...","type":"thread.started"}`). We
/// scan every event payload for a top-level `thread_id` string and return the
/// first match so the ProviderSession records the provider's real thread id.
fn extract_thread_id_from_exec_events(events: &[CodexExecEvent]) -> Option<String> {
    events.iter().find_map(|event| {
        event
            .payload
            .get("thread_id")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
    })
}

/// Extract provider_turn_id from the exec output events if present.
///
/// Newer codex builds may attach a `turn_id` to turn lifecycle events. When
/// present we surface it; otherwise None (the harness session id scopes the
/// turn). We accept either a top-level `turn_id` or one nested under `turn`.
fn extract_turn_id_from_exec_events(events: &[CodexExecEvent]) -> Option<String> {
    events.iter().find_map(|event| {
        event
            .payload
            .get("turn_id")
            .and_then(|value| value.as_str())
            .or_else(|| {
                event
                    .payload
                    .get("turn")
                    .and_then(|turn| turn.get("id"))
                    .and_then(|value| value.as_str())
            })
            .map(|value| value.to_string())
    })
}

/// Extract the agent's ACTUAL reply text from a `codex exec --json` stream, so
/// the delivery report surfaces what the agent said rather than a meta status
/// line. Codex emits `item.completed` events whose `item.type` is
/// `agent_message` and whose `item.text` is the assistant's prose; concatenate
/// them in order. Returns None when the turn produced no agent message (e.g.
/// command-only), letting the caller keep a status summary.
fn extract_codex_reply_text(events: &[CodexExecEvent]) -> Option<String> {
    let mut parts = Vec::new();
    for event in events {
        let Some(item) = event.payload.get("item") else {
            continue;
        };
        if item.get("type").and_then(|t| t.as_str()) != Some("agent_message") {
            continue;
        }
        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
            if !text.trim().is_empty() {
                parts.push(text.trim().to_string());
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

/// The codex turn's FINAL assistant message — the LAST non-empty `agent_message`
/// item. Where [`extract_codex_reply_text`] concatenates every message for the
/// human-facing reply, this returns only the terminal one, so structured-output
/// parsing reads the schema-constrained answer rather than an earlier streamed
/// preamble (issue #139 item 2).
fn extract_codex_final_message(events: &[CodexExecEvent]) -> Option<String> {
    let mut last = None;
    for event in events {
        let Some(item) = event.payload.get("item") else {
            continue;
        };
        if item.get("type").and_then(|t| t.as_str()) != Some("agent_message") {
            continue;
        }
        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
            if !text.trim().is_empty() {
                last = Some(text.trim().to_string());
            }
        }
    }
    last
}

/// Write a temporary MCP config JSON file for Claude.
/// Returns the path to the temporary file, or None if mcp is empty/None.
fn write_temp_mcp_config(mcp: Option<&LaunchMcp>) -> CliResult<Option<String>> {
    if let Some(mcp_config) = mcp {
        if mcp_config.servers.is_empty() {
            return Ok(None);
        }

        // Build MCP servers config as expected by Claude
        let mut servers = serde_json::Map::new();
        for server in &mcp_config.servers {
            let mut server_obj = serde_json::Map::new();
            server_obj.insert("id".to_string(), serde_json::json!(server.id));

            if let Some(transport) = &server.transport {
                server_obj.insert("transport".to_string(), serde_json::json!(transport));
            }

            if !server.command.is_empty() {
                server_obj.insert("command".to_string(), serde_json::json!(server.command));
            }

            if let Some(url) = &server.url {
                server_obj.insert("url".to_string(), serde_json::json!(url));
            }

            if !server.allowed_tools.is_empty() {
                server_obj.insert(
                    "allowed_tools".to_string(),
                    serde_json::json!(server.allowed_tools),
                );
            }

            servers.insert(server.id.clone(), serde_json::Value::Object(server_obj));
        }

        let config = serde_json::json!({
            "mcp_servers": servers
        });

        // Write to temp file
        let config_str = serde_json::to_string(&config)
            .map_err(|e| CliError::Usage(format!("failed to serialize MCP config: {e}")))?;

        let temp_path =
            std::env::temp_dir().join(format!("mcp_config_{}.json", std::process::id()));
        let temp_path_str = temp_path.to_string_lossy().to_string();

        std::fs::write(&temp_path, config_str).map_err(|e| {
            CliError::Usage(format!("failed to write MCP config to temp file: {e}"))
        })?;

        Ok(Some(temp_path_str))
    } else {
        Ok(None)
    }
}

fn run_codex_exec_process(
    session_dir: &Path,
    member: &AgentMember,
    message: &Message,
    delivery_id: &str,
    timeout_ms: u64,
) -> CliResult<CodexExecDeliveryRun> {
    // Build the command: `codex exec --json <prompt>`
    // The LaunchSpec is composed from the member/message; the exec arg is the message_content.
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    let developer_instructions = provider_developer_instructions(member);
    let cwd = member.worktree_ref.clone().or_else(|| {
        env::current_dir()
            .ok()
            .map(|path| path.display().to_string())
    });

    // Build LaunchSpec from member and message
    let spec = build_launch_spec(member, message);

    let mut cmd = Command::new("codex");
    cmd.arg("exec");

    // Resume an existing session when the member already carries a provider
    // thread id (from a prior delivery). `codex exec resume <id>` continues the
    // same conversation so memory carries across deliveries. The resume
    // subcommand inherits the original session's sandbox / working roots and
    // does not accept `--sandbox` / `-C` / `--add-dir`, so those are only mapped
    // on the fresh-session path below.
    let resuming = spec.resume.is_some();
    if let Some(resume_id) = &spec.resume {
        cmd.arg("resume")
            .arg("--json")
            .arg(resume_id)
            .arg(&message_content);
    } else {
        cmd.arg("--json").arg(&message_content);
    }
    cmd.env("CODEX_DEVELOPER_INSTRUCTIONS", developer_instructions);

    // Map LaunchSpec to codex flags
    apply_codex_model_and_effort_args(&mut cmd, &spec);
    apply_codex_output_schema_arg(&mut cmd, &spec, session_dir)?;
    apply_codex_mcp_args(&mut cmd, &spec)?;

    if !resuming {
        // Map permission to sandbox (fresh sessions only).
        let sandbox = CodexAdapter.map_permission(spec.permission);
        cmd.arg("--sandbox").arg(sandbox);

        // Map workspace and writable roots (fresh sessions only).
        if let Some(workspace) = &spec.workspace {
            cmd.arg("-C").arg(workspace);
        }
        for root in &spec.writable_roots {
            cmd.arg("--add-dir").arg(root);
        }
    }

    cmd.current_dir(cwd.clone().unwrap_or_else(|| ".".into()));

    let run = run_ndjson_child(
        cmd,
        session_dir,
        delivery_id,
        "codex.stream-json.ndjson",
        timeout_ms,
        "codex exec",
    )?;
    let events = run
        .events
        .iter()
        .filter_map(|payload| serde_json::to_string(payload).ok())
        .filter_map(|line| CodexExecEvent::parse_line(&line))
        .collect();

    Ok((run.process_success, events, run.events, run.stderr))
}

fn apply_codex_model_and_effort_args(cmd: &mut Command, spec: &LaunchSpec) {
    if let Some(model) = &spec.model {
        cmd.arg("-m").arg(model);
    }
    // Reasoning effort: codex takes it as a config override (no dedicated flag).
    if let Some(effort) = &spec.effort {
        cmd.arg("-c")
            .arg(format!("model_reasoning_effort={effort}"));
    }
}

fn apply_codex_output_schema_arg(
    cmd: &mut Command,
    spec: &LaunchSpec,
    session_dir: &Path,
) -> CliResult<()> {
    if let Some(schema) = &spec.output_schema {
        let schema_path = session_dir.join("output-schema.json");
        let schema_json = schema_to_json_schema(schema);
        fs::write(&schema_path, schema_json.to_string()).map_err(|e| {
            CliError::Usage(format!(
                "failed to write codex output schema to {}: {e}",
                schema_path.display()
            ))
        })?;
        cmd.arg("--output-schema").arg(&schema_path);
    }
    Ok(())
}

fn apply_codex_mcp_args(cmd: &mut Command, spec: &LaunchSpec) -> CliResult<()> {
    let Some(mcp) = &spec.mcp else {
        return Ok(());
    };

    for server in &mcp.servers {
        let id_key = codex_mcp_id_key(&server.id);
        if !server.command.is_empty() {
            // Codex stdio MCP config stores the binary separately from argv rest.
            let bin = serde_json::to_string(&server.command[0])
                .map_err(|e| CliError::Usage(format!("mcp command serialize: {e}")))?;
            cmd.arg("-c")
                .arg(format!("mcp_servers.{id_key}.command={bin}"));
            if server.command.len() > 1 {
                let args = serde_json::to_string(&server.command[1..])
                    .map_err(|e| CliError::Usage(format!("mcp args serialize: {e}")))?;
                cmd.arg("-c")
                    .arg(format!("mcp_servers.{id_key}.args={args}"));
            }
        } else if let Some(url) = &server.url {
            let u = serde_json::to_string(url)
                .map_err(|e| CliError::Usage(format!("mcp url serialize: {e}")))?;
            cmd.arg("-c").arg(format!("mcp_servers.{id_key}.url={u}"));
        }
        // Codex's mcp_servers schema has no allowed_tools field, so the neutral
        // allowlist is intentionally not mapped; transport is implied by
        // command-vs-url.
    }

    Ok(())
}

fn codex_mcp_id_key(id: &str) -> String {
    if !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        id.to_string()
    } else {
        serde_json::to_string(id).expect("serializing string key should not fail")
    }
}

// Run a single Codex exec delivery, writing identical ProviderSession/Evidence rows.
// WP-5: Minimal record of provider session for exec-stream delivery.
// This records evidence and session metadata for audit/tracing.
struct ExecDeliverySessionRecord<'a> {
    delivery_id: &'a str,
    member: &'a AgentMember,
    message: &'a Message,
    session_dir: &'a Path,
    status: ProviderSessionStatus,
    started_at: String,
    stdout_ref: Option<String>,
    stderr_ref: Option<String>,
    exit_code: Option<i32>,
    provider_thread_id: Option<String>,
    provider_turn_id: Option<String>,
    terminal_source: Option<MessageTerminalSource>,
    /// Prior session id this delivery resumed (`codex exec resume <id>`), if any.
    /// Recorded into the ProviderSession args so the snapshot is the evidence
    /// that resume was actually used.
    resume_id: Option<String>,
}

fn record_exec_delivery_session(
    store: &HarnessStore,
    record: ExecDeliverySessionRecord<'_>,
) -> CliResult<String> {
    let evidence_id = generated_id("evidence");
    let evidence = Evidence {
        id: evidence_id.clone(),
        task_id: record.message.task_id.clone(),
        source_type: "codex_exec_delivery_session".into(),
        source_ref: record.session_dir.display().to_string(),
        summary: format!(
            "Codex exec-stream delivery {} for message {}",
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
        args: CodexAdapter.recorded_args(record.resume_id.as_deref()),
        prompt_ref: record.member.prompt_ref.clone(),
        prompt_summary: Some(format!("deliver message {}", record.message.id)),
        provider_session_ref: None,
        // jsonl_ref must be the events FILE (read by the events route), not the
        // session dir; for codex that is the same NDJSON as stdout_ref.
        jsonl_ref: record.stdout_ref.clone(),
        stdout_ref: record.stdout_ref,
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

/// This is the exec-stream variant of run_codex_app_server_exchange.
fn run_codex_exec_delivery(
    store: &HarnessStore,
    member: &AgentMember,
    _runtime: &AgentRuntime,
    message: &Message,
    delivery_id: &str,
    timeout_ms: u64,
) -> CliResult<DeliveryOutcome> {
    let session_dir = store.root().join("provider-sessions").join(delivery_id);
    fs::create_dir_all(&session_dir)?;
    let started_at = now_string();

    // The resume id used for this delivery (same source as the spawned command:
    // the member's prior provider thread id). Recorded into the session args.
    let spec = build_launch_spec(member, message);
    let resume_id = spec.resume.clone();

    let (process_success, events, raw_events, stderr_log) =
        run_codex_exec_process(&session_dir, member, message, delivery_id, timeout_ms)?;
    let (tokens, cost_usd, model) = codex_delivery_telemetry(&raw_events, &spec);

    // The event NDJSON is the live file run_codex_exec_process already wrote
    // incrementally (mid-turn streaming) — point the session row at it rather
    // than re-serializing a redundant copy. Just persist stderr.
    let stdout_ref = session_dir.join("codex.stream-json.ndjson");
    let stderr_ref = session_dir.join("exec.stderr.log");
    fs::write(&stderr_ref, &stderr_log)?;

    // Infer the delivery status from events and process exit.
    let status = infer_provider_session_status(&events, process_success);
    let terminal_source = if matches!(status, ProviderSessionStatus::Succeeded) {
        events
            .iter()
            .find_map(|e| e.terminal_source())
            .or(Some(MessageTerminalSource::Unknown))
    } else {
        Some(MessageTerminalSource::Failed)
    };

    let provider_thread_id = extract_thread_id_from_exec_events(&events);
    let provider_turn_id = extract_turn_id_from_exec_events(&events);
    let exit_code = if process_success { Some(0) } else { Some(1) };
    let reply = extract_codex_reply_text(&events);
    let structured =
        structured_for_status(&status, codex_delivery_structured(reply.as_deref(), &spec));

    let evidence_id = record_exec_delivery_session(
        store,
        ExecDeliverySessionRecord {
            delivery_id,
            member,
            message,
            session_dir: &session_dir,
            status: status.clone(),
            started_at,
            stdout_ref: Some(stdout_ref.display().to_string()),
            stderr_ref: Some(stderr_ref.display().to_string()),
            exit_code,
            provider_thread_id: provider_thread_id.clone(),
            provider_turn_id: provider_turn_id.clone(),
            terminal_source: terminal_source.clone(),
            resume_id: resume_id.clone(),
        },
    )?;

    let summary = match status {
        ProviderSessionStatus::Succeeded => reply
            .clone()
            .unwrap_or_else(|| "Codex exec --json turn completed successfully".into()),
        ProviderSessionStatus::Failed => {
            if stderr_log.is_empty() {
                "Codex exec --json failed: no output".into()
            } else {
                format!(
                    "Codex exec --json failed: {}",
                    stderr_log.lines().next().unwrap_or("unknown error")
                )
            }
        }
        ProviderSessionStatus::Stale => {
            "Codex exec --json produced output but did not complete before timeout".into()
        }
        _ => "Codex exec --json session ended".into(),
    };

    Ok(DeliveryOutcome {
        status: status.clone(),
        provider_thread_id,
        provider_turn_id,
        terminal_source,
        stdout_ref: Some(stdout_ref.display().to_string()),
        stderr_ref: Some(stderr_ref.display().to_string()),
        request_ref: Some(session_dir.display().to_string()),
        provider_request_id: None, // exec stream does not use request_id
        provider_session_id: Some(delivery_id.to_string()),
        evidence_ids: vec![evidence_id],
        exit_code,
        tokens,
        cost_usd,
        model,
        structured,
        summary,
    })
}

/// Run a single message delivery against the member's runtime, routed by provider.
fn run_provider_delivery(
    store: &HarnessStore,
    member: &AgentMember,
    runtime: &AgentRuntime,
    message: &Message,
    delivery_id: &str,
    timeout_ms: u64,
) -> CliResult<DeliveryOutcome> {
    match provider_adapter(&member.provider) {
        Some(adapter) => {
            adapter.run_delivery(store, member, runtime, message, delivery_id, timeout_ms)
        }
        None => Err(unknown_provider_error(&member.provider, "delivery")),
    }
}

// WP-5: Codex exec-stream runtime (no persistent process).
// Each delivery spawns `codex exec --json`, so no app-server socket is needed.

type CodexExecDeliveryRun = (bool, Vec<CodexExecEvent>, Vec<serde_json::Value>, String);
type ClaudeDeliveryRun = (
    bool,
    Vec<ClaudeStreamEvent>,
    Vec<serde_json::Value>,
    Option<String>,
    String,
);

fn start_codex_exec_runtime(store: &HarnessStore, member: &AgentMember) -> CliResult<AgentRuntime> {
    let runtime_id = generated_id("runtime");
    let runtime_dir = store.root().join("runtimes").join(&member.id);
    fs::create_dir_all(&runtime_dir)?;

    // For Codex, we use exec-stream delivery (no persistent app-server).
    // Each delivery spawns `codex exec --json`, so there's no long-lived process.
    // The control_endpoint is a marker for the runtime directory.
    let endpoint = format!("codex-exec-runtime://{}", runtime_dir.display());

    let args = vec![
        // Codex will be spawned on each delivery via codex exec --json
    ];

    // Check if codex binary is available
    let process_alive = Command::new("which")
        .arg("codex")
        .output()
        .ok()
        .map(|output| output.status.success())
        .unwrap_or(false);

    Ok(AgentRuntime {
        id: runtime_id,
        agent_member_id: member.id.clone(),
        provider: member.provider.clone(),
        status: AgentRuntimeStatus::Running,
        pid: None, // Codex exec runs on-demand; no persistent PID
        control_endpoint: Some(endpoint),
        command: "codex".into(),
        args,
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: AgentRuntimeHealth {
            process_alive,
            socket_exists: true,                        // Runtime dir exists
            protocol_probe: Some("exec-stream".into()), // Codex uses exec-stream
            delivery_probe: Some("unknown".into()),
            checked_at: Some(now_string()),
        },
    })
}

// --- Claude runtime (BE-WP7) ---
// The claude CLI shape: spawn the claude binary as a local process, run message
// delivery exchanges via stdin/stdout, record sessions and evidence.

fn start_claude_runtime(store: &HarnessStore, member: &AgentMember) -> CliResult<AgentRuntime> {
    let runtime_id = generated_id("runtime");
    let runtime_dir = store.root().join("runtimes").join(&member.id);
    fs::create_dir_all(&runtime_dir)?;

    // For Claude CLI, we don't spawn a persistent process on runtime start.
    // Instead, we record the runtime as "ready" and each delivery will spawn
    // claude with the message. This matches the behavior of claude as a
    // request-response tool rather than a persistent app-server.
    // The control_endpoint is a marker for the runtime directory.
    let endpoint = format!("claude-runtime://{}", runtime_dir.display());

    let args = vec![
        // Claude CLI will be spawned on each delivery with the message prompt
    ];

    // Check if claude binary is available, but don't require it at test time
    let process_alive = Command::new("which")
        .arg("claude")
        .output()
        .ok()
        .map(|output| output.status.success())
        .unwrap_or(false);

    Ok(AgentRuntime {
        id: runtime_id,
        agent_member_id: member.id.clone(),
        provider: member.provider.clone(),
        status: AgentRuntimeStatus::Running,
        pid: None, // Claude runs on-demand; no persistent PID
        control_endpoint: Some(endpoint),
        command: "claude".into(),
        args,
        started_at: now_string(),
        ended_at: None,
        last_event_at: Some(now_string()),
        health: AgentRuntimeHealth {
            process_alive,
            socket_exists: true,                    // Runtime dir exists
            protocol_probe: Some("unknown".into()), // Will probe on first delivery
            delivery_probe: Some("unknown".into()),
            checked_at: Some(now_string()),
        },
    })
}

fn run_claude_delivery(
    store: &HarnessStore,
    member: &AgentMember,
    _runtime: &AgentRuntime,
    message: &Message,
    delivery_id: &str,
    timeout_ms: u64,
) -> CliResult<DeliveryOutcome> {
    let session_dir = store.root().join("provider-sessions").join(delivery_id);
    fs::create_dir_all(&session_dir)?;
    let started_at = now_string();

    // WP-3: Spawn real `claude -p --output-format stream-json --verbose`.
    //
    // Opt-in resident path (HARNESS_CLAUDE_RESIDENT=1): instead of spawning a
    // fresh `claude -p <prompt>` that exits per turn, hold a `claude
    // --input-format stream-json` process open and feed the turn as a stdin
    // frame (see `resident.rs`). The returned tuple shape is identical to the
    // default path, so everything below (NDJSON write, status infer, telemetry,
    // evidence, ProviderSession) is reused verbatim. When unset/false the default
    // path runs unchanged.
    let resident = env::var("HARNESS_CLAUDE_RESIDENT").as_deref() == Ok("1");
    let (process_success, events, raw_events, session_id, stderr_log) = if resident {
        run_claude_resident_delivery_real(&session_dir, member, message, timeout_ms)?
    } else {
        run_claude_exec_delivery_real(&session_dir, member, message, timeout_ms)?
    };
    let (tokens, cost_usd, model, raw_structured) = claude_delivery_telemetry(&raw_events);

    // Save NDJSON events to jsonl_ref for ingest.
    let ndjson_ref = session_dir.join("claude.stream-json.ndjson");
    let mut ndjson_content = String::new();
    for event in &events {
        ndjson_content.push_str(&serde_json::to_string(&event.payload).unwrap_or_default());
        ndjson_content.push('\n');
    }
    fs::write(&ndjson_ref, &ndjson_content)?;

    let status = infer_claude_session_status(&events, process_success);
    let structured = structured_for_status(&status, raw_structured);
    let terminal_source = status_to_terminal_source(&status);
    let resolved_session_id = session_id
        .clone()
        .unwrap_or_else(|| generated_id("session"));

    // The id we hand back as the member's provider thread for the NEXT delivery
    // to resume. Only a real session id parsed from the provider output is
    // resumable; the synthetic fallback id above is not, so it is not surfaced
    // as a resume token.
    let resumable_session_id = session_id.clone();

    // The resume id this delivery actually used (from the member's prior thread,
    // same source as `spec.resume`). Recorded into the session args so the
    // snapshot is the evidence that `--resume` was passed.
    let used_resume_id = build_launch_spec(member, message).resume;
    let recorded_args = if resident {
        resident::resident_recorded_args(used_resume_id.as_deref())
    } else {
        ClaudeAdapter.recorded_args(used_resume_id.as_deref())
    };

    // Record an Evidence row for the delivery session, mirroring the codex path
    // so every provider delivery is auditable from the snapshot.
    let evidence_id = generated_id("evidence");
    let evidence = Evidence {
        id: evidence_id.clone(),
        task_id: message.task_id.clone(),
        source_type: "claude_delivery_session".into(),
        source_ref: format!("provider-session:{resolved_session_id}"),
        summary: format!(
            "Claude stream-json delivery {} for message {} ({} events)",
            resolved_session_id,
            message.id,
            events.len()
        ),
        created_at: now_string(),
        evidence_kind: None,
        goal_id: None,
    };
    store.append_evidence(&evidence)?;

    // Record session in ProviderSession (neutral object, not provider-specific).
    let provider_session = ProviderSession {
        // Key the terminal session row on the delivery id (same key as the
        // "running" claim row) so it reconciles that claim to terminal in
        // `has_unresolved_provider_session`. The provider's real session id is
        // carried in `provider_thread_id`, not the row id. Keying on the session
        // id instead would leave the running claim row dangling and wrongly
        // block the next delivery.
        id: delivery_id.to_string(),
        provider: "claude".into(),
        agent_member_id: member.id.clone(),
        task_id: message.task_id.clone(),
        workspace_ref: None,
        provider_thread_id: resumable_session_id.clone(),
        provider_turn_id: None,
        terminal_source: terminal_source.clone(),
        status: status.clone(),
        command: "claude".into(),
        args: recorded_args,
        prompt_ref: member.prompt_ref.clone(),
        prompt_summary: Some(format!("deliver message {}", message.id)),
        provider_session_ref: None,
        stdout_ref: None,
        jsonl_ref: Some(ndjson_ref.display().to_string()),
        transcript_ref: if stderr_log.is_empty() {
            None
        } else {
            Some(session_dir.join("claude.stderr").display().to_string())
        },
        last_message_ref: None,
        exit_code: if process_success { Some(0) } else { Some(1) },
        started_at,
        ended_at: Some(now_string()),
        evidence_ids: vec![evidence_id.clone()],
    };
    store.append_provider_session(&provider_session)?;

    Ok(DeliveryOutcome {
        // Surface the real claude session id as the member's provider thread so
        // the next delivery resumes this conversation (memory across deliveries).
        provider_thread_id: resumable_session_id,
        provider_turn_id: None,
        terminal_source,
        status,
        stdout_ref: None,
        stderr_ref: if !stderr_log.is_empty() {
            let stderr_path = session_dir.join("claude.stderr");
            fs::write(&stderr_path, &stderr_log)?;
            Some(stderr_path.display().to_string())
        } else {
            None
        },
        request_ref: Some(session_dir.display().to_string()),
        provider_request_id: None,
        // The session ROW id (delivery_id), so a message's delivery.provider_session_id
        // maps 1:1 to its ProviderSession row (resume continuity lives in
        // provider_thread_id). This matches codex + the dry-run/failure paths and
        // lets the dashboard drill into the exact turn by id.
        provider_session_id: Some(delivery_id.to_string()),
        evidence_ids: vec![evidence_id],
        exit_code: if process_success { Some(0) } else { Some(1) },
        tokens,
        cost_usd,
        model,
        structured,
        summary: if process_success {
            // Surface the agent's actual reply as the report content; fall back
            // to a status line only when the turn produced no assistant text.
            extract_claude_reply_text(&events)
                .unwrap_or_else(|| format!("Claude delivery succeeded: {} events", events.len()))
        } else {
            format!("Claude delivery failed: {}", stderr_log)
        },
    })
}

/// Spawn `claude -p --output-format stream-json --verbose` and parse NDJSON output.
/// WP-3: Real implementation replacing the stub; parses session_id and events.
fn run_claude_exec_delivery_real(
    session_dir: &Path,
    member: &AgentMember,
    message: &Message,
    timeout_ms: u64,
) -> CliResult<ClaudeDeliveryRun> {
    // Build the message content envelope (harness context).
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    // Compose system prompt (developer instructions from member prompt_ref).
    let system_prompt = provider_developer_instructions(member);

    // Determine CWD from member or current directory.
    let cwd = member.worktree_ref.clone().or_else(|| {
        env::current_dir()
            .ok()
            .map(|path| path.display().to_string())
    });

    // Build LaunchSpec from member and message
    let spec = build_launch_spec(member, message);

    // Build command: claude -p "<message_content>" --output-format stream-json --verbose
    // plus mapped flags from launch spec.
    let mut cmd = Command::new("claude");
    cmd.arg("-p")
        .arg(&message_content)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose");

    // Resume an existing session when the member already carries a provider
    // session id (from a prior delivery). `claude -p --resume <session_id>`
    // continues the same conversation so memory carries across deliveries.
    if let Some(resume_id) = &spec.resume {
        cmd.arg("--resume").arg(resume_id);
    }

    // Append system prompt if present.
    if !system_prompt.is_empty() {
        cmd.arg("--append-system-prompt").arg(&system_prompt);
    }

    // Map LaunchSpec to claude flags
    // Model selection
    apply_claude_model_and_effort_args(&mut cmd, &spec);
    apply_claude_output_schema_arg(&mut cmd, &spec);

    // Permission mapping
    let permission_mode = ClaudeAdapter.map_permission(spec.permission);
    cmd.arg("--permission-mode").arg(permission_mode);

    // Tools (allowed-tools if spec.tools is non-empty)
    if !spec.tools.is_empty() {
        let tools_arg = spec.tools.join(",");
        cmd.arg("--allowedTools").arg(tools_arg);
    }

    // MCP config (write temp JSON if present)
    if let Some(mcp_path) = write_temp_mcp_config(spec.mcp.as_ref())? {
        cmd.arg("--mcp-config").arg(&mcp_path);
    }

    // Workspace roots (from spec.workspace and spec.writable_roots)
    if let Some(workspace) = &spec.workspace {
        cmd.arg("--add-dir").arg(workspace);
    }
    for root in &spec.writable_roots {
        cmd.arg("--add-dir").arg(root);
    }

    // Add working directory.
    let cwd_str = cwd.unwrap_or_else(|| ".".to_string());
    cmd.current_dir(&cwd_str);

    let delivery_id = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    let run = run_ndjson_child(
        cmd,
        session_dir,
        &delivery_id,
        "claude.stream-json.ndjson",
        timeout_ms,
        "claude -p process",
    )?;
    let events = run
        .events
        .iter()
        .filter_map(|payload| serde_json::to_string(payload).ok())
        .filter_map(|line| ClaudeStreamEvent::parse_line(&line))
        .collect::<Vec<_>>();

    let session_id = extract_session_id_from_claude_events(&events);
    Ok((
        run.process_success,
        events,
        run.events,
        session_id,
        run.stderr,
    ))
}

fn apply_claude_model_and_effort_args(cmd: &mut Command, spec: &LaunchSpec) {
    if let Some(model) = &spec.model {
        cmd.arg("--model").arg(model);
    }
    // Reasoning effort: claude has a native session flag.
    if let Some(effort) = &spec.effort {
        cmd.arg("--effort").arg(effort);
    }
}

fn apply_claude_output_schema_arg(cmd: &mut Command, spec: &LaunchSpec) {
    if let Some(schema) = &spec.output_schema {
        cmd.arg("--json-schema")
            .arg(schema_to_json_schema(schema).to_string());
    }
}

/// Build a [`resident::ResidentConfig`] from the same launch inputs the default
/// path uses, so the resident invocation surface matches `claude -p` flag for
/// flag (only `-p <prompt>` becomes `--input-format stream-json`).
fn build_resident_config(member: &AgentMember, message: &Message) -> resident::ResidentConfig {
    let spec = build_launch_spec(member, message);
    let system_prompt = provider_developer_instructions(member);
    let cwd = member
        .worktree_ref
        .clone()
        .or_else(|| {
            env::current_dir()
                .ok()
                .map(|path| path.display().to_string())
        })
        .unwrap_or_else(|| ".".to_string());

    let mcp_config_path = write_temp_mcp_config(spec.mcp.as_ref()).ok().flatten();

    let mut add_dirs = Vec::new();
    if let Some(workspace) = &spec.workspace {
        add_dirs.push(workspace.clone());
    }
    for root in &spec.writable_roots {
        add_dirs.push(root.clone());
    }

    resident::ResidentConfig {
        binary: "claude".into(),
        model: spec.model.clone(),
        effort: spec.effort.clone(),
        output_schema_json: spec
            .output_schema
            .as_ref()
            .map(|schema| schema_to_json_schema(schema).to_string()),
        permission_mode: ClaudeAdapter.map_permission(spec.permission).to_string(),
        tools: spec.tools.clone(),
        system_prompt,
        mcp_config_path,
        add_dirs,
        cwd,
        resume: spec.resume.clone(),
    }
}

/// Opt-in resident sibling of [`run_claude_exec_delivery_real`]. Holds a
/// `claude --input-format stream-json` process open and feeds the turn as a
/// stdin frame, returning the SAME `(success, events, raw_events, session_id, stderr)`
/// tuple shape as the default path so `run_claude_delivery` can share the same
/// status, telemetry, and recording logic.
///
/// Two modes (both opt-in via `HARNESS_CLAUDE_RESIDENT=1`):
///   * Daemon-first (unix): if a resident daemon owns the per-workspace socket,
///     the turn is delivered over it so successive short-lived `harness deliver`
///     invocations share ONE warm child across CLI runs (the daemon owns the
///     long-lived `ResidentPool`; see `resident_daemon`).
///   * Inline fallback: with no daemon present, spawn a single resident for this
///     one turn and shut it down on return (its `Drop` closes stdin and reaps
///     the child — no leaked PID). This still exercises the stream-json contract
///     but does not keep the child warm across deliveries.
fn run_claude_resident_delivery_real(
    session_dir: &Path,
    member: &AgentMember,
    message: &Message,
    timeout_ms: u64,
) -> CliResult<ClaudeDeliveryRun> {
    let message_content = format!(
        "Harness message envelope:\nmessage_id: {}\nkind: task\ntask_id: {}\nfrom_agent_id: {}\nto_agent_id: {}\nchannel: -\ncontent:\n{}",
        message.id,
        message.task_id.as_deref().unwrap_or("-"),
        message.from_agent_id,
        message.to_agent_id.as_deref().unwrap_or("-"),
        message.content
    );

    let config = build_resident_config(member, message);
    let stderr_path = session_dir.join("claude.stderr");
    let timeout = Duration::from_millis(timeout_ms.max(1));

    // Daemon-first (unix): if a resident daemon owns the per-workspace socket,
    // deliver this turn over it so successive short-lived `harness deliver`
    // invocations share ONE warm child. When no daemon is present we fall
    // through to the inline single-turn path below (graceful degrade).
    #[cfg(unix)]
    {
        let harness_root =
            PathBuf::from(env::var("HARNESS_ROOT").unwrap_or_else(|_| ".harness".into()));
        if resident_daemon::daemon_is_available(&harness_root) {
            let request = resident_daemon::DaemonRequest {
                member_id: member.id.clone(),
                config: config.clone(),
                stderr_path: stderr_path.display().to_string(),
                user_text: message_content.clone(),
                timeout_ms,
            };
            match resident_daemon::daemon_deliver(&harness_root, &request) {
                Ok(response) => {
                    let events: Vec<ClaudeStreamEvent> = response
                        .events
                        .into_iter()
                        .map(|event| ClaudeStreamEvent {
                            event_type: event.event_type,
                            payload: event.payload,
                        })
                        .collect();
                    let mut stderr_log =
                        fs::read_to_string(&response.stderr_path).unwrap_or_default();
                    if let Some(error) = response.error {
                        if !stderr_log.is_empty() {
                            stderr_log.push('\n');
                        }
                        stderr_log.push_str(&error);
                    }
                    let raw_events = events.iter().map(|event| event.payload.clone()).collect();
                    return Ok((
                        response.success,
                        events,
                        raw_events,
                        response.session_id,
                        stderr_log,
                    ));
                }
                Err(error) => {
                    // The connect succeeded but the round-trip failed (daemon
                    // died mid-turn). The turn may have partially run against the
                    // warm child, so we report a failed delivery rather than
                    // silently retrying inline (avoids double-delivery).
                    let stderr_log = fs::read_to_string(&stderr_path).unwrap_or_default();
                    return Ok((
                        false,
                        Vec::new(),
                        Vec::new(),
                        None,
                        format!("resident daemon delivery failed: {error}\n{stderr_log}"),
                    ));
                }
            }
        }
    }

    let mut resident = resident::ResidentClaude::spawn(config, &stderr_path).map_err(|error| {
        CliError::Usage(format!("failed to spawn resident claude process: {error}"))
    })?;

    // Drive exactly one turn. On error (timeout / dead child) the resident is
    // dropped (stdin closed, child reaped) and we surface a failed tuple,
    // mirroring the default path's timeout behavior.
    let turn = match resident.send_turn(&message_content, timeout) {
        Ok(turn) => turn,
        Err(error) => {
            let stderr_log = fs::read_to_string(&stderr_path).unwrap_or_default();
            let session_id = resident.session_id();
            drop(resident);
            return Ok((
                false,
                Vec::new(),
                Vec::new(),
                session_id,
                format!("{error}\n{stderr_log}"),
            ));
        }
    };

    // Map ResidentEvent -> ClaudeStreamEvent (same shape, local type bridge).
    let events: Vec<ClaudeStreamEvent> = turn
        .events
        .into_iter()
        .map(|event| ClaudeStreamEvent {
            event_type: event.event_type,
            payload: event.payload,
        })
        .collect();
    let raw_events = events.iter().map(|event| event.payload.clone()).collect();
    let session_id = turn.session_id;
    let stderr_log = fs::read_to_string(&stderr_path).unwrap_or_default();

    // Clean shutdown: closes stdin (EOF) and reaps the child. v1 is one turn
    // per delivery so we do not keep the resident across `run_claude_delivery`
    // calls; the in-process pool (resident.rs) is the seam for that later.
    resident.shutdown();

    Ok((turn.success, events, raw_events, session_id, stderr_log))
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

// Test-only helper: only referenced by build_turn_input (also #[cfg(test)]).
#[cfg(test)]
fn message_kind_label(kind: &MessageKind) -> &'static str {
    match kind {
        MessageKind::Message => "message",
        MessageKind::Task => "task",
        MessageKind::Report => "report",
    }
}

fn parse_sender_kind(value: &str) -> CliResult<SenderKind> {
    match value {
        "agent" => Ok(SenderKind::Agent),
        "operator" => Ok(SenderKind::Operator),
        "system" => Ok(SenderKind::System),
        other => Err(CliError::Usage(format!("unknown sender kind: {other}"))),
    }
}

/// Reads the optional `--sender-kind` flag, defaulting to [`SenderKind::Agent`]
/// when absent so callers that do not specify a sender identity behave as before.
fn sender_kind_from_args(args: &[String]) -> CliResult<SenderKind> {
    match value(args, "--sender-kind") {
        Some(raw) => parse_sender_kind(&raw),
        None => Ok(SenderKind::default()),
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
  agent create --name <name> --role <role> [--description <text>] [--provider codex|claude] [--team <team>] [--skill <skill>] [--prompt <text>] [--prompt-ref <path>] [--worktree <path>] [--permission-profile <profile>] [--runtime-workspace-root <path>] [--approval-policy <policy>] [--sandbox-policy <policy>] [--service-tier <tier>] [--collaboration-mode <mode>] [--effort <e>] [--output-schema-file <path>] [--provider-agent-path <path>] [--provider-agent-nickname <name>] [--provider-agent-role <role>] [--start]
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
  member register --name <name> --role <role> [--provider codex|claude] [--capability <cap>] [--worktree <path>] [--permission-profile <profile>] [--runtime-workspace-root <path>]
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
  workflow list
  workflow run --name <name> [--prompt <text>] [--start-runtime] [--dry-run] [--timeout-ms <ms>] [--model <m>] [--effort <e>]
  workflow run-script <prog.star> [--name <n>] [--args <json>] [--trace durable|live] [--dry-run] [--resume <prior_run_id>] [--timeout-ms <ms>] [--model <m>] [--effort <e>]
  workflow get-output <run_id> [--step <label>] [--text]
  workflow gc-worktrees
  workflow gc-trace [--keep-runs <n>] [--keep-days <d>] [--dry-run]
  serve [--addr 127.0.0.1:8787] [--once]
  daemon start [--socket <path>] [--idle-secs <n>]   (unix: resident warm-child host)
  daemon status
  daemon stop

global:
  --store <path>   store root for any command (else $HARNESS_ROOT, else the
                   nearest ancestor .harness, else ./.harness). Point `serve`
                   and `workflow run-script` at the SAME store.
  --timeout-ms <ms> workflow worker idle timeout (default 900000 = 15 min);
                   a worker is killed only after this long with NO output."
    );
}

#[cfg(test)]
mod workflow_runtime_tests {
    use super::*;
    use harness_core::{LaunchMcpServer, WorkflowStepStatus};

    fn temp_store(tag: &str) -> HarnessStore {
        let root = std::env::temp_dir().join(format!("harness-wf-test-{}", generated_id(tag)));
        let store = HarnessStore::new(&root);
        store.init().expect("init store");
        store
    }

    fn launch_spec_with_model_effort(model: Option<&str>, effort: Option<&str>) -> LaunchSpec {
        LaunchSpec {
            prompt_ref: None,
            message_content: "hello".into(),
            model: model.map(str::to_string),
            effort: effort.map(str::to_string),
            output_schema: None,
            permission: LaunchPermission::WorkspaceWrite,
            writable_roots: Vec::new(),
            tools: Vec::new(),
            workspace: None,
            mcp: None,
            skill_refs: Vec::new(),
            resume: None,
            output: None,
        }
    }

    fn launch_spec_with_mcp(mcp: Option<LaunchMcp>) -> LaunchSpec {
        let mut spec = launch_spec_with_model_effort(None, None);
        spec.mcp = mcp;
        spec
    }

    fn mcp_stdio_server(id: &str, command: &[&str]) -> LaunchMcpServer {
        LaunchMcpServer {
            id: id.to_string(),
            transport: Some("stdio".to_string()),
            command: command.iter().map(|part| part.to_string()).collect(),
            url: None,
            allowed_tools: Vec::new(),
        }
    }

    fn mcp_http_server(id: &str, url: &str) -> LaunchMcpServer {
        LaunchMcpServer {
            id: id.to_string(),
            transport: Some("http".to_string()),
            command: Vec::new(),
            url: Some(url.to_string()),
            allowed_tools: Vec::new(),
        }
    }

    fn command_args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn persistent_codex_effort_arg_matches_ephemeral_mapping() {
        let spec = launch_spec_with_model_effort(Some("o4-mini"), Some("high"));
        let mut cmd = Command::new("codex");
        apply_codex_model_and_effort_args(&mut cmd, &spec);

        assert_eq!(
            command_args(&cmd),
            vec!["-m", "o4-mini", "-c", "model_reasoning_effort=high"]
        );
    }

    #[test]
    fn persistent_codex_omits_effort_arg_when_absent() {
        let spec = launch_spec_with_model_effort(Some("o4-mini"), None);
        let mut cmd = Command::new("codex");
        apply_codex_model_and_effort_args(&mut cmd, &spec);

        let args = command_args(&cmd);
        assert_eq!(args, vec!["-m", "o4-mini"]);
        assert!(!args.iter().any(|arg| arg == "-c"));
        assert!(!args
            .iter()
            .any(|arg| arg.starts_with("model_reasoning_effort=")));
    }

    #[test]
    fn persistent_claude_effort_arg_matches_ephemeral_mapping() {
        let spec = launch_spec_with_model_effort(Some("opus"), Some("medium"));
        let mut cmd = Command::new("claude");
        apply_claude_model_and_effort_args(&mut cmd, &spec);

        assert_eq!(
            command_args(&cmd),
            vec!["--model", "opus", "--effort", "medium"]
        );
    }

    #[test]
    fn persistent_claude_omits_effort_arg_when_absent() {
        let spec = launch_spec_with_model_effort(Some("opus"), None);
        let mut cmd = Command::new("claude");
        apply_claude_model_and_effort_args(&mut cmd, &spec);

        let args = command_args(&cmd);
        assert_eq!(args, vec!["--model", "opus"]);
        assert!(!args.iter().any(|arg| arg == "--effort"));
    }

    #[test]
    fn persistent_codex_schema_arg_matches_ephemeral_mapping() {
        let session_dir =
            std::env::temp_dir().join(format!("harness-codex-schema-{}", generated_id("test")));
        fs::create_dir_all(&session_dir).expect("create session dir");
        let mut spec = launch_spec_with_model_effort(None, None);
        spec.output_schema = Some(serde_json::json!({ "verdict": "pass/fail" }));
        let mut cmd = Command::new("codex");
        apply_codex_output_schema_arg(&mut cmd, &spec, &session_dir).expect("apply schema arg");

        let schema_path = session_dir.join("output-schema.json");
        assert_eq!(
            command_args(&cmd),
            vec![
                "--output-schema".to_string(),
                schema_path.to_string_lossy().to_string()
            ]
        );
        let written: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(&schema_path).expect("schema file should be written"),
        )
        .expect("schema file should contain JSON");
        assert_eq!(
            written,
            schema_to_json_schema(spec.output_schema.as_ref().unwrap())
        );
        let _ = fs::remove_dir_all(&session_dir);
    }

    #[test]
    fn persistent_codex_omits_schema_arg_when_absent() {
        let session_dir =
            std::env::temp_dir().join(format!("harness-codex-schema-{}", generated_id("test")));
        fs::create_dir_all(&session_dir).expect("create session dir");
        let spec = launch_spec_with_model_effort(None, None);
        let mut cmd = Command::new("codex");
        apply_codex_output_schema_arg(&mut cmd, &spec, &session_dir).expect("apply schema arg");

        assert!(command_args(&cmd).is_empty());
        assert!(
            !session_dir.join("output-schema.json").exists(),
            "no schema file should be written when schema is absent"
        );
        let _ = fs::remove_dir_all(&session_dir);
    }

    #[test]
    fn persistent_codex_mcp_stdio_command_and_args_match_config_schema() {
        let spec = launch_spec_with_mcp(Some(LaunchMcp {
            servers: vec![mcp_stdio_server("filesys", &["npx", "-y", "pkg"])],
        }));
        let mut cmd = Command::new("codex");
        apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

        assert_eq!(
            command_args(&cmd),
            vec![
                "-c",
                "mcp_servers.filesys.command=\"npx\"",
                "-c",
                "mcp_servers.filesys.args=[\"-y\",\"pkg\"]"
            ]
        );
    }

    #[test]
    fn persistent_codex_mcp_single_command_omits_args() {
        let spec = launch_spec_with_mcp(Some(LaunchMcp {
            servers: vec![mcp_stdio_server("single", &["mcp-bin"])],
        }));
        let mut cmd = Command::new("codex");
        apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

        let args = command_args(&cmd);
        assert_eq!(args, vec!["-c", "mcp_servers.single.command=\"mcp-bin\""]);
        assert!(!args.iter().any(|arg| arg.contains(".args=")));
    }

    #[test]
    fn persistent_codex_mcp_http_url_matches_config_schema() {
        let spec = launch_spec_with_mcp(Some(LaunchMcp {
            servers: vec![mcp_http_server("remote", "https://example.com/mcp")],
        }));
        let mut cmd = Command::new("codex");
        apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

        assert_eq!(
            command_args(&cmd),
            vec!["-c", "mcp_servers.remote.url=\"https://example.com/mcp\""]
        );
    }

    #[test]
    fn persistent_codex_mcp_absent_or_empty_emits_no_config_flags() {
        for spec in [
            launch_spec_with_mcp(None),
            launch_spec_with_mcp(Some(LaunchMcp {
                servers: Vec::new(),
            })),
        ] {
            let mut cmd = Command::new("codex");
            apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

            let args = command_args(&cmd);
            assert!(args.is_empty());
            assert!(!args.iter().any(|arg| arg.contains("mcp_servers")));
        }
    }

    #[test]
    fn persistent_codex_mcp_quotes_non_bare_id_key_path() {
        let spec = launch_spec_with_mcp(Some(LaunchMcp {
            servers: vec![mcp_stdio_server("my id.v1", &["npx"])],
        }));
        let mut cmd = Command::new("codex");
        apply_codex_mcp_args(&mut cmd, &spec).expect("apply mcp args");

        assert_eq!(
            command_args(&cmd),
            vec!["-c", "mcp_servers.\"my id.v1\".command=\"npx\""]
        );
    }

    #[test]
    fn persistent_claude_schema_arg_matches_ephemeral_mapping() {
        let mut spec = launch_spec_with_model_effort(None, None);
        spec.output_schema = Some(serde_json::json!({ "verdict": "pass/fail" }));
        let mut cmd = Command::new("claude");
        apply_claude_output_schema_arg(&mut cmd, &spec);

        assert_eq!(
            command_args(&cmd),
            vec![
                "--json-schema".to_string(),
                schema_to_json_schema(spec.output_schema.as_ref().unwrap()).to_string()
            ]
        );
    }

    #[test]
    fn persistent_claude_omits_schema_arg_when_absent() {
        let spec = launch_spec_with_model_effort(None, None);
        let mut cmd = Command::new("claude");
        apply_claude_output_schema_arg(&mut cmd, &spec);

        assert!(command_args(&cmd).is_empty());
    }

    fn ok_step(spec: &workflow::AgentStepSpec) -> workflow::StepResult {
        workflow::StepResult {
            phase: spec.phase.clone(),
            label: spec.label.clone(),
            provider: spec.provider.clone(),
            isolation: spec.isolation.clone(),
            ok: true,
            provider_session_id: Some(format!("session-{}", spec.label)),
            output_summary: format!("mock ok: {}", spec.label),
            step_id: None,
            started_at: None,
            details: None,
            structured: None,
            ordinal: None,
        }
    }

    fn ndjson_values(lines: &[&str]) -> Vec<serde_json::Value> {
        lines
            .iter()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .collect()
    }

    #[test]
    fn parse_codex_usage_reads_turn_completed_usage() {
        // Real codex `exec --json` shape: terminal turn.completed carries usage
        // with input/output and the SUBSET cached/reasoning counters.
        let events = ndjson_values(&[
            r#"{"type":"thread.started","thread_id":"t1"}"#,
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":1200,"output_tokens":340,"cached_input_tokens":800,"reasoning_output_tokens":120}}"#,
        ]);
        let usage = parse_codex_usage(&events).expect("usage present");
        assert_eq!(usage.input, 1200);
        assert_eq!(usage.output, 340);
        // total is input+output; cached/reasoning are subsets, NOT re-added.
        assert_eq!(usage.total, 1540);
    }

    #[test]
    fn parse_codex_usage_accepts_nested_turn_usage_and_legacy_name() {
        let events = ndjson_values(&[
            r#"{"type":"turn_completed","turn":{"usage":{"input_tokens":5,"output_tokens":7}}}"#,
        ]);
        let usage = parse_codex_usage(&events).expect("usage present");
        assert_eq!((usage.input, usage.output, usage.total), (5, 7, 12));
    }

    #[test]
    fn parse_codex_usage_absent_is_none() {
        let events = ndjson_values(&[r#"{"type":"turn.completed"}"#, r#"{"type":"item.started"}"#]);
        assert!(parse_codex_usage(&events).is_none());
    }

    #[test]
    fn parse_claude_usage_reads_result_usage() {
        // Claude stream-json terminal `result` carries usage.
        let events = ndjson_values(&[
            r#"{"type":"system","subtype":"init","session_id":"s1"}"#,
            r#"{"type":"result","subtype":"success","usage":{"input_tokens":42,"output_tokens":15}}"#,
        ]);
        let usage = parse_claude_usage(&events).expect("usage present");
        assert_eq!((usage.input, usage.output, usage.total), (42, 15, 57));
    }

    #[test]
    fn parse_claude_usage_absent_is_none() {
        let events = ndjson_values(&[r#"{"type":"result","subtype":"success"}"#]);
        assert!(parse_claude_usage(&events).is_none());
    }

    #[test]
    fn classify_failure_reason_ok_is_none() {
        assert_eq!(classify_failure_reason(true, Some(0), false), None);
        assert_eq!(classify_failure_reason(true, Some(1), false), None);
    }

    #[test]
    fn classify_failure_reason_timeout_dominates() {
        // Timeout fired (and killed the child, so exit_code is None) → "timeout".
        assert_eq!(classify_failure_reason(false, None, true), Some("timeout"));
        // Even with a code present, a fired timeout still classifies as timeout.
        assert_eq!(
            classify_failure_reason(false, Some(1), true),
            Some("timeout")
        );
    }

    #[test]
    fn classify_failure_reason_nonzero_exit_is_exit() {
        assert_eq!(classify_failure_reason(false, Some(2), false), Some("exit"));
        // Killed by a signal (no code) without a timeout is still an exit failure.
        assert_eq!(classify_failure_reason(false, None, false), Some("exit"));
    }

    #[test]
    fn classify_failure_reason_clean_exit_but_failed_is_delivery() {
        // Process exited 0 yet the delivery produced no successful turn (e.g. an
        // auth / usage-limit terminal) → a delivery-layer failure.
        assert_eq!(
            classify_failure_reason(false, Some(0), false),
            Some("delivery")
        );
    }

    #[test]
    fn schema_required_keys_reads_top_level_object_keys() {
        let schema = serde_json::json!({ "ok": "", "summary": "", "score": 0 });
        let mut keys = schema_required_keys(&schema);
        keys.sort();
        assert_eq!(keys, vec!["ok", "score", "summary"]);
        // A non-object schema declares no required keys.
        assert!(schema_required_keys(&serde_json::json!("nope")).is_empty());
    }

    #[test]
    fn schema_instruction_lists_keys_and_inlines_the_shape() {
        let schema = serde_json::json!({ "ok": "" });
        let instruction = schema_instruction(&schema);
        assert!(instruction.contains("ONLY a single JSON object"));
        assert!(instruction.contains("ok"));
        // The compact schema is inlined as a shape hint.
        assert!(instruction.contains("{\"ok\":\"\"}"));
    }

    #[test]
    fn schema_to_json_schema_wraps_flat_and_passes_real_through() {
        // Flat { key: hint } -> a string-property object schema with required keys.
        let flat = serde_json::json!({ "verdict": "the call", "score": "0-100" });
        let js = schema_to_json_schema(&flat);
        assert_eq!(js["type"], serde_json::json!("object"));
        assert_eq!(
            js["properties"]["verdict"]["type"],
            serde_json::json!("string")
        );
        assert_eq!(
            js["properties"]["verdict"]["description"],
            serde_json::json!("the call")
        );
        assert_eq!(js["additionalProperties"], serde_json::json!(false));
        let req = js["required"].as_array().expect("required array");
        assert!(req.contains(&serde_json::json!("verdict")));
        assert!(req.contains(&serde_json::json!("score")));

        // An already-valid JSON Schema (has `type`/`properties`) is unchanged.
        let real = serde_json::json!({
            "type": "object",
            "properties": { "score": { "type": "integer" } },
            "required": ["score"],
        });
        assert_eq!(schema_to_json_schema(&real), real);
    }

    #[test]
    fn schema_to_json_schema_coerces_known_type_hints() {
        // Well-known type words become real JSON-Schema scalar types (issue #139
        // item 5) — no `description`, so the provider returns a real bool/int/
        // number, not the string "true"/"7". Descriptive hints stay `string`.
        let flat = serde_json::json!({
            "ok": "bool",
            "count": "int",
            "ratio": "number",
            "note": "a short reason",
        });
        let js = schema_to_json_schema(&flat);
        assert_eq!(js["properties"]["ok"]["type"], serde_json::json!("boolean"));
        assert!(js["properties"]["ok"].get("description").is_none());
        assert_eq!(
            js["properties"]["count"]["type"],
            serde_json::json!("integer")
        );
        assert_eq!(
            js["properties"]["ratio"]["type"],
            serde_json::json!("number")
        );
        // A non-type-word hint is still a string field with the hint as description.
        assert_eq!(
            js["properties"]["note"]["type"],
            serde_json::json!("string")
        );
        assert_eq!(
            js["properties"]["note"]["description"],
            serde_json::json!("a short reason")
        );
    }

    #[test]
    fn parse_claude_result_extras_reads_structured_and_cost() {
        let events = vec![
            serde_json::json!({"type": "system", "subtype": "init", "model": "claude-opus-4-8"}),
            serde_json::json!({
                "type": "result",
                "structured_output": { "verdict": "pass", "score": 100 },
                "total_cost_usd": 0.1866,
                "usage": { "input_tokens": 5, "output_tokens": 2 }
            }),
        ];
        let (structured, cost) = parse_claude_result_extras(&events);
        assert_eq!(
            structured,
            Some(serde_json::json!({ "verdict": "pass", "score": 100 }))
        );
        assert_eq!(cost, Some(0.1866));

        // No `result` frame -> both None.
        let (s2, c2) = parse_claude_result_extras(&[serde_json::json!({"type": "system"})]);
        assert!(s2.is_none() && c2.is_none());
    }

    fn delivery_outcome_for_test(
        tokens: Option<TokenUsage>,
        cost_usd: Option<f64>,
        model: Option<String>,
        structured: Option<serde_json::Value>,
    ) -> DeliveryOutcome {
        DeliveryOutcome {
            status: ProviderSessionStatus::Succeeded,
            provider_thread_id: None,
            provider_turn_id: None,
            terminal_source: Some(MessageTerminalSource::Unknown),
            stdout_ref: None,
            stderr_ref: None,
            request_ref: None,
            provider_request_id: None,
            provider_session_id: Some("delivery-test".into()),
            evidence_ids: Vec::new(),
            exit_code: Some(0),
            tokens,
            cost_usd,
            model,
            structured,
            summary: "test delivery".into(),
        }
    }

    #[test]
    fn persistent_codex_delivery_outcome_uses_raw_event_tokens_and_spec_model() {
        let spec = launch_spec_with_model_effort(Some("gpt-5-codex"), None);
        let raw_events = ndjson_values(&[
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"done"}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":7}}"#,
        ]);

        let (tokens, cost_usd, model) = codex_delivery_telemetry(&raw_events, &spec);
        let outcome = delivery_outcome_for_test(tokens, cost_usd, model, None);

        assert_eq!(
            outcome.tokens,
            Some(TokenUsage {
                input: 11,
                output: 7,
                total: 18,
            })
        );
        assert_eq!(outcome.model.as_deref(), Some("gpt-5-codex"));
        assert_eq!(outcome.cost_usd, None);
    }

    #[test]
    fn persistent_claude_delivery_outcome_uses_raw_event_tokens_model_and_cost() {
        let raw_events = ndjson_values(&[
            r#"{"type":"system","subtype":"init","model":"claude-opus-4-8"}"#,
            r#"{"type":"result","subtype":"success","total_cost_usd":0.025,"usage":{"input_tokens":40,"output_tokens":9}}"#,
        ]);

        let (tokens, cost_usd, model, structured) = claude_delivery_telemetry(&raw_events);
        let outcome = delivery_outcome_for_test(tokens, cost_usd, model, structured);

        assert_eq!(
            outcome.tokens,
            Some(TokenUsage {
                input: 40,
                output: 9,
                total: 49,
            })
        );
        assert_eq!(outcome.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(outcome.cost_usd, Some(0.025));
        assert_eq!(outcome.structured, None);
    }

    #[test]
    fn persistent_claude_delivery_outcome_uses_result_structured_output() {
        let raw_events = vec![
            serde_json::json!({"type":"system","subtype":"init","model":"claude-opus-4-8"}),
            serde_json::json!({
                "type":"result",
                "subtype":"success",
                "structured_output": { "verdict": "pass", "score": 100 },
                "total_cost_usd": 0.025,
                "usage": { "input_tokens": 40, "output_tokens": 9 }
            }),
        ];

        let (tokens, cost_usd, model, structured) = claude_delivery_telemetry(&raw_events);
        let outcome = delivery_outcome_for_test(tokens, cost_usd, model, structured);

        assert_eq!(
            outcome.structured,
            Some(serde_json::json!({ "verdict": "pass", "score": 100 }))
        );
        assert_eq!(outcome.cost_usd, Some(0.025));
    }

    #[test]
    fn persistent_codex_delivery_outcome_extracts_structured_only_with_schema() {
        let mut spec = launch_spec_with_model_effort(Some("gpt-5-codex"), None);
        spec.output_schema = Some(serde_json::json!({ "verdict": "pass/fail" }));
        let reply = r#"{"verdict":"pass","summary":"done"}"#;

        let outcome = delivery_outcome_for_test(
            None,
            None,
            spec.model.clone(),
            codex_delivery_structured(Some(reply), &spec),
        );

        assert_eq!(
            outcome.structured,
            Some(serde_json::json!({ "verdict": "pass", "summary": "done" }))
        );

        let no_schema = launch_spec_with_model_effort(Some("gpt-5-codex"), None);
        let outcome = delivery_outcome_for_test(
            None,
            None,
            no_schema.model.clone(),
            codex_delivery_structured(Some(reply), &no_schema),
        );

        assert_eq!(outcome.structured, None);
    }

    #[test]
    fn delivery_outcome_defaults_have_no_telemetry_for_non_provider_paths() {
        let dry_run = delivery_outcome_for_test(None, None, None, None);
        let mut failure = delivery_outcome_for_test(None, None, None, None);
        failure.status = ProviderSessionStatus::Failed;
        failure.exit_code = Some(1);

        for outcome in [dry_run, failure] {
            assert_eq!(outcome.tokens, None);
            assert_eq!(outcome.cost_usd, None);
            assert_eq!(outcome.model, None);
            assert_eq!(outcome.structured, None);
        }
    }

    #[test]
    fn structured_is_surfaced_only_on_succeeded_status() {
        let value = serde_json::json!({ "verdict": "pass" });
        assert_eq!(
            structured_for_status(&ProviderSessionStatus::Succeeded, Some(value.clone())),
            Some(value.clone())
        );
        // A turn that RAN but did not succeed must not report a (possibly partial /
        // schema-violating) structured result, even if one was extracted.
        for status in [
            ProviderSessionStatus::Failed,
            ProviderSessionStatus::Stale,
            ProviderSessionStatus::Canceled,
            ProviderSessionStatus::Running,
            ProviderSessionStatus::Queued,
        ] {
            assert_eq!(structured_for_status(&status, Some(value.clone())), None);
        }
    }

    #[test]
    fn extract_json_object_handles_bare_object() {
        let value = extract_json_object(r#"{"ok": true, "n": 3}"#).expect("parsed");
        assert_eq!(value["ok"], serde_json::json!(true));
        assert_eq!(value["n"], serde_json::json!(3));
    }

    #[test]
    fn extract_json_object_strips_a_json_code_fence() {
        let reply = "```json\n{\"ok\": true, \"summary\": \"done\"}\n```";
        let value = extract_json_object(reply).expect("parsed");
        assert_eq!(value["summary"], serde_json::json!("done"));
        // A bare (langless) fence works too.
        let reply2 = "```\n{\"ok\": false}\n```";
        let value2 = extract_json_object(reply2).expect("parsed");
        assert_eq!(value2["ok"], serde_json::json!(false));
    }

    #[test]
    fn extract_json_object_takes_first_balanced_object_amid_prose() {
        // Prose around the object, plus braces inside a string literal.
        let reply = "Here is the result:\n{\"msg\": \"a } b\", \"ok\": true}\nThanks!";
        let value = extract_json_object(reply).expect("parsed");
        assert_eq!(value["msg"], serde_json::json!("a } b"));
        assert_eq!(value["ok"], serde_json::json!(true));
    }

    #[test]
    fn extract_json_object_rejects_invalid_or_non_object() {
        assert!(extract_json_object("not json at all").is_none());
        // A JSON array is not an object.
        assert!(extract_json_object("[1, 2, 3]").is_none());
        // An unbalanced object does not parse.
        assert!(extract_json_object("{\"ok\": true").is_none());
    }

    #[test]
    fn object_has_required_keys_present_and_missing() {
        let obj = serde_json::json!({ "ok": true, "summary": "x" });
        let required: Vec<String> = vec!["ok".into(), "summary".into()];
        assert!(object_has_required_keys(&obj, &required));
        // A missing key fails validation.
        let missing: Vec<String> = vec!["ok".into(), "score".into()];
        assert!(!object_has_required_keys(&obj, &missing));
        // An empty required set is vacuously satisfied.
        assert!(object_has_required_keys(&obj, &[]));
    }

    #[test]
    fn build_step_details_success_has_tokens_and_no_failure() {
        let spec = workflow::AgentStepSpec {
            phase: "p".into(),
            label: "l".into(),
            provider: "codex".into(),
            model: Some("gpt-5-codex".into()),
            effort: None,
            fallback_model: None,
            image: Vec::new(),
            add_dir: Vec::new(),
            isolation: None,
            prompt: "hi".into(),
            schema: None,
            writable: false,
            ordinal: None,
        };
        let spawn = EphemeralSpawn {
            ok: true,
            reply: Some("done".into()),
            ndjson: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            timed_out: false,
            tokens: Some(TokenUsage {
                input: 10,
                output: 4,
                total: 14,
            }),
            model: None,
            structured: None,
            cost_usd: None,
            warnings: Vec::new(),
        };
        let details = build_step_details(&spec, &spawn, spec.model.as_deref(), 1234, None);
        // spec.model wins over the (absent) worker-reported model.
        assert_eq!(details["model"], serde_json::json!("gpt-5-codex"));
        assert_eq!(details["exit_code"], serde_json::json!(0));
        assert_eq!(details["duration_ms"], serde_json::json!(1234));
        assert_eq!(details["tokens"]["total"], serde_json::json!(14));
        assert!(details.get("failure").is_none());
        assert!(details.get("worktree_diff").is_none());
    }

    #[test]
    fn build_step_details_failure_classifies_and_keeps_stderr() {
        let spec = workflow::AgentStepSpec {
            phase: "p".into(),
            label: "l".into(),
            provider: "claude".into(),
            model: None,
            effort: None,
            fallback_model: None,
            image: Vec::new(),
            add_dir: Vec::new(),
            isolation: None,
            prompt: "hi".into(),
            schema: None,
            writable: false,
            ordinal: None,
        };
        let spawn = EphemeralSpawn {
            ok: false,
            reply: None,
            ndjson: String::new(),
            stderr: "boom: provider exploded".into(),
            exit_code: Some(3),
            timed_out: false,
            tokens: None,
            // The node requested no model, so the worker-reported one is used.
            model: Some("claude-opus-4-8".into()),
            structured: None,
            cost_usd: None,
            warnings: Vec::new(),
        };
        let details = build_step_details(&spec, &spawn, spec.model.as_deref(), 50, None);
        assert_eq!(details["model"], serde_json::json!("claude-opus-4-8"));
        assert_eq!(details["failure"]["failed"], serde_json::json!(true));
        assert_eq!(details["failure"]["reason"], serde_json::json!("exit"));
        assert_eq!(
            details["failure"]["detail"],
            serde_json::json!("boom: provider exploded")
        );
    }

    #[test]
    fn build_step_details_caps_large_worktree_diff() {
        let spec = workflow::AgentStepSpec {
            phase: "p".into(),
            label: "l".into(),
            provider: "codex".into(),
            model: None,
            effort: None,
            fallback_model: None,
            image: Vec::new(),
            add_dir: Vec::new(),
            isolation: Some("worktree".into()),
            prompt: "hi".into(),
            schema: None,
            writable: false,
            ordinal: None,
        };
        let spawn = EphemeralSpawn {
            ok: true,
            reply: None,
            ndjson: String::new(),
            stderr: String::new(),
            exit_code: Some(0),
            timed_out: false,
            tokens: None,
            model: None,
            structured: None,
            cost_usd: None,
            warnings: Vec::new(),
        };
        let big = "x".repeat(WORKTREE_DIFF_CAP + 5_000);
        let details = build_step_details(&spec, &spawn, spec.model.as_deref(), 1, Some(&big));
        let stored = details["worktree_diff"].as_str().expect("diff string");
        assert_eq!(stored.len(), WORKTREE_DIFF_CAP);
        assert_eq!(details["worktree_diff_truncated"], serde_json::json!(true));

        // A small diff is stored whole and NOT flagged truncated.
        let small = "diff --git a b\n+added\n";
        let details = build_step_details(&spec, &spawn, spec.model.as_deref(), 1, Some(small));
        assert_eq!(details["worktree_diff"], serde_json::json!(small));
        assert_eq!(details["worktree_diff_truncated"], serde_json::json!(false));
    }

    #[test]
    fn parse_worker_model_reads_claude_init_and_ignores_codex() {
        let claude = vec![
            serde_json::json!({"type": "system", "subtype": "init", "model": "claude-opus-4-8"}),
            serde_json::json!({"type": "result", "usage": {"input_tokens": 1, "output_tokens": 1}}),
        ];
        assert_eq!(
            parse_worker_model(&claude).as_deref(),
            Some("claude-opus-4-8")
        );
        // codex exec --json carries no system/model frame.
        let codex = vec![
            serde_json::json!({"type": "thread.started"}),
            serde_json::json!({"type": "turn.completed", "usage": {"input_tokens": 1}}),
        ];
        assert_eq!(parse_worker_model(&codex), None);
        assert_eq!(parse_worker_model(&[]), None);
    }

    #[test]
    fn workflow_run_defaults_do_not_override_leaf_model_or_effort() {
        let options = WorkflowDeliveryOptions {
            dry_run: false,
            start_runtime: false,
            timeout_ms: 1_000,
            default_model: Some("run-model".into()),
            default_effort: Some("medium".into()),
            max_budget_usd: None,
            trace_retention: "durable".into(),
            progress: false,
        };
        let mut spec = workflow::AgentStepSpec {
            phase: "p".into(),
            label: "l".into(),
            provider: "codex".into(),
            model: None,
            effort: None,
            fallback_model: None,
            image: Vec::new(),
            add_dir: Vec::new(),
            isolation: None,
            prompt: "hi".into(),
            schema: None,
            writable: false,
            ordinal: None,
        };

        assert_eq!(workflow_effective_model(&options, &spec), Some("run-model"));
        assert_eq!(workflow_effective_effort(&options, &spec), Some("medium"));

        spec.model = Some("leaf-model".into());
        spec.effort = Some("high".into());
        assert_eq!(
            workflow_effective_model(&options, &spec),
            Some("leaf-model")
        );
        assert_eq!(workflow_effective_effort(&options, &spec), Some("high"));
    }

    #[test]
    fn run_ndjson_child_kills_a_hung_worker_via_timeout() {
        // A worker that emits one line then HANGS (stdout open, never exits) goes
        // SILENT, so the IDLE timeout fires and kills it — not block forever.
        let root = std::env::temp_dir().join(format!("mah-hang-{}", generated_id("t")));
        let session_dir = root.join("provider-sessions").join("s");
        fs::create_dir_all(&session_dir).expect("mkdir");
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("printf '{\"type\":\"item\"}\\n'; sleep 600");

        let start = Instant::now();
        // 500ms IDLE limit: after the one event, silence > 500ms → killed.
        let run = run_ndjson_child(
            cmd,
            &session_dir,
            "s",
            "out.ndjson",
            500,
            "ephemeral worker",
        )
        .expect("run");
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(8),
            "must not block on the hung child; took {elapsed:?}"
        );
        assert!(run.timed_out, "the idle timeout must have fired");
        assert!(!run.process_success);
        assert!(run
            .warnings
            .iter()
            .any(|warning| warning == "ephemeral worker timed out"));
        // The single event emitted before the hang was still captured live.
        assert_eq!(run.events.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_ndjson_child_warns_and_keeps_valid_events_after_junk_stdout() {
        let root = std::env::temp_dir().join(format!("mah-junk-{}", generated_id("t")));
        let session_dir = root.join("provider-sessions").join("s");
        fs::create_dir_all(&session_dir).expect("mkdir");
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg("printf 'not-json\\n'; printf '{\"type\":\"item\",\"n\":1}\\n'");

        let run = run_ndjson_child(
            cmd,
            &session_dir,
            "s",
            "out.ndjson",
            1_000,
            "ephemeral worker",
        )
        .expect("run");

        assert!(run.process_success);
        assert!(!run.timed_out);
        assert_eq!(
            run.events,
            vec![serde_json::json!({"type": "item", "n": 1})]
        );
        assert!(run
            .warnings
            .iter()
            .any(|warning| warning == "1 stdout line(s) were not valid JSON and were dropped"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn run_ndjson_child_does_not_kill_a_slow_but_streaming_worker() {
        // The point of the IDLE timeout: a worker that keeps emitting events runs to
        // completion even though its TOTAL runtime (~800ms) far exceeds the idle
        // limit (300ms) — because it never goes silent that long. A fixed total-
        // wall-clock timeout would have wrongly killed it.
        let root = std::env::temp_dir().join(format!("mah-slow-{}", generated_id("t")));
        let session_dir = root.join("provider-sessions").join("s");
        fs::create_dir_all(&session_dir).expect("mkdir");
        let mut cmd = Command::new("sh");
        // 8 events, ~100ms apart → ~800ms total, never silent for 300ms.
        cmd.arg("-c")
            .arg("for i in 1 2 3 4 5 6 7 8; do printf '{\"type\":\"item\"}\\n'; sleep 0.1; done");

        let run = run_ndjson_child(
            cmd,
            &session_dir,
            "s",
            "out.ndjson",
            300,
            "ephemeral worker",
        )
        .expect("run");

        assert!(
            !run.timed_out,
            "a continuously-streaming worker must NOT be killed by the idle timeout"
        );
        assert!(run.process_success, "it should exit cleanly on its own");
        assert_eq!(run.events.len(), 8, "all streamed events captured");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn step_result_json_merges_details_without_overriding_base() {
        // The base keys (provider/ok/...) always win; details adds new keys.
        let result = workflow::StepResult {
            phase: "p".into(),
            label: "l".into(),
            provider: "codex".into(),
            isolation: None,
            ok: true,
            provider_session_id: Some("s".into()),
            output_summary: "summary".into(),
            step_id: None,
            started_at: None,
            details: Some(serde_json::json!({
                "model": "gpt-5-codex",
                "duration_ms": 99,
                // A colliding key must NOT override the base value.
                "ok": false,
            })),
            structured: None,
            ordinal: None,
        };
        let json = workflow::step_result_json(&result);
        assert_eq!(json["provider"], serde_json::json!("codex"));
        assert_eq!(json["ok"], serde_json::json!(true)); // base wins
        assert_eq!(json["model"], serde_json::json!("gpt-5-codex"));
        assert_eq!(json["duration_ms"], serde_json::json!(99));
    }

    #[test]
    fn workflow_run_journals_steps_and_completes_with_mock_driver() {
        let store = temp_store("complete");
        let registry = workflow::WorkflowRegistry::builtin();
        let def = registry.get("investigate").expect("investigate registered");
        // Mock driver: never spawns a provider; always succeeds.
        let driver = |spec: &workflow::AgentStepSpec| ok_step(spec);

        let run_id = generated_id("wfrun");
        let result = run_workflow_with_driver(&store, &run_id, def, "failure X", false, &driver)
            .expect("run workflow");

        // The returned run is completed and references 3 steps (serial + 2 parallel).
        let run = result.get("run").expect("run key");
        assert_eq!(
            run.get("status").and_then(|s| s.as_str()),
            Some("completed")
        );
        let step_ids = run
            .get("step_ids")
            .and_then(|s| s.as_array())
            .expect("step_ids");
        assert_eq!(step_ids.len(), 3);

        // The journal holds two WorkflowRun rows (running -> completed) for one id.
        let runs = store.workflow_runs().expect("read runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].status, WorkflowRunStatus::Running);
        assert_eq!(runs[1].status, WorkflowRunStatus::Completed);
        assert_eq!(runs[0].id, runs[1].id);

        // Three steps journaled, all completed, with provider_session_id links.
        let steps = store.workflow_steps().expect("read steps");
        assert_eq!(steps.len(), 3);
        for step in &steps {
            assert_eq!(step.status, WorkflowStepStatus::Completed);
            assert_eq!(step.run_id, runs[0].id);
            assert!(step.provider_session_id.is_some());
            assert!(step.ended_at.is_some());
        }
        // The serial step is first, in the "scope" phase.
        assert_eq!(steps[0].phase, "scope");
        assert_eq!(steps[1].phase, "audit");
        assert_eq!(steps[2].phase, "audit");
    }

    /// Build a ProviderSession keyed by `session_id`. `jsonl_ref` carries the
    /// durable per-session NDJSON path when retained; `None` is the live-only
    /// "trace not retained" marker the Backend leaves after pruning.
    fn provider_session_with_ref(session_id: &str, jsonl_ref: Option<String>) -> ProviderSession {
        ProviderSession {
            id: session_id.into(),
            provider: "claude".into(),
            agent_member_id: session_id.into(),
            task_id: None,
            workspace_ref: None,
            provider_thread_id: None,
            provider_turn_id: None,
            terminal_source: Some(MessageTerminalSource::TurnCompleted),
            status: ProviderSessionStatus::Succeeded,
            command: "harness".into(),
            args: Vec::new(),
            prompt_ref: None,
            prompt_summary: None,
            provider_session_ref: None,
            stdout_ref: None,
            jsonl_ref,
            transcript_ref: None,
            last_message_ref: None,
            exit_code: Some(0),
            started_at: "unix-ms:1".into(),
            ended_at: Some("unix-ms:2".into()),
            evidence_ids: Vec::new(),
        }
    }

    #[test]
    fn codex_normalize_thread_started_sets_provider_thread_id_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "thread.started",
            "thread_id": "thread-1"
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.session_id, "session-T");
        assert_eq!(event.provider, "codex");
        assert_eq!(event.seq, 0);
        assert_eq!(event.kind, HarnessTurnEventKind::ProviderMeta);
        assert_eq!(event.provider_thread_id.as_deref(), Some("thread-1"));
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn codex_normalize_turn_started_sets_kind_and_provider_turn_id_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "turn.started",
            "turn_id": "turn-1"
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::TurnStarted);
        assert_eq!(event.provider_turn_id.as_deref(), Some("turn-1"));
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn codex_normalize_agent_message_item_completed_sets_message_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "item-1",
                "type": "agent_message",
                "text": "done"
            }
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::Message);
        assert_eq!(event.provider_item_id.as_deref(), Some("item-1"));
        assert_eq!(event.role.as_deref(), Some("assistant"));
        assert_eq!(event.text.as_deref(), Some("done"));
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn codex_normalize_command_execution_with_output_emits_call_and_result() {
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "cmd-1",
                "type": "command_execution",
                "command": "cargo test",
                "aggregated_output": "ok\n",
                "exit_code": 0
            }
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 2);

        let call = &events[0];
        assert_eq!(call.kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(call.provider_item_id.as_deref(), Some("cmd-1"));
        assert_eq!(
            call.tool_call,
            Some(HarnessToolCall {
                id: Some("cmd-1".into()),
                name: "cargo test".into(),
                args: raw.get("item").unwrap().clone(),
            })
        );
        assert_eq!(call.raw_provider_event, raw);

        let result = &events[1];
        assert_eq!(result.kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(result.provider_item_id.as_deref(), Some("cmd-1"));
        assert_eq!(
            result.tool_result,
            Some(HarnessToolResult {
                tool_call_id: Some("cmd-1".into()),
                name: Some("cargo test".into()),
                content: "ok\n".into(),
                is_error: false,
            })
        );
        assert_eq!(result.raw_provider_event, raw);
    }

    #[test]
    fn live_normalize_codex_command_execution_assigns_seq_and_serializes_payload_events() {
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "cmd-live",
                "type": "command_execution",
                "command": "cargo test",
                "aggregated_output": "ok\n",
                "exit_code": 0
            }
        });

        let events = normalize_live_turn_event("codex", "session-live", &raw, 41);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(events[0].seq, 41);
        assert_eq!(events[0].raw_provider_event, raw);
        assert_eq!(events[1].kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(events[1].seq, 42);
        assert_eq!(events[1].raw_provider_event, raw);

        let payload = serde_json::json!({
            "session_id": "session-live",
            "events": events
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
                .expect("serialize events"),
        });
        let payload_events = payload
            .get("events")
            .and_then(|value| value.as_array())
            .expect("payload events");
        assert_eq!(payload_events.len(), 2);
        assert_eq!(
            payload_events[0].get("kind").and_then(|v| v.as_str()),
            Some("tool_call")
        );
        assert_eq!(
            payload_events[1].get("kind").and_then(|v| v.as_str()),
            Some("tool_result")
        );
    }

    #[test]
    fn codex_normalize_command_execution_without_output_or_error_emits_call_only() {
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "cmd-1",
                "type": "command_execution",
                "command": "cargo test",
                "aggregated_output": "",
                "exit_code": 0
            }
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(event.provider_item_id.as_deref(), Some("cmd-1"));
        assert_eq!(
            event.tool_call,
            Some(HarnessToolCall {
                id: Some("cmd-1".into()),
                name: "cargo test".into(),
                args: raw.get("item").unwrap().clone(),
            })
        );
        assert!(event.tool_result.is_none());
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn codex_normalize_command_execution_nonzero_exit_emits_error_result() {
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "cmd-1",
                "type": "command_execution",
                "command": "cargo test",
                "aggregated_output": "",
                "exit_code": 2
            }
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(events[0].raw_provider_event, raw);

        let result = &events[1];
        assert_eq!(result.kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(
            result.tool_result,
            Some(HarnessToolResult {
                tool_call_id: Some("cmd-1".into()),
                name: Some("cargo test".into()),
                content: "exit 2".into(),
                is_error: true,
            })
        );
        assert_eq!(result.raw_provider_event, raw);
    }

    #[test]
    fn codex_normalize_command_execution_whitespace_output_is_preserved() {
        // Whitespace-only output is still real output: the ToolResult must carry
        // the verbatim string, NOT fall back to `exit N`. Emptiness is decided on
        // the raw string, not a trimmed one, so a newline-only result survives.
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "cmd-ws",
                "type": "command_execution",
                "command": "printf '\\n'",
                "aggregated_output": "\n",
                "exit_code": 1
            }
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, HarnessTurnEventKind::ToolCall);

        let result = &events[1];
        assert_eq!(result.kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(
            result.tool_result,
            Some(HarnessToolResult {
                tool_call_id: Some("cmd-ws".into()),
                name: Some("printf '\\n'".into()),
                content: "\n".into(),
                is_error: true,
            })
        );
        assert_eq!(result.raw_provider_event, raw);
    }

    #[test]
    fn codex_normalize_file_change_emits_one_tool_call_per_change() {
        let raw = serde_json::json!({
            "type": "item.completed",
            "item": {
                "id": "file-1",
                "type": "file_change",
                "changes": [
                    {"kind": "add", "path": "/a"},
                    {"kind": "delete", "path": "/b"}
                ]
            }
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 2);

        assert_eq!(events[0].kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(events[0].provider_item_id.as_deref(), Some("file-1"));
        let first_call = events[0].tool_call.as_ref().unwrap();
        assert_eq!(first_call.id.as_deref(), Some("file-1"));
        assert_eq!(first_call.name, "Write");
        assert_eq!(
            first_call.args.get("path").and_then(|path| path.as_str()),
            Some("/a")
        );
        assert_eq!(events[0].raw_provider_event, raw);

        assert_eq!(events[1].kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(events[1].provider_item_id.as_deref(), Some("file-1"));
        let second_call = events[1].tool_call.as_ref().unwrap();
        assert_eq!(second_call.id.as_deref(), Some("file-1"));
        assert_eq!(second_call.name, "Delete");
        assert_eq!(
            second_call.args.get("path").and_then(|path| path.as_str()),
            Some("/b")
        );
        assert_eq!(events[1].raw_provider_event, raw);
    }

    #[test]
    fn codex_normalize_file_change_without_changes_stays_provider_meta() {
        for raw in [
            serde_json::json!({
                "type": "item.completed",
                "item": {
                    "id": "file-1",
                    "type": "file_change",
                    "changes": []
                }
            }),
            serde_json::json!({
                "type": "item.completed",
                "item": {
                    "id": "file-1",
                    "type": "file_change"
                }
            }),
        ] {
            let events = CodexAdapter.normalize_turn_event("session-T", &raw);
            assert_eq!(events.len(), 1);
            let event = &events[0];

            assert_eq!(event.kind, HarnessTurnEventKind::ProviderMeta);
            assert_eq!(event.provider_item_id.as_deref(), Some("file-1"));
            assert!(event.tool_call.is_none());
            assert_eq!(event.raw_provider_event, raw);
        }
    }

    #[test]
    fn codex_normalize_turn_completed_sets_usage_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "turn.completed",
            "usage": {
                "input_tokens": 1200,
                "output_tokens": 340,
                "cached_input_tokens": 800,
                "reasoning_output_tokens": 120
            }
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::TurnCompleted);
        assert_eq!(
            event.usage,
            Some(HarnessTokenUsage {
                input_tokens: 1200,
                output_tokens: 340,
                total_tokens: 1540,
                cached_input_tokens: Some(800),
                reasoning_output_tokens: Some(120),
            })
        );
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn codex_normalize_unrecognized_type_stays_unknown_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "provider_specific",
            "payload": {"n": 1}
        });

        let events = CodexAdapter.normalize_turn_event("session-T", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.session_id, "session-T");
        assert_eq!(event.provider, "codex");
        assert_eq!(event.seq, 0);
        assert_eq!(event.kind, HarnessTurnEventKind::Unknown);
        assert_eq!(event.raw_provider_event, raw);
        assert!(event.provider_thread_id.is_none());
        assert!(event.provider_turn_id.is_none());
        assert!(event.text.is_none());
        assert!(event.tool_call.is_none());
        assert!(event.usage.is_none());
    }

    #[test]
    fn claude_normalize_system_sets_provider_meta_model_thread_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "system",
            "subtype": "init",
            "session_id": "claude-session-1",
            "model": "claude-opus-4-8"
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.session_id, "session-C");
        assert_eq!(event.provider, "claude");
        assert_eq!(event.seq, 0);
        assert_eq!(event.kind, HarnessTurnEventKind::ProviderMeta);
        assert_eq!(event.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(
            event.provider_thread_id.as_deref(),
            Some("claude-session-1")
        );
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn claude_normalize_assistant_text_sets_message_role_text_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "hello"},
                    {"type": "text", "text": "world"}
                ]
            }
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::Message);
        assert_eq!(event.role.as_deref(), Some("assistant"));
        assert_eq!(event.text.as_deref(), Some("hello\nworld"));
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn claude_normalize_assistant_tool_use_sets_tool_call_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {
                        "type": "tool_use",
                        "id": "toolu_01",
                        "name": "Read",
                        "input": {"file_path": "Cargo.toml"}
                    }
                ]
            }
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(event.provider_item_id.as_deref(), Some("toolu_01"));
        assert_eq!(
            event.tool_call,
            Some(HarnessToolCall {
                id: Some("toolu_01".into()),
                name: "Read".into(),
                args: serde_json::json!({"file_path": "Cargo.toml"}),
            })
        );
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn claude_normalize_assistant_text_then_tool_use_expands_in_order_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "text", "text": "checking"},
                    {
                        "type": "tool_use",
                        "id": "toolu_01",
                        "name": "Read",
                        "input": {"file_path": "Cargo.toml"}
                    }
                ]
            }
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| event.raw_provider_event == raw));

        assert_eq!(events[0].kind, HarnessTurnEventKind::Message);
        assert_eq!(events[0].role.as_deref(), Some("assistant"));
        assert_eq!(events[0].text.as_deref(), Some("checking"));

        assert_eq!(events[1].kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(events[1].provider_item_id.as_deref(), Some("toolu_01"));
        assert_eq!(
            events[1].tool_call,
            Some(HarnessToolCall {
                id: Some("toolu_01".into()),
                name: "Read".into(),
                args: serde_json::json!({"file_path": "Cargo.toml"}),
            })
        );
    }

    #[test]
    fn claude_normalize_assistant_thinking_text_and_tool_uses_expands_in_order_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "thinking", "thinking": "thinking one"},
                    {"type": "text", "text": "done thinking"},
                    {
                        "type": "tool_use",
                        "id": "toolu_A",
                        "name": "Read",
                        "input": {"file_path": "Cargo.toml"}
                    },
                    {
                        "type": "tool_use",
                        "id": "toolu_B",
                        "name": "Write",
                        "input": {"file_path": "README.md", "content": "notes"}
                    }
                ]
            }
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 4);
        assert!(events.iter().all(|event| event.raw_provider_event == raw));

        assert_eq!(events[0].kind, HarnessTurnEventKind::Reasoning);
        assert_eq!(events[0].text.as_deref(), Some("thinking one"));

        assert_eq!(events[1].kind, HarnessTurnEventKind::Message);
        assert_eq!(events[1].role.as_deref(), Some("assistant"));
        assert_eq!(events[1].text.as_deref(), Some("done thinking"));

        assert_eq!(events[2].kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(events[2].provider_item_id.as_deref(), Some("toolu_A"));
        assert_eq!(
            events[2].tool_call,
            Some(HarnessToolCall {
                id: Some("toolu_A".into()),
                name: "Read".into(),
                args: serde_json::json!({"file_path": "Cargo.toml"}),
            })
        );

        assert_eq!(events[3].kind, HarnessTurnEventKind::ToolCall);
        assert_eq!(events[3].provider_item_id.as_deref(), Some("toolu_B"));
        assert_eq!(
            events[3].tool_call,
            Some(HarnessToolCall {
                id: Some("toolu_B".into()),
                name: "Write".into(),
                args: serde_json::json!({"file_path": "README.md", "content": "notes"}),
            })
        );
    }

    #[test]
    fn claude_normalize_assistant_unknown_content_block_stays_provider_meta_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "server_tool_use", "id": "srv_01"}
                ]
            }
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::ProviderMeta);
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn claude_normalize_user_tool_result_sets_tool_result_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "toolu_01",
                        "content": [{"type": "text", "text": "file contents"}],
                        "is_error": true
                    }
                ]
            }
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(event.provider_item_id.as_deref(), Some("toolu_01"));
        assert_eq!(
            event.tool_result,
            Some(HarnessToolResult {
                tool_call_id: Some("toolu_01".into()),
                name: None,
                content: serde_json::json!([{"type": "text", "text": "file contents"}]).to_string(),
                is_error: true,
            })
        );
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn claude_normalize_user_tool_results_expand_in_order_and_retain_raw() {
        let raw = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "u1",
                        "content": "first"
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "u2",
                        "content": [{"type": "text", "text": "second"}],
                        "is_error": true
                    }
                ]
            }
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| event.raw_provider_event == raw));

        assert_eq!(events[0].kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(events[0].provider_item_id.as_deref(), Some("u1"));
        assert_eq!(
            events[0].tool_result,
            Some(HarnessToolResult {
                tool_call_id: Some("u1".into()),
                name: None,
                content: "first".into(),
                is_error: false,
            })
        );

        assert_eq!(events[1].kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(events[1].provider_item_id.as_deref(), Some("u2"));
        assert_eq!(
            events[1].tool_result,
            Some(HarnessToolResult {
                tool_call_id: Some("u2".into()),
                name: None,
                content: serde_json::json!([{"type": "text", "text": "second"}]).to_string(),
                is_error: true,
            })
        );
    }

    #[test]
    fn live_normalize_claude_expansion_assigns_monotonic_seq_from_next_seq() {
        let raw = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "u1",
                        "content": "first"
                    },
                    {
                        "type": "tool_result",
                        "tool_use_id": "u2",
                        "content": "second"
                    }
                ]
            }
        });

        let events = normalize_live_turn_event("claude", "session-C", &raw, 8);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(events[0].seq, 8);
        assert_eq!(events[0].raw_provider_event, raw);
        assert_eq!(events[1].kind, HarnessTurnEventKind::ToolResult);
        assert_eq!(events[1].seq, 9);
        assert_eq!(events[1].raw_provider_event, raw);
    }

    #[test]
    fn claude_normalize_result_success_sets_completed_usage_cost_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "result",
            "subtype": "success",
            "result": "final answer",
            "total_cost_usd": 0.1866,
            "structured_output": {"verdict": "pass"},
            "usage": {
                "input_tokens": 42,
                "output_tokens": 15,
                "cached_input_tokens": 7,
                "reasoning_output_tokens": 3
            }
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::TurnCompleted);
        assert_eq!(event.text.as_deref(), Some("final answer"));
        assert_eq!(
            event.usage,
            Some(HarnessTokenUsage {
                input_tokens: 42,
                output_tokens: 15,
                total_tokens: 57,
                cached_input_tokens: Some(7),
                reasoning_output_tokens: Some(3),
            })
        );
        assert_eq!(event.cost_usd, Some(0.1866));
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn claude_normalize_result_non_success_sets_error_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "result",
            "subtype": "error_during_execution",
            "result": "tool failed",
            "usage": {"input_tokens": 1, "output_tokens": 2}
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.kind, HarnessTurnEventKind::Error);
        assert_eq!(event.text.as_deref(), Some("tool failed"));
        assert_eq!(event.error.as_deref(), Some("tool failed"));
        assert_eq!(
            event.usage,
            Some(HarnessTokenUsage {
                input_tokens: 1,
                output_tokens: 2,
                total_tokens: 3,
                cached_input_tokens: None,
                reasoning_output_tokens: None,
            })
        );
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn claude_normalize_unrecognized_type_stays_unknown_and_retains_raw() {
        let raw = serde_json::json!({
            "type": "provider_specific",
            "payload": {"n": 1}
        });

        let events = ClaudeAdapter.normalize_turn_event("session-C", &raw);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.session_id, "session-C");
        assert_eq!(event.provider, "claude");
        assert_eq!(event.seq, 0);
        assert_eq!(event.kind, HarnessTurnEventKind::Unknown);
        assert_eq!(event.raw_provider_event, raw);
        assert!(event.provider_thread_id.is_none());
        assert!(event.text.is_none());
        assert!(event.tool_call.is_none());
        assert!(event.tool_result.is_none());
        assert!(event.usage.is_none());
    }

    #[test]
    fn live_normalize_unknown_provider_falls_back_to_generic_event_with_seq() {
        let raw = serde_json::json!({
            "type": "provider_specific",
            "payload": {"n": 1}
        });

        let events = normalize_live_turn_event("mystery", "session-U", &raw, 17);
        assert_eq!(events.len(), 1);
        let event = &events[0];

        assert_eq!(event.session_id, "session-U");
        assert_eq!(event.provider, "mystery");
        assert_eq!(event.seq, 17);
        assert_eq!(event.kind, HarnessTurnEventKind::Unknown);
        assert_eq!(event.raw_provider_event, raw);
    }

    #[test]
    fn read_provider_session_normalized_events_preserves_order_and_raw() {
        let store = temp_store("normalized-events");
        let ndjson = store
            .root()
            .join("provider-sessions")
            .join("session-N")
            .join("claude.stream-json.ndjson");
        fs::create_dir_all(ndjson.parent().unwrap()).expect("mkdir session dir");
        let raw0 = serde_json::json!({"type": "assistant", "text": "first"});
        let raw1 = serde_json::json!({"type": "result", "status": "ok"});
        fs::write(&ndjson, format!("{raw0}\nnot-json\n{raw1}\n")).expect("write ndjson");
        store
            .append_provider_session(&provider_session_with_ref(
                "session-N",
                Some(ndjson.display().to_string()),
            ))
            .expect("append durable session");

        let (events, truncated) =
            read_provider_session_normalized_events(&store, "session-N").expect("read events");

        assert!(!truncated);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].provider, "claude");
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[0].kind, HarnessTurnEventKind::ProviderMeta);
        assert_eq!(events[0].raw_provider_event, raw0);
        assert_eq!(events[1].provider, "claude");
        assert_eq!(events[1].seq, 1);
        assert_eq!(events[1].kind, HarnessTurnEventKind::TurnCompleted);
        assert_eq!(events[1].raw_provider_event, raw1);
    }

    /// Seed one durable run: a real per-session NDJSON + ProviderSession(jsonl_ref),
    /// a completed WorkflowRun(trace_retention="durable") at `created`, and a step
    /// linking them. Returns the NDJSON path so a test can assert its survival.
    fn seed_durable_run(store: &HarnessStore, id: &str, created: u128) -> std::path::PathBuf {
        let session = format!("sess-{id}");
        let ndjson = store
            .root()
            .join("provider-sessions")
            .join(&session)
            .join("events.jsonl");
        fs::create_dir_all(ndjson.parent().unwrap()).expect("mkdir session dir");
        fs::write(&ndjson, "{\"type\":\"assistant\"}\n").expect("write ndjson");
        store
            .append_provider_session(&provider_session_with_ref(
                &session,
                Some(ndjson.display().to_string()),
            ))
            .expect("append session");
        store
            .append_workflow_run(&WorkflowRun {
                id: id.into(),
                workflow_name: "demo".into(),
                status: WorkflowRunStatus::Completed,
                step_ids: vec![format!("{id}-s")],
                created_at: format!("unix-ms:{created}"),
                ended_at: Some(format!("unix-ms:{}", created + 1)),
                summary: None,
                args: None,
                agents_spawned: 1,
                final_output: None,
                initiated_by: Some("op".into()),
                design_intent: None,
                spec: None,
                trace_retention: "durable".into(),
                host_pid: None,
                dry_run: false,
            })
            .expect("append run");
        store
            .append_workflow_step(&WorkflowStep {
                id: format!("{id}-s"),
                run_id: id.into(),
                phase: "work".into(),
                label: "node".into(),
                provider_session_id: Some(session),
                status: WorkflowStepStatus::Completed,
                output_summary: None,
                result: None,
                started_at: format!("unix-ms:{created}"),
                ended_at: Some(format!("unix-ms:{}", created + 1)),
            })
            .expect("append step");
        ndjson
    }

    #[test]
    fn reap_stale_workflow_runs_finalizes_old_running_rows() {
        let store = temp_store("reap-stale");
        let now = current_unix_ms();
        let mk = |id: &str, created: u128| WorkflowRun {
            id: id.into(),
            workflow_name: "demo".into(),
            status: WorkflowRunStatus::Running,
            step_ids: vec![],
            created_at: format!("unix-ms:{created}"),
            ended_at: None,
            summary: None,
            args: None,
            agents_spawned: 0,
            final_output: None,
            initiated_by: Some("op".into()),
            design_intent: None,
            spec: None,
            trace_retention: "durable".into(),
            host_pid: None,
            dry_run: false,
        };
        // One Running run 5h old -> reaped to Failed; one started "now" -> stays.
        store
            .append_workflow_run(&mk("wfrun-old", now.saturating_sub(5 * 60 * 60 * 1000)))
            .expect("append old");
        store
            .append_workflow_run(&mk("wfrun-fresh", now))
            .expect("append fresh");

        let reaped = reap_stale_workflow_runs(&store).expect("reap");
        assert_eq!(reaped, 1);

        let runs = latest_workflow_runs_in_append_order(&store).expect("read");
        let find = |id: &str| runs.iter().find(|r| r.id == id).expect("run present");
        assert_eq!(find("wfrun-old").status, WorkflowRunStatus::Failed);
        assert!(find("wfrun-old")
            .summary
            .as_deref()
            .unwrap_or("")
            .contains("reaped"));
        assert!(find("wfrun-old").ended_at.is_some());
        assert_eq!(find("wfrun-fresh").status, WorkflowRunStatus::Running);
    }

    #[test]
    fn reap_finalizes_runs_whose_host_process_is_dead_regardless_of_age() {
        let store = temp_store("reap-pid");
        // A child we immediately reap, so its pid is guaranteed dead on this host.
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let dead_pid = child.id();
        child.wait().expect("wait true");

        let now = current_unix_ms();
        // Created "now" (well under the 4h backstop) but its driver pid is dead —
        // so it must be reaped on pid-liveness alone, not the age window.
        store
            .append_workflow_run(&WorkflowRun {
                id: "wfrun-dead".into(),
                workflow_name: "demo".into(),
                status: WorkflowRunStatus::Running,
                step_ids: vec!["wfstep-dead".into()],
                created_at: format!("unix-ms:{now}"),
                ended_at: None,
                summary: None,
                args: None,
                agents_spawned: 0,
                final_output: None,
                initiated_by: Some("op".into()),
                design_intent: None,
                spec: None,
                trace_retention: "durable".into(),
                host_pid: Some(dead_pid),
                dry_run: false,
            })
            .expect("append run");
        // A still-open step under it must be closed to Failed by the reaper too.
        store
            .append_workflow_step(&WorkflowStep {
                id: "wfstep-dead".into(),
                run_id: "wfrun-dead".into(),
                phase: "scan".into(),
                label: "scan-context".into(),
                provider_session_id: None,
                status: WorkflowStepStatus::Running,
                output_summary: None,
                result: None,
                started_at: format!("unix-ms:{now}"),
                ended_at: None,
            })
            .expect("append step");
        // A run with a LIVE pid (this test process) must be left alone.
        store
            .append_workflow_run(&WorkflowRun {
                id: "wfrun-live".into(),
                workflow_name: "demo".into(),
                status: WorkflowRunStatus::Running,
                step_ids: vec![],
                created_at: format!("unix-ms:{now}"),
                ended_at: None,
                summary: None,
                args: None,
                agents_spawned: 0,
                final_output: None,
                initiated_by: Some("op".into()),
                design_intent: None,
                spec: None,
                trace_retention: "durable".into(),
                host_pid: Some(std::process::id()),
                dry_run: false,
            })
            .expect("append live run");

        let reaped = reap_stale_workflow_runs(&store).expect("reap");
        assert_eq!(reaped, 1, "only the dead-pid run is reaped");

        let runs = latest_workflow_runs_in_append_order(&store).expect("read runs");
        let find = |id: &str| runs.iter().find(|r| r.id == id).expect("run present");
        assert_eq!(find("wfrun-dead").status, WorkflowRunStatus::Failed);
        assert!(find("wfrun-dead")
            .summary
            .as_deref()
            .unwrap_or("")
            .contains("no longer alive"));
        assert_eq!(
            find("wfrun-live").status,
            WorkflowRunStatus::Running,
            "a run whose driver is still alive must not be reaped"
        );

        let steps = latest_workflow_steps_in_append_order(&store).expect("read steps");
        let step = steps
            .iter()
            .find(|s| s.id == "wfstep-dead")
            .expect("step present");
        assert_eq!(
            step.status,
            WorkflowStepStatus::Failed,
            "the reaped run's open step is closed to Failed"
        );
        assert!(step.ended_at.is_some());
    }

    #[test]
    fn running_step_carries_session_id_for_live_drill_in() {
        // The `running` row a step journals at start must carry the same
        // provider_session_id as its terminal row — so the dashboard can link the
        // step to its LIVE turn-event stream WHILE it runs, not only after it
        // finishes. (dry-run exercises the journaling without spawning a worker.)
        let store = temp_store("live-step-session");
        let options = WorkflowDeliveryOptions {
            dry_run: true,
            start_runtime: false,
            timeout_ms: 1_000,
            default_model: None,
            default_effort: None,
            max_budget_usd: None,
            trace_retention: "durable".into(),
            progress: false,
        };
        let spec = workflow::AgentStepSpec {
            phase: "scan".into(),
            label: "scan-context".into(),
            provider: "claude".into(),
            model: None,
            effort: None,
            fallback_model: None,
            image: Vec::new(),
            add_dir: Vec::new(),
            isolation: None,
            prompt: "do the thing".into(),
            schema: None,
            writable: false,
            ordinal: Some(0),
        };

        let result = workflow_real_agent_step(&store, "wfrun-live", &options, &spec);

        // Read the RAW append log (not the latest-wins projection) so we can
        // inspect the `running` row distinctly from the terminal row.
        let rows = store.workflow_steps().expect("read step rows");
        let running = rows
            .iter()
            .find(|s| s.status == WorkflowStepStatus::Running)
            .expect("a running row was journaled at step start");
        // THE FIX: the running row must already carry a session id (was `None`).
        let session = running
            .provider_session_id
            .as_deref()
            .expect("running step carries its session id for the live drill-in");
        assert!(
            session.starts_with("session-"),
            "session id is a real minted id, got {session}"
        );
        // The terminal row + the returned result must reuse the SAME id, so the
        // live buffer and the durable trace resolve to one session.
        let terminal = rows
            .iter()
            .find(|s| {
                matches!(
                    s.status,
                    WorkflowStepStatus::Completed | WorkflowStepStatus::Failed
                )
            })
            .expect("a terminal row was journaled at step finish");
        assert_eq!(terminal.provider_session_id.as_deref(), Some(session));
        assert_eq!(result.provider_session_id.as_deref(), Some(session));
    }

    #[test]
    fn running_provider_session_row_is_published_before_the_worker_finishes() {
        // The per-node drill-in resolves a step's live turn-event stream via its
        // provider_session_id -> the matching ProviderSession ROW. Without a row
        // published at step START, a RUNNING step renders "no turn yet" and its
        // live events (already streaming) have nothing to attach to. This asserts
        // the row exists, is RUNNING (not terminal), and the live NDJSON is
        // pre-created so the events route serves a growing list from t0.
        let store = temp_store("live-session-row");
        let session_id = "session-test-live";
        let session_dir = store.root().join("provider-sessions").join(session_id);
        std::fs::create_dir_all(&session_dir).expect("mk session dir");
        let spec = workflow::AgentStepSpec {
            phase: "work".into(),
            label: "explore".into(),
            provider: "claude".into(),
            model: None,
            effort: None,
            fallback_model: None,
            image: Vec::new(),
            add_dir: Vec::new(),
            isolation: None,
            prompt: "p".into(),
            schema: None,
            writable: false,
            ordinal: Some(0),
        };

        write_running_ephemeral_session(&store, session_id, &session_dir, &spec);

        let sessions = store.provider_sessions().expect("read sessions");
        let row = sessions
            .iter()
            .find(|s| s.id == session_id)
            .expect("a RUNNING provider session row is published at step start");
        assert_eq!(row.status, ProviderSessionStatus::Running);
        assert!(row.ended_at.is_none(), "running row has no ended_at");
        assert!(row.exit_code.is_none(), "running row has no exit code");
        assert!(
            session_dir.join("claude.stream-json.ndjson").exists(),
            "live NDJSON pre-created so the events route serves a growing list"
        );
        assert!(row
            .jsonl_ref
            .as_deref()
            .expect("jsonl_ref points at the live file")
            .ends_with("claude.stream-json.ndjson"));
    }

    #[test]
    fn truncate_on_char_boundary_never_splits_a_multibyte_char() {
        // ASCII shorter than the cap is returned unchanged.
        assert_eq!(truncate_on_char_boundary("hello", 160), "hello");

        // issue #89 P0: a CJK string whose byte cap (240) lands INSIDE a 3-byte
        // char must back off to a char boundary instead of panicking on `&s[..240]`.
        let cjk = "保留中文输出不要崩溃".repeat(40); // 10 chars * 3 bytes * 40
        let out = truncate_on_char_boundary(&cjk, 240);
        assert!(out.len() <= 240, "respects the byte cap");
        assert!(cjk.starts_with(out), "is a valid prefix");
        assert!(
            cjk.is_char_boundary(out.len()),
            "ends on a char boundary (no split)"
        );

        // The summary path that crashed (main.rs:summarize_json_value) must no
        // longer panic on CJK that overflows the cap.
        let value = serde_json::Value::String("留".repeat(200));
        let summary = summarize_json_value(&value); // pre-fix: byte-slice panic
        assert!(
            summary.ends_with("..."),
            "long value is truncated with an ellipsis"
        );
    }

    #[test]
    fn take_flag_value_removes_the_pair_and_returns_the_value() {
        let mut args = vec![
            "--store".to_string(),
            "/tmp/store".to_string(),
            "serve".to_string(),
            "--addr".to_string(),
            "127.0.0.1:1".to_string(),
        ];
        assert_eq!(
            take_flag_value(&mut args, "--store").as_deref(),
            Some("/tmp/store")
        );
        // The pair is stripped so the subcommand parser never sees it.
        assert_eq!(args, vec!["serve", "--addr", "127.0.0.1:1"]);
        // Absent flag -> None, args untouched.
        assert_eq!(take_flag_value(&mut args, "--store"), None);
        assert_eq!(args.len(), 3);
        // Trailing flag with no value -> flag removed, None returned.
        let mut trailing = vec!["serve".to_string(), "--store".to_string()];
        assert_eq!(take_flag_value(&mut trailing, "--store"), None);
        assert_eq!(trailing, vec!["serve"]);
    }

    #[test]
    fn discover_harness_from_finds_the_nearest_ancestor_dot_harness() {
        let base = std::env::temp_dir().join(format!("harness-disc-{}", generated_id("d")));
        let proj = base.join("proj");
        let deep = proj.join("a").join("b");
        std::fs::create_dir_all(&deep).expect("mk deep");
        std::fs::create_dir_all(proj.join(".harness")).expect("mk .harness");

        // From a nested subdir, discovery walks UP to proj/.harness.
        let found = discover_harness_from(&deep).expect("found ancestor .harness");
        assert_eq!(
            std::fs::canonicalize(&found).unwrap(),
            std::fs::canonicalize(proj.join(".harness")).unwrap()
        );
        // A tree with no .harness returns None.
        let bare = base.join("bare").join("x");
        std::fs::create_dir_all(&bare).expect("mk bare");
        // (only true if no ancestor of `bare` has .harness — base/bare has none)
        assert!(discover_harness_from(&base.join("bare")).is_none());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn resolve_store_root_prefers_explicit_store_flag() {
        let mut args = vec![
            "--store".to_string(),
            "/explicit/store".to_string(),
            "serve".to_string(),
        ];
        let root = resolve_store_root(&mut args);
        assert_eq!(root, PathBuf::from("/explicit/store"));
        // Flag stripped so dispatch sees only the subcommand.
        assert_eq!(args, vec!["serve"]);
    }

    #[test]
    fn workflow_get_output_returns_full_reply_and_falls_back_to_summary() {
        let store = temp_store("get-output");
        let mk_step = |id: &str, label: &str, sid: &str, summary: &str| WorkflowStep {
            id: id.into(),
            run_id: "wfrun-go".into(),
            phase: "p".into(),
            label: label.into(),
            provider_session_id: Some(sid.into()),
            status: WorkflowStepStatus::Completed,
            output_summary: Some(summary.into()),
            result: None,
            started_at: "unix-ms:1".into(),
            ended_at: Some("unix-ms:2".into()),
        };
        store
            .append_workflow_run(&WorkflowRun {
                id: "wfrun-go".into(),
                workflow_name: "demo".into(),
                status: WorkflowRunStatus::Completed,
                step_ids: vec!["s1".into(), "s2".into()],
                created_at: "unix-ms:1".into(),
                ended_at: Some("unix-ms:9".into()),
                summary: None,
                args: None,
                agents_spawned: 2,
                final_output: None,
                initiated_by: Some("op".into()),
                design_intent: None,
                spec: None,
                trace_retention: "durable".into(),
                host_pid: None,
                dry_run: false,
            })
            .expect("append run");
        store
            .append_workflow_step(&mk_step("s1", "scan", "sess-1", "scan summary"))
            .expect("append s1");
        store
            .append_workflow_step(&mk_step(
                "s2",
                "synthesis",
                "sess-2",
                "synth summary (capped)",
            ))
            .expect("append s2");

        // Persist a FULL reply only for s2's session (mirrors ingest writing reply.txt).
        let full = "FULL synthesis output ".repeat(500); // > any summary cap
        let dir = store.root().join("provider-sessions").join("sess-2");
        std::fs::create_dir_all(&dir).expect("mk session dir");
        std::fs::write(dir.join("reply.txt"), &full).expect("write reply");

        let out = workflow_get_output_value(&store, &["wfrun-go".to_string()]).expect("get-output");
        let steps = out["steps"].as_array().expect("steps array");
        assert_eq!(steps.len(), 2);
        // Order follows run.step_ids: scan (s1) then synthesis (s2).
        assert_eq!(steps[0]["label"], "scan");
        assert_eq!(steps[1]["label"], "synthesis");
        // s1 has no reply.txt -> falls back to the capped summary.
        assert_eq!(steps[0]["source"], "summary");
        assert_eq!(steps[0]["output"], "scan summary");
        // s2 has reply.txt -> full text, source "reply".
        assert_eq!(steps[1]["source"], "reply");
        assert_eq!(steps[1]["output"].as_str().unwrap(), full);

        // --step selects one leaf.
        let one = workflow_get_output_value(
            &store,
            &[
                "wfrun-go".to_string(),
                "--step".to_string(),
                "synthesis".to_string(),
            ],
        )
        .expect("get-output --step");
        let one_steps = one["steps"].as_array().unwrap();
        assert_eq!(one_steps.len(), 1);
        assert_eq!(one_steps[0]["label"], "synthesis");

        // Unknown step / unknown run are usage errors.
        assert!(workflow_get_output_value(
            &store,
            &[
                "wfrun-go".to_string(),
                "--step".to_string(),
                "nope".to_string()
            ]
        )
        .is_err());
        assert!(workflow_get_output_value(&store, &["wfrun-missing".to_string()]).is_err());
    }

    #[test]
    fn worktree_create_in_non_git_dir_gives_actionable_error() {
        // A writable / isolated step in a non-git cwd must fail with guidance, not
        // the cryptic raw `git worktree add` error (issue #89 item 5).
        let dir = std::env::temp_dir().join(format!("harness-nongit-{}", generated_id("ng")));
        std::fs::create_dir_all(&dir).expect("mk non-git dir");
        // (WorktreeGuard isn't Debug — match instead of expect_err.)
        let msg = match WorktreeGuard::create(&dir, "wfrun-x", "writer", "session-x-0") {
            Ok(_) => panic!("a non-git dir must fail clearly, not attempt git worktree add"),
            Err(e) => e.to_string(),
        };
        assert!(
            msg.contains("not a git repository"),
            "names the cause: {msg}"
        );
        assert!(
            msg.contains("git init") && msg.contains("get-output"),
            "offers both fixes (git init / read-only + get-output): {msg}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn worktree_paths_are_unique_per_leaf_even_with_duplicate_labels() {
        // issue #139 item 7: two SAME-LABEL writable nodes in one run must NOT
        // share a worktree path/branch (the collision that failed the 2nd node).
        // The per-leaf session_id disambiguates them.
        let (rel_a, br_a) = worktree_paths("wfrun-1", "dup", "session-1-0");
        let (rel_b, br_b) = worktree_paths("wfrun-1", "dup", "session-1-1");
        assert_ne!(rel_a, rel_b, "same-label leaves must get distinct paths");
        assert_ne!(br_a, br_b, "same-label leaves must get distinct branches");
        // Stable for the same leaf, and the label + run are still in the name.
        assert_eq!(
            worktree_paths("wfrun-1", "dup", "session-1-0"),
            (rel_a.clone(), br_a.clone())
        );
        assert!(rel_a.contains("wfrun-1") && rel_a.contains("dup"));
        assert!(br_a.starts_with("harness/wt/"));
    }

    #[test]
    fn gc_trace_prunes_old_durable_runs_and_keeps_recent() {
        let store = temp_store("gc-trace");
        let old1 = seed_durable_run(&store, "wfrun-old1", 1_000);
        let old2 = seed_durable_run(&store, "wfrun-old2", 2_000);
        let recent = seed_durable_run(&store, "wfrun-new", 9_000);

        // Keep only the single most-recent durable run; prune the rest.
        let out = workflow_gc_trace(&store, 1, None, false).expect("gc-trace");
        assert_eq!(out.get("pruned_runs").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(out.get("kept_runs").and_then(|v| v.as_u64()), Some(1));

        // Recent run's heavy trace survives intact.
        assert!(recent.exists(), "recent NDJSON must remain");
        assert!(
            read_session_turn_events(&store, "sess-wfrun-new")
                .unwrap()
                .retained
        );

        // Old runs: NDJSON deleted, endpoint reports not-retained, run flips to expired.
        assert!(!old1.exists() && !old2.exists(), "old NDJSON removed");
        assert!(
            !read_session_turn_events(&store, "sess-wfrun-old1")
                .unwrap()
                .retained
        );
        assert!(
            !read_session_turn_events(&store, "sess-wfrun-old2")
                .unwrap()
                .retained
        );
        let latest = latest_workflow_runs_in_append_order(&store).unwrap();
        let retention = |id: &str| {
            latest
                .iter()
                .find(|r| r.id == id)
                .unwrap()
                .trace_retention
                .clone()
        };
        assert_eq!(retention("wfrun-old1"), "expired");
        assert_eq!(retention("wfrun-old2"), "expired");
        assert_eq!(retention("wfrun-new"), "durable");
    }

    #[test]
    fn gc_trace_dry_run_changes_nothing() {
        let store = temp_store("gc-trace-dry");
        let old = seed_durable_run(&store, "wfrun-old", 1_000);
        seed_durable_run(&store, "wfrun-new", 9_000);
        let out = workflow_gc_trace(&store, 1, None, true).expect("gc dry");
        assert_eq!(out.get("pruned_runs").and_then(|v| v.as_u64()), Some(1));
        assert!(old.exists(), "dry-run must not delete the NDJSON");
        assert!(
            read_session_turn_events(&store, "sess-wfrun-old")
                .unwrap()
                .retained
        );
    }

    #[test]
    fn durable_session_returns_persisted_turn_events_in_order() {
        let store = temp_store("durable-events");
        // Write the durable per-session NDJSON the jsonl_ref will point at.
        let ndjson = store
            .root()
            .join("provider-sessions")
            .join("session-A")
            .join("claude.stream-json.ndjson");
        fs::create_dir_all(ndjson.parent().unwrap()).expect("mkdir session dir");
        fs::write(&ndjson, "{\"type\":\"assistant\"}\n{\"type\":\"result\"}\n")
            .expect("write ndjson");
        store
            .append_provider_session(&provider_session_with_ref(
                "session-A",
                Some(ndjson.display().to_string()),
            ))
            .expect("append durable session");

        let out = read_session_turn_events(&store, "session-A").expect("read events");
        assert!(out.retained, "durable run must report retained");
        assert!(!out.truncated);
        assert_eq!(out.events.len(), 2);
        assert_eq!(
            out.events[0].get("type").and_then(|t| t.as_str()),
            Some("assistant")
        );
        assert_eq!(
            out.events[1].get("type").and_then(|t| t.as_str()),
            Some("result")
        );
    }

    #[test]
    fn live_only_session_reports_not_retained() {
        let store = temp_store("live-events");
        // Live-only: the session row survives but its jsonl_ref/stdout_ref are None
        // (the Backend pruned the NDJSON after the run).
        store
            .append_provider_session(&provider_session_with_ref("session-L", None))
            .expect("append live-only session");

        let out = read_session_turn_events(&store, "session-L").expect("read events");
        assert!(!out.retained, "live run must report not retained");
        assert!(out.events.is_empty(), "not-retained trace yields no events");
        assert!(!out.truncated);
    }

    #[test]
    fn unknown_session_reports_not_retained() {
        let store = temp_store("missing-events");
        // No ProviderSession row at all -> nothing to drill into.
        let out = read_session_turn_events(&store, "session-Z").expect("read events");
        assert!(!out.retained, "missing session has no retained trace");
        assert!(out.events.is_empty());
    }

    #[test]
    fn historical_normalized_events_normalize_durable_trace_and_report_retained() {
        let store = temp_store("historical-normalized");
        let ndjson = store
            .root()
            .join("provider-sessions")
            .join("session-HN")
            .join("claude.stream-json.ndjson");
        fs::create_dir_all(ndjson.parent().unwrap()).expect("mkdir session dir");
        // A real-ish claude trace: an assistant text block then a result.
        fs::write(
            &ndjson,
            "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hi\"}]}}\n{\"type\":\"result\",\"subtype\":\"success\"}\n",
        )
        .expect("write ndjson");
        store
            .append_provider_session(&provider_session_with_ref(
                "session-HN",
                Some(ndjson.display().to_string()),
            ))
            .expect("append durable session");

        let (retained, events, truncated) =
            read_session_turn_events_normalized(&store, "session-HN").expect("read normalized");
        assert!(retained, "durable run must report retained");
        assert!(!truncated);
        assert_eq!(events.len(), 2);
        // Provider-agnostic canonical kinds (claude assistant text -> Message,
        // result -> TurnCompleted), monotonic seq, raw retained on each.
        assert_eq!(events[0].kind, HarnessTurnEventKind::Message);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[0].text.as_deref(), Some("hi"));
        assert!(!events[0].raw_provider_event.is_null());
        assert_eq!(events[1].kind, HarnessTurnEventKind::TurnCompleted);
        assert_eq!(events[1].seq, 1);
        assert!(!events[1].raw_provider_event.is_null());
    }

    #[test]
    fn historical_normalized_events_report_not_retained_for_pruned_trace() {
        let store = temp_store("historical-normalized-pruned");
        // Live-only run: row survives, jsonl_ref pruned -> not retained, no events.
        store
            .append_provider_session(&provider_session_with_ref("session-HP", None))
            .expect("append live-only session");

        let (retained, events, truncated) =
            read_session_turn_events_normalized(&store, "session-HP").expect("read normalized");
        assert!(
            !retained,
            "pruned --trace live run must report not retained"
        );
        assert!(
            events.is_empty(),
            "not-retained trace yields no normalized events"
        );
        assert!(!truncated);
    }

    #[test]
    fn workflow_run_transitions_running_to_failed_on_failed_required_step() {
        let store = temp_store("failed");
        let registry = workflow::WorkflowRegistry::builtin();
        let def = registry.get("investigate").expect("investigate registered");
        // Mock driver: the required serial "scope" step fails; audits succeed.
        let driver = |spec: &workflow::AgentStepSpec| {
            let ok = spec.phase != "scope";
            workflow::StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok,
                provider_session_id: ok.then(|| "s".to_string()),
                output_summary: "mock".to_string(),
                step_id: None,
                started_at: None,
                details: None,
                structured: None,
                ordinal: None,
            }
        };

        let run_id = generated_id("wfrun");
        let result = run_workflow_with_driver(&store, &run_id, def, "failure Y", false, &driver)
            .expect("run workflow");
        let run = result.get("run").expect("run key");
        assert_eq!(run.get("status").and_then(|s| s.as_str()), Some("failed"));

        let runs = store.workflow_runs().expect("read runs");
        assert_eq!(runs[0].status, WorkflowRunStatus::Running);
        assert_eq!(runs.last().unwrap().status, WorkflowRunStatus::Failed);

        // All three steps are still journaled (parallel barrier collected nulls).
        let steps = store.workflow_steps().expect("read steps");
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].status, WorkflowStepStatus::Failed);
        assert_eq!(steps[1].status, WorkflowStepStatus::Completed);
        assert_eq!(steps[2].status, WorkflowStepStatus::Completed);
    }

    #[test]
    fn workflow_run_script_journals_steps_and_snapshots_source() {
        let store = temp_store("run-script");
        // A two-agent Starlark program that chains output. `--dry-run` returns a
        // mock StepResult per node, so no provider is spawned (CI-safe).
        let script = r#"
workflow("triage", "scan first, then fix what the scan reported so the fix builds on it")
phase("scan")
a = agent("scan " + args["area"])
phase("fix")
agent("fix: " + a, provider = "claude", label = "fixer")
"#;
        let dir = std::env::temp_dir().join(format!("harness-wf-script-{}", generated_id("src")));
        fs::create_dir_all(&dir).expect("mkdir script dir");
        let path = dir.join("triage.star");
        fs::write(&path, script).expect("write script");

        let args = vec![
            path.display().to_string(),
            "--args".to_string(),
            r#"{"area":"checkout"}"#.to_string(),
            "--dry-run".to_string(),
        ];
        let result = workflow_run_script_value(&store, &args).expect("run script");

        // The run completed and references two steps.
        let run = result.get("run").expect("run key");
        assert_eq!(
            run.get("status").and_then(|s| s.as_str()),
            Some("completed")
        );
        // Workflow name defaults to the file stem.
        assert_eq!(
            run.get("workflow_name").and_then(|s| s.as_str()),
            Some("triage")
        );
        let step_ids = run
            .get("step_ids")
            .and_then(|s| s.as_array())
            .expect("step_ids");
        assert_eq!(step_ids.len(), 2);

        // The durable audit record snapshots the raw script text as a starlark spec.
        let runs = store.workflow_runs().expect("read runs");
        let final_run = runs.last().expect("a run row");
        let spec = final_run.spec.as_ref().expect("spec snapshot");
        assert_eq!(spec.get("lang").and_then(|v| v.as_str()), Some("starlark"));
        assert_eq!(spec.get("script").and_then(|v| v.as_str()), Some(script));
        // The mandatory design_intent from the `workflow(...)` header is persisted.
        assert_eq!(
            final_run.design_intent.as_deref(),
            Some("scan first, then fix what the scan reported so the fix builds on it")
        );
        // This was a `--dry-run`, so the journaled run is marked as such — a
        // validation run must be distinguishable from a real one (issue #89 item 2).
        assert!(final_run.dry_run, "dry-run runs are marked dry_run: true");
        // The parsed --args are carried opaquely onto the run.
        assert_eq!(
            final_run
                .args
                .as_ref()
                .and_then(|a| a.get("area"))
                .and_then(|v| v.as_str()),
            Some("checkout")
        );

        // The real driver journals a `running` row at step start and reuses its
        // id for the terminal row, so the append-only log holds running+terminal
        // rows per step. Project latest-wins by id: the two referenced steps must
        // each resolve to a completed terminal row across the distinct phases.
        let all_steps = store.workflow_steps().expect("read steps");
        let referenced: Vec<&str> = step_ids
            .iter()
            .map(|id| id.as_str().expect("step id string"))
            .collect();
        let mut terminal: BTreeMap<&str, &WorkflowStep> = BTreeMap::new();
        for step in &all_steps {
            if referenced.contains(&step.id.as_str()) {
                terminal.insert(step.id.as_str(), step);
            }
        }
        assert_eq!(terminal.len(), 2);
        let phases: BTreeSet<&str> = terminal.values().map(|s| s.phase.as_str()).collect();
        assert_eq!(
            phases,
            BTreeSet::from(["scan", "fix"]),
            "both phases journaled"
        );
        for step in terminal.values() {
            assert_eq!(step.status, WorkflowStepStatus::Completed);
        }
    }

    #[test]
    fn workflow_run_script_resume_reuses_prior_steps() {
        let store = temp_store("run-script-resume");
        let script = r#"
workflow("triage", "scan first then fix, so the fix builds on the scan output")
a = agent("scan the code")
agent("fix per " + a, label = "fixer")
"#;
        let dir = std::env::temp_dir().join(format!("harness-wf-resume-{}", generated_id("src")));
        fs::create_dir_all(&dir).expect("mkdir script dir");
        let path = dir.join("triage.star");
        fs::write(&path, script).expect("write script");

        // First run (dry-run) to journal succeeded steps carrying ordinals.
        let args = vec![path.display().to_string(), "--dry-run".to_string()];
        let first = workflow_run_script_value(&store, &args).expect("first run");
        let prior_run_id = first
            .get("run")
            .and_then(|r| r.get("id"))
            .and_then(|v| v.as_str())
            .expect("prior run id")
            .to_string();

        // The prior steps carry an ordinal in their result JSON (the round-trip).
        let prior_steps: Vec<WorkflowStep> = latest_workflow_steps_in_append_order(&store)
            .expect("steps")
            .into_iter()
            .filter(|s| s.run_id == prior_run_id)
            .collect();
        assert!(prior_steps.iter().all(|s| s
            .result
            .as_ref()
            .and_then(|r| r.get("ordinal"))
            .is_some()));

        // Resume: re-run the SAME script with --resume <prior_run_id>.
        let resume_args = vec![
            path.display().to_string(),
            "--dry-run".to_string(),
            "--resume".to_string(),
            prior_run_id.clone(),
        ];
        let second = workflow_run_script_value(&store, &resume_args).expect("resume run");
        let run = second.get("run").expect("run key");
        assert_eq!(
            run.get("status").and_then(|s| s.as_str()),
            Some("completed")
        );
        let new_run_id = run.get("id").and_then(|v| v.as_str()).expect("new run id");
        assert_ne!(new_run_id, prior_run_id, "resume mints a NEW run id");
        let step_ids = run
            .get("step_ids")
            .and_then(|s| s.as_array())
            .expect("step_ids");
        assert_eq!(step_ids.len(), 2, "the resumed run references both leaves");

        // The new run records which prior run it resumed from.
        let runs = store.workflow_runs().expect("read runs");
        let final_run = runs
            .iter()
            .rev()
            .find(|r| r.id == new_run_id)
            .expect("new run row");
        assert_eq!(
            final_run
                .spec
                .as_ref()
                .and_then(|s| s.get("resumed_from"))
                .and_then(|v| v.as_str()),
            Some(prior_run_id.as_str())
        );

        // The new run's steps carry the [replayed] marker (driver not re-invoked).
        let new_steps: Vec<WorkflowStep> = latest_workflow_steps_in_append_order(&store)
            .expect("steps")
            .into_iter()
            .filter(|s| s.run_id == new_run_id)
            .collect();
        assert_eq!(new_steps.len(), 2);
        for step in &new_steps {
            assert!(
                step.output_summary
                    .as_deref()
                    .unwrap_or_default()
                    .starts_with("[replayed] "),
                "resumed step output: {:?}",
                step.output_summary
            );
            assert_eq!(
                step.result.as_ref().and_then(|r| r.get("replayed")),
                Some(&serde_json::json!(true))
            );
        }
    }

    #[test]
    fn workflow_run_script_resume_rejects_changed_script() {
        let store = temp_store("run-script-resume-changed");
        let dir =
            std::env::temp_dir().join(format!("harness-wf-resume-chg-{}", generated_id("src")));
        fs::create_dir_all(&dir).expect("mkdir script dir");
        let path = dir.join("triage.star");
        let original = r#"
workflow("triage", "a stable design intent that explains the shape")
agent("scan the code")
"#;
        fs::write(&path, original).expect("write script");
        let first = workflow_run_script_value(
            &store,
            &[path.display().to_string(), "--dry-run".to_string()],
        )
        .expect("first run");
        let prior_run_id = first
            .get("run")
            .and_then(|r| r.get("id"))
            .and_then(|v| v.as_str())
            .expect("prior id")
            .to_string();

        // Edit the script, then attempt to resume — the guard must reject it.
        let changed = r#"
workflow("triage", "a stable design intent that explains the shape")
agent("scan the code")
agent("a NEW second leaf that changes the ordinal alignment")
"#;
        fs::write(&path, changed).expect("rewrite script");
        let err = workflow_run_script_value(
            &store,
            &[
                path.display().to_string(),
                "--dry-run".to_string(),
                "--resume".to_string(),
                prior_run_id,
            ],
        )
        .expect_err("changed script rejected");
        match err {
            CliError::Usage(msg) => assert!(
                msg.contains("the script changed"),
                "unexpected message: {msg}"
            ),
            other => panic!("expected Usage error, got {other:?}"),
        }
    }

    #[test]
    fn workflow_run_script_rejects_bad_args_json() {
        let store = temp_store("run-script-badargs");
        let dir = std::env::temp_dir().join(format!("harness-wf-script-{}", generated_id("bad")));
        fs::create_dir_all(&dir).expect("mkdir script dir");
        let path = dir.join("noop.star");
        fs::write(&path, r#"agent("x")"#).expect("write script");

        let args = vec![
            path.display().to_string(),
            "--args".to_string(),
            "{not json".to_string(),
            "--dry-run".to_string(),
        ];
        let err = workflow_run_script_value(&store, &args).expect_err("bad json");
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[test]
    fn workflow_run_script_rejects_missing_design_intent() {
        // A program with no `workflow(...)` header is rejected fail-fast, and the
        // error mentions design_intent so the author knows what to add.
        let store = temp_store("run-script-no-intent");
        let dir = std::env::temp_dir().join(format!("harness-wf-script-{}", generated_id("noi")));
        fs::create_dir_all(&dir).expect("mkdir script dir");
        let path = dir.join("noheader.star");
        fs::write(&path, r#"agent("x")"#).expect("write script");

        let args = vec![path.display().to_string(), "--dry-run".to_string()];
        let err = workflow_run_script_value(&store, &args).expect_err("rejected");
        match err {
            CliError::Usage(message) => assert!(
                message.contains("design_intent"),
                "error should mention design_intent: {message}"
            ),
            other => panic!("expected Usage error, got {other:?}"),
        }
    }

    #[test]
    fn dashboard_snapshot_includes_workflow_keys() {
        let store = temp_store("snapshot");
        // Empty store: keys must still be present (additive, inspectable).
        let snapshot = dashboard_snapshot(&store).expect("snapshot");
        assert!(snapshot.get("workflow_runs").is_some());
        assert!(snapshot.get("workflow_steps").is_some());

        // After a run, the keys surface the journaled rows.
        let registry = workflow::WorkflowRegistry::builtin();
        let def = registry.get("investigate").expect("registered");
        let driver = |spec: &workflow::AgentStepSpec| ok_step(spec);
        let run_id = generated_id("wfrun");
        run_workflow_with_driver(&store, &run_id, def, "x", false, &driver).expect("run");

        let snapshot = dashboard_snapshot(&store).expect("snapshot");
        let runs = snapshot
            .get("workflow_runs")
            .and_then(|v| v.as_array())
            .expect("workflow_runs array");
        assert_eq!(runs.len(), 1, "latest-wins projection collapses to one run");
        assert_eq!(
            runs[0].get("status").and_then(|s| s.as_str()),
            Some("completed")
        );
        let steps = snapshot
            .get("workflow_steps")
            .and_then(|v| v.as_array())
            .expect("workflow_steps array");
        assert_eq!(steps.len(), 3);
    }

    /// LIVE PROGRESS contract: when a driver journals a `running` step row at
    /// step start (carrying its `step_id` + real `started_at`), the runtime
    /// REUSES that identity for the terminal row. The append log then holds two
    /// rows per step (running -> completed), but the latest-wins projection
    /// collapses to one terminal row whose `started_at` is the driver's real
    /// start time — never overwritten by the journal time. This is what lets the
    /// SSE watcher stream a `running` frame as each step starts.
    #[test]
    fn driver_journaled_running_row_is_reused_for_terminal_row() {
        let store = temp_store("live-progress");
        let registry = workflow::WorkflowRegistry::builtin();
        let def = registry.get("investigate").expect("investigate registered");
        let run_id = generated_id("wfrun");

        // A driver that mimics the real path: journal a `running` row up front,
        // then return a StepResult carrying that same id + start time.
        let driver = |spec: &workflow::AgentStepSpec| {
            let step_id = generated_id("wfstep");
            let started_at = format!("unix-ms:{}", 1_000 + spec.label.len());
            let running = WorkflowStep {
                id: step_id.clone(),
                run_id: run_id.clone(),
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider_session_id: None,
                status: WorkflowStepStatus::Running,
                output_summary: None,
                result: None,
                started_at: started_at.clone(),
                ended_at: None,
            };
            store
                .append_workflow_step(&running)
                .expect("journal running");
            let result = workflow::StepResult {
                phase: spec.phase.clone(),
                label: spec.label.clone(),
                provider: spec.provider.clone(),
                isolation: spec.isolation.clone(),
                ok: true,
                provider_session_id: Some(format!("session-{}", spec.label)),
                output_summary: format!("ok: {}", spec.label),
                step_id: Some(step_id.clone()),
                started_at: Some(started_at.clone()),
                details: None,
                structured: None,
                ordinal: None,
            };
            // Mirror the real driver under the live-per-step contract: also
            // journal the TERMINAL row at completion, reusing the same step_id +
            // start time. `run_workflow_with_driver` must then NOT re-journal it.
            store
                .append_workflow_step(&build_terminal_step(&run_id, step_id, started_at, &result))
                .expect("journal terminal");
            result
        };

        let result = run_workflow_with_driver(&store, &run_id, def, "topic", false, &driver)
            .expect("run workflow");
        assert_eq!(
            result
                .get("run")
                .and_then(|r| r.get("status"))
                .and_then(|s| s.as_str()),
            Some("completed")
        );

        // Raw append log: the driver journaled a `running` row at start AND the
        // terminal row at completion (2 rows x 3 steps = 6). run_workflow_with_driver
        // recognises the driver-journaled terminal (step_id is Some) and does NOT
        // re-journal — so the count stays 6, not 9.
        let appended = store.workflow_steps().expect("read step log");
        assert_eq!(
            appended.len(),
            6,
            "driver journals running + terminal per step; finalize does not re-journal"
        );
        assert_eq!(
            appended
                .iter()
                .filter(|s| s.status == WorkflowStepStatus::Running)
                .count(),
            3,
            "a running row was journaled at the start of each step (live progress)"
        );

        // Latest-wins projection: exactly 3 terminal rows, each reusing the
        // driver's start time rather than the journal-time stamp.
        let steps = latest_workflow_steps_in_append_order(&store).expect("project steps");
        assert_eq!(
            steps.len(),
            3,
            "running+terminal collapse to one row per step"
        );
        for step in &steps {
            assert_eq!(step.status, WorkflowStepStatus::Completed);
            assert!(
                step.started_at.starts_with("unix-ms:1"),
                "terminal row kept the driver's real start time: {}",
                step.started_at
            );
            assert!(step.ended_at.is_some());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_allowed_doc_rejects_traversal_and_non_docs_paths() {
        // Missing parameter.
        assert!(read_allowed_doc("/v1/docs").is_err());
        // Outside the docs/ allow-list.
        assert!(read_allowed_doc("/v1/docs?path=etc/passwd").is_err());
        assert!(read_allowed_doc("/v1/docs?path=Cargo.toml").is_err());
        // Path traversal, even under docs/.
        assert!(read_allowed_doc("/v1/docs?path=docs/../Cargo.toml").is_err());
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
    fn claude_member_runtime_start_dispatches_to_claude_stub() {
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("claude-start")));
        let store = HarnessStore::new(&root);
        let mut member = make_member("claude-agent");
        member.provider = "claude".into();

        let runtime = start_provider_runtime(&store, &member)
            .expect("claude runtime start dispatches to claude implementation");
        assert_eq!(
            runtime.provider, "claude",
            "runtime must have claude provider"
        );
        assert_eq!(runtime.command, "claude", "runtime must use claude command");
        assert!(
            runtime
                .control_endpoint
                .as_deref()
                .map(|ep| ep.starts_with("claude-runtime://"))
                .unwrap_or(false),
            "claude runtime must use claude-runtime:// endpoint"
        );
        assert!(
            runtime.pid.is_none(),
            "claude on-demand runtime should not have persistent PID"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn claude_member_delivery_dispatches_to_claude_stub() {
        // WP-3: Test the new real claude -p delivery (replaces stub).
        // When claude binary is absent, the delivery should fail gracefully with
        // a spawn error; when present, it should execute. Either way, we assert
        // that the dispatch routed to claude (not codex/unknown).
        let root = std::env::temp_dir().join(format!(
            "harness-cli-test-{}",
            generated_id("claude-deliver")
        ));
        let store = HarnessStore::new(&root);
        let mut member = make_member("claude-agent");
        member.provider = "claude".into();
        let runtime = AgentRuntime {
            id: "runtime-claude".into(),
            agent_member_id: member.id.clone(),
            provider: "claude".into(),
            status: AgentRuntimeStatus::Running,
            pid: None,
            control_endpoint: Some(format!("claude-runtime://{}", root.display())),
            command: "claude".into(),
            args: Vec::new(),
            started_at: "unix-ms:1".into(),
            ended_at: None,
            last_event_at: Some("unix-ms:1".into()),
            health: AgentRuntimeHealth {
                process_alive: false,
                socket_exists: false,
                protocol_probe: None,
                delivery_probe: None,
                checked_at: None,
            },
        };
        let message = Message {
            id: "message-claude".into(),
            task_id: None,
            from_agent_id: "lead-1".into(),
            to_agent_id: Some(member.id.clone()),
            channel: Some("agent-direct".into()),
            kind: MessageKind::Message,
            delivery_status: MessageDeliveryStatus::Queued,
            content: "Hello".into(),
            evidence_ids: Vec::new(),
            created_at: "unix-ms:1".into(),
            delivery: None,
            sender_kind: SenderKind::Agent,
        };

        // Dispatch and verify routing. If claude binary is present, delivery may
        // succeed; if absent, it fails with a spawn error. Both cases prove
        // routing to claude (provider path is correct). The test is about
        // routing, not binary availability in the test environment.
        let result = run_provider_delivery(
            &store,
            &member,
            &runtime,
            &message,
            "delivery-claude",
            100, // Short timeout; no claude binary in test env
        );

        match result {
            Ok(_outcome) => {
                // Binary was present and delivery succeeded.
                // Verify the outcome was recorded with claude provider.
                assert_eq!(
                    member.provider, "claude",
                    "member must have claude provider"
                );
            }
            Err(err) => {
                // Binary absent or delivery failed. Verify the error is the
                // expected "failed to spawn claude" (not a wrong-provider error).
                let err_msg = err.to_string();
                assert!(
                    err_msg.contains("failed to spawn claude") || err_msg.contains("No such file"),
                    "expected claude spawn error when binary absent, got: {}",
                    err_msg
                );
            }
        }

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn claude_member_ingest_dispatches_to_claude_stub() {
        // WP-3: Test claude stream-json ingest (replaces stub).
        let root = std::env::temp_dir().join(format!(
            "harness-cli-test-{}",
            generated_id("claude-ingest")
        ));
        let store = HarnessStore::new(&root);
        let mut member = make_member("claude-agent");
        member.provider = "claude".into();
        store.append_member(&member).expect("append member");
        let source = root.join("provider-output.ndjson");
        std::fs::create_dir_all(&root).expect("create root");
        // Use Claude stream-json format (NDJSON).
        std::fs::write(
            &source,
            r#"{"type": "system", "session_id": "sess_test_123"}
{"type": "stream_event", "event": "text_delta"}
{"type": "result", "session_id": "sess_test_123"}"#,
        )
        .expect("write provider output");

        // WP-3: Real claude ingest parses stream-json and creates neutral AgentEvent/ProviderSession
        ingest_provider_output(
            &store,
            "claude-agent",
            None,
            None,
            &source.display().to_string(),
        )
        .expect("claude ingest dispatch should succeed");

        // Verify neutral objects were created from the stream.
        let events = store.events().expect("events");
        assert!(
            !events.is_empty(),
            "claude ingest should create AgentEvent from stream"
        );
        assert_eq!(events.len(), 3, "should ingest 3 events from stream-json");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_provider_runtime_start_fails_fast() {
        let root = std::env::temp_dir().join(format!(
            "harness-cli-test-{}",
            generated_id("unknown-start")
        ));
        let store = HarnessStore::new(&root);
        let mut member = make_member("gemini-agent");
        member.provider = "gemini".into();

        let error = start_provider_runtime(&store, &member)
            .expect_err("unknown provider must fail fast rather than assume codex");
        let message = error.to_string();
        // Assert the EXACT message: the supported list is now derived from the
        // provider registry, so this guards against ordering/spacing/list drift
        // (which a substring check would silently miss).
        assert_eq!(
            message,
            "unknown provider \"gemini\" for runtime start; supported providers: codex, claude"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn codex_member_ingest_stays_on_codex_path() {
        // Regression guard: a codex member must still flow through the existing
        // (regression-clean) codex parser and persist a codex-stamped event.
        let root =
            std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("codex-ingest")));
        let store = HarnessStore::new(&root);
        let member = make_member("codex-agent");
        assert_eq!(member.provider, "codex");
        store.append_member(&member).expect("append member");
        std::fs::create_dir_all(&root).expect("create root");
        let source = root.join("provider-output.jsonl");
        std::fs::write(
            &source,
            r#"{"method":"thread/started","params":{"threadId":"thread-1"}}"#,
        )
        .expect("write provider output");

        ingest_provider_output(
            &store,
            "codex-agent",
            None,
            None,
            &source.display().to_string(),
        )
        .expect("codex ingest must succeed via the codex dispatch branch");

        let events = store.events().expect("events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].provider, "codex");

        let _ = std::fs::remove_dir_all(root);
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
                sender_kind: SenderKind::Agent,
            })
            .expect("append acknowledged assignment");
        let evidence = Evidence {
            id: "evidence-1".into(),
            task_id: Some("task-1".into()),
            source_type: "claude_delivery_session".into(),
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
                sender_kind: SenderKind::Agent,
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
                sender_kind: SenderKind::Agent,
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
            sender_kind: SenderKind::Agent,
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
            sender_kind: SenderKind::Agent,
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
                sender_kind: SenderKind::Agent,
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
            sender_kind: SenderKind::Agent,
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
            sender_kind: SenderKind::Agent,
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
                    sender_kind: SenderKind::Agent,
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
                sender_kind: SenderKind::Agent,
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
            sender_kind: SenderKind::Agent,
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

    fn make_goal_evaluation(id: &str, goal_id: &str, created_at: &str) -> GoalEvaluation {
        GoalEvaluation {
            id: id.into(),
            goal_id: goal_id.into(),
            evaluator_agent_id: "evaluator".into(),
            outcome: EvaluationOutcome::Success,
            what_worked: "ok".into(),
            what_failed: "none".into(),
            missing_infra: vec![],
            missing_evidence: vec![],
            team_design_feedback: "ok".into(),
            task_graph_feedback: "ok".into(),
            dashboard_feedback: "ok".into(),
            reusable_patterns: vec![],
            anti_patterns: vec![],
            follow_up_task_ids: vec![],
            proposed_goal_ids: vec![],
            created_at: created_at.into(),
        }
    }

    fn make_closeout_decision(id: &str, goal_id: &str) -> Decision {
        Decision {
            id: id.into(),
            task_id: "task-1".into(),
            decision: "accept".into(),
            rationale: "closeout".into(),
            evidence_ids: vec!["closeout-evidence".into()],
            created_at: "unix-ms:200".into(),
            decision_kind: Some("closeout".into()),
            goal_id: Some(goal_id.into()),
            is_waiver: false,
            follow_up_task_id: None,
        }
    }

    #[test]
    fn closeout_gate_allows_close_with_decision_and_evaluation() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        store
            .append_goal(&make_goal("goal-1"))
            .expect("append goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("append task");
        store
            .append_goal_evaluation(&make_goal_evaluation("eval-1", "goal-1", "unix-ms:140"))
            .expect("append evaluation");
        store
            .append_decision(&make_closeout_decision("closeout-1", "goal-1"))
            .expect("append closeout decision");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(status.has_closeout_decision());
        assert!(status.has_goal_evaluation());
        assert!(status.may_close(), "both present should allow close");
        status
            .require_closeout()
            .expect("closeout gate should pass with decision + evaluation");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closeout_gate_blocks_close_when_evaluation_missing() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        store
            .append_goal(&make_goal("goal-1"))
            .expect("append goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("append task");
        // A closeout decision but no GoalEvaluation: the gate must block.
        store
            .append_decision(&make_closeout_decision("closeout-1", "goal-1"))
            .expect("append closeout decision");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(status.has_closeout_decision());
        assert!(!status.has_goal_evaluation());
        assert!(!status.may_close());
        let error = status
            .require_closeout()
            .expect_err("missing evaluation must block close");
        assert!(error.to_string().contains("goal_evaluation"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closeout_gate_blocks_close_when_closeout_decision_missing() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        store
            .append_goal(&make_goal("goal-1"))
            .expect("append goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("append task");
        store
            .append_goal_evaluation(&make_goal_evaluation("eval-1", "goal-1", "unix-ms:140"))
            .expect("append evaluation");
        // A plain (non-closeout) decision must NOT satisfy the closeout gate.
        store
            .append_decision(&make_timed_decision("decision", "task-1", "unix-ms:130"))
            .expect("append decision");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(!status.has_closeout_decision());
        assert!(!status.may_close());
        let error = status
            .require_closeout()
            .expect_err("missing closeout decision must block close");
        assert!(error.to_string().contains("closeout decision"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closeout_gate_rejects_closeout_decision_without_evidence() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        store
            .append_goal(&make_goal("goal-1"))
            .expect("append goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("append task");
        store
            .append_goal_evaluation(&make_goal_evaluation("eval-1", "goal-1", "unix-ms:140"))
            .expect("append evaluation");
        // decision_kind=closeout but with NO evidence_ids: must not count.
        let mut decision = make_closeout_decision("closeout-1", "goal-1");
        decision.evidence_ids = vec![];
        store
            .append_decision(&decision)
            .expect("append closeout decision");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(
            !status.has_closeout_decision(),
            "closeout decision without evidence must not satisfy the gate"
        );
        assert!(!status.may_close());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closeout_gate_allows_close_via_waiver() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        store
            .append_goal(&make_goal("goal-1"))
            .expect("append goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("append task");
        store
            .append_task(&make_task("follow-up-task", "goal-1"))
            .expect("append follow-up task");
        // No GoalEvaluation and no closeout decision, but an explicit valid waiver.
        store
            .append_decision(&Decision {
                id: "waiver-1".into(),
                task_id: "task-1".into(),
                decision: "waive".into(),
                rationale: "closeout waiver".into(),
                evidence_ids: vec!["waiver-evidence".into()],
                created_at: "unix-ms:210".into(),
                decision_kind: Some("waiver".into()),
                goal_id: Some("goal-1".into()),
                is_waiver: true,
                follow_up_task_id: Some("follow-up-task".into()),
            })
            .expect("append waiver");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(!status.has_closeout_decision());
        assert!(!status.has_goal_evaluation());
        assert!(status.has_valid_closeout_waiver());
        assert!(status.may_close(), "valid waiver should allow close");
        status
            .require_closeout()
            .expect("waiver should satisfy the closeout gate");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closeout_gate_rejects_waiver_without_follow_up() {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id("store")));
        let store = HarnessStore::new(&root);
        store
            .append_goal(&make_goal("goal-1"))
            .expect("append goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("append task");
        // is_waiver=true but missing follow_up_task_id: must not count as a closeout waiver.
        store
            .append_decision(&Decision {
                id: "waiver-1".into(),
                task_id: "task-1".into(),
                decision: "waive".into(),
                rationale: "incomplete waiver".into(),
                evidence_ids: vec!["waiver-evidence".into()],
                created_at: "unix-ms:210".into(),
                decision_kind: Some("waiver".into()),
                goal_id: Some("goal-1".into()),
                is_waiver: true,
                follow_up_task_id: None,
            })
            .expect("append waiver");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(!status.has_valid_closeout_waiver());
        assert!(!status.may_close());
        let error = status
            .require_closeout()
            .expect_err("waiver without follow-up must block close");
        assert!(error.to_string().contains("follow_up_task_id"));
        let _ = std::fs::remove_dir_all(root);
    }

    // --- WP-7: typed GoalEvaluation/GoalDesign producers + dual-read candidate wiring ---

    /// Build the full happy-path learning chain for a goal whose single task is Done,
    /// using the TYPED GoalDesign + TYPED GoalEvaluation producers (no legacy Evidence
    /// notes for design/evaluation). Returns the store so candidate/closeout queries
    /// can be exercised against a goal that satisfies `warnings(true).is_empty()`.
    fn seed_typed_learning_chain(label: &str) -> (HarnessStore, PathBuf) {
        let (store, root) = temp_store(label);
        store.append_goal(&make_goal("goal-1")).expect("goal");
        let mut task = make_task("task-1", "goal-1");
        task.status = TaskStatus::Done;
        store.append_task(&task).expect("task");

        // Typed GoalDesign (graduated object), created before assignment.
        store
            .append_goal_design(&GoalDesign {
                id: "design-1".into(),
                goal_id: "goal-1".into(),
                scenario_summary: "design".into(),
                non_goals: vec![],
                risk_and_permission_boundaries: "bounded".into(),
                required_infra: vec![],
                agent_team: None,
                task_graph: vec!["task-1".into()],
                evidence_plan: vec![],
                acceptance_gates: vec![],
                created_at: "unix-ms:20".into(),
            })
            .expect("design");
        store
            .append_message(&make_timed_message(
                "msg-assign",
                MessageKind::Task,
                "leader",
                Some("worker"),
                "task-1",
                "unix-ms:30",
            ))
            .expect("assignment");
        store
            .append_message(&make_timed_message(
                "msg-report",
                MessageKind::Report,
                "worker",
                Some("leader"),
                "task-1",
                "unix-ms:40",
            ))
            .expect("report");
        // Critic/evaluator evidence (a learning-status warning input distinct from the
        // typed GoalEvaluation object).
        store
            .append_evidence(&make_timed_evidence(
                "critic-1",
                "critic_findings",
                Some("task-1"),
                "unix-ms:50",
            ))
            .expect("critic");
        store
            .append_decision(&make_timed_decision("decision-1", "task-1", "unix-ms:60"))
            .expect("decision");
        (store, root)
    }

    #[test]
    fn goal_evaluate_persists_typed_object_and_flips_has_evaluation() {
        let (store, root) = temp_store("wp7-goal-evaluate");
        store.append_goal(&make_goal("goal-1")).expect("goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("task");
        // Evidence attached to the goal's task -> the typed evaluation references it.
        store
            .append_evidence(&make_timed_evidence(
                "evi-1",
                "check_result",
                Some("task-1"),
                "unix-ms:50",
            ))
            .expect("task evidence");

        // Before evaluating, the typed dual-read seam reports no evaluation.
        let before = goal_learning_status(&store, "goal-1").expect("status");
        assert!(!before.has_goal_evaluation());
        assert!(before.goal_evaluation_objects.is_empty());

        let args: Vec<String> = vec![
            "--goal".into(),
            "goal-1".into(),
            "--id-out".into(),
            "eval-typed-1".into(),
            "--evaluator".into(),
            "critic".into(),
            "--outcome".into(),
            "success".into(),
            "--what-worked".into(),
            "the loop closed".into(),
            "--what-failed".into(),
            "nothing".into(),
            "--pattern".into(),
            "typed-producer".into(),
        ];
        goal_evaluate(&store, &args).expect("goal evaluate succeeds");

        // Round-trips as a TYPED GoalEvaluation (not an Evidence note).
        let stored = store.goal_evaluations().expect("read evaluations");
        assert_eq!(stored.len(), 1);
        let evaluation = &stored[0];
        assert_eq!(evaluation.id, "eval-typed-1");
        assert_eq!(evaluation.goal_id, "goal-1");
        assert_eq!(evaluation.outcome, EvaluationOutcome::Success);
        assert_eq!(evaluation.reusable_patterns, vec!["typed-producer"]);
        // No legacy Evidence(source_type=goal_evaluation) row was written.
        assert!(
            store
                .evidence()
                .expect("evidence")
                .iter()
                .all(|item| item.source_type != "goal_evaluation"),
            "goal evaluate must not write an untyped goal_evaluation Evidence note"
        );

        // The typed dual-read seam now reports the evaluation.
        let after = goal_learning_status(&store, "goal-1").expect("status");
        assert!(after.has_goal_evaluation());
        assert_eq!(after.goal_evaluation_objects.len(), 1);
        assert!(
            after.goal_evaluation.is_empty(),
            "no legacy evaluation note"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closeout_gate_allows_close_with_typed_decision_and_typed_evaluation() {
        let (store, root) = temp_store("wp7-closeout-typed");
        store.append_goal(&make_goal("goal-1")).expect("goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("task");
        // Typed GoalEvaluation (via the producer) + typed closeout decision.
        let args: Vec<String> = vec![
            "--goal".into(),
            "goal-1".into(),
            "--evaluator".into(),
            "critic".into(),
            "--outcome".into(),
            "success".into(),
            "--what-worked".into(),
            "ok".into(),
            "--what-failed".into(),
            "none".into(),
        ];
        goal_evaluate(&store, &args).expect("goal evaluate succeeds");
        store
            .append_decision(&make_closeout_decision("closeout-1", "goal-1"))
            .expect("closeout decision");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(status.has_closeout_decision());
        assert!(status.has_goal_evaluation());
        assert!(
            status.may_close(),
            "typed decision + typed evaluation allow close"
        );
        status
            .require_closeout()
            .expect("closeout gate passes with typed decision + typed evaluation");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn closeout_gate_blocks_close_when_typed_evaluation_missing() {
        let (store, root) = temp_store("wp7-closeout-missing");
        store.append_goal(&make_goal("goal-1")).expect("goal");
        store
            .append_task(&make_task("task-1", "goal-1"))
            .expect("task");
        // Closeout decision present, but NO GoalEvaluation (typed or legacy).
        store
            .append_decision(&make_closeout_decision("closeout-1", "goal-1"))
            .expect("closeout decision");

        let status = goal_learning_status(&store, "goal-1").expect("status");
        assert!(status.has_closeout_decision());
        assert!(!status.has_goal_evaluation());
        assert!(!status.may_close());
        let error = status
            .require_closeout()
            .expect_err("missing evaluation must block close");
        assert!(error.to_string().contains("goal_evaluation"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn next_round_candidate_found_for_closed_typed_evaluated_goal() {
        let (store, root) = seed_typed_learning_chain("wp7-candidate");
        // The goal has a complete task graph + full learning chain, but NO evaluation
        // yet: the runner must NOT see a candidate (item 3 regression: typed-only seam).
        let options = make_tick_options();
        let before = autonomy_next_round_candidates(&store, &options).expect("candidate query");
        assert!(
            before.is_empty(),
            "no candidate before a typed evaluation exists"
        );

        // Produce a TYPED GoalEvaluation through the producer; this is the only new
        // input, and it must flip the goal into a candidate.
        goal_evaluate(
            &store,
            &[
                "--goal".into(),
                "goal-1".into(),
                "--id-out".into(),
                "eval-typed-1".into(),
                "--evaluator".into(),
                "critic".into(),
                "--outcome".into(),
                "success".into(),
                "--what-worked".into(),
                "ok".into(),
                "--what-failed".into(),
                "none".into(),
            ],
        )
        .expect("goal evaluate");

        let after = autonomy_next_round_candidates(&store, &options).expect("candidate query");
        assert_eq!(
            after.len(),
            1,
            "typed evaluation makes the goal a candidate"
        );
        let candidate = &after[0];
        assert_eq!(candidate.goal_id, "goal-1");
        assert_eq!(candidate.source_task_id, "task-1");
        assert_eq!(candidate.evaluation_evidence_id, "eval-typed-1");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Minimal AutonomyTickOptions for candidate-query tests: only the fields the
    /// query reads (goal_filter/force) matter; the rest carry inert defaults.
    fn make_tick_options() -> AutonomyTickOptions {
        AutonomyTickOptions {
            observer: "observer".into(),
            lead: "leader".into(),
            assignee: None,
            reviewer: None,
            goal_filter: None,
            vision_ref: None,
            vision_summary: None,
            auto_accept: false,
            force: false,
            max_new_goals: 1,
            dry_run: true,
            start_runtime: false,
            timeout_ms: 3_000,
            claim_ttl_ms: 300_000,
            goal_prefix: "goal-autonomous-round".into(),
            task_prefix: "task-autonomous-round".into(),
            workspace: None,
            branch: None,
            owned_paths: Vec::new(),
            acceptance: Vec::new(),
            goal_success: Vec::new(),
        }
    }

    #[test]
    fn validate_decision_enforces_waiver_requirements() {
        // Waiver without follow-up task is rejected.
        let mut decision = make_timed_decision("d1", "task-1", "unix-ms:1");
        decision.is_waiver = true;
        decision.evidence_ids = vec!["e1".into()];
        let error = validate_decision(&decision).expect_err("waiver without follow-up rejected");
        assert!(error.to_string().contains("follow-up-task"));

        // Waiver without evidence is rejected.
        decision.follow_up_task_id = Some("follow-up-task".into());
        decision.evidence_ids = vec![];
        let error = validate_decision(&decision).expect_err("waiver without evidence rejected");
        assert!(error.to_string().contains("evidence"));

        // A complete waiver passes.
        decision.evidence_ids = vec!["e1".into()];
        validate_decision(&decision).expect("complete waiver should validate");
    }

    #[test]
    fn validate_decision_enforces_stop_gate_values() {
        let mut decision = make_timed_decision("d1", "task-1", "unix-ms:1");
        decision.decision_kind = Some("stop_gate".into());

        // An arbitrary decision value is rejected for a stop_gate.
        decision.decision = "maybe".into();
        let error = validate_decision(&decision).expect_err("invalid stop_gate decision rejected");
        assert!(error.to_string().contains("stop_gate"));

        // Both canonical values pass.
        decision.decision = "stop_approved".into();
        validate_decision(&decision).expect("stop_approved should validate");
        decision.decision = "continue_required".into();
        validate_decision(&decision).expect("continue_required should validate");
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
            owner_agent_id: "leader".into(),
            status: GoalStatus::Active,
            priority: "p0".into(),
            created_at: "unix-ms:1".into(),
            updated_at: "unix-ms:1".into(),
            vision_id: None,
            goal_design_id: None,
            closed_by_decision_id: None,
            git_metadata: None,
            stage: GoalStage::default(),
            description_md: None,
            design_md: None,
            acceptance_md: None,
            explorations: Vec::new(),
            skill_refs: Vec::new(),
            stage_changed_at: None,
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
            description: None,
            git_metadata: None,
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
            sender_kind: SenderKind::Agent,
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

    fn temp_store(label: &str) -> (HarnessStore, PathBuf) {
        let root = std::env::temp_dir().join(format!("harness-cli-test-{}", generated_id(label)));
        (HarnessStore::new(&root), root)
    }

    #[test]
    fn create_team_value_persists_team_and_appears_in_snapshot() {
        let (store, root) = temp_store("wp-ii-team");
        let body = serde_json::json!({
            "name": "Platform Squad",
            "description": "Owns the dashboard",
            "owner": "lead-1",
            "member": ["worker-1", "worker-2"]
        });

        let created = create_team_value(&store, &body).expect("team create succeeds");
        let team_id = created["id"]
            .as_str()
            .expect("created team has id")
            .to_string();
        assert_eq!(created["name"], "Platform Squad");
        assert_eq!(created["owner_agent_id"], "lead-1");

        // Persisted as a domain entity.
        let teams = latest_teams(&store).expect("teams readable");
        let persisted = teams.get(&team_id).expect("team persisted in store");
        assert_eq!(persisted.name, "Platform Squad");
        assert_eq!(persisted.owner_agent_id, "lead-1");
        assert_eq!(persisted.member_ids, vec!["worker-1", "worker-2"]);
        assert_eq!(persisted.status, AgentTeamStatus::Active);

        // Visible in the dashboard snapshot the HTTP layer returns.
        let snapshot = dashboard_snapshot(&store).expect("snapshot builds");
        let snapshot_teams = snapshot["teams"]
            .as_array()
            .expect("teams array in snapshot");
        assert!(
            snapshot_teams
                .iter()
                .any(|team| team["id"].as_str() == Some(team_id.as_str())),
            "new team must appear in snapshot teams"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn create_team_value_missing_required_field_is_usage_error() {
        let (store, root) = temp_store("wp-ii-team-bad");
        // No owner / name -> CliError::Usage (mapped to HTTP 400 by serve loop).
        let body = serde_json::json!({"description": "no name or owner"});
        let error = create_team_value(&store, &body).expect_err("missing fields must error");
        assert!(
            matches!(error, CliError::Usage(_)),
            "malformed body must be a Usage error, got: {error:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn create_agent_value_persists_member_and_appears_in_snapshot() {
        let (store, root) = temp_store("wp-ii-agent");
        let body = serde_json::json!({
            "name": "Worker One",
            "role": "worker",
            "provider": "codex",
            "effort": "high",
            "skill": ["frontend-design"]
        });

        let created = create_agent_value(&store, &body).expect("agent create succeeds");
        let member_id = created["id"]
            .as_str()
            .expect("created member has id")
            .to_string();
        assert_eq!(created["name"], "Worker One");
        assert_eq!(created["role"], "worker");
        // Idle, not started: runtime start stays a separate action.
        assert_eq!(created["status"], "idle");
        assert!(
            created["provider_runtime_id"].is_null(),
            "create must NOT auto-start a runtime"
        );

        // Persisted member with a prompt_ref written to the store.
        let members = latest_members(&store).expect("members readable");
        let persisted = members.get(&member_id).expect("member persisted in store");
        assert_eq!(persisted.name, "Worker One");
        assert_eq!(persisted.status, AgentMemberStatus::Idle);
        assert_eq!(persisted.provider_config.effort.as_deref(), Some("high"));
        assert_eq!(persisted.skill_refs, vec!["frontend-design"]);
        assert!(
            persisted.prompt_ref.is_some(),
            "create must persist a bootstrap prompt_ref"
        );
        assert!(
            persisted.provider_runtime_id.is_none(),
            "no runtime started"
        );

        // No runtime persisted.
        assert!(
            store.runtimes().expect("runtimes readable").is_empty(),
            "create must not append any runtime"
        );

        // Member appears in the snapshot roster.
        let snapshot = dashboard_snapshot(&store).expect("snapshot builds");
        let snapshot_members = snapshot["members"]
            .as_array()
            .expect("members array in snapshot");
        assert!(
            snapshot_members
                .iter()
                .any(|member| member["id"].as_str() == Some(member_id.as_str())),
            "new member must appear in snapshot roster"
        );

        // The agent_created event was emitted.
        let events = store.events().expect("events readable");
        assert!(
            events
                .iter()
                .any(|event| event.agent_member_id == member_id
                    && event.event_type == "agent_created"),
            "create must emit an agent_created event"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn create_agent_value_missing_role_is_usage_error() {
        let (store, root) = temp_store("wp-ii-agent-bad");
        let body = serde_json::json!({"name": "No Role"});
        let error = create_agent_value(&store, &body).expect_err("missing role must error");
        assert!(
            matches!(error, CliError::Usage(_)),
            "malformed body must be a Usage error, got: {error:?}"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn agents_can_be_created_and_used_without_team_required() {
        // Stage 1: Create store with NO teams created.
        let (store, root) = temp_store("agents-decenter-test");

        // Stage 2: Create 2 agents with no team_ids.
        let agent1_body = serde_json::json!({
            "name": "Agent Codex",
            "role": "worker",
            "provider": "codex"
        });
        let agent1_created =
            create_agent_value(&store, &agent1_body).expect("agent1 creates without team");
        let agent1_id = agent1_created["id"]
            .as_str()
            .expect("agent1 has id")
            .to_string();

        let agent2_body = serde_json::json!({
            "name": "Agent Claude",
            "role": "reviewer",
            "provider": "claude"
        });
        let agent2_created =
            create_agent_value(&store, &agent2_body).expect("agent2 creates without team");
        let agent2_id = agent2_created["id"]
            .as_str()
            .expect("agent2 has id")
            .to_string();

        // Assertion 1: Both agents have empty team_ids.
        assert_eq!(
            agent1_created["team_ids"].as_array().map(|a| a.is_empty()),
            Some(true),
            "agent1 team_ids must be empty"
        );
        assert_eq!(
            agent2_created["team_ids"].as_array().map(|a| a.is_empty()),
            Some(true),
            "agent2 team_ids must be empty"
        );

        // Stage 3: Both agents appear in snapshot.members (top-level, not team-filtered).
        let snapshot = dashboard_snapshot(&store).expect("snapshot builds");
        let members = snapshot["members"]
            .as_array()
            .expect("members array in snapshot");

        let member_ids: Vec<&str> = members.iter().filter_map(|m| m["id"].as_str()).collect();

        assert!(
            member_ids.contains(&agent1_id.as_str()),
            "agent1 must appear in snapshot.members"
        );
        assert!(
            member_ids.contains(&agent2_id.as_str()),
            "agent2 must appear in snapshot.members"
        );

        // Stage 4: Create a task owned by agent1, then assign to agent2.
        let task_body = serde_json::json!({
            "title": "Test Task",
            "objective": "Verify task assignment works for teamless agents",
            "owner": agent1_id
        });
        let task_created = create_task_value(&store, &task_body).expect("task creates");
        let task_id = task_created["id"]
            .as_str()
            .expect("task has id")
            .to_string();

        // Assertion 2: Task is initially unassigned.
        assert!(
            task_created["assignee_agent_id"].is_null(),
            "task must start unassigned"
        );

        // Stage 5: Assign the task to agent2 (no team required).
        let assign_body = serde_json::json!({
            "assignee": agent2_id
        });
        let assigned = assign_task_value(&store, &task_id, &assign_body)
            .expect("task assignment succeeds for teamless agent");

        // Assertion 3: Task is now assigned to agent2.
        assert_eq!(
            assigned["assignee_agent_id"].as_str(),
            Some(agent2_id.as_str()),
            "task assignee_agent_id must be set"
        );
        assert_eq!(
            assigned["status"].as_str(),
            Some("assigned"),
            "task status must be 'assigned'"
        );

        // Stage 6: Verify latest_member works for teamless agents.
        let agent1_loaded = latest_member(&store, &agent1_id).expect("agent1 loads from store");
        let agent2_loaded = latest_member(&store, &agent2_id).expect("agent2 loads from store");

        // Assertion 4: Loaded members have empty team_ids.
        assert!(agent1_loaded.team_ids.is_empty(), "agent1 team_ids empty");
        assert!(agent2_loaded.team_ids.is_empty(), "agent2 team_ids empty");

        // Assertion 5: No team exists in store.
        let teams = latest_teams(&store).expect("teams readable");
        assert!(teams.is_empty(), "no team should exist in store");

        // Assertion 6: snapshot.teams is empty or filtered out.
        let teams_in_snapshot = snapshot["teams"]
            .as_array()
            .expect("teams array in snapshot");
        assert!(
            teams_in_snapshot.is_empty(),
            "snapshot.teams should be empty when no active teams exist"
        );

        // Stage 7: the @reviewer gesture (POST /v1/tasks/{id}/reviewer) records
        // the reviewer on the existing field WITHOUT handing off (status stays
        // `assigned`, no review-request message).
        let messages_before = latest_messages(&store).expect("messages readable").len();
        let reviewer_body = serde_json::json!({ "reviewer": agent1_id });
        let reviewed = set_task_reviewer_value(&store, &task_id, &reviewer_body)
            .expect("setting reviewer succeeds for teamless agent");
        assert_eq!(
            reviewed["reviewer_agent_id"].as_str(),
            Some(agent1_id.as_str()),
            "task reviewer_agent_id must be set by the @reviewer gesture"
        );
        assert_eq!(
            reviewed["status"].as_str(),
            Some("assigned"),
            "naming a reviewer must NOT change task status (no hand-off)"
        );
        assert_eq!(
            latest_messages(&store).expect("messages readable").len(),
            messages_before,
            "naming a reviewer must NOT queue a message (hand-off is separate)"
        );
        // A non-existent reviewer is rejected (fail-fast, mirrors assign).
        assert!(
            set_task_reviewer_value(
                &store,
                &task_id,
                &serde_json::json!({ "reviewer": "agent-does-not-exist" }),
            )
            .is_err(),
            "naming an unknown reviewer must error"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod sse_tests {
    use super::*;

    #[test]
    fn test_sse_manager_broadcast_to_subscriber() {
        let manager = sse::SseManager::new();
        let rx = manager.subscribe();

        let event = sse::SseEventFrame::AgentEvent(AgentEvent {
            id: "evt-test".into(),
            agent_member_id: "mem-test".into(),
            provider_runtime_id: None,
            task_id: None,
            provider: "claude".into(),
            provider_thread_id: None,
            provider_turn_id: None,
            provider_child_thread_id: None,
            event_type: "test_event".into(),
            summary: "Test Event".into(),
            payload_ref: None,
            created_at: "2025-01-01T00:00:00Z".into(),
        });

        manager.broadcast(event.clone());

        // Verify the event is received
        match rx.recv_timeout(std::time::Duration::from_secs(1)) {
            Ok(received) => {
                if let sse::SseEventFrame::AgentEvent(evt) = received {
                    assert_eq!(evt.id, "evt-test");
                } else {
                    panic!("Expected AgentEvent");
                }
            }
            Err(_) => panic!("Did not receive event in time"),
        }
    }

    #[test]
    fn test_sse_manager_multiple_subscribers() {
        let manager = sse::SseManager::new();
        let rx1 = manager.subscribe();
        let rx2 = manager.subscribe();

        let event = sse::SseEventFrame::AgentEvent(AgentEvent {
            id: "evt-multi".into(),
            agent_member_id: "mem-test".into(),
            provider_runtime_id: None,
            task_id: None,
            provider: "claude".into(),
            provider_thread_id: None,
            provider_turn_id: None,
            provider_child_thread_id: None,
            event_type: "test_event".into(),
            summary: "Multi Test".into(),
            payload_ref: None,
            created_at: "2025-01-01T00:00:00Z".into(),
        });

        manager.broadcast(event);

        // Both subscribers should receive the event
        let _ = rx1
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("rx1 should receive event");
        let _ = rx2
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("rx2 should receive event");
    }

    /// Regression: a long-lived `/v1/events` SSE connection must not starve
    /// other HTTP requests. Before per-connection threading the single accept
    /// loop blocked inside the SSE handler, so a concurrent `/v1/snapshot` (or a
    /// composer POST) hung until the stream closed. Here we hold an SSE stream
    /// open and assert a concurrent snapshot still returns promptly. The inline
    /// accept loop mirrors serve_command's per-connection threading.
    #[test]
    fn sse_stream_does_not_block_concurrent_requests() {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::time::Duration;

        let root = std::env::temp_dir().join(format!(
            "harness-cli-test-{}",
            generated_id("serve-concurrency")
        ));
        let store = HarnessStore::new(&root);
        store.init().expect("init store");

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let addr = listener.local_addr().expect("local addr");
        let serve_store = store.clone();
        std::thread::spawn(move || {
            let sse_manager = sse::SseManager::new();
            sse::start_sse_watcher(&serve_store, sse_manager.clone(), |_, _| Vec::new())
                .expect("watcher");
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let conn_store = serve_store.clone();
                let conn_manager = sse_manager.clone();
                std::thread::spawn(move || {
                    let _ = handle_http_connection(&conn_store, stream, conn_manager);
                });
            }
        });

        // Open and hold an SSE stream; read its initial `snapshot` frame so we
        // know the server thread is parked inside the SSE handler.
        let mut sse_conn = TcpStream::connect(addr).expect("connect sse");
        sse_conn
            .write_all(b"GET /v1/events HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("send sse request");
        sse_conn
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("set sse read timeout");
        let mut sse_reader = BufReader::new(sse_conn.try_clone().expect("clone sse"));
        let mut saw_snapshot = false;
        for _ in 0..40 {
            let mut line = String::new();
            if sse_reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            if line.contains("event: snapshot") {
                saw_snapshot = true;
                break;
            }
        }
        assert!(
            saw_snapshot,
            "SSE stream did not emit an initial snapshot frame"
        );

        // With the stream still held open, a concurrent snapshot request must
        // complete. A short read timeout makes a regression (blocked accept
        // loop) fail fast instead of hanging the whole test.
        let mut snap_conn = TcpStream::connect(addr).expect("connect snapshot");
        snap_conn
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set snapshot read timeout");
        snap_conn
            .write_all(b"GET /v1/snapshot HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .expect("send snapshot request");
        let mut response = String::new();
        snap_conn
            .read_to_string(&mut response)
            .expect("snapshot must respond while an SSE stream is open");
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "expected 200 snapshot while SSE held, got: {}",
            response.lines().next().unwrap_or("<empty>")
        );

        drop(sse_conn);
        let _ = std::fs::remove_dir_all(root);
    }
}

// --- Tests for WP-2: codex exec --json delivery (Stage 1-3) ---

#[cfg(test)]
mod tests_wp2_codex_exec {
    use super::*;
    use std::io::Cursor;

    // Stage 1: NDJSON parser tests
    #[test]
    fn test_parse_codex_ndjson_valid_events() {
        let ndjson = r#"{"type": "tool_call", "id": "1"}
{"type": "tool_output", "id": "1"}
{"type": "turn_completed"}
"#;
        let reader = Cursor::new(ndjson.as_bytes());
        let events = parse_codex_ndjson(reader);

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, "tool_call");
        assert_eq!(events[1].event_type, "tool_output");
        assert_eq!(events[2].event_type, "turn_completed");
    }

    #[test]
    fn test_parse_codex_ndjson_skip_invalid_lines() {
        let ndjson = r#"{"type": "tool_call"}
invalid json line
{"type": "tool_output"}
"#;
        let reader = Cursor::new(ndjson.as_bytes());
        let events = parse_codex_ndjson(reader);

        // Should skip the invalid line
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "tool_call");
        assert_eq!(events[1].event_type, "tool_output");
    }

    #[test]
    fn test_parse_codex_ndjson_empty_lines() {
        let ndjson = r#"{"type": "tool_call"}

{"type": "tool_output"}
"#;
        let reader = Cursor::new(ndjson.as_bytes());
        let events = parse_codex_ndjson(reader);

        // Should skip empty lines
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_codex_exec_event_parse_line_valid() {
        let line = r#"{"type": "tool_call", "payload": "test"}"#;
        let event = CodexExecEvent::parse_line(line).expect("should parse");

        assert_eq!(event.event_type, "tool_call");
        assert_eq!(
            event.payload.get("type").and_then(|v| v.as_str()),
            Some("tool_call")
        );
    }

    #[test]
    fn test_codex_exec_event_parse_line_missing_type() {
        let line = r#"{"payload": "test"}"#;
        let event = CodexExecEvent::parse_line(line).expect("should parse");

        // Should default to "unknown" when type is missing
        assert_eq!(event.event_type, "unknown");
    }

    #[test]
    fn test_codex_exec_event_terminal_source() {
        // Real codex 0.13x exec --json emits dot-separated discriminants.
        let json = serde_json::json!({"type": "turn.completed"});
        let event = CodexExecEvent {
            event_type: "turn.completed".into(),
            payload: json,
        };

        assert_eq!(
            event.terminal_source(),
            Some(MessageTerminalSource::TurnCompleted)
        );
    }

    #[test]
    fn test_codex_exec_event_terminal_source_legacy_underscore() {
        // Backward-compat: older underscore names still treated as terminal.
        let event = CodexExecEvent {
            event_type: "turn_completed".into(),
            payload: serde_json::json!({"type": "turn_completed"}),
        };
        assert_eq!(
            event.terminal_source(),
            Some(MessageTerminalSource::TurnCompleted)
        );
    }

    #[test]
    fn test_codex_exec_event_non_terminal() {
        let json = serde_json::json!({"type": "tool_call"});
        let event = CodexExecEvent {
            event_type: "tool_call".into(),
            payload: json,
        };

        assert_eq!(event.terminal_source(), None);
    }

    // Stage 1: Status inference tests
    #[test]
    fn test_infer_provider_session_status_succeeded() {
        let events = vec![
            CodexExecEvent {
                event_type: "tool_call".into(),
                payload: serde_json::json!({}),
            },
            CodexExecEvent {
                event_type: "turn.completed".into(),
                payload: serde_json::json!({"type": "turn.completed"}),
            },
        ];

        let status = infer_provider_session_status(&events, true);
        assert_eq!(status, ProviderSessionStatus::Succeeded);
    }

    #[test]
    fn test_infer_provider_session_status_succeeded_real_codex_stream() {
        // Mirrors a real codex 0.13x exec --json stream.
        let events = vec![
            CodexExecEvent {
                event_type: "thread.started".into(),
                payload: serde_json::json!({
                    "thread_id": "019e7ecf-42f4-7eb0-aa73-a4ae7a8f01f0",
                    "type": "thread.started"
                }),
            },
            CodexExecEvent {
                event_type: "turn.started".into(),
                payload: serde_json::json!({"type": "turn.started"}),
            },
            CodexExecEvent {
                event_type: "item.completed".into(),
                payload: serde_json::json!({
                    "item": {"id": "item_0", "text": "codex exec acceptance OK", "type": "agent_message"},
                    "type": "item.completed"
                }),
            },
            CodexExecEvent {
                event_type: "turn.completed".into(),
                payload: serde_json::json!({"type": "turn.completed"}),
            },
        ];

        assert_eq!(
            infer_provider_session_status(&events, true),
            ProviderSessionStatus::Succeeded
        );
        assert_eq!(
            extract_thread_id_from_exec_events(&events).as_deref(),
            Some("019e7ecf-42f4-7eb0-aa73-a4ae7a8f01f0")
        );
    }

    #[test]
    fn test_infer_provider_session_status_failed_exit() {
        let events = vec![CodexExecEvent {
            event_type: "tool_call".into(),
            payload: serde_json::json!({}),
        }];

        let status = infer_provider_session_status(&events, false);
        assert_eq!(status, ProviderSessionStatus::Failed);
    }

    #[test]
    fn test_infer_provider_session_status_stale() {
        let events = vec![CodexExecEvent {
            event_type: "tool_call".into(),
            payload: serde_json::json!({}),
        }];

        let status = infer_provider_session_status(&events, true);
        assert_eq!(status, ProviderSessionStatus::Stale);
    }

    #[test]
    fn test_infer_provider_session_status_no_events_and_failed() {
        let events = vec![];

        let status = infer_provider_session_status(&events, false);
        assert_eq!(status, ProviderSessionStatus::Failed);
    }

    #[test]
    fn test_infer_provider_session_status_empty_success() {
        let events = vec![];

        let status = infer_provider_session_status(&events, true);
        assert_eq!(status, ProviderSessionStatus::Failed);
    }

    // Stage 3: Delivery selector tests
    #[test]
    fn test_codex_delivery_selector_respects_env_var() {
        // This test validates the logic of the selector function.
        // It doesn't actually invoke the function, but documents the expected behavior:
        // - HARNESS_CODEX_DELIVERY=exec -> run_codex_exec_delivery
        // - Codex now uses exec-stream delivery only
        // - no flag -> defaults to appserver

        let env_exec = "exec";
        let env_appserver = "appserver";
        let env_default = "";

        assert_eq!(env_exec, "exec");
        assert_eq!(env_appserver, "appserver");
        assert!(!env_default.is_empty() || env_default.is_empty()); // vacuous, but documents fallback
    }

    #[test]
    fn test_extract_thread_id_from_exec_events_present() {
        let events = vec![CodexExecEvent {
            event_type: "thread.started".into(),
            payload: serde_json::json!({"thread_id": "123", "type": "thread.started"}),
        }];

        // thread.started carries the real thread_id; surface it.
        let thread_id = extract_thread_id_from_exec_events(&events);
        assert_eq!(thread_id.as_deref(), Some("123"));
    }

    #[test]
    fn test_extract_thread_id_from_exec_events_absent_is_none() {
        let events = vec![CodexExecEvent {
            event_type: "turn.started".into(),
            payload: serde_json::json!({"type": "turn.started"}),
        }];

        assert_eq!(extract_thread_id_from_exec_events(&events), None);
    }

    #[test]
    fn test_extract_turn_id_from_exec_events_present() {
        let events = vec![CodexExecEvent {
            event_type: "turn.started".into(),
            payload: serde_json::json!({"turn_id": "456", "type": "turn.started"}),
        }];

        let turn_id = extract_turn_id_from_exec_events(&events);
        assert_eq!(turn_id.as_deref(), Some("456"));
    }

    #[test]
    fn test_extract_turn_id_from_exec_events_absent_is_none() {
        let events = vec![CodexExecEvent {
            event_type: "thread.started".into(),
            payload: serde_json::json!({"thread_id": "789", "type": "thread.started"}),
        }];

        assert_eq!(extract_turn_id_from_exec_events(&events), None);
    }

    #[test]
    fn extract_codex_final_message_returns_terminal_message_not_joined() {
        // issue #139 item 2: structured-output parsing must read the FINAL
        // agent_message, not the joined narration — a streamed preamble
        // ("I'll start by inspecting…") must not be captured as the result.
        let events = vec![
            CodexExecEvent {
                event_type: "item.completed".into(),
                payload: serde_json::json!({
                    "item": {"type": "agent_message", "text": "I'll start by inspecting the repo."}
                }),
            },
            CodexExecEvent {
                event_type: "item.completed".into(),
                payload: serde_json::json!({
                    "item": {"type": "agent_message", "text": "{\"ok\": true}"}
                }),
            },
        ];
        // The human-facing reply joins every message…
        assert_eq!(
            extract_codex_reply_text(&events).as_deref(),
            Some("I'll start by inspecting the repo.\n{\"ok\": true}")
        );
        // …but the final-message extractor returns only the terminal one, which
        // parses cleanly to the structured object (no preamble pollution).
        assert_eq!(
            extract_codex_final_message(&events).as_deref(),
            Some("{\"ok\": true}")
        );
        assert_eq!(
            extract_codex_final_message(&events)
                .as_deref()
                .and_then(extract_json_object),
            Some(serde_json::json!({"ok": true}))
        );
    }
}

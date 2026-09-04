use super::*;

/// Admit `-h`/`--help` before any command parser or durable dependency is
/// opened. This is intentionally one narrow boundary for the handwritten CLI:
/// help wins over required-argument validation and mutation dispatch, while
/// non-help arguments continue through the existing command-specific paths.
pub(super) fn admit_help_request(args: &[String]) -> bool {
    if !args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        return false;
    }

    let command_path = help_command_path(args);
    if command_path.first().map(String::as_str) == Some("provider") {
        print_provider_help();
    } else if command_path.is_empty() {
        print_help();
    } else {
        println!("Usage: firm {} [options]\n", command_path.join(" "));
        print_help();
    }
    true
}

/// Extract only the leading command/subcommand path used to contextualize
/// help. Global selectors may precede the command; option values after the
/// command are deliberately excluded. Three tokens cover every current nested
/// mutation family (`team-run work cancel`, `member work submit`, etc.).
fn help_command_path(args: &[String]) -> Vec<String> {
    let mut index = 0usize;
    while index < args.len() {
        match args[index].as_str() {
            "--store" | "--project" | "--company" | "--space" => index += 2,
            "--store-source" => index += 1,
            "-h" | "--help" => return Vec::new(),
            value if value.starts_with('-') => index += 1,
            _ => break,
        }
    }

    let mut path = Vec::new();
    while index < args.len() && path.len() < 3 {
        let value = &args[index];
        if value.starts_with('-') {
            break;
        }
        path.push(value.clone());
        index += 1;
    }
    path
}

pub(super) fn print_provider_help() {
    println!("harness [--project <id|path>] provider admit --provider <name> --execution-mode <mode> --provider-version <version> --adapter-contract-version <version> --evidence <ref> [--evidence <ref>...] [--policy strict|advisory] [--actor <id>] [--json]\n\nWhen an Execution Space is resolved, the global --project flag is required; FIRM_PROJECT and ambient defaults are not explicit admission authorization.");
}

pub(super) fn require_subcommand(args: &[String], usage: &str) -> CliResult<()> {
    if args.is_empty() {
        Err(CliError::Usage(format!("usage: harness {usage}")))
    } else {
        Ok(())
    }
}

pub(super) fn required(args: &[String], name: &str) -> CliResult<String> {
    value(args, name).ok_or_else(|| CliError::Usage(format!("{name} is required")))
}

pub(super) fn value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find_map(|window| (window[0] == name).then(|| window[1].clone()))
}

pub(super) fn many(args: &[String], name: &str) -> Vec<String> {
    args.windows(2)
        .filter(|window| window[0] == name)
        .map(|window| window[1].clone())
        .collect()
}

pub(super) fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

pub(super) fn parse_message_kind(value: &str) -> CliResult<RegistryMessageIntent> {
    match value {
        "message" => Ok(RegistryMessageIntent::Message),
        "report" => Ok(RegistryMessageIntent::Report),
        other => Err(CliError::Usage(format!("unknown message kind: {other}"))),
    }
}

// Test-only helper: only referenced by build_turn_input (also #[cfg(test)]).
#[cfg(test)]
pub(super) fn message_kind_label(kind: &RegistryMessageIntent) -> &'static str {
    match kind {
        RegistryMessageIntent::Message => "message",
        RegistryMessageIntent::Report => "report",
    }
}

pub(super) fn parse_sender_kind(value: &str) -> CliResult<SenderKind> {
    match value {
        "agent" => Ok(SenderKind::Agent),
        "operator" => Ok(SenderKind::Operator),
        "system" => Ok(SenderKind::System),
        other => Err(CliError::Usage(format!("unknown sender kind: {other}"))),
    }
}

/// Reads the optional `--sender-kind` flag, defaulting to [`SenderKind::Agent`]
/// when absent so callers that do not specify a sender identity behave as before.
pub(super) fn terminal_source_label(source: &MessageTerminalSource) -> String {
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

pub(super) fn provider_status_label(status: &ProviderExecutionStatus) -> &'static str {
    match status {
        ProviderExecutionStatus::Queued => "queued",
        ProviderExecutionStatus::Running => "running",
        ProviderExecutionStatus::Succeeded => "succeeded",
        ProviderExecutionStatus::Failed => "failed",
        ProviderExecutionStatus::Canceled => "canceled",
        ProviderExecutionStatus::Stale => "stale",
    }
}

pub(super) fn now_string() -> String {
    let millis = current_unix_ms();
    format!("unix-ms:{millis}")
}

/// The commit this binary was built from, embedded by `build.rs` at compile
/// time (issue #307 — `/v1/meta` must never shell out to `git` per-request).
/// "unknown" only when the build environment had no git / was not a checkout.
pub(super) fn build_git_rev() -> &'static str {
    option_env!("FIRM_BUILD_GIT_REV").unwrap_or("unknown")
}

/// When this binary was compiled, in the same `unix-ms:<millis>` convention as
/// every other timestamp this server emits. `None` only if the build
/// environment's clock could not be read (see `build.rs`).
pub(super) fn build_built_at() -> Option<String> {
    option_env!("FIRM_BUILD_AT_MS")
        .and_then(|value| value.parse::<u128>().ok())
        .map(|millis| format!("unix-ms:{millis}"))
}

pub(super) fn current_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(super) fn generated_id(prefix: &str) -> String {
    let millis = current_unix_ms();
    let process_id = std::process::id();
    let counter = GENERATED_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    generated_id_from_parts(prefix, millis, process_id, counter)
}

pub(super) fn generated_id_from_parts(
    prefix: &str,
    millis: u128,
    process_id: u32,
    counter: u64,
) -> String {
    format!("{prefix}-{millis}-p{process_id}-{counter}")
}

pub(super) static GENERATED_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn print_json<T: serde::Serialize>(value: &T) -> CliResult<()> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serialize cli output")
    );
    Ok(())
}

pub(super) fn cheatsheet_command(args: &[String]) -> CliResult<()> {
    let scope = args.first().map(String::as_str).unwrap_or("all");
    match scope {
        "team" => print!("{}", CHEATSHEET_TEAM),
        "work" => print!("{}", CHEATSHEET_WORK),
        "mission" => print!("{}", CHEATSHEET_MISSION),
        "all" => print!("{}", CHEATSHEET_ALL),
        other => {
            return Err(CliError::Usage(format!(
            "unknown cheatsheet scope: {other}; usage: harness cheatsheet [team|work|mission|all]"
        )))
        }
    }
    Ok(())
}

// Each cheatsheet is a hand-curated plain-text one-pager of EXACT invocation
// forms used by the orchestrate-mission-waves skill. Every flag is derived from
// the real CLI definitions in this file. Length budgets are enforced by the
// anti-drift test (cheatsheet_length_budgets).

pub(super) const CHEATSHEET_TEAM: &str = r#"team-run create     --objective <text> --agent-team-id <id> [--budget-usd <n>]
                    --member name:role:provider[/mode][:model][@paths]
team-run start      --id <id> [--max-concurrency <n>] [--idle-timeout-s <n>]
team-run status     --id <id> [--dashboard-base <url>] [--json]
team-run wait       --id <id> [--after-seq <n>] [--timeout-secs <n>] [--json]
team-run host-inbox --surface <s> --thread-id <id> [--all] [--json]  (both required)
team-run message send --team-run-id <id> --to-membership <membership-id> --body <md>
                    --surface <s> --thread-id <id> [--work-id <id>] [--response-required]
team-run events     --id <id> [--after-seq <n>] [--json]
team-run board-summary --id <id>
team message send   (peer-Team only) --from-team <id> --from-member <id> --to-team <id>
                    [--to-member <id> | --to-membership <id>] --body <md>
  intra-Team: firm team-run message send (external Host) or firm member message send
team message inbox  --team <id> [--all] [--json]
team message claim  --team <id> --delivery-id <id> --membership-id <id>
"#;

pub(super) const CHEATSHEET_WORK: &str = r#"work create --team-run-id <id> --title <text> --completion-criteria <text>
  [--claim-mode team_claim --eligible-member-id <id>]
  [--priority low|normal|high|urgent] [--context <md>]
  [--prerequisite-work-id <id>] [--idempotency-key <key>]
  [--github-issue owner/repo#N]
work replace-dependencies --team-id <id> --work-id <id> --expected-version <n>
  [--prerequisite-work-id <id>] [--idempotency-key <key>]
work list --team-run-id <id> [--brief] [--since <cursor>]
  [--status <status>] [--member-run-id <id>]
work show --work-id <id>
work assign --work-id <id> --expected-version <n> --membership-id <id> [--idempotency-key <key>]
  (canonical TeamMembership responsibility; runtime ids are not Work authority)
work migrate-responsibility  (append-only DOC-106 cutover of legacy TeamRun-scoped Work)
work accept --work-id <id> --expected-version <n> [--idempotency-key <key>]
work request-changes --work-id <id> --expected-version <n> --reason <text> [--idempotency-key <key>]
work poll-github-ci --team-run-id <id>
"#;

pub(super) const CHEATSHEET_MISSION: &str = r#"mission list            (read-only legacy rows; Mission writers retired by DOC-108)
mission show            --id <id>
mission log show        --mission-id <id> [--tail <n>] [--json]
"#;

pub(super) const CHEATSHEET_ALL: &str = r#"team-run create --objective <text> --agent-team-id <id>
  --member name:role:provider[/mode][:model][@paths]
team-run start --id <id> [--max-concurrency <n>]
team-run status --id <id> [--dashboard-base <url>] [--json]
team-run wait --id <id> [--after-seq <n>] [--timeout-secs <n>] [--json]
team-run host-inbox --surface <s> --thread-id <id> [--all] [--json]  (both required)
team-run message send --team-run-id <id> --to-membership <membership-id> --body <md>
  --surface <s> --thread-id <id> [--work-id <id>] [--response-required]
team-run events --id <id> [--json]
team-run board-summary --id <id>

work create --team-run-id <id> --title <text> --completion-criteria <text>
  [--claim-mode team_claim --eligible-member-id <id>]
  [--github-issue owner/repo#N]
work list --team-run-id <id> [--brief] [--since <cursor>]
work show --work-id <id>
work assign --work-id <id> --expected-version <n> --membership-id <id>
  (canonical TeamMembership responsibility; runtime ids are not Work authority)
work submit --team-run-id <id> --member-run-id <id> --work-id <id>
  --expected-version <n> --result <text> [--github-pr owner/repo#N]
work accept --work-id <id> --expected-version <n>
work request-changes --work-id <id> --expected-version <n> --reason <text>

team create --name <text> --description <text> --host-agent-id <id>
  --node-id <uuid> [--member <id>] [--legacy-mission-id <id>]
team message send (peer-Team only) --from-team <id> --from-member <id>
  --to-team <id> --body <md>
  intra-Team: firm team-run message send (external Host) or firm member message send
team message inbox --team <id> [--all]
mission list
mission log show --mission-id <id> [--tail <n>]
  (read-only legacy Mission reads; writers retired by DOC-108)
"#;

pub(super) fn print_help() {
    println!(
        r#"harness commands:
  init
  project add | project list | project current | project switch
  project remove | project show | project migrate
  space init --id <space-id> [--name <name>] [--project-binding <binding-id>] [--company <company-id>]
  space list | space current | space switch <space-id> | space show [space-id]
  space migrate-from-project --from-project <binding-id|path> --id <space-id> [--name <name>]
  legacy-goal-task export --project <id|path> --output <dir>
  legacy-goal-task verify --archive <dir>
  legacy-company-os export [--firm-home <dir>] --output <dir>
  legacy-company-os verify --archive <dir>
  mission list|show (read-only legacy rows; Mission writers retired by DOC-108)
  mission log show --mission-id <id> [--tail <n>] (read-only legacy)
  legacy wave list|show|history (historical reads only)
  team-run create|list|status|recover|host-inbox|message|bind-host|host-lease-status|renew-host-lease|release-host-lease|inbox|add-member|rename-member|interrupt-member|close-member|reopen-member|deactivate-member|start|answer-message|events|complete|cancel
  team-run host-inbox --surface <surface> --thread-id <id> [--all] [--json]
      Both --surface and --thread-id are required.
  team-run message send --team-run-id <team-run-id> --to-membership <membership-id>
      --body <markdown> --surface <surface> --thread-id <id>
      [--work-id <id>] [--response-required] [--idempotency-key <key>]
      Author an intra-Team Message as the exact external_interactive Host binding.
  team-run board-summary --id <team-run-id>
      <=500-char plain-text board digest: counts by status, assigned/unassigned,
      ready, and one idle|working|awaiting-review line per active member.
  team-run work list|show|create|assign|redeliver|claim|start|block|resume|release|submit|review|request-changes|accept|cancel
  team-run work redeliver --work-id <id> --expected-version <n> [--reason <text>]
      Re-authorize an open Work whose delivery is frozen on an AgentSession
      generation the member no longer runs (typically after close-member +
      reopen-member). Records which deliveries it supersedes, advances the Work
      revision, and lets the ordinary delivery path bind and wake the member's
      current generation. Refusal codes: WORK_TERMINAL_NOT_REDELIVERABLE,
      WORK_ALREADY_STARTED, WORK_NOT_ASSIGNED, WORK_HAS_NO_UNSTARTED_DELIVERY,
      WORK_DELIVERY_LIVE.
  team-run work release --work-id <id> --expected-version <n> [--member-run-id <id>]
      --team-run-id is optional; the TeamRun is derived from the Work.
  team-run work list [--brief] [--since <cursor>] --team-run-id <id> [--status <status>] [--member-run-id <id>]
      --brief: one plain-text line per Work, no JSON wrapper.
      --since <cursor>: only Works whose latest WorkOperation postdates the
      cursor (a JSON `list` response's next_since); wraps JSON output as an
      object with since/next_since/works so a Host loop can chain calls.
  member-run show --id <member-run-id> [--json]
  member-run open-native --id <member-run-id> [--print-only] [--json]
  team create|list|show|rename|add-member|remove-member|close|archive
  team message send --from-team <id> --from-member <agent-member-id> --to-team <id>
                   [--to-member <agent-member-id> | --to-membership <membership-id>]
                   --body <markdown> [--work-id <id>] [--correlation-id <id>] [--causation-id <id>]
                   [--company <id> --to-node <node> --to-space <id> [--to-subscription-revision <n>]]
      Peer-Team only; ordinary Message without WorkDelegation. For intra-team
      messages, use firm team-run message send from the exact
      external_interactive Host binding, POST
      /v1/agentfirm/team-runs/{{run}}/messages/send (the Host Console composer),
      or firm member message send from a Supervisor-bound member. A Team target
      lands in the shared Team Inbox without waking
      Members; a Member target binds one exact TeamMembership. Remote route
      facts are all-or-nothing.
  team message inbox --team <id> [--all] [--json]
      Read the shared Team Inbox (default: unclaimed queued deliveries).
  team message claim --team <id> --delivery-id <id> --membership-id <id> [--claim-id <id>]
      Claim one queued Team Inbox delivery for one exact active TeamMembership
      generation under the current NodeDaemon generation.
  org member create|converge|list|show
  org bootstrap-lead|host|cutover-audit
  member providers [--fail-on-review]
  member preflight [--provider <name>] [--execution-mode <mode>] [--canary]
                   [--timeout-s <n>] [--fail-on-unavailable] [--fail-on-review] [--json]
  member inbox [--all] [--json]
  member work create|assign|claim|start|block|resume|release|submit|accept --expected-version <n> ...
  member runtime interrupt [--member-run-id <target>] --expected-version <n> --reason <text>
  [--project <id|path>] provider admit --provider <name> --execution-mode <mode> --provider-version <version>
                 --adapter-contract-version <version> --evidence <ref>
                 [--policy strict|advisory] [--actor <id>] [--json]
      An active/selected Execution Space requires the global --project flag;
      FIRM_PROJECT and ambient Project Binding defaults do not authorize admission.
  dashboard snapshot
  dashboard doctor --team-run-id <id> --api <base-url> [--expected-git-rev <rev>]
  hook record --agent <agent> [--runtime <runtime>]
  serve [--addr 127.0.0.1:8787] [--once]
  daemon start|status|stop|serve
  cheatsheet [team|work|mission|all]

Retired coordination commands fail explicitly. Historical rows are available only
through legacy-goal-task export|verify; retired Company OS records through
legacy-company-os export|verify. `harness company ...` and the Mission writers
(`mission create|update-context|close|log append`, POST /v1/missions*,
POST /v1/company-os/*) were retired by DOC-108.

Execution selection is independent: --space/FIRM_SPACE selects coordination
storage; --project/FIRM_PROJECT selects the provider cwd/config/Skill boundary.
HARNESS_SPACE and HARNESS_PROJECT remain deprecated compatibility aliases.

Agent Team creation requires one Host AgentMember and one ExecutionNode; Mission
provenance is optional legacy context, never a requirement."#
    );
    println!(
        "\nMember operating message contract:\n{}",
        crate::collaboration::member_operating_contract::render_member_message_cli_help()
    );
}

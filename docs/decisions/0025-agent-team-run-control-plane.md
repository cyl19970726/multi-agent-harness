# ADR 0025: Agent Team Run Control Plane

## Context

Two motivations, both grounded in one real closeout.

First, the problem. When an engineering effort enters multi-module
integration closeout, a single-session agent has three inevitable failure
modes:

- **Context collapse.** The intermediate state of many parallel lanes
  (handoff docs, test logs, screenshots, deploy state, blockers) cannot fit
  back into one context. In Stage 6 this was measured, not theoretical: "too
  many delegated tasks, completing too slowly", with the main thread
  context-switching across 11 acceptance gates. Raising `max_threads` only
  scales function-call concurrency; it does not fix the disease that every
  sub-agent's context must return to the main thread.
- **Responsibility vacuum.** A sub-agent returns a summary and disappears.
  A lane's branch, PR, and evidence chain then have no durable owner, so the
  main thread personally backs every lane and cannot truly parallelize.
- **Boundary loss.** External-change authorizations (deploy, payment choice,
  remote deletion) and physically exclusive resources (one real device, one
  DevTools project path) have no enforcement carrier under "main thread
  verbal arrangement"; they get silently crossed by reasonable defaults.

Second, the scenario where the problem exists: Issue #54 new-architecture
migration, Stage 6 integration closeout — 4 waves of work, 7 lanes, 11
terminal acceptance gates, 5 real test accounts, 1 physically exclusive
device, PR #78 (draft umbrella) / PR #79 merge boundaries, and a CloudBase
remote deletion requiring explicit user authorization. The Stage 6
checkpoint report was already an Agent Team prototype: its work item /
owner / dependency / status table is a task graph; its "next handoff" role
split is assignment; its blockers are blocker messages; its immutable
packet chain is an evidence ledger. Agent Team (Issue #206 direction) turns
those static roles into executable, observable members instead of inventing
a new structure.

The essential boundary — the one that drives prompts, protocol, and UI:

> A sub-agent is one function call. An Agent Team member is a living
> collaborator.

A function call takes a task, returns a result, ends, and is stateless to
its caller. A living collaborator has its own state, its own mailbox, and
its own responsibility domain: it keeps accepting new work, speaks up
mid-execution, and owns an outcome (PR merged, evidence complete) to the
end. Every other difference — granularity, context, communication,
lifecycle, deliverable, accountability — is a corollary of this one, not a
parallel feature list. The judgment question for which layer to use:

> Does the result need to come back into my context for me to keep using,
> or should it stay with the executor while I keep a pointer?

If it comes back, use a sub-agent. If it cannot come back (too large, too
long, needs continuous follow-through), use an Agent Team member.

## Decision

### Layering: Goal -> Wave -> MemberRun -> DelegationRun

```text
Goal
  -> Wave (= one AgentTeamRun; wave boundary is an integration gate, not a time)
    -> MemberRun (own provider session, own worktree/branch/PR, accountable
       for its lane until acceptance)
      -> DelegationRun (the member autonomously invokes its provider-native
         sub-agent capability per our prompt; the harness captures
         attribution, it does not schedule the member's sub-agents)
```

- A **wave** is one `AgentTeamRun`. A wave ends when the Lead completes the
  integration check and updates the plan, not when members go idle.
- **Deviation is normal input, not an exception.** Every wave boundary runs
  a re-plan loop: plan vs actual -> deviation -> decision -> next wave plan.
  This wires into the existing goal skeleton: a wave attaches to a
  `GoalPhase`, and the re-plan loop is `GoalEvaluation` -> `NextRoundPlan`
  (see [../goal-phase-loop.md](../goal-phase-loop.md) and ADR
  [0024](0024-goal-phase-execution-modes.md)). Agent Team adds no new
  planning layer.
- Sub-agents do not disappear; they sink one level. Scheduling authority
  over a member's sub-agents stays with the member: a Codex member uses
  Codex subagents, a Claude member uses Claude Task, a Kimi member uses
  Kimi Agent / AgentSwarm. The harness operates one level up: team
  formation, assignment, communication, observation, acceptance.

### v0 object model: six entities

```text
AgentTeamRun    id, definition_id?, host{surface, thread_id}, objective,
                status(planning|running|waiting|reviewing|completed|failed|cancelled),
                wave_index, member_run_ids[], task_ids[], budget_limit_usd?,
                created_at / started_at / ended_at

MemberRun       id, team_run_id, slot_id?, name, role,
                provider(codex|claude|kimi), model?,
                status(starting|idle|queued|running|waiting|reviewing|blocked|
                       completed|failed|stopped),
                provider_session_id?, acp_session_id?, current_task_id?,
                worktree_ref?, owned_paths[], created_at / ended_at

TeamMessage     id, team_run_id, task_id?, from, to[],
 + deliveries   kind(assignment|question|answer|progress|blocker|handoff|
                     review_request|review_result|control|broadcast),
                correlation_id, causation_id?, evidence_refs[],
                deliveries[{ member_id,
                             policy(queue|inject|interrupt|manual_ack),
                             status(queued|delivered|acknowledged|failed|expired),
                             attempt, updated_at }]

MemberAction    id, seq, team_run_id, member_run_id, task_id?,
                type(plan_updated|message_sent|message_received|tool_started|
                     tool_completed|file_changed|command_started|
                     command_completed|test_started|test_completed|
                     delegation_started|delegation_completed|review_started|
                     review_completed|waiting_for_input|waiting_for_approval|
                     blocked|error|completed),
                status(started|progress|succeeded|failed|cancelled),
                title, summary, evidence_refs[], started_at / ended_at

DelegationRun   id, team_run_id, parent_member_run_id, parent_task_id?,
                mode(provider_native|harness_worker|dynamic_workflow),
                provider, provider_child_thread_id?, workflow_run_id?,
                objective, status, evidence_ids[]

TeamRunEvent    id, seq, team_run_id,
                source{kind(host|member|delegation), member_run_id?,
                       delegation_run_id?},
                entity_type, entity_id, operation(created|updated|completed),
                summary, occurred_at
```

Rules:

- An `AgentTeamRun` is created by the Host Session through the plugin. When
  the run ends it becomes read-only history; it is not a standing
  organization.
- A `MemberRun` is released when its run ends. It is an execution instance,
  not a durable `AgentMember`; what persists across runs is the reusable
  definition (roles, provider preferences, policies) and the lessons, not
  the member.
- `TeamMessage` separates semantics from delivery: one message, one
  delivery record per recipient. Key messages (handoff, key tasks) must be
  ACKed; unacknowledged deliveries past threshold re-send and escalate.
- `MemberAction` is the normalized, auditable action reduced from all three
  providers' raw output. It never contains hidden reasoning.
- `TeamRunEvent` is a single ordered event log per run with a monotonic
  `seq`. SSE pushes it live; a disconnected client resumes by `seq`
  (Last-Event-ID). Event payloads are sanitized before they enter the
  store.

### Delegation guardrails: capture vs orchestrate

- **Capture mode** (`provider_native`): the member spawns its own
  provider-native sub-agent. Guardrails are prompt constraints plus
  post-hoc audit; depth and fan-out limits are the member-side provider's
  own (`max_depth` / `max_threads`). The harness observes, attributes the
  delegation to the parent task, and audits permission and budget scope —
  it does not control the child's lifecycle.
- **Orchestrated mode** (`harness_worker`, `dynamic_workflow`): the harness
  launches the child itself, so it enforces hard caps: delegation depth
  <= 2, child permissions <= parent, child `owned_paths` a subset of the
  parent's, and an explicit budget limit.
- `dynamic_workflow` is also the honest degradation path: when a provider
  capability is real but not yet verified through our adapter (observation
  not wired), the run degrades to `dynamic_workflow` and labels the
  delegation as such. `unsupported` / `unverified` means "the adapter has
  not verified or wired it", never "the provider lacks it" — and the UI
  must not pretend otherwise.

### Plugin packaging and the call-surface split

One harness, three thin plugin packages. The resident `harness serve`
process owns the store, the SSE event stream, the Read Model, and the MCP
server; `plugins/codex` (`.codex-plugin`), `plugins/claude`
(`.claude-plugin`), and `plugins/kimi` (`kimi.plugin.json`) are thin
host-native packages over it.

The call surface is layered — these are not four alternatives, each owns
one layer:

| Layer | Role | Why |
| --- | --- | --- |
| Plugin | distribution | packaging and install only; not a call mechanism |
| MCP | primary call surface | tool schema is machine-readable interface documentation; hosts render native approval UI and structured errors |
| Skill | teaches method | when to form a team, how to split waves, delivery contracts, handoff format — method, not capability |
| CLI | plumbing | executable body for monitors/hooks, debug fallback, CI |
| Hook | nerves | event-triggered injection and light interception (fail-open); never a call surface |

Two in-software display requirements follow:

1. **The host knows the Web Dashboard exists** and can point to it, through
   four injection points: `dashboardUrl` in every MCP return value;
   `sessionStart.skill` (Kimi) or skill description (Claude/Codex); an
   explicit `/agent-team:dashboard` command; and key events (run created /
   completed / blocked) re-injecting the URL into the session.
2. **Member status and live messages are visible inside the host
   software**: pull via `/agent-team:status` -> MCP `team_status`; push via
   Claude `monitors/monitors.json`, Kimi hook-injected summaries, and the
   Codex App UI. All three surfaces render one shared Read Model
   (`teamRunConsole(runId)`); the CLI text view is a compact projection of
   the same truth, not a second dataset.

### Member drive surface

Members are driven over persistent session protocols — Codex `app-server`
(JSON-RPC), Claude Agent SDK, and Kimi `acp` (JSON-RPC over stdio) — behind
one provider-neutral adapter:

```text
start . resume . prompt(stream) . cancel . permission . list
```

A `MemberRun` needs message injection, interruption, resume, and streaming
observation; one-shot print mode cannot provide these and is used only as
the `dynamic_workflow` leaf executor. `DelegationRun` capture uses each
provider's `SubagentStart` / `SubagentStop` hooks plus session artifacts
(Kimi `wire.jsonl`, Claude stream-json, Codex thread events); hook
callbacks report to the harness and are reduced into `DelegationRun` +
`MemberAction` rows.

### Non-goals

- Standing agent organizations, org directories ("digital employee"
  address books), cross-task inboxes, long-term presence.
- Displaying hidden model reasoning (chain-of-thought).
- Unlimited delegation depth, or a child with permissions above its
  parent.
- Members deploying, merging, or creating durable members on their own:
  external changes escalate to explicit user authorization.

## Consequences

- This composes with, and does not replace, the existing spine: ADR
  [0011](0011-provider-neutral-runtime.md) (provider-neutral runtime), ADR
  [0018](0018-exec-stream-primary-substrate.md) (exec substrate — print
  mode survives as the workflow leaf), ADR
  [0022](0022-dynamic-workflow-runtime-json-ir.md) (`dynamic_workflow` is
  an orchestration and degradation mode), and ADR
  [0024](0024-goal-phase-execution-modes.md) (a wave is one run attached to
  a `GoalPhase`, whichever execution mode the phase chose).
- Prompts split into three distinct kinds: wave plans carry orchestration
  facts (dependencies, exclusive resources, authorization gates, acceptance
  gates); member prompts carry role plus delivery contract (`ownedPaths`,
  completion standard, evidence requirements, handoff format, permission
  ceiling); delegation prompts carry task plus return format.
- The `TeamMessage` ledger and `TeamRunEvent` log are retained as audit
  evidence after the run; the frontend enters read-only history when the
  run completes.
- The page-level contract for observing a run lives in
  [../dashboard/pages/team-run-console.md](../dashboard/pages/team-run-console.md);
  the Kimi ACP drive surface is specified in
  [../integration/kimi.md](../integration/kimi.md); object semantics are
  summarized in [../concept-model.md](../concept-model.md).
- The older persistent-team workspace direction
  ([../dashboard/pages/team-workspace.md](../dashboard/pages/team-workspace.md))
  is untouched; this ADR's runs are ephemeral per-wave executions, not the
  standing-team surface.

## Validation

The v0 slice landed Kimi-first:

- Kimi ACP session driver (`crates/harness-cli/src/kimi_acp.rs`) behind the
  neutral adapter direction
  (`start`/`resume`/`prompt`/`cancel`/`permission`/`list`), with the ACP
  `sessionId` recorded on the `MemberRun`.
- `team-run` CLI and HTTP surface over the resident `harness serve` (create
  run, assign, send message, status, events follow), SSE event
  `team_run_event`, and snapshot keys for the six objects.
- The `/team-console` page rendering `teamRunConsole(runId)` over SSE
  (`crates/harness-cli/src/team_console.html`), including the member
  configuration composer.
- The `harness mcp` stdio MCP server
  (`team_run_create/list/status/send_message/events`) as the primary
  invocation plane for host CLIs.
- The `plugins/kimi-agent-team` package (`kimi.plugin.json`, orchestrator and
  member skills, status/dashboard/new-run commands, session hooks).

Validation commands for that slice (all green as of 2026-07-18; the four
`resident_daemon` failures on macOS are a pre-existing environment issue
reproducible on pristine HEAD):

- `cargo test -p harness-core -p harness-store`
- `cargo test -p harness-cli --test team_run_api --test team_run_start --test mcp_stdio`
- `pnpm check:schema-fixtures`
- `cargo run -q -p harness-cli -- governance check`
- Real-provider smoke: `harness team-run create … --member writer:docs:kimi@notes/`
  then `harness team-run start --id <id>` drives a real `kimi acp` session
  end-to-end (file written inside owned paths, contract report, handoff
  message with `manual_ack` delivery, run transitions to `completed`).

# Host Agent MCP Integration

## Product Contract

The Host Agent is the user's interactive Codex, Claude Code, Kimi Code, or
another long-lived coding agent. It is not an Agent Team member. Its complete
control surface is the canonical CLI; MCP is an optional thin typed adapter:

```text
Host Agent
  -> thin orchestration skill
  -> harness CLI (complete authoring and control)
  -> shared Rust application operations
  -> Mission / Host-plan Wave / AgentTeam / AgentTeamRun / Store
  -> provider member adapter

MCP       <- optional typed adapter over the same operations
Dashboard <- HTTP + SSE projections of the same store
```

Skills may teach the Host when to form a team and how to advance a Wave, but they
do not own product truth or execute runtime operations. Commands and hooks are
optional conveniences. Provider-specific integration packs configure these
parts; they do not fork the core model.

## Current Executable Boundary

- Host: Codex can call the stdio MCP server after local registration below.
- Coordination: Mission context, ordered Host-plan Wave revisions, each
  Mission's one flat AgentTeam, and Team/Node/Project-fenced AgentTeamRuns are
  native.
- Member execution: Codex app-server (`codex_app_server`), Kimi ACP
  (`kimi_acp`), and Claude Agent SDK streaming (`claude_agent_sdk`) are the
  executable Team Member modes. Codex app-server is the only Codex Team mode;
  Agent SDK streaming is the only Claude Team mode. Bounded `codex_exec` and
  `claude_cli` belong to Dynamic Workflow and other one-shot paths. They cannot
  create or start Team members. Harness never silently falls back.
- `team_run_start` reserves the run and returns immediately while members run
  in the background.
- The service that starts a TeamRun acquires the latest durable Supervisor
  generation and owns its live provider transports. Other CLI/MCP/Dashboard
  processes route mail and real controls to that owner over its loopback
  locator; the owner fences the generation again.
- Team mail uses typed Host, Member, stable Agent, Operator, and Service actors.
  An unbound MCP client may author as Host/Operator/Service but cannot claim to
  be a Member or stable Agent by supplying an id.
- Every create/start/status/cancel/ACK result includes an exact TeamRun URL on
  the UI origin (`127.0.0.1:5173`), with `api=.` so API and SSE requests use the
  UI's same-origin `/v1` proxy. When project identity is available it includes
  `project=<project-binding-id>`.
- Current Agent Team runtime policy gives every member full execution
  permission. Codex app-server threads launch with `danger-full-access` and
  approval policy `never`. A trusted Kimi ACP tool request that advertises an
  exact `allow_always` or `allow_once` option is acknowledged synchronously and
  creates only one bounded `provider_control` audit row—never a false
  PendingInteraction or waiting state. Questions, plan reviews, unknown
  requests, and tool requests without a safe allow option still pause and
  route to Lead, Policy, or Human. Provider transcripts and thinking do not
  enter Harness state.
- Thinking is allowed only as sanitized transient live state. It is never
  persisted, replayed, forwarded to peers, or accepted as evidence.

## Codex Registration

Build the binary, initialize/select the Workspace, then register its absolute
path and explicit project identity:

```bash
cargo build -p firm-cli
target/debug/firm init
codex mcp add harness -- \
  /absolute/path/to/target/debug/firm \
  --space <execution-space-id> \
  --project <project-binding-id> mcp
codex mcp get harness
```

An existing Codex conversation may require a new session before the newly
registered MCP tools appear. The API and Dashboard UI are separate long-running
processes. Start the Vite UI with its same-origin proxy pointed at the API:

```bash
target/debug/firm --space <execution-space-id> \
  --project <project-binding-id> serve --addr 127.0.0.1:8787
HARNESS_CAPTURE_API_PROXY=http://127.0.0.1:8787 npm run dashboard:dev
```

The MCP URL opens `http://127.0.0.1:5173` and sets `api=.`. Port 8787 is an API
origin, not a human Dashboard URL.

`space_id` selects the technical Execution Space that owns Mission/Wave, Team,
Workflow, and coordination state. `project_id` selects the Project Binding
that owns provider cwd, instruction/Skill discovery, Git/worktree, and
permission boundaries. Neither is a Company OS Project business object;
product copy should say **Execution Space** and **Project**.

## Store root is not execution root

The selected Execution Space store contains Harness JSONL coordination ledgers.
Provider processes do not run there. Their cwd is selected in this order:
member `provider_cwd_hint`,
TeamRun `execution_root`, then selected Workspace `project_root`; the Host cwd
is only the creation default for an unrouted legacy raw-store invocation.
`team_run_create` exposes `execution_root` and `members[].provider_cwd_hint` through
CLI (`--execution-root`, `--member-worktree name:path`,
`--member-owned-path name:path`), HTTP, and MCP. An
override must be the selected project root or a Git worktree sharing its Git
common directory, including external Codex worktrees.

That provider cwd controls project instruction and configuration discovery:
Codex walks `AGENTS.md` and its project/root skill/config locations from that
execution root; Claude and Kimi likewise load project-level instruction and
configuration files from the spawned project/worktree context. Moving the
central store must therefore never change provider cwd, and passing a store
path as an execution root is a routing defect.

## Host Journey

1. Call `mission_create` for durable intent and Markdown context.
2. Create the Mission's one flat AgentTeam with its Host Agent and immutable
   ExecutionNode placement. Create the next Host-plan Wave with full Markdown
   context; the Wave remains planning history and never owns runtime.
3. Call `team_run_create` with the required `agent_team_id`; Node and Mission
   are derived from the Team and the selected Project Binding is frozen on the
   run. Supply supported provider member identities/roles, disjoint owned paths,
   and workspace overrides only when needed. Keep the returned execution/member roots. Create bounded Works
   with explicit completion criteria, directly assign them or expose eligible
   unassigned Works for atomic claim, and keep the returned Work ids/versions.
4. Call `team_run_start`; immediately give the user its `dashboard_url`.
   For a Mission-scoped long-lived TeamRun, the URL includes the Mission and
   the Host's current Wave as navigation context even though the run itself has
   no Wave owner. Direct legacy Wave runs use their stored Wave id. The machine
   NodeDaemon owns the new Team Supervisor generation; a later Host process
   inspects that parent-fenced lease rather than assuming it owns provider handles.
5. Follow `team_run_status` or `team_run_events(after_seq=...)`. The browser
   receives durable Harness coordination plus transient/on-demand activity
   projected from provider-native sessions through SSE/API. Its compatibility
   `unacked_messages` field counts only actionable deliveries: at least one
   `manual_ack` delivery in `delivered` status. Queued, injected, failed,
   expired, and acknowledged deliveries do not increase it.
6. When a provider genuinely pauses for input, inspect its
   `PendingInteraction` and call `team_run_resolve_interaction` with the exact
   option id and authorized actor. Trusted full-access Kimi tool permissions
   with a provider-advertised safe allow option are already acknowledged and
   do not appear in this queue. Do not treat provider `completed` as proof of
   semantic approval or answer.
7. For a running `codex_app_server` member, use `team_run_steer_member` to
   inject input into the same turn. Use `team_run_interrupt_member` for Codex
   app-server, Kimi ACP, or Claude Agent SDK when the current turn must stop.
   Use `team_run_close_member` when the Host is releasing the current Member
   runtime while preserving its identity/history. Use
   `team_run_reopen_member` to resume that same MemberRun/native session, and
   deactivate only for a permanent coordination end. Other messages use
   `team_run_send_message` and preserve the native session.
8. Acknowledge consumed conversation with `team_message_acknowledge`; review
   submitted Works through the Work acceptance commands.
9. Check Work results, artifacts, and checks; update the current Wave with the Host's actual
   judgment, then record `accepted | revise | blocked`. Active
   MemberRuns may carry forward; Wave advance never completes them implicitly.

## Message Receipt Boundary

`TeamMessage` persistence, provider delivery, recipient acknowledgement,
semantic response, and Host acceptance are different facts:

```text
queued
  -> claimed by current Supervisor after provider transport preflight
  -> provider-native receipt / delivered to exact Host surface
  -> acknowledged by that recipient
  -> causation-linked answer / review conversation
  -> explicit Work transition, Host resolution, or outcome
```

Messages created while a Member is running are delivered at the next provider
round boundary. An unclosed idle Member is automatically woken by new mail or
ready owned WorkDelivery on the same MemberRun and provider-native session.
Provider turn completion, Work submission, Wave advance, TeamRun completion,
and Mission completion do not end
that lifetime. After a Host process restart, starting the TeamRun reattaches
unclosed Members to their recorded native sessions; it reconciles the latest
Work versions and never replays acknowledged conversation.

If the transport is dead before claim, mail stays queued. If a crash occurs
after claim but before provider receipt, the message remains explicitly
uncertain until `reconcile-delivery`; it is never blindly replayed. Explicit
Close is latched durably and freezes start/delivery across lease rollover. Only
explicit Reopen increments the runtime generation and resumes the recorded
native session; ordinary mail never revives a closed member.

Host-bound mail is scoped by the TeamRun's exact `host_surface +
host_thread_id`. The Codex Plugin reads only that native task's aggregate
Inbox. At `Stop`, it may use Codex's real one-shot continuation protocol to
handle mail that arrived while the Host was busy. Mail arriving after an
unowned Codex Desktop task is already idle remains actionable until the next
`UserPromptSubmit` or `SessionStart`; Harness does not spawn a second app-server
and pretend it owns the open Desktop task. See
[ADR 0040](../../decisions/0040-native-host-inbox-delivery.md).

This boundary is intentionally provider-neutral:

```text
Member native session
  -> explicit TeamMessage(to=host)
  -> Harness Host Inbox (delivered + manual_ack)
  -> exact native Host binding
  -> bounded Plugin context or one-shot Codex Stop continuation
  -> Host reads Inbox and ACKs transport
  -> Host sends a causation-linked answer/review/acceptance
```

Codex and Claude do not own separate mailbox Skills. Both use the canonical
`orchestrate-mission-waves` Host contract and
`collaborate-as-agent-team-member` Member contract; app-server versus Agent SDK
differences remain Adapter capabilities, not different team semantics.

Stable external Agent mail uses `agent route-inbox` and an
canonical `MessageDelivery` to bind one TeamMessage recipient to one active MemberRun and
TeamMessage. This is an explicit transport relation, not evidence that the
AgentMember is the durable identity; MemberRun is one execution attempt, and Company membership is only an AgentMember ActorRef projection.

An ACK means “the recipient consumed this envelope,” not “the recipient agrees”
and not “the Host accepts the work.” A reviewer must inspect the submitted Work,
its evidence, and the relevant native session before the Host claims cross-lane review.
Member-to-Member replies retain Host visibility and use `work_id`,
`correlation_id`, and `causation_id` where relevant; direct communication never
transfers Work ownership or Host decision authority.

## Host Acceptance Checklist

Run completion is only the start of Host review. For every standalone or
Mission-scoped Agent Team, inspect all of the following before describing its
result as accepted:

1. **Intent:** shared objective, decision boundary, non-goals, permissions,
   budget, workspace, and provider/version are explicit.
2. **Responsibility:** every required lane is a Work with distinguishable scope,
   completion criteria, owner/eligibility, version, and status.
3. **Execution truth:** every provider claim resolves to the expected
   provider-native session; missing, fresh, resumed, or incompatible state is
   labelled honestly.
4. **Receipt:** required Host/Member and Member/Member messages reached their
   intended native sessions or Host inbox; no queued message is stranded behind
   a terminal MemberRun.
5. **Lane outcome:** every required Work has a submitted result, structured
   blocker, explicit cancellation, or documented carry-over with useful
   evidence/artifact/check references.
6. **Cross-review:** a reviewer receives the completed claims it is supposed to
   review and separates agreement from independent reproduction.
7. **Contradictions:** the Host records accepted claims, mandatory corrections,
   unresolved unknowns, and active work carried forward.
8. **Semantics:** provider/Member completion is not treated as Host acceptance.
   Every TeamRun belongs to its Mission-owned Team, and semantic closeout
   requires the Host's explicit acceptance of the relevant Works.
9. **Reproducibility:** cited paths, revisions, session locators, checks, and
   external product/version facts can be reconstructed without copied provider
   transcripts or persisted thinking.
10. **Next action:** accept, request changes, block, cancel, or create/reassign
    Work against the persistent Member session; never overwrite history.

## Experience Acceptance

The integration is usable only when a user can start from a Codex prompt and
reconstruct the result from native state:

- Mission and ordered Host-plan Wave exist;
- the TeamRun is linked to the Mission and stable AgentTeam; the Wave may cite
  important Work ids/outcome through context or optional origin metadata without
  owning the run;
- actual MemberRuns have owned or claimed Works, WorkEvents, and WorkDelivery;
- start returns without blocking the Host conversation;
- the exact URL opens the correct Workspace and selected TeamRun;
- Work submissions/Host acceptance and conversational ACKs appear in their
  respective event streams;
- provider interactions preserve route, resolution actor, exact option id, and
  distinct transport/semantic status;
- outcome, useful artifacts/checks, and explicit Host Wave advance explain the
  plan decision;
- no durable thinking rows are created.

Run the deterministic product gate with:

```bash
npx pnpm@9.15.4 acceptance:mission-wave
```

This gate is not proof of a real provider call. Live claims require the native
records from a separately executed run in the claimed provider mode.

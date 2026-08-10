---
name: orchestrate-mission-waves
description: Use when a Host Agent must create, resume, or re-plan a long-running Mission, coordinate one or more persistent Agent Teams through shared Works, preserve provider-native sessions across re-plans, review submitted Work, or close the Mission. Use for Mission context, Mission Log judgment, Works allocation, Team composition, blocker handling, carry-over, and explicit Host acceptance. Do not use for a small one-shot task that fits safely in the Host context.
---

# Orchestrate Missions

This skill is a procedural capability, not product authority. Use the Harness CLI
as the complete authority path. Treat this Skill as a thin operating guide;
canonical architecture, schemas, store state, and native Provider records win
any conflict. After a compaction or whenever CLI syntax is
uncertain, run `harness cheatsheet` first — never rediscover flags via repeated
`--help` calls or source greps.

## Keep One Small Mental Model

```text
Store          = append-only JSONL, sole source of truth
Event          = every state change, monotonically sequenced (seq)
Daemon         = detached process, owns delivery loop + heartbeat
Work           = durable responsibility, owner, status, result, acceptance
TeamMessage    = authored conversation, optionally linked to one Work
Member         = autonomous worker, one turn = one round, idles after each
Host (Lead)    = decision-maker: review, accept, assign, close, re-plan
```

**Work states**: `open → in_progress → review → done` (Host accepts) or `→ cancelled`; assignment/ownership is metadata, not a status
**Message states**: `queued → claimed → delivered` (daemon handles)
**Member states**: `queued → running → idle` (normal breathing) or `→ stopped` (Host closes)
**Host receives**: hook auto-injects host-inbox at turn start — no polling needed
**Member receives**: daemon injects messages + work into CONTRACT prompt each round

These hard invariants apply to every Host and Member. The full shared text lives in [`skills/shared-references/SKILL.md`](../shared-references/SKILL.md); when a rule appears in both skills, the shared copy is authoritative. The rules below are the Host-Lead-specific application.

### Wave Vocabulary (Disambiguation)

The word **wave** in this skill name and in batch labels (e.g. "Governance wave
2") is a **planning-rhythm / batch label**, not a governed object. Wave as a
writable governed object (`wave create|update|advance`) was retired by ADR 0051;
no Wave state exists. Mission phases that were once recorded under wave
transitions are now recorded exclusively through Mission Log entries
(`--kind judgment|replan|closeout_evidence`).

Retired-object pointers:
- `HARNESS_ORIGIN_WAVE_ID` is a compatibility-only environment variable (see
  Collaboration Envelope below).
- `wave list|show|history` are historical read-only commands. They read
  compatibility records from pre-ADR-0051 runs but cannot create, mutate, or
  advance a wave.
- **Convention**: use lowercase `wave` as a batch noun, never capitalized
  `Wave` as an object name in new documentation, code, or Work titles.

Never turn the Mission Log into a task list, dependency graph, executor
container, synchronization barrier, or raw transcript dump. Never use a
Message as responsibility or status. Agent Team responsibility is only through the shared Works board — see shared hard invariants §1 (no Assignment Message compatibility path).

The Host using this Skill is the Team Lead. Lead is a control-plane role, not
an implicit MemberRun. Create a Lead MemberRun only when the Host deliberately
owns an execution Work with its own native session.

## Choose The Smallest Truthful Executor

| Need | Executor |
| --- | --- |
| Safe work that fits the Host context | Host |
| Addressable owner with Workspace, mailbox, sustained chat, or resume | Agent Team Member |
| Bounded helper inside one Member's responsibility | Provider-native subagent |
| Repeatable deterministic steps with step state | Dynamic Workflow |

For Team Members use only persistent bidirectional modes:
`codex_app_server`, `claude_agent_sdk`, or `kimi_acp`. Keep bounded
`codex_exec`/`claude_cli` execution in Dynamic Workflow. Do not silently fall
back from a persistent Team mode to a one-shot mode.

Select exactly one top-level execution driver per MemberRun — see shared hard invariants §2 (one execution driver).

## Run The Host Loop

1. **Observe.** Select the Execution Space and Project Binding explicitly.
   Inspect Mission, the Mission Log, its Mission-owned Team, Works, messages, pending
   interactions, Member/Supervisor health, and native-session bindings.
2. **Orient.** Create or update Mission Markdown with the durable objective,
   constraints, decision boundary, and success standard.
3. **Record judgment.** Append a Mission Log entry (`harness mission log
   append --mission-id <id> --kind judgment --body <markdown>`) containing
   changed facts, composition decisions, important Work ids, carry-over, and
   evidence needed to advance. Log before you act on the judgment, never as
   after-the-fact narration.
4. **Form the Team.** Create the Mission's one flat AgentTeam with its Host and
   immutable Node placement. Start one Team/Node/Project-fenced TeamRun when
   persistent collaborators are useful. TeamRun ownership is the Host's; no
   Mission Log entry owns a run.
5. **Create Works.** Put every schedulable responsibility on the shared board.
   Directly assign bounded lanes or create eligible unassigned Works for
   atomic Member claim. Give parallel code owners disjoint paths or require
   their own same-repository worktrees.
6. **Coordinate.** Use TeamMessage only for questions, answers, plans,
   explanation, and peer discussion. Link relevant messages with `--work-id`.
   If conversation creates a durable obligation, explicitly create/update
   Work; never infer one from prose.
7. **Integrate.** Inspect the submitted result, the exact durable artifact/check
   references required by the current Work candidate, and the resolvable native
   session. Artifact/check gates only match those references; they do not read
   artifacts, rerun checks, or establish that a referenced claim is true.
   Request changes or accept through Work operations. Do not wait for unrelated
   active Works.
8. **Re-plan.** At material decision points — a new Work tranche, a
   composition change, recovery, or a model/provider switch — append the
   Mission Log entry (`mission log append --kind judgment|replan|recovery`)
   before mutating runs or Works, never as after-the-fact narration. Use
   `--kind replan` when plan, composition, responsibility, risk, or decision
   boundary changes materially; `--kind judgment` for an ordinary material
   decision; `--kind recovery` while recovering a Mission, TeamRun, or Host
   session. Active Work keeps the same Work id, MemberRun, Workspace, and
   native session across every Log entry — see shared hard invariants §9.
9. **Close.** Append a `--kind closeout_evidence` Mission Log entry, then
   record an explicit Mission outcome. Closing a Mission does not erase its
   Team or provider-session history; runtime closure remains explicit.

## Host Scheduling Policy

This policy governs how the Host loop above actually runs a wake cycle; it
adds no new commands, only a discipline for the ones already listed.

- **Per-wake kernel.** Block on `harness team-run wait --id <team-run-id>
  --after-seq <last-seq> --timeout-secs <bounded-seconds>`. On wake, drain
  everything pending in priority order before sleeping again: (1) the
  review queue first — `review` is non-terminal and blocks its owner's
  downstream work; (2) blocked or crashed members; (3) the supply check
  below; (4) idle-member x unassigned-Work matching; (5) record any
  judgment not already logged inline at a material decision point above
  (`mission log append --kind judgment`, per the log-before-act discipline
  in **Re-plan** — a material decision inside steps 1-4, e.g. a recovery or
  composition change, is logged before that mutation, not deferred here);
  (6) recompute the wait predicate and sleep. One wake processes every
  pending fact, not one event at a time.
- **Supply watermark.** Keep ready claimable Works at or above the count of
  currently idle-capable Members. Start decomposing the next tranche once
  the current one is roughly two-thirds consumed; do not wait for the board
  to drain. Never let the board reach zero ready Works while Members remain
  active.
- **Claim-mode default.** Create Work with `--claim-mode team_claim` and an
  empty eligible list (every active Member may claim) by default. Reserve
  `--claim-mode host_assign` for the exception: a lane that needs one
  specific owner because of disjoint paths or a required capability.
- **Budget discipline.** Keep the Host window to policy, the current
  judgment memo, and this wake's events only. Re-read global state fresh
  from the board each wake instead of trusting window memory; judgment
  history lives in durable records, never only in the window.
- **Work context template.** Every Work needs a mini mental model so the
  Member can orient in one turn. Use this structure in `--context`:

  ```
  ┌─ What ──────────────────────────────┐
  │ One sentence: what to build/fix/audit│
  ├─ Mental Model ──────────────────────┤
  │ ASCII diagram of states / flow      │
  │ Key invariants the Member must hold  │
  ├─ Boundary ──────────────────────────┤
  │ Paths to touch / never touch        │
  │ Worktree convention (OUTSIDE repo)  │
  │ Other members' lanes (don't collide) │
  ├─ Delivery Requirements ─────────────┤
  │ 1. Use the exact WorkspaceBinding    │
  │    assigned to this MemberRun        │
  │ 2. Follow delivery criteria and the  │
  │    candidate-scoped requirements     │
  │ 3. Submit exact durable refs        │
  │ 4. Wait for Host review             │
  ├─ Verification ─────────────────────┤
  │ Exact GateRequirements, evaluator   │
  │ identity and evidence expectations  │
  ├─ Evidence ──────────────────────────┤
  │ What counts as done                 │
  │ Required artifact_refs / check_refs  │
  └─────────────────────────────────────┘
  ```

  Use `--completion-criteria` for the acceptance checklist, not prose.
  The `--context` should make the Member understand the problem in one read;
  the `--completion-criteria` should make the Host accept/reject in one look.

  **Standard delivery requirements** (include in every code-changing work):
  - Workspace: declared via `--worktree` (harness creates and cleans up)
  - Candidate identity: the exact submitted Work id and version
  - Evidence: attach the exact durable refs named by declared gates and criteria
  - Review: wait for Host review and explicit acceptance
  - No half-finished work: if blocked, report why, don't leave uncommitted changes

## Workspace Management

Workspace placement belongs to `MemberWorkspaceBinding`, not Work. Provision an
absolute workspace outside the repository through the canonical member-trust
service and bind it to the exact AgentMember, MemberRun, TeamRun, Work and
Execution Space.

The lifecycle is `requested → provisioning → ready → attached → archived →
removed`. Every transition uses CAS and a safety proof. Git workspaces must
prove the expected repository identity, base revision and clean/dirty policy;
directory workspaces must reject relative paths, parent traversal, symlink
escape and cross-project placement. Cleanup is explicit and cannot traverse
links or remove a dirty/unverified workspace.

The supervisor may start provider execution only after the binding is Ready or
Attached and its identity/generation still matches the canonical MemberRun.
Never infer placement from process cwd or from prose in Work context.
## Create And Allocate Works

List and inspect the board before allocating new Work:

```bash
harness team-run work list --team-run-id <team-run-id>
harness team-run work show --work-id <work-id>
```

Create one bounded responsibility with explicit context, completion criteria,
owner or claim pool, prerequisite Work ids and a stable idempotency key. Do not
put verification plugins or filesystem placement inside the Work row.

```bash
harness team-run work create \
  --team-run-id <team-run-id> \
  --title "<bounded responsibility>" \
  --context "<Markdown context and constraints>" \
  --completion-criteria "<observable acceptance criteria>" \
  --owner-member-run-id <member-run-id> \
  --claim-mode host_assign \
  --idempotency-key <stable-command-key>
```

Use `MemberWorkspaceBinding` for execution placement. Use
`WorkModuleBinding` for reusable process bundles, and derive explicit
candidate-scoped `GateRequirement` rows from the bound Module or add direct
requirements. Evaluations and waivers are separate durable records; Work itself
remains the simple responsibility atom.

Empty `eligible_member_ids` means every active Member may claim. Use
`prerequisite_work_ids` only for minimal readiness, not as a general Task
Graph. Assignment changes owner and creates a canonical WorkDelivery; it is not
a Message. The runtime claims the exact delivery under supervisor and MemberRun
generation fences, then records a provider receipt before considering it
received.
## Use Messages Only For Conversation

Start a Work-linked conversation:

```bash
harness team-run send --id <team-run-id> \
  --from host --to <member-run-id> --kind message \
  --work-id <work-id> \
  --body "<question, clarification, plan request, or explanation>" --json
```

Reply to a specific message without changing Work state:

```bash
harness team-run send --id <team-run-id> \
  --from host --to <member-run-id> --kind message \
  --work-id <work-id> \
  --body "<reply>" \
  --correlation-id <conversation-correlation-id> \
  --causation-id <message-id> --json
```

Harness has no Plan Mode or Plan Gate. When you want a plan first, ask for a Markdown plan in an ordinary linked conversation, argue/revise there, then explicitly tell the Member to proceed — see shared hard invariants §8.

At Host safe boundaries, read the bound Lead Inbox. ACK means receipt, not
semantic approval:

```bash
harness team-run host-inbox \
  --surface <provider-surface> --thread-id <native-host-task-id> --json
harness team-run ack --id <team-run-id> \
  --message-id <message-id> --member-id host
```

Ordinary mail never interrupts the middle of a Host or Member turn. Use real
Steer only when the selected Provider mode acknowledges current-turn injection;
otherwise send a queued Message for the next safe boundary.

## Review Work Explicitly

Provider completion and conversational updates never submit or accept Work. The
Member submits one canonical Result `WorkReport` for the exact Work revision and
Candidate fingerprint. That report carries the result, evidence refs, findings,
difficulties, residual risks, and failure analysis when applicable; creating it
atomically moves an active Work into Review.

Verification is candidate-scoped. The Host creates explicit
`GateRequirement` rows, evaluators append exact `GateEvaluation` rows, and a
waiver is valid only when its authority, scope, expiry and justification match
the current requirement set. Integration-plan `WorkModuleBinding` rows freeze
the module version and config that produced the requirements. A stale report,
Candidate fingerprint, requirement-set fingerprint, evaluation, waiver or Work
revision must fail closed with zero acceptance side effects.

Before accepting, independently inspect the current Candidate, rerun the checks
required by its risk, and verify the canonical Result evidence. Then choose
exactly one:

- request changes on the current Work revision with a specific reason; or
- call canonical `AcceptWork` through `member-trust mutate`, naming the exact
  Team, Work, Result report and Candidate fingerprint.

Canonical acceptance rereads all of those records under one Store writer lock
and commits the accepted Work revision through the canonical operation ledger.
A reviewer may recommend a decision but cannot impersonate Host authority.
Submission moves Work to Review; only exact Host acceptance closes it as
accepted.
## Handle Lifecycle And Failure

### Wake-Latency Expectations

Between-round silence is normal and expected — a Member finishing a provider
turn may take seconds to minutes before the next cycle starts. Never treat
silence alone as a stall.

Before nudging a Member, progress-probe first: check the board for status
changes, read its native session tail, or inspect its last delivery. If
progress is genuinely stalled, send a control message (ordinary Work-linked
TeamMessage) — it is the standard nudge. Do not repeatedly send duplicate
nudges; one message with the specific concern and a decision request is
sufficient.

### Member-Failure Recovery Checklist

When a Member has genuinely failed (crashed, unresponsive, or terminated):

1. **Its `in_progress` Works cannot be released or reassigned** — the Work
   state machine does not permit ownership transfer while in progress.
2. **Cancel each `in_progress` Work with an honest reason** describing what
   the Member was attempting and why the attempt failed.
3. **Move responsibility to another Work** — create a new Work capturing the
   remaining scope, or assign a new owner to an existing open Work.
4. **Close the Member with the failure reason** (`team-run close-member`)
   before any replacement Member claims the new Work.

Never silently drop a failed Member's in-progress Works; always record the
cancellation reason and create explicit follow-up responsibility.

- `idle`: assign or expose ready claimable Work.
- `working`: queue new Work without interrupting the active turn.
- `waiting interaction`: resolve the exact PendingInteraction before driving.
- `crashed/disconnected`: run `harness team-run recover --id <run>` to adopt/restart
  the supervisor generation, reconcile stale deliveries, resume compatible native
  sessions, and rebind incompatible Works. Never run `team-run create` during
  recovery — recovery must rebind the existing run and Work ids, never mint
  new ones (ADR 0050). `team-run recover` prints the linked Mission's Log tail
  first, before any mutation, so read it before acting (ADR 0051).
- `closed`: explicitly Reopen, rebind, reassign, or cancel unfinished Work.
- `retired`: never revive; reassign or cancel Work.

Interrupt stops one current turn. Close releases the managed runtime. Reopen
preserves MemberRun and resumes the exact compatible native session under a
higher Supervisor generation. If the session is incompatible, retain it as
history, create a replacement binding, and append the explicit Work rebound.
Never reconstruct a session from Harness messages — see shared hard invariants §3.

When a Member appears stuck, inspect control-plane facts first, then perform a
bounded read of its native session. Do not repeatedly poll full status or send
duplicate Work. Prefer event waits:

```bash
harness team-run wait --id <team-run-id> \
  --after-seq <last-seq> --timeout-secs <bounded-seconds>
```

### NodeDaemon Recovery Ladder (L0 → L4)

One machine-scoped NodeDaemon owns every local TeamRun. Each Team Supervisor is
a child context fenced by the current NodeDaemon generation. `team-run start`
requires that daemon and never launches a private per-run fallback.

When `team-run status` or `team-run recover` shows no live supervisor:

**L0 — Diagnose** (always start here):
```bash
harness team-run status --id <team-run-id>
# Look for: supervisor current=false, pid_alive=false, heartbeat_age_s
harness team-run status --id <team-run-id> --json | jq '.supervisor'
harness daemon status
```

**L1 — Start or restart the machine daemon** (covers crash or lease expiry):
```bash
harness daemon start
harness team-run start --id <team-run-id>
```
The NodeDaemon reacquires all registered Execution Spaces and resumes eligible
TeamRun contexts under a new parent generation.

**L2 — Stop and restart a wedged NodeDaemon** (process alive but no progress):
```bash
harness daemon stop
harness daemon start
harness team-run start --id <team-run-id>
```
Do not kill a PID directly; preserve the daemon lease and recovery diagnostics.

**L3 — Per-member close/reopen** (single bad member, NodeDaemon healthy):
```bash
harness team-run close-member --id <team-run-id> --member-run-id <id> --reason "..."
harness team-run reopen-member --id <team-run-id> --member-run-id <id>
harness team-run start --id <team-run-id>
```
The daemon picks up the reopened member automatically.

**L4 — Nuclear recreate** (last resort, store state suspect):
```bash
harness team-run cancel --id <team-run-id>
# Rebuild from stored Work ids:
harness team-run recover --id <team-run-id>
harness team-run start --id <team-run-id>
```

### Quick Board Reads

For bounded Host context, prefer these compact reads over full `work list`:

```bash
harness team-run board-summary --id <team-run-id>
harness team-run work list --team-run-id <team-run-id> --brief
harness team-run work list --team-run-id <team-run-id> --since <cursor>
```

`board-summary` prints a ≤500-character summary: open/in-progress/blocked/review/done/cancelled counts plus each Member's idle/working/awaiting-review state. `--brief` prints one plain-text line per Work. `--since` takes a monotonic cursor from a prior `list` response and returns only new or updated Works.

To acknowledge all delivered manual-ack messages at once:

```bash
harness team-run ack --id <team-run-id> --member-id host --all-delivered
```

## Execution Driver Reference

| Driver | Who drives cycles | Used for |
| --- | --- | --- |
| `host_driven` | Harness starts each cycle via mailbox delivery | Default for persistent Team Members |
| `provider_driven` | A reviewed native continuation loop starts cycles | Members with verified provider-native continuation |
| `user_driven` | A human drives their own open provider session | `external_interactive` members only |

The driver is a field on `ProviderIntegrationProfile.execution_driver`, not a CLI flag. The Host selects it when composing the Team; the Member reads it from the collaboration envelope.

## Delegate Without Losing Accountability

A Member may use native subagents internally; they do not become Team Members
or own Work — see shared hard invariants §6. For durable cross-Team delegation,
the Host creates an explicit WorkDelegation to another flat Team. Target
completion never auto-submits or accepts the source Work — see shared hard
invariants §7.

## Flat Organization Target Contract

- **AgentMember is the organization-agent identity**, durable across MemberRuns, provider processes, native sessions, and execution attempts.
- **Organization contains flat AgentTeams**: each Team has one Mission, one Host,
  and immutable single-Node placement; no Member or Team nests another Team.
- **One Work kernel serves Team and Organization**: Team Work semantics are the
  atom, while explicit WorkDelegation connects flat Teams and optional business
  relations connect Document, Milestone, Module, Approval, Finance, Mission, or
  external delivery.

Company organization membership projects the same canonical AgentMember ActorRef; no second durable agent identity or execution join exists.

## Collaboration Envelope

When the Host starts a Member run, the harness injects these environment variables into the Member's runtime:

| Variable | Presence | Meaning |
| --- | --- | --- |
| `HARNESS_TEAM_RUN_ID` | Yes | The TeamRun this Member belongs to |
| `HARNESS_MEMBER_RUN_ID` | Yes | This Member's own run identity |
| `HARNESS_BIN` | Yes | Absolute path to the harness CLI binary |
| `HARNESS_SPACE` | Yes | Current Execution Space |
| `HARNESS_PROJECT` | Yes | Active Project Binding path |
| `HARNESS_PROJECT_ID` | Yes | Active Project Binding id |
| `HARNESS_MISSION_ID` | When Mission-scoped | The Mission this TeamRun serves |
| `HARNESS_WORK_ID` | When delivered with Work | The Work id for the current delivery |
| `HARNESS_WORK_VERSION` | When delivered with Work | The Work version for the current delivery |
| `HARNESS_ORIGIN_WAVE_ID` | Historical | Deprecated; preserved for compatibility reads only |

The Host must never infer a Member's identity from a display name; the injection binds identity. Member-side Work commands (`work claim`, `work start`, `work block`, `work submit`) validate `HARNESS_MEMBER_RUN_ID` against the collaboration envelope and reject calls where the bound value does not match.

## Acceptance Checklist

Before claiming completion, prove from durable state:

- Mission intent, Mission Log judgment, linked Team, and TeamRun are
  reconstructable;
- every responsibility is a Work, not an Assignment Message or private Host
  memory;
- WorkEvent versions and WorkDelivery receipt/recovery facts are consistent;
- Messages explain coordination and use `work_id` where relevant without
  changing Work state (shared hard invariants §4);
- submitted Works have a result summary, the artifact/check refs required by
  their criteria, and explicit Host acceptance or clear requested changes;
- TeamRun completion is recorded only after all Works are `done` or
  `cancelled`;
- native-session references support claims about Provider execution;
- carried Works retain identity across re-plans; and
- the Host records explicit Mission Log judgment and Mission closeout.

When developing Star Harness itself and the product contract is in question,
read canonical repository files `docs/current/product/agent-team-works.md`,
`docs/decisions/0050-agent-team-work-board-and-message-boundary.md`,
`docs/decisions/0051-single-intent-spine.md`, and
`docs/current/product/mission-wave-host-plan.md`.

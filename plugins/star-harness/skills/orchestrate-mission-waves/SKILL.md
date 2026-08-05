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
Mission        = durable intent, shared context, outcome, and closeout
Mission Log    = versioned Host judgment: appended entries (judgment/replan/recovery/closeout)
AgentTeam      = independent reusable collaboration identity
AgentTeamRun   = one live or historical execution of that Team
Work           = durable responsibility, owner, status, result, acceptance
Works          = shared TeamRun board derived from Work records
TeamMessage    = authored conversation, optionally linked to one Work
WorkDelivery   = reliable delivery of one Work version to a Member runtime
Native Session = transcript, tools, commands, turns, internal subagents
```

These hard invariants apply to every Host and Member. The full shared text lives in [`skills/shared-references/SKILL.md`](../shared-references/SKILL.md); when a rule appears in both skills, the shared copy is authoritative. The rules below are the Host-Lead-specific application.

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
   Inspect Mission, the Mission Log, linked Teams, Works, messages, pending
   interactions, Member/Supervisor health, and native-session bindings.
2. **Orient.** Create or update Mission Markdown with the durable objective,
   constraints, decision boundary, and success standard.
3. **Record judgment.** Append a Mission Log entry (`harness mission log
   append --mission-id <id> --kind judgment --body <markdown>`) containing
   changed facts, composition decisions, important Work ids, carry-over, and
   evidence needed to advance. Log before you act on the judgment, never as
   after-the-fact narration.
4. **Form the Team.** Link an existing AgentTeam or create one. Start one
   Mission-scoped TeamRun when persistent collaborators are useful. TeamRun
   ownership is the Team Lead's; no Mission Log entry owns a run.
5. **Create Works.** Put every schedulable responsibility on the shared board.
   Directly assign bounded lanes or create eligible unassigned Works for
   atomic Member claim. Give parallel code owners disjoint paths or require
   their own same-repository worktrees.
6. **Coordinate.** Use TeamMessage only for questions, answers, plans,
   explanation, and peer discussion. Link relevant messages with `--work-id`.
   If conversation creates a durable obligation, explicitly create/update
   Work; never infer one from prose.
7. **Integrate.** Inspect the submitted result, the artifact/check references
   required by its completion criteria, and the resolvable native session.
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
   record an explicit Mission outcome. Closing a Mission never closes the
   independent Team or its Members.

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

## Create And Allocate Works

List and inspect the board before allocating new work:

```bash
harness team-run work list --team-run-id <team-run-id>
harness team-run work show --work-id <work-id>
```

Create an assigned Work:

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

Create a claimable shared-pool Work:

```bash
harness team-run work create \
  --team-run-id <team-run-id> \
  --title "<ready responsibility>" \
  --context "<Markdown context>" \
  --completion-criteria "<observable acceptance criteria>" \
  --claim-mode team_claim \
  --eligible-member-id <member-run-id-a> \
  --eligible-member-id <member-run-id-b> \
  --idempotency-key <stable-command-key>
```

Empty `eligible_member_ids` means every active Member may claim. Use
`--prerequisite-work-id` only for minimal readiness; do not encode branches,
loops, conditions, retries, or a general Task Graph.

An idle eligible Member may be woken from the shared board without creating a
WorkDelivery or TeamMessage. The wake is only a discovery hint; responsibility
begins at the winning atomic `claimed` WorkEvent. Active-work continuation is
restricted to `in_progress`: never repeatedly wake `review`, `blocked`, or
terminal Work.

Assign an existing open Work with optimistic concurrency:

```bash
harness team-run work assign \
  --work-id <work-id> \
  --member-run-id <member-run-id> \
  --expected-version <latest-version> \
  --idempotency-key <stable-command-key>
```

Assignment changes owner and emits WorkDelivery; it is not a Message. An idle
runtime may be woken at its reviewed delivery boundary. A busy runtime receives
the Work at the next safe boundary. Never silently interrupt active work.

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

Provider completion and conversational updates never submit or accept Work.
When a Member moves Work to `review`, inspect the required result summary plus
the artifact/check refs, changed files, tests, and native session that its
completion criteria and risk require. Empty artifact/check arrays are valid
when the criteria need no such reference. Then choose exactly one:

```bash
harness team-run work request-changes \
  --work-id <work-id> --expected-version <latest-version> \
  --reason "<specific required change>" \
  --idempotency-key <stable-command-key>

harness team-run work accept \
  --work-id <work-id> --expected-version <latest-version> \
  --idempotency-key <stable-command-key>
```

Host acceptance moves Work to `done`. A reviewer Member may recommend but cannot impersonate the Host's acceptance authority. Submission moves Work to `review`, not `done`; only explicit Host acceptance moves Work to `done` — see shared hard invariants §5.

## Handle Lifecycle And Failure

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

A Member may use native subagents internally; they do not become Team Members or own Work — see shared hard invariants §6. For durable multi-level delegation, the parent owner creates a child Team and becomes its Host. Child Works remain in the child TeamRun; an explicit WorkDelegation links them. Child completion never auto-submits or accepts the parent Work — see shared hard invariants §7.

## ADR 0052 Target Contract: Recursive Agent Teams

ADR 0052 adopted [Nested Agent Team Organization](../docs/company-os/nested-agent-team-organization.md) as the accepted target contract. Under this contract:

- **AgentMember is the organization-agent identity**, durable across MemberRuns, provider processes, native sessions, and execution attempts.
- **Organization is recursive AgentTeam topology**: the Lead AgentMember Hosts the root AgentTeam; any Member may create and Host a child AgentTeam.
- **One Work kernel serves Team and Organization**: ADR 0050 Work semantics become the base responsibility model, with optional business relations for Document, Milestone, Module, Approval, Finance, Mission, or external delivery.

The current separate StandingAgent record is a compatibility implementation; new target architecture must not add another durable agent identity. The current deploy/Host loop in this skill works with both the compatibility and target models.

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
read canonical repository files `docs/product/agent-team-works.md`,
`docs/decisions/0050-agent-team-work-board-and-message-boundary.md`,
`docs/decisions/0051-single-intent-spine.md`, and
`docs/product/mission-wave-host-plan.md`.

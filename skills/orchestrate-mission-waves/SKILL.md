---
name: orchestrate-mission-waves
description: Use when a Host Agent must create, resume, or re-plan a long-running Mission, coordinate one or more persistent Agent Teams through shared Works, preserve provider-native sessions across Waves, review submitted Work, or close the Mission. Use for Mission context, Wave judgment, Works allocation, Team composition, blocker handling, carry-over, and explicit Host acceptance. Do not use for a small one-shot task that fits safely in the Host context.
---

# Orchestrate Mission Waves

Use the Harness CLI as the complete authority path. Treat this Skill as a thin
operating guide; canonical architecture, schemas, store state, and native
Provider records win any conflict. After a compaction or whenever CLI syntax is
uncertain, run `harness cheatsheet` first — never rediscover flags via repeated
`--help` calls or source greps.

## Keep One Small Mental Model

```text
Mission        = durable intent, shared context, outcome, and closeout
Wave           = versioned Host memo: changed facts, plan, judgment, re-plan
AgentTeam      = independent reusable collaboration identity
AgentTeamRun   = one live or historical execution of that Team
Work           = durable responsibility, owner, status, result, acceptance
Works          = shared TeamRun board derived from Work records
TeamMessage    = authored conversation, optionally linked to one Work
WorkDelivery   = reliable delivery of one Work version to a Member runtime
Native Session = transcript, tools, commands, turns, internal subagents
```

Never turn Wave into a task list, dependency graph, executor container,
synchronization barrier, or transcript. Never use a Message as responsibility
or status. Agent Team has no Assignment Message compatibility path.

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

Select exactly one top-level execution driver for each MemberRun/native
session/writable Workspace:

- `host_driven`: Harness starts the next eligible Provider cycle;
- `provider_driven`: a reviewed native continuation loop starts cycles.

Never activate a native Goal and also start ordinary Harness cycles for the
same Work. Provider Goal satisfaction, Provider turn completion, transport
receipt, Work submission, and Host acceptance are different facts.

## Run The Host Loop

1. **Observe.** Select the Execution Space and Project Binding explicitly.
   Inspect Mission, Waves, linked Teams, Works, messages, pending interactions,
   Member/Supervisor health, and native-session bindings.
2. **Orient.** Create or update Mission Markdown with the durable objective,
   constraints, decision boundary, and success standard.
3. **Record judgment.** Create the current Wave as a concise memo containing
   changed facts, composition decisions, important Work ids, carry-over, and
   evidence needed to advance.
4. **Form the Team.** Link an existing AgentTeam or create one. Start one
   Mission-scoped TeamRun when persistent collaborators are useful. Do not make
   the selected Wave own the run.
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
8. **Re-plan.** At material decision points, record the judgment before
   acting, not as after-the-fact narration. Revise the current Wave while
   judgment is materially unchanged. Advance and create Wave N+1 when plan,
   composition, responsibility, risk, or decision boundary changes
   materially. Active Work keeps the same Work id, MemberRun, Workspace, and
   native session.
9. **Close.** Record an explicit Mission outcome. Closing a Mission or
   advancing a Wave never closes the independent Team or its Members.

## Host Scheduling Policy

This policy governs how the Host loop above actually runs a wake cycle; it
adds no new commands, only a discipline for the ones already listed.

- **Per-wake kernel.** Block on `harness team-run wait --id <team-run-id>
  --after-seq <last-seq> --timeout-secs <bounded-seconds>`. On wake, drain
  everything pending in priority order before sleeping again: (1) the
  review queue first — `review` is non-terminal and blocks its owner's
  downstream work; (2) blocked or crashed members; (3) the supply check
  below; (4) idle-member x unassigned-Work matching; (5) record the
  judgment (today, updating the current Wave revision); (6) recompute the
  wait predicate and sleep. One wake processes every pending fact, not one
  event at a time.
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

When you want a plan first, ask for a Markdown plan in an ordinary linked
conversation, argue/revise there, then explicitly tell the Member to proceed.
Harness has no Plan Mode or Plan Gate.

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

Host acceptance moves Work to `done`. A reviewer Member may recommend but
cannot impersonate the Host's acceptance authority.

Complete a TeamRun only after every current Work is `done` or `cancelled`.
Treat `review` as non-terminal: submission without Host acceptance must block
completion. The Store owns this as an atomic check-and-complete gate; do not
pre-check a snapshot and assume a later completion call is safe.

## Handle Lifecycle And Failure

- `idle`: assign or expose ready claimable Work.
- `working`: queue new Work without interrupting the active turn.
- `waiting interaction`: resolve the exact PendingInteraction before driving.
- `crashed/disconnected`: run `harness team-run recover --id <run>` to adopt/restart
  the supervisor generation, reconcile stale deliveries, resume compatible native
  sessions, and rebind incompatible Works. Never run `team-run create` during
  recovery — recovery must rebind the existing run and Work ids, never mint
  new ones (ADR 0050).
- `closed`: explicitly Reopen, rebind, reassign, or cancel unfinished Work.
- `retired`: never revive; reassign or cancel Work.

Interrupt stops one current turn. Close releases the managed runtime. Reopen
preserves MemberRun and resumes the exact compatible native session under a
higher Supervisor generation. If the session is incompatible, retain it as
history, create a replacement binding, and append the explicit Work rebound.
Never reconstruct a session from Harness messages.

When a Member appears stuck, inspect control-plane facts first, then perform a
bounded read of its native session. Do not repeatedly poll full status or send
duplicate Work. Prefer event waits:

```bash
harness team-run wait --id <team-run-id> \
  --after-seq <last-seq> --timeout-secs <bounded-seconds>
```

## Delegate Without Losing Accountability

A Member may use native subagents internally; they do not become Team Members
or own Work. For durable multi-level delegation, the parent owner creates a
child Team and becomes its Host. Child Works remain in the child TeamRun; an
explicit WorkDelegation links them. Child completion never auto-submits or
accepts the parent Work.

## Acceptance Checklist

Before claiming completion, prove from durable state:

- Mission intent, Wave judgments, linked Team, and TeamRun are reconstructable;
- every responsibility is a Work, not an Assignment Message or private Host
  memory;
- WorkEvent versions and WorkDelivery receipt/recovery facts are consistent;
- Messages explain coordination and use `work_id` where relevant without
  changing Work state;
- submitted Works have a result summary, the artifact/check refs required by
  their criteria, and explicit Host acceptance or clear requested changes;
- TeamRun completion is recorded only after all Works are `done` or
  `cancelled`;
- native-session references support claims about Provider execution;
- carried Works retain identity across Wave changes; and
- the Host records an explicit Wave outcome and Mission closeout.

When developing Star Harness itself and the product contract is in question,
read canonical repository files `docs/product/agent-team-works.md`,
`docs/decisions/0050-agent-team-work-board-and-message-boundary.md`, and
`docs/product/mission-wave-host-plan.md`.

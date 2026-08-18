# Codex Integration

```text
status: implementation reference; Work/WorkDelivery target pending ADR 0050
```

This document defines the current Codex provider contract for Star Harness.
Provider-neutral lifecycle and mailbox semantics live in
Agent Runtime; this file only explains how Codex
implements them.

## Current Mode Boundary

| Executor | Mode | Status |
| --- | --- | --- |
| Agent Team Member | `codex_app_server` | only executable mode for new Codex MemberRuns |
| Dynamic Workflow / bounded execution | `codex_exec` | supported one-shot mode |
| Historical Team record | `codex_exec` | readable, never startable |

Harness never silently falls back from app-server to exec. Provider brand,
execution-mode capability, adapter coverage, and product permission remain
separate claims. See
[ADR 0031](../../decisions/0031-interactive-provider-modes-and-version-drift.md).

### Model, effort, and service tier

For `codex_app_server`, Harness sends the requested model, reasoning effort,
and service tier through the native thread/turn configuration. `MemberRun`
keeps the requested values separately from the effective values returned by
app-server. A missing native receipt stays `requested` or becomes
`review_required`; it is never reported as effective by copying launch input.

The Dashboard therefore renders `requested -> effective` with a status for
each control. Changing one of these controls for a long-lived Agent Membership is
a runtime-contract change: drain or interrupt the old active turn, then
reattach the reviewed native thread under the replacement runtime generation.

## Agent Team Runtime

One live Codex MemberRun owns one app-server child and one native Codex thread:

```text
MemberRun + active Work/version
  -> codex app-server --listen stdio://
  -> initialize
  -> thread/start or explicit thread/resume
  -> thread/name/set "Agent Team · <member name>"
  -> turn/start for queued ordinary mail
  -> turn/steer only for a real same-turn Steer
  -> turn/interrupt only for current-turn interruption
  -> explicit Host Close ends the app-server runtime but retains the thread id
  -> explicit Reopen starts a new adapter generation with thread/resume
```

The app-server handshake returns the thread identity and effective execution
controls, but response nesting is version-specific. Reviewed Codex
`0.148.0-alpha.9` returns the effective model, reasoning effort, service tier,
approval policy, and sandbox beside `result.thread`. The adapter validates the
returned thread id and every requested effective control; omission or mismatch
fails closed. Historical model fixtures also returned `result.thread.model`,
which remains readable for old records but is not used to invent a current
permission or execution-setting receipt.

Ordinary provider turn completion does not destroy the Member. Interrupt and
Close are different:

- **Interrupt** requests `turn/interrupt`, waits for provider acknowledgement,
  and leaves the Member/native thread available for later mail.
- **Close** ends the app-server process, freezes coordination, and retains the
  same MemberRun/native thread for explicit Reopen. A TeamRun completion or an
  empty mailbox never substitutes for Close.
- **Reopen/Resume** increments the MemberRun runtime generation and uses the
  recorded native thread id with the provider's real
  `thread/resume` operation. Harness never reconstructs a session by replaying
  its coordination records.
- **Deactivate/Retire** permanently ends the coordination identity; it cannot
  reopen.
- **Disconnect** records a recoverable lifecycle action and resumes the same
  native thread under the TeamRun supervisor. It does not replay an already
  delivered Work version.

Physical app-server handles remain process-local, but a durable Team Supervisor
lease is the cross-process authority and publishes the owning service's
loopback locator. Dashboard/MCP/CLI clients route controls to that service,
which fences the generation again before `turn/steer`, `turn/interrupt`, or
Close. Another process cannot attach or claim mail while that lease is live.
Re-running start after expiry or release acquires a new generation and
reattaches every unclosed Member to its recorded thread.

The owner verifies that the app-server transport is live before claiming
queued mail. If that probe fails, the message remains queued and the owner
reattaches the recorded thread first. Close intent is durably latched before
process teardown, so losing the loopback receiver during a close/reattach race
cannot revive the Member.

## Mailbox Delivery

Codex does not poll Harness storage. Harness owns the Member mailbox and the
app-server adapter accepts eligible envelopes:

```text
WorkDelivery (Work plane) or CanonicalMessageDelivery (Message plane)
  -> target NodeDaemon under current Supervisor authority claims its row
  -> exact recipient AgentSession id/generation is frozen
  -> turn/start on the bound thread
  -> provider turn id records native acceptance
  -> provider-native turn/session remains execution truth
  -> provider_received / recipient acknowledgement in Harness
```

Ordinary Host/peer messages queued while a turn is busy wait for the next
eligible round. They do not interrupt the current turn. `provider_received`
means the adapter recorded a native provider receipt for that exact delivery;
semantic
understanding requires an explicit Work transition or conversational reply.

When a turn completes, the Member returns to `idle` and the adapter
keeps polling. Later mail starts one new turn on the same thread. Work
acceptance and TeamRun completion do not stop that loop (nor did the append to
the retired Mission Log, or Mission completion); only explicit Close does.

The complete message-selection and delivery contract is in
[Codex Message Delivery](codex-message-delivery.md).

## Codex Host Inbox

Codex Members and a Codex Host use different adapters. A Member app-server is
owned by Harness; the user's Codex Desktop Host task is normally owned by the
Desktop app. TeamRuns therefore bind the Host explicitly:

```bash
firm team-run create ... \
  --host-surface codex-app \
  --host-thread-id <Codex hook session_id>
```

The Star Harness hook queries `team-run host-inbox` with that exact pair.
`SessionStart` and `UserPromptSubmit` surface actionable mail. When mail exists
at `Stop`, Codex's native continuation protocol keeps the same task running
once and supplies the bounded Inbox summary. `stop_hook_active` prevents a
continuation loop.

If mail arrives after Desktop is already idle, no hook event occurs. Unless
Harness owns a live app-server connection for that Host, the mail remains
durable until the next prompt/resume. Known thread identity is not live
connection ownership. Full contract: [ADR 0040](../../decisions/0040-native-host-inbox-delivery.md).

## Collaboration And Planning

The current Work plus its version is the durable Member responsibility.
Harness has no Codex-specific Plan Mode, Plan Gate, or Goal object. The Host can
ask for planning through ordinary Work-linked Markdown conversation:

```text
Host -> Member: PLAN: Return a plan first; do not execute.
Member -> Host: PLAN: <Markdown proposal>
Host -> Member: DECISION: Revise these points.
Member -> Host: PLAN: <revised proposal>
Host -> Member: DECISION: Execute.
```

Codex may use native Goal/Plan features internally in the same native thread,
but their raw updates remain provider-native activity. They do not change Work
owner/status, Harness permission, or Host acceptance. See
[ADR 0039](../../decisions/0039-ordinary-member-planning-and-durable-mailbox-delivery.md).

## Native Continuation

Codex app-server exposes a native Goal continuation path, but Harness must not
confuse that capability with the current Work or run it beside the
ordinary Host-driven loop. The provider-neutral contract is
Member Continuation Model.

A 2026-07-28 canary proved the failure mode: `thread/goal/set(active)` started a
provider-owned cycle while Harness also called `turn/start`. Two top-level
turns then ran concurrently in the same native thread and writable worktree,
and the provider-owned turn used a different sandbox posture. The Goal turn
eventually completed the work; that does not make dual-driver operation valid.

The required posture until the adapter implements and live-validates an
exclusive `provider_driven` lease is `host_driven`. The Agent Team adapter now
enforces that posture: it snapshots `execution_driver=host_driven`, starts
mailbox work with `turn/start`, and does not call `thread/goal/set`. Native Goal
remains a provider capability, not an active Team scheduler:

- do not combine `thread/goal/set(active)` with Harness `turn/start` for one
  Work;
- retain native Goal state only as an on-demand provider projection;
- do not infer Host acceptance from native Goal satisfaction; and
- keep version/mode compatibility `review_required` when continuation or
  permission behavior has not been canaried.

`--max-concurrency` limits active provider turns through a shared execution
lease. Idle persistent Members remain alive and addressable without consuming a
permit.

## Correlated Provider Questions

User questions that pause the provider become correlated
`provider_interaction_request` Messages with exact provider option ids; the
Lead's causation-linked `provider_interaction_response` routes back to the same
live app-server request. The AgentSession freezes one permission ceiling before
start. Codex runs with `approvalPolicy=never`; an unexpected permission callback
fails closed instead of opening a second approval workflow.

A provider `completed` tool update is not an answer, approval, or semantic
success. Unknown reverse-RPC methods fail closed and surface as adapter gaps.

## Native Session And Activity

Codex rollout/state storage is the sole execution truth for chat, turns, tool
calls, commands, file events, native subagents, and resume. Harness stores:

- MemberRun identity and selected `ProviderIntegrationProfile`;
- Work/WorkEvent/WorkDelivery and ordinary Work-linked conversation;
- `NativeSessionRef` locator, version and availability;
- correlated `provider_interaction_request`/`provider_interaction_response`
  Messages and real RuntimeCommand receipts;
- submitted Work result, explicit Host acceptance and outcome, plus artifact
  and check references when the Work's completion criteria require them.

The adapter reads native activity on demand into a bounded, sanitized,
ephemeral projection. Browser code never reads private Codex files directly.
Thinking may appear only as a sanitized transient live preview; it is never
persisted, replayed, forwarded to peers, or accepted as evidence.

Codex-native subagents remain internal to the Member that invoked them.
Harness may show honest native child activity, but it does not create an
implicit MemberRun or claim lifecycle control it does not possess.

## Workspace And Permissions

Provider cwd resolves in this order:

```text
MemberRun.provider_cwd_hint
  > AgentTeamRun.execution_root
  > registered project_root
```

It never resolves to the centralized Harness store. cwd is an instruction,
Skill, Plugin, MCP, and permission boundary.

The current temporary Team policy launches Codex with
`danger-full-access` and approval policy `never`. This is an explicit product
policy, not a provider capability claim. Work context/criteria, `owned_paths`,
and worktree choice define responsibility, but they are not an enforced
filesystem sandbox under this policy.

## Host Operations

The Host has one coherent lifecycle surface across CLI, HTTP, MCP and
Dashboard application logic:

```text
create/add member
create/assign/claim/review Work; send ordinary Work-linked conversation
read status, inbox, outbox and member detail
answer correlated provider Message
steer current turn when supported
interrupt current turn
explicitly close member runtime
resume from native session
```

MCP uses `team_run_close_member` and `team_run_reopen_member`. CLI uses:

```bash
firm team-run close-member --id <team-run-id> \
  --member-run-id <member-run-id> --requested-by host --reason <reason>
firm team-run reopen-member --id <team-run-id> \
  --member-run-id <member-run-id> --reopened-by host --reason <reason>
```

## Provider Version Review

Provider capability claims are execution-mode and version specific. Run:

```bash
firm member providers --fail-on-review
```

An unreviewed Codex version is `review_required`, not silently compatible.
Provider maintenance follows ADR 0031's Agent-managed, one-Provider-at-a-time
update loop. It must not hot-replace an active MemberRun/native session and
must retain a rollback path until deterministic and live acceptance pass.

Current local probe at this documentation closure: Codex
`0.148.0-alpha.9`, compatibility `current`, adapter contract
`codex-app-server-v1`, reviewed on 2026-08-16. The exact-version DEV-26 live
canary proved a new native thread, one completed round, explicit process Close
with the thread retained, exact `thread/resume`, a shell command that became
active before `turn/interrupt`, matching `turn/completed=interrupted` plus idle
observation, a later completed round on the same thread, and a second explicit
Close. The generated 0.148 schemas and deterministic matrix additionally cover
strict thread/turn identity, effective permission receipts, failed terminal
projection, and safe `requestUserInput` routing.

The live protocol matrix was not repeated merely to add coordination records.
Mission `mission-dev26-codex-01480-v1` supplies that missing durable half:
TeamRun `team-run-1786846271494-p46553-0`, MemberRun
`member-run-1786846271495-p46553-1`, and accepted Work
`work-1786846272156-p46553-4` are bound to Codex 0.148.0-alpha.9 native Thread
`01a00856-e54a-7bb1-8a09-cd6c61b2e61a`. The provider-owned rollout at
`~/.codex/sessions/2026/08/16/rollout-2026-08-16T10-11-47-01a00856-e54a-7bb1-8a09-cd6c61b2e61a.jsonl`
contains the exact no-write response and bound Work start/submit commands. Host
accepted revision 4, explicit Close settled `applied`, an idempotent retry
reconciled `already_closed` without a second provider effect, the TeamRun
completed, and the Mission recorded closeout evidence.

That canary did not exercise `turn/steer` or activate a provider-native Goal.
Those semantic bindings remain `review_required`; strong quiesce/release also
remains degraded because the app-server cannot prove detached writable-child
drain and durable rollout flush. This point-in-time evidence must not be
generalized to a later Codex build; always rerun the provider audit and live
upgrade gate.

## Account Capacity

Compatibility is not availability. Codex account capacity is read separately
through the reviewed `account/read` and `account/rateLimits/read` app-server
RPCs, which complete after `initialize` + `initialized` and therefore require
no `thread/start`, no rollout, and no billable turn:

```bash
firm member preflight --provider codex --json
```

The contract, classification thresholds, start guard, and truth matrix live in
[provider-capacity.md](provider-capacity.md).

## Acceptance

A Codex Team integration claim requires:

1. a real `codex_app_server` MemberRun and active Work/version;
2. a resolvable native Codex session;
3. ordinary mail delivered to the same live Member across multiple turns;
4. at least one verified lifecycle operation with terminal acknowledgement;
5. honest correlated Message routing when a provider question occurs;
6. explicit Work submission/Host acceptance and useful evidence references;
7. no copied transcript, tool stream, file stream, subagent transcript, or
   thinking in Harness storage; and
8. CLI and Dashboard reconstruction of the same coordination facts.

Deterministic tests prove the adapter contract. Claims about real Codex
execution additionally require a proportional live canary.

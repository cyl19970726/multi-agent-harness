# Agent Team Foundation Closure Plan

```text
status: in_progress
owner_role: execution-foundation
canonical_for: implementation sequence and acceptance boundary for multi-client
  Agent Team messaging, provider control, crash recovery, and the Organization
  runtime substrate
replace_when: ADR 0044 is enforced by schemas, stores, provider adapters,
  acceptance tests, operator surfaces, and live dogfood
```

## Outcome

Close the gap between the current persistent Agent Team implementation and a
provider-neutral runtime that can safely support both bounded execution teams
and long-lived Organization Agents.

The completed foundation must let an external Human or Agent:

- open one durable Agent identity from Dashboard, Codex, Claude Code, Kimi,
  CLI, MCP, or a future client;
- send readable information without impersonating the Host or another Member;
- choose ordinary queued delivery or a real same-turn Steer when the selected
  Provider mode supports it;
- inspect, interrupt, resume, or explicitly Close the exact `MemberRun` and
  provider-native session;
- survive an idle process, Provider transport loss, Harness restart, or client
  reconnect without losing mail, replaying an accepted Assignment, or creating
  two top-level execution drivers; and
- reconstruct coordination from Harness while leaving transcript, tools,
  commands, file activity, subagents, and thinking in provider-native storage.

This plan does not change Company Store, Execution Space, or Project Binding
ownership. It consumes their stable selection and Workspace contracts.

## Existing truth and gaps

| Area | Implemented truth | Remaining gap |
| --- | --- | --- |
| Persistent Member modes | Codex `codex_app_server`, Claude `claude_agent_sdk`, and Kimi `kimi_acp` | proportional live canary remains |
| Native execution truth | Harness stores a locator, not a second transcript | native receipt reconciliation still requires operator judgment after ambiguous crash |
| Team coordination | correlated Assignment, message, Handoff, Inbox/Outbox, atomic ACK | cross-process claim, Provider receipt, two-service control routing, MCP provenance, and full deterministic regression passed; ambiguous post-crash receipts still require explicit reconciliation |
| Member lifecycle | idle, disconnected, resume, Interrupt, explicit Close | durable lease, `TeamMemberCloseRequest`, and cross-service loopback routing implemented; Provider handles remain process-local behind the owning Supervisor |
| Host Inbox | exact native surface + thread binding | Codex/Claude/Kimi safe-boundary hooks implemented; live canary remains |
| External input | typed actors plus stable Agent Inbox → MemberRun route | remote authentication/policy remains additive gateway work |

As of 2026-07-29 the deterministic repository gate and one real Codex
generation-1 → generation-2 reattach/Steer canary pass. Claude live execution
is not claimed because the current probe cannot locate the reviewed Agent SDK
package beside the runner, and installed Kimi `0.29.1` remains
`review_required`; neither Provider was upgraded, installed, or silently
treated as accepted.

Organization identity work is additive and independently owned. This plan
provides the runtime and mailbox substrate it can consume; it does not edit the
Organization UI or create a second Agent identity.

## One model, several clients

### Identity, runtime, and attachment

```text
Agent identity
  ├─ Organization profile and authority
  ├─ reusable AgentTeam membership
  └─ MemberRun
       ├─ provider-native session
       ├─ durable coordination address
       └─ RuntimeControlLease

ClientAttachment
  ├─ observe: read Harness/native projections
  └─ control: route commands through the current Supervisor lease locator
```

A Codex Desktop, Claude Code/Desktop, Kimi client, Dashboard, CLI, or MCP
surface may observe the same Agent. Observation does not grant permission to
start another Provider turn.

While Harness owns the execution driver, an external native client routes
Message, Steer, Interrupt, and Close through the owning Team Supervisor.
Opening the same stored session in a second Provider process is
observation-only unless the Supervisor explicitly transfers control.

### Managed and external Hosts

`Host` is a Team role, not a Provider:

```text
Managed Host
  Harness owns the persistent Provider connection and may wake an idle task.

External Host
  A user-owned Codex Desktop, Claude Code, or Kimi task is bound by exact
  native surface + session/thread id. Plugin hooks pull mail at safe boundaries.
```

Every TeamRun records who the Host actor is, where its native task lives, and
whether Harness owns a managed connection:

```text
host_actor
host_surface + host_thread_id
host_control_mode = managed | external
```

## Typed external messages

External messaging is first-class input, not a forged Member message.

```text
TeamActorRef =
  HostRef | MemberRunRef | AgentMemberRef | OperatorRef | ServiceRef

TeamRecipientRef =
  HostBinding | MemberRunRef | AgentMemberRef
```

Compatibility fields remain readable. New Dashboard, Plugin, CLI, and MCP
writes derive sender identity from the authenticated caller or supervising
connection. They do not accept an arbitrary Member id as proof of authorship.

Direct Agent identity routing follows four rules:

1. exactly one eligible runtime: route to that `MemberRun`;
2. no eligible runtime: keep the original message queued in the Agent Inbox;
3. several eligible runtimes: require an explicit target or routing policy;
4. routing appends a causation-linked TeamMessage and never rewrites provenance.

Company Work, Docs, Approval, and Finance references remain subject-owned
truth. A message may reference them; it does not become those records.

## Message versus control

| Operation | Contract |
| --- | --- |
| Message | Durable readable input with `next_boundary` delivery. It never silently interrupts. |
| Steer | Same-turn injection through a reviewed native primitive. Unsupported modes disable it. |
| Interrupt | Stop only the current turn and wait for a terminal Provider acknowledgement. |
| Resume | Reattach the exact native session and continue unconsumed mail. |
| Close | End the Member runtime explicitly without deleting Agent identity or native history. |

Unsupported Steer never becomes a silently queued pseudo-Steer. The operator
may deliberately choose Message, or Interrupt followed by Message.

## Delivery facts

Transport facts and semantic decisions are separate:

```text
queued
  -> claimed by Supervisor generation
  -> accepted by Provider/native request
  -> acknowledged by recipient
  -> correlated reply / Handoff / Host acceptance
```

If a Supervisor dies after claiming and before recording a Provider receipt,
the delivery becomes uncertain. Recovery must reconcile it against
provider-native state or require an explicit operator decision; it must not
blindly replay the message.

Member-to-Host creation currently means “accepted by Harness,” not “read by
Host.” Operator surfaces must keep those labels separate.

## Member state matrix

| State | Message | Steer | Interrupt | Recovery |
| --- | --- | --- | --- | --- |
| working | queue; inject at next reviewed boundary | real primitive only | native interrupt + receipt | never start a second top-level turn |
| idle | claim once and start the next turn | reject; use Message | `no_active_turn` | keep native session |
| waiting interaction | queue unless it resolves the request | do not bypass authority | cancel the paused turn | resume the same request/session |
| disconnected | keep queued | reject | report no live control | reattach exact native session |
| supervisor down | keep queued | reject | never claim success | acquire a new lease before control |
| stopped | do not auto-resume | reject | already stopped | explicit lifecycle action only |

## Host state matrix

| Host state | Managed Host | External Host |
| --- | --- | --- |
| working | ordinary mail waits for a safe boundary | Stop/safe-boundary hook may continue the exact task once |
| idle | owned connection may start the next Host turn | next prompt, SessionStart, or explicit resume pulls mail |
| waiting for authority | route only the matching interaction resolution | surface mail without impersonating Human approval |
| crashed | retain Inbox; resume exact native session | retain Inbox until exact task resumes or is rebound |
| resumed | claim once, inject, then ACK | bounded hook context; explicit Inbox read remains authority |

## Provider control matrix

| Team mode | Ordinary input | Steer | Interrupt | Resume | Close truth |
| --- | --- | --- | --- | --- | --- |
| Codex `codex_app_server` | `turn/start` | `turn/steer` | `turn/interrupt` | `thread/resume` | Harness ends its owned app-server runtime |
| Claude `claude_agent_sdk` | streaming input | unsupported until reviewed | `query.interrupt()` | SDK session resume | runner Close |
| Kimi `kimi_acp` | `session/prompt` | unsupported | `session/cancel` | `session/load` / `session/resume` | ACP has no native session-close; Harness ends its client runtime |

Provider release review remains version and mode specific. Unsupported
capability is an honest disabled state, not fallback to one-shot execution.

## Durable Team Supervisor

The Supervisor is the single runtime controller:

```text
TeamSupervisorLease
  team_run_id
  supervisor_id
  generation
  process/service locator
  acquired / heartbeat / expires / released
  host binding
```

It must:

1. acquire a durable compare-and-append lease before Provider side effects;
2. claim one delivery before Provider injection;
3. record Provider request/session/turn receipts;
4. renew while controlling live runtimes and stop immediately if renewal fails;
5. recover incomplete claims without blind replay;
6. resume the same native session;
7. keep Member lifecycle independent of Wave, Mission, TeamRun completion, and
   client presence; and
8. publish a loopback control locator and fence every routed operation against
   the current lease immediately before the Provider primitive.
8. Close only through an explicit authorized action.

## Operator contract

Agent and Member pages expose the same application service:

- Message;
- Steer only while working and reviewed;
- Interrupt only with a real active turn;
- Resume only with an available native session and no competing lease;
- explicit Close;
- open/import native session as observation;
- inspect Inbox, Outbox, claims, correlation, PendingInteraction, Supervisor
  health, Workspace, and native locator.

The UI shows typed avatars, queued/claimed/provider-accepted/acknowledged/failed
receipts, observation versus control, disabled-control reasons, runtime
generation, and last Supervisor heartbeat.

## Organization layering

Organization consumes this foundation:

```text
Human Owner
  -> Lead Agent identity / managed Host
      -> long-lived Governance Agent MemberRuns
          -> ordinary messages request temporary or durable capability
```

- reporting is an Organization relation, not nested TeamRun process ownership;
- the root Supervisor controls Provider runtimes;
- a provider-native subagent remains an internal implementation detail;
- a temporary specialist is a Host-managed MemberRun;
- a recurring role is a governed durable Agent identity plus Organization
  profile and reporting relation; and
- direct Agent-page messages use the durable Inbox and Supervisor router.

## Implementation Waves

### Wave 1 — Freeze the contract

- accept ADR 0044;
- register this implementation plan;
- remove contradictory Kimi one-shot and pseudo-Steer wording;
- define stable Host surface and control-mode identifiers;
- document Kimi Plugin installation and current live-canary truth.

Acceptance: docs governance reports one authority per rule and no active
contract claims unsupported Host wake, Steer, native Close, or Desktop control.

### Wave 2 — Durable Supervisor and delivery

- implement cross-process lease, heartbeat, generation, and status;
- add atomic TeamMessage claim/provider-receipt transitions;
- make recovery preserve uncertain claims rather than replay them;
- expose deterministic reconciliation.

Acceptance: concurrent Supervisors cannot inject one message twice; kill and
restart retains the same native binding and never blindly replays a claim.

### Wave 3 — Typed actors and direct Agent Inbox

- add typed sender and recipient provenance;
- stop rendering external input as Host/Member authorship;
- route direct Agent messages to one eligible runtime or durable waiting Inbox;
- preserve compatibility reads for historical TeamMessages.

Acceptance: sender provenance is stable; offline mail survives; ambiguous
multi-runtime routing fails clearly.

### Wave 4 — Provider parity

- fix explicit Codex, Claude, and Kimi Host gateways;
- add Kimi `UserPromptSubmit`;
- use Provider-correct Stop continuation;
- test busy, idle, waiting, disconnected, down, resumed, and closed states.

Acceptance: deterministic provider tests plus bounded live canaries prove
ordinary input, Host Inbox, Member Inbox, Interrupt, Resume,
PendingInteraction, Close truth, and native-session reconstruction.

### Wave 5 — Multi-client UX and Plugin

- surface Supervisor/receipt/control state in CLI, API, Dashboard, and Plugin;
- distinguish observation from control;
- show disabled-control reasons and reconnect history;
- update canonical Skills and generated Plugin copies.

Acceptance: concurrent Dashboard and native-client observation never creates a
second driver, and every control reaches the owning Supervisor.

### Wave 6 — Dogfood and Organization handoff

- run a real self-host Mission with persistent Members;
- exercise Host, peer, and external messages plus restart recovery;
- record handoff, checks, native Session refs, Wave advances, and closeout;
- provide the stable runtime contract to the Organization identity branch.

Acceptance: a future Host can reconstruct why the work existed, who controlled
each side effect, what each Provider accepted, what survived recovery, and why
the Mission closed.

## Final gate

The foundation is complete only when:

1. every input has a typed sender, recipient, correlation, and delivery intent;
2. every live Provider side effect has one durable Supervisor and delivery
   claim;
3. busy, idle, waiting, disconnected, supervisor-down, stopped, and resumed
   behavior is deterministic and tested;
4. observation and control are visibly different;
5. Provider differences are explicit and version-reviewed;
6. receipt, transport ACK, reply, Handoff, and acceptance are not collapsed;
7. CLI, API, Dashboard, Plugin, Harness records, and native Sessions agree; and
8. Organization adds governance without inventing another runtime or process
   manager.

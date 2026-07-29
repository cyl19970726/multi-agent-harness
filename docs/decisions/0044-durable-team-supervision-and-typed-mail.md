# ADR 0044: Durable Team Supervision And Typed Mail

```text
status: accepted
owner_role: architecture
canonical_for: Agent Team Supervisor ownership, typed coordination actors,
  delivery claims, multi-client control, and Provider lifecycle truth
```

## Context

Agent Team members now use persistent Codex app-server, Claude Agent SDK, and
Kimi ACP sessions. A Member can remain addressable across many Host-plan Waves,
ordinary provider turns, interruptions, and idle periods.

The first implementation still has two unsafe boundaries:

1. the Team Supervisor and live Provider controls exist only in one process;
2. a Member loop reads a queued TeamMessage before Provider injection and
   records delivery afterwards, without a durable cross-process claim.

Two Harness processes can therefore attach the same TeamRun or observe the same
queued message. Public message surfaces also accept a string sender id; Team
membership validation does not prove that the caller is that Member.

Organization and multi-client use make these gaps product-critical. A durable
Agent may be visible in Dashboard and a Provider-native application while its
runtime remains controlled by Harness. Observation must not accidentally create
a second execution driver.

## Decision

### One Supervisor lease per TeamRun

Before starting or resuming a Provider runtime, a Supervisor acquires a durable
latest-wins `TeamSupervisorLease` under the Harness Store write lock:

```text
TeamSupervisorLease
  team_run_id
  supervisor_id
  generation
  owner_process_id / service locator
  status
  acquired_at
  heartbeat_at
  expires_at
  released_at?
```

An unexpired lease held by another Supervisor rejects attachment. The owner
renews the lease while it controls member sessions. Losing renewal authority
means losing control authority: the process must stop issuing new Provider
operations.

An expired lease may be replaced with a higher generation. Provider-native
session ids and MemberRun identities remain unchanged. Lease replacement does
not itself prove that an incomplete Provider operation is safe to retry.

The active lease publishes a local-only service locator. CLI, MCP, Dashboard,
and another Harness service process read that locator and route live control to
the owning Supervisor. The owner validates the lease again immediately before
touching its process-local Provider handle. A stale generation, expired lease,
unroutable locator, or missing handle is an explicit failure; no client falls
back to starting a second Provider driver.

TeamRun, Mission, Wave, UI task, and Supervisor lifecycles remain independent.
Completing or advancing a Wave never releases or closes a Member. A Supervisor
release never deletes provider-native history.

### Claim before Provider side effect

Each TeamMessage delivery progresses independently:

```text
queued
  -> claimed(supervisor_id, generation, claim_id, expiry)
  -> provider accepted(native receipt)
  -> recipient acknowledged
```

Claim is an atomic compare-and-append Store operation. It verifies:

- the current TeamRun Supervisor lease;
- the exact latest message row;
- the recipient and still-queued delivery;
- no active claim by another generation.

The Provider adapter records acceptance only after the selected native protocol
returns its real request/turn/session receipt. A crash between claim and receipt
leaves an uncertain claim. Recovery must reconcile it against Provider-native
state or require an explicit operator choice. It does not silently requeue.

Acknowledgement proves recipient intake only. Correlated reply, Handoff,
review, Host acceptance, and Mission closeout remain separate facts.

### Typed actors and recipients

New coordination writes carry typed provenance:

```text
TeamActorRef =
  host | member_run | agent_member | operator | service

TeamRecipientRef =
  host | member_run | agent_member
```

Historical `from_member_id` and `to_member_ids` fields remain readable. New
writes populate both the typed fields and compatibility projections.

Caller context chooses the actor:

- a live member connection may author only as its bound MemberRun;
- a bound Host connection may author as that Host;
- Dashboard/CLI without a Host or Member binding authors as an Operator;
- a Service uses an explicitly configured service identity;
- a durable Agent identity authors as `agent_member`, not as whichever runtime
  happens to be active.

An unbound MCP connection is a Host/operator/service surface. It may not choose
`member_run` or `agent_member` merely by supplying an id. Member-originated
writes come from the bound Provider runtime and carry that connection's
authorship provenance.

Store access remains the local administrative trust boundary in this release.
The schema prevents provenance collapse; remote authentication and policy may
strengthen who can create each actor type without changing message semantics.

### Direct Agent Inbox routing

A message addressed to a durable Agent identity is not rewritten as a Team
member message.

- One eligible MemberRun: append a causation-linked runtime TeamMessage.
- No eligible MemberRun: retain it in the durable Agent Inbox.
- Several eligible MemberRuns: require an explicit target or routing policy.

Starting or resuming a runtime may route waiting mail only after the current
Supervisor lease is acquired. The original sender and Agent-level message
remain durable.

### Message and Steer are different operations

An ordinary Message is durable content for the next safe input boundary. It
never implicitly interrupts a current turn.

Steer is a control request for the current active turn:

| Mode | Same-turn Steer |
| --- | --- |
| Codex `codex_app_server` | real `turn/steer` |
| Claude `claude_agent_sdk` | unsupported until an exact SDK operation is reviewed |
| Kimi `kimi_acp` | unsupported |

Unsupported Steer is disabled with a reason. It is not recorded as successful
and silently queued. The caller may deliberately use Message, or Interrupt then
Message.

Interrupt stops one current Provider activity and waits for a terminal native
acknowledgement. Close ends the Harness-owned runtime. Kimi ACP has no native
session-close operation; closing its client runtime must not claim otherwise.

### Observation and control attachments

Many clients may observe one MemberRun and native session. Only the active
Supervisor lease may drive it.

```text
observe attachment: read status, coordination, and native projection
control attachment: route controls through the current Supervisor
```

Opening a Codex or Claude native session in another client does not transfer
control. Explicit transfer requires the old owner to stop driving, a terminal
control receipt when applicable, and a new lease generation.

### Host delivery

A TeamRun binds Host mail to:

```text
typed host actor
host_surface
host_thread_id
host_control_mode = managed | external
```

A managed Host has a Harness-owned persistent Provider connection. An external
Host is a user-owned Provider task; Plugin hooks may pull bounded Inbox context
only at supported safe boundaries.

Member-to-Host creation records that Harness accepted the message. It does not
mean the native Host task read it. Operator surfaces use separate labels for
stored, injected, acknowledged, replied, and accepted.

### Stable Provider surfaces

New writes use:

```text
codex-app
claude-code
kimi-cli
```

Compatibility readers may normalize historical aliases. Provider manifests and
hooks must set or derive the exact canonical surface rather than falling back
to another Provider's value.

### Native execution truth remains Provider-owned

Supervisor leases, TeamMessage claims, control acknowledgements, Handoffs, and
evidence references are Harness truth. Provider transcripts, tool calls,
commands, file activity, subagent activity, turn history, and thinking remain
in Provider-native storage.

Recovery uses the recorded native session id and a verified Provider resume
operation. Harness never reconstructs a replacement transcript from Team
events.

## State contract

| Member state | Ordinary Message | Steer | Interrupt |
| --- | --- | --- | --- |
| working | queue for reviewed safe boundary | reviewed native primitive only | native interrupt |
| idle | claim and start next native cycle | reject | `no_active_turn` |
| waiting interaction | queue unless resolving request | reject | cancel paused turn |
| disconnected | retain queued | reject | no live-control result |
| supervisor down | retain queued | reject | no completed-control claim |
| stopped | wait for explicit lifecycle action | reject | already stopped |

Provider transport loss moves an unclosed Member to disconnected/waiting while
retaining its native binding. Supervisor restart does not create a new
MemberRun. Explicit Close is the only normal action that terminalizes the
runtime.

An idle Supervisor verifies that the Provider transport is alive before it
claims queued mail. If the previous transport ended, it first reattaches the
same native session and only then takes delivery ownership. This avoids
manufacturing an uncertain claim before `turn/start` can reach the Provider.
Close is durably latched by the owning Supervisor before process-local
dispatch; a transport-generation race may return `supervisor_close_latched`
instead of a turn-level acknowledgement, but the accepted Close remains in
force across reattachment.

## Consequences

### Positive

- concurrent Harness processes cannot legitimately drive one TeamRun;
- queued coordination cannot be duplicated through an ordinary race;
- ambiguous post-crash delivery is visible instead of silently replayed;
- external input retains provenance;
- Dashboard, CLI, MCP, Plugins, and future Organization Agents share one
  lifecycle model;
- additional Providers can implement a small capability matrix without
  pretending to be Codex.

### Costs

- Store transitions gain compare-and-append operations and lease time;
- crash recovery may require Provider-specific reconciliation or a Human
  decision;
- the owner service must keep a loopback control endpoint alive and routable
  for the duration of its lease;
- compatibility fields remain until historical stores no longer need them.

### Explicit non-goals

- no Harness Goal, Plan Mode, Plan Gate, Task Graph, conditional delivery, or
  Wave executor ownership;
- no transcript, tool stream, file stream, subagent stream, or thinking copy;
- no automatic semantic acceptance from delivery or Provider completion;
- no implicit client control transfer;
- no provider upgrade as part of this decision.

## Implementation order

1. add lease and delivery-claim schemas plus Store atomic operations;
2. make all three persistent Member adapters claim before Provider input;
3. add typed actor compatibility projections and direct Agent Inbox routing;
4. make controls target the active Supervisor service;
5. align Codex, Claude, and Kimi Host hooks;
6. expose state in CLI/API/Dashboard/Plugin;
7. run deterministic concurrency tests and real-provider dogfood.

The current execution checklist is
[Agent Team Foundation Closure Plan](../product/agent-team-foundation-closure-plan.md).

## Validation

The accepted implementation must prove:

- two concurrent Supervisor starts yield one owner;
- a second Harness service can route control to the owning Supervisor, while a
  stale generation is fenced before the Provider call;
- lease expiry creates one higher generation;
- two concurrent claims yield one claim;
- an unfinished claim is not automatically replayed;
- ACK is idempotent and does not imply semantic acceptance;
- external actors are not rendered as Host or Member;
- unbound MCP calls cannot author as `member_run` or `agent_member`;
- exact Host binding never leaks another task's Inbox;
- Codex, Claude, and Kimi expose only reviewed controls;
- restart resumes the same native session;
- CLI and Dashboard can reconstruct the same state; and
- a native Mission/Wave dogfood run records handoff, checks, advances, and
  closeout.

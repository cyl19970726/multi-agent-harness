# Agent Member Continuation Model

```text
status: canonical architecture contract
owner_role: provider-integration
canonical_for: provider-neutral Member execution ownership, continuation, completion, and Workspace lease semantics
```

This document is the smallest required context for adding a new Agent Team
provider or changing how a persistent `MemberRun` continues work. It extends
the mailbox and runtime substrate in [Agent Runtime](agent-runtime.md) without
creating a Harness `Goal`, Plan Gate, task graph, or provider transcript copy.

## The Mental Model

Star Harness owns durable collaboration. The selected provider owns native
execution:

```text
Mission
  -> ordered Host-plan Wave
  -> independent AgentTeam / AgentTeamRun
  -> MemberRun
       -> correlated Assignment
       -> Harness Mailbox
       -> Workspace
       -> provider-native session
            -> native plan, goal, turns, tools and subagents
```

These layers answer different questions:

| Layer | Question | Authority |
| --- | --- | --- |
| Mission | Why does the long-running work exist? | Harness |
| Wave | What is the Host's current plan and judgment? | Harness |
| Assignment | What result does this Member own? | Harness `TeamMessage` correlation |
| MemberRun | Which durable team participant owns the lane? | Harness |
| Mailbox | What coordination has been sent, delivered and acknowledged? | Harness |
| Native session | What did the provider actually execute? | Provider |
| Native continuation | Why does the provider start another execution cycle? | Provider plus selected Adapter contract |

An Assignment is the durable responsibility contract. A provider-native Goal
or completion condition is a session-local way to continue that Assignment. It
must never become a second product Goal or silently replace Host acceptance.

## Continuation Has Two Independent Axes

### Execution Driver

`execution_driver` answers who may start the next provider execution cycle:

| Driver | Meaning | Agent Team use |
| --- | --- | --- |
| `host_driven` | Harness claims eligible mail and starts the next provider cycle. | Default when native continuation is absent, unsafe or not observable. |
| `provider_driven` | One provider-native continuation mechanism starts later cycles until its condition stops it. | Allowed only when the Adapter can inspect and control it honestly. |
| `bounded` | One invocation owns a finite result and then exits. | Dynamic Workflow only; not a persistent Team Member mode. |

One MemberRun must have exactly one active execution driver. Setting a native
Goal and also issuing an independent Harness `turn/start` violates this
contract.

### Completion Policy

`completion_policy` answers who decides that work may stop:

| Policy | Meaning |
| --- | --- |
| `member_declared` | The Member reports that its current work is done. |
| `provider_evaluator` | A provider-native evaluator decides its continuation condition is satisfied. |
| `deterministic_check` | A named command, check or external condition is satisfied. |
| `host_reviewed` | The Host explicitly accepts a correlated Handoff. |

Policies compose. Provider Goal achievement can stop provider continuation,
but it never substitutes for `host_reviewed` when the Assignment requires Host
acceptance.

## Execution Lease

The hard concurrency invariant is not “one Member has one Turn.” Native
providers may perform many turns or cycles inside one Assignment.

The invariant is:

> One native session and writable Workspace have one top-level execution
> driver holding the execution lease at a time.

Valid:

```text
provider_driven:
  native Goal -> cycle 1 -> cycle 2 -> cycle 3 -> satisfied

host_driven:
  mail 1 -> provider cycle 1 -> idle -> mail 2 -> provider cycle 2
```

Invalid:

```text
Harness turn/start ----\
                        +--> concurrent writes in the same Workspace
native Goal loop ------/
```

Provider-internal read-only parallelism and native subagents remain internal to
the lease holder. Concurrent writable lanes require separate worktrees or an
explicit integration boundary.

## Continuation State Is A Projection

Harness must not create a generic persisted Goal object. The Adapter exposes a
bounded current projection:

```text
NativeContinuationProjection
  driver: host_driven | provider_driven
  state: inactive | active | evaluating | waiting | blocked | satisfied |
         interrupted | unknown
  condition_summary
  native_ref
  cycle_count?
  resource_usage?
  latest_reason?
  observed_at
  capability_confidence: verified | review_required | unavailable | unknown
```

The provider-native store remains authoritative. Harness may keep the selected
driver, capability snapshot, control acknowledgements and coordination facts;
it does not mirror native Goal history, turn history or evaluator reasoning.
The Dashboard reads this projection on demand and shows `unknown` when the
Adapter cannot inspect it.

## Mailbox Behavior Does Not Change

Harness remains the communication authority in both driver modes:

| Situation | Required behavior |
| --- | --- |
| Member idle under `host_driven` | Claim and deliver the next eligible message. |
| Provider transport unhealthy before claim | Leave mail queued; current Supervisor reconnects the recorded native session first. |
| Member busy | Queue ordinary messages; never silently interrupt. |
| Provider continuation active | Inject only through a verified safe provider operation or cycle boundary; otherwise leave mail queued. |
| Host chooses Steer | Use the selected mode's real current-activity injection and terminal acknowledgement. |
| Provider asks for authority | Create `PendingInteraction`; do not infer approval from tool completion. |
| Native continuation satisfies its condition | Record/project the provider fact, then await Handoff/Host acceptance as required. |
| Host explicitly closes Member | Latch terminal Close before teardown; no driver, delivery, or later Supervisor may revive it. |

Ordinary message visibility is an explicit execution-mode capability, not a
uniform mailbox promise:

| Team execution mode | `ordinary_message_boundary` | Host expectation |
| --- | --- | --- |
| Claude `claude_agent_sdk` | `in_turn` | Streaming input may reach the active provider turn. |
| Codex `codex_app_server` | `next_round` | Mail remains queued until the next native round. |
| Kimi `kimi_acp` | `next_round_batched` | Mail is claimed and rendered together at the next round boundary. |

This field describes delivery timing only. Provider-native transcripts remain
the sole turn/execution record and are never copied into TeamMessage storage.

Self-activation is allowed only when observable. If a Member activates native
continuation through natural language or a provider command, the Adapter must
observe the provider-native state transition before treating the execution
driver as `provider_driven`. Prompt text alone is not proof.

## Provider Adapter Contract

A future provider does not need a Goal feature to become an Agent Team Member.
It must implement persistent identity, mailbox delivery, native-session
binding and explicit lifecycle controls. Native continuation is additive.

Each concrete execution mode declares:

```text
ContinuationCapabilities
  can_start_native_continuation
  can_inspect_condition
  can_inspect_state
  can_replace_condition
  can_clear_condition
  can_resume_active_condition
  can_inject_while_running
  can_interrupt_current_activity
  emits_cycle_boundaries
  emits_completion_reason
  continuation_permission_scope
```

Capability is proven in four layers:

```text
provider feature
  -> execution-mode feature
  -> Adapter wiring and version review
  -> product policy permission
```

`native`, `emulated` or a successful one-off prompt is not enough evidence.
An Adapter must state which operations are verified for the exact provider
version and selected execution mode.

## Current Provider Mapping

This table is architectural routing, not a compatibility claim. Provider docs
own version-specific evidence.

| Provider mode | Native mechanism | Initial driver posture | Important boundary |
| --- | --- | --- | --- |
| Codex `codex_app_server` | thread-native Goal | `host_driven` until native Goal ownership, permission inheritance and inspection are verified together | Never call an independent `turn/start` while the Goal owns continuation. |
| Claude `claude_agent_sdk` | Claude Code `/goal`, implemented as a session-scoped Stop hook in supported versions | `host_driven` until the SDK Adapter has a reviewed native-goal canary | `/goal` starts work immediately and does not itself change permissions. |
| Kimi `kimi_acp` | no reviewed native continuation contract | `host_driven` | Native plan updates do not imply native continuation. |
| Future provider | optional | `host_driven` | Add native continuation only after capability and lifecycle review. |

“Initial driver posture” is the required product routing. Current Agent Team
profiles snapshot `host_driven`, and the Codex adapter no longer activates a
native Goal beside Harness turns. A detected dual-driver adapter remains
nonconforming and `review_required` until repaired and canaried.

## Host And Member Rules

The Host:

1. creates or selects the Member and Assignment;
2. selects one execution driver from reviewed capabilities;
3. gives writable members disjoint worktrees or explicit shared-file
   coordination;
4. observes Inbox, PendingInteraction, native continuation and Handoffs;
5. uses explicit Steer, Interrupt, driver change and Close operations;
6. accepts the Assignment separately from provider completion.

The Member:

1. owns the Assignment across provider cycles and Host-plan Waves;
2. may use native planning, continuation and subagents within its permission
   and Workspace boundary;
3. communicates questions, blockers, progress and Handoffs through
   `TeamMessage`;
4. does not claim that a native Goal or final response equals Host acceptance;
5. reports when native continuation or permissions prevent safe coordination.

## Dashboard Contract

The primary Member view shows the durable contract before provider details:

```text
Current Assignment
Execution driver
Continuation state and condition
Workspace execution lease
Permission posture
Pending interactions
Queued/delivered team mail
Native session availability
Current native activity
Handoff and Host acceptance
```

Turns and provider evaluator details belong in expandable native activity.
They are diagnostic handles, not the product hierarchy.

## Acceptance

A continuation integration is not accepted until tests prove:

1. exactly one execution driver can own a Member/session/Workspace;
2. native continuation never overlaps an independent Harness start;
3. ordinary busy mail remains queued or is injected through a verified safe
   operation;
4. permissions remain correct for every provider-created cycle;
5. inspect, interrupt, clear/stop and resume report real provider state;
6. provider satisfaction remains distinct from correlated Handoff and Host
   acceptance;
7. Dashboard and CLI show unknown/review-required states honestly; and
8. native history stays in the provider store.

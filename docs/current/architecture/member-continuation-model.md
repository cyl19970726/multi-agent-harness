# Agent Member Continuation Model

```text
status: canonical architecture contract
owner_role: provider-integration
canonical_for: provider-neutral Member execution ownership, continuation, completion, and Workspace lease semantics
work_contract: ADR 0050; Work/WorkEvent/WorkDelivery are the responsibility path
```

This document is the smallest required context for adding a new Agent Team
provider or changing how a persistent `MemberRun`—including the Team Host's
MemberRun—continues work. It extends
the mailbox and runtime substrate in [Agent Runtime](agent-runtime.md) without
creating a Harness `Goal`, Plan Gate, task graph, or provider transcript copy.

## The Mental Model

Star Harness owns durable collaboration. The selected provider owns native
execution:

```text
AgentTeam (durable, flat)
  -> AgentTeamRun
  -> MemberRun
       -> TeamMembership role (host | member)
       -> active Work + WorkDelivery
       -> Harness Mailbox
       -> Workspace
       -> provider-native session
            -> native plan, goal, turns, tools and subagents
```

These layers answer different questions:

| Layer | Question | Authority |
| --- | --- | --- |
| AgentTeam | Which durable Team owns the long-running work? | Harness |
| Work context | Why does the work exist and what did the Host decide? | Harness |
| Work | What result does this Member own and what is its state? | Harness `Work` + `WorkEvent` |
| MemberRun | Which durable team participant owns the lane? | Harness |
| Mailbox | What coordination has been sent, delivered and acknowledged? | Harness |
| Native session | What did the provider actually execute? | Provider |
| Native continuation | Why does the provider start another execution cycle? | Provider plus selected Adapter contract |

Work is the durable responsibility contract. A provider-native Goal or
completion condition is a session-local way to continue that Work. It
must never become a second product Goal or silently replace Host acceptance.

Host and Member use this same continuation model. Their Team permissions and
status subscription policies differ by exact TeamMembership role; provider
capability never grants Host authority. A managed Host is daemon-driven like a
managed Member. An external interactive Host is the declared `user_driven`
exception and receives no timely-wake or provider-receipt promise.

## Continuation Has Two Independent Axes

### Execution Driver

`execution_driver` answers who may start the next provider execution cycle:

| Driver | Meaning | Agent Team use |
| --- | --- | --- |
| `host_driven` | Harness claims eligible mail and starts the next provider cycle. | Default when native continuation is absent, unsafe or not observable. |
| `provider_driven` | One provider-native continuation mechanism starts later cycles until its condition stops it. | Allowed only when the Adapter can inspect and control it honestly. |
| `user_driven` | The human drives their own already-open interactive provider session out-of-band; Harness never starts a cycle and no native session record exists. | Declared `external_interactive` members only. |
| `bounded` | One invocation owns a finite result and then exits. | Historical/unsupported Team mode; Dynamic Workflow is retired and this is not a new-member fallback. |

One MemberRun must have exactly one active execution driver. Setting a native
Goal and also issuing an independent Harness `turn/start` violates this
contract.

Implementation note: DEV-31 stores this as `AgentSession.control_state`, not as
a new Goal object. The exact fence includes `execution_driver`,
`driver_generation`, `driver_ref`, runtime generation, NativeSessionRef,
composition fingerprint, and capability fingerprint. A provider-driven native
continuation may be defined without being armed; activation requires the exact
runtime and driver generations.

### Completion Policy

`completion_policy` answers who decides that work may stop:

| Policy | Meaning |
| --- | --- |
| `member_declared` | The Member reports that its current work is done. |
| `provider_evaluator` | A provider-native evaluator decides its continuation condition is satisfied. |
| `deterministic_check` | A named command, check or external condition is satisfied. |
| `host_reviewed` | The Host explicitly accepts a submitted Work version. |

Policies compose. Provider Goal achievement can stop provider continuation,
but it never substitutes for `host_reviewed` when the Work requires Host
acceptance.

## Execution Lease

The hard concurrency invariant is not “one Member has one Turn.” Native
providers may perform many turns or cycles while executing one Work.

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
  definition:
    continuation_ref?
    revision?
    phase: inactive | active | paused | blocked | satisfied | unknown
    budget?
  activation:
    armed(runtime_generation, driver_generation) | disarmed | unknown
  observed_at
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
| Provider asks a question | Create a correlated Message and wait for its correlated reply. |
| Provider asks for a protected project action | Require the appropriate Human or policy approval and record the decision on the Work record; do not create a generic interaction object, do not revive the retired Approval ledger, and never infer approval from tool completion. |
| Native continuation satisfies its condition | Record/project the provider fact, then await explicit Work submission/Host acceptance as required. |
| Host explicitly closes Member | Latch Close before teardown, release the managed runtime, and freeze delivery without deleting the MemberRun or native-session binding. |
| Host explicitly reopens Member | Increment `runtime_generation`; a managed adapter resumes the exact recorded native session and frozen mail becomes actionable. |
| Host deactivates/retires Member | End coordination permanently; delivery and Reopen are rejected. |

Ordinary message visibility is an explicit execution-mode capability, not a
uniform mailbox promise:

| Team execution mode | `ordinary_message_boundary` | Host expectation |
| --- | --- | --- |
| Claude `claude_agent_sdk` | `in_turn` | Streaming input may reach the active provider turn. |
| Codex `codex_app_server` | `next_round` | Mail remains queued until the next native round. |
| Kimi `kimi_acp` | `next_round_batched` | Mail is claimed and rendered together at the next round boundary. |
| DeepSeek Harness `deepseek_sdk` | `next_round_batched` | Mail enters the next host-driven DSH cycle through `Agent.followup`; native Goal plugins are absent. |

This field describes delivery timing only. Provider-native transcripts remain
the sole turn/execution record and are never copied into current Message or
`CanonicalMessageDelivery` storage. Legacy TeamMessage storage is read/export
only and is not a fallback mailbox.

Self-activation is allowed only when observable. If a Member activates native
continuation through natural language or a provider command, the Adapter must
observe the provider-native state transition before treating the execution
driver as `provider_driven`. Prompt text alone is not proof.

Providers without reviewed native continuation capability remain first-class
`host_driven` members. DeepSeek Harness is admitted in that form: its reviewed
composition deliberately omits DSH Goal plugins, so it cannot silently become
`provider_driven` even though the upstream framework supports plugin-defined
continuation.

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
| Kimi `kimi_acp` | native Goals persist across turns, but ACP has no reviewed Goal inspect/replace/cancel/terminal contract | `host_driven` | Goals and built-in/custom subagents remain Member-internal; native plan updates do not imply Harness-owned continuation. |
| Future provider | optional | `host_driven` | Add native continuation only after capability and lifecycle review. |

“Initial driver posture” is the required product routing. Current Agent Team
profiles snapshot `host_driven`, and the Codex adapter no longer activates a
native Goal beside Harness turns. A detected dual-driver adapter remains
nonconforming and `review_required` until repaired and canaried.

`review_required`, `incompatible`, and `unavailable` persistent Adapter
snapshots are execution refusals, not warnings. The gate runs before initial
start, native-session resume/reopen, recovery rebind, and a live Supervisor's
rebound Work drive. It must run before a provider process/session is started,
before a WorkDelivery is claimed, and before Work responsibility is rebound.
Historical native-session locators remain readable and are never promoted or
replayed by the refusal. The Host promotion path is explicit: inspect
`firm member providers --fail-on-review`, regenerate protocol schemas, run
deterministic acceptance plus a live canary for the exact version and mode,
then add that exact version to the Adapter's reviewed set before retrying the
same durable MemberRun and Work.

## Live Projection Recovery Boundary

The Runtime SSE contract is a bounded full-snapshot-on-reconnect protocol. It
does not implement a durable cursor and does not accept or claim
`Last-Event-ID` replay. Each connection begins with a `snapshot` marker carrying
the selected Execution Space, optional Company scope, and a process-local
`stream_epoch`. The client then fetches the authoritative scoped snapshot.

Incremental typed frames are convenience deltas inside that connection.
`projection_invalidated` is a scoped refresh hint carrying `scope`, `scope_id`,
`ledger`, an epoch-local `revision`, and one of
`append | replace | truncate | delete`. Atomic same-size replacement and direct
deletion invalidate the affected projection even when a byte offset cannot see
new content. On open, reconnect, visibility recovery, invalidation, or scope
change, the client refetches the authoritative selected snapshot and rejects
late responses from an older scope/generation. Revisions are monotonic only for
one `(stream_epoch, scope, scope_id, ledger)` key and must never be persisted or
presented as durable resume tokens.

## Host And Member Rules

The Host:

1. creates or selects the Member and Work;
2. selects one execution driver from reviewed capabilities;
3. gives writable members disjoint worktrees or explicit shared-file
   coordination;
4. observes Works, WorkDelivery, Inbox, correlated questions and native continuation;
5. uses explicit Steer, Interrupt, driver change and Close operations;
6. accepts submitted Work separately from provider completion.

The Member:

1. owns the Work across provider cycles and Host replans recorded on the Work records;
2. may use native planning, continuation and subagents within its permission
   and Workspace boundary;
3. records block/submission through Work operations and communicates questions,
   explanation and peer coordination through Work-linked `Message` rows;
4. does not claim that a native Goal or final response equals Host acceptance;
5. reports when native continuation or permissions prevent safe coordination.

## Dashboard Contract

The primary Member view shows the durable contract before provider details:

```text
Current Work id, version, owner and status
Execution driver
Continuation state and condition
Workspace execution lease
Permission posture
Correlated provider questions
Queued/delivered team mail
Native session availability
Current native activity
Work submission and Host acceptance
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
6. provider satisfaction remains distinct from Work submission and Host
   acceptance;
7. Dashboard and CLI show unknown/review-required states honestly; and
8. native history stays in the provider store.

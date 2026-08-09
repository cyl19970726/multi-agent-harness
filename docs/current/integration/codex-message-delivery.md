# Codex Agent Team Work And Message Delivery

```text
status: implemented Message baseline; WorkDelivery redesign pending ADR 0050
```

This document defines how a Codex Agent Team Member receives durable Harness
coordination. It extends [Codex Integration](codex.md) and the
provider-neutral runtime contract.

## Boundary

Harness owns mailbox identity, correlation and delivery facts. Codex
app-server owns the native thread, turns, chat, tools and execution history.
Codex does not poll Harness storage.

```text
TeamMessage or WorkDelivery
  -> latest-row mailbox projection
  -> eligible recipient/runtime
  -> adapter delivery reservation
  -> app-server turn/start or turn/steer
  -> provider receipt / terminal control acknowledgement
  -> updated Harness delivery fact
```

New Codex Team members always use `codex_app_server`. `codex_exec` delivery is
bounded Dynamic Workflow or historical behavior and is not a Team fallback.

## Delivery Shapes

New coordination delivery uses three durable shapes:

| Delivered object | Meaning |
| --- | --- |
| `WorkDelivery` | delivers one assigned or changed Work version |
| `TeamMessage(message)` | ordinary Host/Member/peer conversation, including planning |
| `TeamMessage(control)` | requests a real supported runtime operation |

Human-readable intents such as `PLAN:`, `QUESTION:`, `BLOCKER:`, `REVIEW:` and
`DECISION:` remain Markdown inside ordinary messages. Assignment and Handoff
message kinds are removed rather than retained as compatibility after the
ADR 0050 migration.

Provider-native questions and approvals that pause the current turn are
`PendingInteraction`, not ordinary TeamMessage delivery.

## Direction And Initial State

- Member → Host is `manual_ack + delivered` when appended because the Harness
  control plane has received it.
- Host → Member and Member → Member ordinary coordination begins `queued`.
- Peer sender, recipient, correlation and causation must resolve inside the
  same TeamRun.
- Work owner/version and WorkEvents prove responsibility and state.
- WorkDelivery carries the exact `work_id` and version that must enter the
  Member's safe-boundary context.
- TeamMessage remains authored conversation with optional `work_id`,
  correlation and reply lineage; it does not assign or submit Work.

Codex submits results through a Work operation with evidence references. The
Adapter never converts provider final text into an automatic Work submission
or duplicate Message. A submission is fenced while a newer WorkDelivery, or a
newer linked response-required Message, remains queued or claimed.

If real same-turn Steer succeeds after submission, Work remains in review until
the Host accepts it or requests changes. Steer does not create another Work or
submission. A Host request-changes operation increments the Work version and
creates a new WorkDelivery; explanatory text may travel in a linked Message.

After the provider acknowledges Steer, Harness constructs the final
`Control(Inject, Delivered)` row and publishes it exactly once through the
checked append before folding its event. No queued Control revision is exposed,
so an Inject-descendant sibling cannot enter between two Control publications.
The broader provider-effect-before-Control crash reservation gap remains a
follow-up; this bounded convergence rule does not claim that gap is closed.

Reading `firm team-run inbox` or `firm member-run show` is a projection;
it does not itself consume or semantically acknowledge mail.

## Latest-Row Selection

Harness stores mutable coordination append-only. A dispatcher selects only the
latest row for each Message or WorkDelivery id:

```text
message-1 queued
message-1 delivered

deliverable projection: message-1 delivered
```

A stale earlier `queued` row must never be delivered again. The same
latest-row rule drives CLI inbox, Dashboard mailbox counts and delivery
warnings.

## Reservation And Receipt

Provider side effects may start a turn, so delivery must reserve the latest
eligible Message or WorkDelivery before injection:

```text
latest queued Message or WorkDelivery
  -> verify MemberRun, Work/message lineage, runtime and native-session binding
  -> record in-flight reservation/receipt boundary
  -> submit envelope to the same-process app-server adapter
  -> adapter accepts or rejects envelope
  -> record delivered or failed
```

`delivered` means the adapter accepted the envelope for that MemberRun and
native thread. It does not mean the model understood, executed, or accepted
the request. Semantic acknowledgement is a correlated reply, Work transition,
Host review action, or real control acknowledgement.

If the adapter fails before receipt, mail remains queued or visibly failed.
The implementation must prevent duplicate injection while a receipt is in
flight. Exactly-once semantic execution is not inferred from a transport
receipt.

## Busy Member Policy

| Member/runtime state | Ordinary mail |
| --- | --- |
| live and idle | deliver next eligible message as a new turn |
| current turn running | retain queued until the next eligible round |
| waiting on PendingInteraction | resolve the interaction through its authority route |
| interrupted but runtime open | allow later ordinary turn |
| explicitly closed | reject normal delivery |
| native session unavailable/incompatible | show blocker; do not fabricate resume |

Ordinary messages never interrupt a busy turn. A real `Steer` is a separate
control request and uses `turn/steer` only when the snapshotted mode supports
it. `Interrupt` uses `turn/interrupt` and waits for acknowledgement. `Close`
ends the app-server runtime; it is not a message and is not implied by turn,
TeamRun, Wave or Mission completion.

## Envelope

Each delivered turn includes the smallest stable coordination envelope:

```text
project_id
mission_id (derived from AgentTeam) / origin_wave_id? (navigation only)
agent_team_id / execution_node_id
team_run_id
member_run_id
work_id / work_version / work_delivery_id
sender and recipient
team roster and roles
owned paths / worktree / permission boundary
Work context and completion criteria
optional linked Message Markdown + correlation/reply lineage
exact CLI examples for Work list/claim/start/block/submit and Work-linked Message
```

The envelope provides identity and responsibility, not a copy of earlier
provider chat. The native thread supplies provider history.

## Native Thread Continuity

One live Codex MemberRun binds one native Codex thread. Later ordinary messages
use new turns on that same thread. Resume after process loss must explicitly
use the recorded native thread id and verified `thread/resume`.

Harness does not rebuild continuity by concatenating TeamMessages. Missing or
incompatible native records remain visible as unavailable.

Provider-native subagents remain inside the Member's own thread tree. Their
activity may appear through an ephemeral native projection, but they do not
receive Harness mailbox identities unless the Host explicitly creates a new
MemberRun.

## Read Surfaces

The same application behavior is exposed through:

- `firm team-run inbox --id <run> --member-run-id <member> [--all] --json`;
- `firm team-run host-inbox --surface <surface> --thread-id <native-host-task> [--all] --json`;
- `firm member-run show --id <member> --json`;
- Team message/status commands;
- HTTP and MCP equivalents; and
- Dashboard Team mailboxes, group conversation and Member Focus.

Default Inbox returns actionable current mail. `--all` returns complete durable
coordination lineage. Provider-native execution history is represented by a
locator and on-demand projection, not copied into these results.

## Supervisor Boundary

The live app-server control handle is process-local, while the
`TeamSupervisorLease` is durable cross-process authority. Its service locator
lets a second Dashboard, MCP, CLI, or Harness service route control to the
owner, but only the current generation may claim a queued message or drive the
handle. The owner fences the lease again immediately before the Provider
operation. After a crash, an expired lease may be replaced; a delivery left
`claimed` remains uncertain until an operator reconciles a provider receipt or
explicitly requeues it.

Before an idle Codex Member claims new mail, the Supervisor verifies the
app-server transport is still alive. A dead transport is resumed on the same
thread first, so known pre-turn disconnects do not create avoidable uncertain
claims. Close is latched before live dispatch and therefore remains accepted
even if the process-local control receiver disappears during that reattach
boundary.

Host delivery follows the same ownership rule. Codex `Stop` is a real
same-task safe boundary and may continue once with actionable Host mail.
An already-idle Desktop task cannot be asynchronously woken by its thread id
alone; it receives mail at its next prompt or resume. See
[ADR 0040](../../decisions/0040-native-host-inbox-delivery.md).

## Acceptance

1. Host and peer mail validate same-TeamRun identity and optional Work link.
2. A busy Member receives ordinary mail and WorkDelivery once at a safe boundary.
3. Adapter receipt, semantic reply and control acknowledgement remain distinct.
4. Member → Host delivery is immediately visible.
5. Closed or incompatible members reject delivery.
6. `member-run show`, Inbox and Dashboard reconstruct the same mailbox state.
7. A second-round Work submission uses the latest delivered Work version and
   preserves linked Message lineage where relevant.
8. Provider final text never creates a duplicate Work submission or Message.
9. Native Codex transcript, tools, commands, files, reasoning and subagent
   transcript remain outside Harness storage.

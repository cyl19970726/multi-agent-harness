# Codex Agent Team Message Delivery

This document defines how a Codex Agent Team Member receives durable Harness
coordination. It extends [Codex Integration](codex.md) and the
[provider-neutral runtime contract](../agent-runtime.md).

## Boundary

Harness owns mailbox identity, correlation and delivery facts. Codex
app-server owns the native thread, turns, chat, tools and execution history.
Codex does not poll Harness storage.

```text
TeamMessage
  -> latest-row mailbox projection
  -> eligible recipient/runtime
  -> adapter delivery reservation
  -> app-server turn/start or turn/steer
  -> provider receipt / terminal control acknowledgement
  -> updated Harness delivery fact
```

New Codex Team members always use `codex_app_server`. `codex_exec` delivery is
bounded Dynamic Workflow or historical behavior and is not a Team fallback.

## Message Shapes

New public coordination writes use four durable shapes:

| Shape | Delivery meaning |
| --- | --- |
| `assignment` | starts accountable ownership and correlation |
| `message` | ordinary Host/Member/peer coordination, including planning |
| `handoff` | returns an explicit lane outcome/evidence |
| `control` | requests a real supported runtime operation |

Human-readable intents such as `PLAN:`, `QUESTION:`, `BLOCKER:`, `REVIEW:` and
`DECISION:` remain Markdown inside ordinary messages. Historical specialized
kinds remain readable but are not new lifecycle machines.

Provider-native questions and approvals that pause the current turn are
`PendingInteraction`, not ordinary TeamMessage delivery.

## Direction And Initial State

- Member → Host is `manual_ack + delivered` when appended because the Harness
  control plane has received it.
- Host → Member and Member → Member ordinary coordination begins `queued`.
- Peer sender, recipient, correlation and causation must resolve inside the
  same TeamRun.
- Assignment ownership is proven only by the Assignment message and its
  correlation id.

Reading `harness team-run inbox` or `harness member-run show` is a projection;
it does not itself consume or semantically acknowledge mail.

## Latest-Row Selection

Harness stores mutable coordination append-only. A dispatcher selects only the
latest row for each message id:

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
eligible message before injection:

```text
latest queued message
  -> verify MemberRun, correlation, runtime and native-session binding
  -> record in-flight reservation/receipt boundary
  -> submit envelope to the same-process app-server adapter
  -> adapter accepts or rejects envelope
  -> record delivered or failed
```

`delivered` means the adapter accepted the envelope for that MemberRun and
native thread. It does not mean the model understood, executed, or accepted
the request. Semantic acknowledgement is a correlated reply, Handoff,
review result, or real control acknowledgement.

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
mission_id? / origin_wave_id?
team_run_id
member_run_id
assignment_message_id
correlation_id
sender and recipient
team roster and roles
owned paths / worktree / permission boundary
completion standard
message Markdown
exact CLI examples for Inbox, Host/peer message and Handoff
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

- `harness team-run inbox --id <run> --member-run-id <member> [--all] --json`;
- `harness team-run host-inbox --surface <surface> --thread-id <native-host-task> [--all] --json`;
- `harness member-run show --id <member> --json`;
- Team message/status commands;
- HTTP and MCP equivalents; and
- Dashboard Team mailboxes, group conversation and Member Focus.

Default Inbox returns actionable current mail. `--all` returns complete durable
coordination lineage. Provider-native execution history is represented by a
locator and on-demand projection, not copied into these results.

## Process Boundary

The live app-server control handle is currently process-local. The
Dashboard/MCP service can deliver to members it launched in the same service
process. A second CLI process cannot inject into a foreground
`team-run start` child merely because it can read the same JSONL store. Until a
durable Team Supervisor exists, that case must remain visibly queued or fail
honestly rather than claim delivery.

Host delivery follows the same ownership rule. Codex `Stop` is a real
same-task safe boundary and may continue once with actionable Host mail.
An already-idle Desktop task cannot be asynchronously woken by its thread id
alone; it receives mail at its next prompt or resume. See
[ADR 0040](../decisions/0040-native-host-inbox-delivery.md).

## Acceptance

1. Host and peer mail validate same-TeamRun identity/correlation.
2. A busy Member receives ordinary mail once at the next safe round.
3. Adapter receipt, semantic reply and control acknowledgement remain distinct.
4. Member → Host delivery is immediately visible.
5. Closed or incompatible members reject delivery.
6. `member-run show`, Inbox and Dashboard reconstruct the same mailbox state.
7. Native Codex transcript, tools, commands, files, reasoning and subagent
   transcript remain outside Harness storage.

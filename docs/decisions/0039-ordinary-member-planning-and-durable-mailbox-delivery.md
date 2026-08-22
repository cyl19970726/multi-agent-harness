# ADR 0039: Ordinary Member Planning And Durable Mailbox Delivery

> Work-graph amendment (ADR 0058): planning remains ordinary conversation, but
> accepted execution ordering is no longer encoded by correlation or natural
> language. Current Works use durable `depends_on` edges and kernel-derived
> readiness; Messages never mutate those edges.

> Successor note (DOC-16 Keep row, DEV-40 flip 2026-08-18): this ADR is kept; the governing successor context is [DOC-106](https://app.notion.com/p/3be49a4fa3798126a598e634ed5d0807).

```text
status: active; Assignment/Handoff kinds amended by ADR 0050
date: 2026-07-27
supersedes: ADR 0038 provider-native member plan negotiation
extends: ADR 0037 member autonomy and collaboration
```

ADR 0050 removes Assignment Message and moves assignment, blocker, submission,
review, and acceptance into Work operations. This ADR remains authoritative for
ordinary planning conversation, durable authored-message delivery, busy/idle
queueing, and Provider-safe mailbox boundaries.
ADR 0056 removes PendingInteraction as a product object: paused provider
questions are ordinary correlated Messages, while permission callbacks are
handled inside the frozen AgentSession ceiling or fail closed.

## Context

Agent Team needs members that can discuss a plan with the Host, receive later
instructions while busy or idle, coordinate with peers, and remain inspectable
without copying provider transcripts into Harness.

ADR 0038 introduced a dedicated plan negotiation state machine and provider
tool gate. In practice that duplicated ordinary conversation, varied by
provider, and could not enforce its claimed boundary: a hook that blocks an
`Edit` tool cannot prevent the same write through `Bash`. It added complexity
without a trustworthy execution boundary.

The real cross-provider problem is reliable delivery at a safe boundary:

```text
durable TeamMessage or WorkDelivery
  -> recipient queue
  -> adapter delivery receipt
  -> provider-native turn/session
  -> explicit reply, handoff, or PendingInteraction
```

## Decision

### The four-part mental model

```text
Host      = goal, boundaries, lane ownership, conflict resolution, acceptance
Member    = autonomous end-to-end owner of one lane
Harness   = durable identity, mailbox, correlation, delivery facts, evidence refs
Provider  = native session, internal plan, tools, subagents, execution and resume
```

This boundary is intentionally small. Harness must not model an action merely
because the Host can say it clearly in natural language. The Host may tell a
Member to propose a plan, create a Git worktree, ask a peer for an interface,
or wait for review. The Member decides how to perform those actions and reports
the actual result. Harness records Work responsibility and communication facts;
it does not schedule the Member's Git commands or copy the provider's execution
stream.

Team collaboration needs only four durable coordination shapes:

| Shape | Purpose |
| --- | --- |
| `Work` + `WorkEvent` | Preserve responsibility, criteria, state and acceptance |
| `message` | Ask, answer, plan, revise, report progress/blockers, or coordinate with a peer |
| `WorkDelivery` | Reliably notify one runtime of a Work version it must consume |
| control/PendingInteraction | Represent a real runtime control or provider pause |

Question, answer, plan, review, progress, and blocker are human-readable
message intents, not separate lifecycle machines. Put the intent in the first
line of the Markdown body when it improves scanning, for example `QUESTION:`,
`PLAN:`, `BLOCKER:`, or `REVIEW:`. Add a typed intent only if future evidence
shows filtering cannot be served without it.

### Planning is ordinary correlated conversation

Harness has no Plan Mode, Plan Gate, Plan object, or plan approval lifecycle.
The Host may send an ordinary correlated message:

> Return a Markdown plan first. Do not execute yet.

The Member answers with an ordinary message or an artifact reference. The Host
then answers in the same correlation with revisions or permission to execute.
Provider-native planning features remain optional execution aids inside the
native session. They never create Harness state, change permission, or prove
Host approval.

New authored coordination uses ordinary TeamMessage conversation and control.
Assignment and Handoff message kinds are deleted with their readers, fixtures,
and active dogfood data; there is no dual-read compatibility projection.
Planning and routine coordination attach no special validation or runtime
behavior to their human-readable intent.

### Example: two autonomous feature lanes

The Host creates one TeamRun and two Works:

```text
Host -> Member A
Build module A end to end. Create a separate Git worktree before editing.
Return the worktree path, branch, commit, checks, and interface contract.

Host -> Member B
Build module B end to end in a separate Git worktree. Coordinate directly with
Member A about the shared interface before changing shared files.
```

Each Member may use its own provider-native design, implementation, and test
subagents. Those subagents remain internal to that Member. A typical durable
conversation is:

```text
Member A -> Member B  message   INTERFACE: proposed request/response shape
Member B -> Member A  message   REVIEW: one incompatibility and suggested fix
Member A -> Host      message   PROGRESS: worktree path and agreed interface
Member B -> Host      message   BLOCKER: shared file needs Host ownership decision
Host -> Member B      message   DECISION: Member A owns shared file; adapt locally
Member A -> Work A    submit    patch, commit, checks, evidence
Member B -> Work B    submit    patch, commit, checks, evidence
```

The Host accepts or requests changes on each Work. There is no Task Graph, worktree
state machine, plan gate, or automatic peer dependency. Correlation preserves
the work chain; natural language carries the judgment.

### Harness owns the mailbox; adapters own delivery

Harness stores the latest delivery state for each recipient. A provider adapter
must poll or subscribe independently of provider turn completion. A busy member
may queue normal coordination, but a turn boundary cannot be the only occasion
on which the adapter checks for mail.

`delivered` means the live adapter recorded a provider-native receipt for the
selected MemberRun and native session. It does not mean the model understood or
acted on the message. Semantic acknowledgement is an explicit reply, Work
submission, review action, or control acknowledgement.

The current Supervisor generation atomically claims a delivery before provider
side effects, but only after verifying that the selected provider transport is
live. Transport failure before claim leaves mail queued and reattaches the
recorded native session first. A crash between claim and receipt leaves
explicit uncertainty; recovery must reconcile a native receipt or explicitly
return the claim to `queued`, never blindly replay it.

### Member lifetime is independent of a turn or TeamRun status

A Host-created Member remains addressable until the Host explicitly closes it.
Provider turn completion and a Work submission return the MemberRun to `idle`;
they do not end the native runtime. Host or peer mail queued while it is idle
wakes the same MemberRun and provider-native session exactly once.

Interrupt stops only the current provider turn. After the provider acknowledges
that interrupt, the Member returns to `idle` and may receive later mail. Close
is the only ordinary operation that records the Member as `stopped`.
Completing or advancing a Wave, TeamRun, or Mission never implies Close.
Close intent is durably latched before process-local teardown; a racing lease,
receiver, or reconnect cannot revive the Member after Host Close.

The Harness process that starts a TeamRun supervises every unclosed Member.
Unexpected provider transport loss records an explicit `disconnected` action,
keeps the native-session binding, and resumes that session rather than
replaying stale Work content. Re-running TeamRun start after a Host process restart
reattaches unclosed Members to their recorded native sessions. Physical
interrupt, steer, and close handles remain process-local. The active lease
publishes the owning service's loopback locator, so other Harness clients route
controls to that exact generation; the owner fences again before the Provider
operation.

### Member detail is a coordination projection

`harness member-run show --id <member-run-id> --json` is the canonical
single-member operator read. It joins:

- MemberRun identity, status, provider profile, Workspace and worktree facts;
- TeamRun, Mission, AgentTeam and current/queued Works;
- Inbox, Outbox, delivery states and PendingInteractions;
- WorkEvents, latest submission, evidence refs and native-session locator;
- current Team Supervisor lease and stable Agent Inbox route records.

It does not copy provider chat, tool, command, reasoning, or subagent history.
Those remain readable from the provider-native session through its locator.

## Consequences

- Host and Member can debate plans without another product state machine.
- Codex, Claude, Kimi, and future providers implement one mailbox contract.
- Provider-native Plan/Goal features can improve a member's own work without
  dictating Harness architecture.
- Delivery truth is separated from semantic completion.
- The CLI and Dashboard can reveal queued, delivered, acknowledged, failed, and
  unresolved communication without transcript spelunking.

## Acceptance

1. Host-to-Member and Peer-to-Member mail sent during an active turn reaches the
   same MemberRun/native session or remains visibly queued with an explicit
   failure.
2. A runner receipt, not merely a successful stdin write, changes delivery to
   `delivered`.
3. Empty Inbox does not itself mean the Member is accepted or destroyed.
4. Planning works through ordinary correlated messages and optional artifacts;
   no hook or provider mode is presented as a Plan Gate.
5. `member-run show` reconstructs the Member's coordination state while
   returning only a native-session locator for provider execution history.
6. Deterministic tests cover busy Message/WorkDelivery, retry/no-duplicate
   behavior, Member-to-Host visibility, peer delivery, submission evidence, and
   CLI detail.
7. Turn completion, Work submission, Interrupt, Wave advance, TeamRun completion, and
   Mission completion leave an unclosed Member available on the same native
   session.
8. Unexpected transport loss is visible and recoverable without duplicate
   WorkDelivery; explicit Close is the only normal runtime-shutdown
   operation. ADR 0049 adds explicit same-MemberRun Reopen and permanent Retire.

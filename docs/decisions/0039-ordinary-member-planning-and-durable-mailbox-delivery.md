# ADR 0039: Ordinary Member Planning And Durable Mailbox Delivery

```text
status: active
date: 2026-07-27
supersedes: ADR 0038 provider-native member plan negotiation
extends: ADR 0037 member autonomy and collaboration
```

## Context

Agent Team needs members that can discuss a plan with the Host, receive later
instructions while busy or idle, coordinate with peers, and remain inspectable
without copying provider transcripts into Harness.

ADR 0038 introduced a dedicated plan negotiation state machine and provider
tool gate. In practice that duplicated ordinary conversation, varied by
provider, and could not enforce its claimed boundary: a hook that blocks an
`Edit` tool cannot prevent the same write through `Bash`. It added complexity
without a trustworthy execution boundary.

The real cross-provider problem is reliable message delivery:

```text
durable TeamMessage
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
the actual result. Harness records the Assignment and communication facts; it
does not schedule the Member's Git commands or copy the provider's execution
stream.

New work needs only four durable coordination shapes:

| Shape | Purpose |
| --- | --- |
| `assignment` | Give one Member accountable ownership and acceptance criteria |
| `message` | Ask, answer, plan, revise, report progress/blockers, or coordinate with a peer |
| `handoff` | Return a lane outcome and evidence for Host review |
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

Historical `plan_*`, question/answer/progress/blocker/review, and broadcast
TeamMessage kinds remain readable for compatibility. New public writes accept
only `assignment`, `message`, `handoff`, and `control`; planning and routine
coordination attach no special validation or runtime behavior to their
human-readable intent.

### Example: two autonomous feature lanes

The Host creates one TeamRun and two correlated Assignments:

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
Member A -> Host      handoff   patch, commit, checks, evidence
Member B -> Host      handoff   patch, commit, checks, evidence
```

The Host integrates or rejects each Handoff. There is no Task Graph, worktree
state machine, plan gate, or automatic peer dependency. Correlation preserves
the work chain; natural language carries the judgment.

### Harness owns the mailbox; adapters own delivery

Harness stores the latest delivery state for each recipient. A provider adapter
must poll or subscribe independently of provider turn completion. A busy member
may queue normal coordination, but a turn boundary cannot be the only occasion
on which the adapter checks for mail.

`delivered` means the live adapter accepted the envelope for the selected
MemberRun and native session. It does not mean the model understood or acted on
the message. Semantic acknowledgement is an explicit reply, handoff, review, or
control acknowledgement.

If an adapter crashes before its delivery receipt, the message remains queued.
Adapters must prevent duplicate injection while a receipt is in flight. A
future durable claim/lease may strengthen crash recovery; until implemented,
the Dashboard and CLI must expose the gap rather than claim exactly-once
provider consumption.

### Member detail is a coordination projection

`harness member-run show --id <member-run-id> --json` is the canonical
single-member operator read. It joins:

- MemberRun identity, status, provider profile, Workspace and worktree facts;
- TeamRun, Mission, AgentTeam and current Assignment correlation;
- Inbox, Outbox, delivery states and PendingInteractions;
- actions, latest handoff, evidence refs and native-session locator.

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
6. Deterministic tests cover busy delivery, retry/no-duplicate behavior,
   Member-to-Host visibility, peer delivery, handoff evidence, and CLI detail.

# ADR 0037: Agent Member Autonomy And Collaboration

```text
status: active
date: 2026-07-24
extends: ADR 0032 provider-native execution truth; ADR 0034 Host-plan Waves
```

## Context

Starting several provider processes is not yet a complete Agent Team. A useful
team must preserve responsibility while allowing each member to plan, use its
provider's native subagents, ask questions, coordinate with peers, and continue
through more than one Host planning revision.

Without a fixed boundary, four failures recur:

- a `MemberRun`, provider session, and provider-native subagent are treated as
  interchangeable identities;
- routine provider completion is mistaken for accepted work;
- a Wave becomes a scheduler or synchronization barrier; and
- unstructured chat is mistaken for a reliable mailbox.

## Decision

### Member is the accountable autonomous collaborator

An Agent Team `MemberRun` accepts an end-to-end correlated Assignment and owns
its internal plan, Workspace, provider-native session, permission ceiling,
evidence, and follow-up work until the Team Lead accepts its handoff through a
`review_result`.

Member Goal is a Dashboard projection, not a new stored object. It is derived
from the current Assignment, completion standard, owned paths, status, latest
progress or blocker, and latest applicable Steer.

For complex or high-risk Assignments, ADR 0039 keeps planning as ordinary
correlated conversation. The Host can ask the Member to return a Markdown plan
before execution, challenge it, and then instruct the Member to revise or
execute. There is no special Plan Mode or Plan Gate. The Assignment remains the
Goal, and the same MemberRun, correlation, Workspace, and native session
continue through the discussion.

A member may use provider-native subagents for bounded design, implementation,
or verification inside its lane. Those child threads inherit the member's
permissions and return their results to that member. They do not create an
implicit `MemberRun`, own the Assignment, or constitute independent review.
Create a separate reviewer member when acceptance needs independence.

### Select the smallest truthful executor

| Need | Executor |
| --- | --- |
| Host can finish safely in its current context | Host |
| One accountable lane needs sustained multi-turn work, a Workspace, chat, or resume | Agent Team member |
| A bounded internal subtask returns to one accountable member | provider-native subagent |
| Deterministic repeated steps own their own step state | Dynamic Workflow |

Two independent feature modules should normally use two members. Each member
owns its module from design through implementation and validation and may
delegate bounded internal work to subagents. Cross-module integration and
final acceptance remain Lead decisions.

### Harness owns coordination; providers own execution

Ordinary Agent Team collaboration uses `TeamMessage`:

- `assignment` starts an owned correlation;
- `question`, `answer`, `progress`, and `blocker` coordinate work;
- `review_request`, `handoff`, and `review_result` close the acceptance loop;
- control messages record supported steer/interrupt/resume requests and their
  real acknowledgements.

Provider questions, approvals, or plan reviews that pause the provider use
`PendingInteraction`. A provider frame marked `completed` does not answer or
approve it.

A member may message the Host or another active member in the same
`AgentTeamRun`. Direct peer coordination does not require routine Lead
approval, but it remains visible to the Lead and must preserve the Assignment
correlation. Harness rejects cross-TeamRun senders, recipients, causation, and
correlations.

Member-to-Host messages are `manual_ack` and `delivered` when appended because
the control plane has received them. Host-to-member and member-to-member
ordinary messages are queued for the recipient's next available provider
round. Reading an inbox is a projection over latest message rows; it is not a
provider transcript.

Every provider-round Handoff keeps the original Assignment `correlation_id`
and records the exact consumed TeamMessage as `causation_id`. The initial
round is normally caused by the Assignment; later rounds are caused by the
specific Host or peer follow-up that woke them. This is sufficient lineage for
multi-round collaboration and does not introduce conditional delivery or a
Task Graph.

A Member's explicit correlated Handoff is authoritative for that round. The
Adapter may enrich it with observed evidence references but must not also
append an automatic copy of the provider's final reply. Automatic Handoff
creation is a fallback only when the Member did not explicitly send one.

Harness never persists a second copy of provider chat, tool calls, commands,
file events, reasoning, or subagent transcripts. The bound provider-native
session is their sole truth and the only valid resume source.

### Steer and interruption must be honest

Steer means injection into the real current provider turn only when the
snapshotted provider mode supports and acknowledges that operation. Otherwise
the product labels the input as a queued message for the next provider round.
It must not display a synthetic steer acknowledgement.

Interrupt and resume likewise require mode-specific terminal acknowledgement.
Queued ordinary coordination never silently interrupts a busy member.

### Host controls replan, integration, and acceptance

The Host is Team Lead and follows this loop:

```text
observe inbox and outcomes
  -> answer questions or integrate completed lanes
  -> compare plan with actual state
  -> revise current Wave or advance
  -> explicitly assign the next work
```

Small changes inside the same judgment boundary update the current Wave and
append a revision. A material change in plan, member composition,
responsibility, risk, or decision boundary advances the Wave and creates the
next one.

The Host does not wait for unrelated active work. It may integrate completed
lanes, advance, and carry an unfinished member forward with the same
`MemberRun`, Assignment correlation, Workspace, and native session.

There is no conditional-message object or Task Graph. When work depends on a
handoff, the Host observes the durable handoff and explicitly sends the next
Assignment or review message.

### CLI is complete; other surfaces reuse it

The CLI exposes the complete collaboration path, including member inbox
reading. MCP, HTTP, Dashboard, hooks, and provider plugins invoke the same
application behavior and may not create another lifecycle or mailbox.

Plugins distribute thin skills, commands, MCP registration, and fail-open
lifecycle hooks. Canonical product and architecture contracts stay in
repository docs and code. Provider upgrades remain separate, explicitly
approved changes.

## Consequences

- Members can remain responsive across multiple rounds and Waves without
  turning a Wave into runtime ownership.
- Peer collaboration is direct but reconstructable.
- A member's own subagent tree can be visible through provider-native activity
  without multiplying Harness identities.
- Lead Inbox and Member Focus can derive useful work state from assignments,
  messages, controls, and native-session references.
- Deferred dependency scheduling remains intentionally manual and explicit.

## Standard Two-Module Example

```markdown
| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| RuntimeBuilder | Runtime owner | Design, implement, and validate Inbox + delivery semantics; may use internal design/test subagents | Patch, checks, handoff |
| DashboardBuilder | UX owner | Design, implement, and validate Lead Inbox + Member Goal; may use internal UI/test subagents | UI patch, checks, handoff |
| RiskReviewer | Independent reviewer | Review cross-module semantics after both handoffs | review_result |
```

The Lead assigns the first two lanes concurrently. If RuntimeBuilder asks a
decision-shaped question, the Lead answers with the same correlation. When one
member hands off early, the Lead can integrate it immediately. The reviewer is
assigned only after the relevant handoffs exist. No conditional delivery edge
is stored.

## Acceptance

The vertical scenario must prove:

1. two Codex members receive distinct correlated end-to-end Assignments;
2. each member invokes at least one provider-native subagent without creating
   an implicit `MemberRun`;
3. one member-to-Host question/answer and one peer message preserve lineage;
4. member-to-Host delivery is immediately visible while peer delivery queues
   and is consumed only once;
5. one handoff is reviewed explicitly;
6. the Host revises or advances while another member continues on the same
   `MemberRun` and native session; and
7. Mission closeout, Wave history, messages, acknowledgements, evidence, and
   native execution remain reconstructable from CLI and Dashboard.

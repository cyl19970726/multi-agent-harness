# ADR 0037: Agent Member Autonomy And Collaboration

> Work-graph amendment (ADR 0058): the statements below that reject a Task
> Graph or keep dependencies wholly manual are superseded for current Work.
> Independent Works form one flat dependency DAG; conversation may propose an
> edge but cannot create it or change readiness.

> Successor (DOC-16 row, DEV-40 flip 2026-08-18): [DOC-105](https://app.notion.com/p/3be49a4fa379817aa594fd8e7331c30d) + [DOC-106](https://app.notion.com/p/3be49a4fa3798126a598e634ed5d0807).

```text
status: active; responsibility semantics amended by ADR 0050; its Mission/Wave
  premise is retired legacy history (DOC-108)
date: 2026-07-24
extends: ADR 0032 provider-native execution truth; the historical ADR 0034
  Host-plan Waves (retired by DOC-108)
```

ADR 0050 supersedes this ADR's Assignment-message ownership and Handoff-as-task
state. Member autonomy, peer communication, native subagent boundaries, and
Host acceptance remain active. New responsibility uses Work and WorkDelivery;
authored TeamMessage remains conversation.
ADR 0056 replaces the remaining PendingInteraction reference with correlated
question/reply Messages and removes any separate permission lifecycle.

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

An Agent Team `MemberRun` accepts an end-to-end Work and owns its internal plan,
Workspace, provider-native session, permission ceiling, evidence, and follow-up
work until the Team Host accepts that Work.

Member Goal is a Dashboard projection, not a new stored object. It is derived
from the current Work, completion standard, owned paths, status, latest
progress or blocker, and latest applicable Steer.

For complex or high-risk Work, ADR 0039 keeps planning as ordinary
correlated conversation. The Host can ask the Member to return a Markdown plan
before execution, challenge it, and then instruct the Member to revise or
execute. There is no special Plan Mode or Plan Gate. Current Work remains the
derived Goal, and the same MemberRun, Work, Workspace, and native session
continue through the discussion.

A member may use provider-native subagents for bounded design, implementation,
or verification inside its lane. Those child threads inherit the member's
permissions and return their results to that member. They do not create an
implicit `MemberRun`, own the Work, or constitute independent review.
Create a separate reviewer member when acceptance needs independence.

### Select the smallest truthful executor

| Need | Executor |
| --- | --- |
| Host can finish safely in its current context | Host |
| One accountable lane needs sustained multi-turn work, a Workspace, chat, or resume | Agent Team member |
| A bounded internal subtask returns to one accountable member | provider-native subagent |
| Deterministic repeated steps owned their own step state | Dynamic Workflow (retired; historical decision context) |

Two independent feature modules should normally use two members. Each member
owns its module from design through implementation and validation and may
delegate bounded internal work to subagents. Cross-module integration and
final acceptance remain Lead decisions.

### Harness owns coordination; providers own execution

Ordinary Agent Team collaboration uses authored `TeamMessage` conversation.
Questions, answers, plans, explanations and peer coordination may link a Work,
but never mutate its owner or state. Assignment, claim, start, block, submit,
request changes and acceptance are Work operations recorded as WorkEvents.
WorkDelivery, rather than a synthetic assignment message, reliably wakes the
target runtime. Control records preserve supported steer/interrupt/resume
requests and their real acknowledgements.

Provider questions, approvals, or plan reviews that pause the provider use
`PendingInteraction`. A provider frame marked `completed` does not answer or
approve it.

A member may message the Host or another active member in the same
`AgentTeamRun`. Direct peer coordination does not require routine Lead
approval, but it remains visible to the Lead and should link the relevant Work
when one exists. Harness rejects cross-TeamRun senders, recipients, Work links,
causation, and correlations.

Member-to-Host messages are `manual_ack` and `delivered` when appended because
the control plane has received them. Host-to-member and member-to-member
ordinary messages are queued for the recipient's next available provider
round. Reading an inbox is a projection over latest message rows; it is not a
provider transcript.

Every Work submission cites the current Work id and version. Authored follow-up
conversation retains correlation and reply lineage; it does not become the
submission itself. The adapter may attach observed evidence references, but
must not turn provider final text into an automatic submission or duplicate
Message.

Harness never persists a second copy of provider chat, tool calls, commands,
file events, reasoning, or subagent transcripts. The bound provider-native
session is their sole truth and the only valid resume source.

### Steer and interruption must be honest

Steer means injection into the real current provider turn only when the
snapshotted provider mode supports and acknowledges that operation. Unsupported
or unavailable Steer fails with the reason. The caller may separately choose
an ordinary queued Message for the next provider round; Harness never silently
converts one operation into the other or displays a synthetic steer
acknowledgement.

Interrupt and resume likewise require mode-specific terminal acknowledgement.
Queued ordinary coordination never silently interrupts a busy member.

### Host controls replan, integration, and acceptance

The Host is Team Lead and follows this loop:

```text
observe inbox and outcomes
  -> answer questions or integrate completed lanes
  -> compare plan with actual state
  -> revise current Wave or advance
  -> create, assign, claim, reprioritize or accept Works
```

Small changes inside the same judgment boundary update the current Wave and
append a revision. A material change in plan, member composition,
responsibility, risk, or decision boundary advances the Wave and creates the
next one.

The Host does not wait for unrelated active work. It may integrate completed
lanes, advance, and carry an unfinished member forward with the same
`MemberRun`, Work ownership, Workspace, and native session.

There is no conditional-message object or Task Graph. When work depends on a
submission, the Host observes the durable Work transition and explicitly makes
the next Work ready, assigns it, or reviews it.

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
- Lead Inbox and Member Focus derive useful work state from Works,
  WorkDeliveries, messages, controls, and native-session references.
- Deferred dependency scheduling remains intentionally manual and explicit.

## Standard Two-Module Example

```markdown
| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| RuntimeBuilder | Runtime owner | Design, implement, and validate Inbox + delivery semantics; may use internal design/test subagents | Submitted Work with patch and checks |
| DashboardBuilder | UX owner | Design, implement, and validate Lead Inbox + Member Goal; may use internal UI/test subagents | Submitted Work with UI patch and checks |
| RiskReviewer | Independent reviewer | Review cross-module semantics after both handoffs | review_result |
```

The Lead assigns the first two Works concurrently. If RuntimeBuilder asks a
decision-shaped question, the Lead answers with the same correlation. When one
member submits early, the Lead can review and integrate it immediately. The
reviewer receives a separate Work only after the relevant submissions exist.
No conditional delivery edge is stored.

## Acceptance

The vertical scenario must prove:

1. two Codex members receive distinct end-to-end Works;
2. each member invokes at least one provider-native subagent without creating
   an implicit `MemberRun`;
3. one member-to-Host question/answer and one peer message preserve lineage;
4. member-to-Host delivery is immediately visible while peer delivery queues
   and is consumed only once;
5. one submitted Work is reviewed explicitly;
6. the Host revises or advances while another member continues on the same
   `MemberRun` and native session; and
7. Mission closeout, Wave history, messages, acknowledgements, evidence, and
   native execution remain reconstructable from CLI and Dashboard.

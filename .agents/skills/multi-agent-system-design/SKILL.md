---
name: multi-agent-system-design
description: "Use when designing, auditing, or productizing a multi-agent system: persistent agent members, SendMessage/mailbox protocols, task assignment delivery, busy/idle queues, runtime lifecycle, permission messages, hooks/plugins boundaries, dashboard visibility, and acceptance tests."
---

# Multi-Agent System Design

Use this skill before implementing or changing a multi-agent team, provider
integration, agent runtime, dependency graph, message bus, dashboard, or agent
collaboration protocol.

The goal is not to make agents talk. The goal is to make collaboration
reliable, observable, recoverable, and reviewable.

## Core Thesis

A multi-agent system succeeds only when agent collaboration is treated as a
durable workflow:

```text
identity
  -> mailbox
  -> delivery claim
  -> runtime turn injection
  -> provider events
  -> report/evidence
  -> review/decision
  -> dashboard proof
```

Hooks, plugins, transcripts, and prompt text are useful surfaces, but they must
not accidentally become the canonical message bus unless the system is designed
and tested that way.

The most common design failure is confusing a successful provider chat turn
with a reliable multi-agent workflow. A provider can answer once while the
system still cannot prove assignment, delivery, busy queuing, retry,
permission, report, review, or close semantics.

## First Questions

Answer these before coding:

| Question | Design risk |
| --- | --- |
| Who is the agent? | A process, provider thread, subagent, and durable member can be confused. |
| Who owns the mailbox? | Provider transcripts can fake state if the harness does not own messages. |
| How does a message enter a turn? | Busy agents, idle agents, resumed agents, and crashed agents differ. |
| What is the claim/lease model? | Concurrent delivery and crash recovery can duplicate or lose work. |
| What happens when the member is busy? | Interrupting can corrupt current work; dropping messages loses coordination. |
| How does the agent reply? | Plain assistant text may not be visible to the team. |
| How are permissions requested? | Safety decisions need durable request/response state. |
| How are tasks assigned? | Setting `assignee` is not the same as delivering work. |
| What do hooks observe? | Lifecycle hooks are not automatically safe delivery mechanisms. |
| What does the Dashboard prove? | A UI that reads raw chat cannot validate the workflow. |

## Critical Engineering Problems

For any provider or agent-team implementation, explicitly decide these before
writing code:

| Problem | Decision to record |
| --- | --- |
| Identity boundary | Which object is the durable member, and which objects are provider threads, subprocesses, or native subagents? |
| Mailbox ownership | Does the product control plane, provider, file mailbox, database, or external queue own messages? |
| Delivery claim | What atomic claim/lease happens before provider side effects? |
| Turn injection | Which component turns a message into user input, tool input, or provider-native communication? |
| Busy policy | Queue, interrupt, reject, or reroute when the recipient is running? |
| Ack timing | When can a message be marked read, acknowledged, delivered, failed, stale, or canceled? |
| Reply path | Which explicit message/tool/report object makes the result visible to teammates? |
| Hook boundary | Which lifecycle facts can hooks observe, and what are hooks forbidden to decide? |
| Dashboard proof | Which normalized objects let the UI prove state without provider transcript spelunking? |
| Crash recovery | What happens if the dispatcher crashes before, during, or after provider turn creation? |

Record durable choices as ADRs when they affect identity, mailbox ownership,
delivery claim, provider-neutral interfaces, permissions, task/report flow, or
Dashboard control-plane behavior.

## Design Workflow

For a new or changed multi-agent mechanism:

1. Define the actor model.
   - Durable member id, display name, role, team, provider, runtime, workspace,
     and permission profile.
   - Distinguish harness members from provider-native subagents or child
     threads.

2. Define the mailbox.
   - Message schema: id, from, to, channel, task id, content, status, delivery
     attempt, evidence refs.
   - Source of truth: append-only store, database, provider state, or external
     queue.
   - Read policy: latest projection, FIFO, priority, broadcast, direct message.

3. Define delivery.
   - Claim/lease before provider side effects.
   - Turn/input envelope with message id, task id, sender, recipient, and kind.
   - Terminal states for delivered, acknowledged/running, failed, stale, and
     canceled.
   - Crash recovery and retry semantics.

4. Define busy/idle behavior.
   - Idle: deliver next eligible message.
   - Busy: enqueue normal messages; allow explicit interrupt only by policy.
   - Stale: reconcile before more delivery.
   - Closed: reject or fail delivery.

5. Define reply and handoff.
   - Require an explicit tool or message protocol for reports, questions,
     blockers, decisions, and handoffs.
   - Do not treat unstructured assistant text as visible team state unless an
     ingestor materializes it.

6. Define permission and review.
   - Permission request/response should be messages or first-class workflow
     objects.
   - Live, destructive, money-moving, or secret-touching actions require an
     explicit decision trail.

7. Define observation.
   - Provider notifications and hooks reduce into normalized events.
   - Hooks can enrich evidence and lifecycle state; they should not silently
     own mailbox semantics.
   - Plugins package skills/hooks/tools; they should not own canonical state.

8. Define dashboard proof.
   - Show member runtime state, inbox/outbox, current task, provider sessions,
     event timeline, reports, evidence, reviews, and decisions.
   - The dashboard should expose delivery gaps without reading raw provider
     logs.

## Acceptance Checklist

Require evidence for:

- a task assignment was delivered through a message, not only an assignee field;
- the recipient member received the message in a provider turn or a recorded
  failed delivery fixture;
- busy delivery queues instead of corrupting an active turn;
- claim/lease prevents stale queued rows and concurrent duplicate delivery;
- closed/retired members cannot be silently revived by delivery;
- reports and handoffs use explicit messages or materialized objects;
- permission requests and responses are durable;
- hooks/plugins are clearly separated from the canonical message bus;
- dashboard read models match the delivery queue projection.

Reject an implementation as not truly multi-agent when:

- it sets an assignee field but never delivers a task message;
- it relies on final assistant text as the only report channel;
- hooks silently inject or consume messages without a mailbox contract;
- a busy member can lose or unexpectedly interrupt work;
- a closed member can be revived by ordinary message delivery;
- the Dashboard cannot explain a member's current state from durable objects.

## Claude Code Case

When the task needs a concrete reference design, read
[references/claude-code-agent-teams.md](references/claude-code-agent-teams.md).

Use that case for engineering principles, not object names. Extract:

```text
case observation
  -> failure mode
  -> reusable principle
  -> target-system adaptation
```

The key principle from the case is:

```text
Harness owns mailbox; Provider Gateway delivers turns.
```

For systems not named Harness, translate this as:

```text
The product control plane owns durable messages; the provider/runtime adapter
injects claimed messages into agent turns.
```

# Why Agent Teams Need Shared Works, Not Assignment Messages

```text
status: research absorbed by ADR 0050; retained as public failure analysis
owner_role: execution-foundation
authority_class: research
implementation_state: accepted_contract_cutover_in_progress
evidence_snapshot: 2026-08-02
publication_status: draft
```

> This document preserves the investigation that led to
> [ADR 0050](../decisions/0050-agent-team-work-board-and-message-boundary.md).
> The accepted target contract now lives in
> [Agent Team Works](../product/agent-team-works.md). Research observations are
> evidence, not implementation proof.

## Abstract

We tried to scale a long-running Agent Team using durable messages alone. The
team grew to 23 MemberRuns and 1,103 messages, yet the Host still had to remember
what work existed, who owned it, which item was ready, and what an idle Agent
should do next. More Agents increased coordination traffic faster than useful
throughput.

This article reconstructs that failure and derives a smaller model: Works hold
responsibility and mutable execution state; Messages hold authored discussion;
WorkDelivery reliably brings a Work version to a runtime; native Provider
sessions remain execution truth. Mission and Wave stay above the board as
durable intent and Host judgment rather than becoming task containers.

## Finding

Firm Agent Team has a durable mailbox but no durable shared work list.
That omission forced `TeamMessage(kind=assignment) + correlation_id` to act as
work request, ownership record, status, dependency marker, and conversation.

Messages are good at communication and poor at representing mutable shared
state. In the dogfood run studied here, the result was repeated state
reconstruction, duplicate-looking assignments, Host bottlenecks, idle Standing
Agents, and no cheap answer to four questions:

1. What work exists?
2. Which work is assigned, unassigned, blocked, or ready for review?
3. Who owns each item now?
4. What should a newly idle Member claim next?

The accepted direction is a shared `Works` surface built on a minimal `Work`
base object:

```text
Agent          = responsible actor
Work           = durable responsibility and current state
Kanban         = view over Works
TeamMessage    = authored conversation
WorkEvent      = Work state transition
WorkDelivery   = reliable Work notification to a runtime
Native Session = execution truth
```

Company WorkItem remains a separate governed business object and links Agent
Team Works as execution attempts. They share a small semantic vocabulary, not
storage inheritance or owner/status authority.

## Evidence

The full native reconstruction is preserved in
[Agent Team shared work-list failure reconstruction](agent-team-shared-task-list-evidence.md).

The focused two-Member audit expanded into a 23-MemberRun, 1,103-message
operating system without gaining a queryable view of work. Adding Members
increased mail while the Host remained the private task database and scheduler.

Across 94 Assignment messages:

- 14 explicitly mention an existing WorkItem;
- 7 explicitly warn not to create a duplicate; and
- 40 ask the recipient to inspect or re-read current state.

One WorkItem appeared in 13 distinct Assignment correlations over 7.64 hours.
Those were not all duplicate implementations, but every phase reconstructed the
same current work state in a new conversation chain.

## Root Cause

The failure was not primarily too few Agents, weak models, insufficient
permissions, or too little messaging. It was a missing shared state object.

| Symptom | Workaround | Why it failed |
| --- | --- | --- |
| idle Members while work remained | Host sent more Assignments | Host first had to reconstruct demand |
| repeated WorkItem inspection | put more context in messages | copied context became stale |
| duplicate-looking correlations | warn against duplicates | warning is not atomic ownership |
| unclear ordering | describe it in Markdown | no queryable ready pool |
| repeated Handoffs | summarize the lane again | conversation remained the task database |
| slow Org hierarchy | add management layers | every layer repeated reconstruction |

```text
no shared Works
  -> Assignment Message becomes task identity
  -> state lives in private Agent context
  -> idle Members cannot discover ready work
  -> Host becomes scheduler and database
  -> more Members produce more coordination mail
  -> Organization layers amplify the weakness
```

## Claude Code Agent Teams Comparison

Claude Code Agent Teams separates Team Lead, Teammates, a shared Task List, and
a Mailbox. Tasks use pending, in-progress, and completed states. The Lead may
assign work, or a Teammate may atomically claim unassigned unblocked work.
Completing a prerequisite makes dependent work claimable. See
[Claude Code Agent Teams](https://code.claude.com/docs/en/agent-teams).
The source documents Agent Teams as experimental and describes coordination and
resume limitations. The comparison is a mechanism study, not a production or
feature-parity claim.

Its communication path is separate:

```text
SendMessage
  -> locked mailbox write
  -> recipient runtime polls
  -> idle recipient receives next prompt
  -> busy recipient queues until a safe boundary
```

Hooks observe lifecycle; they are not the mailbox. Subagents report to their
caller, while teammates remain independently addressable collaborators. See
[Claude Code hooks](https://code.claude.com/docs/en/hooks) and
[Claude Code subagents](https://code.claude.com/docs/en/agents).

Firm should copy the separation of shared task state and communication,
not Claude's provider-local storage or flat-team lifecycle limitations.

## Why Assignment Message Was Rejected

The intermediate proposal retained Assignment Message as notification and
compatibility. That was rejected because it creates two plausible ownership
truths:

```text
Work.owner
TeamMessage(kind=assignment)
```

Every store, CLI, adapter, Dashboard selector, Skill, and operator would then
have to reconcile them. The accepted model instead uses:

```text
Work assignment
  -> WorkAssigned event
  -> WorkDelivery
  -> provider-safe Work envelope
```

The mailbox delivery substrate remains useful, but the delivered object is a
Work change, not an authored Message. There is no Assignment-message
compatibility path. Historical evidence is retained here or in native exports;
active dogfood data and code move to one model.

## How Work And Messages Cooperate

Separating the objects does not separate the user experience. A Work detail can
show its linked conversation, and a Message can create a draft Work, but the
state transition is always explicit:

```text
allocate responsibility -> create/assign or claim Work
explain or negotiate    -> send a Message linked to Work
change scope            -> update Work; optionally explain with Message
return result           -> submit Work with a result summary and criterion-required evidence
review                  -> accept or request changes on Work
```

The distinction matters when Agents are busy or recover after a crash. A queued
Message says “there is unread conversation.” A queued WorkDelivery says “this
exact Work version must enter the Member's execution context.” Neither implies
that the Provider understood it, started it, completed it, or received Host
acceptance.

The shared pool also changes allocation. The Host can directly assign bounded
responsibility; an idle eligible Member can atomically claim ready unassigned
Work. Completing the last blocker makes an assigned Work deliverable or exposes
an unassigned Work to the pool. No Agent has to replay chat to discover demand.

## Why WorkItem Links Work

The intermediate proposal also named the Team object `TeamTask` and kept it
separate from Company WorkItem. That created two nearly identical scheduling
models.

The accepted relation is:

```text
Company WorkItem --source/attempt relation--> Agent Team Work
```

WorkItem owns business provenance, accountable actor, approvals, finance, and
governed closure. Agent Team Work owns one TeamRun's member responsibility,
readiness, execution state, result, and evidence. A Work event never silently
changes WorkItem authority; a governed Company command aggregates accepted
execution results back into the WorkItem.

## Creation And Delegation Conclusion

Only-Host creation would keep the Host as a bottleneck. Unrestricted peer
assignment would make responsibility unstable. The accepted middle ground is:

- Host manages every Work in its Team;
- Member may create self-owned Work, unassigned Work, and child Work under Work
  it owns;
- Member may atomically claim eligible ready Work;
- ordinary Member cannot force assignment to a same-level peer; and
- a Member allowed to create a child Team becomes that Team's Host and assigns
  child Works there.

This provides autonomy through structure rather than a large permission matrix.

## Mission Conclusion

Works do not replace Mission. They replace Assignment-message ownership and
task enumeration inside Wave prose.

```text
Mission = why and durable outcome
Wave    = material change in Host plan or judgment
Works   = current demand, ownership, readiness, and state
Message = discussion around the work
```

A standalone Team can use Works without Mission. Mission remains valuable when
one outcome spans multiple Teams, Workflows, direct Host actions, or important
re-plan and closeout decisions.

## Publication Takeaway

The lesson is not “add a Kanban because Kanbans are convenient.” Reliable Agent
collaboration needs both conversation and shared mutable work state. Without
both, each Agent receives fragments while the Host becomes the only place where
plan, ownership, readiness, and completion coexist. More Agents then increase
mail faster than throughput.

```text
Messages explain and negotiate work.
Works expose and transfer responsibility.
Mission/Wave preserves Host intent and judgment.
Native Sessions prove execution.
Company WorkItems add business accountability.
```

## Sources

- [Claude Code Agent Teams](https://code.claude.com/docs/en/agent-teams)
- [Claude Code hooks](https://code.claude.com/docs/en/hooks)
- [Claude Code subagents](https://code.claude.com/docs/en/agents)
- Native TeamRun `team-run-1785417589241-p28630-0` and linked Sessions
- [Agent Team Works](../product/agent-team-works.md)
- [ADR 0050](../decisions/0050-agent-team-work-board-and-message-boundary.md)

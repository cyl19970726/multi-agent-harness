# Data Model

This document explains the object and state model that must exist for the
product vision to be true. It does not replace JSON schemas. Schemas own stable
fields; this file owns the relationships, projections, and source-of-truth
rules those fields must preserve.

## Vision Link

Star Harness must turn durable intent into:

```text
Mission -> ordered Wave -> executor attempt(s)
  -> explicit coordination/artifacts/outcome + provider-native session refs
  -> lightweight Wave gate -> next Wave or Mission closeout
```

The executor is `agent_team`, `dynamic_workflow`, or `host`. A Wave never
requires a legacy dependency graph. The data model succeeds when another human or agent can
reconstruct the selected executor, attempts, ownership, outcome, and gate from
Harness state, and resolve member execution detail from provider-native
sessions without duplicating those sessions.

## Key Questions

| Question | Data-model answer |
| --- | --- |
| What is the durable intent? | Native `Mission`. |
| What is the ordered work boundary? | Native `Wave` rows ordered by `index`; there is no required work graph. |
| Which execution happened? | `Wave.executor_run_ids`; executor-specific ledgers own internal state. |
| How is Agent Team work assigned? | `TeamMessage(kind=assignment)` plus its `correlation_id`. |
| Who is accountable inside a team attempt? | `MemberRun` role/identity plus assignment and handoff lineage. |
| What supports an outcome? | Explicit Harness outcome/check/artifact refs and handoffs, plus provider-native records for member-execution claims. |
| What is being accepted? | The Wave outcome and one completed attempt through the lightweight Wave gate. |
| What is provider state? | A mode-aware native session binding; the provider-native store owns transcript, tools, turns, and resume state. |
| What becomes reusable learning? | Mission closeout, follow-up Waves/issues, and optional evaluation/cases. |

## Source Of Truth

| Concept | Canonical object | Projection or evidence |
| --- | --- | --- |
| Product purpose | PRD and design basis | README summaries |
| Object meaning | [concept-model.md](concept-model.md) and schemas | Dashboard labels, CLI help |
| Coordination state | Harness store | Dashboard projections |
| Mission status | latest native `Mission` row | Dashboard summary |
| Wave order and gate | latest native `Wave` rows | Dashboard Wave timeline |
| Agent Team attempts | `AgentTeamRun` rows linked by Mission/Wave ids | run cards |
| Agent Team assignment | assignment `TeamMessage` plus correlation lineage | member current action, lane UI |
| Agent Team identity | `MemberRun` inside one TeamRun | provider thread id, prompt file |
| Runtime health | Harness lifecycle/control acknowledgement plus provider adapter availability | pid, socket, native provider status |
| Provider execution | provider-native session selected by `NativeSessionRef` | ephemeral normalized Dashboard projection |
| Provider interaction routing | Harness `PendingInteraction` | provider reverse-RPC frame in the native session |
| Outcome support | explicit Harness outcome and artifact/check refs; provider-native session for execution claims | unaccepted chat summary |
| Wave acceptance | `Wave.gate_status` + `accepted_run_id` + outcome/artifacts | reviewer comment or provider self-report alone |
| Optional evaluator output | `Review` | report message text |
| Defect / risk ledger | `Gap` (Bug = `Gap(category=bug)`) | `product-gap-inbox.md` flat file |
| Reusable lesson | `LearningNote` or an explicit reusable document | full transcript |
| Long-lived target | `Vision`; Missions link to the intent they advance | loose text reference |

## Object Clusters

```mermaid
flowchart TD
  Mission[Mission] --> Wave[Wave]
  Wave --> TeamRun[AgentTeamRun attempt]
  Wave --> WorkflowRun[WorkflowRun attempt]
  Wave --> HostRun[Host outcome reference]
  TeamRun --> TeamMsg[TeamMessage assignment + correlation]
  TeamRun --> Member[MemberRun]
  Member --> Binding[NativeSessionRef]
  Binding --> Session[Provider-native session]
  TeamMsg --> Artifact[Artifacts/checks/outcome]
  Session -. execution detail .-> Artifact
  WorkflowRun --> Artifact
  HostRun --> Artifact
  Artifact --> Gate[Wave gate]
  Gate --> Wave
```

## Optional Governance Objects

`Review`, `Gap`, `Evidence`, `Decision`, `Evaluation`, and `LearningNote` may be
used when a domain or repository gate needs them. They enrich execution proof;
they are not mandatory levels between Wave outcome and gate. Product-specific
WorkItems, Approvals, finance, metrics, and documents are defined by the
Company OS contracts rather than by a generic task graph.

Retired object fields that remain in internal schemas are removal debt governed
by [ADR 0028](decisions/0028-retire-goal-phase-task-graph.md). They must not be
read into native Mission/Wave projections or used as a reason to retain old UI.

## Projection Rules

- `Task.assignee_agent_id` is allowed only as a read-model or convenience
  projection of assignment; assignment truth is the task message.
- `AgentMember.current_task_id` is a projection of delivery and active runtime
  events; it is not proof that the member received the task.
- Dashboard columns are read models; safe actions must create or update
  canonical harness objects.
- Provider thread/session ids are native execution refs; they do not own
  assignment, Approval, outcome acceptance, or Wave gate state.
- Normalized provider activity is an ephemeral read projection, not a Harness
  ledger and not evidence independent of its native session.
- PR refs and diff refs support a proposal; they are not the proposal itself.

## Invariants To Gate

Native invariants:

1. Every Wave references one native Mission and has a positive, unique order
   within it.
2. Every AgentTeamRun linked to a Wave uses an `agent_team` Wave and the same
   Mission id.
3. Every accepted Agent Team Wave names a completed run already present in its
   immutable attempt list.
4. Explicit message lineage stays inside one TeamRun; assignment correlation is
   never fabricated from body text.
5. New provider transcripts, tool/command/file event streams, and thinking are
   never mirrored into durable Harness actions, snapshots, replay, evidence,
   or peer messages.
6. Domain project facts and behavior enter through adapters, skills, and tool
   descriptors, not generic core state.
7. Parallel file-changing members need distinct workspaces, branches, or
   explicit owned-path coordination.

Retired coordination flows have no separate active invariants. Archive records
exist only to explain removal history.

## Relationship To Schemas

When a relationship is stable, schemas should include the fields needed to
represent it. When a rule is stable, CLI/API/CI should validate it. This file
keeps the reason and invariant so future schema changes do not erase the
product intent.

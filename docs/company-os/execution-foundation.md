# Company OS Execution Foundation

## Position

The Company OS is organized around Documents and a mixed human/Agent
organization. Its execution foundation is the existing provider-neutral
Harness runtime:

```text
Document / TypedRecord
  -> WorkItem
  -> choose execution method when work is ready
  -> outcome, artifacts, evidence, metrics, and record/document updates
```

The execution foundation is essential infrastructure, but it is not the
product homepage, company hierarchy, primary documentation tree, or financial
system. A Mission, Wave, AgentTeamRun, WorkflowRun, ProviderSession, or runtime
does not own a company business domain merely because it executed some work for
it.

## Execution objects retained from the Harness

### Mission and Wave

`Mission -> ordered Wave -> executor` remains the native hierarchy for durable
multi-stage execution. A Mission captures durable execution intent; a Wave is
a lightweight ordered unit with an objective, selected executor, outcome, and
gate. Executor-internal planning remains inside the selected executor.

In the Company OS, a WorkItem may initiate or reference a Mission/Wave when its
business outcome needs staged execution. The WorkItem remains the document- and
responsibility-facing record; Mission/Wave remains the execution-facing record.

### AgentTeamRun and MemberRun

An `AgentTeamRun` is one Agent Team execution attempt for a Wave. A `MemberRun`
is one participant instance inside that attempt. Assignment-message correlation
continues to prove lane ownership inside the TeamRun:

```text
TeamMessage(kind=assignment)
  -> correlation_id
  -> MemberAction / handoff / blocker / review result
  -> artifacts and outcome
```

Neither object is an OrgUnit, a standing organization member, or a business
WorkItem. A durable AgentMember can only appear in a standing Agent projection
when an explicit stable link exists (for example,
`MemberRun.agent_member_id`). A temporary MemberRun remains temporary even if
its displayed name, provider, model, role, or timestamps resemble a standing
Agent.

### Dynamic Workflow

Dynamic Workflow remains the executor for one-shot structured work. A
`WorkflowRun` and its `WorkflowStep`s own the workflow's internal steps,
fan-out, retries, results, and artifacts. They do not become a TeamRun and do
not acquire organizational identity.

A WorkItem may reference the WorkflowRun that fulfilled it. An Agent-centric
projection may cite workflow participation only when a step has an explicit
durable Agent/session link.

### Host execution

Host execution means a resident Host Agent performs a Wave directly. The Host
may use provider-native subagents as an implementation detail. The Harness
records observable outcomes, artifacts, and optional honest attribution; it
must not invent lifecycle control over provider children it does not control.

### Provider foundation

`AgentMember`, `AgentRuntime`, `ProviderSession`, provider child threads,
capability snapshots, permission/budget ceilings, hooks/plugins, and durable
events remain shared infrastructure. Provider transcript detail does not become
the source of truth for assignment, organization responsibility, approval, or
business result. Private thinking remains sanitized, transient live state only:
it is not stored, replayed, forwarded to peers, or used as evidence.

## Selection from a WorkItem

The product does not force every WorkItem to become a Mission/Wave. The
accountable owner chooses proportionate execution:

| Work shape | Appropriate execution |
| --- | --- |
| Small document update or human follow-up | direct human/Agent action recorded on the WorkItem |
| One-shot, structured, bounded work | Dynamic Workflow |
| Collaborative work needing messages, handoffs, or review | Agent Team via Mission/Wave |
| Durable, staged outcome with several gates | Mission with ordered Waves |
| Direct resident-agent operation | Host executor, with observable outcome |

The chosen run is recorded as `WorkItem.execution_ref`; the result must update
the WorkItem's result document/records and attach useful evidence. This closes
the document-to-action-to-document loop without making execution logs the
company knowledge base.

## Boundaries preserved by existing ADRs

ADR 0025 and ADR 0026 remain valid.

- **ADR 0025 — Agent Team Run Control Plane:** AgentTeamRun is a Wave-scoped
  attempt; MemberRun and TeamMessage own run-scoped collaboration. This remains
  separate from standing organization and company documents.
- **ADR 0026 — Mission/Wave Product Architecture:** Mission/Wave is the native
  execution hierarchy and remains the only live orchestration model.

The Company OS model changes their placement, not their execution semantics:

```text
Company OS business layer
  Documents / Modules / Records / Relations / Org / WorkItems / Approvals
    -> execution foundation selected by WorkItem
      Mission -> Wave -> Agent Team | Dynamic Workflow | Host
```

## Retirement boundary

The superseded coordination stack is not an execution option. ADR 0028 freezes,
exports, verifies, and deletes it without coercing its historical rows into
Mission, WorkItem, Approval, or organization membership.

## Execution invariants

1. A WorkItem can exist before an executor is selected; execution selection is
   not business intake.
2. A selected executor cannot overwrite accountable ownership, approval
   authority, or document provenance held by the WorkItem.
3. Agent Team lane ownership is proved by TeamMessage correlation, not an
   assignee display field.
4. A TeamRun/MemberRun never becomes a standing Agent or OrgUnit by inference.
5. Provider-native subagents stay implementation detail unless explicitly
   materialized through a truthful observation or promotion contract.
6. Workflow and Host execution preserve their own semantics; shared sessions,
   artifacts, and events do not collapse them into one universal run object.
7. Execution outcomes are returned as explicit summaries, artifacts, evidence,
   metric observations, and result-document/record updates—not raw transcripts
   or thinking.

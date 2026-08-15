# Company OS Execution Foundation

## Position

The Company OS is organized around Documents and a mixed human/Agent
organization. Its execution foundation is the existing provider-neutral
Harness runtime:

```text
Document / TypedRecord
  -> TeamWork
  -> choose execution method when work is ready
  -> outcome, artifacts, evidence, metrics, and record/document updates
```

The execution foundation is essential infrastructure, but it is not the
product homepage, company hierarchy, primary documentation tree, or financial
system. A Mission, Mission Log entry, AgentTeamRun, WorkflowRun, native session
reference, or runtime does not own a company business domain merely because it
executed some work for it.

ADR 0042 formalizes the identity boundary:

```text
Company Store       Execution Space       Project Binding
     \                    |                    /
      \------ explicit, optional relations ---/
```

Company TeamWorks may reference execution. Execution must not require a
Company. Project/repository selection is a runtime binding, not Company truth.
Company Store, Execution Space, and Project Binding registries and selectors
are implemented independently. Project-derived execution/Company stores remain
an explicitly labelled compatibility path until their data is migrated and
retired through governed operations.

## Execution objects retained from the Harness

### Mission and Mission Log

A Mission captures durable execution intent and owns exactly one flat
AgentTeam. Its append-only Mission Log contains small Markdown records of the
Host's judgment, replan, recovery, and closeout evidence. A Mission Log entry
is not a lifecycle object, task graph, executor container, synchronization
barrier, gate, or provider-session boundary.

In the Company OS, a TeamWork may initiate or reference a Mission when its
business outcome needs coordinated execution. The TeamWork remains the
document- and responsibility-facing record; Mission plus its Mission Log
remains the execution-facing record. Outside Company OS, the same Mission and
Mission Log remain usable in a standalone Execution Space with
`company_id = null`.

Wave create/update/advance/gate is retired by ADR 0051. Pre-cutover `Wave`,
`WaveStatus`, `WaveGateStatus`, `wave_ids`, and `waves.jsonl` exist only for
historical read/export compatibility and cannot define current execution.

### AgentTeamRun and MemberRun

An `AgentTeamRun` is one execution of the Mission's AgentTeam and freezes the
Team's Node plus the selected Project Binding. It may remain active across
many Mission Log judgments and replans. A `MemberRun` is one
participant instance inside that run; its provider-native session may continue
while the Host replans and appends judgment to the Mission Log. The Agent Team
Works contract proves lane
responsibility:

```text
Work assignment/claim -> WorkOperation(WorkEvent + resulting Work + deliveries)
  -> WorkDelivery
  -> MemberRun + provider-native session
  -> Work block / submission / review / acceptance
  -> linked correlated Message/reply when needed
  -> explicit outcome and artifact/check references
```

Neither object is an OrgUnit or business TeamWork. AgentMember is the single
durable organization-agent identity; MemberRun is one execution attempt and
must carry its exact `agent_member_id`.

The implemented Agent Team path preserves this identity when a TeamRun is
created from its Mission-owned AgentTeam. Company Organization stores only an
AgentMember ActorRef membership projection; it never copies provider/runtime
payload or creates a second identity. MemberRun owns participation in one
TeamRun.

An AgentTeamRun may execute against one Project Binding, while a later TeamRun
of the same Mission Team uses a different Project Binding registered on the
same Node. The Mission/Mission Log history belongs to the Execution Space.
`AgentTeamRun.project_binding_id` pins
provider execution context so a later selector change cannot retarget existing
members.

The Host owns member lifecycle explicitly. Starting a Team member creates or
resumes its persistent provider runtime; ordinary turn completion does not
destroy that member. The Host may message, inspect, interrupt one current turn,
resume from the native session, or Close the member runtime. TeamRun or Mission
closeout never substitutes for Close.

Physical live-control handles are process-local to the machine NodeDaemon.
Its lease is the parent authority for every local Team Supervisor lease.
Dashboard/MCP controls route through the Node daemon's loopback locator; that
owner fences the operation against both daemon and Team generations immediately
before using a physical handle. Before a Supervisor claims queued mail it
verifies that the selected provider transport is live. The per-recipient
`CanonicalMessageDelivery` then records an atomic claim, a native provider
receipt, and recipient ACK separately. Transport
failure before claim leaves mail queued and reconnects the recorded native
session first.

Current Team mail is an identity-first immutable `Message`: the server freezes
the authenticated sender AgentIdentity and resolves explicit AgentIdentity or
Team subscription recipients. Each participating recipient is reached only
through its own idempotent `CanonicalMessageDelivery`. This lets Organization
and Agent Team share Inbox UI while keeping AgentMember identity and MemberRun
lifecycle separate. Unbound Dashboard/MCP/API clients cannot
impersonate a Member.

Explicit Close is durably latched before process-local teardown. A racing lease
or control receiver cannot silently revive the member; idle, Work submission,
Mission Log append, TeamRun completion, and Mission closeout are all
non-terminal for the member runtime.

Status-only
cancellation deliberately refuses `running -> cancelled`, because changing a
row cannot stop provider work. If the foreground Host disappears *after the
operator has independently confirmed that every provider process stopped*, the
CLI recovery path is explicit and audited:

```bash
firm team-run cancel --id <run> --confirm-provider-stopped \
  --reason <why-the-host-disappeared> --cancelled-by <actor>
```

Recovery marks unfinished members `stopped`, records cancelled `interrupted`
actions, and preserves the run. The Host appends the recovery and retry
decision to the Mission Log.
The flag is an operator attestation, not a claim of cooperative interruption.
The first real Codex/Kimi evidence for this path and its successful retry is
recorded in
[the live Agent Team acceptance](../integration/live-agent-team-acceptance-2026-07-21.md).

### Dynamic Workflow

Dynamic Workflow remains the engine for one-shot structured work. A
`WorkflowRun` and its `WorkflowStep`s own the workflow's internal steps,
fan-out, retries, results, and artifacts. They do not become a TeamRun and do
not acquire organizational identity. `WorkflowRun.project_binding_id` pins the
provider cwd, instruction/Skill boundary, and patch/artifact root independently
from the Execution Space that owns the run rows.

A TeamWork may reference the WorkflowRun that fulfilled it. An Agent-centric
projection may cite workflow participation only when a step has an explicit
durable Agent/session link.

### Host execution

Host execution means a resident Host Agent performs work directly. The Host
may use provider-native subagents as an implementation detail. The Harness
records observable outcomes, artifacts, and optional honest attribution; it
must not invent lifecycle control over provider children it does not control.

### Provider foundation

`AgentMember`, `MemberRun`, native provider-session bindings, provider child
threads, capability snapshots, permission/budget ceilings, hooks, and plugins
remain shared infrastructure. The provider-native store is the sole truth for
one agent's transcript, tool/command/file events, turn lifecycle, and resume
state. Harness references that session and owns Work responsibility, organization
responsibility, interaction routing, explicit outcomes, artifact/check refs,
and gates. It does not keep a second provider event history. Private thinking
remains sanitized, transient live state only: it is not stored, replayed,
forwarded to peers, or used as evidence.

Provider-native visibility is mode-specific. A Codex `app-server` thread can be
opened in Codex Desktop when the app exposes that native thread. A Claude Agent
SDK session is the native execution record for `claude_agent_sdk`, but it is not
a Claude Desktop conversation and Harness must not claim that it appears
there. The same rule applies to every provider: native session truth does not
imply visibility in an unrelated consumer UI.

## Selection from a TeamWork

The product does not force every TeamWork to become a Mission. The
accountable owner chooses proportionate execution:

| Work shape | Appropriate execution |
| --- | --- |
| Small document update or human follow-up | direct human/Agent action recorded on the TeamWork |
| One-shot, structured, bounded work | Dynamic Workflow |
| Collaborative work needing shared responsibility, messages, or review | the Mission's flat, Node-placed Agent Team |
| Durable, evolving outcome with Host replans/recovery/closeout judgment | Mission with an append-only Mission Log |
| Direct resident-agent operation | Host action, with observable outcome |

The chosen run is recorded as `TeamWork.execution_ref`; the result must update
the TeamWork's result document/records and attach useful evidence. This closes
the document-to-action-to-document loop without making execution logs the
company knowledge base.

## Boundaries preserved by existing ADRs

ADR 0025 and ADR 0026 are partially superseded by ADR 0034 and ADR 0051.

- **ADR 0025 — Agent Team Run Control Plane:** MemberRun, correlated Message,
  and provider-native session boundaries remain valid.
  Wave-scoped attempt ownership and v0 lifecycle/delivery details are
  superseded.
- **ADR 0026 — Mission/Wave Product Architecture:** historical design context;
  its active Wave vocabulary and authoring contract are superseded by ADR
  0051. Its transient-thinking rationale remains separately reflected in the
  current runtime contract.
- **ADR 0034 — Host Plan Waves And Mission-Scoped Agent Teams:** historical
  Mission/Team and Wave language is superseded by the one Mission = one flat
  AgentTeam contract; TeamRuns and native sessions remain execution history.
- **ADR 0044 — Durable Team Supervision And Typed Mail:** latest-wins
  Supervisor generations, typed actors, claim/provider receipt/ACK, stable
  Agent routes, cross-process controls, reconnect, and Close define the current
  Agent Team control substrate.

The Company OS model changes their placement, not their execution semantics:

```text
Company OS business layer
  Documents / Modules / Records / Relations / Org / TeamWorks / Approvals
    -> execution foundation selected by TeamWork
      Mission -> append-only Mission Log
      Mission <-> Agent Team -> TeamRun -> MemberRun
      Dynamic Workflow | Host action
```

## Retirement boundary

The superseded coordination stack is not an execution option. ADR 0028 freezes,
exports, verifies, and deletes it without coercing its historical rows into
Mission, TeamWork, Approval, or organization membership.

## Execution invariants

1. A TeamWork can exist before an executor is selected; execution selection is
   not business intake.
2. Execution can exist without a Company Store; Company linkage is optional for
   every Mission, Mission Log entry, TeamRun, MemberRun, WorkflowRun, and
   provider session.
3. A selected executor cannot overwrite accountable ownership, approval
   authority, or document provenance held by the TeamWork.
4. Agent Team responsibility is proved by Work owner/version and WorkEvents;
   current identity-first Message correlation explains conversation only, and
   per-recipient `CanonicalMessageDelivery` proves delivery state.
5. A TeamRun/MemberRun never becomes an organization Agent Membership or OrgUnit by inference.
6. Provider-native subagents stay implementation detail unless explicitly
   materialized through a truthful observation or promotion contract.
7. Workflow and Host execution preserve their own semantics; shared sessions,
   artifacts, and events do not collapse them into one universal run object.
8. Execution outcomes are returned as explicit summaries, artifact/check
   references, metric observations, and result-document/record updates. Native
   transcripts remain provider-owned and referenced; thinking is never durable.
9. Dashboard activity joins durable Harness coordination with an ephemeral,
   rebuildable provider-native projection. That projection is not a second
   ledger and cannot make the Host's Mission judgment.
10. Appending a Mission Log entry never implicitly stops a TeamRun, MemberRun,
   Work, or native session. Closing a Mission never deletes or archives a
   linked team.
11. Only the current Supervisor generation may claim queued Team mail or use
    live provider controls; it must prove transport health first.
12. Typed message provenance cannot be replaced by display names or caller
    claims, and provider receipt never implies semantic completion.
13. Organization identity is joined to execution only by a stable explicit
    identifier. Runtime status never grants business authority and organization
    status never fabricates a running provider session.
14. Provider cwd is selected from Project Binding roots or validated worktrees;
     Company Store and Execution Space directories are never provider cwd.

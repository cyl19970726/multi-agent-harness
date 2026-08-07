# Architecture Map

This is the canonical product-level architecture map. Detailed object contracts
live under [company-os](../company-os/README.md). Implemented execution details
remain documented by the Mission/Wave, Workflow, Agent Team, runtime, and
provider specifications.

```mermaid
flowchart TB
  Human["Human operators"]
  Home["Company Home"]
  Docs["Docs\npages · modules · records · relations · views"]
  Blocks["Basic Documents + Blocks"]
  Views["Standard structured Views"]
  Custom["Registered Custom Pages\nHTML / React package"]
  Actions["Scoped Queries + Action Commands"]
  Policy["Policy · Approval · Audit"]
  Org["Organization\nhumans · standing agents · external · services"]
  Collab["Collaboration spine\nconversation · activity · artifacts"]
  TeamWork["Agent Team Works\nownership · readiness · Kanban · review"]
  Work["Company Work\nWorkItems · Milestones · business relations"]
  Approval["Approvals and Needs You"]
  Gov["Governance Agents\nDocs · Work · Finance · Org / HR"]
  Finance["Finance and Metrics"]
  Exec["Execution selection"]
  Mission["Mission context / ordered Host-plan Waves"]
  Team["Independent AgentTeam / AgentTeamRun / MemberRun"]
  Supervisor["Durable Team Supervisor\nlease · typed mail · claims · controls"]
  Workflow["Dynamic Workflow"]
  Direct["Human / Standing Agent direct work"]
  Runtime["Providers · sessions · plugins · MCP"]
  Result["Results · evidence · artifacts · observations"]

  Human --> Home
  Home --> Docs
  Home --> Org
  Org --> Collab
  Docs --> Collab
  Collab --> Work
  Docs --> Blocks
  Docs --> Views
  Docs --> Custom
  Custom --> Actions
  Actions --> Policy
  Policy --> Work
  Docs --> Work
  Org --> Work
  Work --> Approval
  Work --> Exec
  Approval --> Exec
  Exec --> Mission
  Exec --> Team
  Exec --> Workflow
  Exec --> Direct
  Mission -.->|relation + plan context| Team
  Mission -.->|plan context| Workflow
  Mission -.->|plan context| Direct
  Team --> Supervisor
  Team --> TeamWork
  TeamWork --> Supervisor
  TeamWork -.->|shared WorkCore| Work
  Supervisor --> Runtime
  Workflow --> Runtime
  Direct --> Runtime
  Runtime --> Result
  Result --> Work
  Work --> Docs
  Result --> Finance
  Finance --> Docs
  Gov --> Docs
  Gov --> Org
  Gov --> Work
  Gov --> Finance
  Gov --> Approval
```

## Layer responsibilities

| Layer | Owns | Does not own |
| --- | --- | --- |
| Docs and Modules | business structure, content, record types, relations, views, templates | provider execution lifecycle |
| Organization | Actor identity, Human Owner → Lead → four Governance Agents, Org/HR → Business Agent hierarchy, role, authority, permissions, availability, capacity | one TeamRun attempt or work-routing inference |
| Collaboration | cross-actor messages, interaction routing, artifacts, explicit outcomes, and provider-native session links | Work ownership, approval, finance truth, copied provider transcripts, or raw thinking |
| Agent Team Works | TeamRun-scoped Work ownership, assigned/unassigned readiness, atomic claim, review, child delegation, and Kanban projection | authored conversation, company approval/finance, or provider transcript |
| Company Work and Approval | WorkCore extension with Milestones, WorkItem responsibility, source/result provenance, policy gates, and execution reference | provider runtime or a second Agent Team scheduler |
| Finance and Metrics | typed values, observations, audit, business relations | copied document display values |
| Execution | Mission context/Host-plan Waves, independent or Mission-scoped Agent Teams, durable Team Supervisors, typed mail, Workflow, direct delivery | company organization or document truth; Wave runtime containment |
| Runtime | provider processes, native sessions, native activity readers/resume, plugins, MCP, and ephemeral projections | business approval, assignment inference, or a second provider history |

For persistent Agent Team members, Work ownership and continuous native
execution are separate. Harness owns Work, WorkEvent, WorkDelivery, and the
conversation mailbox; one
current `TeamSupervisorLease` generation owns delivery claims and live controls,
while one selected execution driver owns provider cycles for a
MemberRun/native session/writable Workspace. A provider receipt proves
transport acceptance, not semantic completion. Provider Goal satisfaction
never implies Host acceptance. See
[Member Continuation Model](member-continuation-model.md) and
[ADR 0041](../../decisions/0041-provider-neutral-member-continuation.md), plus
[ADR 0044](../../decisions/0044-durable-team-supervision-and-typed-mail.md) for
cross-process ownership and typed-mail guarantees, and
[ADR 0050](../../decisions/0050-agent-team-work-board-and-message-boundary.md) for the
Work/Message boundary.

## Source-of-truth rule

Documents compose views of typed records. A value shared by two modules is one
record linked by `Relation`, not duplicated document content. Provider-native
execution remains in its native session. Only explicit outcomes, artifact/check
references, metrics, decisions, or linked record updates are promoted into
Harness/Company OS truth.

## Document runtime rule

Basic Documents, standard Views, and registered Custom Pages all render the
same canonical records. Custom HTML/React receives scoped Queries and named
Action Commands only; it cannot directly mutate company truth or bypass Policy,
Approval, and Audit. Every Custom Page has a standard Document/View fallback.

The obsolete coordination stack is retired by ADR 0028. ADR 0026 continues to
define Mission/Wave execution, while ADR 0029 defines the programmable document
runtime.

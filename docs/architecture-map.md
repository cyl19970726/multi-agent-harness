# Architecture Map

This is the canonical product-level architecture map. Detailed object contracts
live under [company-os](company-os/README.md). Implemented execution details
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
  Collab["Collaboration spine\nconversation · activity · handoff · artifacts"]
  Work["Work\nMilestones · WorkItems · Assignments"]
  Approval["Approvals and Needs You"]
  Gov["Governance Agents\nDocs · Work · Finance · Org / HR"]
  Finance["Finance and Metrics"]
  Exec["Execution selection"]
  Mission["Mission / ordered Waves"]
  Team["AgentTeamRun / MemberRun"]
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
  Exec --> Workflow
  Exec --> Direct
  Mission --> Team
  Mission --> Workflow
  Mission --> Direct
  Team --> Runtime
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
| Collaboration | assignments, cross-actor messages, interaction routing, handoff, artifacts, explicit outcomes, and provider-native session links | responsibility, approval, finance truth, copied provider transcripts, or raw thinking |
| Work and Approval | Milestones, WorkItem responsibility, source/result provenance, policy gates, execution reference | Project hierarchy or executor-internal planning |
| Finance and Metrics | typed values, observations, audit, business relations | copied document display values |
| Execution | Mission/Wave, Agent Team, Workflow, direct delivery | company organization or document truth |
| Runtime | provider processes, native sessions, native activity readers/resume, plugins, MCP, and ephemeral projections | business approval, assignment inference, or a second provider history |

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

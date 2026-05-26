# Design Basis

This document records the design thinking behind Multi-Agent Harness. It sits
between the product requirements and the implementation architecture: PRD says
why the product exists, architecture says what modules exist, and this document
explains why the system is decomposed this way.

## Core Thesis

Raw agents can use project tools, but they often lack durable context,
structured feedback, and a clear way to judge whether work is good. Multi-Agent
Harness turns a project's tools into an agent-governed workflow:

```text
project capability -> adapter -> agent task -> evidence -> decision
```

The product is not a domain engine. It is the governance and coordination layer
that lets agents use domain engines with less guessing and better accountability.

## Design Layers

```mermaid
flowchart TD
  Intent[Intent Layer: user goals and product scenarios]
  Coordination[Coordination Layer: AgentMember, Task, Message]
  Adapter[Adapter Layer: skills and tool descriptors]
  Execution[Execution Layer: project CLI / API / dashboard / artifacts]
  Evidence[Evidence Layer: Evidence, Report, Decision]
  Governance[Governance Layer: permissions, CI/CD, release gates]
  Interface[Interface Layer: Agent Dashboard and human review]

  Intent --> Coordination
  Coordination --> Adapter
  Adapter --> Execution
  Execution --> Evidence
  Evidence --> Governance
  Evidence --> Interface
  Governance --> Coordination
  Interface --> Intent
```

This diagram answers: how intent becomes work, how work touches project tools,
how results become evidence, and how evidence feeds governance and future work.

| Layer | Design basis | Must preserve |
| --- | --- | --- |
| Intent | Start from user goals and project scenarios, not from generic automation. | Every workflow needs a visible goal and acceptance criteria. |
| Coordination | Multi-agent work needs durable tasks and messages, not only provider chat. | A task must be assignable, reportable, reviewable, and auditable. |
| Adapter | The generic harness should not know project internals. | Domain tools enter through skills, descriptors, APIs, and artifacts. |
| Execution | Real value comes from using the project's actual capabilities. | Commands and dashboards must produce evidence, not just text. |
| Evidence | Decisions should be based on references that can be inspected later. | Important claims need evidence refs. |
| Governance | Repeated mistakes should become checks, not memories. | CI/CD gates validate project promises and permission boundaries. |
| Interface | Humans and agents need a shared operational view. | Agent Dashboard is separate from the project dashboard but links to it. |

## Module Culture

Each core module has a design culture. This is the part of the architecture that
should survive implementation changes.

| Module | Owns | Refuses | Invariant |
| --- | --- | --- | --- |
| Agent Runtime | Agent members, capabilities, status | Domain-specific business logic | Agents are manageable instances, not anonymous chat sessions. |
| Task System | Decomposition, ownership, assignment, acceptance | Vague TODOs without an owner or acceptance | Work can be tracked from request to decision. |
| Message System | Agent-to-agent communication and reports | Hidden side channels as the only source of truth | Task assignment and reports can be reconstructed. |
| Evidence System | References to proof | Treating unsupported summaries as facts | Decisions point to inspectable evidence. |
| Decision System | Accept, reject, retry, block, escalate | Silent implicit conclusions | The leader's rationale is durable. |
| Skill System | How agents should use a scenario or tool | Copying project business logic into generic core | Repeated working knowledge becomes reusable. |
| Tool Adapter System | CLI/API/dashboard/artifact access | Direct coupling to a specific project runtime | Project capability has a stable contract. |
| Agent Dashboard | Operational read model for tasks and evidence | Replacing the project dashboard | It shows coordination state and links to domain evidence. |
| CI/CD | Validation of promises and release readiness | Running checks that do not map to real commitments | Broken contracts fail early. |

## Documentation Mapping

The documentation structure should mirror the system thinking:

| Doc | Design role |
| --- | --- |
| `README.md` | Entry point, product boundary, and fastest route to useful context. |
| `docs/prd.md` | Motivation, scenarios, non-goals, and success criteria. |
| `docs/design-basis.md` | Layering, module culture, and the reasoning that connects product to architecture. |
| `docs/architecture.md` | Concrete modules, data flow, object contracts, package boundaries. |
| `docs/operations.md` | How the system is run, checked, released, and recovered. |
| `docs/schemas.md` | Machine-readable contracts that stabilize the workflow. |
| `docs/decisions.md` | Durable tradeoffs that future agents should not re-litigate casually. |
| `skills/` | Operational knowledge that tells agents how to use the project and improve it. |

If a document does not map to a design role, it should not exist yet.

## Design Review Questions

Use these questions before adding large docs, new directories, or new modules:

1. What important design idea is this documenting?
2. Which layer or module culture does it clarify?
3. Does it explain why the decomposition exists, or only list what exists?
4. Can a new agent use it to make a better decision with less context?
5. Does it connect to evidence, CI/CD, schema, CLI, or dashboard surfaces?
6. If the content is stable, should it become a machine contract instead of more prose?

Good documentation should make the system feel smaller because the structure is
clear. Bad documentation makes the system feel larger because it adds text
without revealing the design.

# Company OS

```text
status: canonical product entry
owner_role: product
canonical_for: AI Company OS product boundary and document map
```

Star Harness is becoming an **AI Company Operating System**. Its two product
cores are:

1. **Docs** — the company memory, business structure, decision surface, and
   default place where work begins and returns.
2. **A mixed organization** — durable Standing Agents, human members, and
   limited external participants arranged into accountable teams and, when
   needed, nested organizational units.

This is not a rename of the execution harness. Mission/Wave, Dynamic Workflow,
Agent Team, provider sessions, plugins, and host execution remain the execution
substrate used by the organization. They do not replace the document system or
become the company’s primary information architecture.

## Canonical loop

```text
Document / business record
  -> WorkItem and, when required, Approval
  -> accountable Actors choose or perform execution
  -> outcome, artifacts, evidence, and metrics
  -> update the originating document and related records
  -> improve the document architecture and organization
```

A WorkItem may be performed by a human, a Standing Agent, an external
participant, or an execution substrate such as a Mission/Wave, Agent Team, or
Dynamic Workflow. The execution reference is proof of how work ran; it is not a
substitute for responsibility, approval, or the business context held in Docs.

Before claiming any part of this loop is implemented, read the
[implementation truth matrix](implementation-truth-matrix.md). It maps Docs,
Organization, Work and Finance from contract through acceptance and names the
remaining native gaps in the trademark scenario.

For a visual, navigable overview of how the core pages, business lines, truth
systems, and governed handoffs fit together, open the
[Company OS Live PRD](live-prd.html). Its Expected designs, browser-rendered
Actual evidence links, and review contract are indexed under
[`docs/design/company-os-v3/live-prd-v1`](../design/company-os-v3/live-prd-v1/README.md);
the source Actual comparison plates remain with their owning acceptance slice.

## Knowledge boundary

Company knowledge is deliberate and inspectable: documents, typed business
records, decisions, approvals, final outputs, evidence, and meaningful metrics.
Ordinary chat is activity, not an assignment or authoritative company memory.
Raw provider transcripts and private model thinking are never company knowledge
truth: thinking stays transient, sanitized, and non-replayable.

## Retirement boundary

The superseded coordination stack is leaving active product context and code
under ADR 0028. Historical ledgers are exported and verified before deletion;
they are not projected into Company OS records or retained as a second live
model.

## Default context

Start with [Product system map](product-system-map.md). Then read only the
contract for the system being changed. Repository-wide placement and lifecycle
rules live in [Documentation Governance](../documentation-governance.md).

## Product authority

| Scope | Canonical contract |
| --- | --- |
| Product thesis and whole-system orientation | [Vision](vision.md), [Product system map](product-system-map.md), [Concept model](concept-model.md) |
| Docs and business modules | [Document system](document-system.md), [Module design](module-design.md) |
| Organization and collaboration | [Organization and actors](organization-and-actors.md), [Collaboration and Agent work](collaboration-and-agent-work.md) |
| Work and Approval | [WorkItems and approvals](work-items-and-approvals.md), [Work Operating System](work-operating-system.md) |
| Finance | [Financial relations](financial-relations.md) |
| Cross-system ownership | [Four-system collaboration](four-system-collaboration.md) |
| Governance and internal management | [Governance](governance.md), [Governance Agent workspaces](governance-agent-workspaces.md) |
| Execution boundary | [Execution foundation](execution-foundation.md) |
| Product experience | [Frontend information architecture](frontend-information-architecture.md) |

## Supporting references

- [Agent-programmable pages](agent-programmable-pages.md) and
  [Skill contracts](skill-contracts.md): planned governed capabilities, not
  product authority or implementation claims.
- [Browser Action transport](browser-action-transport.md) and
  [WorkItem lifecycle actions](work-item-lifecycle-actions.md): implemented
  technical slices.
- [Core page matrix](core-page-matrix.md) and
  [Company OS V2 visual inventory](../design/company-os-v2/visual-index.md):
  page/design scope and visual evidence.
- [Trademark registration example](examples/trademark-registration.md): first
  cross-system acceptance scenario.

Historical implementation plans and completion audits are available through Git
history and the native Mission/Wave records that executed them. They are not
maintained as a second documentation layer.

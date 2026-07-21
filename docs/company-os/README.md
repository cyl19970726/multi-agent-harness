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

## Reading map

- [Implementation waves](implementation-waves.md): the seven delivery Waves,
  their dependency order, evidence, and completion gates.
- [Completion audit](completion-audit.md): requirement-by-requirement evidence
  and the remaining Human-only visual sign-off boundary.
- [Store-live V2.2 gap audit](store-live-v2.2-gap-audit.md): the implemented
  backend boundary, the still-disabled interactive product paths, model gaps,
  and the ordered work after the V2.2 visual merge.
- [Vision](vision.md): the product thesis, operating loop, and system boundary.
- [Concept model](concept-model.md): object boundaries and source-of-truth rules.
- [Document system](document-system.md) and [module design](module-design.md):
  Notion-like composition, typed records, relations, views, and business-domain
  growth.
- [Agent-programmable pages](agent-programmable-pages.md): basic documents,
  structured views, governed custom code, scoped Actions, and fallback.
- [Browser Action transport](browser-action-transport.md): the implemented
  `approval.decide` browser slice, session capability, evidence, and the honest
  boundary before actor-bound Human authentication.
- [Skill contracts](skill-contracts.md): the optional module-designer and
  page-builder capabilities.
- [Organization and actors](organization-and-actors.md): human, Standing Agent,
  external, service, and OrgUnit lifecycles.
- [Collaboration and Agent work](collaboration-and-agent-work.md): Lead/direct-report
  collaboration, subject-linked conversation, shared UI primitives, and the
  boundary between Standing Agents and temporary execution members.
- [WorkItems and approvals](work-items-and-approvals.md): responsibility,
  submission provenance, execution references, review, and human gates.
- [Financial relations](financial-relations.md): budget, commitment, invoice,
  payment, refund, metrics, and cross-module linkage.
- [Governance](governance.md): document architecture, organization evolution,
  permissions, risk, and approval authority.
- [Execution foundation](execution-foundation.md): Mission/Wave, Agent Team,
  Dynamic Workflow, Host execution, providers, plugins, and MCP.
- [Frontend information architecture](frontend-information-architecture.md)
  and [core page matrix](core-page-matrix.md): navigation, page responsibilities,
  responsive behavior, and truth constraints.
- [Trademark registration example](examples/trademark-registration.md): the
  first cross-module acceptance scenario.
- [Document migration map](document-migration.md): canonical, compatibility,
  historical, and superseded documentation.

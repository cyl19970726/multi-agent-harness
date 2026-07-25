# Company OS Core Page Matrix

```text
status: canonical design-generation matrix
owner_role: product-design
canonical_for: Company OS core page coverage and visual-contract scope
```

The current visual source of truth is
[`docs/design/company-os-v2/visual-contract.json`](../design/company-os-v2/visual-contract.json),
with the Live PRD journey under
[`docs/design/company-os-v3/live-prd-v1/`](../design/company-os-v3/live-prd-v1/).
The retained `company-os-v1/actual` screenshots are legacy baselines only; they
are not active design intent. This document explains page responsibility
without maintaining a second progress count.

## Core pages

| # | Page | Primary question | Required truth |
| --- | --- | --- | --- |
| 1 | Company Home | What needs my decision today, and is the company healthy? | Approvals, WorkItems, modules, metrics, finance, and organization pulse all link to their sources. |
| 2 | Docs Workspace | Where does company knowledge and business structure live? | Spaces, basic documents, templates, structured Views, module proposals, and structure health. |
| 3 | Document Focus | What is true here, what changes next, and where will results return? | Rich Blocks, actors, WorkItems, Approvals, metrics, finance, relations, and activity. |
| 4 | Workboard | Who submitted, owns, executes, reviews, and approves company work? | Explicit responsibility and source-document provenance; never chat-as-work. |
| 5 | WorkItem Focus | What is the complete accountable chain and execution/result state? | Requester, submitter, owner, contributors, reviewer, approver, source/result, execution, evidence, finance. |
| 6 | Finance | Where is money planned, committed, invoiced, paid, or refunded, and why? | Typed FinancialRecords, business origin, permissions, approval, evidence, and audit. |
| 7 | Organization | Which humans, Standing Agents, external contributors, and services make up the company? | Typed actors, OrgUnits, roles, authority, capacity, gaps, and governance proposals. |
| 8 | Standing Agent Focus | What does this durable Agent own, can it safely accept work, and what has it delivered? | Explicit availability/capacity, maintained Docs, assignments, capabilities, permissions, runtime, sessions. |
| 9 | Governance Proposal | Where should a new business domain live and what changes does it require? | Module structure, relations, actors, permissions, finance, migration, impact, and human gate. |
| 10 | Approval Focus | What exactly am I authorizing, based on which evidence and policy? | Human identity, consequences, finance/legal effects, checks, evidence, immutable history. |
| 11 | Business Module Focus | How does one domain compose knowledge, records, work, money, actors, and governance? | Module, Documents, Views, TypedRecords, Relations, WorkItems, Approvals, finance, people, decisions. |
| 12 | Human Member Focus | What does this person own, decide, and participate in? | Org membership, authority, responsible Docs, WorkItems, Approvals, decisions, activity; no provider/runtime fiction. |

The twelve-page matrix remains the broad visual coverage contract. It is not
the current implementation order. Near-term Company OS work prioritizes
Organization Overview and the multi-view Work Operating System. New rich
Standing/Governance Agent detail workspaces are deferred; the product may show
their responsibility, prompt, tools/Skills, permissions, maintained Docs, and
WorkItems in a compact profile or Context Rail first.

Docs also has one implementation-driven governance subpage:
`?surface=docs&health=structure` (**Document Health Review**). It is not a
thirteenth broad concept-design page; it is the operational drill-in behind
the Docs Workspace "Structure health" rail. Its job is to make document
governance auditable: counts, structural findings, affected durable records,
recommended governed actions, CLI/Skill command hints, and, when a Store-live
Action declaration is present, corrective WorkItem creation. For the narrow
missing Document ↔ TypedRecord Relation case, a scoped `relation.append`
declaration can also let the page execute the direct repair through the
standard Action dispatcher. It must not imply that cleanup has run until a Docs
Action or corrective WorkItem proves it; a created WorkItem is routing truth,
not the repair itself.

## Shared layout and navigation

```text
PRIMARY       Home / Docs / Organization
OPERATIONS    Work / Approvals / Finance
EXECUTION     Missions / Workflows / Agent Teams
PLATFORM      Providers / Plugins / Settings
```

All pages share the warm-white, fine-border, coral-accent visual language. They
share components but not object semantics: Human, Standing Agent, External,
MemberRun, and provider session remain distinct lifecycles.

## Truth and safety requirements

- Every displayed amount, metric, actor, assignment, approval, and source is
  backed by an explicit canonical record.
- A custom page is a registered view over scoped Queries and named Action
  Commands; it cannot directly write business truth.
- Basic Documents and standard Views remain available when custom code fails.
- Human-required actions cannot be approved by an Agent.
- Raw provider transcripts and private thinking never become document history.
- Execution pages link back to their WorkItem and source/result Documents.

## Retained execution drill-ins

Mission/Wave Canvas, Agent Team War Room, MemberRun Focus, WorkflowRun Focus,
Providers, and Plugins remain execution or platform pages. They are not part of
the twelve Company OS expected designs because their separate Workbench visual
contract already exists. Their Company OS adaptation is a compact source panel
linking back to WorkItem, Document, accountable Actor, Approval, and result.

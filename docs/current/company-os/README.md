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
2. **A recursive mixed organization** — humans, durable `AgentMember`
   identities, external participants, and services. Agent hierarchy is the
   topology of persistent, nested `AgentTeam`s: a Member can remain a member of
   its parent Team while hosting a child Team of its own.

This is not a rename of the execution harness. Mission/Wave, Dynamic Workflow,
Agent Team, provider sessions, plugins, and host execution remain the execution
substrate used by the organization. They do not replace the document system or
become the company’s primary information architecture.

Docs are **Agent-operated and Human-reviewed**. Agents primarily use CLI/API
and skills to read, edit, govern, and verify company memory. Humans primarily
use the UI to inspect, review, supervise, and occasionally trigger safe
low-risk actions. The UI should make structure, relations, state, and risk
clear to people; the authoritative machine interface is CLI/API.

The installable Company OS skill suite is indexed in
[Skill and CLI Contracts](skill-contracts.md). Start with
`company-business-project-bootstrap` when turning a real commercial project
into Company OS, then use the dedicated Docs, Work, Org, Finance, module-design,
and page-building skills for each owned surface. Use:

```bash
scripts/install-skill.sh --agent both --suite company-os
```

Dedicated Docs, Work, Organization, and Approval baseline CLI commands
are implemented. Finance baseline CLI commands are parked at the contract layer
(see issue #323); Commitment/Payment code remains dormant.
`firm company org ...` surface and nested `actor/unit/membership` groups for
inspection and **bootstrap-only** Human administrative authoring of Humans,
Standing Agents, OrgUnits, Memberships, declared actor status, and
permission/capability refs. That boundary is not target delegated authority and
cannot substitute for a governed Approval, scoped grant, or broker dispatch. It
does not yet implement the flat AgentTeam organization in
Nested Agent Team organization. Those
records remain current implementation truth during an explicit cutover; they
must not be presented as the target identity model.

Finance CLI v1 is parked at the contract layer (2026-08-05, issue #323).
The flat `firm company finance ...` surface and nested `commitment/payment`
commands remain implemented but are no longer part of the active operator suite.
Commitment/Payment code stays dormant until full decommission; the smoke script
is preserved as historical evidence.

Current Company OS storage remains append-only JSONL ledgers plus latest
projections. SQL is planned as a derived read/query/index layer for Docs query,
search, Views, health, diff, and export; it is not the current canonical write
Store. See [ADR 0035](../../decisions/0035-company-os-sql-read-model.md).

The current implementation now has the first explicit Company Store slice:
`firm company init/list/current/show/switch/migrate-from-project`,
`--company <id>`, and `HARNESS_COMPANY` route `firm company ...` commands to
`<HARNESS_HOME>/companies/<id>/`. If no Company is selected, the older
project-derived Company OS compatibility path still works. The migration command
copies only `company_os_*.jsonl` ledgers; it does not move Mission/Wave, Agent
Team, Workflow, provider sessions, prompts, or runtimes. ADR 0042 defines the
implemented identity split: Company Store owns Agent Company Workspace truth,
Execution Space owns Mission/Wave, Agent Team, Workflow, and Host coordination,
and Project Binding owns repo / worktree / provider-cwd selection. Company is
optional for execution.

## Canonical operating loop

```text
Document / business record
  -> Work and, when required, Approval
  -> accountable Members or Humans choose or perform execution
  -> outcome, artifacts, evidence, and metrics
  -> update the originating document and related records
  -> improve the document architecture and organization
```

`Work` is the shared responsibility kernel. It may be owned by an AgentMember,
performed by a human or external participant, or linked to an execution
substrate such as Mission/Wave, Agent Team, or Dynamic Workflow. Existing
Company `WorkItem` rows are compatibility implementation truth until the
explicit Work-kernel migration; new architecture must not create a second
independent responsibility model.

For AgentOS itself, this is a continuous self-hosting loop rather than one
mandatory sequence. Docs, Work, and Organization may each reveal the next gap
and may each receive an accepted result. The current Codex task can act as a
Supervising Operator, while the Company-owned AgentOS Lead remains an
independent durable AgentMember and root Team Host. See the
AgentOS self-hosting dogfood loop and
[ADR 0052](../../decisions/0052-nested-agent-teams-are-the-agent-organization.md).

The current product slice prioritizes the Docs + Work + Organization loop.
Agents may discover gaps in documents, code, external gateway events, or Work
views; create unassigned or self-owned Work; and return accepted outcomes to
Docs. A Team Host assigns Work inside its direct Team. A Member may host a child
Team and delegate its owned Work downward while remaining accountable upward.
Finance is conditional and enters only when Work requests a monetary effect.

Before claiming any part of this loop is implemented, read the
implementation truth matrix. It maps Docs,
Organization, Work and Finance from contract through acceptance and names the
remaining native gaps in the trademark scenario.

For a visual, navigable overview of how the core pages, business lines, truth
systems, and governed handoffs fit together, open the
Company OS Live PRD. Its Expected designs, browser-rendered
Actual evidence links, and review contract are indexed under
`docs/archive/design/company-os-v3/live-prd-v1`;
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
| Product thesis and whole-system orientation | Vision, [Product system map](product-system-map.md), Concept model |
| Docs and business modules | [Document system](document-system.md), Docs operating surface matrix, [Module design](module-design.md) |
| Organization and collaboration | Nested Agent Team organization, [Organization and actors](organization-and-actors.md), Collaboration and Agent work, [ADR 0052](../../decisions/0052-nested-agent-teams-are-the-agent-organization.md) |
| AgentOS self-hosting | AgentOS self-hosting dogfood loop, [ADR 0046](../../decisions/0046-supervised-agentos-self-hosting-loop.md) |
| Work and Approval | [WorkItems and approvals](work-items-and-approvals.md), [Work Operating System](work-operating-system.md) |
| Finance | Financial relations |
| Cross-system ownership | Four-system collaboration |
| Governance and internal management | [Governance](governance.md), Governance Agent workspaces |
| Company authority root and delegation | Human-rooted Company Constitution, [ADR 0048](../../decisions/0048-human-rooted-company-constitution.md) |
| Scoped Company authority brokerage | Scoped Company Authority Broker, [ADR 0047](../../decisions/0047-scoped-company-authority-broker.md) |
| Company / execution / project identity | [ADR 0042](../../decisions/0042-company-store-execution-space-project-binding.md), [Execution foundation](execution-foundation.md) |
| Execution boundary | [Execution foundation](execution-foundation.md) |
| External gateways and plugins | External Gateway and Plugin Intake |
| External software projects and GitHub PRD mapping | External project product sources |
| Product experience | Frontend information architecture |
| Store/read model direction | [ADR 0035](../../decisions/0035-company-os-sql-read-model.md) |

## Supporting references

- Agent-programmable pages and
  [Skill contracts](skill-contracts.md): CLI-backed Docs Governance primitives
  plus optional governed Docs/module/page skills; none is product authority by
  itself.
- Docs operating surface matrix: current
  Docs UI, CLI/skill, visual, Store-live evidence, and planned gaps.
- AI-first Docs spec: proposed v2 target for the Docs
  module (closed block set, Markdown-first serialization, whole-page
  revisions, content-addressed blobs); design intent accepted by
  [ADR 0054](../../decisions/0054-ai-first-docs-page-model-and-storage.md), not
  yet implementation authority.
- Browser Action transport and
  WorkItem lifecycle actions: implemented
  technical slices.
- [Core page matrix](core-page-matrix.md) and
  Company OS V2 visual inventory:
  page/design scope and visual evidence.
- Trademark registration example: first
  cross-system acceptance scenario.
- Wanchengwanling AR tourism dogfood project:
  first real commercial Company OS dogfood project. Its active local operating
  truth lives in Company Store `agent-company` alongside AgentOS dogfood
  records. GitHub-hosted software PRDs, repo docs, generated reports, and
  historical bootstrap scripts are source observations, design references, or
  acceptance evidence, not the commercial operating database.
- Wanchengwanling Company Store migration:
  local verification that Wanchengwanling Company OS rows were copied into
  `agent-company` without copying execution ledgers.
- Wanchengwanling completion roadmap:
  storage-backed unfinished-goal map for CLI/API, skills, custom pages, GitHub
  source sync, SQL read/search, real launch data, and replication templates.
- [Agent Team foundation closure plan](../product/agent-team-foundation-closure-plan.md):
  staged implementation and acceptance boundary for the durable Supervisor,
  typed mailbox, multi-client controls, and Organization runtime substrate.

Historical implementation plans and completion audits are available through Git
history and the native Mission/Wave records that executed them. They are not
maintained as a second documentation layer.

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

Dedicated Docs, Work, Organization, Approval, and Finance baseline CLI commands
are implemented. Organization CLI v1 covers both the flat
`harness company org ...` surface and nested `actor/unit/membership` groups for
inspection and Human administrative authoring of Humans, Standing Agents,
OrgUnits, Memberships, declared actor status, and permission/capability refs. It
does not yet provide the governed OrgChangeProposal lifecycle for adding actors
or expanding authority.

Finance CLI v1 intentionally preserves the current Store/API governance
boundary. The flat `harness company finance ...` surface keeps the existing
Human administrative initial proposed-Commitment import path, while approval
requests, approval decisions, Commitment transitions, and Payment
records/transitions use the governed Action dispatcher. The nested
`commitment/payment` surface adds the baseline Action-backed operator shape used
by the broader Company OS operator smoke. Budget, invoice, refund, reporting,
and deeper settlement lifecycle remain gated until backed by commands and
acceptance checks.

Current Company OS storage remains append-only JSONL ledgers plus latest
projections. SQL is planned as a derived read/query/index layer for Docs query,
search, Views, health, diff, and export; it is not the current canonical write
Store. See [ADR 0035](../decisions/0035-company-os-sql-read-model.md).

The current implementation now has the first explicit Company Store slice:
`harness company init/list/current/show/switch/migrate-from-project`,
`--company <id>`, and `HARNESS_COMPANY` route `harness company ...` commands to
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

For AgentOS itself, this is a continuous self-hosting loop rather than one
mandatory sequence. Docs, Work, and Organization may each reveal the next gap
and may each receive an accepted result. The current Codex task can act as a
Supervising Operator, while the Company-owned AgentOS Lead remains an
independent durable Standing Agent. See the
[AgentOS self-hosting dogfood loop](agentos-self-hosting-loop.md) and
[ADR 0046](../decisions/0046-supervised-agentos-self-hosting-loop.md).

The current product slice prioritizes the Docs + WorkItem + Organization loop.
Agents may discover gaps in documents, code, external gateway events, or Work
views; create or route WorkItems; assign them to existing Organization Actors;
and return accepted outcomes to Docs. Upper Standing Agents may drive lower
Standing Agents through explicit organization and WorkItem records. Creating a
new durable Agent remains an Org/HR-governed capability change, not a runtime
side effect. Finance is conditional and enters only when a WorkItem requests a
monetary effect.

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
| Docs and business modules | [Document system](document-system.md), [Docs operating surface matrix](docs-operating-surface-matrix.md), [Module design](module-design.md) |
| Organization and collaboration | [Organization and actors](organization-and-actors.md), [Collaboration and Agent work](collaboration-and-agent-work.md) |
| AgentOS self-hosting | [AgentOS self-hosting dogfood loop](agentos-self-hosting-loop.md), [ADR 0046](../decisions/0046-supervised-agentos-self-hosting-loop.md) |
| Work and Approval | [WorkItems and approvals](work-items-and-approvals.md), [Work Operating System](work-operating-system.md) |
| Finance | [Financial relations](financial-relations.md) |
| Cross-system ownership | [Four-system collaboration](four-system-collaboration.md) |
| Governance and internal management | [Governance](governance.md), [Governance Agent workspaces](governance-agent-workspaces.md) |
| Company / execution / project identity | [ADR 0042](../decisions/0042-company-store-execution-space-project-binding.md), [Execution foundation](execution-foundation.md) |
| Execution boundary | [Execution foundation](execution-foundation.md) |
| External gateways and plugins | [External Gateway and Plugin Intake](external-gateway-and-plugins.md) |
| External software projects and GitHub PRD mapping | [External project product sources](external-project-product-sources.md) |
| Product experience | [Frontend information architecture](frontend-information-architecture.md) |
| Store/read model direction | [ADR 0035](../decisions/0035-company-os-sql-read-model.md) |

## Supporting references

- [Agent-programmable pages](agent-programmable-pages.md) and
  [Skill contracts](skill-contracts.md): CLI-backed Docs Governance primitives
  plus optional governed Docs/module/page skills; none is product authority by
  itself.
- [Docs operating surface matrix](docs-operating-surface-matrix.md): current
  Docs UI, CLI/skill, visual, Store-live evidence, and planned gaps.
- [Browser Action transport](browser-action-transport.md) and
  [WorkItem lifecycle actions](work-item-lifecycle-actions.md): implemented
  technical slices.
- [Core page matrix](core-page-matrix.md) and
  [Company OS V2 visual inventory](../design/company-os-v2/visual-index.md):
  page/design scope and visual evidence.
- [Trademark registration example](examples/trademark-registration.md): first
  cross-system acceptance scenario.
- [Wanchengwanling AR tourism dogfood project](examples/wanchengwanling-operations.md):
  first real commercial Company OS dogfood project. Its active local operating
  truth lives in Company Store `agent-company` alongside AgentOS dogfood
  records. GitHub-hosted software PRDs, repo docs, generated reports, and
  historical bootstrap scripts are source observations, design references, or
  acceptance evidence, not the commercial operating database.
- [Wanchengwanling Company Store migration](examples/wanchengwanling-company-store-migration.md):
  local verification that Wanchengwanling Company OS rows were copied into
  `agent-company` without copying execution ledgers.
- [Wanchengwanling completion roadmap](examples/wanchengwanling-completion-roadmap.md):
  storage-backed unfinished-goal map for CLI/API, skills, custom pages, GitHub
  source sync, SQL read/search, real launch data, and replication templates.
- [Agent Team foundation closure plan](../product/agent-team-foundation-closure-plan.md):
  staged implementation and acceptance boundary for the durable Supervisor,
  typed mailbox, multi-client controls, and Organization runtime substrate.

Historical implementation plans and completion audits are available through Git
history and the native Mission/Wave records that executed them. They are not
maintained as a second documentation layer.

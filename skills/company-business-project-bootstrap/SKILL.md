---
name: company-business-project-bootstrap
description: Bootstrap or reorganize a real commercial/business project inside Company OS. Use when a user wants to turn a venture, city rollout, merchant network, procurement plan, content operation, software PRD repository, or similar business into a governed Company OS setup spanning Docs, Work records, Organization, Finance, external product sources, and optional custom pages.
---

# Company Business Project Bootstrap

Use this skill to turn a real business project into an Agent-operable Company
OS workspace. This skill is a procedural capability, not product authority.

It does not replace the module operators:

- `$dogfood-company-os` owns repeated self-hosting cycles after bootstrap.
- `$company-docs-operator` owns durable company memory.
- `$company-work-operator` reads the Company aggregate, routes native TeamWork
  lifecycle commands, and owns Milestone references and result provenance.
- `$company-org-operator` owns humans, Standing Agents, org units, roles,
  permissions, and capability lifecycle.
- `$company-finance-operator` owns budgets, commitments, invoices, payments,
  refunds, and monetary evidence.
- `$company-module-designer` designs a governed business module before
  implementation.
- `$company-page-builder` builds code-declared custom pages from approved
  module/page contracts.
- `$connect-github-company-os` owns GitHub source/delivery mapping and
  connector boundaries.

The normal progression is:

```text
business thesis
  -> DocumentSpace + page information architecture
  -> page_contract records in Store
  -> module records, Relations, Views
  -> Work / Org / Finance operating records
  -> custom page visual contract and implementation when needed
```

Repository markdown may document the design, but the live business system is
the project Store. Do not stop at repo docs when the user expects Agents and
UI to operate the project.

## Load contracts first

Read these before changing durable records or writing a project plan:

- `docs/current/company-os/README.md`
- `docs/current/company-os/document-system.md`
- `docs/current/company-os/work-items-and-approvals.md`
- `docs/current/company-os/organization-and-actors.md`
- `docs/current/company-os/financial-relations.md`
- `docs/current/company-os/module-design.md`
- `docs/current/company-os/skill-contracts.md`
- `docs/current/company-os/implementation-truth-matrix.md`

For page architecture and Docs Store authoring, also use:

- `$company-docs-operator` and its `references/page-contract.md`;
- `$company-docs-operator` and its `references/business-page-archetypes.md`;
- `$company-docs-operator` and its `references/store-authoring-patterns.md`.

If the project has software source truth in GitHub or another repo, also read:

- `docs/current/company-os/external-project-product-sources.md`
- `$connect-github-company-os` for Issue/PR/check/source correlation.

If building custom pages, also read:

- `docs/current/company-os/agent-programmable-pages.md`
- `docs/current/company-os/frontend-information-architecture.md`

Repository docs, schemas, Store/API code, and acceptance checks outrank this
skill when there is a conflict.

## Output expected from a bootstrap

Produce a concise bootstrap plan or implementation report with:

1. `DocumentSpace` and top-level document/module map.
2. Page information architecture: for every core page, the question it answers,
   required sections, standard Views, related-record panels, and navigation
   links to sibling pages.
3. Business modules and their owned facts.
4. Work classifications, Milestones, first Work records, assignment/routing policy, and
   lifecycle views.
5. Organization model: Lead Agent, governance Agents, business Agents, humans,
   external collaborators, services, roles, delegation ceilings, capacity, and
   exception-only Human gates.
6. Finance model: budgets, commitments, payments, invoices, refunds, money
   metrics, and approval gates.
7. External software product sources and sync rules, when applicable.
8. Custom pages to build, with fallback views.
9. Acceptance checks and explicit gaps marked as `implemented`, `partial`,
   `planned`, or `design-only`.

## Bootstrap workflow

### 1. Define the business root

Select the root Company Store first. A business project is normally a
DocumentSpace/module and Work grouping inside that Company, not a new
repository-derived Company and not a new generic `Project` object. Record the
Company's Human Principal / Constitution Owner, durable Company Lead, initial
Domain Lead, Supervisor transport boundary, and the policies that reserve
protected decisions for Humans.

Capture the minimum business thesis:

- What is being sold or delivered?
- Who pays, who receives value, and who operates the process?
- Which lines exist now, and which are future expansion?
- Which facts are commercial truth versus software implementation truth?
- Which actions require human approval, money approval, legal review, or
  organization change?

Do not begin with UI. Begin with owned facts and responsible actors.

### 2. Create the Docs structure

Design the `DocumentSpace` as the company memory for this project. Use Docs for
business context, operating procedures, decisions, evidence, and durable result
records.

Attach the operating area beneath the active Company hierarchy:

```text
Company Home
  -> Governance / company-wide operations
  -> Domain or business-line home
     -> project/module home
     -> current operating pages and accepted results
```

Typical module structure for a commercial launch:

```text
00 Project Home
01 Business Model
02 Product / Offer
03 Experience / Route / Delivery
04 Merchant or Partner Network
05 Rewards, Procurement & Inventory
06 Content Growth
07 Creator / Channel Outreach
08 Launch Readiness
09 IP & Product Design
10 Software Product Sources
```

Adjust names to the business, but keep each module's source-of-truth boundary
clear.

Before writing blocks, define a page contract for each important document:

| Page kind | Must answer | Typical presentation |
| --- | --- | --- |
| Project Home | What is this business, what modules exist, what is live, what is blocked, and where should a human or Agent go next? | hero thesis, operating loop, module cards, launch-state snapshot, top Work records, Finance/Approval watchlist, software-source status, right-side document tree. |
| Business Model | What is sold, who pays, who receives value, why partners join, how money flows, and how the model replicates? | revenue table, customer/merchant value blocks, partner capability matrix, cost/finance boundary, replication canvas, KPI table. |
| Product / Offer | What SKUs/rights exist and how are they sold or fulfilled? | SKU table, pricing and entitlement rules, channel/settlement rules, links to design assets and inventory. |
| Experience / Route | What experience does the user complete and what unlocks at each threshold? | route/spot table, 8/12 reward rules, AR asset readiness, validation/evidence links. |
| Merchant Network | Which merchants exist, what capabilities each has, and what status/action is next? | merchant capability matrix, contact/onboarding board, map/list view, related Work records. |
| Procurement / Inventory | What must be bought, where it is, what it costs, and what can be redeemed? | purchase orders, shipment/inventory table, redemption allocation, Finance Commitment links. |
| Growth / Outreach | What content and creator motions are running and what results came back? | campaign calendar, post/creator pipeline, metrics table, result-return links. |
| Launch Readiness | Can this project go live safely? | cross-module gates, blockers, owner, evidence, required approvals. |

Do not satisfy this step with generic prose. A usable Docs setup must let a
human understand the business from the UI and let an Agent operate it through
CLI/API without scraping pages. If a page needs data from another system, model
that data as a relation or View rather than copying the fact into text.

Define the active/archive boundary during bootstrap: current policy, live
source records, open-work context, and accepted results remain in the active
tree; superseded drafts, closed campaigns, and replaced procedures use native
archive lifecycle while retaining ids, relations, evidence, and provenance.
Do not create a second copied archive tree.

For every core page, decide whether the front-end should be:

```text
standard Document page
  left document tree + center Blocks/Views + right related context

standard Module page
  module records + saved Views + relation/health panels

custom code-declared page
  only when a core surface needs several systems in one deliberate layout
```

Record that choice in the Store page contract. Hand custom page candidates to
`$company-page-builder`; do not embed custom HTML as company truth.

### 3. Turn work into authoritative TeamWork

Every committed action becomes native TeamWork in a selected Execution Space,
not a loose note or a second Company task:

- sourcing a supplier;
- contacting a merchant;
- preparing a filing;
- producing a media asset;
- testing a route;
- syncing a software PRD;
- collecting launch evidence.

Use Team/TeamRun scope, parent Work, phase, condition, resolution, owner,
priority, evidence, and Milestone `work_refs`. Company Work is only the
cross-space read and routing aggregate. Do not create a separate `Project`
object, Company task ledger, Task Graph, GoalPhase, or old planning model.

Define the continuous operating queue at bootstrap:

- Human Principal intent and external observations enter with durable source
  provenance; the Supervisor routes them without becoming Company authority.
- Company Lead triages, deduplicates, prioritizes, checks capacity, and replans
  when facts or constraints change.
- Domain Lead creates or owns one native Work and delegates bounded execution
  through its Work owner and WorkDelivery within the Organization ceiling.
- Human queue contains only named policy gates, protected effects, authority
  expansion, unresolved ambiguity/conflict, or capacity exceptions.

### 4. Model Organization before assignment

Assign only to actors that exist in Organization:

- Human owner / approver;
- Lead Agent;
- governance Agents for Docs, Work, Org/HR, and Finance;
- business Agents such as trademark, development, merchant outreach,
  procurement, content, or creator outreach;
- external collaborators and services.

Business Agents should sit under Org/HR governance. Lead Agent manages the
governance layer. Skills are tools, never authority. Adding an Agent, role, or
permission is an organization effect and should go through an explicit
proposal/approval path when sensitive.

For each Domain Lead record responsibility, accepted Work classifications, maintained
Docs, explicit capacity, escalation policy, and permission/budget/tool ceiling.
Delegation must attenuate: a child actor receives only the subset needed by the
Work and cannot approve its own privilege increase or any protected effect.

Label the bootstrap boundary explicitly. Current Company OS can author actors,
units, memberships, permissions, and Milestones; Work mutations route only
through `team-run work` in the authoritative Execution Space. Hierarchical `ScopedPermissionGrant`
lineage, strict recursive subset checks, sibling resource reservation,
ancestor fencing, autonomous approved-template Standing Agent creation, and
their effective-authority UI are target capabilities until accepted schemas,
Actions, authenticated transport, and tests exist.

### 5. Route money through Finance

A Work may request a monetary effect, but Finance owns the money state:

- budget;
- estimate;
- commitment;
- invoice;
- payment;
- refund;
- monetary metric.

Never infer Payment from a purchase note, Work, Approval, or model answer.
Record Commitments before spend, Payments only after real payment evidence, and
link them back to the source Work and Docs record.

### 6. Sync software PRDs as external product sources

If the project has a software repo, use `$connect-github-company-os` to map
external source and delivery facts. The Docs-owned source snapshot starts with:

```bash
harness --company <company-store-id> --project <project-binding> \
  company docs source sync \
  --definition <custom-page-definition-id> \
  --module <software-product-sources-module-id> \
  --source-document <source-document-id> \
  --actor <human-or-agent-id> \
  --repo-path <local-git-worktree> \
  --repo <owner/repo> \
  --branch <branch> \
  --project-id <external-software-project-id> \
  --path <prd-or-design-path>
```

The top-level `--company` selects Company Store truth. The optional top-level
`--project` selects the Project Binding/worktree context. The command-level
`--project-id` names the external software product source.
Treat GitHub webhooks and sync runs as observations of software product truth.
They do not overwrite commercial truth, create Work records, approve finance,
change Organization, or prove delivery.

### 7. Decide custom pages only after module shape is stable

Use ordinary Docs pages, TypedRecords, Relations, and Views first. Create a
custom code-declared page only when a core page must combine several systems or
decision surfaces.

Good custom page candidates:

- Commercial Command Center;
- Business Model Canvas;
- Merchant Network Console;
- Procurement and Inventory Console;
- Launch Readiness Dashboard;
- Software Product Source Mapping;
- IP/Product Design Asset Board.

Every custom page needs:

- approved module/page purpose;
- fallback standard View;
- source records and relation boundaries;
- no direct ungoverned mutation;
- visual contract and actual screenshot when implemented.

### 8. Use scripts only as historical acceptance fixtures

A seed or materialization script may prove that the current CLI/API can create
the expected records in an isolated fixture Store. It must not become the
normal way a real commercial project is authored. For a real Company Store,
use governed CLI/API commands and module operator skills directly:

1. inspect current Store with `docs query`, `traverse`, `health`, Work, Org, and
   Finance reads;
2. write the approved page contract through governed CLI/API commands;
3. create or update typed records, relations, views, and Finance records through
   their owning module commands, and route Work mutations to `team-run work`; and
4. verify Store-live UI and CLI projections.

Do not leave project-specific seed scripts as active product entrypoints. If a
script exists, treat it as acceptance evidence or fixture generation only and
move recurring operations into CLI/API commands or a scenario-specific skill.

For current Wanchengwanling dogfood, the active local Company Store is:

```text
company_id: agent-company
store: /Users/hhh0x/.harness/companies/agent-company
root_docs_entry: document-wcw-root
project_home: document-wcw-project-home
business_model: document-wcw-business-model
agentos_dogfood_entry: document-cli-11-agentos-dogfood-external-gateway-agentos
```

Treat Wanchengwanling as the first real commercial Company OS dogfood project,
not as a sample dataset. Repo markdown, design images, generated reports, and
scripts are useful only when they point back to Store-backed Documents,
TypedRecords, Relations, Views, Work records, Actors, Finance records, source-sync
observations, and custom page definitions.

Do not use repository markdown as the operating database. The durable business
workspace must be readable from the Company OS Store; markdown docs, generated
reports, and seed scripts are references or acceptance evidence.

The historical four-system and roadmap seeds proved that the project could be
represented as Docs, Work, Organization, Approval, and Finance records. They are
not the authoring path for new dogfood operations. New business pages,
Work records, actor changes, Finance effects, source-sync observations, and
custom-page contracts must be created through the owning governed commands.

## Wanchengwanling example mapping

Use this shape for the AR tourism MVP unless newer product truth says
otherwise:

- Offer: physical NFC bracelet ¥30, virtual bracelet ¥20.
- Physical bracelet channel: merchant consignment; merchant share ¥10,
  company share ¥20.
- Experience: 8 check-ins unlock AR magnet redemption; 12 check-ins unlock
  lottery eligibility.
- Rewards: AR magnet, future figures/derivatives, two Polaroid prizes, and
  local food coupons.
- Merchant network: bracelet sellers, magnet redemption points, prize
  redemption partners, bracelet-benefit merchants, and purchased-supply
  merchants may be separate capability tags on one merchant.
- Software source: `cyl19970726/wanchengwanling` dev branch is software PRD and
  implementation source, not the sole business operating truth.
- IP/Product Design: bracelet, magnet, main IP character, AR animation assets,
  store-facing material, and social content assets are first-class project
  records.
- Platform operations: Xiaohongshu, Douyin, WeChat Channels, WeCom, ecommerce,
  and logistics integrations should be modeled as gateway plugins. A plugin
  may provide Skill instructions, MCP tools, plugin-owned CLI adapters,
  connector sync, and view extensions, but the project truth still returns to
  Docs, Work records, Organization actors, metrics, evidence, and Finance/Approval
  records when money or protected actions are involved.

## Handoff format

When handing off, state:

- created or proposed DocumentSpace, modules, and key documents;
- created or proposed Work classifications, Milestones, and initial Work records;
- Organization actors, governance/business split, and permission gaps;
- Finance records, approvals, and money-state gaps;
- software source sync status;
- custom pages and whether they are design-only, implemented, or verified;
- commands/scripts run and acceptance results;
- stale docs or obsolete records to delete instead of preserving as active
  context.

After bootstrap, hand recurring self-improvement to `$dogfood-company-os`.
Bootstrap defines the first operating shape; it does not become a perpetual
Company supervisor.

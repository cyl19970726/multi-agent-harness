---
name: company-business-project-bootstrap
description: Bootstrap or reorganize a real commercial/business project inside Company OS. Use when a user wants to turn a venture, city rollout, merchant network, procurement plan, content operation, software PRD repository, or similar business into a governed Company OS setup spanning Docs, WorkItems, Organization, Finance, external product sources, and optional custom pages.
---

# Company Business Project Bootstrap

Use this skill to turn a real business project into an Agent-operable Company
OS workspace. This skill is a procedural capability, not product authority.

It does not replace the module operators:

- `$company-docs-operator` owns durable company memory.
- `$company-work-operator` owns WorkItems, Milestones, assignments, lifecycle,
  and result provenance.
- `$company-org-operator` owns humans, Standing Agents, org units, roles,
  permissions, and capability lifecycle.
- `$company-finance-operator` owns budgets, commitments, invoices, payments,
  refunds, and monetary evidence.
- `$company-module-designer` designs a governed business module before
  implementation.
- `$company-page-builder` builds code-declared custom pages from approved
  module/page contracts.

## Load contracts first

Read these before changing durable records or writing a project plan:

- `docs/company-os/README.md`
- `docs/company-os/document-system.md`
- `docs/company-os/work-items-and-approvals.md`
- `docs/company-os/organization-and-actors.md`
- `docs/company-os/financial-relations.md`
- `docs/company-os/module-design.md`
- `docs/company-os/skill-contracts.md`
- `docs/company-os/implementation-truth-matrix.md`

If the project has software source truth in GitHub or another repo, also read:

- `docs/company-os/external-project-product-sources.md`

If building custom pages, also read:

- `docs/company-os/agent-programmable-pages.md`
- `docs/company-os/frontend-information-architecture.md`

Repository docs, schemas, Store/API code, and acceptance checks outrank this
skill when there is a conflict.

## Output expected from a bootstrap

Produce a concise bootstrap plan or implementation report with:

1. `DocumentSpace` and top-level document/module map.
2. Business modules and their owned facts.
3. WorkTypes, Milestones, first WorkItems, assignment/routing policy, and
   lifecycle views.
4. Organization model: Lead Agent, governance Agents, business Agents, humans,
   external collaborators, services, roles, and permission boundaries.
5. Finance model: budgets, commitments, payments, invoices, refunds, money
   metrics, and approval gates.
6. External software product sources and sync rules, when applicable.
7. Custom pages to build, with fallback views.
8. Acceptance checks and explicit gaps marked as `implemented`, `partial`,
   `planned`, or `design-only`.

## Bootstrap workflow

### 1. Define the business root

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

Typical top-level structure for a commercial launch:

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

### 3. Turn work into WorkItems

Every committed action becomes a WorkItem, not a loose note:

- sourcing a supplier;
- contacting a merchant;
- preparing a filing;
- producing a media asset;
- testing a route;
- syncing a software PRD;
- collecting launch evidence.

Group WorkItems by `Milestone`, `WorkType`, business line, source document,
owner, assignee, priority, and due date. Do not create a separate `Project`
object and do not reintroduce Task Graph, GoalPhase, or old planning models.

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

### 5. Route money through Finance

A WorkItem may request a monetary effect, but Finance owns the money state:

- budget;
- estimate;
- commitment;
- invoice;
- payment;
- refund;
- monetary metric.

Never infer Payment from a purchase note, WorkItem, Approval, or model answer.
Record Commitments before spend, Payments only after real payment evidence, and
link them back to the source WorkItem and Docs record.

### 6. Sync software PRDs as external product sources

If the project has a software repo, map it as an external source:

```bash
harness --project <company-os-project-selector> \
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

The top-level `--project` selects the Company OS project Store. The
command-level `--project-id` names the external software product source.
Treat GitHub webhooks and sync runs as observations of software product truth.
They do not overwrite commercial truth, create WorkItems, approve finance,
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

### 8. Seed as acceptance, not as final product entry

A seed script may prove that the current CLI/API can create the expected
records. It is not the final user-facing product entry and should not be the
only operating path.

For the Wanchengwanling example, current seed scripts include:

```bash
node scripts/seed-company-os-wanchengwanling-docs-v0.mjs
node scripts/seed-company-os-wanchengwanling-v0.mjs
node scripts/seed-company-os-wanchengwanling-four-system-v1.mjs
node scripts/seed-company-os-wanchengwanling-roadmap-v1.mjs
```

Use them as fixture/acceptance references, then move repeated operations into
stable CLI/API commands and module-specific scenario skills.

To write a real registered project Store rather than an isolated temporary
acceptance Store:

```bash
target/debug/harness project add /path/to/project-root
node scripts/seed-company-os-wanchengwanling-four-system-v1.mjs \
  --project /path/to/project-root
node scripts/seed-company-os-wanchengwanling-roadmap-v1.mjs \
  --project /path/to/project-root
```

For Wanchengwanling, the intended local project Store is:

```text
/Users/hhh0x/.harness/projects/new-day-wanchengwanling
```

Do not use repository markdown as the operating database. The durable business
workspace must be readable from the Company OS Store; markdown docs, generated
reports, and seed scripts are references or acceptance evidence.

The four-system seed proves the project can exist as Docs, Work, Organization,
Approval, and Finance records. The roadmap seed then adds unfinished goals for
CLI/API, skills, storage-backed custom pages, GitHub source sync, SQL
read/search, real launch data, and replication templates. Treat those seeded
WorkItems as the execution backlog.

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

## Handoff format

When handing off, state:

- created or proposed DocumentSpace, modules, and key documents;
- created or proposed WorkTypes, Milestones, and initial WorkItems;
- Organization actors, governance/business split, and permission gaps;
- Finance records, approvals, and money-state gaps;
- software source sync status;
- custom pages and whether they are design-only, implemented, or verified;
- commands/scripts run and acceptance results;
- stale docs or obsolete records to delete instead of preserving as active
  context.

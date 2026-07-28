# Wanchengwanling: AR Tourism Company OS Dogfood

```text
status: active commercial dogfood project
dogfood_project_id: new-day-wanchengwanling
external_software_project_id: wanchengwanling
canonical_for: applying Company OS to a real AR tourism project whose software PRDs live in GitHub
live_store: /Users/hhh0x/.harness/projects/new-day-wanchengwanling
```

## Scenario

`cyl19970726/wanchengwanling` is a real AR cultural-tourism project, not only a
mini-program repository. Its GitHub `dev` branch contains the software PRDs,
architecture, ADRs, and acceptance contracts for the WeChat mini program and
backend. The company also needs to run offline and commercial operations:

- scenic-spot AR rollout and device signoff;
- merchant onboarding and store go-live;
- prize procurement from merchants or suppliers;
- grand-prize purchasing, such as Polaroid cameras;
- fridge-magnet ordering, delivery, quality check, and shop allocation;
- logistics tracking and receipt evidence;
- merchant communication and operating readiness;
- social-media account operations and content publishing;
- blogger/KOL outreach, deliverables, compensation, and performance review.

The correct boundary is:

```text
GitHub repo
  -> software product truth and delivery evidence

Company OS
  -> business model, operating modules, WorkItems, Organization, Finance,
     procurement, merchant relations, content operations, creator relations,
     launch readiness, and linked result memory
```

## Dogfood contract

Wanchengwanling is the first real commercial project used to operate Company
OS against a non-toy business. This document is not the operating database. The
accepted dogfood target is the registered Company OS Store:

```text
Company OS project id: new-day-wanchengwanling
project root: /Users/hhh0x/new-day/wanchengwanling
Store root: /Users/hhh0x/.harness/projects/new-day-wanchengwanling
```

For this project, “done” means a future Agent can inspect the Store and answer:

- what the business sells, how money is split, and which rules are still
  planned;
- which Docs pages define the commercial model and product structure;
- which WorkItems exist, which Milestone/WorkType/module owns them, and who is
  assigned;
- which Organization actors are governance agents versus business agents;
- which Finance records exist, which are only commitments, and whether any
  payment evidence exists;
- which GitHub `dev` PRD/source observations have been synced and where they
  drift from commercial truth;
- which custom pages are Store-backed UI projections rather than static
  screenshots.

Repo markdown, generated HTML reports, expected images, and scripts can explain
or verify the product. They do not replace Store-backed Docs, Work,
Organization, Finance, source-sync records, or custom-page definitions.
Project-specific scripts are allowed only as bootstrap/acceptance fixtures
until the corresponding CLI/API/skill path exists; they must not become the
normal authoring interface for this commercial dogfood project.

## Initial module map

| Module | Company OS responsibility | GitHub / product sync point |
| --- | --- | --- |
| Product & Software Delivery | Software roadmap, PRD mapping, GitHub issue/PR WorkItems, release readiness, acceptance tracking | `docs/prd/**`, ADRs, API/backend/frontend changes, CI, PRs, device signoff |
| AR Field Rollout | Spot readiness, marker/media asset status, device testing, field validation, AR defects, launch blockers | mini-program AR PRD, asset manifests, COS/media references, AR acceptance evidence |
| Merchant Onboarding | Merchant leads, contact logs, shop profile, agreement state, staff/operator readiness, go-live checklist | approved shop records, staff binding, shop capabilities, admin setup |
| Prize Procurement & Logistics | Supplier/prize plan, purchase order, payment request, shipping, receipt, QC, stock allocation | reward/prize/magnet records and per-shop stock after receipt and approval |
| Content Operations | Channel strategy, account calendar, post briefs, drafts, publishing, metrics, retrospective decisions | product screenshots/assets when needed; not usually app DB |
| Creator / Blogger Outreach | Creator leads, outreach messages, collaboration terms, deliverables, content evidence, fees/gifts, metrics | campaign landing content or product updates only when needed |
| Launch Readiness | Cross-module dashboard: software, AR, stores, inventory, staff, content, creators, finance, risk | release branch/tag, app preview, device signoff, admin data readiness |

These modules should be created in Company OS Docs as linked business modules.
They should not be buried inside the software repository as markdown-only
plans, because most of their truth is operational, financial, relational, or
external.

## Current native bootstrap acceptance

The current executable acceptance path is:

```bash
pnpm acceptance:company-os:wanchengwanling-docs
pnpm acceptance:company-os:wanchengwanling-source
pnpm acceptance:company-os:wanchengwanling-four-system
pnpm acceptance:company-os:wanchengwanling-roadmap
```

The four-system seed composes the Docs seed and then creates native
Organization, Work, Approval, and Finance records in the same Store. It proves
that the project can be represented as Company OS data without introducing a
`Project` container, Task Graph, GoalPhase, or fixture-only business truth.

The verified v1 bootstrap contains:

| Surface | Native rows seeded |
| --- | --- |
| Docs | 12 Wanchengwanling Documents, 11 BusinessModules, core TypedRecords, and custom page definitions |
| Organization | Human Owner, Lead Agent, four Governance Agents, six Business Agents, one external merchant sample, three OrgUnits, and memberships |
| Work | MVP launch Milestone, replication Milestone, eight initial WorkItems, and eight explicit Assignments |
| Finance / Approval | one evidence-backed Human Approval and one approved ¥10 CNY merchant-share unit Commitment; zero Payments |

The approved Commitment models the known physical bracelet consignment split:
¥30 sale price, ¥10 merchant share, ¥20 company share. It is not payment
evidence and it does not approve any specific bank transfer. Unknown purchasing
amounts for Polaroids, food coupons, magnets, and bracelet manufacturing remain
planned until quotes or invoices exist.

The script is:

```bash
node scripts/seed-company-os-wanchengwanling-four-system-v1.mjs
```

By default the acceptance commands use an isolated temporary Store. To operate
the real Wanchengwanling project as Company OS data, register the project and
write to its centralized project Store instead:

```bash
target/debug/harness project add /Users/hhh0x/new-day/wanchengwanling

node scripts/seed-company-os-wanchengwanling-four-system-v1.mjs \
  --project /Users/hhh0x/new-day/wanchengwanling
```

That persistent Store is:

```text
~/.harness/projects/new-day-wanchengwanling
```

The seed is idempotent for an already-bootstrapped Wanchengwanling Store: if
the expected Documents and WorkItems exist, it reports `already_exists` and
does not append duplicate rows. It is still an acceptance/bootstrap path, not
the long-term user-facing entrypoint. Repeated operations should move into
stable CLI/API commands and scenario skills.

The unfinished-goal roadmap is maintained in
[Wanchengwanling Company OS Completion Roadmap](wanchengwanling-completion-roadmap.md)
and can be seeded into the same Store:

```bash
node scripts/seed-company-os-wanchengwanling-roadmap-v1.mjs \
  --project /Users/hhh0x/new-day/wanchengwanling
```

That roadmap covers CLI/API, skills, storage-backed custom pages, GitHub
source sync, SQL read/search, real launch operating data, and replication
templates. It is intentionally not CLI-only.

### Persistent Store state verified on 2026-07-27

The real local project Store currently contains the native bootstrap:

| Surface | Store evidence |
| --- | --- |
| Project registration | `new-day-wanchengwanling` -> `/Users/hhh0x/new-day/wanchengwanling` |
| Store root | `/Users/hhh0x/.harness/projects/new-day-wanchengwanling` |
| Docs | 12 `Document` rows in `space_id=wanchengwanling` |
| Work | 41 `work-wcw-*` WorkItems after the completion-roadmap seed: 15 MVP launch, 22 Company OS operating-surface, and 4 replication WorkItems |
| Organization | Lead Agent, four Governance Agents, six Business Agents, human owner, external participant, org units, memberships |
| Finance / Approval | approved ¥10 CNY merchant-share unit Commitment; no Payment inferred |

This is the correct target for Wanchengwanling Company OS records. Repository
markdown and generated reports may explain or visualize the product, but the
operating records for this commercial project should be queryable from this
Store.

### Docs foundation verified on 2026-07-28

The first dogfood authoring pass established the two root commercial pages as
Store-backed operating pages:

- `00 Project Home / 商业总览` now has a `page_contract`,
  `module_directory`, `operating_loop`, and expanded `project_overview`
  records, with explicit active Relations and standard Views.
- `01 Business Model / 商业模式` now has a `page_contract`, physical and
  virtual bracelet revenue lines, merchant capability value model, 8/12
  incentive model, Finance boundary model, cost model, replication model, MVP
  metric definitions, and explicit active Relations and standard Views.

Both pages were updated through the governed Docs CLI and verified with:

```bash
target/debug/harness --project /Users/hhh0x/new-day/wanchengwanling \
  company docs query --document document-wcw-project-home --json

target/debug/harness --project /Users/hhh0x/new-day/wanchengwanling \
  company docs query --document document-wcw-business-model --json
```

The resulting health findings for these two pages are empty. The next dogfood
slice should build `02 Bracelet & Product` and `03 Route & AR Experience` with
the same pattern before custom-page work is expanded.

### Product and route foundation verified on 2026-07-28

The second dogfood authoring pass established the next two commercial pages:

- `02 Bracelet & Product / 手环与产品售卖` now has a page contract, physical
  and virtual bracelet records, entitlement rules, sales channels, the
  ¥30/¥10/¥20 consignment split, and a design/inventory dependency record.
- `03 Route & AR Experience / 景点路线与 AR 体验` now has a page contract, site
  record, route catalog, twelve spot records from the Wanchengwanling software
  source `docs/reference/stamps/new-twelve-stamps.md`, distinct 8-spot magnet
  and 12-spot lottery rules, AR asset readiness, and field validation models.

Both pages were verified through CLI and frontend Store-live rendering:

```bash
target/debug/harness --project /Users/hhh0x/new-day/wanchengwanling \
  company docs query --document document-wcw-bracelet-product --json

target/debug/harness --project /Users/hhh0x/new-day/wanchengwanling \
  company docs query --document document-wcw-route-ar-experience --json
```

Frontend evidence is recorded in
[`docs/design/company-os-v4/wanchengwanling-dogfood-docs-v1`](../../design/company-os-v4/wanchengwanling-dogfood-docs-v1/README.md).

The resulting health findings for these two pages are empty. The next dogfood
slice should continue with `04 Merchant Network` and
`05 Rewards, Procurement & Inventory`, because those pages connect the product
and route model to real merchant onboarding, reward procurement, inventory,
logistics, and Finance effects.

## Repo PRD mapping

Register the GitHub repo as an `ExternalProject`:

```text
ExternalProject:
  id: wanchengwanling
  repo: cyl19970726/wanchengwanling
  branch: dev
  role: software product source
```

Initial product sources should be synced into the same persistent Store as
Docs `TypedRecord`s:

```bash
HARNESS_COMPANY_OS_TOKEN=<local-or-server-write-token> \
target/debug/harness --project /Users/hhh0x/new-day/wanchengwanling \
  company docs source sync \
  --definition page-wcw-software-product-sources \
  --module module-wcw-software-product-sources \
  --source-document document-wcw-software-product-sources \
  --actor agent-wcw-docs-governance \
  --repo-path /Users/hhh0x/new-day/wanchengwanling \
  --repo cyl19970726/wanchengwanling \
  --branch dev \
  --project-id wanchengwanling \
  --path README.md \
  --path IMPLEMENTATION_PLAN.md \
  --path docs/frontend-design-os \
  --path specs \
  --path tools/mp-agent-os \
  --path infra/nfc-gateway/README.md \
  --path reports/live-prd/README.md
```

Important parameter distinction:

- top-level `--project /Users/hhh0x/new-day/wanchengwanling` selects the
  Company OS project Store;
- command-level `--project-id wanchengwanling` names the external software
  product source.

On 2026-07-27 this sync wrote 29 Docs records to the persistent Store: one
`external_project`, seven `product_doc_source`, twenty
`product_doc_snapshot`, and one `source_sync_run`. The observed local worktree
commit was `36b138fc7b4d59e77f0ea7635ffaaa4261558fff`; the sync record keeps
`branch=dev` as the intended source branch, while the local checkout must still
be treated as an observation.

Current synced product sources:

| Repo path | Source class | Company OS mapping |
| --- | --- | --- |
| `README.md` | external project document | Product & Software Delivery overview |
| `IMPLEMENTATION_PLAN.md` | external project document | implementation plan and Work correlation source |
| `docs/frontend-design-os/**` | frontend design contract | visual/page acceptance and UI design evidence |
| `specs/**` | external project document | feature requirements, design, and task contracts |
| `tools/mp-agent-os/**` | external project document | mini-program acceptance harness contract |
| `infra/nfc-gateway/README.md` | external project document | NFC gateway implementation reference |
| `reports/live-prd/README.md` | external project document | live PRD report contract |

Company OS should store `ProductDocSnapshot` rows for these files and map them
to modules and WorkTypes. It should not copy their full product authority into
commercial docs; instead it links the exact path and commit.

## Commercial model belongs in Company OS

The commercial model is not just a software PRD. It should live as Company OS
Docs with linked records and WorkItems:

```text
Wanchengwanling Commercial Model
  - user journey and value proposition
  - revenue model and cost model
  - merchant participation model
  - prize and inventory policy
  - sponsorship / creator cooperation policy
  - launch stages and risk controls
  - finance views for budget, commitment, payment, and evidence
```

If the commercial model changes product behavior, Company OS creates a
software WorkItem linked to the affected GitHub PRD or code area. If a software
PRD changes the business model, GitHub sync creates a Docs Governance review
WorkItem.

## Merchant onboarding loop

```text
Docs:
  Merchant profile, business terms, contact history, required materials,
  launch checklist, staff/operator notes

Work:
  Contact merchant
  Collect materials
  Confirm participation and prize/store policy
  Configure shop
  Bind staff/operator
  Validate redemption process
  Mark shop go-live

Organization:
  Merchant Ops Agent or human owner
  external merchant contact
  accountable internal owner

Finance:
  only if there is cost, sponsorship, settlement, purchase, commission, or
  payment evidence

GitHub/product:
  approved shop and capability setup becomes backend/admin data or a software
  issue only when the product needs to change
```

The merchant application feature in the mini program is a product sync point,
not the full onboarding workflow. Real communications, terms, readiness, and
go-live evidence belong in Company OS.

## Prize procurement and logistics loop

```text
Docs:
  Prize plan, SKU, supplier, ordering note, allocation policy, receipt evidence

Work:
  choose supplier
  place order
  track shipment
  inspect arrival
  allocate stock to stores
  update launch readiness

Finance:
  purchase commitment
  payment approval
  invoice/receipt
  refund or adjustment if needed

GitHub/product:
  after receipt and approval, sync reward/prize/magnet definitions and
  per-shop inventory allocation into backend/admin flows
```

Examples:

- food prizes from participating stores become prize records and possible
  merchant cooperation records;
- Polaroid grand prizes become high-value prize procurement records with
  finance approval and receipt evidence;
- Pinduoduo fridge-magnet orders become supplier/order/shipment/QC records
  before they create usable magnet inventory in the product.

## Content and creator operations loop

```text
Docs:
  content strategy, account guidelines, creative briefs, creator brief,
  campaign pages, retrospectives

Work:
  write script
  produce asset
  publish post
  review metrics
  contact creator
  negotiate deliverable
  verify publication

Organization:
  Content Agent, Creator Outreach Agent, human owner, external creator

Finance:
  creator fees, gifts, sponsorship, paid promotion, reimbursement

GitHub/product:
  product screenshots/assets may come from the repo; app changes are created
  only when the campaign requires product support
```

Metrics such as views, likes, comments, shares, conversion, store visits, and
redemption usage should be `MetricObservation` records linked to campaign
documents and WorkItems, not manually copied into many pages.

## Agent operating structure

The starting Organization can stay shallow:

```text
Human Owner
└── Lead Agent
    ├── Docs Governance Agent
    ├── Work Governance Agent
    ├── Finance Governance Agent
    └── Org / HR Governance Agent
        ├── Merchant Ops Agent
        ├── Procurement & Logistics Agent
        ├── Content Ops Agent
        ├── Creator Outreach Agent
        ├── Development Agent
        └── IP / Product Design Agent
```

Business Agents are created only when recurring responsibility exists. A
one-time task can use a temporary Agent Team, Workflow, Host execution, human
assignee, or external participant without adding a permanent Agent.

The current native seed follows the active Organization rule: Lead manages the
four Governance Agents; business Agents report under Org/HR and collaborate
through Docs, Work, and Finance records.

## First useful WorkTypes

| WorkType | Typical owner | Result |
| --- | --- | --- |
| `software_delivery` | Product / Development Agent | GitHub PR, acceptance evidence, updated product mapping |
| `device_signoff` | AR Field Agent | real-device evidence and launch decision |
| `ar_asset_acceptance` | AR Field Agent | accepted media/marker/manifest evidence |
| `merchant_onboarding` | Merchant Ops Agent | go-live-ready merchant record |
| `procurement` | Procurement Agent | ordered and financially authorized purchase |
| `logistics_tracking` | Procurement Agent | shipment and receipt evidence |
| `inventory_allocation` | Procurement + Merchant Ops | approved store stock allocation |
| `content_post` | Content Ops Agent | published content and metrics link |
| `creator_outreach` | Creator Outreach Agent | accepted/rejected collaboration path |
| `finance_approval` | Finance Governance Agent | commitment/payment decision |
| `launch_readiness` | Lead Agent | cross-module launch state |

These WorkTypes let Work show all commitments across software, field,
merchant, inventory, content, and finance without inventing a generic
`Project` container.

## Webhook behavior for `dev`

When the `dev` branch changes, Company OS should:

1. verify the GitHub webhook signature and repo registration;
2. append a `SourceChangeEvent`;
3. run a `SourceSyncRun` for mapped paths;
4. update `ProductDocSnapshot` rows for changed PRDs, ADRs, acceptance docs,
   and design contracts;
5. link PR/commit/CI data to matching WorkItems when correlation is explicit;
6. create a Docs Governance review WorkItem for material drift, such as new
   business line, changed acceptance contract, deleted mapped file, finance
   implication, privacy/security implication, or launch-impacting change; and
7. leave commercial strategy, merchant records, purchase records, finance, and
   organization authority untouched unless an Agent or Human performs a
   governed Company OS action.

This makes `dev` updates visible and actionable without letting GitHub become
the company operating system.

## First implementation target

The first practical Company OS slice should not try to model every operation at
once. A useful first milestone is:

```text
Wanchengwanling Launch Readiness v0
  - register GitHub dev PRD sources
  - map the seven software business lines plus admin
  - create Company OS modules for AR, Merchant, Procurement, Content,
    Creator, and Launch Readiness
  - create seed WorkItems for:
      AR real-device signoff
      first five merchants
      fridge-magnet procurement
      grand-prize procurement
      first 14-day content calendar
      first creator outreach batch
  - link every software task to GitHub issue/PR DeliveryRefs
  - require Finance records for any purchase, paid promotion, or external fee
```

This turns the real project into an operated Company OS example without moving
software product truth out of GitHub.

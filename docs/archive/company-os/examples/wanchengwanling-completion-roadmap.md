# Wanchengwanling Company OS Completion Roadmap

```text
status: active roadmap
scope: unfinished goals required to make Wanchengwanling operable through Company OS
company_store_id: agent-company
store: /Users/hhh0x/.harness/companies/agent-company
dogfood_role: first real commercial Company OS dogfood project
canonical_boundary: WorkItems in the Company Store are the execution backlog; this document explains the grouping and acceptance logic
```

## Purpose

Wanchengwanling is now represented in the Company OS storage layer as the first
real commercial dogfood project, but it is not yet fully operable. The
remaining goals must cover both sides:

1. the commercial project itself: merchants, rewards, content, creators, route
   readiness, IP/product assets, launch evidence, and replication; and
2. the Company OS product capabilities required to operate that project:
   dedicated CLI/skills, storage-backed custom pages, GitHub source sync,
   SQL-derived read/search surfaces, expected-vs-actual screenshots, and
   acceptance checks.

The backlog must not collapse these into a generic `Project` object. Work is
grouped by `Milestone`, `WorkType`, business module, owner, assignee, and
source document.

## Current native baseline

The four-system bootstrap baseline contains:

| Surface | Current native state |
| --- | --- |
| Docs | 12 Documents, 11 BusinessModules, typed business records, product-source snapshots |
| Work | 8 launch WorkItems under `milestone-wcw-mvp-launch` |
| Organization | Human Owner, Lead Agent, four Governance Agents, six Business Agents |
| Finance | approved ¥10 CNY merchant-share unit Commitment; no Payment inferred |
| Custom page metadata | base module pages plus work/finance control page definitions |
| Software source sync | 20 `product_doc_snapshot` observations from the local Wanchengwanling worktree |

### Store-backed Docs foundation status

On 2026-07-28, the first dogfood Docs foundation pass moved `00 Project Home`
and `01 Business Model` beyond prose-only pages. The live Store now contains
page contracts, structured business facts, standard Views, and explicit
Document ↔ TypedRecord Relations for both pages. This was done through
`harness company docs ...` CLI commands with `HARNESS_COMPANY_OS_TOKEN`, not by
editing JSONL ledgers directly and not by rerunning a seed script.

Verified Store:

```text
/Users/hhh0x/.harness/companies/agent-company
```

Current completed slice:

| Page | Store-backed records | Views | Health |
| --- | ---: | ---: | --- |
| `document-wcw-project-home` | 4 records: `project_overview`, `page_contract`, `module_directory`, `operating_loop` | 3 | no findings from `docs query` |
| `document-wcw-business-model` | 11 records: page contract, revenue, value, merchant, incentive, cost, finance-boundary, replication, metric records | 3 | no findings from `docs query` |
| `document-wcw-bracelet-product` | 11 records: page contract, bracelet SKUs, entitlement rules, sales channels, consignment, design/inventory dependency | 4 | no findings from `docs query` |
| `document-wcw-route-ar-experience` | 19 records: page contract, site, spot catalog, 12 spot records, 8/12 rules, AR readiness, field validation | 4 | no findings from `docs query` |
| `document-wcw-merchant-network` | 9 records: page contract, merchant capabilities, merchant role segments, listing rule, onboarding and contact model | 4 | no findings from `docs query` |
| `document-wcw-rewards-procurement-inventory` | 10 records: page contract, reward/prize pool, procurement items, inventory, logistics, redemption evidence, Finance boundary | 4 | no findings from `docs query` |

Important record ids:

- `record-wcw-page-contract-project-home`
- `record-wcw-module-directory-v1`
- `record-wcw-operating-loop-mvp`
- `record-wcw-page-contract-business-model`
- `record-wcw-revenue-line-physical-bracelet`
- `record-wcw-revenue-line-virtual-bracelet`
- `record-wcw-merchant-value-capability-tags`
- `record-wcw-incentive-rules-8-12`
- `record-wcw-finance-boundary-business-model`
- `record-wcw-mvp-metric-definitions`
- `record-wcw-page-contract-bracelet-product`
- `record-wcw-entitlement-ar-route`
- `record-wcw-entitlement-8-magnet`
- `record-wcw-entitlement-12-lottery`
- `record-wcw-sales-channel-merchant-consignment`
- `record-wcw-sales-channel-mini-program`
- `record-wcw-page-contract-route-ar-experience`
- `record-wcw-spot-catalog-twelve-stamps`
- `record-wcw-spot-01-koucheng` through `record-wcw-spot-12-mise`
- `record-wcw-route-ar-asset-readiness-model`
- `record-wcw-route-field-validation-model`
- `record-wcw-page-contract-merchant-network`
- `record-wcw-merchant-capabilities-mvp`
- `record-wcw-merchant-segment-consignment`
- `record-wcw-merchant-segment-reward-redemption`
- `record-wcw-merchant-segment-prize-supplier`
- `record-wcw-merchant-segment-bracelet-benefit`
- `record-wcw-merchant-onboarding-model`
- `record-wcw-page-contract-rewards-procurement-inventory`
- `record-wcw-reward-ar-magnet`
- `record-wcw-prize-pool-mvp-lottery`
- `record-wcw-procurement-polaroid-two`
- `record-wcw-procurement-ar-magnet`
- `record-wcw-procurement-food-coupons`
- `record-wcw-finance-boundary-rewards`

Frontend Store-live evidence:

- [`docs/design/company-os-v4/wanchengwanling-dogfood-docs-v1/README.md`](../../design/company-os-v4/wanchengwanling-dogfood-docs-v1/README.md)

Verification commands:

```bash
target/debug/harness --company agent-company \
  company docs query --document document-wcw-project-home --json

target/debug/harness --company agent-company \
  company docs query --document document-wcw-business-model --json

target/debug/harness --company agent-company \
  company docs query --document document-wcw-bracelet-product --json

target/debug/harness --company agent-company \
  company docs query --document document-wcw-route-ar-experience --json

target/debug/harness --company agent-company \
  company docs query --document document-wcw-merchant-network --json

target/debug/harness --company agent-company \
  company docs query --document document-wcw-rewards-procurement-inventory --json

target/debug/harness --company agent-company \
  company docs traverse --document document-wcw-root --depth 2 --json
```

The completion-roadmap seed adds the unfinished backlog on top of that
baseline. The persistent local project Store has been verified with:

```text
total WorkItems: 41
milestone-wcw-mvp-launch: 15
milestone-wcw-company-os-operating-surface: 22
milestone-wcw-first-site-replication-kit: 4
```

The added operating-surface milestone is:

```text
milestone-wcw-company-os-operating-surface
```

Outcome: Wanchengwanling can be operated from storage-backed CLI/skills,
custom pages, GitHub source observations, standard views, and acceptance
evidence, without treating repository markdown, generated reports, or seed
scripts as the commercial source of truth.

## Completion waves

### Wave 1 — Storage-backed operating foundation

Goal: remove ambiguity between repo markdown, generated reports, fixture seed,
and Company OS durable records.

WorkItems:

- `work-wcw-company-os-bootstrap-cli`
- `work-wcw-company-os-skill-install-path`
- `work-wcw-source-sync-dev-branch-policy`

Acceptance:

- a real project can be registered and bootstrapped without relying on hidden
  local assumptions;
- all Company OS skills can be installed from the repository with one command;
- source sync clearly distinguishes Company OS project Store selection from
  external software `project_id`;
- source sync records actual observed commit/branch state and flags branch
  mismatch instead of silently implying remote `dev` truth.

### Wave 2 — Dedicated operator CLI/API

Goal: each governance module has an Agent-friendly CLI/API surface.

WorkItems:

- `work-wcw-finance-cli-v1`
- `work-wcw-org-cli-v1`
- `work-wcw-work-intake-from-docs`
- `work-wcw-docs-custom-page-lifecycle-cli`

Acceptance:

- Docs and Work remain CLI-first and action-backed;
- Finance gets dedicated commands for budget, commitment, approval, invoice,
  payment, refund, and monetary evidence without inferring Payment from
  Commitment;
- Organization gets dedicated commands for humans, Standing Agents, OrgUnits,
  memberships, permissions, and lifecycle proposals;
- Docs can scaffold/verify/publish custom page packages without becoming a
  second database.

### Wave 3 — Core custom pages

Goal: implement the storage-backed pages humans will actually look at.

WorkItems:

- `work-wcw-custom-command-center`
- `work-wcw-custom-work-board`
- `work-wcw-custom-launch-readiness`
- `work-wcw-custom-merchant-console`
- `work-wcw-custom-procurement-finance`
- `work-wcw-custom-route-ar-console`
- `work-wcw-custom-content-creator`
- `work-wcw-custom-ip-design-board`
- `work-wcw-custom-software-source-map`

Acceptance for every page:

- approved expected image or visual reference;
- deterministic fixture;
- actual Store-live screenshot;
- expected-vs-actual comparison;
- declared queries and allowed Actions in `CustomPageDefinition` and
  `CustomPagePackage`;
- fallback standard View;
- no direct store mutation, no synthetic Approval, no inferred Payment.

Page intent:

| Page | Primary question | Owning module | Writes |
| --- | --- | --- | --- |
| Command Center | Is the commercial launch ready and what blocks it? | Project Home / Launch Readiness | route to governed Work/Docs/Finance actions |
| Work Board | What work exists by milestone, type, owner, assignee, and status? | Launch Readiness / Work | WorkItem lifecycle/assignment |
| Launch Readiness | Can the MVP launch safely? | Launch Readiness | readiness evidence and review WorkItems |
| Merchant Console | Which merchants can sell, redeem, or provide benefits? | Merchant Network | merchant records and onboarding WorkItems |
| Procurement + Finance | What rewards/inventory need money or evidence? | Rewards, Procurement & Inventory | Commitment/Approval/Payment only through Finance actions |
| Route + AR Console | Are 12 spots configured and are 8/12 thresholds correct? | Route & AR Experience | AR validation WorkItems and evidence refs |
| Content + Creator | What will be published and who is being contacted? | Content Growth / Creator Outreach | content and outreach WorkItems, metrics refs |
| IP/Product Design Board | What bracelet, magnet, IP, and AR assets are ready? | IP & Product Design | design asset records and review WorkItems |
| Software Source Map | What software PRDs/code contracts are mapped at which commit? | Software Product Sources | Docs source sync and review WorkItems |

### Wave 4 — GitHub and delivery linkage

Goal: make Wanchengwanling dev updates visible without letting GitHub become
the company operating system.

WorkItems:

- `work-wcw-github-source-webhook`
- `work-wcw-github-delivery-refs`
- `work-wcw-prd-drift-review-queue`

Acceptance:

- webhook verifies source and appends `SourceChangeEvent`;
- sync appends `SourceSyncRun` and `ProductDocSnapshot`;
- PRs/issues/commits/CI are `DeliveryRef`s linked to explicit WorkItems;
- material software PRD drift creates a Docs Governance review WorkItem;
- no webhook can approve money, add agents, mutate commercial truth, or mark
  work completed without acceptance evidence.

### Wave 5 — SQL-derived read/search and page performance

Goal: support human-readable UI and Agent queries without replacing the
canonical append-only ledgers.

WorkItems:

- `work-wcw-sql-read-model-v1`
- `work-wcw-global-search-v1`
- `work-wcw-page-query-performance`

Acceptance:

- JSONL ledgers remain canonical writes;
- SQL is rebuildable from ledgers;
- search can answer Docs, Work, Org, Finance, source snapshots, evidence, and
  relation queries;
- read-model rebuild has deterministic acceptance.

### Wave 6 — Operating data completion for launch

Goal: populate the real business records needed to launch.

WorkItems:

- `work-wcw-real-merchant-list`
- `work-wcw-real-reward-quotes`
- `work-wcw-real-inventory-logistics`
- `work-wcw-real-content-calendar`
- `work-wcw-real-creator-leads`
- `work-wcw-real-ip-asset-package`
- `work-wcw-real-launch-runbook`

Acceptance:

- merchant records include capabilities: bracelet seller, magnet redemption,
  prize redemption, bracelet-benefit merchant, purchased-supply merchant;
- every purchase path has a Finance Commitment before Payment;
- rewards and inventory have supplier/order/logistics/QC evidence;
- content/creator work has publish evidence and metrics refs;
- launch readiness can be assessed from Store records without rereading chat.

### Wave 7 — Replication kit

Goal: make the model copyable to the next city, scenic area, or commercial
district.

WorkItems:

- `work-wcw-replication-site-template`
- `work-wcw-replication-merchant-template`
- `work-wcw-replication-reward-template`
- `work-wcw-replication-launch-template`

Acceptance:

- new site setup can be bootstrapped from templates;
- site-specific facts are records, not hard-coded page logic;
- custom pages continue to work from module/store queries;
- finance and org assumptions are explicit before launch.

## Historical seeding path

This roadmap was originally materialized by a seed script:

```bash
node scripts/seed-company-os-wanchengwanling-roadmap-v1.mjs \
  --project /Users/hhh0x/new-day/wanchengwanling
```

That path is now historical acceptance/migration evidence, not the authoring
interface for active dogfood. New roadmap changes should be made in
`agent-company` through `harness company docs ...`, `harness company work ...`,
`harness company org ...`, and `harness company finance ...` commands with the
owning operator skills.

For acceptance-only validation, omit `--project`; the script creates an
isolated temporary Store and reports counts without touching any real Company
Store.

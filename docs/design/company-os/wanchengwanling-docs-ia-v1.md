# Wanchengwanling Company OS Docs IA v1

```text
status: design contract for the first Store-backed commercial dogfood project Docs
owner_role: Docs Governance Agent + Human Owner
canonical_for: Wanchengwanling document hierarchy, page responsibilities, page linking, and custom-page candidates
source_evidence:
  - session 019f7843-4f4f-7f93-a76a-10e1fa59f23d line 33560: commercial model, bracelet prices, merchant split, AR check-in, rewards, merchant types
  - session 019f7843-4f4f-7f93-a76a-10e1fa59f23d line 33572: replication to next city/scenic area/commercial district
  - session 019f7843-4f4f-7f93-a76a-10e1fa59f23d line 33583: 8 spots unlock magnet, 12 spots unlock lottery
  - session 019f7843-4f4f-7f93-a76a-10e1fa59f23d line 33616: add IP & Product Design module
```

## Product stance

Wanchengwanling Docs are not a wiki dump and not a hand-built Notion clone. The
UI exists mainly for humans to understand the business; Agents operate the same
truth through CLI/API. Core pages should therefore be designed as operating
surfaces over native `Document`, `Block`, `TypedRecord`, `Relation`, `View`,
`WorkItem`, `ActorRef`, and `FinancialRecord` objects.

This IA is the product/page contract for the live Company OS dogfood project
`new-day-wanchengwanling`. The authoritative business records should be
inspectable from `/Users/hhh0x/.harness/projects/new-day-wanchengwanling`.
Repo docs, expected images, generated HTML reports, and scripts can guide or
verify the surface, but they are not the live commercial memory.

The commercial thesis is:

```text
Sell physical NFC bracelets and virtual bracelets.
Use bracelet identity to unlock AR scenic check-ins, physical cultural products,
merchant benefits, and prize redemption.
Use merchant participation and content distribution to turn a local scenic area
into a repeatable AR cultural-tourism operating template.
```

Current MVP facts:

- physical NFC bracelet: ¥30;
- virtual bracelet: ¥20;
- physical bracelet consignment split: merchant ¥10, company ¥20;
- route has 12 configured scenic spots;
- completing 8 spots unlocks AR magnet redemption;
- completing 12 spots unlocks lottery eligibility;
- prizes include AR magnets, 2 Polaroid cameras, and local food coupons such as
  low-ticket Chaozhou snacks;
- merchants are capability-tagged, not mutually exclusive: bracelet seller,
  magnet consignment/redemption point, prize supplier, prize redemption point,
  bracelet-benefit partner, mini-program shop-list participant;
- the project should be replicable to another city, scenic area, or commercial
  district by changing configuration, spots, AR assets, merchants, rewards,
  operations, and content plan.

## Top-level document tree

```text
Wanchengwanling / 万城万灵
├── 00 Project Home / 商业总览
├── 01 Business Model / 商业模式
├── 02 Bracelet & Product / 手环与产品售卖
├── 03 Route & AR Experience / 景点路线与 AR 体验
├── 04 Merchant Network / 商家网络
├── 05 Rewards, Procurement & Inventory / 奖品、采购与库存
├── 06 Content Growth / 自媒体内容增长
├── 07 Creator Outreach / 博主合作
├── 08 Launch Readiness / 上线准备
├── 09 IP & Product Design / IP 与产品设计
└── 10 Software Product Sources / GitHub PRD 映射
```

This tree is the primary navigation surface. Each page should expose sibling
links and key related records so the user can move from business overview to
execution detail without guessing where a fact lives.

## Page contracts

| Page | Primary question | Required presentation | Owned facts / records | Cross-page links |
| --- | --- | --- | --- | --- |
| 00 Project Home | What is this project, what is the current operating state, and where should I go next? | business thesis hero, MVP loop diagram, module cards, current blockers, top WorkItems, Finance/Approval watchlist, software-source drift, document tree | `project_overview`, launch snapshot, project KPI summary | all modules, open WorkItems, Finance gates, GitHub source status |
| 01 Business Model | What do we sell, why do users and merchants participate, how does money flow, and how does the model replicate? | product/revenue table, user value proposition, merchant value proposition, capability matrix, cost/finance boundary, replication canvas, KPI table | `revenue_model`, `cost_model`, `merchant_value_model`, `replication_model`, `business_metric_definition` | 02 products, 04 merchants, 05 procurement/finance, 06/07 growth, 10 software sources |
| 02 Bracelet & Product | What SKUs exist, what rights do they grant, where are they sold, and how are they settled? | SKU table, entitlement rules, sales-channel table, consignment rule, design/stock links | `bracelet_product`, `pricing_rule`, `entitlement_rule`, `sales_channel`, `consignment_rule` | 01 model, 03 route eligibility, 04 sellers, 05 inventory, 09 design assets |
| 03 Route & AR Experience | What route does the user complete, what happens at 8/12 spots, and which AR assets are ready? | 12-spot table/map, 8/12 unlock rules, AR asset readiness, shareability notes, test evidence | `site`, `spot`, `ar_asset_ref`, `reward_eligibility_rule`, `lottery_eligibility_rule` | 02 entitlements, 05 rewards, 06 content hooks, 09 AR/design assets |
| 04 Merchant Network | Which merchants exist, what can each do, and what action is next? | merchant capability matrix, contact/onboarding board, go-live checklist, redemption/seller views | `merchant`, `merchant_capability`, `contact_log`, `go_live_status` | 01 merchant value, 02 sales channel, 05 supplier/redemption, Work outreach tasks |
| 05 Rewards, Procurement & Inventory | What rewards must be bought or stocked, where are they, and what monetary effects exist? | reward catalog, prize pool, purchase orders, shipment tracking, inventory allocation, redemption ledger | `reward`, `prize_pool`, `purchase_order`, `shipment`, `inventory_allocation`, `redemption_rule` | 03 eligibility, 04 merchants, Finance commitments/payments, Work procurement |
| 06 Content Growth | What content is planned, published, and working? | campaign calendar, post pipeline, account metrics, content hooks by spot/merchant | `channel_account`, `content_campaign`, `post_draft`, `publish_record`, `metric_observation` | 03 AR moments, 04 merchants, 07 creators |
| 07 Creator Outreach | Which creators should be contacted and what collaboration state exists? | creator CRM, outreach board, deliverables, metrics | `creator_lead`, `outreach_record`, `collaboration_proposal`, `deliverable`, `creator_metric` | 06 campaigns, Work outreach tasks, Finance if paid collaboration |
| 08 Launch Readiness | Can the project go live? | cross-module gate board, blockers, owners, evidence, approvals | `launch_gate`, `risk`, `acceptance_evidence`, `readiness_milestone` | every module, WorkItems, Approvals |
| 09 IP & Product Design | What does the IP/product look like and which assets are approved? | IP character board, visual assets, bracelet/magnet design versions, AR asset board, manufacturing specs | `ip_character`, `visual_asset`, `product_design_asset`, `sku_design`, `design_review`, `manufacturing_spec` | 02 SKUs, 03 AR, 05 manufacturing/procurement, 06 content |
| 10 Software Product Sources | What does the GitHub software PRD currently say, and where does it drift from commercial truth? | repo/source table, synced PRD snapshots, drift list, follow-up WorkItems | `external_project`, `product_doc_source`, `product_doc_snapshot`, `source_sync_run`, `prd_drift` | 00/01 business truth, WorkItems for required software changes |

## Multi-page operating loops

### Bracelet sale and user experience

```text
01 Business Model
  -> defines bracelet-first revenue and user value
02 Bracelet & Product
  -> defines physical/virtual SKUs, price, channel, entitlement
03 Route & AR Experience
  -> applies entitlement to 12-spot route and 8/12 rules
05 Rewards / Inventory
  -> allocates magnets, Polaroids, food coupons
04 Merchant Network
  -> identifies sellers, redemption points, benefit partners
06 Content Growth
  -> turns AR moments and merchant routes into distribution
00 Project Home
  -> summarizes state, blockers, metrics, and next WorkItems
```

### Merchant onboarding

```text
01 Business Model
  -> explains why a merchant joins
04 Merchant Network
  -> records merchant capability tags and onboarding status
Work
  -> creates contact / contract / go-live WorkItems
05 Rewards / Inventory
  -> links supplier, redemption, inventory, logistics, and procurement
Finance
  -> records commitments and payments only when money is involved
00 Project Home
  -> shows launch readiness and unresolved merchant blockers
```

### New city / scenic area replication

```text
01 Business Model
  -> replication thesis and reusable economics
03 Route & AR Experience
  -> new Site and Spot configuration
04 Merchant Network
  -> new local merchant capability map
05 Rewards / Inventory
  -> new reward/procurement allocation
06/07 Growth and Creator Outreach
  -> local content and creator plan
08 Launch Readiness
  -> gate for the new rollout
10 Software Product Sources
  -> verifies mini-program configuration requirements
```

## Custom-page candidates

Do not build custom pages for everything. Use them only where standard document
blocks and Views cannot make the operating state clear.

| Custom page | Priority | Purpose | Standard fallback |
| --- | --- | --- | --- |
| Wanchengwanling Command Center | P0 | one-screen state across Docs, Work, Org, Finance, source sync, and launch blockers | 00 Project Home with module cards and saved Views |
| Business Model Canvas | P1 | make pricing, merchant incentives, 8/12 reward loop, cost boundary, and replication model visible together | 01 Business Model |
| Merchant Network Console | P1 | capability matrix plus onboarding/contact/go-live state | 04 Merchant Network standard table/board views |
| Launch Readiness Cockpit | P1 | cross-module gate view before launch | 08 Launch Readiness |
| Procurement & Inventory Console | P2 | reward procurement, shipment, inventory, redemption, and Finance links | 05 Rewards / Procurement & Inventory |
| IP/Product Design Asset Board | P2 | design versions, review state, usage, manufacturing specs | 09 IP & Product Design |

## Acceptance standard

A Wanchengwanling Docs page is acceptable only when:

1. the page has a clear primary question and navigation role;
2. stable facts are represented as native typed records or relations when they
   will be reused;
3. prose explains intent and trade-offs, not copied state that should come from
   records;
4. Work, Organization, Finance, and software-source effects are linked through
   their owning systems;
5. the Store-live UI shows the same page and document tree humans need to
   review;
6. Agents can retrieve the same operating context through CLI/API; and
7. any custom page has a fallback standard document/view and does not become a
   second source of truth.

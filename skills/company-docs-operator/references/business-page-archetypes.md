# Business page archetypes

Use this when shaping a commercial project DocumentSpace.

| Page | Primary question | Required presentation | Native facts |
| --- | --- | --- | --- |
| Project Home | What is the business, what modules exist, what is blocked, and where next? | thesis, loop, module cards, top WorkItems, Finance/Approval watchlist, source drift, document tree | `project_overview`, KPI summary, launch snapshot, page contracts |
| Business Model | What is sold, who pays, why partners join, how money flows, and how it replicates? | revenue table, user/partner value, cost boundary, capability matrix, replication canvas, KPI table | `revenue_model`, `cost_model`, `value_proposition`, `replication_model`, `metric_definition` |
| Product / Offer | What SKUs and rights exist, and how are they sold or fulfilled? | SKU table, entitlement rules, channel/settlement rules, design and inventory links | `product`, `pricing_rule`, `entitlement_rule`, `sales_channel` |
| Experience / Route | What does the user complete, and what unlocks at each threshold? | route/spot table, rule cards, asset readiness, evidence links | `site`, `spot`, `experience_rule`, `asset_ref`, `eligibility_rule` |
| Merchant / Partner Network | Who participates, what capability each has, and what action is next? | capability matrix, onboarding board, contact log, map/list, related Work | `merchant`, `partner`, `capability`, `contact_log`, `go_live_status` |
| Procurement / Inventory | What must be bought, where is it, what can be redeemed, and what did it cost? | purchase table, shipment status, inventory allocation, redemption ledger, Finance links | `purchase_order`, `shipment`, `inventory_allocation`, `reward`, `redemption_rule` |
| Content Growth | What content is planned, published, and working? | calendar, post pipeline, metric table, content hooks | `campaign`, `post_draft`, `publish_record`, `metric_observation` |
| Creator Outreach | Which creators are targeted and what collaboration state exists? | creator CRM, outreach board, deliverables, collaboration metrics | `creator_lead`, `outreach_record`, `proposal`, `deliverable` |
| Launch Readiness | Can this project go live safely? | gate board, blockers, owners, evidence, approvals | `launch_gate`, `risk`, `acceptance_evidence`, `readiness_milestone` |
| IP & Product Design | What assets are approved and where are they used? | asset board, design versions, review state, manufacturing specs | `ip_character`, `visual_asset`, `product_design_asset`, `design_review`, `manufacturing_spec` |
| Software Product Sources | What does external software truth say, and what drift affects the business? | source table, snapshot list, drift queue, follow-up WorkItems | `external_project`, `product_doc_source`, `product_doc_snapshot`, `source_sync_run`, `prd_drift` |

Choose the closest archetype, then adapt names to the business. Do not create a
custom page just because the page should look good. Use a custom page only when
standard Blocks and Views cannot make the operating state clear.

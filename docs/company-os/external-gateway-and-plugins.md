# External Gateway and Plugin Intake

```text
status: product contract, partial implementation
owner_role: product + platform
canonical_for: how external channels enter Company OS without becoming authority
```

Company OS needs external gateways because a real Agent-operated company will
receive work through systems outside the dashboard: WeCom groups, GitHub,
email, forms, payment systems, supplier portals, and future plugins.

An external gateway is an intake and delivery adapter. It is not a company
actor by itself unless Organization records a service actor for it, and it is
never authority for money, permissions, legal commitments, or business truth.

## Contract

```text
External system event
  -> Gateway adapter normalizes identity, channel, source, and evidence
  -> Organization resolves the responsible Human / Standing Agent / service
  -> Docs provides the knowledge scope and return location
  -> WorkItem is created when follow-up work is required
  -> Finance / Approval / Org changes are routed to their owning systems
  -> result summary and evidence return to Docs
```

The gateway may read scoped Docs and submit governed Actions. It must not write
ledgers directly, approve its own requests, or treat a chat message as a
payment, org permission, or completed WorkItem.

## Social Content Gateway v0

The social-content gateway covers platform accounts and publishing operations
for channels such as Xiaohongshu, WeChat Channels, Douyin, Kuaishou, and future
content platforms. It is an operating bridge for content work, not a marketing
database and not a license to publish without policy.

```text
Content strategy Document
  -> campaign / post plan TypedRecords
  -> WorkItem: draft, produce, review, publish, collect metrics
  -> Organization resolves Content Agent, Creator Agent, Human Owner, service
  -> platform adapter opens API/mobile/browser automation when allowed
  -> evidence refs: draft, screenshot, published URL/id, metrics snapshot
  -> result summary returns to Docs and Work
```

| Object | Responsibility | Boundary |
| --- | --- | --- |
| Social Gateway adapter | Normalizes platform, account, post, draft, publication, metric, and screenshot/evidence refs. | It is transport and observation. It does not own campaign truth or approval. |
| Content Growth Agent | Plans owned-channel content, prepares briefs/scripts/assets, routes publication WorkItems, reviews metrics, and updates retrospectives. | Cannot invent platform credentials, bypass review, or publish high-risk content outside policy. |
| Creator Outreach Agent | Tracks external creator leads, deliverables, collaboration status, and creator-published evidence. | Does not become the external creator and cannot accept paid terms without approval. |
| Docs | Holds content strategy, account guidelines, campaign calendar, creative briefs, publication records, metrics, and retrospectives. | Does not store private platform cookies, raw personal messages, or unbounded scrape archives. |
| Work | Holds concrete commitments: write script, create asset, schedule/publish, collect metrics, contact creator, verify publication. | A social post idea is not done until the WorkItem has result/evidence refs. |
| Organization | Holds who may operate each account, which Agent has tools, and where Human gates apply. | A logged-in phone/app session does not grant Company OS authority by itself. |

Publication is a policy-gated action. The system may automate drafting,
previewing, screenshot capture, metric collection, and low-risk scheduled
publishing only when the account, content class, platform, and responsible
Actor are allowed by Organization policy. Otherwise the WorkItem reaches a
`waiting_for_approval` or `human_action` state with a clear preview and
evidence bundle.

The minimum v0 native records are:

- `social_platform_account`: platform, handle/display name, login state,
  credential boundary, responsible Actor, allowed automation level, Human gate
  policy, and maintained Docs;
- `content_campaign`: thesis, audience, offer/route tie-in, target channels,
  assets, cadence, and success metrics;
- `social_post_plan`: platform, account, topic, hook, copy brief, asset refs,
  desired publish window, status, approval requirement, source WorkItem;
- `social_post_publication`: platform post id/URL when available, published
  timestamp, publisher Actor/service, screenshot/evidence refs, and source
  WorkItem;
- `social_metric_snapshot`: views, likes, comments, shares, saves, follows,
  click/conversion signals, collection timestamp, evidence, and linked
  campaign/post.

These records are ordinary Docs `TypedRecord`s in the relevant business module
until a dedicated schema is implemented. They must remain reconstructable from
the Company Store and visible from the Content Growth / Creator Outreach
Documents and Work views.

## WeCom v0

The first planned gateway is Enterprise WeChat / WeCom for Wanchengwanling
merchant operations.

| Object | Responsibility | Boundary |
| --- | --- | --- |
| WeCom Gateway adapter | Receives merchant group messages, maps group/user/shop identity, stores event/evidence refs, and forwards answerable questions to the responsible Agent. | No policy authority, no direct Finance writes, no permission grants. |
| Merchant Ops Agent | Answers merchant questions from scoped Docs, summarizes important messages, and creates WorkItems for follow-up. | Cannot invent merchant policy or spending approval. Escalates uncertain answers. |
| Docs | Holds merchant FAQ, onboarding rules, shop capability records, contact summaries, and result memory. | Does not become a raw chat archive or payment record. |
| Work | Holds actionable follow-up: update shop info, confirm redemption point, send materials, resolve blocker. | Does not own original chat transport. |
| Organization | Holds who the Merchant Ops Agent is, what tools it has, and which humans/agents may approve sensitive changes. | Agent detail UI is a projection of Org + Work + Docs + Gateway summaries, not a separate object model. |

## Product implications

- The Org UI should allow a durable Standing Agent such as Merchant Ops Agent
  to show gateway inbox summaries, current WorkItems, maintained Docs, tools,
  skills, and permission boundaries.
- The Docs UI should show merchant-facing knowledge and linked WorkItems, but
  most edits should still be performed through CLI/skills by Agents.
- The Work UI should show gateway-created WorkItems by business line,
  milestone, work type, source document, accountable owner, and assignee.
- GitHub PRD/source sync is another gateway-like observation path: it observes
  software product truth and creates source snapshots or review WorkItems, but
  it does not overwrite commercial truth.

## GitHub connector priority

GitHub is the next core connector for AgentOS dogfood because development
WorkItems need a reliable bridge to issues, branches, pull requests, checks,
previews, deployments, and releases.

The connector maps external software-delivery facts into Company OS refs:

| GitHub fact | Company OS usage | Boundary |
| --- | --- | --- |
| Issue | intake, discussion, or delivery tracking ref for a Development WorkItem | Issue open/close is not WorkItem lifecycle authority unless an explicit sync policy says so. |
| Branch / worktree | execution workspace ref | It does not imply assignment or acceptance. |
| Pull Request | delivery ref with review and diff evidence | Merge proves repository delivery, not product acceptance. |
| Check / CI result | evidence ref for acceptance criteria | Passing checks do not approve product, legal, finance, or org changes. |
| Repo PRD docs | source observation for software product docs | GitHub docs can be mapped into Docs, but Company Store remains operating truth. |

Gateway-created development work still follows the same loop: source Document
or external observation -> WorkItem -> Organization Actor assignment ->
execution -> PR/check evidence -> Docs/Work result update. Future WeCom,
social, ecommerce, logistics, and payment connectors should follow the same
adapter pattern instead of becoming separate operating models.

## Current implementation status

| Capability | Status |
| --- | --- |
| Docs/Work/Org/Finance operating substrate | partial, with dedicated CLI and Store-live projections |
| GitHub/local repo source sync into Docs records | implemented for local worktree observation |
| Social content gateway contract | product contract; Store-backed TypedRecords/WorkItems are dogfood-ready |
| Xiaohongshu phone readiness check | local device can be inspected through ADB/scrcpy when the Human authorizes the session |
| Douyin phone readiness check | local device can be inspected through ADB/scrcpy when the Human authorizes the session |
| WeChat Channels readiness check | planned; requires WeChat/Channels account state and policy review |
| WeCom gateway schema/API/CLI | planned |
| Service actor modeling for gateways | planned; current Org CLI v1 covers human/agent/unit/membership admin authoring |
| Gateway event inbox in Agent detail workspace | planned |
| Merchant Ops Agent scoped Docs answering | planned |

The current Wanchengwanling dogfood Store has created the canonical WorkItem
`work-wcw-agentos-wecom-gateway-v0` from
`document-cli-11-agentos-dogfood-external-gateway-agentos` to implement this
slice.

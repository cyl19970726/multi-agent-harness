# External Gateway and Plugin Intake

```text
status: product contract, partial implementation
owner_role: product + platform
canonical_for: how external channels enter Company OS without becoming authority
```

Company OS needs external gateways because a real Agent-operated company will
receive work through systems outside the dashboard: WeCom groups, GitHub,
email, forms, payment systems, supplier portals, social platforms, ecommerce
platforms, logistics systems, and future plugins.

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

## Plugin shape: action + connector + view

A platform plugin is the normal packaging unit for external systems. It is not
just an execution tool. It brings three separable capabilities:

| Capability | Purpose | Company OS boundary |
| --- | --- | --- |
| `action` | Performs a requested operation such as prepare draft, upload media, submit publication, reply, update profile, sync issue, or request order data. | Every write or external side effect is a named governed Action with actor, policy, idempotency, risk tier, and evidence. |
| `connector` | Synchronizes external state such as account profile, posts, metrics, comments, private/business messages, merchant groups, orders, logistics, or repository delivery facts. | Synced facts become typed records, relations, metric observations, message summaries, or evidence refs. Raw private data is scoped and retained by policy. |
| `view extension` | Declares how plugin-provided records should appear in Docs, Work, Organization, and Agent detail surfaces. | A view reads Company OS records and declared projections; it does not become a second store or bypass underlying Documents, WorkItems, ActorRefs, approvals, or permissions. |

The platform-specific implementation belongs in a plugin, skill, MCP server,
or CLI adapter owned by that plugin. The generic Harness CLI must not hard-code
how Xiaohongshu, Douyin, WeChat Channels, WeCom, Taobao, Pinduoduo, GitHub, or
a logistics portal clicks buttons, handles pages, signs requests, stores
tokens, or interprets platform-specific IDs. The core may expose generic
Company OS commands and generic gateway Action/observation contracts; the
plugin supplies the platform manifest, capabilities, transport implementation,
skill instructions, and view declarations.

Existing tools, `MCP`, plugin-owned `CLI`, official APIs, browser automation,
and phone automation are all valid operation transports. Choose the one that
gives the Agent or host the most reliable, testable interface for the current
platform. For GitHub, that can simply be `gh`/Git in the first slice. Either
way, the durable product result is the same:

```text
Plugin action or connector observation
  -> GatewayAction / GatewayEvent evidence
  -> TypedRecord, Relation, MetricObservation, ExternalMessage summary, or WorkItem
  -> Docs / Work / Organization projection
  -> Agent or Human review and follow-up
```

The plugin may also read the current Company OS context to perform its task,
but it may not treat external platform state as higher authority than the
owning Company OS record. For example, a post URL can prove publication
evidence; it does not prove the content WorkItem is accepted. A message can
trigger merchant follow-up; it does not become merchant policy. A paid
promotion configuration can be prepared; it does not authorize spend without
the required Finance and Approval records.

### Gateway plugin manifest

Each plugin declares a manifest at installation or registration time:

```json
{
  "gateway": "social_content",
  "platform": "xiaohongshu",
  "display_name": "Xiaohongshu",
  "transports": ["mcp", "phone_automation", "browser_automation", "official_api"],
  "actions": [
    {"name": "prepare_draft", "risk": "R1", "writes_external_state": false},
    {"name": "upload_media", "risk": "R2", "writes_external_state": true},
    {"name": "submit_publish", "risk": "R2", "writes_external_state": true},
    {"name": "sync_metrics", "risk": "R1", "writes_external_state": false},
    {"name": "sync_inbox", "risk": "R2", "private_data": true},
    {"name": "prepare_paid_promotion", "risk": "R3", "finance_required": true}
  ],
  "record_types": [
    "social_platform_account",
    "social_post_plan",
    "social_post_publication",
    "social_metric_snapshot",
    "external_message_thread"
  ],
  "view_extensions": [
    "account_overview",
    "content_calendar",
    "post_performance_table",
    "inbox_followup_queue",
    "agent_detail_gateway_panel"
  ]
}
```

The manifest is a capability declaration, not authority. Organization policy
still decides which Actor may use which action on which account, and Work /
Approval / Finance still own commitments, gates, and money state.

The gateway manifest **schema** (the exact JSON shape shown above) is
**explicitly-deferred**. The example here is a design sketch; a governed manifest
schema with a canonical JSON Schema definition, validation rules, and a
Store-backed manifest registry is a separate product increment that belongs to
the external gateway roadmap, not the governance retirement wave. Until that
schema is contracted, plugins declare their manifests as unstructured
projection documents, and the harness accepts them as capability claims without
schema enforcement.

## Social Content Gateway v0

The social-content gateway covers platform accounts and publishing operations
for channels such as Xiaohongshu, WeChat Channels, Douyin, Kuaishou, and future
content platforms. It is an operating bridge for content work, not a marketing
database and not a license to publish without policy. Social platforms should
enter through plugin packages that combine skills, MCP or CLI transport,
connector sync, and view extensions.

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
| Social Gateway plugin | Declares account, action, connector, and view capabilities for a platform and ships the skill/MCP/CLI adapter that performs them. | It is capability and transport. It does not own campaign truth, Work acceptance, Organization authority, Approval, or Finance state. |
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
- `external_message_thread`: platform, account, thread/comment/private-message
  scope, external participant refs when known, summary, sensitivity, last
  synced timestamp, evidence refs, responsible Actor, and follow-up WorkItem
  refs. Private or personal content is policy-scoped; broad exports are not a
  default connector behavior.

These records are ordinary Docs `TypedRecord`s in the relevant business module
until a dedicated schema is implemented. They must remain reconstructable from
the Company Store and visible from the Content Growth / Creator Outreach
Documents and Work views.

### Social account sync into Docs and Work

Account and content data must connect to the document system rather than live
only inside a phone session or plugin cache:

```text
Platform account / post / comment / message / metric
  -> plugin connector sync
  -> social_platform_account / social_post_publication /
     social_metric_snapshot / external_message_thread
  -> relation to Content Growth, Merchant Network, Route/AR Experience,
     Creator Outreach, or Rewards/Inventory documents
  -> WorkItem when follow-up is needed
  -> assigned Organization Actor such as Content Growth Agent or Merchant Ops Agent
```

Examples:

- A Xiaohongshu note about the Jinxianmen AR effect gains unusual saves and
  comments. The plugin syncs a `social_metric_snapshot`, links it to the
  campaign and route document, and the Content Growth Agent can create a
  retrospective or next-post WorkItem.
- A comment asks where to buy the bracelet. The plugin records an
  `external_message_thread` summary and evidence ref, then routes a WorkItem to
  Content Growth or Merchant Ops to reply from the approved merchant/store
  knowledge document.
- A paid promotion is prepared for a high-performing note. The plugin may
  create a draft promotion action and evidence bundle, but Finance/Approval
  owns spend authorization before the external platform is charged.

The UI should render these plugin-provided facts through standard views first:
account table, content calendar, post performance table, inbox/follow-up
queue, and Agent detail gateway panel. A custom page is justified only when a
stable operating surface needs to combine campaign, post, metrics, inbox,
WorkItem, and actor context in one reviewable layout. The fallback remains the
standard Documents and Views over the same typed records.

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
  or Content Growth Agent to show gateway inbox summaries, account/campaign
  state, current WorkItems, maintained Docs, tools, skills, and permission
  boundaries.
- The Docs UI should show merchant-facing knowledge and linked WorkItems, but
  most edits should still be performed through CLI/skills by Agents.
- The Work UI should show gateway-created WorkItems by business line,
  milestone, work type, source document, accountable owner, and assignee.
- GitHub PRD/source sync is another gateway-like observation path: it observes
  software product truth and creates source snapshots or review WorkItems, but
  it does not overwrite commercial truth.
- Plugin view extensions may contribute panels or saved Views, but they must
  render Company OS records and declared projections. They do not get direct
  authority to mutate Docs, Work, Organization, Finance, or external accounts.

## GitHub connector priority

GitHub is the first priority connector for AgentOS dogfood because this
repository must run its own development through Company OS. Development
WorkItems need a reliable bridge to issues, branches, pull requests, reviews,
checks, previews, deployments, releases, and repo-hosted software PRDs.

The GitHub connector should be implemented as a plugin package, not as a pile
of repository-specific commands inside Company OS core:

```text
github plugin
  -> Skill: how Agents use GitHub evidence safely
  -> Transport: existing gh/git first; MCP or plugin CLI only when useful later
  -> Connector: gh/API/webhook/local observation sync into Company OS records
  -> View extensions: WorkItem delivery panel, PR/review/check table,
     Agent detail development queue, Docs source mapping panel
```

The first implementation can use local Git plus the existing `gh` CLI for
issues, pull requests, reviews, checks, and repo metadata. A dedicated MCP
server or plugin-owned CLI is not required for the first slice because the
missing product capability is synchronization and projection into Company OS,
not the ability for an Agent to operate GitHub at all. The implementation
should still preserve the same plugin manifest/action/connector/view boundary
so webhook delivery, GitHub API polling, or an MCP tool can replace or augment
`gh` later without changing Company OS truth.

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

Minimum first GitHub connector records:

- `github_repository`: owner/name, default branch, source/delivery role,
  linked Project Binding, responsible Actor, and sync policy;
- `github_issue_ref`: issue number/URL/state/labels/milestone, linked
  Development WorkItem, and source/sync status;
- `github_pull_request_ref`: PR number/URL/branch/base, author, review state,
  merge state, linked WorkItem, and evidence refs;
- `github_check_snapshot`: workflow/check name, status, conclusion, commit SHA,
  collected time, and linked PR/WorkItem;
- `product_doc_source` and `product_doc_snapshot`: existing Docs source-sync
  records for repo PRD/design/architecture files.

Minimum first views:

- Development WorkItem delivery panel: issue, branch, PR, checks, review,
  preview/deployment evidence, and acceptance refs;
- GitHub connector queue: unsynced observations, conflicts, stale PRDs, and
  WorkItems missing delivery refs;
- Agent detail development panel: assigned development WorkItems and their
  GitHub delivery state;
- Docs source mapping panel: repo PRD files mapped into Company OS Docs with
  last sync evidence.

## Current implementation status

| Capability | Status |
| --- | --- |
| Docs/Work/Org operating substrate (Finance parked — see issue #323) | partial, with dedicated CLI and Store-live projections |
| GitHub/local repo source sync into Docs records | implemented for local worktree observation |
| Social content gateway plugin contract | product contract; Store-backed TypedRecords/WorkItems are dogfood-ready; plugin manifest/action/connector/view implementation remains next |
| Xiaohongshu phone readiness check | implemented as read-only core bootstrap; local device can be inspected through ADB when the Human authorizes the session |
| Douyin phone readiness check | implemented as read-only core bootstrap; local device can be inspected through ADB when the Human authorizes the session |
| WeChat Channels readiness check | implemented as read-only core bootstrap; local device can be inspected through ADB when the Human authorizes the session |
| Social publish/upload/message/metrics/profile/promotion plugin actions | planned; should live in platform plugins via MCP/CLI adapters, not hard-coded core CLI |
| Plugin view extensions for account, content calendar, performance, inbox, and Agent detail panels | planned |
| WeCom gateway schema/API/CLI | planned |
| Service actor modeling for gateways | planned; current Org CLI v1 covers human/agent/unit/membership admin authoring |
| Gateway event inbox in Agent detail workspace | planned |
| Merchant Ops Agent scoped Docs answering | planned |

The current Wanchengwanling dogfood Store has created the canonical WorkItem
`work-wcw-agentos-wecom-gateway-v0` from
`document-cli-11-agentos-dogfood-external-gateway-agentos` to implement this
slice.

## Naming: provider agent-gateway vs external gateway

The term "gateway" appears in two distinct domains and must not be conflated:

| Term | Scope | Description |
| --- | --- | --- |
| **Provider agent-gateway** | Execution substrate | The transport adapter that connects a provider (Kimi ACP, Codex App Server, Claude Agent SDK) to the Harness Host. It handles session lifecycle, tool dispatch, event routing, and provider-native protocol translation. It is an execution concern, not a Company OS concern. |
| **External gateway** (this document) | Company OS | An intake and delivery adapter for external business channels (WeCom, GitHub, Xiaohongshu, email, payment systems). It normalizes external events into Company OS records without becoming authority. |

The provider agent-gateway is governed by the execution foundation contracts
([ADR 0032](../integration/native-session-storage.md),
[agent-integration-model.md](../agent-integration-model.md)).
The external gateway is governed by this document and the
[Gateway plugin operator contract](skill-contracts.md#gateway-plugin-operator-contract).

## Kimi ACP integration envelope

Kimi members connect to the Harness Host through the `kimi_acp` persistent
bidirectional mode. The integration envelope is:

- **Transport:** Kimi ACP (Agent Communication Protocol) — the provider-native
  bidirectional session protocol. The Harness Host opens and maintains the ACP
  connection; Kimi's native runtime drives turn execution within it.
- **Host Inbox hooks:** The Host delivers correlated TeamMessages,
  PendingInteractions, and control signals (interrupt, close, reopen) through
  the ACP session. The Kimi member does not poll a separate mailbox or
  message-queue endpoint.
- **cwd skill discovery:** The Kimi member resolves its working directory
  through the standard `member worktree_ref` > TeamRun `execution_root` >
  binding `project_root` chain. Skills are discovered from the normal
  `.agents/skills/` and `skills/` roots under that cwd. No plugin command or
  separate skill registry is involved.
- **No plugin command:** The Kimi integration does not use a plugin-owned CLI
  or a dedicated plugin transport. The Harness core handles ACP session
  management; the Kimi provider handles tool execution and skill loading within
  its own native runtime. Skills are project-scope files discovered by the
  normal Kimi Code skill loader, not by a Harness plugin.

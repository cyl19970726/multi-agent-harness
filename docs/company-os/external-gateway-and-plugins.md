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

## Current implementation status

| Capability | Status |
| --- | --- |
| Docs/Work/Org/Finance operating substrate | partial, with dedicated CLI and Store-live projections |
| GitHub/local repo source sync into Docs records | implemented for local worktree observation |
| WeCom gateway schema/API/CLI | planned |
| Service actor modeling for gateways | planned; current Org CLI v1 covers human/agent/unit/membership admin authoring |
| Gateway event inbox in Agent detail workspace | planned |
| Merchant Ops Agent scoped Docs answering | planned |

The current Wanchengwanling dogfood Store has created the canonical WorkItem
`work-wcw-agentos-wecom-gateway-v0` from
`document-cli-11-agentos-dogfood-external-gateway-agentos` to implement this
slice.

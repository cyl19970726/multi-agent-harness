# Agent Team page family — administrative-model visual review

Status: implementation decision for issue #444
Authority: `docs/mental/agent-firm-mental-model.md`, current schemas and
authenticated RoleViews override generated concept text.

The complete visual and implementation contract, including stable links to all
seven reference images and per-image keep/adapt/remove/add decisions, is
[visual-product-spec.md](visual-product-spec.md). This shorter review remains a
decision summary only.

## Frontend doctrine

The page family exposes one Company through three distinct concerns without
merging their authority:

- Organization identifies the durable `AgentMember`, the flat Mission-owned
  `AgentTeam`, and the explicit Host Agent.
- Execution binds a current `MemberRun`, native provider session and writable
  Workspace to exact Work responsibility.
- Knowledge, evidence, gates and Mission Log remain linked context. They do not
  become another Work lifecycle or occupy the primary workspace by default.

The primary surface follows the operator's task. Team Workspace prioritizes
shared Work; Agent Conversation prioritizes the live conversation; Host
Console prioritizes decisions and runtime control. A context rail is not a
required decoration.

## Generated-concept review

| Concept region | Decision | Product reason |
| --- | --- | --- |
| Global product rail | Reuse | It preserves Company navigation and page identity. |
| Team header and compact pressure | Keep, reduce height | Team, Mission relation and current pressure are first-viewport facts. Project/workspace/debug provenance moves to disclosure. |
| Works board | Revise | Render canonical `open`, `active`, `review`, `closed`. `Assigned` is an Open subgroup; `blocked`/`on_hold` are conditions; Closed exposes `accepted`, `cancelled` or `failed`. The board is a projection, never another state machine. |
| Activity | Keep | One source-labelled stream combines durable Message/Work/runtime facts. Provider-native activity is read on demand and never mirrored. |
| Members roster and portraits | Keep | Portraits carry durable identity and aid scanning. Every row keeps `AgentMember`, `MemberRun` and native-session facts separate. |
| Permanent Team context rail | Remove | Mission id, Team id, attempt and provenance repeat the header and consume the primary canvas. Expose them through a Context disclosure and entity links. |
| Selected-member mini-card beside Members | Remove | It duplicates the selected roster row and the full Agent Conversation destination. Selection opens the conversation workspace directly. |
| Agent Conversation left navigator | Keep | Host Agent and current Members are addressable conversation targets. Host is a real Agent identity and is never fabricated as a MemberRun. |
| Agent Conversation center stream | Keep as P0 | This is the primary MemberRun collaboration surface: authored Messages, source-labelled durable activity, on-demand native activity and a sticky composer. |
| Agent Conversation right rail | Conditional and reduced | Show only current Work, current execution binding and currently allowed controls. Hide the rail when all three are absent. Full identity/configuration, Mission Log, provenance, historical runs and evidence inventories use deep links or Context disclosure. |
| Host Console embedded under Team tabs | Replace | Host Console is a distinct authenticated authority surface, not a second dashboard appended below shared Team truth. |
| Large KPI/stat cards | Remove | Counts that do not change a decision belong in compact pressure rows or filters. Empty cards are not visual value. |
| Full Member Profile | Retain as separate page | Durable identity, memberships, capabilities and configuration are administrative facts, not the live conversation. |

## Mental-model corrections to the concepts

1. Work is one native responsibility object (`TeamWork` is its contextual
   name). Company Work is read-only aggregation.
2. Work lifecycle is three independent axes:
   `phase=open|active|review|closed`,
   `condition=normal|blocked|on_hold`, and
   `resolution=accepted|cancelled|failed` for Closed only.
3. Message, WorkDelivery, MessageDelivery, runtime, provider completion, gate
   evaluation and Work acceptance remain different facts.
4. `AgentMember` is durable identity; `MemberRun` is run-scoped coordination;
   the provider-native session is execution truth. A runtime process does not
   grant membership or Work authority.
5. Team topology is flat and Mission-Team identity is one-to-one. Cross-Team
   responsibility uses `WorkDelegation`, not nested Teams.
6. Host Agent is selectable for conversation but is not synthesized into the
   Member roster. Host-only control comes from `HostConsole.allowed_actions`.
7. Ordinary Message, current-turn Steer, Interrupt, Close and Reopen are
   separate server-authorized operations. The browser never upgrades one into
   another.

## Page family and priority

| Priority | Page | Primary surface | Secondary context |
| --- | --- | --- | --- |
| P0 | Team Workspace / Works | shared Work phase projection | on-demand Team context and selected Work |
| P0 | Agent Conversation | selected Host/Member conversation and live native activity | conditional Work/execution/control rail |
| P0 | Team Activity | source-labelled Team timeline and Lead Inbox pressure | filters and selected relation |
| P1 | Members | identity/capacity roster | opens Agent Conversation or Full Profile |
| P1 | Host Console | Lead decisions, Supervisor/runtime and allowed controls | Mission/Node provenance on demand |
| P1 | Full Member Profile | durable identity, memberships and configuration | execution history deep links |

## RoleView and API gaps

- `TeamWorkspace` already provides the shared members, messages, durable
  activity and Works needed for Host/Member conversation filtering.
- Provider-native MemberRun activity is available only through the existing
  read-on-demand native-activity endpoint. The UI must show unavailable,
  missing and incompatible states instead of substituting durable activity.
- `TeamWorkspace.team.host_agent_id` identifies the Host but does not project a
  display summary. Until that projection exists, use the exact id and a Host
  identity treatment; do not fabricate a MemberRun, provider or model.
- `MemberWorkbench` exposes only unread messages and lacks a complete
  conversation/activity projection. The Host-facing Agent Conversation must
  therefore compose authenticated TeamWorkspace truth, while exact-self
  MemberWorkbench actions remain exact-self only.
- Steer/current-turn capability is not a generic message action. Do not render
  an enabled Steer control until an allowed action and safe-point projection
  exist.
- PendingInteraction resolution and pending TeamMemberCloseRequest visibility
  are not present in the current bounded RoleViews. Keep them as explicit
  follow-up server projections/actions; do not infer them from runtime state.
- Full MessageDelivery ACK/reconciliation remains separate from reply. The
  conversation may inspect projected recipient/status/version/receipt lineage,
  but never authors an ACK as another actor.
- TeamWorkspace currently returns one bounded page with no cursor. Large-Team
  virtualization and restored cursor/scroll acceptance remain unshipped until
  the server exposes a real pagination contract.

## Responsive contract

- Desktop: 240–256px conversation navigator, dominant center stream, optional
  260–288px contextual rail. Center expands when context is empty.
- Tablet: conversation navigator becomes a sheet; contextual rail becomes a
  Context sheet; center stream and composer remain primary.
- Mobile: one stream, compact selected-agent header, Agents and Context
  buttons, sticky composer, bottom sheets with focus restoration. No horizontal
  Kanban or permanently visible side rail.

## Acceptance differences from the generated images

Generated images are direction, not browser evidence. Implementation will
intentionally omit invented provider/model values, fake gate counts, decorative
provenance cards and unsupported controls. It will use real project portraits,
exact ids/revisions, current RoleView freshness, explicit disabled reasons and
real long content. These are product-truth improvements, not visual-fidelity
defects.

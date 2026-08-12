# Team Workspace interaction contract

## Context

- Parent object: one Mission-owned AgentTeam and selected AgentTeamRun.
- Primary journey: identify pressure, inspect or act on Work, follow typed
  activity, answer a correlated message, inspect a member, and return without
  losing Team context.
- Covered viewports: 1440x1000, 900x1180, 390x844 and 320x720.
- Covered states: populated active/pressure, useful empty, loading, last-good
  stale, error, completed run and unavailable Supervisor/native session.

## Hotspots

| # | Object | Kind | Destination/action | Preserved context | Focus result |
| --- | --- | --- | --- | --- | --- |
| 1 | Mission | link | `surface=missions&mission=<id>` | space, project, TeamRun | Mission heading |
| 2 | Works tab/filter | control | change URL-backed Team view/filter | Mission, TeamRun, selected member | active tab/filter |
| 3 | Work card/row | control | open desktop side drawer or mobile sheet | view, filters, scroll anchor | drawer/sheet heading |
| 4 | Work action | control | server-provided Role Action | selected Work and exact revision | success/error status, then refetched Work |
| 5 | Activity message | control/link | select thread or linked Work | Team, filters, message lineage | message/thread heading |
| 6 | Reply | control | correlated authenticated Role Action | source message correlation/causation | composer, then sent row |
| 7 | Host/Member identity | control | `conversation=host:<id>` or `conversation=member:<id>` | Mission, TeamRun, filters | Agent Conversation heading |
| 8 | Context disclosure | control | compact disclosure / tablet sheet / mobile sheet | all URL state | first context heading |
| 9 | Supervisor/runtime | display/control | inspect current generation and allowed controls | TeamRun | selected runtime fact/action |
| 10 | Full Member profile | link | `memberRun=<id>` without Team conversation selection | space, project, durable AgentMember | Member profile heading |
| 11 | Conversation composer | control | authenticated fixed-recipient Message action | selected Agent, related Work, reply lineage | sent row after refetch |

## Scroll owners

| Viewport | Region | Owner | Sticky/fixed elements | Reachability assertion |
| --- | --- | --- | --- | --- |
| 1440x1000 | primary Works/Activity/Members | center work surface | Team header/tabs; composer only in Activity | final item and selected drawer controls reachable without body clipping |
| 1440x1000 | Agent Conversation | agent navigator + center stream + conditional fact rail | center header and compact composer | final event and every real control reachable; absent facts consume no empty column |
| 900x1180 | page | primary surface | compact header/tabs | context follows inline or opens sheet; no hidden gate/pressure |
| 390x844 | page | one document flow | compact tabs; collapsed Activity composer | grouped Work list, final content and composer remain reachable |
| 320x720 | page | one document flow | no fixed element may cover content | `scrollWidth === clientWidth`; 44px actions remain usable |

## State transitions

| Trigger | Start | Pending | Success | Failure | Durable effect |
| --- | --- | --- | --- | --- | --- |
| RoleView initial read | skeleton with identity shell | loading status | current populated/empty page | actionable authenticated error | none |
| SSE invalidation/refetch | last-good view | stale badge, actions disabled | newer sequence replaces view | last-good retained with retry/error | none |
| Open Work/member/context | visible source | immediate selection feedback | URL/drawer/page updates | source remains selected | none |
| Work/message/member action | exact allowed action | control disabled + progress | authoritative refetch and status | typed CAS/auth/recovery error | canonical server mutation only |
| Reply | source message selected | composer pending | correlated message appears after refetch | draft and source retained | canonical Message/Delivery |
| Select Agent | Team view or another conversation | immediate selected portrait | URL-owned conversation and source stream update | prior selection remains | none |
| Load native activity | Member conversation | source-specific loading row | provider-native activity labelled read-on-demand | explicit unavailable/error row | none; never mirrored |

## Motion

- Drawer/sheet disclosure: opacity/transform, 150–200ms ease-out, only to
  explain hierarchy. Reduced motion removes transform and shortens transition.
- Tab/filter selection: color/border only; no fake progress.
- New invalidation data: no animated thinking or fabricated activity.

## Focus and keyboard

- Initial focus remains on the page heading after direct navigation.
- Tabs use semantic tab roles and arrow-key behavior.
- Every Work operation has a non-drag button path.
- Enter/Space opens rows and disclosures; Escape closes drawer/sheet/composer
  disclosure and restores focus to its trigger.
- Status and CAS/conflict errors use an announced live region.
- Context and long Activity/conversation regions are keyboard-scrollable with visible focus.
- Agent list selection is keyboard operable; mobile sheets restore focus to
  their trigger and the composer never traps navigation.

## Browser journeys

| Id | Fixture/route | Actions | Assertions |
| --- | --- | --- | --- |
| content-reachability | populated store-live Team | open each tab, scroll longest region | first and final content reachable; no overlapping composer |
| entity-deep-link | populated Team | open a member and linked Mission | exact ids in URL and headings; no inferred join |
| agent-conversation | populated Team | select Member, send message, select Host, return to Team | fixed recipient, source labels, Host read-only boundary, URL selection preserved |
| return-context | populated Team | Browser Back and explicit return | tab, filters, TeamRun, selected Work/member and scroll anchor preserved |
| keyboard-path | populated Team | tabs -> Work -> non-drag action -> close | same result as pointer; focus restored |
| responsive-path | all four viewports | open Work, context and composer | correct drawer/sheet/inline transformation; no overflow |
| motion-policy | desktop/mobile | repeat disclosure with reduced motion | no non-essential motion |
| authenticated-host-action | store-live Host | execute one safe message or Work action | exact server actor/version/idempotency; refetch shows result |
| authenticated-member-read | store-live Member | open exact-self MemberWorkbench | Host cannot impersonate Member; authorization failure is explicit |
| stale-refetch | simulated invalidation/read failure | invalidate then fail refetch | last-good remains, provenance stale, actions disabled |

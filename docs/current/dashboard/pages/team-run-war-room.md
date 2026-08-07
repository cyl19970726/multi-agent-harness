# Agent Team Workbench Page Spec

```text
status: implemented baseline; Works visual and responsive closure in progress
owner_role: product-design
canonical_for: one standalone or Mission-scoped AgentTeamRun
route_or_surface: Agent Teams -> TeamRun
architecture: ADR 0025 retained runtime contracts + ADR 0034 lifecycle +
              ADR 0037 collaboration + ADR 0044 supervision/typed mail +
              ADR 0050 Works/Message boundary
```

## User Problem

The Host and Human need one surface to understand and steer a living Agent
Team: what Work exists, who owns it, what is ready or blocked, what needs
review, what questions need
answers, which native sessions can be resumed, and what evidence has arrived.

The page must remain useful when the same TeamRun spans several Host-plan Waves.

## Canonical Semantics

Required data:

- independent `AgentTeam` definition, explicit Team Lead, and editable member
  identities;
- `AgentTeamRun`, optional `mission_id`, optional legacy `wave_id`, status,
  previous run, host/runtime facts, and outcome;
- `MemberRun` identity, role, provider/model, status, capability profile,
  worktree, and native-session binding;
- current `TeamSupervisorLease` generation, heartbeat, owner locator,
  provider-transport/reconnect state, and Close latch;
- `Work`, `WorkOperation`/`WorkEvent`, Work
  owner/readiness/claim/review/parent-child state and `WorkDelivery`
  claim/provider receipt/failure/invalidation;
- typed Message sender and recipients, optional Work relation, conversational
  correlation, pending interactions, controls, artifacts, and checks;
- `AgentMessageRoute` when stable Agent Inbox mail was explicitly routed to a
  participating MemberRun;
- provider-native activity read on demand, clearly labeled by source and
  availability.

Harness does not mirror provider transcripts, tool calls, commands, file
events, turns, or thinking. A provider `completed` lifecycle update is not an
answer, approval, or semantic result.

The TeamRun may be standalone or linked to a Mission. In the primary
Mission-scoped path it is not owned by one Wave. Wave context explains how the
Host is currently using the team.

The Host Agent that created and coordinates the team is the Team Lead. The page
must show that identity separately from the member roster. `host` means the
current Host Agent. Lead messages, Work changes, composition changes, and
acceptance decisions are control-plane actions; they do not create an implicit
Lead `MemberRun`. If the Lead also executes a lane, that requires an explicit
member with its own native-session binding.

## Desktop Layout

Use the shared Workbench shell with the compact execution rail, a primary Works
surface, Activity conversation, Members capacity, and flexible context modules.

```text
+----------------------+--------------------------------------+------------------+
| Compact exec rail    | Team header                          | Mission context  |
|                      | definition · Lead · run · actions    | Current Wave     |
|                      +--------------------------------------+ Selected member  |
|                      | Works | Activity | Members           | Runtime          |
|                      | Assigned · Unassigned · Review      | Artifacts        |
|                      +--------------------------------------+                  |
|                      | Kanban/list or selected activity     |                  |
|                      | Work drawer / group conversation     |                  |
+----------------------+--------------------------------------+------------------+
```

`Works` is the default view. `Owned by me` appears only for a current actor
bound to a participating MemberRun; Host/Operator views use `By owner`. It
offers Unassigned, Assigned, Blocked, Review, and All filters; Open, Assigned,
In progress, Review, and Done Kanban columns; and an optional dense list.
Blocked is a canonical Work state rendered with explicit red/amber pressure in
the active-work region rather than a sixth ownership lane. Cards expose owner portrait, readiness,
criteria preview, blockers, child progress, source WorkItem, unread discussion,
and update time. Kanban is a projection over Work, never separate state.

Every Host/Member mailbox is computed from TeamMessage recipients and delivery
records. It is a read-model projection, not a new stored mailbox object. The
Host mailbox is visible even though Host is not a fabricated MemberRun. Mailbox
selection filters sent/received conversation; Member portraits and names open
Member Focus. A non-modal details drawer is not a replacement for the full page.
Every row renders typed author → recipient identity. A Dashboard Operator and
the Host Lead remain visually and structurally distinct, and neither can
impersonate a Member.

The selected view tabs and source-aware Activity stream precede any expanded
mailbox detail. Lead Inbox is a pressure disclosure and filter, not a large
standalone block that can push the first conversation below the viewport.

`Activity` is one source-aware timeline:

- WorkEvents, WorkDeliveries, authored messages, pending interactions, controls,
  and outcomes;
- ephemeral provider-native tool/command/chat/turn activity when available;
- Work submissions, artifacts, and checks;
- explicit “native session unavailable” states instead of invented history.

The page is a joined read model, not a transcript database. Native activity is
read on demand and remains rebuildable.

Tool icons are meaningful and consistent; provider and member avatars never
replace status or source labels.

Participant, Work/event/message, and text-search filters combine locally
without mutating coordination truth. The default projection prioritizes Work
assign/claim/review changes, questions, answers and linked discussion; the complete durable record
remains one click away. Large message bodies use the safe shared Markdown
renderer rather than displaying raw Markdown syntax.

## Context Modules

1. **MissionCompact** — optional Mission relation and open-Mission action.
2. **CurrentHostPlan** — selected/latest Wave context excerpt for orientation;
   never claims runtime ownership.
3. **SelectedMember** — identity, active/queued Works, capability, message, steer,
   interrupt, resume, and open-member actions supported by the real adapter.
4. **Runtime** — worktree, native session id, provider mode/version,
   permission/budget, current Supervisor generation, transport/reconnect
   health, and honest control availability.
5. **Artifacts** — explicit files/checks/evidence with open/download actions.

The Host mailbox and conversation pressure rows form the **Lead Inbox** for
member-authored questions and coordination. Blocked and Review queues come
from Works and link their discussion. Every item shows sender, Work when
present, conversational correlation, delivery/ACK state, and the
responsible next action. Answering reuses the source correlation, records the
source message as causation, and acknowledges the source delivery.

Conversation rows expose reply lineage and optional Work relation. WorkEvent
history separately shows who assigned, claimed, blocked, submitted, requested
changes, accepted, released, or cancelled Work.

## Actions

- Create, assign, claim, start, block, submit, request changes, accept, release,
  cancel, or delegate Work through canonical Work actions.
- Host may assign and accept, but may claim/execute only through an explicit
  Lead MemberRun. Each mutation renders the actual actor plus delegated Host
  authority where present, requires the expected Work version, and exposes
  claim/version/reconciliation conflicts rather than retrying invisibly.
- Message the whole team or one explicit member. The composer distinguishes a
  new conversation from a reply and may link the selected Work.
- Make it clear that Host-authored coordination comes from the Team Lead;
  Human/operator authorship remains separately attributable where supported.
- Add, rename, deactivate, steer, interrupt, explicitly close, or reopen a
  member where the selected provider mode honestly supports it. Interrupt ends
  one turn; Close ends one runtime generation; Reopen resumes the same
  MemberRun/native session; Deactivate retires permanently.
- Roll up pending `TeamMemberCloseRequest` rows in the Team header so a lost
  Supervisor connection cannot make an accepted Close disappear from view.
- Inspect WorkDelivery claim/receipt/failure lineage, authored Message
  delivery/ACK lineage, and answer PendingInteractions.
- Answer Lead Inbox items with inherited correlation and causation. The
  Dashboard may author Host/operator messages; it never impersonates a member.
- Open Mission, current Wave context, Member Focus, artifact, or native-session
  summary.
- Complete or stop the TeamRun only through a real acknowledged lifecycle
  transition.

Wave creation/advance occurs from Mission Canvas. It never implicitly stops or
restarts this TeamRun.

## States And Responsive Behavior

- No members: explain whether the stable team definition is empty or run
  materialization failed.
- Starting: show admission/runtime acquisition without calling it working.
- Blocked/question: attach pressure and action to the exact record.
- Provider/session unavailable: retain coordination and show the missing
  source.
- Completed/stopped: coordination history is read-only, but any still-live
  member runtime retains an explicit Host Close action. Resume/new-run choices
  follow the provider/session contract; do not imply a Mission or Wave
  completed.
- **Works responsive surface:** desktop uses Kanban or a windowed dense list;
  tablet uses compact columns or grouped list; mobile uses grouped status lists
  and a Work bottom sheet, never horizontal Kanban. Drag/drop is optional and
  never the only mutation path.
- **Activity responsive surface:** preserve one source-aware timeline and its
  composer; mailbox filters may scroll horizontally when keyboard accessible;
  context moves into a sheet/bottom sheet.
- **Members responsive surface:** desktop uses a factual capacity table/grid;
  tablet/mobile use a compact capacity list. Capacity means addressability,
  active/queued/blocked-review/eligible-ready counts, plus separately labeled
  provider-account capacity—never a synthetic percentage.
- **Truthful team summary:** Active turns is the count of Members whose current
  runtime status proves a running turn; Ready members is the eligible accepting
  count over current participating members; Queued Works, Needs review and
  Blocked derive from current Works and Member runtime states. A concurrency
  denominator is omitted until the selected runtime ceiling is durable on the
  TeamRun. Provider-account capacity remains separately labelled per member;
  absent data means `not observed`, never `available`.
- Navigation preserves filters, selected member, scroll, Mission id, selected
  Wave id, TeamRun id, and project id across Team → Member → Team deep links.
- A canonical MCP Dashboard URL for a Mission-scoped run includes the current
  Host-plan Wave as cold-link navigation context. This does not attach runtime
  ownership to that Wave and may change when the Host advances its plan.

## Screenshot And UX Acceptance

Pre-Works execution-workbench images are legacy visual baselines, not evidence
that ADR 0050 is implemented. New acceptance uses distinct expected and actual
captures for Works, Activity, and Members at desktop `1440x1000`, tablet
`900x1180`, mobile `390x844`, plus a `320px` overflow check.

Works acceptance shows the shared shell, assigned/unassigned ownership with
portraits, Kanban/list, Mission/Wave orientation, and the selected Work drawer.
Activity acceptance shows source-aware rendered Markdown conversation, typed
sender -> recipient routes, events, delivery state, and composer. Members
acceptance shows factual capacity and runtime pressure. Across all views verify:

- member controls open the correct Member Focus and return without state loss;
- Work, mailbox, participant, event/message, and search filters preserve Team context;
- PendingInteraction answer, chat, steer, interrupt, Close, and resume states
  match real adapter acknowledgements;
- Work submissions, Markdown discussion and tool activity render with suitable icons and density;
- the same TeamRun remains visible after Mission Wave advance;
- empty, loading, error, unavailable-native-session, and long-stream behavior;
- actual screenshot against the approved expected reference.

The state matrix covers initial loading, partial-source failure, last-good stale
data, filtered empty, pending mutation, claim lost, version conflict, delivery
queued/uncertain/failed, busy member, crash/disconnect, closed, retired, and
Supervisor-generation change. A 1,000-Work/100-Member fixture proves stable
sorting, bounded DOM/windowing, visible totals/load-more, and restorable URL
state for view, filters, sort, selected Work, scroll anchor, Mission/Wave, and
cursor.

Accessibility requires semantic tabs, keyboard board/list and non-drag action
paths, focus restoration after drawers/dialogs, Escape handling, live-region
claim/conflict announcements, non-color status, reduced motion, 44px mobile
targets, accessible Markdown tables, zero serious/critical automated findings,
and manual VoiceOver journeys. Every Activity row shows avatar, textual actor
name/type, sender -> full recipient route, source/status, and absolute time on
disclosure; multi-recipient `+N` exposes the full accessible list.

## Explicit Boundaries

- A TeamRun is not a Standing Agent or OrgUnit.
- Work owns responsibility; Message carries conversation; Wave prose explains Host intent.
- Provider-native subagents are observations unless a real orchestrated
  lifecycle exists.
- A member-to-member message is allowed inside the same TeamRun and remains
  visible to the Lead. It is queued for the peer's next eligible round rather
  than interrupting the current turn.
- TeamRun completion does not advance a Wave; Wave advance does not complete a
  TeamRun.

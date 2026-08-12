# Agent Team Workbench Page Spec

```text
status: authenticated RoleView parity slice implemented by issue #444; deferred projections explicit below
owner_role: product-design
canonical_for: one Mission-owned AgentTeamRun
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

The issue #444 slice restores Works, Activity, Members, Agent Conversation and
the independent Host Console. It does not claim the complete future runtime or
large-Team contract: authenticated PendingInteraction resolution, pending
Close-request projection, Steer/Interrupt safe-point actions, full
MessageDelivery ACK control, and cursor-backed 1,000-Work/100-Member
virtualization remain deferred until server-built RoleViews/actions exist. The
browser never fills those gaps with snapshot joins or fake controls.

The page must remain useful while the same TeamRun spans several append-only
Mission Log judgments and replans. Runs created before ADR 0051 may also retain
read-only historical Wave navigation context.

## Canonical Semantics

Required data:

- one flat, Mission-owned `AgentTeam`, explicit Host Agent, immutable Node
  placement, and editable member identities;
- `AgentTeamRun`, required `agent_team_id`, `execution_node_id`, and
  `project_binding_id`, plus status, previous same-Team run, host/runtime facts,
  and outcome; Mission is derived through `AgentTeam.mission_id` and Wave is
  never a TeamRun ownership field;
- `MemberRun` identity, role, provider/model, status, capability profile,
  worktree, and native-session binding;
- current `TeamSupervisorLease` generation, heartbeat, owner locator,
  provider-transport/reconnect state, and Close latch;
- `Work`, `WorkOperation`/`WorkEvent`, Work
  owner/readiness/claim/review/parent-child state and `WorkDelivery`
  claim/provider receipt/failure/invalidation;
- typed Message sender and recipients, optional Work relation, conversational
  correlation, projected controls, artifacts, and checks; pending interactions
  remain a declared server-projection gap in the issue #444 slice;
- canonical `MessageDelivery` when AgentMember mail was addressed to a
  participating MemberRun;
- provider-native activity read on demand, clearly labeled by source and
  availability.

Harness does not mirror provider transcripts, tool calls, commands, file
events, turns, or thinking. A provider `completed` lifecycle update is not an
answer, approval, or semantic result.

Every TeamRun belongs to its required AgentTeam, and every AgentTeam belongs to
exactly one Mission. A selected historical Wave is navigation context only; it
never owns the TeamRun or accepts new Host judgment.

The Host Agent that created and coordinates the team is the Team Lead. The page
must show that identity separately from the member roster. `host` means the
current Host Agent. Lead messages, Work changes, composition changes, and
acceptance decisions are control-plane actions; they do not create an implicit
Lead `MemberRun`. If the Lead also executes a lane, that requires an explicit
member with its own native-session binding.

## Product Composition

Use the shared Workbench shell with the compact execution rail, a primary Works
surface, Activity conversation, Members capacity, and an Agent Conversation
workspace. Team context is an on-demand disclosure, not a permanent third
column. A right rail appears only inside Agent Conversation and only when it
has current Work, execution provenance, or authorized controls to show.
The active page composes authenticated `TeamWorkspace` shared truth with
authenticated `HostConsole` Host-only truth. It never reconstructs this page
from the global snapshot.

The implementation deliberately reuses the mature visual primitives that are
still valid under the current model: canonical member avatars, capacity rows,
the shared Works board and Work sheet, conversation/activity rows, composer,
authorized action panels, and the exact-self Member home. It does not restore
Wave-as-executor/gate UI, Assignment Message or legacy ACK paths, browser-side
authority joins/writers, provider transcript mirrors, parent/child Team
topology, or any second Team/Message/Delivery/Work model.

The Host Console is an independent Host-only surface. It is not appended below
the Team tabs. A full Member Profile is a separate deep link for identity and
history; it does not replace the live conversation workspace.

`Works` is the default view. `Owned by me` appears only for a current actor
bound to a participating MemberRun; Host/Operator views use `By owner`. It
offers Unassigned, Assigned, Blocked, Review, and All filters; the four
canonical phase lanes Open, Active, Review, and Closed; and an optional dense
list. Open may be grouped into Unassigned and Assigned without inventing an
Assigned phase. `blocked` and `on_hold` are orthogonal Work conditions rendered
as pressure, while Closed cards show `accepted`, `cancelled`, or `failed`
resolution. Cards expose owner portrait, readiness,
criteria preview, blockers, child progress, source TeamWork, unread discussion,
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

- WorkEvents, WorkDeliveries, authored messages, projected controls,
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

## Layout Contract

### Desktop — 1440x1000

```text
+----------+----------------------------------------------------------------+
| product  | Team identity · run · Supervisor · pressure · Context          |
| rail     +----------------------------------------------------------------+
|          | Works | Activity | Members                                     |
|          +----------------------------------------------------------------+
|          | primary board/list or source-aware timeline                    |
|          | selected Work drawer within center surface                     |
|          +----------------------------------------------------------------+
|          | Activity composer (only when Activity active)                  |
+----------+----------------------------------------------------------------+
```

- The center surface is visually dominant and owns its long-content scroll.
- Mission, runtime and provenance facts open from the compact Context
  disclosure; they do not consume a permanently empty right column.
- Works/Activity/Members tabs and current pressure remain above the fold.
- The selected Work uses a non-modal drawer inside the center surface; it does
  not replace entity deep links.

### Tablet — 900x1180

```text
+--------------------------------------------------------------------------+
| compact shell · Team identity · Supervisor/pressure · context disclosure |
+--------------------------------------------------------------------------+
| Works | Activity | Members                                               |
+--------------------------------------------------------------------------+
| compact columns or grouped Work list / source-aware timeline             |
| selected Work inline panel                                               |
+--------------------------------------------------------------------------+
| Mission Log · runtime · member context inline or accessible side sheet   |
+--------------------------------------------------------------------------+
```

- Product rail collapses; context never disappears silently.
- One page scroll owner is preferred. A side sheet traps focus and restores it
  to its disclosure control.
- Work mutations retain non-drag controls and full disabled reasons.

### Mobile — 390x844 and 320x720 overflow gate

```text
+--------------------------------------+
| compact Team header · pressure       |
| Supervisor status · context button   |
+--------------------------------------+
| Works | Activity | Members           |
+--------------------------------------+
| grouped status list / timeline       |
| 44px row and action targets          |
+--------------------------------------+
| collapsed composer when applicable   |
+--------------------------------------+
| Work/context bottom sheet on demand  |
+--------------------------------------+
```

- No horizontal Kanban. Works become grouped status lists.
- Context and selected Work use bottom sheets with Escape/Back and focus
  restoration. The Activity composer starts collapsed and never covers rows.
- At 320px, long ids truncate or wrap intentionally and
  `scrollWidth === clientWidth`.

### Scroll and state ownership

- Initial loading shows an identity-preserving skeleton.
- Refetch preserves last-good content, marks provenance stale and disables
  mutations until a current authenticated view arrives.
- Empty Team state explains member/runtime readiness and offers only actions
  returned by HostConsole; it is never a blank main canvas.
- Partial-source, unavailable Supervisor/native session, CAS conflict and
  authorization failure remain distinct visible states.

## Agent Conversation Workspace

Selecting the Host Agent or a Member keeps the user inside the Team and opens
a Codex-like conversation workspace:

```text
+----------+----------------+--------------------------------+---------------+
| product  | Team agents    | selected Agent conversation    | current facts |
| rail     | Host + Members | source-labelled event stream   | only when any |
|          | portrait/state | provider-native read on demand | fact/action   |
|          | pressure       | sticky Message composer        | exists        |
+----------+----------------+--------------------------------+---------------+
```

- The center conversation is the dominant surface. It combines authored Team
  Messages, relevant Work activity, and read-on-demand provider-native
  activity without copying native transcripts into Harness state.
- Selecting a Member fixes the authenticated Message recipient. Ordinary
  Message and Steer remain separate actions.
- Selecting the Host uses exact `host_agent_id`. Until an authenticated
  operator-to-Host authoring action is projected, the Host conversation is
  honestly read-only; the client must not fabricate a Host MemberRun.
- The conditional right rail contains only selected/current Work, exact
  execution provenance, a full-profile deep link, and server-authorized
  controls. If none exist it is absent.
- On tablet the agent navigator and contextual facts become sheets. On mobile
  the navigator opens from an Agent-list control, the center remains one
  scrollable stream, and the compact composer never covers the final row.

## Context Disclosures

1. **MissionCompact** — optional Mission relation and open-Mission action.
2. **CurrentHostJudgment** — latest append-only Mission Log entries for
   orientation. Historical Wave context may be shown read-only; it never owns
   runtime or accepts new judgment.
3. **SelectedMember** — lives in Agent Conversation rather than a duplicate
   Team-level mini-card; it shows identity, current Work, exact execution
   provenance and actions supported by the real adapter.
4. **Runtime** — worktree, native session id, provider mode/version,
   permission/budget, current Supervisor generation, transport/reconnect
   health, and honest control availability.
5. **Artifacts** — explicit files/checks/evidence with open/download actions.

The Host mailbox and conversation pressure rows form the **Lead Inbox** for
member-authored questions and coordination. Blocked and Review queues come
from Works and link their discussion. Every item shows sender, Work when
present, conversational correlation, delivery/ACK state, and the
responsible next action. Answering reuses the source correlation, records the
  source message as causation. Reply and MessageDelivery acknowledgement remain
  separate authority operations; a browser reply never impersonates recipient
  ACK. Delivery rows expose recipient, status, version and provider receipt.

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
- Pending `TeamMemberCloseRequest` roll-up is deferred until a bounded
  authenticated HostConsole projection exists; Close availability must not be
  inferred from member runtime state in the meantime.
- Inspect projected WorkDelivery and authored Message delivery lineage.
  PendingInteraction answer and MessageDelivery ACK/reconciliation remain
  explicit follow-up server-action gaps.
- Answer Lead Inbox items with inherited correlation and causation. The
  Dashboard may author Host/operator messages; it never impersonates a member.
- Open Mission/Log context, optional historical Wave, Member Focus, artifact, or native-session
  summary.
- Complete or stop the TeamRun only through a real acknowledged lifecycle
  transition.

Mission judgment is appended through the canonical Mission Log contract. New
Wave creation/advance is retired by ADR 0051. Reading a historical Wave never
stops or restarts this TeamRun.

## States And Responsive Behavior

- No members: explain whether the stable team definition is empty or run
  materialization failed.
- Starting: show admission/runtime acquisition without calling it working.
- Blocked/question: attach pressure and action to the exact record.
- Provider/session unavailable: retain coordination and show the missing
  source.
- Completed/stopped: coordination history is read-only, but any still-live
  member runtime retains an explicit Host Close action. Resume/new-run choices
  follow the provider/session contract; do not imply the Mission closed.
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
- Navigation preserves filters, selected member, scroll, Mission id, optional
  historical Wave id, TeamRun id, and project id across Team → Member → Team
  deep links.
- A canonical Dashboard URL for a Mission-scoped run includes the Mission and
  TeamRun. It includes a historical Wave only when that row already exists and
  the user opened the Team from it; Mission Log revisions do not rewrite the
  run URL.

## Screenshot And UX Acceptance

Pre-Works execution-workbench images are legacy visual baselines, not evidence
that ADR 0050 is implemented. New acceptance uses distinct expected and actual
captures for Works, Activity, and Members at desktop `1440x1000`, tablet
`900x1180`, mobile `390x844`, plus a `320px` overflow check.

Works acceptance shows the shared shell, assigned/unassigned ownership with
portraits, Kanban/list, Mission/Mission-Log orientation, and the selected Work drawer.
Activity acceptance shows source-aware rendered Markdown conversation, typed
sender -> recipient routes, events, delivery state, and composer. Members
acceptance shows factual capacity and runtime pressure. Across the shipped
issue #444 slice verify:

- member controls open the correct Member Focus and return without state loss;
- Work, mailbox, participant, event/message, and search filters preserve Team context;
- chat and projected Close/resume states match real adapter acknowledgements;
- Work submissions, Markdown discussion and tool activity render with suitable icons and density;
- the same TeamRun remains visible after a new Mission Log judgment/replan;
- empty, loading, error, unavailable-native-session, and long-stream behavior;
- actual screenshot against the approved expected reference.

The shipped state matrix covers initial loading, last-good stale data, useful
empty, mutation-disabled stale projection, unavailable native activity and a
completed TeamRun with a still-live runtime. Partial-source failure, pending
interaction, pending Close request, active-turn Steer/Interrupt,
delivery-reconciliation and Supervisor-generation recovery remain dependent on
the deferred server projections above.

The full future contract covers filtered empty, pending mutation, claim lost,
version conflict, delivery queued/uncertain/failed, busy member,
crash/disconnect, closed, retired, and Supervisor-generation change. A
1,000-Work/100-Member fixture must prove stable
sorting, bounded DOM/windowing, visible totals/load-more, and restorable URL
state for view, filters, sort, selected Work, scroll anchor, Mission/optional
historical Wave, and
cursor. That virtualization case is not acceptance evidence for issue #444 and
must not be marked shipped until a bounded cursor RoleView exists.

Accessibility requires semantic tabs, keyboard board/list and non-drag action
paths, focus restoration after drawers/dialogs, Escape handling, live-region
claim/conflict announcements, non-color status, reduced motion, 44px mobile
targets, accessible Markdown tables, zero serious/critical automated findings,
and manual VoiceOver journeys. Every Activity row shows avatar, textual actor
name/type, sender -> full recipient route, source/status, and absolute time on
disclosure; multi-recipient `+N` exposes the full accessible list.

## Explicit Boundaries

- A TeamRun is not a Agent Membership or OrgUnit.
- Work owns responsibility; Message carries conversation; Mission Log prose
  explains current Host intent. Historical Wave prose is read-only context.
- Provider-native subagents are observations unless a real orchestrated
  lifecycle exists.
- A member-to-member message is allowed inside the same TeamRun and remains
  visible to the Lead. It is queued for the peer's next eligible round rather
  than interrupting the current turn.
- TeamRun completion does not close a Mission; appending Mission Log judgment
  does not complete a TeamRun.

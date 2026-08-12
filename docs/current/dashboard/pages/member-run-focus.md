# MemberRun Focus Page Spec

```text
status: authenticated Agent Conversation implemented; full profile retained
owner_role: dashboard
canonical_for: one autonomous MemberRun working within one AgentTeamRun
route_or_surface: Agent Teams -> TeamRun -> Agent Conversation -> Full profile
```

## User Problem

An operator needs to understand one agent's work without reconstructing it
from separate message, action, session, and evidence tabs. They need to answer
four questions in the first viewport:

1. What Mission/Team context and current Host judgment is this member serving?
2. Which Work does it own, what is queued, and under which boundaries?
3. What is it doing or waiting for now?
4. What output supports its contribution to the Mission and current Work?

The default selected-member experience is a focused, continuous Agent
Conversation: durable Harness coordination, relevant Works, on-demand native
provider activity and messages appear in one chronological presentation. It is
not a copied provider transcript or a duplicate Team Kanban. A separate Full
Profile deep link carries durable identity and expanded historical detail.

## Canonical Data And Semantics

The following is the full target contract. The issue #444 authenticated Agent
Conversation slice currently ships TeamWorkspace messages and delivery
lineage, all Works exactly bound to the selected MemberRun, current member
runtime/session summary, read-on-demand native activity, and HostConsole
allowed actions. PendingInteraction resolution, pending Close-request
projection, recipient ACK action, Steer/Interrupt safe-point state and the full
runtime/reconnect matrix remain server-projection gaps; the UI does not infer
them.

Full-contract required data:

- optional `Mission`, selected/current Wave context, and Host judgment/advance
  projection;
- parent `AgentTeamRun` and retry lineage;
- the selected `MemberRun`;
- current `TeamSupervisorLease` generation and control/reconnect state;
- current/queued/eligible `Work`, WorkEvent history, WorkDelivery receipts,
  criteria, ownership, blockers, child Works, results and evidence;
- `TeamMessage` with optional Work relation and conversational correlation;
- typed message actors, delivery claim, provider receipt, recipient ACK, and
  canonical MessageDelivery state;
- Harness-owned control/lifecycle facts, observed `DelegationRun`, artifacts,
  outcomes, and evidence/check references;
- `PendingInteraction` records attributable to this MemberRun, with exact
  provider options and Lead/Policy/Human routing;
- `NativeSessionRef`, native session availability/resume capability,
  runtime summary, provider/model, worktree, owned paths, permissions,
  budget/availability signals;
- selected execution driver, ephemeral native continuation state/condition,
  completion policy, and top-level Workspace lease health; and
- ephemeral `NativeActivityProjection` read from the provider session, plus a
  sanitized `member_activity` preview only when live data exists.

The latest Work projection is the sole run-scoped responsibility proof; a
Message or provider self-description does not replace it.

`AgentMember` is the stable organization-agent identity.
`MemberRun` is one participation of that identity in one TeamRun. Company
Organization may project the canonical AgentMember ActorRef but never owns the
runtime lifecycle or a second agent identity.

Thinking is a best-effort live preview: sanitized, TTL-bound, local to the
current project/session, never persisted, replayed, forwarded, or accepted as
evidence. On refresh or expiry it disappears rather than becoming a blank
historical event.

The projection must distinguish source and durability. WorkEvents,
PendingInteraction resolution, explicit outcome, control acknowledgement, and
Host Wave decisions are durable Harness records. Native chat/tool/command/file/turn
activity is read from the provider session and is rebuildable, non-evidence UI
state. Harness does not silently fall back to a mirrored history.

## Layout Contract

The desktop/tablet/mobile MemberRun Focus V3 set in
`../../design/execution-workbench-v3/`.
It predates Works and is a legacy visual baseline, not ADR 0050 product-truth
evidence. A new immutable expected/actual set must be registered before Works
is accepted. The older Workbench V2 image is historical baseline only.

### Desktop — `1440x1000`

Use the shared Workbench shell: compact product rail, a 240–256px Team agent
navigator, dominant central conversation, and a conditional 260–288px fact
rail. The central stream, not a tab bar, owns the page. When no current fact or
authorized action exists, omit the right rail and give that width to chat.

```text
+----------------------+--------------------------------------+------------------+
| Product rail         | Team agents | Member header + source-labelled stream |
|                      | Host Agent  | messages / Work / native read-on-demand |
|                      | Members     |                                      | facts |
|                      | portraits   |                                      | only  |
|                      | pressure    | Message this member… (sticky)         | when  |
|                      |             |                                      | useful|
+----------------------+--------------------------------------+------------------+
```

The header exposes identity, status, role, provider/model, and a compact
breadcrumb. It must not turn the center into an overview dashboard. The
composer remains visible when the member can receive messages; it identifies
the fixed recipient and permits an ordinary message. Steer remains a separate
server-authorized runtime control.

### Tablet — `900x1180`

- Keep a narrow/collapsed product sidebar and a full-width main stream.
- Team agents and contextual facts move into separate sheets; `Current Work`
  and exact execution provenance are the only default contextual facts.
- Header stays above the stream; the composer stays sticky at the bottom.
- A selected module opens without hiding the activity stream permanently.

### Mobile — `390x844`

- Use a compact top bar with back-to-Team, member identity/status, and a
  context button.
- Preserve one vertical stream and fixed composer; do not create separate
  Chat and Activity tabs.
- Agent navigation and contextual facts are separate bottom sheets. Current
  Work, exact execution provenance and allowed controls take priority.
- Long paths, IDs, and raw data truncate or disclose progressively; no
  horizontal page overflow.

## Conditional Context

The contextual rail is not a required third column. It appears only when at
least one selected/current fact or authorized action exists, and uses shared
density variants rather than page-specific decorative cards. Its order is:

1. **BoundWorks** — every Work bound to the selected exact MemberRun
   (`current_member_run_ref` / server-projected execution binding), with
   id/version, creator/owner, phase/condition/resolution,
   readiness, context,
   completion criteria, owned paths, permissions, blockers, child progress and
   applicable constraints. No single Work is called current unless a future
   server projection says so. Other durable AgentMember-owned Works are shown
   separately and never guessed to be the selected execution.
2. **RuntimeSummary** — exact MemberRun, provider/model/native-session binding, availability,
   resume compatibility, selected execution driver, continuation state,
   Team Supervisor generation/heartbeat, provider-transport and reconnect
   state, Close latch, worktree lease, permission posture, and actionable
   failure state. It is operational context, not the primary page.
   It also shows `MemberRun.provider_capacity` — account capacity state,
   account/source boundary, observed time, reset time, evidence source and
   confidence — as a row BESIDE adapter compatibility, never merged into it. A
   `blocked` member whose latest action is `provider_unavailable` reads as
   "not started, Work delivery still queued", not as a failed member. `unknown`
   renders as unknown; it must never render as healthy, and an absent snapshot
   renders as "not observed".
3. **AllowedControls** — only current server-projected actions with their
   disabled reasons. Message, Steer, runtime control and Work transition stay
   separate.
4. **FullProfileLink** — durable identity/configuration and broader history.
   Mission Log, Team summary, outputs and delegation inventories stay behind
   entity deep links instead of repeating the center conversation.

The first module group is labeled **Current Work (Member Goal)**. It is a
derived projection, not a Goal record: Work context and completion standard,
owned paths, member state, latest progress/blocker, and latest applicable
Steer. Missing inputs are shown as missing rather than inferred from provider
chat.

The label must not imply that Harness owns a Goal lifecycle. Show these as
separate rows:

```text
Work             durable · work-... · version
Execution        host-driven | provider-driven
Continuation     inactive | active | waiting | satisfied | unknown
Acceptance       pending | accepted | changes requested
```

When native continuation is active, disclose its bounded condition, observed
cycle/reason, timestamp, and confidence without copying reasoning. If two
top-level drivers or turns claim the same MemberRun/worktree, show a blocking
`Execution lease conflict`; never normalize it as ordinary parallel activity.

Modules are collapsible. First release uses system ordering; pinning or free
reordering is not a requirement.

## Actions

- Send a message, clarification, or review discussion directly to this
  member when it is addressable.
- **Steer** is a separate explicit action. Only that selection may inject into
  a currently active provider turn; ordinary Clarify/Review messages stay in
  the coordination queue. If the selected execution mode cannot steer the
  active turn, Steer is disabled with the reason. The operator may deliberately
  choose an ordinary queued Message, but the UI never converts one into the
  other.
- Link the current Work when replying about execution. A new conversation is
  visually distinct and never silently changes Work state.
- Render the selected typed author explicitly. Operator-authored messages remain
  Operator messages; only the bound provider session can author as this Member.
- Open the Work card, WorkEvent history and linked messages.
- Open the Team or selected Host-plan Wave without losing navigation context.
- Open an artifact, check, or provider session summary.
- Acknowledge a waiting/blocker signal where the message protocol permits it.
- Resolve a provider question, tool approval, or plan review when the current
  actor is allowed; same-turn resume is available only when the snapshotted
  execution-mode profile supports it.

Do not offer fake lifecycle control. Interrupt appears only when the provider
exposes cooperative turn interruption. Close is a separate Host-owned action:
it sends the selected adapter's real close/cancel protocol and must not be
presented as ordinary turn completion. Completion of the MemberRun is an
execution fact, not an implicit Wave advance.

Full-contract follow-up: render the latest `TeamMemberCloseRequest` beside
those controls once HostConsole projects it. `pending`
disables duplicate Close actions and remains visible across Supervisor restart;
`applied` is retained as lifecycle evidence.

For a bound Claude `claude_agent_sdk` session, Member Focus may expose **Open in
Claude Desktop** using `claude://resume?session=<native-session-id>`. This is an
explicit provider-owned import/view action, not a Harness resume or transcript
copy. The control must warn that Desktop is observation-only while Harness
drives the Member because simultaneous writers are not verified. Other provider
modes do not receive a fabricated Desktop target.

## Empty, Loading, And Failure States

- **No Work:** show `No active Work` prominently; preserve
  observed activity but do not infer ownership.
- **No coordination/native activity yet:** show the member's starting state and
  explain which source is empty.
- **Native session unavailable:** retain Harness identity, Work, outcome,
  and gate history; mark native detail `missing`, `stale`, or `incompatible`
  and offer reconnect/resume only if the mode supports it.
- **Member failed/blocked:** show the explicit failure or blocker action, its
  correlation when present, and the responsible next action; never fabricate a
  reason from status alone.
- **Supervisor disconnected/stale:** preserve durable mail and native-session
  locator, disable fake live controls, and offer reattach only through the
  current generation-acquisition path. Unclaimed mail stays queued.
- **Read/model error:** keep the last successful header/context state marked
  stale, show scoped retry, and do not replace the page with an empty shell.
- **Finished Work or TeamRun:** render history read-only and disable
  ordinary coordination when the member is no longer addressable. If the
  provider runtime is still live, Host Close remains available and explicitly
  explains that completion did not end the runtime.
- **Mutation conflict:** show pending mutation, claim lost, version conflict,
  delivery uncertainty/failure, and reconciliation-required states with the
  latest Work version and a non-destructive retry path. Never imply the
  Member-owned Work changed when the command failed.

## Screenshot Acceptance

The previous `member-run-focus--running-needs-you--desktop` case is a legacy
baseline. The Works contract adds new desktop/tablet/mobile cases in which:

- baseline, approval-pending expected candidate, implementation capture, and labeled comparison
  all use the registered fixture, route, and `1440x1000` viewport;
- first viewport visibly contains the Member header, a continuous mixed
  activity/chat stream, Work context, a Wave module, Team module, and
  sticky composer;
- a live preview, when fixture-provided, is visibly labelled `not saved`; it
  must not appear in stored activity after a refresh fixture;
- Work context appears before dependent result/evidence and exposes linked
  discussion;
- the implementation does not use the legacy Member drawer or
  Overview/Activity/Messages primary tabs;
- deviations from the approved image are recorded in
  `visual-contract.json`, not silently normalized by changing the expected
  image.

Pre-Works implementation evidence is complete only for the legacy baseline.
Works product truth remains pending until the new expected candidate is frozen,
the implementation is captured at exact viewports, and product/visual/browser
checks pass without mutating the expected image.

## Explicit Boundaries

- This page is for a `MemberRun`; durable identity links directly to its exact
  canonical AgentMember.
- It does not require or display a legacy dependency graph as the ownership model.
- Provider-native subagents remain observed delegation unless the harness owns
  their lifecycle.
- Provider-native subagents remain inside this member's responsibility and
  permission ceiling; they are not independent acceptance.
- TeamRun completion only says that one run ended. The Host separately records
  `accepted | revise | blocked` judgment or advances the plan; a Wave does not
  own or implicitly stop the MemberRun.

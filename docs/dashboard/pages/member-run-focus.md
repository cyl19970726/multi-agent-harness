# MemberRun Focus Page Spec

```text
status: planned
owner_role: product-design
canonical_for: one MemberRun working within one AgentTeamRun attempt
route_or_surface: Missions -> Wave -> Agent Team -> MemberRun
```

## User Problem

An operator needs to understand one agent's work without reconstructing it
from separate message, action, session, and evidence tabs. They need to answer
four questions in the first viewport:

1. What Wave and Team attempt is this member serving?
2. What was it assigned to do, and under which boundaries?
3. What is it doing or waiting for now?
4. What output supports its contribution to the Wave?

The page is a focused, continuous working surface: conversation, explicit
activity, artifacts, and review requests appear in one chronological stream.
It is not a provider transcript or a task-management page.

## Canonical Data And Semantics

Required data:

- `Mission`, `Wave`, and Wave exit criteria/gate projection;
- parent `AgentTeamRun` attempt and retry lineage;
- the selected `MemberRun`;
- `TeamMessage`, especially `kind=assignment` and its `correlation_id`;
- `MemberAction`, `TeamRunEvent`, `DelegationRun`, artifacts, and evidence
  references;
- runtime/session summary, provider/model, worktree, owned paths, permissions,
  budget/availability signals; and
- transient, sanitized `member_activity` preview only when live data exists.

The assignment message plus correlation is the run-scoped ownership proof.
Do not substitute a legacy dependency graph or a provider self-description for it.

`MemberRun` is an execution instance. `StandingAgent` is a future, long-lived
identity/capability object. A MemberRun may optionally be sourced from a
StandingAgent, a plugin/provider instance, or an ad-hoc host-created member;
it must never be rendered or stored as though it *is* a StandingAgent. Shared
layout components are allowed; shared object identity is not assumed.

Thinking is a best-effort live preview: sanitized, TTL-bound, local to the
current project/session, never persisted, replayed, forwarded, or accepted as
evidence. On refresh or expiry it disappears rather than becoming a blank
historical event.

## Layout Contract

The visual reference is the approved
`member-run-focus/running-needs-you--desktop` expected design in
[`../../design/workbench-layout-v2/`](../../design/workbench-layout-v2/README.md).

### Desktop — `1440x1000`

Use the shared Workbench shell: product sidebar about 230px, central work
surface about 800px, and Context Rail about 340px. The central stream, not a
tab bar, owns the page.

```text
+----------------------+--------------------------------------+------------------+
| Product sidebar      | Member header                        | Context Rail     |
| Missions / Agents    | role · provider/model · status       | Wave compact     |
| Workflows / Knowledge| Mission > Wave > Team > Member       | Team compact     |
| Active context tree  +--------------------------------------+ Assignment       |
|                      | unified chronological activity        | Outputs/evidence |
|                      | host/member messages                  | Runtime          |
|                      | actions / file changes / reviews      | Delegations      |
|                      | live preview (when currently present) |                  |
|                      +--------------------------------------+                  |
|                      | Message this member… (sticky)         |                  |
+----------------------+--------------------------------------+------------------+
```

The header exposes identity, status, role, provider/model, and a compact
breadcrumb. It must not turn the center into an overview dashboard. The
composer remains visible when the member can receive messages; it identifies
the recipient and permits a reply, clarification, or review request.

### Tablet — `900x1180`

- Keep a narrow/collapsed product sidebar and a full-width main stream.
- Context modules move into a right sheet or an ordered inline section; only
  `Wave`, `Assignment`, and `Needs You` are initially visible.
- Header stays above the stream; the composer stays sticky at the bottom.
- A selected module opens without hiding the activity stream permanently.

### Mobile — `390x844`

- Use a compact top bar with back-to-Team, member identity/status, and a
  context button.
- Preserve one vertical stream and fixed composer; do not create separate
  Chat and Activity tabs.
- Context modules are a bottom sheet in this priority: `Needs You`,
  `Assignment`, `Wave`, `Outputs`, `Runtime`, `Delegations`.
- Long paths, IDs, and raw data truncate or disclose progressively; no
  horizontal page overflow.

## Context Rail Modules

The rail uses shared density variants (`micro`, `compact`, `panel`) rather
than page-specific cards. Its default order is:

1. **WaveCompact** — title/index, objective, executor, exit-criteria progress,
   gate state, and open-Wave action.
2. **TeamCompact** — attempt identity, member status roll-up, one blocked or
   waiting signal, and open-war-room action.
3. **AssignmentContract** — assignment sender/time/correlation, requested
   outcome, owned paths, permissions, and applicable constraints.
4. **OutputsEvidence** — artifacts, checks, report, and contribution to the
   parent Wave gate. It must label absent evidence honestly.
5. **RuntimeSummary** — provider/model/session, availability, worktree, and
   actionable failure state. It is operational context, not the primary page.
6. **DelegationSummary** — observed provider-native or orchestrated child work,
   with attribution and control limits made explicit.

Modules are collapsible. First release uses system ordering; pinning or free
reordering is not a requirement.

## Actions

- Send a message, clarification, handoff, or review request directly to this
  member when it is addressable.
- Open the assignment anchor and other correlated messages.
- Open parent Team or Wave without losing selection context.
- Open an artifact, check, or provider session summary.
- Acknowledge a waiting/blocker signal where the message protocol permits it.

Do not offer fake lifecycle control. A stop/cancel action appears only after
the provider exposes cooperative interruption and the backend can prove its
outcome. Completion of the MemberRun is an attempt fact, not Wave acceptance.

## Empty, Loading, And Failure States

- **No assignment:** show `No assignment recorded` prominently; preserve
  observed activity but do not infer ownership.
- **No messages/actions yet:** show the member's starting state and explain
  that the stream will appear after a durable message or explicit action.
- **Runtime unavailable:** retain stored identity and history, show the
  unavailable provider/session and a retry/reconnect path only if supported.
- **Member failed/blocked:** show the explicit failure or blocker action, its
  correlation when present, and the responsible next action; never fabricate a
  reason from status alone.
- **Read/model error:** keep the last successful header/context state marked
  stale, show scoped retry, and do not replace the page with an empty shell.
- **Finished attempt:** render read-only history; composer and lifecycle
  controls are disabled with an explanation.

## Screenshot Acceptance

For `member-run-focus--running-needs-you--desktop` in the visual contract:

- baseline, approved expected, implementation capture, and labeled comparison
  all use the registered fixture, route, and `1440x1000` viewport;
- first viewport visibly contains the Member header, a continuous mixed
  activity/chat stream, assignment context, a Wave module, Team module, and
  sticky composer;
- a live preview, when fixture-provided, is visibly labelled `not saved`; it
  must not appear in stored activity after a refresh fixture;
- Assignment appears before dependent report/evidence in the stream or exposes
  a clear correlation link;
- the implementation does not use the legacy Member drawer or
  Overview/Activity/Messages primary tabs;
- deviations from the approved image are recorded in
  `visual-contract.json`, not silently normalized by changing the expected
  image.

## Explicit Boundaries

- This page is for a `MemberRun`, not a StandingAgent profile.
- It does not require or display a legacy dependency graph as the ownership model.
- Provider-native subagents remain observed delegation unless the harness owns
  their lifecycle.
- TeamRun completion only says that one attempt ended; the parent Wave gate is
  the sole `accepted | revise | blocked` decision and names any accepted run.

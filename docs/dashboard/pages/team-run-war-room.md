# Agent Team War Room Page Spec

```text
status: planned
owner_role: product-design
canonical_for: one AgentTeamRun attempt that executes an agent_team Wave
route_or_surface: Missions -> Wave -> Agent Team attempt
```

## User Problem

An operator coordinating a live Agent Team needs one answerable surface:
which members are working, blocked, or waiting; what messages and outputs are
moving through the team; and what the operator needs to decide next. The
existing split between overview, actions, and messages makes the story hard to
follow and hides urgency.

The War Room makes the team attempt observable and steerable without claiming
that a Wave is a legacy dependency graph or that provider-native child agents are fully
controlled by the harness.

## Canonical Data And Semantics

Required data:

- parent `Mission` and `Wave`, including objective, exit criteria, and gate;
- selected `AgentTeamRun`, its status, `previous_run_id`, and host/runtime
  facts;
- `MemberRun` state, role, provider/model, current explicit action, pressure,
  availability, and last update;
- `TeamMessage` delivery state and correlation lineage;
- `MemberAction`, `TeamRunEvent`, observed `DelegationRun`, artifacts,
  evidence/check references, and attempt outcome;
- resource/worktree/session summaries; and
- transient member activity previews when available.

Ownership belongs to assignment messages plus `correlation_id`. The page may
summarize owned paths/constraints but must not manufacture per-member Tasks.

An AgentTeamRun is an attempt for one Wave. It may complete, fail, or be
stopped. That does **not** accept its parent Wave. The Wave gate is separately
recorded by the Host as `accepted`, `revise`, or `blocked`; only that gate can
name an accepted completed attempt.

## Layout Contract

The visual reference is the approved
`team-war-room/running-needs-you--desktop` expected design in
[`../../design/execution-workbench-v3/`](../../design/execution-workbench-v3/README.md).

### Desktop — `1440x1000`

Use the shared shell: 230px product sidebar, about 800px main work surface,
and 340px Context Rail.

```text
+----------------------+--------------------------------------+------------------+
| Product sidebar      | Team header                          | Context Rail     |
|                      | Mission > Wave > Attempt · status    | Wave compact     |
| Active context tree  +--------------------------------------+ Gate readiness   |
|                      | compact Member controls              | Attempt           |
|                      | role/model/status/action/pressure    | Selected member  |
|                      +--------------------------------------+ Resources        |
|                      | Needs-you band (only if actionable)  |                  |
|                      +--------------------------------------+                  |
|                      | unified Team activity stream          |                  |
|                      | filters: All/Messages/Actions/        |                  |
|                      | Decisions/Evidence                    |                  |
|                      | sticky Message team or @member…       |                  |
+----------------------+--------------------------------------+------------------+
```

Member controls are compact controls, not dashboard metric cards. Each shows
identity, role, provider/model, status, current action, pressure, and last
meaningful update. Selecting one populates the right rail's Selected Member
module; `Open member` navigates to the MemberRun Focus page rather than opening
a blocking drawer.

The center uses one chronological team stream. Filters change visibility but
not the data model. Messages show sender/recipient/delivery/correlation;
actions and evidence retain their member attribution; decision rows distinguish
attempt outcome from parent Wave-gate decision.

### Tablet — `900x1180`

- Collapse the product sidebar; retain mission/wave/attempt breadcrumb.
- Member controls become a horizontally scrollable but keyboard-accessible
  strip, or a two-column grid with no hidden critical status.
- Show `Needs You` before the stream.
- Context Rail becomes an inline section after the stream or a right sheet;
  Wave and Gate remain first.
- Composer remains fixed to the safe area.

### Mobile — `390x844`

- Header has back-to-Wave, attempt state, and context-sheet affordance.
- Member controls form a vertical priority list: blocked/waiting first, then
  running, then completed.
- The `Needs You` band is immediately below the controls.
- One stream with filter chips scrolls beneath it; composer supports Team or
  an explicitly chosen member, never an ambiguous recipient.
- Context appears in a bottom sheet ordered Wave, Gate, Attempt, Selected
  Member, Resources. No horizontal overflow or modal/drawer replacement for a
  full member page.

## Context Rail Modules

1. **WaveCompact** — Wave objective, executor, exit-criteria progress, gate
   state, and open-Wave action.
2. **GateReadiness** — satisfied/missing criteria, candidate attempt outcome,
   blockers, and a clear statement that this page cannot accept the Wave.
3. **AttemptSummary** — attempt number, retry lineage, start/end, host surface,
   status, and honest capability degradation.
4. **SelectedMemberCompact** — selected member identity, assignment/current
   action, message action, and open-member-page action.
5. **Resources** — aggregate sessions/worktrees/budget signals and release or
   acquisition failures. It must not imply termination control that is absent.

## Actions

- Message the whole team or one explicit member.
- Inspect a message's assignment, delivery, or correlation lineage.
- Acknowledge/re-deliver an eligible message.
- Open MemberRun Focus, Wave Canvas, an artifact, or a provider session
  summary.
- Review blockers, waiting-for-approval, handoffs, and evidence.
- Complete or stop the attempt only when the backend supports that transition;
  running-provider cancellation is hidden until cooperative interruption is
  real.

Wave gating occurs from the Wave Canvas/gate surface after a completed attempt
is eligible. It is never an implicit side effect of a TeamRun button.

## Empty, Loading, And Failure States

- **No members:** explain that no runnable member instances were created and
  surface the attempt creation error or next Wave action.
- **Starting:** show admitted members and pending runtime acquisition; do not
  label them as working before an explicit action/state supports it.
- **No Team activity:** retain member controls and explain that durable
  messages/actions will arrive here; do not add invented placeholders.
- **One or more blocked:** show a precise Needs You card only when an action or
  decision is known; otherwise say the blocker is being reported.
- **Provider/session failure:** attribute it to the affected member and retain
  attempt history; do not mark the whole Wave accepted/failed automatically.
- **Completed/failed/stopped attempt:** read-only stream, outcome, artifacts,
  and next instruction: review/gate/retry from the parent Wave.
- **Read error:** preserve a stale last projection with timestamp and scoped
  retry instead of an empty dashboard.

## Screenshot Acceptance

For `team-war-room--running-needs-you--desktop`:

- captures use the registered native fixture, route, and `1440x1000` viewport;
- first viewport includes the shared sidebar, attempt header, four compact
  member controls, one clear Needs You signal, unified activity stream, sticky
  composer, and Wave/Gate/Attempt context modules;
- filters read `All`, `Messages`, `Actions`, `Decisions`, and `Evidence`; they
  are stream filters, not replacing primary page tabs;
- selected member context provides `Message` and `Open member`, and the latter
  resolves to a standalone MemberRun page;
- Team completion and Wave gate state are visibly different in text and color;
- implementation/baseline/expected comparison and intentional deviations are
  recorded through the Workbench Layout V2 visual contract.

## Explicit Boundaries

- The page is an `AgentTeamRun` attempt, not a standing team directory.
- A `MemberRun` is not a StandingAgent, even when it has a source identity.
- Host/provider-native subagents are observed delegation facts, not controlled
  MemberRuns, unless a separate orchestrated contract exists.
- There is no legacy dependency graph requirement or task-centric ownership UI.
- Attempt completion never equals Wave acceptance; the Wave gate remains the
  durable parent decision.

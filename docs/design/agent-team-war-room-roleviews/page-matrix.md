# Page matrix

The Agent Team War Room is the only P0 page in issue #444. Product-truth and
visual-fidelity are separate gates. Exact browser artifacts use device scale
factor 1, light theme, reduced motion and `domcontentloaded` plus a visible
`Team Workspace` readiness assertion.

| Priority | Page | State | Viewport | Baseline | Expected | Implemented | Review |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P0 | Team Workspace | populated active + Host pressure | desktop 1440x1000 | current regression + historical War Room | canonical layout contract | implemented | product truth + visual fidelity |
| P0 | Team Workspace | populated active + Host pressure | tablet 900x1180 | current regression + historical War Room | canonical layout contract | implemented | product truth + visual fidelity |
| P0 | Team Workspace | populated active + Host pressure | mobile 390x844 | current regression + historical War Room | canonical layout contract | implemented | product truth + visual fidelity |
| P0 | Team Workspace | populated active + Host pressure | mobile 320x844 | current regression + historical War Room | canonical overflow contract | implemented | overflow + keyboard/touch |
| P0 | Team Workspace | useful empty Team | desktop/mobile | `.visual-evidence/agent-team-war-room-roleviews/baseline-exact/` | canonical empty hierarchy | implemented | no blank canvas |
| P1 | Team Workspace | loading / last-good stale / error | desktop/mobile | current `ViewState` | interaction contract | implemented | recoverability |
| P1 | Team Workspace | completed run with live member runtime | desktop | historical War Room | canonical lifecycle contract | implemented | honest controls |

## Product objects and actions

- Parent: one Mission-owned flat AgentTeam on one immutable Node.
- Attempt: latest or selected AgentTeamRun; a historical Wave is optional
  navigation context only.
- Responsibility: versioned Work plus WorkEvent/WorkDelivery.
- Conversation: canonical Message plus typed MessageDelivery.
- Runtime: exact NodeDaemon parent, Team Supervisor generation and native
  session summaries.
- Writes: only authenticated `allowed_actions` with exact CAS and idempotency.

## Baseline decision

The exact-size current-regression baseline is ignored runtime evidence. The
historical `5c0258fa^` War Room is a layout/interaction reference, not a data or
writer reference. No generated expected image is required because this task
restores an already approved page contract rather than selecting a new visual
direction; the canonical ASCII layouts and historical accepted captures are
the expected design. Final acceptance still requires exact-size implemented
captures, comparisons and two iterations.

## Implemented evidence

The deterministic authenticated fixture covers a populated Team with Open,
Assigned, In progress, blocked, Review and Done responsibility; its shared
Works/Activity/Members views; selected Work and member context; the exact-self
Member home; embedded Host tools; and a useful empty Team at desktop, mobile,
and the 320px overflow gate. It
negotiates the RoleView protocol, supplies a memory-only capability, exercises
SSE invalidation/refetch, proves Browser Back and URL-owned Team filters,
performs one prepared `send_message` action with exact CAS and idempotency
headers, proves a correlated reply path, displays enabled and server-disabled
contextual Work actions with exact reasons, proves completed-Run/live-runtime
Close availability, proves keyboard focus containment/restoration for the Work sheet, and
records zero unexpected console or HTTP errors and no horizontal overflow at
every normal viewport. A separate deliberate initial-503 case records the
recoverable initial error state. Durable selected captures live under
`docs/design/agent-team-war-room-roleviews/implemented/`; raw runs remain under
`.visual-evidence/agent-team-war-room-roleviews/final-exact/`.

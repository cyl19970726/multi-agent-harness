# Page matrix

The Agent Team War Room is the only P0 page in issue #444. Product-truth and
visual-fidelity are separate gates. Exact browser artifacts use device scale
factor 1, light theme, reduced motion and `domcontentloaded` plus a visible
`Team Workspace` readiness assertion.

| Priority | Page | State | Viewport | Baseline | Expected | Implemented | Review |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P0 | Team Workspace | populated active + Host pressure | desktop 1440x1000 | current regression + historical War Room | canonical layout contract | pending | product truth + visual fidelity |
| P0 | Team Workspace | populated active + Host pressure | tablet 900x1180 | current regression + historical War Room | canonical layout contract | pending | product truth + visual fidelity |
| P0 | Team Workspace | populated active + Host pressure | mobile 390x844 | current regression + historical War Room | canonical layout contract | pending | product truth + visual fidelity |
| P0 | Team Workspace | populated active + Host pressure | mobile 320x720 | current regression + historical War Room | canonical overflow contract | pending | overflow + keyboard/touch |
| P0 | Team Workspace | useful empty Team | desktop/mobile | `.visual-evidence/agent-team-war-room-roleviews/baseline-exact/` | canonical empty hierarchy | pending | no blank canvas |
| P1 | Team Workspace | loading / last-good stale / error | desktop/mobile | current `ViewState` | interaction contract | pending | recoverability |
| P1 | Team Workspace | completed run with live member runtime | desktop | historical War Room | canonical lifecycle contract | pending | honest controls |

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

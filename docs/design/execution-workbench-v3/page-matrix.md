# Page matrix

| Priority | Page | Representative state | User question | Primary action | Expected |
| --- | --- | --- | --- | --- | --- |
| P0 | Mission Wave canvas | running, gate pending | What is running now, what is complete, and what blocks the next Wave? | Review gate | approved 2026-07-21, implemented |
| P0 | Agent Team war room | running, member blocked | Who is doing what, where did work hand off, and why is QA blocked? | Review request | approved 2026-07-21, implemented |
| P0 | Mission Wave canvas | responsive implementation | Can the ordered journey and gate remain legible without the right rail? | Open context | captured at tablet and mobile |
| P0 | Agent Team war room | responsive implementation | Can presence, activity, pressure, and composer remain one usable flow? | Message/review | captured at tablet and mobile |
| P1 | MemberRun focus | completed native work history and handoff | What did this member do, what did it produce, and how can I inspect the execution? | Inspect history / follow up | V4 desktop warm-editorial implementation approved 2026-07-22 |

## Fidelity v2 · approved and implemented

- Mission Wave canvas: approved `1440×1000` expected, design spec, prompt provenance, asset inventory, final browser evidence, comparison, and Overlay are complete.
- Agent Team war room: approved `1440×1000` expected, design spec, prompt provenance, asset inventory, corrected node-first Team Activity timeline, two retained correction captures, comparison, and overlay are complete.
- Product-truth and visual-fidelity gates pass independently for both P0 cases.

## Responsive contract

- Desktop keeps the navigation, central workspace, and contextual rail.
- Tablet collapses navigation and turns the context rail into a drawer.
- Mobile presents one execution flow; Wave/member context becomes a bottom sheet.
- The composer remains reachable and never covers the most recent event.
- Motion is disabled or reduced under `prefers-reduced-motion`.

## MemberRun V4 approved direction

- Desktop preserves the shared Company OS shell while making one continuous
  member narrative the primary surface.
- Complete chronological history is the default; `Focus key activity` offers a
  compact lens without deleting evidence.
- The readable projection groups native work into Briefing, Exploration,
  Implementation, Verification, and Handoff without persisting a second plan.
- Tool invocation and result render as one compact execution step while both
  native records remain inspectable.
- The generated eight-portrait execution identity set is shared by Agent Team
  and Organization surfaces; text identity remains authoritative.
- Tablet Context is a right sheet and mobile Context is a bottom sheet. Both
  preserve the member workspace behind the control surface.
- V4 expected/actual comparison and 50% overlay are stored under
  `comparisons/member-run-focus/` and `overlays/member-run-focus/`.

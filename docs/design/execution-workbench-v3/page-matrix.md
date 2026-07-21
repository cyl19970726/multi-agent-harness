# Page matrix

| Priority | Page | Representative state | User question | Primary action | Expected |
| --- | --- | --- | --- | --- | --- |
| P0 | Mission Wave canvas | running, gate pending | What is running now, what is complete, and what blocks the next Wave? | Review gate | approved 2026-07-21, implemented |
| P0 | Agent Team war room | running, member blocked | Who is doing what, where did work hand off, and why is QA blocked? | Review request | approved 2026-07-21, implemented |
| P0 | Mission Wave canvas | responsive implementation | Can the ordered journey and gate remain legible without the right rail? | Open context | captured at tablet and mobile |
| P0 | Agent Team war room | responsive implementation | Can presence, activity, pressure, and composer remain one usable flow? | Message/review | captured at tablet and mobile |
| P1 | MemberRun focus | active assignment | What has this member done and what does it need? | Message member | reuse V3 visual system after P0 approval |

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

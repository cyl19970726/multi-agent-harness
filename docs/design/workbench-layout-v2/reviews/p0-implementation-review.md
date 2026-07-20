# P0 implementation review

Date: 2026-07-19

The three P0 pages pass visual acceptance with documented deviations. The
deterministic native fixture now represents the pressure states named by the
approved designs, and the repeatable capture run proves them at desktop,
tablet, and mobile sizes.

## What the implementation now proves

- Mission, Wave, Agent Team attempt, and MemberRun remain separate product
  objects with addressable pages.
- Desktop uses the approved three-part grammar: product sidebar, primary work
  surface, and composed context rail.
- Member and Team pages keep activity/chat primary and pin the composer to the
  working surface.
- Tablet and mobile preserve the central workflow. Member/Team context is
  available through `Context & controls`; Mission exposes a visible `Context`
  action.
- Team mobile shows the highest-pressure member first and can expand the rest
  as a vertical list. Desktop keeps all four member controls visible.
- Wave gate state remains separate from Team attempt lifecycle.
- Transient thinking can only enter the UI through live activity and is not
  part of the durable activity selector.
- Wave 1 is accepted, Wave 2 is a running retry with an explicit pending gate,
  and Wave 3 remains planned.
- The current Team attempt exposes reviewing, running, and blocked members,
  assignment correlation, evidence, provider-native delegation, and operator
  pressure without introducing a legacy dependency graph.
- Mission desktop keeps Re-plan and the start of Wave 3 in the first viewport.
  Mobile Mission context is prioritized as Needs You, Gate & outcome, Selected
  Wave, Agent Team, then Mission brief.

## Acceptance evidence

The versioned `workbench-layout-v2-native-v1` fixture contains:

1. Wave 1 completed and accepted with output evidence;
2. Wave 2 on retry Attempt 2, running or reviewing;
3. at least one blocked or waiting MemberRun and a concrete review request;
4. assignment correlation, handoff/action history, artifact/check evidence,
   and two of three exit criteria ready;
5. a sanitized live-only thinking preview with deterministic expiry;
6. Wave 3 planned.

The capture runner materializes the fixture into an isolated temporary store,
starts the dashboard on free ports, fixes browser time, injects the transient
preview only after SSE connects, and captures canonical routes. It checks
console errors and horizontal overflow and records browser, revision, dirty
state, routes, and viewports in `capture-run.json`. The fixture invariants pass
12/12.

The historical baselines remain product-direction comparisons, not same-state
regression proofs. The approved generated designs are 1536x1024 while browser
evidence uses the contract viewport 1440x1000. Both are intentional and visible
in the three-way comparisons.

## Final review decision

- MemberRun Focus: `pass_with_deviations`.
- Agent Team War Room: `pass_with_deviations`.
- Mission Wave Canvas: `pass_with_deviations`.

The independent final Mission review confirmed that the previous three P0
issues are closed: Re-plan and Wave 3 are visible on desktop, the Agent Team
compact exposes all member statuses including QA blocked, and mobile context
uses the required decision-first order.

The P1 pressure pass also makes Needs You member-specific: QA's own blocker is
preferred over later unrelated review requests, and the action opens that exact
MemberRun. The capture runner verifies this navigation. Gate readiness now
shows 2/3 plus all three declared criteria without pretending that individual
criterion-to-evidence mappings exist. At 900px, a 64px compact product rail
replaces the full sidebar, and tablet context-open evidence is captured for all
three P0 pages.

## Deferred visual refinements

- Increase secondary-text legibility where real dense data confirms it is too
  small or low contrast.
- Validate the Member and Team composers with long real messages and the mobile
  keyboard open.
- Decide whether Mission mobile should retain the inline context jump or move
  to the same sheet/disclosure pattern as Member and Team.
- Replace the temporary gate-note readiness parser with a structured
  criterion-to-evidence contract when the Wave schema is extended.
- Compress or reorder the desktop context rail if every Gate detail must fit
  without independent rail scrolling.

# Agent Workbench Acceptance

This document owns frontend acceptance for Agent Workbench layout changes.
Product-level acceptance stays in [../dashboard.md](../dashboard.md). Local
commands stay in [runbook.md](runbook.md). The complete frontend design under
test stays in [frontend-design.md](frontend-design.md).

## Layout Spec Gate

Before implementation acceptance can start, the PR must point to a hard layout
implementation spec for the slice under test. Specs live under
`docs/dashboard/hard-layout-specs/<slice>.md` unless a Reviewer records a
different path in [layout-decisions.md](layout-decisions.md). Each spec needs a
stable `spec_id`, selected design refs, Reviewer `stop | continue | blocked`
decision, screenshot matrix, and non-waivable failure checklist.

The spec must define:

- desktop, tablet, and mobile ASCII box diagrams;
- region dimensions and collapsed regions;
- first-viewport content;
- scroll containers and overflow rules;
- empty, loading, loaded, warning, and error states;
- data density and text wrapping constraints;
- explicit forbidden primary surfaces such as raw JSON, card dumps, and
  unstructured stacked reports;
- screenshot acceptance for every viewport.

If browser screenshots show that the implementation does not match the hard
layout spec, the correct action is to stop implementation, record a rejected
implementation in [layout-decisions.md](layout-decisions.md), and rerun the
Designer -> Questioner -> Reviewer loop. Do not continue styling the same
failed direction until the spec gap is fixed.

Missing ASCII diagrams fail the layout spec gate. Prose-only layout descriptions
are not enough for implementation acceptance.

## Rebuild Boundary Checklist

Renewed Workbench implementation must not restyle the old dashboard structure.
Every implementation PR must include an import or `rg` audit proving the old
primary UI is not still driving the first viewport:

- prohibited as primary layout: `SummaryGrid`, always-visible snapshot
  `textarea`, `RawViews`, old summary/Kanban/detail/raw-view composition, and
  raw JSON/debug panels outside the collapsed debug drawer or `/debug` route;
- allowed only after review: pure `types.ts`, API helpers, and read-model
  selectors that still serve the new hard layout spec;
- required audit: changed component list, retained imports, removed old primary
  imports, and evidence that raw snapshot input is not in the primary viewport.

If the PR keeps an old component, it must state whether it is temporary,
secondary/debug-only, or pure data logic. Otherwise the implementation is a
failed rebuild boundary.

## Implementation Sequence

1. Documentation: update Workbench docs before changing component structure or
   CSS.
2. Layout spec: add the hard layout implementation spec for the slice and get a
   Reviewer stop/continue decision.
3. Rebuild boundary: remove the old summary/Kanban/detail/raw-view product
   structure as the basis of the UI. Preserve only stable API/types/read-model
   contracts that still serve the new design.
4. Read model: add selectors for latest-row message projection, full active
   team roster, vision/goal ladder, and member activity timeline.
5. Shell: replace always-visible snapshot textarea and raw views with a
   collapsed debug drawer.
6. Vision and goal header: show vision context, goal collection, selected goal,
   goal learning completeness, distance-to-vision, and next-round proposals
   above the task graph.
7. Goal design and team: expose designed AgentTeam, current active team, role
   gaps, and team adjustments for the selected goal.
8. Workbench: expose goal graph/lane projections, task graph/lane projections,
   graph revisions, and selected task detail in the main pane.
9. Inspector: convert stacked member/warnings panels into tabbed inspector.
10. Member page: add URL-addressable member detail with chronological activity.
11. Acceptance: verify browser screenshots, console health, no page-level
   horizontal overflow, and actionable warning/member navigation.

## Screenshot-First Browser Evidence

Every layout implementation PR must attach browser evidence:

- desktop screenshot at `1440x1000`;
- tablet screenshot at `900x1180`;
- mobile screenshot at `390x844`;
- console output showing no React key warnings and no runtime errors;
- proof that live mode still loads `/v1/snapshot`;
- proof that selected goal displays vision context, goal collection, and
  distance-to-vision state, or explicit missing-context warnings;
- proof that completed and not-complete goals are visually distinct;
- proof that selected goal shows designed AgentTeam, current team state, and
  task graph revision state;
- proof that selecting a member shows runtime health, queue, current task, and
  activity stream;
- proof that raw JSON/debug views are collapsed by default;
- proof that page-level horizontal overflow is absent;
- proof that the implementation matches the hard layout implementation spec, or
  a rejected implementation record explaining why it does not.
- PM acceptance agent output that validates end-to-end product logic using the
  browser screenshots and live UI.
- User acceptance agent output that validates operator usability using the
  browser screenshots and live UI.

Every screenshot must be reviewed as the product artifact:

```text
Screenshot Review
  route:
  viewport:
  screenshot_ref:
  first_impression:
  workbench_or_dashboard:
  matched_ascii_spec:
  team_as_collaboration_space:
  agent_member_as_workspace:
  goal_task_docs_connected:
  debug_secondary:
  overflow_result:
  console_result:
  decision: pass | fix | reject
```

The review may not pass because data rendered, console is clean, network
requests succeeded, or horizontal overflow is absent. Those checks are required
but not sufficient.

Each viewport matrix must cover the default Team workspace, Member detail,
Goal/Task document surface, Graph/Kanban surface, Warnings surface, and Debug
closed default state. A single happy-path homepage screenshot is not enough.

The Workbench is acceptable only when the first viewport looks like an operator
workbench over harness state, not a stacked report of cards and raw objects.

## Non-Waivable Failures

These fail implementation acceptance and require a rejected implementation
record or a return to design:

- missing hard layout implementation spec at the agreed docs path;
- missing changed-core-module option loop or Reviewer decision;
- missing implementation Questioner/Critic screenshot comparison against the
  hard layout spec;
- missing PM/User browser acceptance agent findings;
- known failed implementation attempt not recorded as rejected;
- no browser screenshot evidence;
- primary page is blank, raw/debug-only, a card dump, or an unstructured stacked
  report instead of a Workbench;
- screenshot first impression is dashboard, report, card dump, or raw object
  viewer;
- runtime JavaScript error on initial load;
- horizontal overflow on mobile caused by the layout;
- missing AgentMember realtime/detail surface when the change claims to support
  agent observation;
- old dashboard primary components or imports still drive the first viewport
  without a Reviewer-approved debug/secondary rationale.
- PM/User acceptance passes based on object presence, console cleanliness,
  network success, component presence, or no overflow instead of screenshot
  product shape and operator flow.

## PM And User Acceptance Subagents

Implementation acceptance requires two independent read-only subagents:

- PM acceptance subagent: judges product logic, workflow proof, Vision/Goal
  coherence, and whether the surface supports self-hosting.
- User acceptance subagent: judges real operator usability, navigation,
  comprehension, and action confidence.

The implementer cannot act as either subagent. Each subagent must use browser
automation and screenshots, not only code, DOM, docs, or console output.

PM acceptance must judge product logic from screenshots and browser
interaction:

```text
If the first viewport looks like a dashboard, card dump, report page, or raw
object viewer, mark P0 fail even when all data is present.
```

User acceptance must judge operator usability:

```text
Fail when the user cannot identify the active team, enter a member workbench,
inspect inbox/outbox/activity, find Goal/Task context, locate docs/evidence/
decision, and know the next action without reading raw JSON or code.
```

Any unresolved P0/P1 from either acceptance agent blocks implementation
acceptance.

## Web Quality Skill Gate

Frontend implementation PRs should also use a web-quality acceptance pass. The
recommended external skill source is
`https://github.com/addyosmani/web-quality-skills`, which provides stack-agnostic
skills for web quality audits, performance, Core Web Vitals, accessibility, SEO,
and best-practices reviews.

Use it as an additional frontend gate after harness-specific browser
acceptance:

```text
harness Workbench acceptance
  -> vision/goal/task/member workflow is visible and operable

web-quality acceptance
  -> page is accessible, performant, stable, and free of avoidable browser issues
```

Required checks for Workbench layout PRs:

- run a web-quality audit for the Workbench route;
- run an accessibility pass for keyboard navigation, labels, focus state,
  contrast, and readable panel structure;
- run a Core Web Vitals/performance pass for layout shift, input delay, bundle
  size, and expensive rendering in large snapshots;
- run a best-practices pass for console cleanliness, browser errors, modern API
  usage, and security-sensitive UI behavior;
- include desktop, tablet, and mobile browser screenshots with the audit
  summary.

Suggested acceptance targets:

| Category | Target |
| --- | --- |
| Accessibility | 100 or documented exception with follow-up task |
| Performance | 90+ for local static build, or documented fixture limitation |
| Best Practices | 95+ |
| SEO | 95+ unless intentionally waived for local-only operator UI |
| Core Web Vitals | LCP <= 2.5s, INP <= 200ms, CLS <= 0.1 on representative local fixture |

These checks do not decide whether the harness workflow is correct. They decide
whether the implemented Workbench is a high-quality web surface for that
workflow.

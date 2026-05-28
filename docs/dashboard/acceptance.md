# Agent Workbench Acceptance

This document owns frontend acceptance for Agent Workbench layout changes.
Product-level acceptance stays in [../dashboard.md](../dashboard.md). Local
commands stay in [runbook.md](runbook.md). The complete frontend design under
test stays in [frontend-design.md](frontend-design.md).

## Implementation Sequence

1. Documentation: update Workbench docs before changing component structure or
   CSS.
2. Rebuild boundary: remove the old summary/Kanban/detail/raw-view product
   structure as the basis of the UI. Preserve only stable API/types/read-model
   contracts that still serve the new design.
3. Read model: add selectors for latest-row message projection, full active
   team roster, vision/goal ladder, and member activity timeline.
4. Shell: replace always-visible snapshot textarea and raw views with a
   collapsed debug drawer.
5. Vision and goal header: show vision context, goal collection, selected goal,
   goal learning completeness, distance-to-vision, and next-round proposals
   above the task graph.
6. Goal design and team: expose designed AgentTeam, current active team, role
   gaps, and team adjustments for the selected goal.
7. Workbench: expose goal graph/lane projections, task graph/lane projections,
   graph revisions, and selected task detail in the main pane.
8. Inspector: convert stacked member/warnings panels into tabbed inspector.
9. Member page: add URL-addressable member detail with chronological activity.
10. Acceptance: verify browser screenshots, console health, no page-level
   horizontal overflow, and actionable warning/member navigation.

## Browser Evidence

Every layout implementation PR must attach browser evidence:

- desktop screenshot at `1440x900`;
- tablet screenshot around `900x900`;
- mobile screenshot around `390x844`;
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
- proof that page-level horizontal overflow is absent.

The Workbench is acceptable only when the first viewport looks like an operator
workbench over harness state, not a stacked report of cards and raw objects.

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
- include desktop and mobile browser screenshots with the audit summary.

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

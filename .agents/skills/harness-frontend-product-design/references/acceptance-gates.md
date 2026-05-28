# Acceptance Gates

## Harness Workflow Acceptance

The frontend must show:

- Vision and Goal collection;
- completed and not-complete goals;
- selected GoalDesign;
- persistent AgentTeam;
- goal-level graph plus goal-level Kanban/lane view;
- dynamic TaskGraph with graph and Kanban/lane views;
- assignment messages;
- AgentMember realtime state;
- evidence, proposal, review, decision, and GoalEvaluation;
- distance-to-vision and next-round proposal.

## Browser Acceptance

Attach evidence:

- reference to the hard layout implementation spec used for this PR;
- spec path, normally `docs/dashboard/hard-layout-specs/<slice>.md`, plus the
  Reviewer `continue` decision;
- desktop screenshot, normally 1440 x 1000;
- tablet screenshot, normally 900 x 1180;
- mobile screenshot, normally 390 x 844;
- console output with no runtime errors and no React key/layout warnings;
- proof of no page-level horizontal overflow at each viewport;
- proof that raw/debug surfaces are not primary;
- proof that selecting a member shows realtime activity and send-message UI;
- proof that graph and Kanban/lane projections are both reachable where the
  design claims they exist.
- proof that the first viewport matches the selected spec instead of becoming a
  stacked report, card dump, metrics dashboard, or raw snapshot tool.
- proof that the screenshot matrix covers default Team workspace, Member
  detail, Goal/Task document surface, Graph/Kanban, Warnings, and Debug closed
  default state.
- PM acceptance agent findings based on browser screenshots and product-flow
  inspection.
- User acceptance agent findings based on browser screenshots and hands-on
  operation.

Suggested artifact names:

```text
docs/dashboard/evidence/<date>-desktop.png
docs/dashboard/evidence/<date>-tablet.png
docs/dashboard/evidence/<date>-mobile.png
docs/dashboard/evidence/<date>-console.md
docs/dashboard/evidence/<date>-overflow.md
docs/dashboard/evidence/<date>-web-quality.md
```

If the project uses harness evidence instead of committed screenshots, attach
the same artifacts through `evidence add` and link the evidence ids from the PR.

Overflow proof should include an evaluated value for:

```text
document.documentElement.scrollWidth <= document.documentElement.clientWidth
```

Run it after the page has loaded representative data and after opening the
member detail, Goal/Task document, graph/Kanban, docs, and warnings surfaces.

## PM / User Browser Acceptance Agents

Run these agents after the implementation is available in a browser and after
the screenshot matrix exists. They are read-only validators. Their input must
include the hard layout spec, active product docs, route URL, screenshot paths,
console/overflow proof, and any known missing live data. Do not give them the
developer's preferred answer.

Required outputs:

- product or usability pass/fail;
- P0/P1/P2 findings with screenshot or route refs;
- concrete repair requests;
- whether another browser pass is required after fixes;
- explicit waiver candidates when an issue is real but outside the slice.

### PM Acceptance Prompt

```text
You are the PM acceptance agent for Multi-Agent Harness Agent Workbench. You are
read-only. Use browser automation and the provided screenshots to inspect the
working product, not only the docs or code.

First restate the product purpose, active Vision, selected frontend Goal, and
the end-to-end workflow the UI must make understandable:
Vision -> Goal collection -> selected Goal -> GoalDesign -> persistent
AgentTeam -> TaskGraph -> Message assignment -> AgentMember runtime -> Evidence
-> Proposal/Review/Decision -> GoalEvaluation -> distance-to-vision/next Goal.

Evaluate whether the implemented Workbench expresses that product logic. Check
that the first viewport reads as a collaboration workbench rather than a metric
dashboard, report stack, card dump, or raw snapshot tool. Verify that Team,
AgentMember, Goal/Task document, Graph/Kanban, Docs, Warnings, and Debug-closed
surfaces are reachable and mapped to canonical harness objects.

Return P0/P1/P2 product findings with screenshot or route refs, missing workflow
proof, confusing object relationships, gaps that weaken self-hosting, and
specific fixes. Say whether the implementation can be accepted from a product
logic perspective, or whether another PM browser pass is required after fixes.
```

### User Acceptance Prompt

```text
You are the User acceptance agent for Multi-Agent Harness Agent Workbench. You
are read-only. Act like an operator trying to understand and control a
multi-agent team. Use browser automation and the provided screenshots to
experience the product.

Perform this walkthrough: enter the Workbench, identify the active team, select
an AgentMember, inspect inbox/outbox or message/activity state, find the current
Goal and TaskGraph, switch between graph and Kanban/lane views, locate related
docs, inspect warnings, and confirm that debug/raw data stays secondary.

Judge the experience from a real user perspective: orientation, navigation,
readability, information density, action affordances, mobile/tablet usability,
empty/error state clarity, trust in live status, and whether you know what to do
next. Do not reward visual style when the workflow is confusing.

Return P0/P1/P2 usability findings with screenshot or route refs, concrete UI
fixes, and whether another user browser pass is required after fixes.
```

## Web Quality Acceptance

Use a web-quality audit when available, including the external skill source
`https://github.com/addyosmani/web-quality-skills`.

Check:

- accessibility;
- keyboard navigation and focus;
- Core Web Vitals;
- performance on representative snapshots;
- best practices;
- console cleanliness.

## Pass / Waiver Rules

Cannot be waived for implementation acceptance:

- missing hard layout implementation spec;
- missing changed-core-module option loop or Reviewer decision;
- missing implementation Questioner/Critic screenshot comparison against the
  hard layout spec;
- known failed implementation attempt not recorded as rejected;
- no browser screenshot evidence;
- missing PM/User browser acceptance agent output;
- primary page is blank or raw/debug-only;
- primary page reads as a stacked report/card dump instead of a Workbench;
- runtime JavaScript error on initial load;
- horizontal overflow on mobile caused by the layout;
- missing AgentMember realtime/detail surface when the change claims to support
  agent observation.

May be waived only with rationale, owner, and follow-up task:

- non-critical accessibility issues;
- performance regressions caused by known sample-data volume;
- partial graph/Kanban behavior while the read model is not yet available;
- missing live streaming when polling state is visibly accurate and documented;
- web-quality tool unavailable in the local environment.

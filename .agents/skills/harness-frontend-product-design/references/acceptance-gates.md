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

- references to every page-local layout contract used for this PR, normally
  `docs/dashboard/pages/<page>.md#layout-contract`, plus the Reviewer
  `continue` decision;
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

Screenshots are the review object, not a passive attachment. For every required
screenshot the Reviewer must write:

```text
first_impression:
workbench_or_dashboard:
matches_page_layout_contract:
team_as_collaboration_space:
agent_member_as_workspace:
goal_task_docs_connected:
debug_secondary:
decision: pass | fix | reject
```

Do not accept based only on rendered data, clean console, network success, no
React warnings, or no horizontal overflow.

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

## PM / User Browser Acceptance Subagents

Run two independent subagents after the implementation is available in a
browser and after the screenshot matrix exists:

- PM acceptance subagent: product logic, Vision/Goal/Task/Agent workflow, and
  self-hosting coherence.
- User acceptance subagent: operator usability, navigation, comprehension, and
  action confidence.

They are read-only validators. Their input must include the page-local layout
contracts, active product docs, route URL, screenshot paths, console/overflow
proof, and any known missing live data. Do not give them the developer's
preferred answer. Do not let the implementer act as either acceptance subagent.

Required outputs:

- product or usability pass/fail;
- P0/P1/P2 findings with screenshot or route refs;
- concrete repair requests;
- whether another browser pass is required after fixes;
- explicit waiver candidates when an issue is real but outside the slice.
- whether the subagent recommends another browser pass after fixes.

### PM Acceptance Prompt

```text
You are the PM acceptance subagent for Multi-Agent Harness Agent Workbench. You
are independent from the implementer and read-only. Use browser automation and
the provided screenshots to inspect the working product, not only the docs or
code.

Do not pass the implementation because objects exist, console is clean, network
requests succeeded, or overflow is absent. First state what the screenshot looks
like in one sentence. If it looks like a dashboard, card dump, report page, or
raw object viewer, mark P0 fail.

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
You are the User acceptance subagent for Multi-Agent Harness Agent Workbench.
You are independent from the implementer and read-only. Act like an operator
trying to understand and control a multi-agent team. Use browser automation and
the provided screenshots to experience the product.

You are not reviewing code. Fail when the page feels like a dashboard instead
of a workbench, when the first action path is unclear, when AgentMember is only
a card, when Team/Goal/Task/Docs feel disconnected, or when mobile/tablet turns
into a long report.

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

- missing page-local layout contract in any changed page spec;
- missing changed-core-module option loop or Reviewer decision;
- missing implementation Questioner/Critic screenshot comparison against the
  page-local layout contract;
- known failed implementation attempt not recorded as rejected;
- no browser screenshot evidence;
- missing PM/User browser acceptance agent output;
- PM/User roles were not run by two independent subagents;
- primary page is blank or raw/debug-only;
- primary page reads as a stacked report/card dump instead of a Workbench;
- runtime JavaScript error on initial load;
- horizontal overflow on mobile caused by the layout;
- missing AgentMember realtime/detail surface when the change claims to support
  agent observation.
- screenshot first impression is dashboard, report, card dump, or raw object
  viewer;
- PM/User acceptance relies on data presence, console cleanliness, network
  success, component presence, or lack of overflow instead of product shape and
  operator flow.

May be waived only with rationale, owner, and follow-up task:

- non-critical accessibility issues;
- performance regressions caused by known sample-data volume;
- partial graph/Kanban behavior while the read model is not yet available;
- missing live streaming when polling state is visibly accurate and documented;
- web-quality tool unavailable in the local environment.

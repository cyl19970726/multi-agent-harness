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

- desktop screenshot, normally 1440 x 1000;
- tablet screenshot, normally 900 x 1180;
- mobile screenshot, normally 390 x 844;
- console output with no runtime errors and no React key/layout warnings;
- proof of no page-level horizontal overflow at each viewport;
- proof that raw/debug surfaces are not primary;
- proof that selecting a member shows realtime activity and send-message UI;
- proof that graph and Kanban/lane projections are both reachable where the
  design claims they exist.

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

- no browser screenshot evidence;
- primary page is blank or raw/debug-only;
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

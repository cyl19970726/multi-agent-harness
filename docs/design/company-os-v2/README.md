# Company OS V2 visual direction

V2 is a visual and interaction redesign built on the accepted Company OS
product model. V1 remains the historical functional baseline; its generated
Expected images were not promoted into V2 and did not constrain V2 composition.

```text
V1 Actual = current browser baseline
V2.2 Expected = generated product and visual intent, approved for implementation
V2.2 Fixture Actual = deterministic browser implementation evidence
V2.2 Store-live Actual = browser evidence from an authority-labelled Harness Store projection
```

The redesign starts with six P0 mother pages:

1. Company Home;
2. Docs Workspace;
3. Organization;
4. Lead Agent Workspace;
5. Business Module;
6. Work with Milestones and WorkItems.

These pages establish the shell, brand language, object hierarchy, document
grammar, organization collaboration, contextual right rail, and work density.
Document Focus, Standing Agent Focus, Milestone Focus, typed WorkItem pages,
Approvals, Finance, Mission/Wave, Agent Team, Workflow, Human Member, and
governance pages are derived only after this direction is reviewed.

Canonical product inputs:

- [`../../prd.md`](../../prd.md)
- [`../../architecture-map.md`](../../architecture-map.md)
- [`../../company-os/frontend-information-architecture.md`](../../company-os/frontend-information-architecture.md)
- [`../../company-os/organization-and-actors.md`](../../company-os/organization-and-actors.md)
- [`../../company-os/collaboration-and-agent-work.md`](../../company-os/collaboration-and-agent-work.md)
- [`../../company-os/work-items-and-approvals.md`](../../company-os/work-items-and-approvals.md)

Review artifacts:

- [`page-matrix.md`](page-matrix.md)
- [`visual-language.md`](visual-language.md)
- [`visual-contract.json`](visual-contract.json)
- [`v2.2-revision.md`](v2.2-revision.md)
- [`v2.2-asset-inventory.md`](v2.2-asset-inventory.md)
- [`review-v2.2.html`](review-v2.2.html) — V2 functional base → V2.2
  art-directed workbench review;
- [`expected-vs-actual-v2.2.html`](expected-vs-actual-v2.2.html) — V1 browser
  baseline → V2.2 generated direction → V2.2 browser implementation;
- [`expected-vs-store-live-v2.2.html`](expected-vs-store-live-v2.2.html) — the
  four-way truth review: V1 Before → V2.2 Expected → fixture Actual →
  Store-live Actual;
- [`comparison-manifest-v2.2.json`](comparison-manifest-v2.2.json) — hashes,
  routes, fixture mode, and truth classification for all six pages;
- [`store-live-comparison-manifest-v2.2.json`](store-live-comparison-manifest-v2.2.json)
  — authoritative source metadata, hashes, routes, and assertions for the six
  Store-live pages;
- `prompts/`
- `expected/`
- `actual/` — deterministic-fixture browser evidence;
- `store-live-actual/` — browser renders backed by an archived Harness Store.
- [`approval-action-v1/review.html`](approval-action-v1/review.html) — the first
  governed browser interaction: requested, invalid capability denied, approved,
  and rejected, with ActionCommand/audit evidence in the adjacent manifest.
- [`workitem-action-v1/review.html`](workitem-action-v1/review.html) — Store-live
  WorkItem lifecycle proof from explicit Standing Agent execution through
  accountable Human completion, without creating a Payment.

Reproduce the Store-live evidence with
`pnpm visual:capture:company-os-v2:live`, then update the checked-in comparison
with `pnpm visual:compare:company-os-v2:live`. The seed stops at requested Human
approval and a pending ¥3,000 Commitment; it must not create a Payment.

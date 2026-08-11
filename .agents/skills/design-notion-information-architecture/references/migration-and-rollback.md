# Migration and rollback

## Treat staging as an isolated system

A duplicated Notion area is not automatically an isolated fork. Before editing staging, inventory and verify:

- linked database views and their underlying data sources;
- relations and rollups in both directions;
- synced blocks;
- buttons, database automations, templates, and formulas with references;
- embeds, external automations, permissions, and public links;
- navigation and agent entry points.

Any staging element that still writes to production is a hard blocker. Label staging clearly, for example `STAGING · DO NOT CLAIM`, and disable claim/dispatch actions until isolation passes.

## Keep production authoritative during the trial

Continue active execution in production while staging is being shaped. Do not double-write current state. Structural edits belong in staging; production receives only ordinary work updates until the agreed cutover window.

Use a closed, representative work item for the first migration sample. Include enough complexity to exercise multiple runs/candidates, specification, execution evidence, and review without risking an active workflow.

## Maintain a migration ledger

For every migrated object, record:

| Field | Purpose |
|---|---|
| Source object | stable source identity and title |
| Target object | target identity and container |
| Target parent | intended module, hub, database, or archive placement |
| Action | keep, transform, merge, split, archive, or defer |
| Authority after cutover | where future updates belong |
| Relationship rewrite | relations, backlinks, linked views, and entry points to rebuild |
| Validation | content, relation, view, permission, and rendering checks |
| Exception | unresolved loss, ambiguity, or manual follow-up |

Never claim fidelity from record counts alone. Compare meaningful content, attachments, comments when in scope, relations, statuses, and rendered layouts.

## Use staged migration gates

1. **Discover:** inventory production and identify active workflows.
2. **Isolate:** duplicate or construct staging and prove no writes reach production.
3. **Model:** build target databases, wiki, templates, and semantic relations.
4. **Sample:** migrate one closed representative case and selected canonical docs.
5. **Review:** test reading journeys, lifecycle behavior, links, views, and permissions.
6. **Prepare cutover:** define a short freeze, delta capture, entry-point switch, owner, and rollback triggers.
7. **Cut over:** reconcile the delta once, switch authoritative entry points, and announce the new authority.
8. **Observe:** keep the legacy system as a read-only snapshot until acceptance criteria and observation period pass.

Do not delete the legacy system as part of cutover.

After every structural move, re-fetch both the moved object and its surrounding hubs. Verify parentage, relation targets, linked-view data sources, direct deep-link labeling, and the absence of newly orphaned pages. Moving a page successfully is not proof that the information architecture still works.

## Define rollback before cutover

Before cutover, rollback means abandoning or archiving staging while production remains authoritative.

After cutover, rollback means:

1. freeze new writes in the target;
2. capture the target-only delta;
3. restore the production entry point and claim/dispatch path;
4. declare the legacy system authoritative again;
5. preserve the failed target for diagnosis rather than deleting evidence.

Specify who can trigger rollback and measurable triggers, such as missing active work, broken claim actions, incorrect relations, permission loss, or unreconciled content differences.

## Verify the final operating state

Test as the actual user or agent persona, not only as the migration operator. Verify:

- entry point reaches the intended authority;
- active work remains claimable exactly once;
- filtered views show the correct records;
- relation navigation works in both required directions;
- no synced content or automation changes the legacy system;
- current canonical documents are discoverable and superseded ones are labeled;
- rollback materials and source-to-target mappings remain accessible.

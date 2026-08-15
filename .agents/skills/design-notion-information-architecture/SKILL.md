---
name: design-notion-information-architecture
description: Audit, design, review, or migrate governed Notion workspaces and page systems. Use when Codex must decide how pages, databases, wikis, hubs, views, links, and relations should work together; repair navigation or generic "Related pages" patterns; redesign development or canonical-document systems; create an isolated staging copy; plan a reversible migration; or validate a Notion information architecture before cutover.
---

# Design Notion Information Architecture

Design the human reading and operating system; use Notion-native tools or official Notion skills for search, read, create, and update mechanics. Do not duplicate connector syntax here.

## Load the right references

Read `references/architecture-principles.md` for every task. Then read only the references required by scope:

- Development work, runs, specs, or reviews: `references/development-system-pattern.md`
- Canonical docs, wiki, section hubs, or page navigation: `references/knowledge-system-pattern.md`
- Any copy, restructuring, migration, cutover, or rollback: `references/migration-and-rollback.md`
- AgentFirm work: `references/agentfirm-profile.md`
- Final design review or pre-cutover validation: `references/review-checklist.md`

## Establish scope before changing anything

1. Identify the people, recurring questions, and actions the system must support.
2. Locate the current entry points and the authority for each important fact.
3. Inventory pages, databases and data sources, views, relations and rollups, synced blocks, buttons, automations, templates, and external links.
4. Build three distinct maps: the container tree, the authority/relationship graph, and the reader or operator journey. A tidy sidebar alone is not an architecture.
5. Classify each object as authority, workflow record, report/artifact, navigation/view, or archive/snapshot.
6. Separate workspace operating modules from the product or subject model described by documents. A documented domain is not automatically a top-level workspace module.
7. Separate a design request from change authorization. Do not mutate production unless the user explicitly authorized implementation.

If current state is ambiguous or cannot be read, report the uncertainty. Do not invent database properties, page IDs, relations, or migration completeness.

## Design from reading journeys

For every audience, write the shortest path from entry to answer to action. Assign exactly one authority for each governed concept, then choose its container:

- Use a page for a singular, stable subject that benefits from authored hierarchy.
- Use a database for repeatable records with shared metadata, lifecycle, ownership, or views.
- Use a wiki for canonical documents that need governance, discovery, and deprecation.
- Use a hub as navigation over authorities; do not let it become a second authority.
- Use a view to expose source records in context; do not copy records to create another list.

Name relations by meaning, such as `Task`, `Specification`, `Reviews`, `Reviewed by`, or `Supersedes`. Reject generic `Related pages` and URL fields that impersonate Notion relations. For AgentFirm, do not introduce `Run` as a default second Task authority.

Choose top-level modules from recurring user operations and maintained systems, not from the nouns in the product architecture. Place product domains inside the Docs System as document taxonomy or curated hubs unless they have real operational records, actions, ownership, and lifecycle in the workspace.

## Produce an auditable proposal

Include, in proportion to the task:

1. Current-state inventory and observed failure modes.
2. Target page/database/wiki model and authority map.
3. Human reading journeys, page layouts, and top-level module rationale.
4. Source-to-target mapping, parent/container placement, relation semantics, linked-view wiring, and lifecycle ownership.
5. Staged implementation, validation evidence, cutover, and rollback plan.
6. Explicit assumptions, unresolved decisions, and destructive actions requiring approval.

Prefer a compact table for authority or field mappings and a small diagram for multi-step reading or migration flows. Keep operational IDs and live status outside the reusable design; rediscover them at execution time.

## Execute safely

Use the smallest representative sample first. For structural redesign, work in an isolated staging area, prove that linked views and automations point to staging sources, and prohibit production/staging double writes. Preserve content provenance and source-to-target traceability. Do not delete or archive the old authority until validation and cutover acceptance are recorded.

When a request includes implementation, perform the staged changes, inspect the rendered result, and validate both structure and human reading experience. Stop before cutover if any hard blocker in `references/review-checklist.md` remains.

## Close with evidence

Report what was inspected, created, changed, left untouched, and how rollback works. Distinguish verified facts from recommendations. A migration is not complete merely because pages exist: relations, views, content fidelity, permissions, entry points, and active workflows must also pass review.

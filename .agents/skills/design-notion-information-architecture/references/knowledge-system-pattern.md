# Knowledge system pattern

## Distinguish canonical documents from navigation

Use a Canonical Docs Wiki for durable product, architecture, governance, and operating knowledge. Use section hubs to guide readers into the wiki and relevant operational systems.

A canonical document owns an answer. A hub owns a reading journey. A database view exposes a collection. Do not copy the canonical answer into every hub.

## Build a Docs System, not a pile of documents

A Docs System should normally contain:

- a small set of domain hubs or document chapters;
- the canonical document authority or wiki;
- Architecture Decisions when decisions have their own lifecycle;
- an Implementation Crosswalk when target-to-shipped alignment must be queried;
- explicit legacy, archive, and migration surfaces outside the default reading path.

Product domains such as Company, AgentTeam, Product Views, or Runtime usually belong here as second-level document hubs. Do not promote them to workspace-level operating modules unless the workspace actually runs those domains with maintained records and actions.

## Govern canonical documents

Use only metadata that supports real governance. Common fields include:

- document type or domain;
- owner/steward;
- status such as Draft, In Review, Current, Superseded, or Deprecated;
- review date when periodic review is meaningful;
- `Supersedes` / `Superseded by` when replacement history matters.

Do not treat “last edited” as proof that content is current. Do not add fields that no workflow maintains.

## Write canonical pages for retrieval

Prefer this page shape:

1. answer-first summary;
2. scope and decision boundary;
3. normative content or accepted design;
4. rationale and tradeoffs where needed;
5. evidence, provenance, or implementation crosswalk;
6. explicit successor/predecessor links and a small `Read next` section.

Separate normative statements from examples and historical context. Mark drafts and superseded pages visibly in the body as well as metadata when readers could land on them directly.

## Keep hubs small and intentional

A section hub should contain:

- one sentence explaining the domain;
- two to five common reader routes;
- a curated view or shortlist of current canonical documents;
- links to the operational system when action happens elsewhere.

Do not turn the hub into a manual mirror of the wiki. If every new document requires editing several index pages, the navigation model is too expensive.

## Replace generic related-page footers

Choose one of these instead:

- a contextual body link explaining why the target matters;
- a named semantic relation used by workflow or governance;
- automatically available backlinks for provenance;
- a short, editorial `Read next` list;
- a return link to a meaningful hub when the page is commonly entered directly.

Remove generic `Related pages` blocks that mix implementation records, neighboring topics, parent pages, and references without explaining their roles.

## Preserve provenance without clutter

Use an implementation crosswalk when canonical design must connect to code, work items, or decisions. Name each mapping by what it proves. Avoid turning the main document into a dump of URLs or activity logs.

Archive does not mean delete. Keep superseded knowledge discoverable through replacement history, while default views emphasize Current content.

## Wire the knowledge graph deliberately

Define and maintain only relationships with real reading, governance, or verification value. A common chain is:

`Canonical Doc → Governing ADR → Crosswalk Capability → Development Work / verified implementation evidence`.

Use linked views on hubs to render this graph in context. Record the source page, target parent, canonical successor, relation rewiring, and default view for every migrated document. Reject orphan pages, databases with no reader entry point, and relations that exist only because they were easy to add.

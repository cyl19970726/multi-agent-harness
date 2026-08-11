# Architecture principles

## Start with use, not containers

Design from recurring human questions and actions. A clean schema that makes readers hunt is still a poor information architecture. For each audience, define:

- entry point;
- question being answered;
- authoritative object;
- next action;
- safe way back or onward.

## Do not mirror the subject model into the workspace

The workspace information architecture and the product model documented inside it are different things. A product may contain Company, Team, Work, Runtime, or Machine concepts without needing top-level Notion modules with those names.

Create a top-level workspace module only when it has a recurring user operation, maintained records, ownership, lifecycle, and entry-to-action journey. Otherwise keep the concept inside the Docs System as a document domain or section hub. This prevents the product ontology from polluting the operator's navigation.

For a docs-first product workspace, top-level modules may be as small as Docs, Development, and staging administration. Add another module only when its actual operating system exists.

## Assign authority once

Every governed fact needs one owner and one lifecycle. Other locations may summarize or display it, but must link to or view the authority rather than becoming editable copies.

Distinguish:

- **Authority:** owns the current fact or decision.
- **View:** renders authority records for a context.
- **Hub:** curates routes into authorities.
- **Artifact:** records what happened or what was produced.
- **Snapshot:** preserves a historical state and must be visibly non-current.

Duplicated text can be useful for a short summary, but duplicated status, ownership, decisions, or acceptance state creates drift.

## Choose the container by behavior

Use a normal page when the subject is singular and the structure is primarily narrative. Use a database when instances repeat and require shared metadata, workflow, ownership, filtering, or reporting. Use a wiki when authored documents need canonical status, stewardship, review, and deprecation. Use a hub only to help readers choose where to go.

Do not create a database merely because information can be tabulated. Do not use a page tree to simulate records that need lifecycle queries.

## Use links with explicit meaning

Choose the lightest link that preserves meaning:

- **Body link:** supports a specific sentence or instruction.
- **Read next:** a small, curated continuation of the reading journey.
- **Backlink:** shows references automatically; it is not authored taxonomy.
- **Relation:** connects repeatable objects whose identity matters to workflow, governance, or reporting.

Name each relation after its role. `Work`, `Run`, `Owner`, `Decision`, `Specification`, `Reviews`, and `Supersedes` are meaningful. `Related`, `Links`, and `See also` hide why the connection exists.

Never store the URL of an internal Notion record as a substitute for a relation when identity, filtering, rollups, or referential integrity matter.

## Make pages answer-first

A governed page should usually expose, in this order:

1. title and plain-language purpose;
2. current status or decision, when applicable;
3. owner and next action, when actionable;
4. essential content and evidence;
5. contextual records or supporting documents;
6. a small, intentional onward path.

Avoid a large metadata wall above the answer. Hide operational fields from default reading views when they do not help the current audience.

## Keep navigation distinct from taxonomy

Navigation answers “where should I go next?” Taxonomy answers “what kind of thing is this?” Relations answer “how do these governed objects participate together?” Do not force one mechanism to do all three jobs.

Use a shallow, stable top-level navigation. Prefer curated entry points for common journeys and database views for changing collections. A page footer should not automatically enumerate everything connected to the page.

## Maintain three graphs together

Review three structures independently and then check that they agree:

1. **Container tree:** where pages, databases, archives, and systems live.
2. **Authority graph:** which records govern, supersede, implement, review, or derive from which other records.
3. **Journey graph:** how a reader or operator moves from entry to answer, evidence, and action through hubs and views.

A good tree with broken relations is incomplete. A rich relation graph with no usable entry point is also incomplete. Every important object needs an intentional parent, authority role, incoming/outgoing relationship policy, and at least one appropriate reading or operating surface.

## Design for change and recovery

Prefer reversible changes, explicit source-to-target mapping, and preserved history. Treat permissions, linked data sources, relations, synced blocks, buttons, automations, templates, and external integrations as part of the architecture, not implementation trivia.

The rendered reading experience and the underlying data wiring must both pass review.

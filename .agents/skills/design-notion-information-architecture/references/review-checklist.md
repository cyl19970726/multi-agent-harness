# Review checklist

## Hard blockers

Do not approve cutover while any item is true:

- A staging view, synced block, button, or automation can mutate production unexpectedly.
- The same governed state is editable in more than one authority.
- Active work, ownership, acceptance, or next action cannot be reconstructed.
- Required content, relation, attachment, permission, or provenance is missing.
- Production and staging require ongoing double writes.
- Rollback authority, trigger, entry point, or delta handling is undefined.
- The default reader can land on an unlabeled obsolete or staging page and mistake it for current truth.

## Architecture review

- Do top-level modules represent real workspace operating systems rather than product-domain nouns copied from the documents?
- Can each important fact be traced to exactly one authority?
- Does each database represent one coherent object and lifecycle?
- Are hubs, views, artifacts, snapshots, and authorities visibly distinct?
- Are page, database, and wiki choices based on behavior rather than visual preference?
- Can the design support retries, multiple candidates, deprecation, and history without overwriting evidence?
- Does every important page or database have an intentional parent, authority role, incoming/outgoing relationship policy, and reader entry point?
- Are there loose root-level documents, orphan databases, or hidden records with no maintained view?

## Link and relation review

- Does every modeled relation have a specific name and operational meaning?
- Are internal identities represented as relations when filtering, rollups, or integrity matter?
- Have generic `Related pages` blocks been removed or replaced intentionally?
- Are body links contextual and `Read next` lists short and curated?
- Are backlinks used as provenance rather than mistaken for authored taxonomy?
- After moves or migrations, do relations, rollups, linked views, templates, and backlinks still point to the intended authority and data source?

## Human reading review

- Can a new reader understand the page's purpose in the first screen?
- Are current decision, state, owner, and next action prominent when relevant?
- Is essential content above secondary metadata and historical detail?
- Can each target audience move from entry to answer to action without guessing?
- Does mobile or narrow-width rendering remain usable when it matters?
- Can a reader distinguish an operating module, a document domain, and a product concept without inferring the distinction from naming alone?

## Operational review

- Do views use the intended data source and filters?
- Do templates create records with correct relations and defaults?
- Are buttons and automations disabled in staging or proven to target staging only?
- Do permissions match the intended readers and editors?
- Are active workflows, integrations, and agent entry points tested end to end?

## Migration and recovery review

- Does every source object have a recorded disposition?
- Were representative content and relationships compared, not only counts?
- Was a closed representative sample accepted before active migration?
- Is the cutover delta captured once under a defined freeze?
- Is the legacy authority retained read-only for the observation period?
- Has rollback been rehearsed or at least checked step by step against actual entry points?

## Review output

Record:

1. verdict: Accept, Accept with conditions, or Reject;
2. hard blockers and owners;
3. evidence inspected;
4. reading-journey findings;
5. data-wiring and lifecycle findings;
6. required follow-up and rollback readiness.

Do not accept based on appearance alone. Do not reject a sound architecture only because it uses fewer databases or decorative elements than the previous system.

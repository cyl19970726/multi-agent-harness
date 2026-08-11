# AgentFirm profile

## Scope and authority

Treat the AgentFirm Home's current authority notice as the sole authority selector. Locate the current root and entry points at run time; do not rely on stored page or database IDs.

Locate and read the current AgentFirm Notion information-architecture design or migration record when available. Verify its status and resolve contradictions with newer accepted decisions before implementation.

## Target architecture hypothesis

Validate, rather than blindly assume, this direction:

- `Development Work` for durable work items and overall state;
- `Delivery Runs` for candidate, retry, provider, or execution attempts;
- `Development Documents` for Specification, Execution Report, and Review Report;
- `Canonical Docs` for durable product, architecture, governance, and operating knowledge;
- section hubs as curated navigation, not duplicate authorities.

Execution journal and completion summary belong inside the Execution Report. Generic `Related pages` must be replaced with contextual body links, curated `Read next`, backlinks, or named semantic relations.

Do not mirror the AgentFirm product ontology into the Notion root. Company, Organization, AgentTeam, Work, Runtime, and Machine are subjects described by product documents; they become top-level Notion modules only if an actual maintained operating surface exists. In the current docs-first stage, validate a minimal root centered on the Docs System, Development System, and clearly isolated migration/archive administration. Keep Product Constitution, Company model, AgentTeam model, Product Views, and Runtime model inside the Docs System.

## Staging convention

Use a clearly labeled isolated area such as `AgentFirm — IA v2 Staging · DO NOT CLAIM`. The exact title may change, but it must remain visibly non-production and must not expose working claim, dispatch, button, or automation paths until isolation is verified.

The current authority continues to receive normal updates for active work during a staging trial. Do not write current execution state to both systems.

## Representative sample

Prefer a closed, representative Wave as the first full migration sample. Confirm that it is closed before use. The sample should exercise:

- one durable Work item;
- multiple candidate or execution Runs when present;
- a governing Specification;
- Execution Report content, including journal and completion;
- Review Report and acceptance evidence.

If the preferred sample is no longer closed, accessible, or representative, select another closed case and record why.

## AgentFirm-specific discovery

Before designing or migrating, rediscover:

- the current AgentFirm Home and agent entry point;
- active Waves and records that must remain untouched;
- current databases, data sources, templates, relations, and linked views;
- buttons or automations used for claim, dispatch, status, or reporting;
- canonical product and architecture documents;
- Architecture Decisions and Implementation Crosswalk pages;
- existing generic `Related pages` properties or page sections;
- external integrations and permissions that affect execution.

Also verify that every root item is a real workspace system. Move loose architecture chapters, ADRs, crosswalks, implementation specs, and migration records under their owning Docs, Development, or migration/archive module rather than leaving them at the root.

Do not store live Wave status, database IDs, or page IDs in this skill. Put dynamic findings in the migration ledger or current Notion design record.

## Cutover and rollback

Cutover requires an explicit review decision, a short update freeze, one delta reconciliation, and an entry-point switch. Keep the original AgentFirm system as a labeled read-only Legacy Snapshot through the observation period.

If target execution fails after cutover, freeze target writes, capture target-only changes, restore the original agent entry and claim path, and declare the legacy system authoritative again. Do not delete either side while reconciling failure evidence.

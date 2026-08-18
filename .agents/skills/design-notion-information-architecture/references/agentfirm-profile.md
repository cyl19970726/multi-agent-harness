# AgentFirm profile

## Scope and authority

Treat the AgentFirm Home authority notice as the sole authority selector.
Rediscover the current Home, Development System, Playbook, databases, and views
at run time; never rely on stored page or database IDs.

Notion owns current product intent and the simple development Task loop. The
repository owns implemented code, tests, schemas, CI, and versioned contracts.
When they diverge, record the gap instead of silently rewriting either side.

## Current AgentFirm information architecture

Validate this minimal operating shape:

- `Development Tasks`: one current authority per development outcome;
- `Development Reviews`: immutable review history, one row per submission;
- `Development Documents`: typed Specifications and durable supporting records;
- `Canonical Docs`: product, architecture, governance, and operating knowledge;
- curated section hubs: navigation, not duplicate authorities.

Do not make Delivery Run, Candidate, readiness gate, protocol-event ledger, or
merge authorization part of the default development path. Historical records
may remain in a hidden Legacy/Advanced area but must not appear in default
views, templates, or instructions.

## Root and module discipline

Do not mirror every AgentFirm product noun into the Notion root. Product
Constitution, Company model, AgentTeam model, Product Views, Runtime model, and
Docs/Company Memory are chapters inside the Docs System unless a real maintained
database or operator surface justifies a separate system.

Replace generic `Related pages` with contextual body links, curated `Read next`,
backlinks, or named relations such as `Task`, `Specification`, `Reviews`, and
`Supersedes`.

## Development sample

Use one representative closed Task to verify:

- goal and acceptance criteria;
- one Task lifecycle;
- a governing Specification;
- exact-revision Review history, including Changes Required when present;
- GitHub Issue/PR/CI/merge links;
- no Task/Run or Task/document status double-write.

## Discovery and migration

Before designing or migrating, inspect:

- the current authority notice and entry points;
- active Tasks that must remain writable;
- Task, Review, Document schemas, relations, templates, and views;
- current buttons or automations used for assignment, review, or closeout;
- canonical product and architecture documents;
- Architecture Decisions and Implementation Crosswalk;
- generic relation sections and duplicate execution fields;
- external integrations and permissions.

Use an isolated staging root for risky IA surgery. Keep production authoritative
until a short freeze, delta reconciliation, review, entry-point switch, and
rollback rehearsal complete. During a staging trial, never double-write current
Task state to both systems.

## Runtime mental-model guardrail

Notion documents the product model; it must not invent runtime objects. Current
AgentFirm runtime authority is: one machine-scoped NodeDaemon owns local
AgentSessions, AgentTeams are durable and Mission-less with immutable Node
placement, `AgentMember` is the sole durable agent identity and
`TeamMembership` records only participation, provider questions use correlated
Messages, and AgentSession permission is frozen before start. Do not
reintroduce a second provider-interaction or permission lifecycle through a
Notion template or Spec.

# Company OS Governance

```text
status: canonical Company OS contract
owner_role: governance
canonical_for: module and organization change governance, risk tiers, and human gates
```

## Purpose

Governance lets the company deliberately change its document structure,
organization, permissions, and business operations. It prevents new requests
from becoming orphaned pages, unowned Agents, or untraceable automated actions.

Governance is not a single all-powerful Agent. It separates discovery,
structure, organization, domain review, accountable leadership, and human
authority.

## Standing roles

| Role | Primary responsibility | Cannot replace |
| --- | --- | --- |
| Docs Governance Agent | Designs modules, document trees, typed records, templates, views, relations, retention, and migration proposals. | Business accountability or required human approval. |
| Work Governance Agent | Classifies durable commitments, routes responsibility, checks required fields, and surfaces Approval/Finance/execution impact. | Business ownership, human approval, or executor-native planning. |
| Finance Governance Agent | Validates monetary requests, evidence, budgets, controls, and authorized record transitions. | Human payment authority or permission to infer settlement. |
| Org / HR Governance Agent | Evaluates capability gaps and governs Business Agent proposal, provisioning, reporting, permissions, evaluation, and retirement. | Approval of its own privileges or mandatory Human gates. |
| Lead | Owns business outcome; sponsors proposals, resolves trade-offs, and accepts completed change. | Mandatory independent review or human gate. |
| Finance reviewer | Reviews financial design, budget/payment impact, controls, and separation of duties. | Accountable human payment approver. |
| Legal reviewer | Reviews legal obligations, jurisdiction, evidence, retention, and external counsel boundaries. | Human or authorized-counsel sign-off required by policy. |
| Human gate | Grants explicit authority for high-risk decisions reserved to people. | Evidence, review, or documented policy. |

The Lead may be a human or Standing Agent if policy permits. A human gate is a
named `ActorRef` with `actor_type=human`; an Agent is never presented as a
human approver.

## What requires a governance proposal

A proposal is required before material change to a document module, directory,
database/template, relation, retention rule, company-wide view, organization
unit, standing Agent role, human membership, accountability, capacity,
permission, sensitive access, financial authority, or a new business domain
that has no approved module.

Each proposal includes purpose, scope, source documents, affected records and
views, responsible actors, permission impact, risk tier, reviews, implementation
and rollback plan, and success criteria.

## Review flow

```text
New business need or governance gap
  -> Docs Governance Agent maps documents, records, and relations
  -> Work Governance Agent maps durable commitments and delivery
  -> Finance Governance Agent maps monetary effects and controls
  -> Org/HR Governance Agent maps actors, authority, and capacity
  -> Lead sponsors the selected design
  -> Finance / Legal reviewers assess domain impact as required
  -> Human gate approves when policy or risk requires it
  -> create or change the module / organization
  -> audit outcome and review it after use
```

The Governance Agents can prepare one coordinated proposal, but none can
unilaterally approve its own additional access, authority, or reporting line.
Accepted changes retain links to their request, reviews, approval, created
objects, and later evidence of effectiveness.

## Risk tiers

| Tier | Example | Minimum decision path |
| --- | --- | --- |
| R0 — reversible | Private draft page or non-sensitive view. | Accountable owner; audit event. |
| R1 — operational | Reusable template, normal routing, or low-risk Agent role without new sensitive access. | Lead acceptance; relevant review when affected. |
| R2 — controlled | New module, cross-module relation, external collaborator, or increased data/tool access. | Lead + affected reviewer(s) + human approval where policy applies. |
| R3 — regulated / irreversible | Financial commitment/payment, legal filing, authority delegation, privileged data access, or production-wide governance change. | Independent Finance/Legal review as applicable + named human gate; complete audit evidence. |

Risk is the greatest affected dimension (money, legal exposure, privacy,
security, external commitment, or reversibility). Policies may elevate a tier;
they must never silently lower one.

## Governance output

An approved module design produces linked objects, not just a folder:

```text
ModuleDesign
  -> document hierarchy and templates
  -> typed records and relation rules
  -> views, metrics, and WorkItem routes
  -> Actor/organization responsibilities
  -> permissions, approvals, retention, and audit policy
```

A new Standing Agent additionally has a sponsoring Lead, organization unit,
purpose, capability and permission limits, capacity, maintained documents,
escalation target, review cadence, and retirement/replacement plan.

## Safety invariants

- A source document records intent; it does not itself confer authority.
- No Agent approves its own privilege escalation, financial action, or required
  independent review.
- External people are `ActorRef(actor_type=external)`, have explicit scope and
  expiry, and do not inherit internal access by default.
- Tasks, approvals, decisions, and changes record submitter, accountable owner,
  contributors, reviewers, and approvers where applicable.
- Provider sessions, chat, and raw thinking are not governance evidence. Only
  durable decisions, evidence, and auditable state changes are authoritative.

---
name: company-module-designer
description: Design or revise a governed Company OS business module from a new operating need. Use when a user introduces a new domain such as trademark registration, procurement, hiring, or content operations and needs its documents, typed records, relations, organization roles, WorkItems, approvals, finance links, permissions, views, automations, and archive policy defined before implementation.
---

# Company Module Designer

Turn an unfamiliar business process into a reviewable `ModuleDesign`. Do not
create UI code or mutate live company records while designing the module.
This skill is a procedural capability, not product authority.

## Load the product contracts

Read these repository documents before proposing a design:

- `docs/company-os/document-system.md`
- `docs/company-os/module-design.md`
- `docs/company-os/organization-and-actors.md`
- `docs/company-os/work-items-and-approvals.md`
- `docs/company-os/financial-relations.md`
- `docs/company-os/governance.md`

Read [the output contract](references/module-design-contract.md) before writing
the deliverable.

## Design workflow

1. Identify the business event, accountable outcome, non-goals, and source
   documents. Mark unknown legal, financial, permission, and retention facts.
2. Reuse existing Documents, TypedRecords, Relations, Views, OrgUnits, Actors,
   WorkItems, and policies where their meaning is identical. Do not create a
   parallel record merely to simplify one page.
3. Define new record types and relations. Give every authoritative fact exactly
   one source of truth and name the standard fallback views.
4. Model responsibility explicitly: requester, submitter, assignee,
   accountable owner, reviewer, approver, contributor, and external party are
   separate roles even when one actor fills several of them.
5. Model financial effects as a chain. A budget, estimate, commitment, invoice,
   payment, refund, and metric are different records. Never infer Payment from
   Commitment or Approval.
6. Define governed Action Commands. Name required evidence, policy checks,
   permissions, human approval, idempotency, effects, and audit output.
7. Define organization effects as proposals. An Agent may recommend a role,
   unit, permission, or module; it cannot silently grant authority.
8. Specify ordinary document pages and standard views before proposing any
   custom page. Reserve custom code for a core page that must combine several
   record types or decision surfaces.
9. Define migration, archive, retention, failure, and rollback behavior.
10. Produce the design as JSON and a short human-readable rationale. Run:

```bash
python3 skills/company-module-designer/scripts/validate_module_design.py <design.json>
```

## Gate the design

Do not mark the design approved. Report it as `proposed` until the named human
authority decides. Block implementation when any of these remain ambiguous:

- authoritative source or record owner;
- human approval for money, legal submission, access, or organization change;
- finance relation type or currency;
- external-party scope;
- direct-write behavior outside governed Actions;
- missing standard-view fallback;
- migration or archival impact on existing records.

Hand the approved design, its approval reference, fixture, and visual scenarios
to `$company-page-builder` only after the decision exists.

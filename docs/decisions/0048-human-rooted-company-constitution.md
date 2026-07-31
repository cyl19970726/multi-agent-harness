# ADR 0048: Human-rooted Company Constitution

```text
status: accepted target contract; implementation pending
date: 2026-07-31
owner_role: company-authority-architecture
```

## Context

ADR 0046 separates the durable Company Lead from a Supervising Operator and
Runtime Supervisor. ADR 0047 adopts a target one-command authority broker and
keeps grant/receipt grammar canonical in its product contract. Neither decision
defines who owns the Company authority root, how routine decisions reach Domain
Leads, or when execution must return to a Human.

A Company that asks the Human to approve every routine command does not operate
autonomously. A Company that lets a Lead, Domain Agent, runtime Supervisor, or
provider process create authority has no trustworthy root.

## Decision

Adopt the target [Human-rooted Company Constitution](../company-os/company-constitution.md).

The Human Principal is the continuous requester, Constitution owner, and
exception decider. The Company Lead triages intent, sets priority, allocates
declared capacity, routes Work, and replans within an exact Human-approved root.
Domain Leads execute autonomously within their domain and may subdelegate only
through one strictly attenuating `ScopedPermissionGrant` lineage and durable
Assignments.

The Runtime Supervisor remains outside the authority hierarchy. It proves
delivery, MemberRun/native-session provenance, generation, runtime control, and
recovery state; it cannot create intent, choose priority, grant authority,
approve exceptions, or accept Company Work.

Child delivery or execution requires one atomic reservation of execution
budget, concurrency, and delegation depth. Expiry or revocation fences
descendants and unconsumed authority. Routine in-scope decisions emit a
canonical audit digest and proceed without a new Human prompt. Only
constitutional, permission, protected-effect, resource-ceiling, missing-owner,
or ambiguous-recovery exceptions enter the Human queue.

Company OS Work remains canonical for intent, responsibility, Assignment,
Approval, acceptance, and result. GitHub/Git remains canonical for software
delivery evidence. Store-backed Organization and Work views must expose the
actual hierarchy, grant lineage, reservations, delivery, audit, exceptions,
and GitHub refs without inference.

ADR 0047 and its product contract continue to own broker object grammar,
immutable generation/digest rules, identity binding, and one-command denial
semantics. Its V1 proof is a one-node lineage with no nested delegation.
Constitutional child delegation is a later implementation phase and must first
extend that canonical grammar and acceptance.

The root `ChangePermission` Approval remains an exact R3 proposal bound to the
Company, Constitution version/digest, Company Lead ActorRef, and root
`grant_id + grant_generation + canonical_grant_digest`. This decision and its
merge do not request, approve, or activate that proposal.

## Consequences

- Human authority remains continuously attributable without creating a Human
  bottleneck for routine, bounded work.
- Company leadership, domain execution, and runtime supervision have separate
  responsibilities and evidence.
- Delegation cannot union sibling authority or outrun its parent resource and
  time ceilings.
- Reservation races, cascading revocation, indeterminate recovery, exception
  routing, and Store-backed UI truth become required acceptance surfaces.
- The existing Company Store and broker remain unchanged until separately
  implemented and activated.

## Rejected alternatives

- **Supervisor as Company principal:** runtime ownership and provider control
  are not business judgment or permission authority.
- **Agent-created root authority:** lets the grantee approve or expand its own
  power.
- **Human approval on every command:** obscures true exceptions and prevents
  bounded autonomous operation.
- **Union multiple grants:** makes attenuation and revocation
  non-reconstructable.
- **Treat GitHub merge as Work acceptance:** confuses software delivery
  evidence with Company responsibility and authority.

## Validation

Acceptance requires deterministic proof that:

- the Human-rooted topology and one active lineage resolve from Store truth;
- a routine exact Work command can proceed without a Human prompt while a
  protected or broadened command reaches denial or the exact exception path;
- concurrent child requests cannot oversubscribe budget, concurrency, or
  depth;
- expiry, revocation, and indeterminate recovery fence every affected
  descendant and receipt;
- the Supervisor cannot make Company authority or acceptance decisions; and
- Organization and Work views render Store relations, reservations, audits,
  exceptions, and GitHub delivery refs without inferred joins.

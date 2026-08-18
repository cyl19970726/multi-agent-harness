# ADR 0048: Human-rooted Company Constitution

> Successor (DOC-16 row, DEV-40 flip 2026-08-18): [DOC-105](https://app.notion.com/p/3be49a4fa379817aa594fd8e7331c30d) + [DOC-108](https://app.notion.com/p/3be49a4fa37981afa320f6c8a5f3a8b4).


> **Partially superseded by DOC-108 (legacy CompanyOS retirement, 2026-08-17).**
> The Company-constitution object model is retired and not current authority.
> Per the DOC-16 Merge row, these principles SURVIVE as current authority
> (carried by the execution-foundation docs): Human authority over protected
> or irreversible external effects; the Runtime Supervisor cannot create
> intent, grants, or Work acceptance; attenuation, revocation, and
> no-authority-widening.

```text
status: superseded (DOC-108); was: accepted target contract; implementation pending
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

Adopt the target Human-rooted Company Constitution.

The Human Principal is the continuous requester, Constitution owner, and
exception decider. The Company Lead triages intent, sets priority, allocates
declared capacity, routes Work, and replans within an exact Human-approved root.
Domain Leads execute autonomously within their domain and may subdelegate only
through one strictly attenuating `ScopedPermissionGrant` lineage and durable
Assignments.

Human request provenance passes through Supervising Operator capture and
Runtime Supervisor delivery evidence before the Company Lead promotes it into
Docs, Work, priority, replan, or an exception. Intake and transport create no
Company authority, and `WorkItem.requested_by` preserves the actual originator.

The Runtime Supervisor remains outside the authority hierarchy. It proves
delivery, MemberRun/native-session provenance, generation, runtime control, and
recovery state; it cannot create intent, choose priority, grant authority,
approve exceptions, or accept Company Work.

Temporary Team Members remain execution-only and bind exact Assignment,
TeamMessage, MemberRun, native session, and ProjectBinding. Durable child
Standing Agents require explicit Organization truth and an approved template
id/version/digest. Child ProjectBinding and template selectors must be equal to
or narrower than the parent allowlists and cannot be retargeted later.

Child delivery or execution requires one atomic reservation of execution
budget, concurrency, and delegation depth. Expiry or revocation fences
descendants and unconsumed authority. A strictly attenuating child or
approved-template instance consumes the existing Human-approved envelope and
is routine only when policy is known, the effect is reversible, blast radius is
bounded, and there is no material external commitment. Unknown policy,
irreversibility, broad/root-security blast radius, external commitment, or any
scope/ceiling expansion enters the Human queue.

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

The root `ChangePermission` Approval is one evidence-shaped R3 request bound to
the Company, Constitution version/digest, Company Lead ActorRef, root
`grant_id + grant_generation + canonical_grant_digest`, exact acceptance
evidence, policy, subject, command, approver, and expiry. Preparation and
acceptance gathering do not request or decide it. After acceptance, one
governed request receives one Human decision and the exact activation command
may run only from that same approved, unexpired decision. This ADR and its
merge do none of those things.

## Consequences

- Human authority remains continuously attributable without creating a Human
  bottleneck for routine, bounded work.
- Company leadership, domain execution, and runtime supervision have separate
  responsibilities and evidence.
- Delegation cannot union sibling authority or outrun its parent resource and
  time ceilings.
- Intake delivery, temporary Team membership, template similarity, and
  ProjectBinding selection create no implied Company authority.
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
- the reversibility, blast-radius, external-commitment, ProjectBinding, and
  approved-template tests distinguish routine attenuation from an R3 exception;
- concurrent child requests cannot oversubscribe budget, concurrency, or
  depth;
- expiry, revocation, and indeterminate recovery fence every affected
  descendant and receipt;
- the Supervisor cannot make Company authority or acceptance decisions; and
- Organization and Work views render Store relations, reservations, audits,
  exceptions, and GitHub delivery refs without inferred joins.

# Human-rooted Company Constitution

```text
status: target canonical contract; not implemented
owner_role: Human Principal with Company Governance
canonical_for: Company authority root, delegation invariants, exception routing, and Store-truth operator requirements
```

## Current truth and target boundary

Company OS currently has durable Human and Standing Agent identities,
Organization records, WorkItems, Assignments, Approvals, Action policies, and
AuditEvents. ADR 0045 defines the explicit StandingAgent-to-execution relation,
and ADR 0046 separates the durable Company Lead from the Runtime Supervisor.
Company writes still use the service-side `HARNESS_COMPANY_OS_TOKEN`.

The [Scoped Company Authority Broker](scoped-authority-broker.md) defines
`ScopedPermissionGrant` and `CapabilityLeaseReceipt` as target contracts. They
are not implemented schemas or live authority. The Constitution, grant
lineage, atomic execution-budget/concurrency/depth reservations, routine audit
digest, and exception-only Human queue defined here are also **target
contracts only**. No current Store, API, CLI, Dashboard, skill, plugin, or
provider runtime implements them.

This document does not create an Actor, Approval, grant, receipt, reservation,
or queue item. It does not activate R3 authority. The Company root token
remains service-side and must never be delivered to a StandingAgent,
AgentMember, MemberRun, or provider-native session.

## Constitutional topology

```text
Human Principal
  continuous requester
  Constitution owner
  exception decider
  └── Company Lead
        triage, priority, capacity, and replanning
        ├── Domain Lead: Docs
        ├── Domain Lead: Work
        ├── Domain Lead: Organization
        └── other explicitly constituted Domain Leads
              autonomous execution inside one attenuating grant lineage
              └── bounded child Assignment and execution binding

Runtime Supervisor
  provenance, delivery, session control, and recovery only
  (outside the Company authority hierarchy)
```

Only durable Company Actors occupy the authority hierarchy. An OrgUnit,
membership, reporting relation, provider process, Team role, or online runtime
does not create authority by inference.

### Human Principal

The Human Principal remains the continuous source of Company intent even while
routine, in-scope work proceeds asynchronously. The Principal:

- owns and versions this Constitution;
- is the durable requester for the constituted Company operating loop;
- appoints or removes the Company Lead through governed Organization and
  permission changes; and
- decides only the exceptions that policy routes to the Human queue.

“Continuous” does not mean approving every command. It means routine authority
is an explicit, revocable attenuation of a Human-owned root, never authority
created by an Agent or runtime.

### Company Lead

Within an active root grant and the Constitution, the Company Lead may triage
incoming intent, set priorities, allocate declared execution capacity, route
Work to Domain Leads, and replan when evidence or capacity changes. It remains
accountable for the portfolio and escalation path.

The Company Lead cannot amend the Constitution, approve its own authority,
expand its grant, bypass an Approval, or convert operating priority into
Finance, legal, credential, Organization, or permission authority.

### Domain Leads and bounded executors

A Domain Lead may autonomously accept, plan, execute, and return Work inside
its exact domain, Assignment, and grant scope. It may subdelegate only when all
of these are true:

- the child has a durable Company Assignment and exact execution delivery;
- one parent-to-child `ScopedPermissionGrant` lineage is proved;
- every child scope dimension and resource ceiling is equal to or narrower
  than the parent;
- budget, concurrency, and depth are reserved atomically before delivery or
  execution; and
- the parent remains accountable and can reconstruct the child result.

Sibling, unrelated, expired, or revoked grants cannot be combined. A missing
edge is a denial, not a reason to infer authority.

### Runtime Supervisor

The Runtime Supervisor proves Supervisor generation, mailbox delivery,
MemberRun/native-session binding, runtime health, control acknowledgements,
and recovery state. It cannot originate Company intent, prioritize Work,
select a Domain Lead, grant permission, approve an exception, or claim Company
acceptance. Supervisor provenance is required execution evidence, never an
authority root.

## One attenuating grant lineage

Every brokered Company command must resolve to exactly one active
`ScopedPermissionGrant` lineage from the constituted Company Lead grant to the
executing leaf. Authorization is the intersection of every generation in that
lineage. It is never the union of sibling or historical grants.

Each child must strictly attenuate at least one authorization or resource
dimension and may never broaden Company, domain, Actor, WorkItem, Assignment,
correlation, permission, command, subject, payload, effect, validity, expiry,
lease TTL, successful-use budget, execution budget, concurrency, or remaining
delegation depth. Child expiry cannot outlive parent expiry. A parent cannot
delegate more depth than it retained.

The broker document remains canonical for grant and receipt object grammar,
immutable generation/digest rules, identity binding, dispatch, and denial
semantics. Its first proof is a one-node, one-use lineage and permits no nested
delegation. Child grants remain a later constitutional implementation phase;
their grammar must be added to that canonical broker contract and acceptance
before use.

## Atomic resource reservations

Before a child Assignment is delivered or an execution cycle starts, the
Company authority service must atomically reserve, against the exact parent
grant generation and digest:

- an execution-budget amount from a declared non-financial resource ceiling;
- one concurrency slot; and
- one delegation-depth unit.

All three succeed or none succeeds. A failed reservation delivers no child
authority and starts no child execution. Completion, cancellation, or expiry
reconciles the reservation according to a server-owned rule. A crash or
indeterminate outcome keeps capacity unavailable until reconciliation; it
must not silently release and permit duplicate work.

Execution budget limits provider/runtime consumption. It is not a Finance
Commitment, payment authorization, legal approval, or credential grant.

## Expiry, revocation, and recovery

Expiry or revocation of any generation fences its unconsumed receipts,
unstarted reservations, and every descendant grant immediately. It prevents
new effects but does not erase completed effects or their evidence.

Recovery must resolve the last durable receipt, ActionCommand, reservation,
Supervisor generation, and provider-native terminal state. An uncertain effect
is `indeterminate` and remains fenced until reconciled. Recovery never creates
a replacement grant, expands scope, releases uncertain capacity, or blindly
replays a command.

## Routine audit digest and Human exception queue

Every routine dispatch or child delegation must preserve a canonical audit
digest that binds:

```text
Constitution version + canonical digest
grant lineage ids + generations + canonical grant digests
Company Actors + StandingAgent/AgentMember/MemberRun/native-session binding
WorkItem + Assignment + delivered TeamMessage/correlation
reservation ids and budget/concurrency/depth amounts
ActionCommand/request/result and AuditEvent references
```

This digest is evidence, not a capability. Display metadata and UI labels
cannot change its meaning.

Routine work that satisfies the Constitution, one active lineage, resource
reservations, Action policy, and any existing Approval proceeds without a new
Human decision. The Human queue contains exceptions only:

- Constitution or root-grant creation, activation, expansion, replacement, or
  revocation;
- R3 permission or Organization authority changes;
- cross-domain work or a proposed child that cannot strictly attenuate;
- budget, concurrency, or depth exhaustion requiring a higher ceiling;
- Finance, payment, legal, credential, external-publication, or other
  protected effects requiring named Human authority;
- ambiguous identity, delivery, recovery, or indeterminate execution; and
- Work with no accountable Company Actor or applicable policy.

Queue placement is not approval. Only an exact, unexpired Human decision by the
required approver permits the named effect.

## Exact root Approval/R3 proposal

The constitutional root is reserved as this exact proposal:

```text
proposal_id: proposal-agentos-company-constitution-root-v1
approval_id: approval-agentos-company-constitution-root-v1
risk_tier: R3
effect: ChangePermission
requester: human:human-wcw-owner
approver: human:human-wcw-owner
grantee: agent:agent-agentos-lead
subject:
  company_id: agent-company
  organization_id: company
  root_org_unit_id: orgunit-agentos-root
  constitution_version: 1
  canonical_constitution_digest: required computed value at request time
  root_grant_id: grant-agentos-company-lead-root-v1
  root_grant_generation: 1
  canonical_grant_digest: required computed value at request time
decision_scope: activate only the exact root grant snapshot above
status: documentation-only proposal; no Store record, request, approval, or activation
```

The Actor and Organization ids above are current Store observations; the
proposal, Approval, grant, and digest values are reserved target inputs, not
claims that those records exist. Before an Approval can be requested, both
digests must be computed from the final canonical snapshots and the whole
proposal must be persisted through the governed Approval path. Any later
change to one bound value is a new proposal and Human R3 decision. Human
confirmation of this architecture, documentation merge, deployment, runtime
start, or Assignment delivery is not that activation decision.

## Concrete routine and exception example

Assume the constituted Company Lead routes the already `in_progress`
`work-agentos-org-role-permission-closure-v1` WorkItem to the Work Domain Lead.
The exact V1 leaf grant and delivered Assignment may allow only:

```text
work_item.transition
subject: work-agentos-org-role-permission-closure-v1
from: in_progress
to: in_review
payload: the exact frozen result, evidence, and outcome refs
```

That one command may proceed and emit its receipt, AuditEvents, and routine
audit digest without another Human prompt. A request to change any other
payload field, transition unrelated Work, alter Organization or permissions,
increase a reservation ceiling, pay money, perform a legal filing, use a
credential, or publish externally is denied or placed in the applicable Human
exception path. GitHub PR merge does not widen or satisfy this authority.

## Work and GitHub responsibility boundary

Company OS Work owns intent, accountable Actor, Assignment, Approval,
acceptance, result, and exception state. GitHub and Git own software delivery
evidence such as issues, branches, commits, pull requests, reviews, CI, and
releases.

A Company Lead or Domain Lead may update Company Work only through its active
grant and Action policy. A GitHub issue does not assign Company responsibility;
a PR review does not grant Company permission; and merge does not accept a
WorkItem, approve an exception, or activate authority. Company Work links
delivery evidence through explicit refs.

## Store-truth Organization and Work UI

The Organization UI must read the selected Company Store and show:

- the exact Human Principal, Company Lead, Domain Leads, OrgUnit memberships,
  and reporting relations;
- Constitution version/digest and effective grant lineage, generation,
  expiry, revocation, attenuation, and resource ceilings;
- reserved and available execution budget, concurrency, and delegation depth;
  and
- runtime/Supervisor health as a separate provenance panel, never as Company
  authority.

The Work UI must show accountable Actor, Assignment, delivery TeamMessage and
correlation, active leaf lineage, lease/ActionCommand/AuditEvent refs,
reservation state, exception state, result/evidence/outcome refs, and GitHub
DeliveryRefs. Missing Store relations stay visibly missing. The UI must not
infer relations from matching names, ids, provider sessions, the first
available row, or the selected Execution Space/Project Binding.

## Non-goals

- A universal provider, filesystem, shell, MCP, plugin, or cloud permission
  model.
- Human approval for every routine Company command.
- Finance authority disguised as execution budget.
- Authority inferred from Organization charts, chat, runtime health, GitHub,
  or receipt possession.
- Implementing schemas, APIs, CLI, UI, skills, plugins, or live authority in
  this documentation change.

## Short implementation sequence

1. Freeze and approve the Constitution version/digest plus exact root R3
   proposal without activating it.
2. Implement and accept the one-node V1 broker proof from ADR 0047, including
   durable allow/deny audit and root-token non-disclosure.
3. Extend the canonical broker grammar and Store transactions for strict child
   lineage, atomic budget/concurrency/depth reservations, cascading fences, and
   indeterminate recovery; then run deterministic race and denial tests.
4. Add the exception-only queue, routine audit digest, and Store-backed
   Organization/Work projections.
5. Request the exact Human R3 activation only after independent review proves
   scope attenuation, protected-effect denial, recovery, audit, and UI truth.

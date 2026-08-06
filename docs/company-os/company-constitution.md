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
Company writes still use the service-side `FIRM_COMPANY_OS_TOKEN`.
The [implementation truth matrix](implementation-truth-matrix.md) is canonical
for what those current surfaces can prove.

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
              ├── temporary Team Member
              │     execution-only; exact Assignment, MemberRun, and ProjectBinding
              └── approved-template Standing Agent
                    durable Actor, reporting relation, and child grant

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

### Human request intake and durable promotion

Human intent reaches the Company through an explicit provenance and promotion
chain:

```text
Human request + exact provenance
  -> Supervising Operator capture
  -> Runtime Supervisor delivery evidence
  -> Company Lead classification and promotion
       -> Docs update
       -> WorkItem create/update
       -> priority change
       -> execution replan
       -> Human exception
  -> Domain Lead execution
```

The intake envelope preserves the originating Human or other Actor, source
message or governed-surface ref, received time and channel, attachments or
evidence, requested urgency, and delivery ref. The Supervising Operator may
capture, sanitize, and route it. The Runtime Supervisor may prove delivery and
runtime facts. Neither role creates Company intent, responsibility, Work, or
authority merely by receiving or forwarding the request.

The Company Lead chooses whether and how to promote intake into durable Docs,
Work, priority, or replan truth. `WorkItem.requested_by` preserves the actual
originating Actor or record; it is not overwritten with the constitutional
Human by default. Unpromoted intake confers no authority. Ordinary
requirements, feedback, reprioritization, and replanning do not rotate or
reopen the root Approval unless they change the Constitution, root envelope,
or a protected boundary.

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
its exact domain, Assignment, and grant scope. Every child execution requires:

- the child has a durable Company Assignment and exact execution delivery;
- the exact temporary or durable identity, TeamMessage, MemberRun/native
  session, and ProjectBinding resolve without inference;
- budget, concurrency, and depth are reserved atomically before delivery or
  execution; and
- the parent remains accountable and can reconstruct the child result.

Only a durable Standing Agent child may receive Company command authority. For
that form, one parent-to-child `ScopedPermissionGrant` lineage must additionally
be proved, no scope or resource dimension may be broader than the parent, and
at least one authority or resource dimension must be strictly narrower.

There are two child forms:

- A **temporary Team Member** is execution-only and is bound to the exact
  Company Assignment, Agent Team Work/WorkDelivery, MemberRun, native session, and
  ProjectBinding. It never becomes a Company Actor, receives no Company grant
  as grantee, cannot possess, present, or transport the parent grant, cannot
  subdelegate Company authority, and returns evidence to the accountable
  Standing Agent. It operates solely under its execution binding. Any Company
  Action remains attributable to and dispatches through an eligible
  StandingAgent-bound leaf.
- A **durable Standing Agent** requires an explicit Organization record and
  reporting relation and may receive a child grant only from an approved
  Standing Agent template identified by exact template id, version, and
  canonical digest. Template approval is not inferred from a similar prompt,
  role, skill set, provider, or prior instance.

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

Temporary Team Members are not grant-lineage nodes. Each durable Standing
Agent child grant must strictly attenuate at least one authorization or
resource dimension and may never broaden Company, domain, Actor, WorkItem,
Assignment, correlation, permission, command, subject, payload, effect,
validity, expiry, lease TTL, successful-use budget, execution budget,
concurrency, or remaining delegation depth. Exact allowed ProjectBinding
selectors and approved template id/version/digest are also authority-bearing
dimensions. Child selections must be equal to or narrower than parent
allowlists. A later ProjectBinding or template selector change cannot retarget
existing authority. Child expiry cannot outlive parent expiry. A parent cannot
delegate more depth than it retained.

The authority service denies an unapproved or stale template, a retargeted or
unlisted ProjectBinding, equal-or-broader authorization, parent-budget
oversubscription, concurrency excess, and depth excess. ProjectBinding proves
the bounded execution resource; it never grants Company authority.

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
WorkItem + Company Assignment + Agent Team Work/WorkDelivery
approved Standing Agent template id/version/digest + exact ProjectBinding
reservation ids and budget/concurrency/depth amounts
ActionCommand/request/result and AuditEvent references
```

This digest is evidence, not a capability. Display metadata and UI labels
cannot change its meaning.

Routine work, compliant child-grant issuance, and approved-template staffing
may proceed under an already approved parent envelope only when policy is
known, the effect is reversible, blast radius is bounded to the exact Company,
Work, Actors, ProjectBinding, command, payload, and resource ceilings, and
there is no material external commitment. The action remains audited and
digested, but does not require a fresh R3 decision merely because it consumes a
strictly attenuated part of the existing envelope.

The Human queue contains exceptions only:

- Constitution or root-grant creation, activation, expansion, replacement, or
  revocation;
- a child that cannot strictly attenuate, uses an unapproved/stale template,
  retargets ProjectBinding, or changes protected Organization/permission state;
- budget, concurrency, or depth exhaustion requiring a higher ceiling;
- unknown policy, a materially irreversible or destructive effect, broad/root
  security blast radius, or material Finance, legal, credential,
  major-publication, or other external commitment;
- ambiguous identity, delivery, recovery, or indeterminate execution; and
- Work with no accountable Company Actor or applicable policy.

Queue placement is not approval. Only an exact, unexpired Human decision by the
required approver permits the named effect.

## Exact root Approval/R3 proposal

The constitutional root is reserved as one target
[Approval](work-items-and-approvals.md#approval-contract) envelope:

```text
Approval
  id: approval-agentos-company-constitution-root-v1
  subject_ref:
    kind: scoped_permission_grant       # target subject kind; not in current schema
    id: grant-agentos-company-lead-root-v1
  action_summary: activate root ScopedPermissionGrant generation 1 only
  requested_by: { actor_type: human, actor_id: human-wcw-owner }
  required_approver_refs:
    - { actor_type: human, actor_id: human-wcw-owner }
  required_actor_type: human
  policy_ref: policy-company-authority-constitution-v1
  status: requested
  decided_by: []
  decision_note: null
  evidence_refs:
    - scoped-permission-grant-activation:grant-agentos-company-lead-root-v1:1:<canonical-grant-digest>
    - company-constitution:1:<canonical-constitution-digest>
    - company-authority-acceptance:<exact-acceptance-evidence-ref>
  requested_at: <exact request time>
  decided_at: null
  expires_at: <required bounded expiry at request time>

activation_action_command: permission.grant.activate
risk_tier: R3
effect: ChangePermission
company_id: agent-company
organization_id: company
root_org_unit_id: orgunit-agentos-root
grantee: { actor_type: agent, actor_id: agent-agentos-lead }
root_grant_generation: 1
```

This is the exact shape the one Approval must have **when it is requested**;
no such record currently exists. The Actor and Organization ids above are
current Store observations. The target subject kind, activation command,
grant, acceptance ref, and digest values are reserved target inputs, not
claims that those schema members or records exist. ADR 0047 and the broker
contract remain canonical for the bound grant generation/digest and activation
semantics.

Before the single Approval can be requested, both digests and the exact
acceptance evidence ref must replace the placeholders. The governed path then
persists this one request, records one Human decision, and, only if it is
approved and unexpired, dispatches the exact activation command. There is no
earlier or second root Approval. Any change to a bound value requires a new
request and Human R3 decision. Human confirmation of this architecture,
documentation merge, deployment, runtime start, or Assignment delivery is not
that decision.

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

The Work UI must show accountable Actor, Company Assignment, Agent Team
Work/version, WorkDelivery/provider receipt, active leaf lineage,
lease/ActionCommand/AuditEvent refs,
reservation state, exception state, result/evidence/outcome refs, and GitHub
DeliveryRefs. Conversation remains an adjacent optional activity source, never
responsibility or delivery proof. Missing Store relations stay visibly missing.
The UI must not
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

1. Freeze the Constitution and grant snapshots, compute their digests, and
   prepare the exact root R3 envelope without requesting or deciding it.
2. Implement deterministic, non-activating acceptance for the one-node V1
   broker from ADR 0047, including durable allow/deny audit and root-token
   non-disclosure.
3. Extend the canonical broker grammar and Store transactions for strict child
   lineage, atomic budget/concurrency/depth reservations, cascading fences, and
   indeterminate recovery; then run deterministic race and denial tests.
4. Add the exception-only queue, routine audit digest, and Store-backed
   Organization/Work projections.
5. After independent review proves attenuation, protected-effect denial,
   recovery, audit, and UI truth, persist and request the single exact root
   Approval, attach the acceptance evidence, record one Human decision, and
   dispatch activation only if that same decision is approved and unexpired;
   then run the exact live V1 allow/deny proof.

# Organization and Actors

```text
status: canonical Company OS contract
owner_role: product
canonical_for: organization hierarchy, actor identity, hybrid membership, and organization governance
```

## Purpose

The Company OS has one organization that can include people, Standing Agents,
external participants, and technical services. It establishes durable
responsibility and authority around Docs and WorkItems; it is not a projection
of a provider runtime, an Agent Team attempt, or a chat roster.

The initial product is deliberately **lead-first**. One Human Owner governs one
Lead Agent; several second-level Standing Agents report directly to that Lead.
This gives the company a legible assignment and escalation path without
inventing departments. The same contract supports later, deliberate nesting
when a business area has enough distinct responsibility, policy, or capacity
to warrant it.

## Organization model

```text
Organization
  -> OrgUnit (one root unit)
     -> OrgUnit (zero or more children)
        -> ...
  -> OrganizationMembership (actor to unit)
  -> OrganizationPolicy (authority, permissions, escalation)
```

```text
OrgUnit
- id
- organization_id
- name
- purpose
- parent_unit_id?                 # null only for the organization root
- status = active | paused | archived
- human_lead_actor_ref?
- agent_lead_actor_ref?
- policy_refs[]
- document_space_ref?             # the responsible Docs space, if any
- created_at / updated_at
```

`parent_unit_id` is optional by design, not an invitation to create hierarchy
before it helps. A child unit is appropriate only when it has a durable purpose,
membership or ownership boundary, and explicit lead or escalation policy.
Org-unit names, charts, and reporting views are projections of these records;
they must not infer a reporting relationship from document authorship, a task
assignee, or a provider session.

### Initial lead-first example

```text
Company
└── Human Owner
    └── Lead Agent
        ├── Document Architecture Agent
        ├── Finance Agent
        ├── Content Strategy Agent
        └── Trademark Agent
```

### Elastic hierarchy example

```text
Company
├── Brand & IP
│   ├── Brand Owner (human)
│   ├── IP Lead (Standing Agent)
│   ├── Trademark Agent (Standing Agent)
│   └── External Lawyer (external)
├── Content
│   ├── Content Director (human)
│   ├── Strategy Agent (Standing Agent)
│   └── Analytics Agent (Standing Agent)
└── Governance
    ├── Governance Owner (human)
    ├── Document Architecture Agent (Standing Agent)
    └── Organization Governance Agent (Standing Agent)
```

Membership is many-to-many: an actor can be a member of more than one unit,
with a separately recorded role and effective dates in each. One membership
must not silently grant the permissions, accountability, or lead role of
another.

```text
OrganizationMembership
- id
- organization_id
- org_unit_id
- actor_ref
- membership_role = lead | member | advisor | observer | external_partner
- title_or_function?
- status = active | invited | paused | ended
- starts_at / ends_at?
- authority_policy_refs[]
- created_by_actor_ref
```

The initial reporting relation is explicit and separate from membership:

```text
ReportingRelation
- manager_actor_ref
- report_actor_ref
- scope
- authority_policy_refs[]
- effective_from / effective_until?
```

V1 optimizes for `Human Owner -> Lead Agent -> direct Standing Agents`.
Second-level Agents may collaborate through a shared WorkItem or business
object, but the Lead remains their default assignment and escalation path.

## A shared reference, distinct actor lifecycles

All Company OS records point to an actor through a stable `ActorRef`. This
makes it possible for a document, WorkItem, approval, financial record, or
comment to identify a participant consistently without pretending that all
participants are the same object.

```text
ActorRef
- actor_type = human | agent | external | service
- actor_id
```

`ActorRef` is a reference contract only. Each actor type has its own lifecycle
and fields:

| Actor type | Durable identity and lifecycle | May hold responsibility | Distinct boundary |
| --- | --- | --- | --- |
| `human` | Person identity, membership status, availability, permissions | Yes | Can be required for legal, financial, and governance authority; has no provider runtime. |
| `agent` | Durable AgentMember identity, organization role, capacity, skills, permissions | Yes, within policy | A Standing Agent is an AgentMember operating mode; process health never creates business authority. |
| `external` | Named outside person or organization, engagement and access expiry | Limited, explicit only | Never receives implied internal membership or broad visibility. |
| `service` | Technical identity such as an integration or automation | Only when policy explicitly permits it | Cannot impersonate a human approver or a Standing Agent. |

```text
HumanMember
- id, display_name, status, availability?
- organization memberships
- permission and authority policies

StandingAgent
- id, display_name, role, availability, assignment_capacity?
- organization memberships, capabilities, permissions
- runtime and provider-session references

ExternalParticipant
- id, display_name_or_organization, engagement_scope
- sponsor_actor_ref, access_expiry, confidentiality/contract refs
- organization memberships and restricted permissions

ServiceActor
- id, display_name, service_kind, owner_actor_ref
- credential/permission boundary, audit policy
```

An absent or offline agent runtime does not make a Standing Agent inactive in
the organization; it changes operational availability. Conversely, a running
runtime does not make an agent available or authorized. A human may be offline
while remaining the accountable owner. External access ending must revoke the
external participant's effective permissions without deleting their historical
attribution.

## Hybrid teams and authority

An `OrgUnit` can have both a human lead and an agent lead, with their scopes
made explicit in policy. A common safe pattern is that a Standing Agent leads
operational coordination while a human lead retains financial, legal, hiring,
or organization-change authority. A unit can instead have only one lead, but
the missing counterpart must not be inferred.

Organization policy declares:

- responsibilities and document spaces owned by the unit;
- which actor types may accept WorkItems, make decisions, or create execution
  runs;
- capacity and escalation rules;
- which action classes require human approval;
- external participant visibility and time limits; and
- delegation limits, audit requirements, and the fallback owner.

This permits teams such as `Trademark Agent + external lawyer + Brand Owner`
without blurring who owns the legal record, who performs work, and who is
authorized to approve spending or filing.

## Lead Agent operating contract

The Lead Agent is a durable organizational role, not the temporary lead member
of an AgentTeamRun. Within policy it may:

- receive intent from the Human Owner and create or assign WorkItems;
- coordinate direct reports and inspect their explicit work, blockers, and
  durable outcomes;
- start a Mission, AgentTeamRun, WorkflowRun, or direct execution for a complex
  WorkItem;
- propose a new second-level Standing Agent when recurring work exposes a
  missing capability; and
- propose role, permission, capacity, or reporting changes.

Adding a temporary MemberRun to one execution does not change Organization.
Adding a Standing Agent requires a role charter, reporting relation,
responsibility scope, permissions, business-module access, cost/provider
policy, creation rationale, and the approvals required by organization policy.
Low-risk creation may be delegated to the Lead; financial, legal, credential,
external-access, or organization-wide authority changes require Human approval.

## Governance

Organization changes are governed company actions, not an editable roster.
They use a documented proposal, impact assessment, required approval(s), and
an audit event. Typical changes include creating or nesting an OrgUnit, adding
or retiring a Standing Agent, changing authority, moving an actor, or inviting
an external participant.

The Organization Governance Agent may identify overlap, uncovered work,
capacity pressure, stale permissions, or a need for a new role. It may draft a
proposal and assemble evidence. It must not auto-grant itself or any other
actor authority beyond policy. A human approval is mandatory where the policy
marks the change as financial, legal, security-sensitive, employment-related,
or a change to organization-level authority.

The Document Architecture Agent is a peer governance role: it proposes document
spaces, templates, typed records, relations, and lifecycle rules when new
business domains arise. It does not independently create a department or grant
an Actor authority. The two roles coordinate through a documented module or
organization proposal when a new domain needs both an information structure and
new organizational capacity.

## UI and projection requirements

The `Organization` area is a mixed company structure, not a flat runtime list:

- default to a compact organization chart/list with the root unit and direct
  members; disclose children only where they exist;
- visually distinguish humans, Standing Agents, external participants, and
  services without judging their importance by type;
- show role, unit membership, accountable document spaces, declared
  availability/capacity, and pending governance actions;
- distinguish a Standing Agent's organizational status from runtime health and
  from provider-session history;
- show external scope and expiry prominently, never as an ordinary employee;
- allow a person or agent detail page to show memberships, authority scope,
  explicit WorkItems, and documented activity across units.
- make the initial Human Owner, Lead Agent, and direct-report structure obvious
  before presenting optional deeper OrgUnits;
- give the Lead a collaboration workspace with direct-report status, assigned
  WorkItems, blockers, active one-time executions, organization proposals, and
  a durable object-linked conversation surface;
- give each second-level Agent a workspace centred on its conversation and
  activity with the Lead, with related WorkItems, BusinessModules, Documents,
  and execution attempts composed in context.

The organization chart is never the only responsibility view. Every visible
lead, ownership, or membership relation must link to its durable source record.
Agent Team `MemberRun`s and provider-native child threads may appear as
execution history on an eligible Standing Agent detail page only through an
explicit stable link; they are not organization members and cannot populate a
chart.

## Non-goals

- No universal employee object that erases human, agent, external, and service
  boundaries.
- No hierarchy inferred from chats, names, sessions, or model
  providers.
- No automatic organizational mutation merely because an Agent recommends it.
- No replacement of Mission/Wave, Agent Team, Dynamic Workflow, or host
  execution lifecycle contracts.
- No persistence, replay, or governance use of raw provider thinking.

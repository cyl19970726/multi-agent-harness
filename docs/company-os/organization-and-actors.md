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

The initial product is deliberately **governance-led**. One Human Owner governs
one Lead Agent. The Lead directly manages four Governance Agents for Docs,
Work, Finance, and Org/HR. All Business Agents report to Org/HR, which manages
their organizational lifecycle; they collaborate with the other Governance
Agents through governed records and Actions. This keeps the Lead's management
span stable while allowing business capability to grow deliberately.

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

### Initial governance-led example

```text
Company
└── Human Owner
    └── Lead Agent
        ├── Docs Governance Agent
        ├── Work Governance Agent
        ├── Finance Governance Agent
        └── Org / HR Governance Agent
            ├── Trademark Agent
            ├── Development Agent
            └── Content Agent
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
    ├── Docs Governance Agent (Standing Agent)
    └── Org / HR Governance Agent (Standing Agent)
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

V1 optimizes for `Human Owner -> Lead -> four Governance Agents`, followed by
`Org/HR -> Business Agents`. Reporting and collaboration are separate graphs:
Docs, Work, and Finance Governance Agents may collaborate with Business Agents
through shared records, but do not become their organizational manager.

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
| `agent` | Durable StandingAgent identity, organization role, capacity, skills, permissions | Yes, within policy | A StandingAgent is not an Agent Team MemberRun or provider session; process health never creates business authority. |
| `external` | Named outside person or organization, engagement and access expiry | Limited, explicit only | Never receives implied internal membership or broad visibility. |
| `service` | Technical identity such as an integration or automation | Only when policy explicitly permits it | Cannot impersonate a human approver or a Standing Agent. |

```text
HumanMember
- id, display_name, status, availability?
- organization memberships
- permission and authority policies

StandingAgent
- id, display_name, role, responsibility_scope, availability, assignment_capacity?
- organization memberships, capabilities, permissions
- system_prompt_ref?, tool_refs[], skill_refs[]
- maintained_document_refs[], accepted_work_type_refs[], escalation_policy_ref?
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

When a Standing Agent participates in an Agent Team, an explicit stable id join
links it to reusable `AgentMember` configuration and the current `MemberRun`.
Organization authority remains on StandingAgent; Team participation,
Supervisor generation, provider-native session, mailbox delivery, and Close
remain in the Execution Space. Stable Agent Inbox mail reaches the Member only
through an explicit `AgentMessageRoute`, never through identity inference.

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
- coordinate the four Governance Agent direct reports and inspect company-level
  work, blockers, proposals, and durable outcomes;
- start a Mission, AgentTeamRun, WorkflowRun, or direct execution for a complex
  WorkItem;
- ask Org/HR to evaluate a recurring capability gap and sponsor the resulting
  Agent proposal when justified; and
- propose role, permission, capacity, or reporting changes.

Adding a temporary MemberRun to one execution does not change Organization.
Adding a Standing Agent requires a role charter, reporting relation,
responsibility scope, permissions, business-module access, cost/provider
policy, creation rationale, and the approvals required by organization policy.
Low-risk creation may be delegated to the Lead; financial, legal, credential,
external-access, or organization-wide authority changes require Human approval.

## Cascading Standing Agent delegation

Organization supports a deliberately nested agent hierarchy. An upper Standing
Agent may drive lower Standing Agents when all of the following are explicit:

- the reporting or delegation relation exists in Organization;
- the lower Agent has a role charter, capability scope, tool/skill refs, and
  permission boundary compatible with the delegated WorkItem;
- the WorkItem names the accountable owner and assignees through `ActorRef`;
- the source Document or business record explains why the work exists; and
- result updates return through Docs, Work, or the owning module rather than an
  unstructured chat transcript.

This lets a Lead Agent coordinate Governance Agents, a Docs Governance Agent
maintain company memory, or a Development Agent route code work without turning
every execution into a manually managed flat team.

The same cascade can create new capability, but creation is governed. If a
Docs Agent, Development Agent, Merchant Ops Agent, or any other Standing Agent
finds recurring work that does not fit the current organization, it may draft a
capability-gap record and ask Org/HR to reuse an existing Actor, use temporary
execution, engage an external collaborator, or create a new lower Standing
Agent. A new Standing Agent is not created merely because a provider spawned a
subagent or because an Agent said it would be useful.

Temporary provider-native subagents and Agent Team `MemberRun`s are execution
implementation details. They may help a Standing Agent perform one WorkItem,
and optional hooks may record honest execution attribution, but they do not own
Organization authority, cannot appear as durable reports, and cannot receive
WorkItems unless Org/HR promotes them into explicit Standing Agents.

For the current Company OS implementation focus, the core loop is Docs + Work
+ Organization. Finance remains conditional: it is invoked only when the
WorkItem requests a monetary effect such as a budget, commitment, purchase,
refund, invoice, or payment.

## Governance

Organization changes are governed company actions, not an editable roster.
They use a documented proposal, impact assessment, required approval(s), and
an audit event. Typical changes include creating or nesting an OrgUnit, adding
or retiring a Standing Agent, changing authority, moving an actor, or inviting
an external participant.

The Org/HR Governance Agent owns the Business Agent lifecycle. It evaluates
whether a capability gap should reuse an existing Actor, use temporary
execution, engage an external collaborator, or justify a new Standing Agent.
It may draft an `OrgChangeProposal`, provision after approval, evaluate, pause,
and propose retirement. It must not auto-grant itself or any other actor
authority beyond policy. A human approval is mandatory where the policy marks
the change as financial, legal, security-sensitive, employment-related, or a
change to organization-level authority.

The Docs Governance Agent is a peer governance role: it proposes document
spaces, templates, typed records, relations, and lifecycle rules when new
business domains arise. It does not independently create a department or grant
an Actor authority. The two roles coordinate through a documented module or
organization proposal when a new domain needs both an information structure and
new organizational capacity.

## UI and projection requirements

The `Organization` area is a mixed company structure, not a flat runtime list:

- default to a connected hierarchy that first shows Human Owner, Lead, the four
  Governance Agents, and the Business Agent branch beneath Org/HR;
- visually distinguish humans, Standing Agents, external participants, and
  services without judging their importance by type;
- show role, unit membership, accountable document spaces, declared
  availability/capacity, and pending governance actions;
- distinguish a Standing Agent's organizational status from runtime health and
  from provider-session history;
- show external scope and expiry prominently, never as an ordinary employee;
- provide a compact Actor configuration view for responsibility, reporting,
  prompt, tools/Skills, permissions, maintained Docs, and explicit WorkItems;
- make solid reporting relations distinct from optional dashed work
  collaboration overlays;
- surface capability gaps and `OrgChangeProposal`s from the Organization area.

Dedicated Agent workspaces are not required for the next implementation slice.
The current product may use the Organization overview plus an Actor drawer or
profile route. Rich Governance Agent workspace designs remain future references
until the organization model, Actions, and Work surfaces are implemented.

The organization chart is never the only responsibility view. Every visible
lead, ownership, or membership relation must link to its durable source record.
Agent Team `MemberRun`s and provider-native child threads may appear as
execution history on an eligible Standing Agent profile only through an
explicit stable link; they are not organization members and cannot populate a
chart.

The implemented link is
`StandingAgent.execution_agent_member_ref -> AgentMember.id ->
MemberRun.agent_member_id`. Company OS owns the optional first edge; equal
StandingAgent and AgentMember ids never create it implicitly. TeamRuns created
from a reusable team preserve the AgentMember identifier automatically. The
Company OS read projection includes every explicitly linked MemberRun, including
assignment-less participation, and retains its chronological assignment
history. Ad-hoc or unlinked members remain execution-only. Organization cards
route to Actor profiles, and explicitly linked Agent Team participation
deep-links to the Team/Member execution page.

### Governed link and unlink

The first edge is authored by one explicit command per pair:

```bash
harness company org link-execution \
  --authority human-wcw-owner \
  --actor agent-wcw-ops \
  --agent-member agent-wcw-ops \
  --execution-space wcw-ops

harness company org unlink-execution \
  --authority human-wcw-owner \
  --actor agent-wcw-ops \
  --expect-agent-member agent-wcw-ops
```

Contract:

- Both records must already exist. The command never creates a StandingAgent
  and never registers an AgentMember.
- `--agent-member` is required and never defaults to `--actor`. Equal ids are
  allowed but must be typed twice, once per side.
- The StandingAgent is read latest-row-wins and re-appended with only
  `execution_agent_member_ref` and `updated_at` changed, so every other actor
  field round-trips. The write goes through the Human administrative governance
  envelope; no command edits raw JSONL.
- `--authority` must be an active Human holding `company_os.admin` on **every**
  invocation, including the ones that change nothing. An idempotent no-op is
  never an authorization bypass: it authorizes first and then declines to write.
- Re-running the same explicit pair appends no row and reports
  `changed: false`, so a migration over many pairs is safely re-runnable. The
  idempotent path still resolves `--execution-space` and revalidates the
  AgentMember, so a re-run against a deleted or renamed space fails loudly even
  though it would have changed nothing.
- Repointing an already-linked StandingAgent to a different AgentMember requires
  `--replace`. Stealing an AgentMember already owned by another StandingAgent is
  rejected by the store's one-to-one guard regardless of `--replace`.
- `unlink-execution` needs no Execution Space because clearing a reference
  validates nothing in the execution store. `--expect-agent-member` is an
  optional optimistic guard for scripted relink flows.

### Cross-store validation boundary

AgentMember truth lives in an Execution Space, not in the Company Store
(ADR 0042). `harness company ...` resolves the Company Store and returns before
the global `--space` selector is consumed, so a Company command holds no
execution store. `link-execution` therefore requires an explicit
`--execution-space <id>`, resolved through the Execution Space registry and
opened read-only to confirm the AgentMember exists. There is deliberately no
fallback to the active space and no fallback to a Project Binding: a Project
Binding describes provider cwd, not identity, and a link validated against an
unnamed store is not a governed link.

The space id is a write-time assertion only. It is **not** persisted onto the
StandingAgent, because storing execution-space truth in a Company OS row is
exactly the coupling ADR 0042 and ADR 0045 remove. The consequence is explicit:
the read projection resolves `execution_agent_member_ref` against whichever
Execution Space the reader selects, so a Dashboard pointed at a different space
shows an empty `standing_assignments` rather than an error. Operators must point
the reader at the same space the link was validated against.

### Duplicate links degrade locally

The store rejects a new duplicate `execution_agent_member_ref`. A store that
already carries one — from a legacy import, a hand edit, or a racing writer —
must not take down the whole Dashboard. The read projection withholds only the
ambiguous `agent_member_id` from the join, guesses no winner, and reports the
defect in `standing_assignment_conflicts`, while every other Standing Agent
projects normally and the snapshot still returns `200`.

## Non-goals

- No universal employee object that erases human, agent, external, and service
  boundaries.
- No hierarchy inferred from chats, names, sessions, or model
  providers.
- No automatic organizational mutation merely because an Agent recommends it.
- No replacement of Mission/Wave, Agent Team, Dynamic Workflow, or host
  execution lifecycle contracts.
- No persistence, replay, or governance use of raw provider thinking.

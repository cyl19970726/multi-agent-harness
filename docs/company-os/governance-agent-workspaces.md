# Governance Agent workspaces

```text
status: canonical product contract; configuration and shared workspace partial, lifecycle planned
owner_role: product-architecture
canonical_for: Lead-managed governance roles and their operating surfaces
```

The initial governance layer contains four Standing Agents that report directly
to Lead. They operate the four Company OS systems without becoming the source
of truth themselves.

The minimum product contract for each Governance Agent is configuration, not a
special page:

```text
GovernanceAgentDefinition
- actor_ref
- role and responsibility_scope
- reports_to = Lead Agent
- system_prompt_ref
- tool_refs[] / skill_refs[]
- permission_policy_refs[]
- maintained_document_refs[]
- accepted_work_types[]
- escalation_policy_ref
- status / availability? / explicit capacity?
```

Prompt describes how the Agent should behave. Tools and optional Skills provide
bounded capabilities. Permission policy defines what it may actually do. None
of these fields may be inferred from a provider session or rendered page.

| Agent | Primary decision | Governed outputs |
| --- | --- | --- |
| Docs Governance | Where does new company knowledge belong and how should it remain healthy? | Document/Module structure proposals, TypedRecord/Relation design, result integration |
| Work Governance | What durable commitment exists and how should responsibility be routed? | WorkItem intake, classification, Milestone, responsibility, Approval/Finance impact, execution route |
| Finance Governance | What monetary effect is requested and what evidence/authority permits it? | Budget/Commitment/Invoice/Payment/Refund commands and control exceptions |
| Org / HR Governance | What durable company capability is missing and how should the organization change? | OrgChangeProposal, Agent provisioning, permission placement, evaluation and retirement |

The corresponding optional operator skills are:

| Governance role | Optional skill |
| --- | --- |
| Docs Governance | [`company-docs-operator`](../../skills/company-docs-operator/SKILL.md) |
| Work Governance | [`company-work-operator`](../../skills/company-work-operator/SKILL.md) |
| Finance Governance | [`company-finance-operator`](../../skills/company-finance-operator/SKILL.md) |
| Org / HR Governance | [`company-org-operator`](../../skills/company-org-operator/SKILL.md) |

These skill references belong in `skill_refs[]`. They do not replace
`permission_policy_refs[]`, maintained document refs, WorkType routing, Human
approval, or governed Actions.

Each role requires a clear decision contract, durable activity, supporting
evidence, authority, Skills, maintained Docs, linked work, and required gates.
The shared Standing Agent workspace is now implemented as an Organization
profile plus native WorkItem/Assignment activity and composable context rail.
It deliberately reuses the visual shell of an execution MemberRun without
reusing TeamRun, Wave, attempt, or provider-lifecycle semantics. The four
governance roles still need governed organization-change provisioning and
role-specific queues. Private thinking never appears. Skills reduce execution
variance but never grant authority or replace product Actions.

The workspace also projects Agent Team participation when, and only when,
`StandingAgent.execution_agent_member_ref` names the AgentMember carried by
`MemberRun.agent_member_id`. Equal ids never bind. It shows exact TeamRun,
MemberRun, assignment correlation, status,
native-session locator, and a deep link to execution. It does not infer a link
from names or providers, and it does not treat runtime health as organization
availability or authority.

The native `StandingAgent` schema now carries configuration references for
`system_prompt_ref`, tools, Skills, maintained Documents, accepted WorkTypes,
escalation and permission policy. Prompt content remains in Docs; reporting
level and title remain in OrganizationMembership/OrgUnit; runtime activity
remains in execution records. This is shared substrate, not one universal
record. The added configuration fields are optional and default safely so
historical Standing Agent rows remain readable; missing references stay visibly
missing instead of being inferred.

Lead manages priorities and cross-governance conflicts. Ordinary Business
Agents report to Org/HR and collaborate with the other Governance Agents through
explicit Documents, WorkItems, ActorRefs, FinancialRecords, Approvals, and
governed Actions.

## Simple permission v1 in Governance workspaces

The canonical operator surface is the fixed-template and module-envelope model
in [Organization and Actors](organization-and-actors.md#simple-organization-permission-v1).
Governance workspaces show it; they do not invent another authority model.

| Workspace fact | Required presentation |
| --- | --- |
| Fixed role | Exact `company_lead`, `domain_lead`, or `execution_member` template id and its fixed responsibility. Company Lead owns priority and capacity; Domain Leads execute and review; execution Members deliver assigned evidence. |
| Declared envelope | Exact scoped `docs`, `work`, `org`, and `github` entries using only `read`, `write`, `execute`, and `delegate`. Missing entries remain missing. |
| Effective authority | Evaluated intersection of template, envelope, current Actor/Action policy, and scope. Until an evaluator exists, show `not implemented`, never declared permissions as effective. |
| Runtime | Exact StandingAgent → AgentMember → MemberRun/native Session linkage, availability evidence, and current Supervisor generation. Runtime is neither role nor authority. |
| Delegation | Parent Actor, child execution Member, assignment/correlation, same-or-narrower envelope, bound project/workspace, and returned evidence. No recursive grant browser is required. |
| Protected effect | Link to the single protected-effects list in Organization and the named Human Approval when one is required. Ordinary in-envelope work stays out of the Human exception queue. |

A `company_lead` or `domain_lead` with `delegate` may create child execution
Members and give them the same or a narrower scoped module envelope. This does
not create an OrgUnit, Standing Agent, reporting relation, permission policy, or
Company credential. The Lead sets company priorities, capacity, and
cross-WorkItem conflict decisions; a Domain Lead operates autonomously inside
the delegated domain and escalates only boundary crossings or protected
effects. Skills and tools remain capabilities, not grants.

The Runtime Supervisor transports authenticated requests for the exact live
Member and owns process leases, delivery, and control. It does not select role
templates, issue envelopes, approve Actions, or own Company priority/capacity.
Bearer tokens and other Company credentials must not be written into Team
messages, MemberRun records, native transcripts, workspace reports, or UI
projections.

Implementation truth is intentionally explicit. Current master implements
Organization identity/configuration, Human administrative Organization writes,
and Action policy checks, but not the simple template evaluator. Accepted
candidate `ea28b908a99ce8e05ecc5fbbcd1aaee952f3382b` separates declared
configuration, effective-authority absence, and runtime in the Organization UI;
secure transport candidate `604bc069ab162775dcbaeddc290f2a76d260ab98` fences scoped
transport on the Supervisor lease latch. Neither candidate is master, and
neither proves template Store/API enforcement. Recursive grant UX, sibling
budget algebra, and a new permission lifecycle are outside this v1 surface.

## Capability-gap decision contract

Org/HR does not create a permanent Agent merely because Lead or a WorkItem asks
for new capacity. It records the gap, compares four mutually exclusive routes,
and keeps the organization change separate from execution:

| Route | Use when | Durable company change | Required boundary |
| --- | --- | --- | --- |
| Reuse an existing Actor | an accountable Human, Standing Agent or service already has the role, permission and capacity | none; create or reroute the WorkItem/Assignment | ordinary Work policy; no Organization approval |
| Temporary execution | the need is one-off or exploratory and can run through Agent Team, Workflow or Host | none; execution refs remain attached to Work | executor gate accepts only the execution outcome, never organization membership |
| External collaborator | expertise or legal delivery must come from outside the company | scoped external Actor/engagement with expiry and visibility limits | affected policy owner; Human gate only when the engagement has an effect in the canonical protected-effects list |
| New Standing Agent | the capability is recurring, durable, measurable and cannot be satisfied safely by the first three routes | `OrgChangeProposal`, membership, reporting, configuration and evaluation policy | Current Human administrative bootstrap; target Lead sponsorship with a Human gate only for an effect in the canonical protected-effects list |

An `OrgChangeProposal` must name the capability gap, rejected alternatives,
proposed reporting line, permission ceiling, prompt/Docs references, Tools and
Skills, accepted WorkTypes, evaluation cadence, escalation and retirement
conditions. Approval authorizes only the declared organization change. It does
not approve future WorkItems, spending, legal submissions or execution Waves.

This table is the canonical decision model. The proposal, approval and
provisioning Action family remains planned until native schemas, Store commands,
API transport and acceptance tests prove it; the current Organization UI must
therefore render those controls disabled or as design-only proposals.

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

Each role requires a clear decision contract, durable activity, supporting
evidence, authority, Skills, maintained Docs, linked work, and required gates.
The shared Standing Agent workspace is now implemented as an Organization
profile plus native WorkItem/Assignment activity and composable context rail.
It deliberately reuses the visual shell of an execution MemberRun without
reusing TeamRun, Wave, attempt, or provider-lifecycle semantics. The four
governance roles still need governed organization-change provisioning and
role-specific queues. Private thinking never appears. Skills reduce execution
variance but never grant authority or replace product Actions.

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

## Capability-gap decision contract

Org/HR does not create a permanent Agent merely because Lead or a WorkItem asks
for new capacity. It records the gap, compares four mutually exclusive routes,
and keeps the organization change separate from execution:

| Route | Use when | Durable company change | Required boundary |
| --- | --- | --- | --- |
| Reuse an existing Actor | an accountable Human, Standing Agent or service already has the role, permission and capacity | none; create or reroute the WorkItem/Assignment | ordinary Work policy; no Organization approval |
| Temporary execution | the need is one-off or exploratory and can run through Agent Team, Workflow or Host | none; execution refs remain attached to Work | executor gate accepts only the execution outcome, never organization membership |
| External collaborator | expertise or legal delivery must come from outside the company | scoped external Actor/engagement with expiry and visibility limits | affected policy owner; Human approval when legal, data or external-access policy requires it |
| New Standing Agent | the capability is recurring, durable, measurable and cannot be satisfied safely by the first three routes | `OrgChangeProposal`, membership, reporting, configuration and evaluation policy | Lead sponsorship plus Human approval for authority, credential, finance, legal or other sensitive changes |

An `OrgChangeProposal` must name the capability gap, rejected alternatives,
proposed reporting line, permission ceiling, prompt/Docs references, Tools and
Skills, accepted WorkTypes, evaluation cadence, escalation and retirement
conditions. Approval authorizes only the declared organization change. It does
not approve future WorkItems, spending, legal submissions or execution Waves.

This table is the canonical decision model. The proposal, approval and
provisioning Action family remains planned until native schemas, Store commands,
API transport and acceptance tests prove it; the current Organization UI must
therefore render those controls disabled or as design-only proposals.

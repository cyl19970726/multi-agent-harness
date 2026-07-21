# Governance Agent workspaces

```text
status: canonical product contract; implementation planned
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
The current implementation does not require four rich standalone Agent pages:
these can first appear as Organization profile/configuration panels and
module-specific queues. Existing high-fidelity workspace images are future
references only. Private thinking never appears. Skills reduce execution
variance but never grant authority or replace product Actions.

Lead manages priorities and cross-governance conflicts. Ordinary Business
Agents report to Org/HR and collaborate with the other Governance Agents through
explicit Documents, WorkItems, ActorRefs, FinancialRecords, Approvals, and
governed Actions.

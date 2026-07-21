# Docs, Organization, Work, and Finance

```text
status: canonical product responsibility map
owner_role: product-architecture
canonical_for: four-system ownership and cross-system operating loop
```

The Company OS has four cooperating systems. They do not own four copies of the
same company state. Each system owns one kind of truth and connects to the
others through stable relations.

The systems are operated by a governance layer inside Organization. The
canonical initial reporting structure is:

```text
Human Owner
└── Lead Agent
    ├── Docs Governance Agent
    ├── Work Governance Agent
    ├── Finance Governance Agent
    └── Org / HR Governance Agent
        ├── Trademark Agent
        ├── Development Agent
        ├── Content Agent
        └── future Business Agents
```

Lead manages the four Governance Agents. Business Agents do not report directly
to Lead; they live under Org/HR, which owns their proposal, provisioning,
permissions, reporting placement, evaluation, and retirement. Docs, Work, and
Finance Governance Agents collaborate with Business Agents through governed
records and Actions but are not their organizational manager.

The four Governance Agents are the bootstrap internal-management team. A new
Business Agent exists only after Org/HR determines that recurring capability is
needed and an `OrgChangeProposal` passes the applicable Lead or Human gate.
One-off demand should reuse an existing Actor, temporary Agent Team, Workflow,
Host execution, or external collaborator instead of automatically expanding
the standing organization.

## Responsibility map

| System | Owns | Does not own |
| --- | --- | --- |
| **Docs** | Documents, Blocks, TypedRecords, Relations, Views, BusinessModules, durable decisions and result narratives | task lifecycle, actor authority, payment state |
| **Organization** | Human and Agent identity, OrgUnits, roles, reporting structure, permissions, authority policies, availability and capacity when explicit | WorkItem status, document content, financial transaction state |
| **Work** | WorkItems, Milestones, responsibility roles, Assignments, lifecycle, Approval links, execution/delivery references, evidence and result routing | source knowledge, organization identity, financial ledger effects |
| **Finance** | budgets, Commitments, invoices, Payments, refunds, financial metrics, evidence and financial state transitions | general tasks, company knowledge narrative, actor hierarchy |

The owning system is the only place allowed to assert its truth. Other systems
render linked projections. For example, Docs may display a ¥3,000 Commitment,
but Finance owns its amount and state. Work may display an accountable Agent,
but Organization owns the Agent's identity and authority.

## Shared operating loop

```text
Docs: source context and proposed action
  -> Work: durable commitment and responsibility
  -> Organization: accountable, assigned, reviewing, and approving Actors
  -> Finance: governed monetary effect when the work has one
  -> execution: human / Standing Agent / Mission-Wave / Team / Workflow / Host
  -> Work: outcome, evidence, review, and completion
  -> Finance: authorized durable financial transition
  -> Docs: result and updated company memory
```

This is a relation loop, not a pipeline that copies records. A single operation
may update more than one owning store through governed commands, but every
effect remains typed and attributable.

## Collaboration rules

1. **Docs originates and receives context.** A durable Document or TypedRecord
   explains why work exists and receives the final result. Chat alone does not.
2. **Work coordinates responsibility.** A WorkItem names submitter, requester,
   accountable owner, assignees, reviewer, approver, source, result, evidence,
   and execution references.
3. **Organization decides who may act.** Actor identity, reporting structure,
   permissions, capacity, and named authority come from Organization; Work
   references them through `ActorRef`.
4. **Finance owns every monetary effect.** A WorkItem can request a financial
   action, but Commitment, Invoice, Payment, and Refund states exist in Finance.
5. **Approval is a governed bridge.** A sensitive Work or Finance effect names
   its policy and authorized Human. Approval never becomes a casual comment or
   a Wave gate.
6. **Execution remains evidence, not company structure.** Mission/Wave,
   AgentTeamRun, WorkflowRun, Host execution, Git, and external delivery prove
   how work ran; they do not replace WorkItem, Organization, or Docs.

## Trademark example

- **Docs** holds the trademark strategy and application record.
- **Work** creates “Submit CN trademark filing”, links its Milestone, assigns
  the IP Agent, and waits for the required approval.
- **Organization** supplies the IP Agent, accountable Human owner, external
  counsel relationship, and Founder approval authority.
- **Finance** creates a pending ¥3,000 Commitment linked to the WorkItem and
  application. It creates no Payment before authorization and settlement.
- After approval and filing, Work records evidence and completion, Finance
  records only the effects that occurred, and Docs receives the filing result.

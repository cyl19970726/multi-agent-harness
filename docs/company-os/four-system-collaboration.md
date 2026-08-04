# Docs, Organization, Work, and Finance

```text
status: canonical product responsibility map
owner_role: product-architecture
canonical_for: four-system ownership and cross-system operating loop
```

The Company OS has four cooperating systems. They do not own four copies of the
same company state. Each system owns one kind of truth and connects to the
others through stable relations.

The systems are operated by AgentMembers inside recursive AgentTeams. The
minimum initial topology is:

```text
Supervising Operator
  <-> Lead AgentMember (root Team Host)
        ├── Docs Member
        ├── Work / Product Member
        └── CTO or domain Member
            └── optional child AgentTeam
```

The Lead manages only direct Members. Any Member may Host a child Team and
delegate its owned Work while remaining accountable upward. Docs, Work,
Finance, Org/HR, CTO, or domain names are roles chosen by a company rather than
required scheduling layers. The Supervising Operator can see the complete tree,
create unassigned intake Work, and message the Lead, but does not perform
routine assignment or acceptance.

## Responsibility map

| System | Owns | Does not own |
| --- | --- | --- |
| **Docs** | Documents, Blocks, TypedRecords, Relations, Views, BusinessModules, durable decisions and result narratives | task lifecycle, actor authority, payment state |
| **Organization** | Human and AgentMember identity, recursive AgentTeam topology, local Host authority, availability and capacity when explicit | Work status, document content, financial transaction state |
| **Work** | shared Works, Milestones, ownership, lifecycle, Approval/business relations, execution/delivery references, evidence and result routing | source knowledge, organization identity, financial ledger effects |
| **Finance** | budgets, Commitments, invoices, Payments, refunds, financial metrics, evidence and financial state transitions | general tasks, company knowledge narrative, actor hierarchy |

The owning system is the only place allowed to assert its truth. Other systems
render linked projections. For example, Docs may display a ¥3,000 Commitment,
but Finance owns its amount and state. Work may display an accountable Agent,
but Organization owns the Agent's identity and authority.

## Shared operating loop

```text
Docs: source context and proposed action
  -> Work: durable commitment and responsibility
  -> Organization: accountable Members and direct-Team administration
  -> Finance: governed monetary effect when the work has one
  -> execution: human / AgentMember / Mission-Wave / Team / Workflow / Host
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
2. **Work coordinates responsibility.** Work names Team scope, creator,
   assignee, parent Work, completion criteria, source, result, evidence, and
   execution references.
3. **Organization decides who may administer whom.** AgentMember identity and
   direct Host/Member topology come from recursive AgentTeams; Work references
   them explicitly.
4. **Finance owns every monetary effect.** A WorkItem can request a financial
   action, but Commitment, Invoice, Payment, and Refund states exist in Finance.
5. **Approval is a governed bridge.** A sensitive Work or Finance effect names
   its policy and authorized Human. Approval never becomes a casual comment or
   a Wave gate.
6. **Execution remains evidence, not identity.** MemberRun, provider-native
   session, Mission/Wave, WorkflowRun, Git, and external delivery prove how Work
   ran; they do not replace durable AgentMember, Work, or Docs truth.

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

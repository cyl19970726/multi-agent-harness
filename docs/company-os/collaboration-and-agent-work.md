# Collaboration and Agent Work

```text
status: canonical Company OS contract
owner_role: product
canonical_for: Lead/direct-report collaboration, object-linked conversation,
  Standing Agent versus execution-member boundaries, and result promotion
```

## Product boundary

Organization contains durable company members. Mission, Wave, AgentTeamRun,
MemberRun, and WorkflowRun are one-time long-task capabilities used to perform
a bounded WorkItem. They may share collaboration UI, transport, and artifact
components, but they do not share identity or lifecycle.

```text
Human Owner
  -> Lead Agent
     -> second-level Standing Agents
     -> starts one-time execution when a WorkItem needs it
        Mission -> ordered Wave -> Agent Team | Workflow | Host/direct work
```

A Standing Agent may explicitly participate in a TeamRun through a MemberRun.
The link is `StandingAgent -> participates_as -> MemberRun`; the MemberRun does
not become a new organization member, and its completion does not retire the
Standing Agent.

## Collaboration spine

The product does not create a global chat room as a second source of company
context. Collaboration is attached to a durable subject:

```text
CollaborationSubject = Document | BusinessModule | Milestone | WorkItem |
                       Approval | OrganizationRelationship | Mission |
                       Wave | AgentTeamRun | WorkflowRun
```

Shared primitives are deliberately small:

- `Conversation`: ordered, subject-linked communication context;
- `Message`: readable communication from a typed ActorRef or MemberRunRef;
- `ActivityEvent`: a source-labelled durable change or delivery event;
- `Handoff`: explicit sender, recipient, scope, context, and expected result;
- `ArtifactRef`: a Document, Evidence, record, file, diff, page, or external
  resource referenced by collaboration;
- `Presence`: transient availability or live execution signal;
- `Promotion`: deliberate movement of a useful execution summary, evidence,
  deliverable, or decision request into Work, Docs, Approval, or Finance.

Messages communicate context. They do not establish responsibility, approval,
or payment. Responsibility requires WorkItem and Assignment; authority requires
Approval; financial truth requires FinancialRecord.

## Where collaboration appears

| Surface | Primary collaboration question | Durable content |
| --- | --- | --- |
| Document | What changed, why, and what work follows? | comments, suggestions, linked WorkItems, accepted result updates |
| WorkItem | Who owns delivery, what is blocked, and what is the result? | assignments, handoffs, progress reports, evidence, review |
| BusinessModule | How does a recurring business function coordinate? | role roster, active Milestones, WorkItems, decisions, operating changes |
| Approval | What evidence and impact inform this controlled decision? | questions, recommendations, evidence, formal decision link |
| Lead Agent workspace | What do direct reports need, and what must the Lead coordinate? | object-linked conversation, assignments, blockers, summaries, org proposals |
| Standing Agent workspace | What is this Agent responsible for and how is it collaborating with the Lead? | explicit WorkItems, maintained Docs, handoffs, promoted execution outcomes |
| Mission/Team console | How is one bounded execution progressing? | execution messages, member handoffs, artifacts, review requests, live state |

## Lead and direct-report flow

1. Human gives the Lead business intent in the Lead workspace or a Document.
2. Lead links or creates the appropriate Milestone and WorkItem.
3. Lead assigns a direct Standing Agent, preserving sender, recipient, scope,
   acceptance, source, and result destination.
4. The direct report performs simple work directly or asks the Lead to start a
   one-time Mission, Agent Team, or Workflow.
5. Blockers and review requests roll up to the Lead's Needs Attention view.
6. Execution produces summaries, evidence, artifacts, and decision requests.
7. Only promoted outcomes update the WorkItem, source Document, Approval, or
   FinancialRecord.

Direct reports may communicate through shared object conversations. V1 does
not require an unstructured peer-to-peer channel graph; the Lead remains the
default assignment and escalation path.

## Creating organizational capability

The Lead distinguishes temporary execution capacity from a durable company
role:

- a temporary specialist becomes a MemberRun in the current one-time
  execution;
- a recurring missing capability becomes a Standing Agent proposal.

A Standing Agent proposal declares role charter, `reports_to`, responsibilities,
BusinessModule and Document scope, allowed actions, approval boundaries,
provider/budget policy, creation reason, and responsible Human authority.
Policy decides whether the Lead may activate a low-risk Agent directly or must
obtain Human approval. Financial, legal, external-access, credential, and
organization-wide authority changes require Human approval.

## Thinking and live state

Sanitized thinking preview may be shown while an eligible Agent or MemberRun is
actively working. It is transient Presence, not Message, Evidence, Activity,
or company knowledge. It is not persisted, replayed, searched, or used for
governance. Durable history contains only readable summaries, actions,
artifacts, evidence, and decisions.

## UI reuse rule

Organization and execution may reuse Actor cards, Conversation, Message,
Activity, Handoff, Artifact, Composer, Presence, compact Team/Wave controls,
and Context Rail modules. They do not reuse the same complete page template:

- Lead and Standing Agent workspaces emphasize long-lived responsibility,
  organizational relationships, WorkItems, Docs, and BusinessModules;
- Mission, Wave, TeamRun, and MemberRun pages emphasize one-time execution,
  attempts, member state, delivery, evidence, and gates.


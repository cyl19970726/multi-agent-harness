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
     -> Docs / Work / Finance / Org-HR Governance Agents
        -> Org-HR manages Business Agents
     -> starts one-time execution when a WorkItem needs it
        Mission -> ordered Wave -> Agent Team | Workflow | Host/direct work
```

A Standing Agent may explicitly participate in a TeamRun through a MemberRun.
The link is `StandingAgent -> participates_as -> MemberRun`; the MemberRun does
not become a new organization member, and its completion does not retire the
Standing Agent.

The two records remain in their native stores. Company snapshots build this
read-only relation by joining the Company Store's explicit Standing Agent id
with the selected Execution Space's `MemberRun.agent_member_id`, assignment
correlation, and TeamRun. They do not copy MemberRuns or provider sessions into
Company storage.

External messages to a Standing Agent use its stable Agent Inbox. When that
Agent is participating in a live TeamRun, an explicit idempotent
`AgentMessageRoute` may route the message to its active MemberRun and typed
TeamMessage. The current Team Supervisor delivers it to the provider-native
session. This transport join never makes StandingAgent, AgentMember, and
MemberRun one object.

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

Agent Team delivery adds transport facts without changing those rules:
Supervisor claim, provider receipt, recipient ACK, semantic reply, and Host
acceptance are distinct. An Operator or Host cannot impersonate a Member merely
by choosing its name in a composer.

## Where collaboration appears

| Surface | Primary collaboration question | Durable content |
| --- | --- | --- |
| Document | What changed, why, and what work follows? | comments, suggestions, linked WorkItems, accepted result updates |
| WorkItem | Who owns delivery, what is blocked, and what is the result? | assignments, handoffs, progress reports, evidence, review |
| BusinessModule | How does a recurring business function coordinate? | role roster, active Milestones, WorkItems, decisions, operating changes |
| Approval | What evidence and impact inform this controlled decision? | questions, recommendations, evidence, formal decision link |
| Organization overview | Who reports to whom, what capability is missing, and which changes are pending? | reporting relations, configuration, explicit WorkItems, capability gaps, org proposals |
| Agent configuration/profile | What responsibility, prompt, tools/Skills, permissions, and records are assigned? | declared configuration and stable linked records; rich standalone workspace deferred |
| Mission/Team console | How is one bounded execution progressing? | execution messages, member handoffs, artifacts, review requests, live state |

## Lead and direct-report flow

1. Human gives the Lead business intent in a Document or governed company
   surface.
2. Lead routes the need to the appropriate Governance Agent.
3. Docs Governance places durable context; Work Governance creates or routes
   the WorkItem; Finance handles monetary effects; Org/HR supplies organization
   identity, capacity, and Business Agents.
4. A Business Agent performs simple work directly or uses a linked one-time
   Mission, Agent Team, Workflow, Host, external, or human execution path.
5. Blockers and review requests roll up to the Lead's Needs Attention view.
6. Execution produces summaries, evidence, artifacts, and decision requests.
7. Only promoted outcomes update the WorkItem, source Document, Approval, or
   FinancialRecord.

Actors may communicate through shared object conversations. V1 does not require
an unstructured peer-to-peer channel graph. Lead is the company escalation
path; Org/HR is the organizational manager for Business Agents; WorkItem roles
remain the source of execution responsibility.

## Current core loop: Docs + WorkItem + Organization

The current implementation priority is intentionally narrower than the full
four-system model:

```text
Docs holds context and receives durable results
  -> WorkItem holds the commitment and lifecycle
  -> Organization holds who exists, reports, and may act
  -> Execution performs the work through the appropriate substrate
  -> Docs and Work receive the accepted outcome and evidence
```

Finance is not in the default loop. It enters only when a WorkItem asks for a
monetary effect. That keeps normal document maintenance, development, merchant
follow-up, content planning, and org-capability work lightweight while still
preserving a clean escalation path for purchases, invoices, payouts, refunds,
and budgets.

The key autonomous pattern is:

1. an Agent reads Docs, typed records, Work views, Org state, or a gateway
   observation;
2. it identifies a gap, blocker, outdated record, missing capability, or next
   action;
3. it creates or routes a WorkItem with source context, role assignments,
   acceptance criteria, and relevant refs;
4. the responsible Actor performs the work directly or through Mission/Wave,
   Agent Team, Workflow, host execution, external work, human work, or a
   provider-native subagent;
5. only the promoted outcome updates Docs, Work, Org proposals, or related
   typed records.

For example, a Docs Governance Agent may inspect a module, find that merchant
FAQ and reward redemption policy are inconsistent, create a docs-maintenance
WorkItem, perform it itself, or ask a lower Docs Agent / temporary subagent to
draft the patch. The durable company facts remain the WorkItem, source and
result Documents, explicit ActorRefs, and evidence refs.

## Standard documents, module views, and custom pages

Company OS should not require a heavy Notion-style editor to make progress.
The baseline surface is standard Document rendering with Blocks, TypedRecords,
Relations, Views, and related-module panels. This is the default for most
company memory because Agents primarily edit through CLI/API and humans mostly
read, review, and supervise.

Custom pages are reserved for high-value operating surfaces where standard
Documents and Views are insufficient: a launch command center, Work board,
merchant network control page, Agent profile workspace, GitHub delivery page,
or visual acceptance report. A custom page is presentation over native Store
records; it must keep a standard Document/View fallback so Agents can still
operate through CLI and future UIs can reconstruct the same truth.

## Creating organizational capability

Org/HR evaluates temporary execution capacity versus a durable company role,
and Lead sponsors or approves within policy:

- a temporary specialist becomes a MemberRun in the current one-time
  execution;
- a recurring missing capability becomes a Standing Agent proposal.

A Standing Agent proposal declares role charter, `reports_to`, responsibilities,
BusinessModule and Document scope, allowed actions, approval boundaries,
provider/budget policy, creation reason, and responsible Human authority.
Policy decides whether Org/HR may provision a low-risk Agent after Lead approval
or must obtain Human approval. Financial, legal, external-access, credential, and
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

- Organization profiles emphasize declared responsibility, prompt,
  tools/Skills, permissions, reporting, WorkItems, Docs, and BusinessModules;
- Mission, Wave, TeamRun, and MemberRun pages emphasize one-time execution,
  attempts, member state, delivery, evidence, and gates.

Rich standalone Agent workspaces are optional future composition, not a current
prerequisite for the organization or Work operating model.

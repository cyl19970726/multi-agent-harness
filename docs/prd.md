# Product Requirements — AI Company OS

## Product mission

Star Harness helps a person and a mixed organization of standing Agents run a
company through durable documents, explicit responsibility, governed actions,
and provider-neutral execution tools.

The product is not primarily a multi-agent run dashboard. It is a Company OS:

```text
Docs organize business intent and knowledge.
Organization supplies long-lived human and Agent capability.
WorkItems connect intent to accountable execution.
Approvals protect high-risk actions.
Execution tools perform the work.
Results and effects return to Docs and related records.
```

## Product thesis

An AI-native company needs more than parallel agents. It needs:

- a place where company context, data, decisions, and operating structures
  remain understandable as they grow;
- durable Agent identities with roles, permissions, availability, capacity,
  responsibilities, and provider runtime history;
- first-class human, external, service, and Agent participation without
  pretending their lifecycles are identical;
- explicit records of who requested, submitted, owns, executes, reviews, and
  approves work;
- typed relations so a business action, its cost, its approval, and its result
  remain one connected truth;
- execution tools that can be selected when useful without forcing every piece
  of company work into a predefined execution structure.

## Primary systems

### Docs

Docs is the company memory and operating hub. It must support:

- basic rich documents: hierarchical pages, text, lists, checklists, callouts,
  media, attachments, comments, mentions, and ordinary tables;
- structured documents: standard table, board, timeline, calendar, chart, and
  embedded related-record views over TypedRecords and Relations;
- typed records and relations rather than copied values;
- templates and Module Designs for repeatable business domains;
- actions that create WorkItems and Approvals with source-document provenance;
- result, evidence, metric, and financial-effect updates back into the source;
- structure-health, reorganization, archival, and conflict detection.

For a stable, high-value surface that must coordinate several kinds of data and
actions, a Module may register a custom HTML/React page. An Agent may compose
this page from approved components, declared queries, and named Action
Commands. The page is never a data store: it cannot directly write business
facts or bypass permissions, audit, relation validation, or Approval policy.
Every custom page links to its underlying Documents and records and has a
standard document/view fallback.

### Organization

Organization models `HumanMember`, standing `AgentMember`, external
collaborators, and services through common `ActorRef` references while
preserving their distinct identity and runtime rules.

The initial operating model is deliberately governance-led: one Human Owner,
one Lead Agent, and four direct Governance Agents for Docs, Work, Finance, and
Org/HR. All Business Agents report to Org/HR. Docs, Work, and Finance Governance
Agents collaborate with them through governed records and Actions without
becoming their organizational manager. `reports_to_actor_ref` and
`OrgUnit.parent_unit_id` keep later hierarchy explicit and additive.

Organization collaboration is object-centred. Human and Agent conversation,
handoff, activity, and artifacts remain linked to a Document, BusinessModule,
Milestone, WorkItem, Approval, or execution attempt. The Organization overview
and compact Actor configuration compose those explicit links; dedicated Agent
workspaces are deferred and ordinary provider logs are never company context.

### Work and approvals

`WorkItem` is the product-level work record. It is distinct from executor
internals and from ordinary messages.

`Milestone` is the only grouping layer above WorkItems. It records a named
stage outcome, owner, target date, acceptance criteria, and the WorkItems that
contribute to it. There is no separate canonical `Project` object. A
BusinessModule or Document supplies durable business context while Work owns
Milestones and WorkItems.

Every WorkItem records:

- source document and result document;
- requested by and submitted by;
- accountable owner;
- assignees and contributors;
- reviewer and approver when required;
- execution reference;
- result, evidence, metrics, and linked financial records.

`Approval` records legal, financial, permission, publication, and organization
gates. Policies may require a human actor; an Agent cannot impersonate that
approval.

### Relations, finance, and metrics

Structured records are linked, not duplicated. A trademark filing fee shown in
a trademark document and in Finance is one `FinancialRecord` with relations to
the application, BusinessModule, Milestone, source document, WorkItem,
approval, and evidence.

Finance distinguishes budget, commitment, invoice, payment, refund, and
forecast. Metrics distinguish definitions from timestamped observations and
retain their source.

### Governance

- Docs Governance Agent proposes new or reorganized Document and Module
  structures.
- Work Governance Agent classifies and routes durable WorkItems and their
  cross-system effects.
- Finance Governance Agent manages monetary requests, controls, evidence, and
  authorized financial transitions.
- Org/HR Governance Agent evaluates capability gaps and proposes, provisions,
  evaluates, or retires Business Agents through governed organization Actions.
- Finance, legal, security, and domain reviewers evaluate affected relations.
- A Lead or human authority approves changes according to risk policy.
- Proposals and decisions remain reconstructable from source to effect.

## Execution foundation

A WorkItem may be executed directly by a human or Standing Agent, or may start
one of the product's one-time long-task capabilities:

```text
Mission -> ordered Host-plan Wave
Mission <-> independent AgentTeam
execution = Agent Team | Dynamic Workflow | Host work
```

- `Mission` structures one bounded long-running outcome and links reusable
  Agent Teams.
- `Wave` preserves the Host's evolving plan and judgment without becoming a
  runtime container or barrier.
- `AgentTeamRun/MemberRun` records temporary collaboration that may span
  several Waves while native sessions continue.
- `DynamicWorkflow` runs a provider-neutral process for the bounded outcome.
- provider sessions, plugins, MCP, and child work remain execution evidence.

No executor owns the originating document, organization, approval, or company
record. Results return through the WorkItem relation.

## Required product experiences

1. Company Home surfaces decisions, milestone state, metrics, financial pressure,
   and organization capacity with links to source documents.
2. Docs supports Notion-like editing and nesting, ordinary tables, structured
   relations, Module templates, embedded operating views, and governed custom
   pages for core surfaces.
3. Organization shows a mixed company and distinct details for humans and
   standing Agents.
4. Work is the company-wide ledger of Milestones and typed WorkItems and makes
   submission, responsibility, remaining work, and acceptance visible.
5. Approvals provides a focused `Needs You` queue and complete audit history.
6. Finance provides typed, permissioned records linked to business origin.
7. Governance handles new business domains, document growth, organization
   change, and missing capability.
8. Execution pages retain Mission/Wave, Team, MemberRun, Workflow, and provider
   observability as professional drill-ins.

## Near-term acceptance scenario

The first Company OS scenario is a new Trademark Management module:

- Document Architecture proposes its document space, templates, record types,
  relations, views, permissions, and archival rules.
- Organization Governance identifies a Brand Owner human, Trademark Agent,
  Finance Agent, and External Lawyer participation.
- a filing WorkItem preserves requester, submitter, owner, contributors,
  reviewer, and human approver;
- the ¥3,000 filing fee is one linked FinancialRecord, not copied text;
- approval updates Work, Finance, and the trademark application;
- documents receive the filing result, evidence, dates, cost, and next action.

## Non-goals

- do not make raw provider transcripts or thinking the company knowledge base;
- do not infer assignment from matching names, roles, providers, or sessions;
- do not make every message a WorkItem;
- do not introduce a separate Project object above Milestone and WorkItem;
- do not force every WorkItem into Mission/Wave or another executor;
- do not use runtime status as business availability;
- do not let an Agent satisfy a human-required approval;
- do not copy finance or metric values between modules;
- do not let page code become a second source of truth or a policy bypass.

## Implementation truth

The execution foundation is substantially implemented. The Company OS objects
and primary frontend are an additive migration in progress. Documentation must
label planned fields and projections honestly until schemas, store, APIs,
fixtures, and acceptance checks exist.

See [Company OS docs](company-os/README.md) and
[ADR 0027](decisions/0027-company-os-primary-model.md).

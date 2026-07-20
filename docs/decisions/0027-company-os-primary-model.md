# ADR 0027: Docs and mixed Organization are the Company OS primary model

## Status

Accepted for product direction; additive implementation in progress.

## Context

Mission/Wave and Agent Team solve structured execution, but execution records do
not by themselves organize a company. A company also needs durable business
context, evolving document architecture, explicit responsibility, human
authority, financial provenance, and a way to add new domains without turning
provider activity into the source of business truth.

The earlier product framing placed Standing Agents and Docs after the execution
control plane. Product discovery established that they are the primary user
experience, while Mission/Wave, Agent Team, Dynamic Workflow, Host execution,
providers, plugins, and MCP are reusable tools beneath it.

## Decision

Star Harness is an AI Company OS with two primary systems:

1. Docs owns company memory, business modules, typed records, relations, views,
   decisions, and the source/result context of company work.
2. Organization owns explicit human, Standing Agent, external, and service
   actors, their roles, OrgUnits, permissions, availability, and capacity.

`WorkItem`, `Assignment`, and `Approval` connect those systems to execution.
Mission/Wave remains the native ordered execution model, but it is optional per
WorkItem and never owns company documents, financial approval, or organization
truth.

Shared business facts are represented once and linked. For example, a ¥3,000
trademark filing commitment is one FinancialRecord related to the trademark
application, WorkItem, approval, project, and source document.

Human-required approval cannot be satisfied by an Agent. Human, Agent,
external, service, TeamRun MemberRun, and provider-session lifecycles remain
distinct even where their UI components are shared.

## Consequences

- Primary navigation becomes Home, Docs, and Agents. Work, Approvals, Finance,
  and Governance are shared operating views; Missions, Workflows, Agent Teams,
  providers, and plugins are execution drill-ins.
- New company work uses WorkItem rather than legacy engineering Task.
- Agent activity, chat, raw provider transcript, and model thinking are not
  authoritative company knowledge. Sanitized thinking may be transient live UI
  only and is neither persisted nor evidence.
- New business domains require a Module Design covering records, relations,
  views, permissions, finance/metric effects, actors, and migration—not only a
  folder.
- The superseded coordination model is retired under ADR 0028. Historical data
  is exported and verified before destructive deletion; it is not retained as
  an active product compatibility layer.
- Documents support basic rich content, standard structured Views, and optional
  governed Custom Pages as decided by ADR 0029.
- Planned Company OS contracts must be labelled planned until implemented and
  accepted in schemas, stores, APIs, fixtures, and UI.

## Supersedes and preserves

This ADR supersedes the product-scope assumption in ADR 0026 that Standing
Agents and Docs are future layers. It preserves ADR 0026's native Mission/Wave
execution hierarchy, lightweight Wave model, executor ownership, and
transient-thinking policy. ADR 0028 separately supersedes ADR 0026's legacy
compatibility policy and retires that older model after verified export.

## Validation

The first acceptance case is the governed Trademark Management module described
in `docs/company-os/examples/trademark-registration.md`. Its application,
WorkItem, participants, human approval, ¥3,000 financial commitment, evidence,
and source/result documents must remain linked across Docs, Work, Finance,
Agents, and Governance views.

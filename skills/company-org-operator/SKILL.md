---
name: company-org-operator
description: Operate Company OS Organization through governed Store/API/Action contracts. Use when a Governance Agent needs to inspect, propose, or manage Humans, Standing Agents, OrgUnits, roles, reporting, permissions, and capability lifecycle without confusing standing actors with one-off Agent Team members.
---

# Company Org Operator

Operate the Company OS Organization surface. This skill is a procedural
capability, not product authority. It helps an Agent inspect and prepare
Organization changes while respecting Human approval, permissions, and the
boundary between durable Standing Agents and one-off execution members.

## Load the contracts

Before proposing or executing a durable Organization change, read:

- `docs/company-os/organization-and-actors.md`
- `docs/company-os/governance-agent-workspaces.md`
- `docs/company-os/collaboration-and-agent-work.md`
- `docs/company-os/implementation-truth-matrix.md`
- `docs/company-os/skill-contracts.md`
- `docs/company-os/governance.md`

If repository files, schemas, API code, or acceptance checks conflict with this
skill, the canonical implementation contract wins.

## Operating boundary

Organization owns who exists and who may act:

- `HumanMember`
- `AgentMember` / durable Standing Agent
- external collaborator or service actor
- `OrgUnit`
- role and reporting relation
- permission and authority profile
- membership lifecycle
- organization change proposal and approval path

Organization does not own:

- WorkItem lifecycle or milestone status.
- Docs content and module structure.
- Finance commitments or payments.
- Mission/Wave, AgentTeamRun, MemberRun, provider-native sessions, or workflow
  steps.

A Standing Agent is a durable company actor. An Agent Team MemberRun is a
one-off execution participant bound to an AgentTeamRun and provider-native
session. They may share UI components, but they are not the same product object.

## Current interface state

Organization records exist through the Company OS Store/API and governed Action
path. Until dedicated `harness company org ...` commands are implemented, use
the current API/action contract and report CLI coverage honestly as `partial`.

The intended command family is:

```bash
harness company org query --actor <actor-id>
harness company org list --unit <org-unit-id>
harness company org propose-agent --reports-to <lead-agent-id> --role <role-id> --reason <reason>
harness company org update-permissions --actor <actor-id> --permission <permission> --approval <approval-id>
harness company org transition-agent --actor <actor-id> --status <status>
harness company org record-capability-review --actor <actor-id> --evidence <ref>
```

Do not present those commands as implemented until the CLI and acceptance tests
exist.

## Governance model

The first Company OS layer is governance:

- Human Owner sets company direction and Human gates.
- Lead Agent manages Governance Agents.
- Docs Governance Agent owns company memory structure.
- Work Governance Agent owns WorkItem routing and commitment visibility.
- Finance Governance Agent owns money state and finance controls.
- Org / HR Governance Agent owns actors, roles, authority, capability, and
  lifecycle.

Business Agents sit under Org / HR governance. HR/Org may identify capability
gaps, reuse existing agents, request temporary execution, propose a new
Standing Agent, provision approved tools/skills/permissions, and later evaluate,
adjust, or retire the actor. Skills are tools; they never grant authority.

## Safe workflow

1. Inspect the actor, org unit, role, and permission context before proposing a
   change.
2. Classify the request: view current org, route work to existing actor, propose
   new business agent, update permission, pause/retire actor, or review
   capability.
3. Prefer reuse. Check whether an existing Human, Standing Agent, external
   collaborator, service, Agent Team, Dynamic Workflow, or Host execution path
   can do the work before adding a durable actor.
4. For new actors or permission expansion, prepare an Organization change
   proposal and route required Human/Lead approval.
5. Provision only approved tools, skills, budgets, and permissions. Do not infer
   authority from a prompt, profile, avatar, or UI card.
6. Link initial WorkItems and maintained Docs so the actor's purpose is
   observable.
7. Record evaluation and lifecycle changes as durable Organization records.

## Validation checklist

- Actor kind is explicit: Human, Standing Agent, external collaborator, service,
  or one-off execution participant.
- Reporting line and OrgUnit are explicit.
- Role and permission set are bounded.
- Required approval exists for adding actors or expanding authority.
- Related WorkItems and maintained Docs are linked.
- Skill/tool list is treated as capability, not authority.
- AgentTeam MemberRun/provider session is not mistaken for a durable Agent.

## Report format

When handing off, state:

- organization capability status: `implemented`, `partial`, `planned`, or
  `design-only`;
- actor/org-unit ids;
- role and permission changes;
- approval refs;
- linked WorkItems and Docs;
- capability/evaluation evidence;
- remaining system gaps.

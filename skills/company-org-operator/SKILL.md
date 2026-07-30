---
name: company-org-operator
description: Operate Company OS Organization through governed Store/API/Action contracts. Use when a Governance Agent needs to inspect, propose, or manage Humans, Standing Agents, OrgUnits, roles, reporting, permissions, and capability lifecycle without confusing standing actors with one-off Agent Team members.
---

# Company Org Operator

Operate the Company OS Organization surface. This skill is a procedural
capability, not product authority. It helps an Agent inspect and prepare
Organization changes while respecting Human approval, permissions, and the
boundary between durable Standing Agents and one-off execution members.

## Select the Company Store

Before reading or writing Company OS records, identify the Company Store. Prefer
one of:

```bash
harness company current
harness --company <company-id> company org ...
HARNESS_COMPANY=<company-id> harness company org ...
```

If no Company is selected, `harness company ...` falls back to the current
project-derived compatibility Store. Treat that as legacy compatibility, not
the target Agent Company Workspace boundary.

To move legacy Company OS rows into a real Company Store, use
`harness company migrate-from-project --from-project <project-id|path> --id <company-id>`.
It copies only `company_os_*.jsonl`; it does not migrate execution records,
provider sessions, prompts, or runtimes.

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

Use `$dogfood-company-os` when the task is the repeated cross-system
self-hosting loop. This Skill owns only Organization reads and changes inside
that loop.

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
Link them only with the optional Company-owned
`StandingAgent.execution_agent_member_ref -> AgentMember.id`; never infer a
link from equal ids, names, roles, or providers. At creation time author the
link with `--execution-agent-member-ref <agent-member-id>`. For a Standing Agent
that already exists, use the governed relation commands instead:

```bash
harness company org link-execution \
  --authority <human-id> --actor <standing-agent-id> \
  --agent-member <agent-member-id> --execution-space <space-id>

harness company org unlink-execution \
  --authority <human-id> --actor <standing-agent-id> \
  [--expect-agent-member <agent-member-id>]
```

`--execution-space` is required and never defaults: AgentMember truth lives in
an Execution Space, and `harness company ...` resolves the Company Store only
(ADR 0042). The named space is opened read-only to confirm the AgentMember
exists, so a typo fails loudly instead of persisting a dangling reference. Type
both ids even when they are equal. Re-running the same pair changes nothing
(`changed: false`); repointing an existing link requires `--replace`.

An absent link means the Standing Agent has no reusable execution identity,
while an unlinked MemberRun remains ad-hoc execution. If the Dashboard reports
`standing_assignment_conflicts`, two Standing Agents claim the same AgentMember:
resolve it with `unlink-execution` on the wrong claimant rather than by editing
a ledger.

## Docs page integration

Business Docs pages may show responsible humans, Standing Agents, governance
Agents, external collaborators, and services in right rails or module cards.
Those UI appearances are references only. Organization remains the source of
truth for who exists, who reports to whom, and who may act.

When a page contract references actors, require:

- explicit `ActorRef` kind: human, agent, external, or service;
- OrgUnit / membership / role when relevant;
- permission and capability refs when the page expects the actor to execute or
  approve an action;
- escalation path or Human gate for sensitive operations;
- no authority inferred from a profile card, avatar, prompt, skill list, or
  chat participation.

If a page exposes an Agent detail or governance panel, it may reuse UI
components from Agent Team member pages, but the underlying object must remain
a durable Organization actor, not a MemberRun.

## Gateway-facing Standing Agents

Business-facing Standing Agents may receive input through external gateways
such as WeCom, GitHub, email, social platforms, ecommerce/logistics systems, or
future plugins. Organization owns the durable Agent identity, role,
permissions, tools, skill refs, maintained Docs, accepted WorkTypes, and
escalation policy. The gateway service is an intake adapter or service actor;
it does not grant authority to the Agent.

An Agent detail workspace should show Org identity, current WorkItems, maintained
Docs, permission/capability refs, plugin capabilities, account/message/order
or metric summaries, and recent gateway events. Agent-specific knowledge such
as merchant FAQ, reply policy, content guidelines, customer requests, or
evidence should remain in Docs/Relations and be loaded as context, not stored
as a different Agent-specific database.

Plugin view extensions may add an Agent detail panel such as "merchant inbox",
"social account status", "content performance", or "logistics blockers". The
panel must render Company OS records or scoped connector projections and link
back to source Documents, WorkItems, and evidence. It must not infer authority
from a logged-in platform account, phone session, profile card, or plugin
installation.

For GitHub, `$connect-github-company-os` owns repository facts and delivery
links. Organization owns which durable actor may triage, write, review, merge,
release, or change repository permissions; a valid `gh` login does not grant
that company authority.

## Current interface state

Organization records exist through the Company OS Store/API. The first
dedicated `harness company org ...` command family is implemented for
inspection and Human administrative authoring of actors, OrgUnits,
Memberships, declared actor status, and permission/capability refs.

Use:

```bash
harness company org list [--actor-kind human|agent|external|service] [--status <status>] [--unit <org-unit-id>]
harness company org query --actor <actor-id> [--actor-kind human|agent|external|service]
harness company org query --unit <org-unit-id>
harness company org query --membership <membership-id>
harness company org create-human \
  --id <human-id> \
  --display-name <name> \
  --responsibility <summary> \
  --authority <human-admin-id>
harness company org create-agent \
  --id <standing-agent-id> \
  --display-name <name> \
  --role <role> \
  --responsibility <summary> \
  --authority <human-admin-id> \
  [--execution-agent-member-ref <agent-member-id>] \
  [--skill <skill-id> --tool <tool-id> --permission <policy> --capability <capability>]
harness company org create-unit \
  --id <org-unit-id> \
  --name <name> \
  --purpose <purpose> \
  --authority <human-admin-id> \
  [--parent-unit <id> --human-lead <human-id> --agent-lead <agent-id>]
harness company org add-membership \
  --unit <org-unit-id> \
  --actor <actor-id> \
  --actor-kind human|agent|external|service \
  --role lead|member|advisor|observer|external_partner \
  --authority <human-admin-id>
harness company org transition-actor \
  --actor <actor-id> \
  --actor-kind human|agent|external|service \
  --status active|invited|paused|ended|archived \
  --authority <human-admin-id>
harness company org update-permissions \
  --actor <actor-id> \
  --actor-kind human|agent|external|service \
  --permission <policy-ref> \
  --authority <human-admin-id>
```

The nested operator aliases are also available:

```bash
harness company org actor list
harness company org actor show --actor <actor-id>
harness company org actor create-human --id <human-id> --name <name> --responsibility <summary>
harness company org actor create-agent --authority <human-admin-id> --id <agent-id> --name <name> --role <role> --responsibility <summary> --permission <policy-ref> --capability <capability-ref>
harness company org actor update-status --authority <human-admin-id> --actor <actor-id> --status active|paused|ended|archived
harness company org unit list
harness company org unit show --unit <org-unit-id>
harness company org unit create --authority <human-admin-id> --id <unit-id> --name <name> --purpose <purpose> --human-lead <human-id> --agent-lead <agent-id>
harness company org unit update-status --authority <human-admin-id> --unit <unit-id> --status active|paused|archived
harness company org membership list
harness company org membership assign --authority <human-admin-id> --unit <unit-id> --actor <actor-id> --actor-kind human|agent|external|service --role lead|member|advisor|observer|external_partner
harness company org membership update-status --authority <human-admin-id> --membership <membership-id> --status active|invited|paused|ended
```

Current v1 boundary:

- Writes use the existing Human administrative authoring surface. The authority
  must be an active Human with `company_os.admin`.
- The CLI does not yet implement a governed OrgChangeProposal lifecycle,
  multi-party approval workflow, promotion policy, retirement evaluation, or
  capability-review record type.
- For permission expansion, new durable actors, or org-structure changes,
  report the write as administrative v1 and preserve the follow-up need for a
  governed proposal/approval path.
- A Standing Agent record is organization identity and authority context. It is
  not an Agent Team MemberRun, provider-native session, or runtime health row.

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
   If the request comes from a Docs page, inspect its page contract and related
   WorkItems so the actor is linked to the intended business responsibility.
2. Classify the request: view current org, route work to existing actor, propose
   new business agent, update permission, pause/retire actor, or review
   capability.
3. Prefer reuse. Check whether an existing Human, Standing Agent, external
   collaborator, service, Agent Team, Dynamic Workflow, or Host execution path
   can do the work before adding a durable actor.
4. For new actors or permission expansion, use the Org CLI only when the Human
   administrative boundary is acceptable; otherwise prepare an Organization
   change proposal and route required Human/Lead approval once that lifecycle
   exists.
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
- Docs pages show actor refs from Organization, not copied names or inferred
  permissions.

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

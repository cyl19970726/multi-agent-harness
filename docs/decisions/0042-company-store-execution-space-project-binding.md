# ADR 0042: Company Store, Execution Space, and Project Binding

```text
status: accepted
date: 2026-07-28
tracks: https://github.com/cyl19970726/multi-agent-harness/issues/252
supersedes: repo-derived Company OS Store ownership implied by ADR 0033 docs
```

## Context

The current multi-project implementation resolves one `ProjectContext` from a
repository or path. That context carries both:

- `project_root`, the workspace where providers run and discover project
  instructions; and
- `store_root`, the JSONL store where Mission/Wave, Agent Team, Workflow, and
  Company OS records are written.

That coupling is useful for repository execution and compatibility, but it is
not the right product boundary for an Agent-native company.

An Agent Company can contain multiple operating areas and software sources:

```text
Agent Company Workspace
├── Wanchengwanling
│   ├── commercial model
│   ├── merchant operations
│   ├── procurement
│   └── content operations
└── AgentOS / Star Harness
    ├── product strategy
    ├── development operations
    ├── plugin and gateway model
    └── dogfood intake
```

Those areas share Docs, Organization, Work, Finance, governance, and company
identity even when implementation sources live in different Git repositories.
Conversely, Mission/Wave, Agent Team, Dynamic Workflow, and Host execution must
remain usable by people who never initialize Company OS.

## Decision

Introduce three independent native identities, connected only by explicit
relations:

```text
Company Store       Execution Space       Project Binding
     \                    |                    /
      \------ explicit, optional relations ---/
```

### Company Store

The Company Store owns durable company operating truth:

- `Document`, `Block`, `TypedRecord`, `Relation`, `View`, `BusinessModule`;
- Organization actors, OrgUnits, memberships, authority, and permissions;
- `WorkItem`, `Milestone`, `Assignment`, `Approval`, and result routing;
- financial commitments, invoices, payments, refunds, evidence, and metrics;
- company governance records and operating-area structure.

It does not own Mission/Wave lifecycle, Team messages, provider sessions,
runtime process handles, repository roots, worktrees, or source-control state.

Product language may present this as an **Agent Company Workspace**. The
technical boundary is `Company Store`.

### Execution Space

An Execution Space is a provider-neutral coordination namespace. It owns:

- Mission and ordered Host-plan Wave revisions;
- reusable Agent Team definitions and Mission relations;
- AgentTeamRun, MemberRun, TeamMessage, PendingInteraction, outcomes, and
  artifact/check references;
- Dynamic Workflow runs and steps;
- Host execution outcomes and execution-facing provider/runtime bindings.

The key invariant is:

```text
Company is optional for every execution object.
```

No Mission, Wave, Agent Team, TeamRun, MemberRun, WorkflowRun, Host execution,
or provider session requires a `company_id`. An Execution Space may later link
to a Company Store, but standalone execution remains first-class.

### Project Binding

A Project Binding describes an execution resource, not a Store owner:

- stable project id;
- canonical local project root;
- repository URL and default branch where applicable;
- Git common directory;
- project instructions/config discovery boundary;
- permission and worktree policy;
- software source and delivery reference metadata.

Provider cwd selection remains:

```text
MemberRun.worktree_ref
  > AgentTeamRun.execution_root
  > ProjectBinding.project_root
```

`store_root` must never become provider cwd.

## Current compatibility boundary

`ProjectContext { id, project_root, store_root, kind, is_git_repo }` remains the
implemented compatibility path for repository execution and project-derived
stores. Existing `harness init`, `harness project ...`, `harness mission ...`,
and `harness team-run ...` commands keep their current behavior until later
phases add Execution Space and Project Binding registries.

As of the first ADR 0040 implementation slice, `harness company
init/list/current/show/switch/migrate-from-project`, `--company <id>`, and
`HARNESS_COMPANY` create, select, and populate explicit Company Stores for
`harness company ...` commands. If no Company is selected, the old
project-derived Company OS compatibility path still works.

This ADR changes the target architecture and documentation authority. It does
not silently migrate stores, dual-write ledgers, or reinterpret existing rows.

## Target storage layout

Logical separation is mandatory. Physical co-location is not.

```text
~/.harness/
├── companies/
│   └── <agent-company-id>/
│       ├── company_os_documents.jsonl
│       ├── company_os_typed_records.jsonl
│       ├── company_os_work_items.jsonl
│       ├── company_os_human_members.jsonl
│       ├── company_os_standing_agents.jsonl
│       └── company_os_commitments.jsonl
│
├── execution-spaces/
│   ├── <execution-space-id>/
│   │   ├── missions.jsonl
│   │   ├── waves.jsonl
│   │   ├── agent_teams.jsonl
│   │   ├── team_runs.jsonl
│   │   ├── member_runs.jsonl
│   │   └── team_messages.jsonl
│   └── <standalone-space-id>/
│
└── projects/
    ├── registry.json
    ├── multi-agent-harness.json
    └── wanchengwanling.json
```

## CLI direction

Standalone execution remains possible:

```bash
harness space init --id personal-dev --name "Personal Development"
harness space switch personal-dev
harness mission create --title "Refactor API" --objective "..."
```

The current low-friction repo workflow remains a compatibility path:

```bash
cd multi-agent-harness
harness init
```

It should create or select a repo-derived compatibility Execution Space and a
Project Binding. It must not implicitly create a Company Store.

Company OS commands resolve an explicit or current selected Company Store:

```bash
harness company init --id <agent-company-id> --name <display-name>
harness company switch <agent-company-id>
harness company current
harness --company <agent-company-id> company docs query --document <doc-id>
harness company migrate-from-project \
  --from-project <project-id-or-path> \
  --id <agent-company-id> \
  --name <display-name>

harness company docs ...
harness company work ...
harness company org ...
harness company finance ...
```

Project selection becomes execution-resource selection:

```bash
harness team-run create \
  --space <execution-space-id> \
  --mission-id <mission> \
  --project multi-agent-harness
```

## Relations

Relations do not transfer truth ownership:

- Company `WorkItem.execution_ref` points to an execution object.
- Execution returns outcome, artifact/check refs, metrics, and `DeliveryRef`s.
- Accepted results update Company Docs/Work/Finance only through governed
  Company actions.
- Provider-native sessions remain execution truth and are referenced rather
  than copied.
- Git repositories own source code, software PRDs, commits, PRs, CI, releases,
  and delivery evidence.

## Migration phases

1. **Freeze boundaries.** Add this ADR and update system maps so Company Store,
   Execution Space, and Project Binding are distinct target identities. Preserve
   current behavior.
2. **Company Store v1.** Add Company registry, init/switch/list/show commands,
   and Store routing for Docs/Work/Org/Finance. The CLI/store-routing slice and
   guarded `company_os_*.jsonl` migration from project-derived stores are
   implemented; serve API selectors and the Dashboard Company Store picker are
   implemented; broader execution-space migration remains pending.
3. **Project Binding.** Extract repo/path/worktree metadata from Store
   ownership while preserving cwd/worktree validation.
4. **Execution Space.** Add Execution Space registry and route new
   Mission/Wave/Agent Team/Workflow writes through it.
5. **Migration and UI.** Export and verify existing project-scoped Company OS
   rows, migrate selected records into a Company Store, add Company and
   Execution Space selectors, and preserve provider-native history.

Do not silently dual-write. Migration must be explicit, latest-wins safe, and
reconstructable.

## Acceptance

1. Standalone execution can create Mission/Wave/Agent Team/Workflow records
   without any Company Store.
2. A Company Store can contain Wanchengwanling and AgentOS operating areas in
   one company truth boundary while mapping multiple external repositories.
3. One Mission can use multiple Project Bindings through separate TeamRuns or
   MemberRuns without duplicating Mission/Wave history.
4. Switching Company never silently switches Execution Space; switching
   Execution Space never rewrites Company truth; selecting Project Binding never
   reroutes Company writes.
5. Provider cwd is always a project root or validated worktree, never a Company
   Store or Execution Space directory.
6. Execution completion or Wave advance cannot approve finance, legal,
   permission, or Organization changes.

## Consequences

- The current `ProjectContext` is reclassified as compatibility infrastructure,
  not the long-term owner of Company OS truth.
- `--project` remains valid for current commands until migrated, but docs must
  describe it as compatibility when discussing Company OS Store ownership.
- Wanchengwanling and AgentOS should eventually live as operating areas in one
  Agent Company Workspace, with their GitHub repositories mapped as external
  source and delivery systems.

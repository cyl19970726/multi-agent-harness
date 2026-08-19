# ADR 0042: Company Store (retired), Execution Space, and Project Binding

> Successor (DOC-16 row, DEV-40 flip 2026-08-18): [DOC-105](https://app.notion.com/p/3be49a4fa379817aa594fd8e7331c30d) + [DOC-108](https://app.notion.com/p/3be49a4fa37981afa320f6c8a5f3a8b4).

> **Partially superseded by DOC-108 (legacy CompanyOS retirement, 2026-08-17).**
> The Company Store third of this decision is retired: its identity, selector,
> ledgers, and commands are gone and survive only as export/verify history.
> The Execution Space vs Project Binding separation, the cwd precedence, and
> the never-silently-migrate rule remain current authority. Read every Company
> Store passage below as decision-time history, never as a current contract.

```text
status: partially superseded (DOC-108); was: accepted
date: 2026-07-28
tracks: https://github.com/cyl19970726/multi-agent-harness/issues/252
supersedes: repo-derived legacy Company OS Store ownership implied by ADR 0033 docs
current_authority: docs/mental/agent-firm-mental-model.md
```

ADR 0056 supersedes the historical PendingInteraction item in the coordination
inventory. Correlated Messages now carry provider questions and replies.

## Context

The current multi-project implementation resolves one `ProjectContext` from a
repository or path. That context carries both:

- `project_root`, the workspace where providers run and discover project
  instructions; and
- `store_root`, the JSONL store where Mission/Wave, Agent Team, Workflow, and
  legacy Company OS records were written.

That coupling is useful for repository execution and compatibility, but it is
not the right product boundary for an Agent-native company.

An Agent Company can contain multiple operating areas and software sources:

```text
Agent Company Workspace          # legacy shape; retired by DOC-108
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
remain usable by people who never initialize the legacy Company OS.

## Decision

Introduce three independent native identities, connected only by explicit
relations:

```text
Company Store (retired)   Execution Space       Project Binding
     \                          |                    /
      \------- explicit, optional relations --------/
```

### Company Store (retired by DOC-108)

At decision time the legacy Company Store owned durable company operating truth:

- `Document`, `Block`, `TypedRecord`, `Relation`, `View`, `BusinessModule`;
- Organization actors, OrgUnits, memberships, authority, and permissions;
- `WorkItem`, `Milestone`, `Assignment`, `Approval`, and result routing;
- financial commitments, invoices, payments, refunds, evidence, and metrics;
- company governance records and operating-area structure.

It does not own Mission/Wave lifecycle, Team messages, provider sessions,
runtime process handles, repository roots, worktrees, or source-control state.

Product language presented this as an **Agent Company Workspace** — legacy
naming. That former technical boundary was `Company Store`.

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
to a legacy Company Store, but standalone execution remains first-class.

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

## Implemented boundary

All three selectors now exist independently:

- `harness company ...`, `--company`, and `HARNESS_COMPANY` selected Company
  Store truth (that selector is retired by DOC-108);
- `harness space ...`, `--space`, and `HARNESS_SPACE` select Mission/Wave,
  Agent Team, and Workflow coordination truth;
- `harness project ...`, `--project`, and `HARNESS_PROJECT` select the Project
  Binding used for provider cwd, repository instructions, Skills,
  Git/worktree, and permission boundaries.

`ProjectContext { id, project_root, store_root, kind, is_git_repo }` remains an
internal compatibility adapter while Project Binding metadata is extracted
from it. Its `store_root` is labelled `compatibility_store_root` in public
projections and does not own new execution rows when an Execution Space is
selected.

`harness init` is the low-friction compatibility entry: it registers the
current repository as a Project Binding and, when no prior execution history
would be shadowed, creates a repo-derived Execution Space. It never creates a
legacy Company Store.

Existing project-derived stores are not silently reinterpreted, dual-written,
or deleted. `harness space migrate-from-project` performs an explicit,
copy-only, byte-verified migration of active execution ledgers and whitelisted
execution-evidence files, and leaves the source intact with a rollback command
in the migration manifest.

When no legacy Company Store was selected, `harness company ...` retained a narrow
compatibility fallback to the selected Project Binding's old `company_os_*`
ledgers. It never writes Company truth into an Execution Space.

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
│   │   ├── teams.jsonl
│   │   ├── team_runs.jsonl
│   │   ├── member_runs.jsonl
│   │   └── team_messages.jsonl
│   └── <standalone-space-id>/
│
└── projects/
    ├── registry.json
    ├── multi-agent-harness/metadata.json
    └── wanchengwanling/metadata.json
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
Project Binding. It must not implicitly create a legacy Company Store.

Retired Company OS commands resolved an explicit or selected Company Store:

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

## Migration status

1. **Freeze boundaries — implemented.** Canonical docs and native identities
   distinguish the legacy Company Store, Execution Space, and Project Binding.
2. **Company Store v1 — implemented slice, since retired (DOC-108).** Registry,
   CLI/API routing, Dashboard selector, and guarded Company-row migration
   existed; all of it is now export/verify-only history.
3. **Project Binding — implemented slice.** Git/source metadata and discovery,
   worktree, and permission boundaries are projected independently from Store
   ownership. `ProjectContext` remains internal compatibility infrastructure.
4. **Execution Space — implemented slice.** Registry, active marker,
   CLI/API/Dashboard selectors, and Mission/Wave/Agent Team/Workflow routing
   exist. `AgentTeamRun` and `WorkflowRun` pin `project_binding_id`.
5. **Migration and broader cleanup — in progress.** Explicit execution
   migration is copy-only and verified; provider-native sessions are never
   copied. Old project-derived compatibility stores remain readable until
   governed retirement is separately approved.

Do not silently dual-write. Migration must be explicit, latest-wins safe, and
reconstructable.

## Acceptance

1. Standalone execution can create Agent Team/Workflow records (and, at
   decision time, the since-retired Mission/Wave records) without any
   legacy Company Store.
2. A legacy Company Store could contain Wanchengwanling and AgentOS operating
   areas in one retired company truth boundary while mapping external
   repositories.
3. At decision time, one legacy Mission could use multiple Project Bindings
   through separate TeamRuns or MemberRuns without duplicating its history;
   the same multi-binding rule now applies to durable Teams.
4. Switching Company never silently switches Execution Space; switching
   Execution Space never rewrites Company truth; selecting Project Binding never
   reroutes Company writes.
5. Provider cwd is always a project root or validated worktree, never the
   retired Company Store or an Execution Space directory.
6. Execution completion or Wave advance cannot approve finance, legal,
   permission, or Organization changes.

## Consequences

- `ProjectContext` is compatibility infrastructure, not an ownership object.
- `--project` remains first-class as an execution-resource selector; only its
  former Store-selection meaning is compatibility behavior.
- Wanchengwanling and AgentOS were expected to live as operating areas in one
  Agent Company Workspace — a legacy shape retired by DOC-108 — with their
  GitHub repositories mapped as external source and delivery systems.

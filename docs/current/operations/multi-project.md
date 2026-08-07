# Execution Spaces and Project Bindings

```text
status: implemented
canonical_boundary: ADR 0042
```

Star Harness separates coordination truth from the directory in which an
Agent executes:

```text
Execution Space                    Project Binding
Mission / Wave                     provider cwd
Agent Team / TeamRun / MemberRun   AGENTS.md / CLAUDE.md / config
TeamMessage / PendingInteraction   project-local Skills
WorkflowRun / WorkflowStep         Git / worktree / permission boundary
```

A Company Store is a third, independent identity for Docs, Organization, Work,
Finance, and governance. Execution does not require a Company.

## Core invariants

1. `--space` selects coordination storage.
2. `--project` selects provider execution context.
3. Selecting a Project Binding never moves or switches Mission/Wave, Agent
   Team, or Workflow rows.
4. Selecting an Execution Space never changes Company truth.
5. Provider cwd is never a Company Store or Execution Space directory.
6. Provider-native sessions remain the sole transcript/tool/turn truth and are
   referenced rather than copied.
7. The latest Team Supervisor generation and typed mailbox route records live
   in the Execution Space; Project Binding changes never transfer live control
   ownership or message provenance.

## Physical layout

```text
~/.harness/
├── execution-spaces/
│   ├── registry.json
│   └── <space-id>/
│       ├── metadata.json
│       ├── missions.jsonl
│       ├── waves.jsonl
│       ├── teams.jsonl
│       ├── team_runs.jsonl
│       ├── member_runs.jsonl
│       ├── works.jsonl
│       ├── work_events.jsonl
│       ├── work_deliveries.jsonl
│       ├── team_messages.jsonl
│       ├── team_supervisor_leases.jsonl
│       ├── agent_message_routes.jsonl
│       ├── workflow_runs.jsonl
│       └── workflow_steps.jsonl
├── projects/
│   ├── registry.json
│   └── <binding-id>/
│       └── metadata.json          # compatibility locator, not new truth owner
├── companies/
│   ├── registry.json
│   └── <company-id>/company_os_*.jsonl
├── ACTIVE_SPACE
├── ACTIVE_PROJECT
└── ACTIVE_COMPANY
```

Logical separation is mandatory even if a deployment later co-locates some
physical files.

## Execution Space

An Execution Space is a provider-neutral coordination namespace:

```bash
harness space init \
  --id company-dev \
  --name "Company Development" \
  --project-binding multi-agent-harness

harness space list
harness space current
harness space show company-dev
harness space switch company-dev
```

The optional default Project Binding is a convenience for provider execution;
it does not transfer ownership. A command can override it:

```bash
harness --space company-dev --project another-repo mission list
harness --space company-dev --project another-repo team-run create ...
```

The Mission remains in `company-dev`; only the new TeamRun's execution binding
is `another-repo`.

## Project Binding

A Project Binding describes an execution resource:

- stable id and canonical `project_root`;
- repository URL, default branch, and Git common directory when available;
- project instruction and Skill discovery boundary;
- worktree policy and permission policy.

Commands:

```bash
harness project add [<path>] [--switch]
harness project list
harness project current
harness project show [<id|path>]
harness project switch <id|path>
harness project remove <id> [--force]
```

`harness project switch` changes the default Project Binding only. It does not
switch the active Execution Space or Company Store.

### Provider cwd precedence

```text
MemberRun.worktree_ref
  > AgentTeamRun.execution_root
  > ProjectBinding.project_root
```

An override must be the Project Binding root or a Git worktree with the same
canonical Git common directory. External Codex worktrees are therefore valid;
an unrelated directory is rejected.

`AgentTeamRun.project_binding_id` and `WorkflowRun.project_binding_id` pin the
binding used at creation. Later UI or CLI selection changes do not retarget a
running or resumed execution. If the pinned binding is unavailable, Harness
fails explicitly rather than falling back to the coordination store or current
cwd.

### Instructions and Skills

Changing cwd can change which instructions, Skills, plugins, and MCP
configuration a provider discovers. Harness therefore treats Project Binding
as an execution and permission boundary.

Harness records only non-secret facts:

- effective cwd;
- Project Binding id and resolution source;
- Git head/branch;
- directories in which instruction or Skill markers were discovered.

Project-local discovery stops at the selected Git worktree root or Project
Binding root. Global provider locations such as `~/.agents/skills` or
`~/.codex/skills` are reported separately. Harness does not persist Skill
contents, provider transcript, tool stream, credentials, or private thinking.

Provider processes receive:

```text
HARNESS_SPACE
HARNESS_PROJECT_ID
HARNESS_PROJECT
HARNESS_TEAM_RUN_ID
HARNESS_MEMBER_RUN_ID
HARNESS_WORK_ID
HARNESS_WORK_VERSION
HARNESS_BIN
```

`HARNESS_PROJECT_ID` is the stable binding id. `HARNESS_PROJECT` is an
executable selector, normally the canonical project root.
Conversation correlation belongs to an actual TeamMessage envelope; it is not
a process-wide responsibility variable.

## Selection precedence

Coordination store:

1. raw `--store`, workflow-child store, or `HARNESS_ROOT` compatibility
   override;
2. `--space`;
3. `HARNESS_SPACE`;
4. active `ACTIVE_SPACE`;
5. project-derived compatibility store only when no Execution Space exists;
6. legacy repo-local `.harness`, then the active/global compatibility project.

Project Binding is resolved independently:

1. `--project`;
2. `HARNESS_PROJECT`;
3. selected Execution Space's `default_project_binding_id`;
4. active `ACTIVE_PROJECT`.

The reserved `_global` binding points at `~/`. Read-only work is allowed there.
Writable or worktree-isolated work is rejected because it is normally not a
Git repository and cannot produce diff evidence.

## Low-friction initialization

```bash
cd <repository>
harness init
```

`init` registers the repository as a Project Binding. If no active Execution
Space exists and doing so would not shadow existing execution rows, it also
creates a repo-derived Execution Space with that binding as its default.

`init` never creates a Company Store and never silently migrates an old
project-derived Store.

## Explicit migration

Legacy project-derived execution rows can be copied into a native Execution
Space:

```bash
harness space migrate-from-project \
  --from-project <binding-id-or-path> \
  --id <space-id> \
  --name <display-name>
```

The migration:

- copies only active execution/coordination ledgers plus checks, compiled
  workflow data, and workflow patches;
- excludes `company_os_*`, provider session directories, and runtime process
  payloads;
- byte-verifies every copied ledger and whitelisted execution-evidence file;
- leaves the source intact;
- writes `execution_space_migration.json` with counts, exclusions, the prior
  active space, and a rollback command;
- never dual-writes.

Company records use the separate guarded `harness company
migrate-from-project` path.

## Dashboard and HTTP

One `harness serve` exposes independent selectors:

```text
GET  /v1/spaces
GET  /v1/spaces/current
POST /v1/spaces/switch

GET  /v1/projects
GET  /v1/projects/current
POST /v1/projects/switch
```

`?space=<id>` selects coordination snapshot/SSE data.
`?project=<id>` selects provider execution/source context.
`?company=<id>` selects Company OS truth.

The Dashboard TopBar shows separate **Execution Space**, **Project Binding**,
and **Company Store** controls. Member and Workflow native-activity reads carry
both space and project selectors so switching spaces cannot display another
space's execution object.

## Compatibility boundary

`ProjectContext { id, project_root, store_root, kind, is_git_repo }` remains an
internal adapter for legacy project-derived stores and existing spawn code.
Public Project projections label its old store path
`compatibility_store_root` and `owns_execution_store: false`.

Old repo-local or project-derived stores remain readable until an explicit,
verified migration and later governed retirement. They are not silently
rewritten or deleted.

Until a Company Store is selected, `harness company ...` alone may read and
write the selected Project Binding's legacy `company_os_*` compatibility
ledgers. It never falls through into the active Execution Space. Selecting a
Company Store removes that fallback.

## Verification

The deterministic suite uses isolated HOME directories and fake providers:

```bash
pnpm test:multi-project
cargo test -p harness-cli --test execution_space_cli
cargo test -p harness-cli --test team_run_start
cargo test -p harness-cli --test workflow_cwd
pnpm check:dashboard
```

It proves selector independence, store isolation, pinned provider cwd,
worktree policy, provider collaboration environment, migration exclusions,
SSE routing, and Dashboard type/build integrity. A real-provider claim still
requires a provider-native live run.

# Execution Spaces and Project Bindings

```text
status: implemented
canonical_boundary: ADR 0042
```

Star Harness separates coordination truth from the directory in which an
Agent executes:

```text
Execution Space                    Project Binding
Agent Team / TeamRun / MemberRun   provider cwd
Message / correlated reply         AGENTS.md / CLAUDE.md / config
legacy Workflow records (archive)  historical Project Binding metadata
legacy Mission/Wave (read-only)    Git / worktree / permission boundary
```

The retired Company Store was a third, independent identity for the legacy
Docs/Organization/Finance layer (DOC-108). Its writers and reads are closed;
its stores remain only as export/verify sources for
`firm legacy-company-os export|verify`.

## Core invariants

1. `--space` selects coordination storage.
2. `--project` selects provider execution context.
3. Selecting a Project Binding never moves or switches Agent Team or Message
   rows, and never reactivates historical Workflow archive records.
4. Selecting an Execution Space never changes legacy Company truth.
5. Provider cwd is never a legacy Company Store or Execution Space directory.
6. Provider-native sessions remain the sole transcript/tool/turn truth and are
   referenced rather than copied.
7. The latest Team Supervisor generation, MessageSubscription policies and
   per-recipient CanonicalMessageDelivery records live in the Execution Space;
   Project Binding changes never transfer live control ownership or Message
   provenance.

## Physical layout

```text
~/.firm/
├── execution-spaces/
│   ├── registry.json
│   └── <space-id>/
│       ├── metadata.json
│       ├── missions.jsonl         # DOC-108 legacy read/export only
│       ├── waves.jsonl            # ADR-0051-predecessor Legacy read/export only
│       ├── teams.jsonl
│       ├── team_runs.jsonl
│       ├── member_runs.jsonl
│       ├── works.jsonl
│       ├── work_events.jsonl
│       ├── work_deliveries.jsonl
│       ├── agentfirm_trust_operations.jsonl  # current Message fabric projections
│       ├── team_messages.jsonl                # Legacy read/export only
│       ├── team_supervisor_leases.jsonl
│       └── legacy-archive/        # lossless export/verify/restore-read only
├── projects/
│   ├── registry.json
│   └── <binding-id>/
│       └── metadata.json          # compatibility locator, not new truth owner
├── companies/                   # DOC-108 retired; export/verify sources only
│   ├── registry.json
│   └── <company-id>/company_os_*.jsonl
├── ACTIVE_SPACE
└── ACTIVE_PROJECT
```

Logical separation is mandatory even if a deployment later co-locates some
physical files.

## Execution Space

An Execution Space is a provider-neutral coordination namespace:

```bash
firm space init \
  --id company-dev \
  --name "Company Development" \
  --project-binding multi-agent-harness

firm space list
firm space current
firm space show company-dev
firm space switch company-dev
```

The optional default Project Binding is a convenience for provider execution;
it does not transfer ownership. A command can override it:

```bash
harness --space company-dev --project another-repo team-run create ...
```

The Team and its runs remain in `company-dev`; only the new TeamRun's
execution binding is `another-repo`.

## Project Binding

A Project Binding describes an execution resource:

- stable id and canonical `project_root`;
- repository URL, default branch, and Git common directory when available;
- project instruction and Skill discovery boundary;
- worktree policy and permission policy.

Commands:

```bash
firm project add [<path>] [--switch]
firm project list
firm project current
firm project show [<id|path>]
firm project switch <id|path>
firm project remove <id> [--force]
```

`firm project switch` changes the default Project Binding only. It does not
switch the active Execution Space.

### Provider cwd precedence

```text
MemberWorkspaceBinding.canonical_root
  > AgentTeamRun.execution_root
  > ProjectBinding.project_root
```

The Host explicitly assigns the binding when it creates the MemberRun/session;
Harness validates and freezes it but does not allocate worktrees or impose cwd
exclusivity. A binding must be the Project Binding root or a Git worktree with
the same canonical Git common directory. Shared canonical cwd is allowed; an
unrelated directory is rejected.

`AgentTeamRun.project_binding_id` pins the binding used at creation. Historical
Workflow binding fields remain archive evidence and cannot resume execution.
Later UI or CLI selection changes do not retarget a running Agent Team. If the pinned binding is unavailable, Harness
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
Conversation correlation belongs to an actual immutable Message envelope; it is not
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
firm init
```

`init` registers the repository as a Project Binding. If no active Execution
Space exists and doing so would not shadow existing execution rows, it also
creates a repo-derived Execution Space with that binding as its default.

`init` never creates a Company Store (the Company layer is retired by
DOC-108) and never silently migrates an old project-derived Store.

## Explicit migration

Legacy project-derived execution rows can be copied into a native Execution
Space:

```bash
firm space migrate-from-project \
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

Legacy Company records are export/verify-only through
`firm legacy-company-os export|verify`; there is no Company migration writer.

## Dashboard and HTTP

One `firm serve` exposes independent selectors:

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

The Dashboard TopBar shows separate **Execution Space** and **Project
Binding** controls. AgentWorkspace provider history resolves its
Session from the selected Execution Space and its source from the server-owned
Project Binding. Private live SSE additionally binds the authenticated exact
AgentMember owner, so switching spaces or selecting another Member cannot
display another execution object's provider data.

## Compatibility boundary

`ProjectContext { id, project_root, store_root, kind, is_git_repo }` remains an
internal adapter for legacy project-derived stores and existing spawn code.
Public Project projections label its old store path
`compatibility_store_root` and `owns_execution_store: false`.

Old repo-local or project-derived stores remain readable until an explicit,
verified migration and later governed retirement. They are not silently
rewritten or deleted.

Legacy `company_os_*` ledgers in any store are export/verify-only history
(DOC-108); nothing reads or writes them on a current path.

## Verification

The deterministic suite uses isolated HOME directories and fake providers:

```bash
pnpm test:multi-project
cargo test -p firm-cli --test execution_space_cli
cargo test -p firm-cli --test team_run_api --test team_run_daemon
cargo test -p firm-cli --test workflow_cwd
pnpm check:dashboard
```

It proves selector independence, store isolation, pinned provider cwd,
worktree policy, provider collaboration environment, migration exclusions,
SSE routing, and Dashboard type/build integrity. A real-provider claim still
requires a provider-native live run.

# Multi-Project Harness

One operator (and one `serve` / dashboard) can manage **many** Workspaces — each
with its own Mission/Wave coordination and execution records — plus a reserved
**GLOBAL** Workspace rooted at `~/`. This is the operator-facing reference for
the layout, Workspace commands, GLOBAL policy, migration, and live acceptance.
The architectural rationale for Agent Team execution paths is durable in
[ADR 0033](decisions/0033-agent-team-workspace-contract.md), not in a retired
Goal ledger.

## Workspace contract: four distinct paths

Project selection and Agent Team execution use four deliberately distinct
values (ADR 0033):

- **`store_root`** = `~/.harness/projects/<id>/` — the centralized, repo-independent
  JSONL coordination ledgers and locks. Sibling `harness` processes (a
  `serve` and a `run-script` from different cwds) converge here via the registry's
  `current_project_id`, preserving the issue #89 single-store invariant.
- **`project_root`** = the git repo (or `~/` for GLOBAL) — where `CLAUDE.md`,
  `AGENTS.md`, and project configuration live. It is the registered Workspace
  identity, not necessarily every member's cwd.
- **`AgentTeamRun.execution_root`** = the run-level provider cwd, defaulting to
  `project_root`. An explicit override must be `project_root` or a Git worktree
  sharing its Git common directory.
- **`MemberRun.worktree_ref`** = an optional member-specific override with the
  same validation. Provider spawn precedence is `worktree_ref` >
  `execution_root` > `project_root`; `store_root` is never a provider cwd.

These are bundled into a `ProjectContext { id, project_root, store_root, kind,
is_git_repo }` (in `harness-core`) that is threaded through every spawn site
instead of reading the harness process `env::current_dir()`.

### Worktrees share repository identity, not path containment

Harness-created Dynamic Workflow worktrees remain under `project_root` by
convention:

```
<project_root>/.harness/worktrees/<run_id>-<slug>-<unique>
```

Git itself also permits linked worktrees elsewhere. Agent Team overrides may
therefore point to external Codex worktrees; Harness validates the candidate is
the worktree top level and shares the selected project's canonical Git common
directory. A simple `starts_with(project_root)` check is incorrect.

### Layout

```
~/.harness/
  projects/
    _global/                  # reserved id for ~ (HOME); usually NOT a git repo
      missions.jsonl waves.jsonl members.jsonl messages.jsonl ...
      runtimes/  metadata.json
    ai-luodi-jyx3d/           # under $HOME → slug = relpath with '/'→'-'
    proj-<sha256[:16]>/       # outside $HOME → content-addressed
    registry.json             # {current_project_id, projects:[...]}
  ACTIVE_PROJECT              # single-line current id (also in the registry)
```

## Project identity

The id is derived from the **canonicalized absolute path** (`realpath`, so
symlinks / `~` vs `/Users/...` normalize to one id):

- `path == $HOME` → `_global` (reserved, hardcoded).
- under `$HOME` → relpath slug, `/`→`-` (e.g. `~/ai-luodi/jyx3d` → `ai-luodi-jyx3d`).
- outside `$HOME` → `proj-<sha256(canonical_path)[:16]>` (stable, content-addressed).

## Project resolution precedence

A store root is resolved by this precedence (highest first):

1. `--store <path>` / `HARNESS_ROOT` — back-compat overrides (deprecation-warned).
2. `--project <id|path>` — explicit selection.
3. `HARNESS_PROJECT` env — explicit selection.
4. registry `current_project_id` / `ACTIVE_PROJECT` — the active project.
5. cwd walk-up to the nearest `.harness/`, mapped to its central id
   (legacy, deprecation-warned).
6. `_global`.

`--store-source` prints which store was chosen and why (the dual-read decision is
always logged, never silent):

```bash
harness --store-source goal list
# store-source: central store ...; root=/Users/me/.harness/projects/<id>
```

## Project commands

```bash
harness init                          # register + activate the project rooted at cwd
harness --project <path> init         # register + activate a project by path
harness project add [<path>] [--switch]  # register a project (default cwd) WITHOUT switching unless --switch
harness project list                  # enumerate registered projects + _global
harness project current               # print the currently-active project context
harness project show [<id|path>]      # metadata for the (selected) project; no arg = current
harness project switch <id|path>      # flip ACTIVE_PROJECT + registry current
harness project migrate               # centralize a repo-local .harness (see below)
harness project remove <id> [--force] # drop a registration (_global is protected)
```

The dashboard exposes the same surface over HTTP: `GET /v1/projects`,
`GET /v1/projects/current`, `POST /v1/projects/switch`, and a `?project=<id>`
parameter on `/v1/snapshot` and `/v1/events`. A header picker re-points the
scoped read model + SSE stream on switch and persists the choice to
`?project=<id>` + `localStorage`.

## Migration path (repo-local `.harness` → central store)

Existing repos have a repo-local `.harness/` store. `harness project migrate`
(run from the repo) **copies** (never moves) active JSONL ledgers plus
`prompts/` and `runtimes/` into `~/.harness/projects/<id>/`, writes `metadata.json` with
`migrated_from`, and drops a `.harness/MIGRATED_TO_CENTRAL` marker in the old
store (tooling then reads the central store and ignores the marked local one).
The retired provider-session mirror ledger/directory is intentionally omitted.

- **No data loss**: it is a copy; `records_after == records_before`, and the old
  store is left intact (only marked).
- **Non-destructive to the active project**: migrate does *not* switch — use
  `harness project switch <id>` to activate the central store.
- **Idempotent**: re-running is a no-op once the marker exists.

During the grace period resolution tries the central store first and falls back
to an *unmarked* local `.harness` only if no central store exists, always logging
the choice (`--store-source`).

## GLOBAL `_global` (`~/`) project — non-git limitations

`_global` is rooted at `~/` and is normally **not a git repo**. Because a writable
/ `isolation="worktree"` node needs a git worktree that cannot exist there:

- Read-only (`isolation="none"`) workflow nodes and read-only deliveries run
  everywhere, including `_global`.
- A `writable` / `isolation="worktree"` node against `_global` (or any non-git
  project) is **rejected before any provider spawn** with an actionable message
  naming the project and offering the read-only fix (run read-only and fetch the
  output with `harness workflow get-output <run_id> --step <label>`, or use a
  git-backed project).
- **Diff evidence is unavailable** for `_global` (no worktree to diff) — accepted.

## Provider activity

Provider activity is read from each provider's native session and is not a
per-project Harness ledger. Restarting `serve` therefore cannot truncate or
silently rewrite provider history; only ephemeral UI projections reconnect.

## Live acceptance

The deterministic multi-project section of the verification script creates two
projects + `_global` under an isolated `HOME` (never the developer's real
`~/.harness`), runs **one** `serve`, and asserts per-project store routing +
isolation, a workflow leaf rooted in project A, a persistent member delivery in
project B, the GLOBAL policy, and migration — all with fake provider shims (no
codex/claude, no network):

```bash
scripts/verify-fixes.sh          # deterministic tier (includes the multi-project demo)
pnpm acceptance:multi-project    # same, via package.json

pnpm test:multi-project          # just the multi-project deterministic Rust suite
```

The fake provider shims live in `scripts/multi-project-demo/`. The real
live-codex multi-project demo (writing into a real project tree) is run by the
operator separately with `scripts/verify-fixes.sh --real`.

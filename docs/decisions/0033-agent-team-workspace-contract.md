# ADR 0033: Agent Team Workspace Contract

Status: active, implemented

## Context

Centralized Harness storage, registered project identity, provider launch cwd,
and member worktrees were previously easy to conflate. That can write correct
coordination rows while a provider loads the wrong project instructions. Path
containment is also insufficient because valid Git/Codex worktrees may live
outside the registered repository path.

## Decision

Keep four explicit values:

- `ProjectContext.store_root`: centralized Harness coordination storage only;
- `ProjectContext.project_root`: registered Workspace identity;
- `AgentTeamRun.execution_root`: run-level cwd, defaulting to `project_root`;
- `MemberRun.worktree_ref`: optional member-level cwd override.

New CLI, HTTP, and MCP creation accepts the latter two overrides. With a
registered project, each override must be the canonical project root or a Git
worktree top level whose canonical Git common directory matches the project.
Provider spawn resolves `worktree_ref > execution_root > project_root` and
never falls back to `store_root`.

The provider collaboration environment preserves the same separation:

- `HARNESS_PROJECT_ID` carries stable Workspace identity;
- `HARNESS_PROJECT` carries an executable selector (normally canonical
  `project_root`) so a nested provider process can resolve the same store even
  when its cwd is an unregistered linked worktree;
- `HARNESS_BIN` carries the exact Host executable so Member CLI calls cannot
  drift to an older binary on `PATH`.

An in-memory `serve` context created from an unregistered worktree retains its
exact `project_root` and `store_root`. Project enumeration must not reconstruct
that context by treating `store_root` as a repository root.

Immediately before spawn, Harness records `MemberRun.workspace_snapshot` with
the actual canonical cwd, Git HEAD/branch when available, and non-secret
discovered instruction/skill directory paths. It does not copy file contents,
configuration values, credentials, environment dumps, provider transcript,
tool stream, or thinking. The Dashboard projects these fields directly.

All new fields are optional on read so historical JSONL remains valid.

## Consequences

- Moving the centralized store cannot change provider context.
- External linked worktrees are supported without weakening repository
  identity validation.
- Operators can compare requested and actual launch workspace facts.
- Raw-store compatibility writes snapshot their creation cwd because no
  registered project identity exists; raw-store use remains deprecated.

## Validation

- Core/store serde and schema fixtures prove sparse-row compatibility and the
  privacy boundary.
- CLI/API/MCP tests prove create-surface round trips and spawn precedence.
- Git fixture tests accept an external same-common-dir worktree and reject an
  unrelated directory.
- Dashboard fixture checks render each distinct value and reject prohibited
  persisted keys.

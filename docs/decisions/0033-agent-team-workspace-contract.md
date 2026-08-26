# ADR 0033: Agent Team Workspace Contract

> Work-graph amendment (ADR 0058): Harness still does not schedule Git or
> worktree steps. That boundary does not prohibit kernel-owned Work dependency
> edges; Work ordering and workspace mechanics are independent planes.

Status: active, implemented

## Context

Centralized Harness storage, registered project identity, provider launch cwd,
and member worktrees were previously easy to conflate. That can write correct
coordination rows while a provider loads the wrong project instructions. Path
containment is also insufficient because valid Git/Codex worktrees may live
outside the registered repository path.

## Decision

Keep four explicit values:

- `ExecutionSpace.store_root`: Harness coordination storage only;
- `ProjectBinding.project_root`: registered execution-resource identity;
- `AgentTeamRun.execution_root`: run-level cwd, defaulting to `project_root`;
- `MemberWorkspaceBinding.canonical_root`: optional member-run workspace
  binding and highest-precedence provider cwd.

Host/operator creation surfaces accept the latter two explicit bindings;
managed agents use authenticated `firm` CLI Role Actions and receive no
Harness MCP mutation surface. With a registered project, each override must be
the canonical project root or a Git worktree top level whose canonical Git
common directory matches the project.
Provider spawn resolves `MemberWorkspaceBinding.canonical_root > execution_root > project_root` and
never falls back to an Execution Space or the legacy Company Store. The internal
`ProjectContext` adapter may still carry a compatibility store locator, but it
does not own native coordination writes.

The provider collaboration environment preserves the same separation:

- `HARNESS_PROJECT_ID` carries stable Workspace identity;
- `HARNESS_PROJECT` carries an executable selector (normally canonical
  `project_root`) so a nested provider process resolves the same execution
  boundary even when its cwd is an unregistered linked worktree;
- `HARNESS_SPACE` carries the coordination namespace independently;
- `HARNESS_BIN` carries the exact Host executable so Member CLI calls cannot
  drift to an older binary on `PATH`.

An in-memory `serve` context created from an unregistered worktree retains its
exact `project_root`. Project enumeration must not reconstruct that context by
treating any coordination Store as a repository root.

Immediately before spawn, Harness records `MemberRun.workspace_snapshot` with
the actual canonical cwd, Git HEAD/branch when available, and non-secret
discovered instruction/skill directory paths. It does not copy file contents,
configuration values, credentials, environment dumps, provider transcript,
tool stream, or thinking. The Dashboard projects these fields directly.

Historical sparse records remain readable, but they do not create a current
workspace binding or authorize a fallback cwd.

The Host explicitly assigns each MemberRun a project root, existing cwd, or
worktree when it creates the Team/member/session. The Host may delegate the Git
mechanics to a trusted development Member, but the resulting canonical root
must still be explicitly bound before launch. Harness validates and freezes the
assignment; it does not allocate worktrees, enforce cwd exclusivity, or create
a scheduler or Task Graph.

## Consequences

- Moving the centralized store cannot change provider context.
- External linked worktrees are supported without weakening repository
  identity validation.
- Host-assigned cwd/worktree placement keeps responsibility and conflict
  boundaries explicit while allowing shared cwd or optional isolation.
- Operators can compare requested and actual launch workspace facts.
- Missing current workspace authority fails closed instead of deriving a
  provider cwd from a legacy store location.

## Validation

- Core/store serde and schema fixtures prove sparse-row compatibility and the
  privacy boundary.
- CLI/API/MCP tests prove create-surface round trips and spawn precedence.
- Git fixture tests accept an external same-common-dir worktree and reject an
  unrelated directory.
- Dashboard fixture checks render each distinct value and reject prohibited
  persisted keys.

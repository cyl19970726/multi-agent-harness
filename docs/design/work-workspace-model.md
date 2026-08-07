# Work Workspace Model

```text
status: planned
owner_role: execution-foundation
canonical_for: Work workspace declaration, MemberRun cwd derivation, workspace lifecycle
authorityClass: design_intent
implementationState: design_only
truthRefs:
  - kind: schema
    ref: schemas/work.schema.json
  - kind: store
    ref: crates/firm-store/src/lib.rs
  - kind: decision
    ref: docs/decisions/0050-agent-team-work-board-and-message-boundary.md
dependsOn:
  - docs/concept-model.md
  - docs/architecture.md
  - docs/design/task-gate-contracts.md
machineConsumers: []
reviewAfter: 2026-09-08
reorgTrigger: Work workspace, MemberRun cwd derivation, or workspace lifecycle no longer match implemented surfaces.
```

## Problem

Today a Work tells a Member WHAT to do (`context_markdown`, `completion_criteria_markdown`) and HOW to submit (`gates`), but never WHERE to work. The Member must deduce its working directory from prose inside `context_markdown` or from skill-level conventions:

```
"Create clean worktree: ../multi-agent-harness-<task>"
```

This is fragile. When two parallel Works both need to touch the same repository, they silently collide. When a `github-pr` gate checks for a merged PR, it has no structured link to the branch or worktree that produced it. When a Member restarts mid-Work, no durable record says which directory it was in.

## Design

### WorkWorkspace

A new struct on `Work` that declares WHERE the work happens:

```rust
/// Where a Work executes. The harness manages lifecycle: creates the
/// workspace before the first member start, injects it as cwd, and
/// cleans up on Work completion or cancellation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkWorkspace {
    /// "worktree" | "dir" | "inherit"
    pub kind: WorkWorkspaceKind,

    /// Absolute or project-relative path. For worktrees, this is
    /// OUTSIDE the main repository (e.g. "../repo-feat-login").
    pub path: String,

    /// For worktrees: the base ref to branch from (e.g. "origin/master").
    /// Defaults to the project's default branch when omitted.
    #[serde(default)]
    pub base_ref: Option<String>,

    /// Whether the workspace should be removed after Work completes.
    /// Defaults to true for worktrees, false for dirs.
    #[serde(default = "default_auto_cleanup")]
    pub auto_cleanup: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkWorkspaceKind {
    /// A git worktree: isolated checkout, its own branch.
    /// `github-pr` gate can verify the branch → PR mapping.
    Worktree,
    /// A plain directory (no git isolation). For exploration,
    /// research, or single-file documentation work.
    Dir,
    /// The project root. For read-only analysis or ops work
    /// that doesn't need isolation. Member's cwd is the project
    /// root as resolved by the selected Project Binding.
    Inherit,
}

fn default_auto_cleanup() -> bool { true }
```

### Integration points

**On Work:**

```rust
pub struct Work {
    // ... existing fields ...

    /// Where this Work executes. `None` → Member inherits the project
    /// root (back-compat with today's implicit behaviour).
    #[serde(default)]
    pub workspace: Option<WorkWorkspace>,
}
```

**On MemberRun** (existing fields, clarified semantics):

```rust
pub struct MemberRun {
    // Existing — now populated from Work.workspace when starting work:
    pub worktree_ref: Option<String>,       // WorkWorkspace.path for worktrees
    pub workspace_snapshot: Option<MemberWorkspaceSnapshot>,  // observed facts

    // Existing — now derived from Work.workspace + Work.gates:
    pub owned_paths: Vec<String>,
}
```

**Resolution order** (what wins when multiple sources specify a cwd):

1. `Work.workspace` (explicit per-work declaration) — strongest
2. `MemberRun.worktree_ref` (from team-run member spec `@paths`) — fallback
3. Project Binding root — default

### Lifecycle

```
Work created with workspace:
  workspace: { kind: "worktree", path: "../repo-feat-login", base_ref: "origin/master" }

  ┌─ Member claims Work ──────────────────────────────────────┐
  │ 1. Harness creates workspace:                              │
  │    git worktree add ../repo-feat-login origin/master       │
  │    → MemberRun.worktree_ref = "../repo-feat-login"         │
  │                                                            │
  │ 2. Harness starts member:                                  │
  │    cwd = ../repo-feat-login                                │
  │    LaunchSpec.workspace = "../repo-feat-login"             │
  │    LaunchSpec.writable_roots = ["../repo-feat-login"]      │
  │    → MemberWorkspaceSnapshot captured on first observe     │
  │                                                            │
  │ 3. Member works, submits                                   │
  │    → work submit attaches --github-pr, --artifact-ref etc  │
  │                                                            │
  │ 4. Work accepted / cancelled:                              │
  │    if auto_cleanup: git worktree remove ../repo-feat-login │
  └────────────────────────────────────────────────────────────┘
```

### CLI

```bash
# Code work with worktree
firm work create --title "implement login" \
  --workspace-kind worktree --workspace-path ../repo-feat-login \
  --workspace-base origin/master \
  --gate github-pr:require_merged=true

# Exploration work with plain directory
firm work create --title "research async runtime" \
  --workspace-kind dir --workspace-path ../research-runtime \
  --workspace-auto-cleanup=false

# Shorthand for worktree (most common case)
firm work create --title "implement login" \
  --worktree ../repo-feat-login

# Inherit project root (explicit, same as omitting --workspace)
firm work create --title "audit repo" \
  --workspace-kind inherit
```

### Gate integration

| Gate | Workspace usage |
|---|---|
| `github-pr` | Verifies branch at `workspace.path` matches the linked PR |
| `owned-path-check` | Validates changed paths ⊆ `owned_paths` relative to workspace |
| `artifact-exists` | Resolves artifact paths against workspace root |

### Backward compatibility

- `Work.workspace = None` → Member inherits project root (today's behaviour)
- `worktree_ref` on `MemberRun` keeps working when populated by team-run member spec
- `LaunchSpec.workspace` already exists, now populated from `Work.workspace` when present
- `MemberWorkspaceSnapshot.cwd` already records the actual cwd — unchanged

## Implementation phases

### Phase 1: Data model

- Add `WorkWorkspace`, `WorkWorkspaceKind` to `firm-core`
- Add `workspace: Option<WorkWorkspace>` to `Work`
- Update `schemas/work.schema.json`
- Add `--workspace-*` / `--worktree` flags to `work create`

### Phase 2: Lifecycle

- Workspace creation on first member claim/start (worktree add / mkdir)
- `cwd` injection into `LaunchSpec.workspace`
- Cleanup on Work completion/cancellation

### Phase 3: Gate integration

- `github-pr` gate validates branch from workspace context
- `owned-path-check` uses workspace to scope path validation
- `artifact-exists` resolves paths relative to workspace root

### Phase 4: Skills + docs

- Update `orchestrate-mission-waves` skill with workspace usage
- Update `collaborate-as-agent-team-member` skill
- Update `docs/concept-model.md`

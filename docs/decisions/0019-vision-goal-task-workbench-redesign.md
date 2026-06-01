# 0019: Vision → Goal → Task Workbench Redesign

## Status

Accepted (2026-06-01). Implementation pending across schema, read-model, and UI
work packages. This ADR fixes the contract and the decisions so they are not
re-litigated; the full struct/IA detail lives in
[../dashboard/work-board-design.md](../dashboard/work-board-design.md).

## Context

The current Vision / Goal / Task surfaces
([apps/agent-dashboard/src/surfaces/Surfaces.tsx](../../apps/agent-dashboard/src/surfaces/Surfaces.tsx))
render every object as a fully-expanded "proof wall": `GoalDocument` and
`TaskDocument` stack seven-plus sections (objective, success/acceptance,
GoalDesign, GoalEvaluation, closeout invariant, governance, proposals) open by
default, and the "Tasks" surface is a status-only lane strip with the dependency
graph still a `coming in WP5` placeholder. The product owner's verdict: the
three layers are cluttered and do not read like a clean board + detail product.

The target is Linear/Notion clarity for the **Vision → Goal → Task management
mechanism** specifically (agent/runtime surfaces are out of scope for this
redesign):

- a single board with a `Goals | Tasks` switch;
- Notion-style detail pages for Goal and Task;
- a per-Goal Task board (reached by filtering the global Task board);
- clean acceptance criteria, full descriptions, and a visible task graph.

Two constraints shape the decision:

1. The frontend is a read-model projection
   ([dashboard/frontend-architecture.md](../dashboard/frontend-architecture.md));
   most of the redesign is presentational, but a few schema fields are genuinely
   missing.
2. Every schema is `additionalProperties:false` with all properties `required`
   ([0017](0017-generic-object-model.md)). Removing an enum value or adding a
   required field breaks persisted JSONL — forbidden. Only additive-optional
   changes are allowed without a versioned schema + migration.

## Decision

### 1. Status models (additive-only at the schema layer)

Product-facing lifecycles:

| Object | Board columns (product) |
| --- | --- |
| Goal | `active` → `blocked` → `review` → `done` |
| Task | `planned` → `assigned` → `running` → `blocked` → `review` → `done` |

Schema reconciliation, to honor [0017](0017-generic-object-model.md):

- **Add** `review` and `done` to the `Goal.status` enum. Adding enum values is
  backward-compatible (old rows validate against the larger set).
- **Do NOT remove** `complete` or `archived` from the schema enums. Deleting them
  would invalidate persisted rows. Instead they are **deprecated**: new writers
  never emit them, the read model folds legacy `Goal.status=complete` into
  `done`, and `archived` (Goal and Task) is hidden from the board.
- `Task.status` already contains `archived`; it is likewise deprecated in product
  (no column, never newly written) but retained in the schema.

The Goal `review`/`done` columns **do not weaken the closeout gate**. A Goal
enters `done` only when the closeout `Decision` (`decision_kind=closeout`, with
backing evidence) plus a `GoalEvaluation` exist, or an explicit waiver — exactly
the existing invariant enforced by CLI `goal close`
([concept-model.md](../concept-model.md) §Closeout Gate). `review` means "all
tasks resolved, awaiting closeout". This preserves the anti-drift rule: a goal is
never "done" from task activity alone.

### 2. Task content = three tiers

A Task page is `meta` + `description` + `acceptance_criteria`:

- **meta** — the structured fields (status, goal, owner/assignee/reviewer ids,
  git metadata, dependencies). Rendered as page properties.
- **description** — a **new, additive-optional `description` string (markdown)**
  carrying the full task write-up. Today `Task.objective` is a one-liner and
  there is no long-form field; `objective` is retained as the card/summary line.
- **acceptance_criteria** — the existing `string[]` checklist (verifiable at
  review).

No `priority` field is added to Task (priority stays a Goal-only concept).

### 3. `git_metadata` on Goal + Task (additive-optional), not Vision

A new optional object captures git/worktree context:

```text
git_metadata = {
  repo?, worktree_path?, branch?, base_branch?, pr_ref?, commit?, owned_paths?[]
}
```

- Attached to **Goal** (long-lived integration branch) and **Task** (working
  branch / worktree / PR). **Vision does NOT get `git_metadata`** — a Vision is a
  direction whose content is its `source_refs` docs, not a unit of work with a
  worktree.
- `Task` already has required top-level `branch_ref`, `pr_ref`, `workspace_ref`,
  `owned_paths`. These **cannot be removed** ([0017](0017-generic-object-model.md)),
  so they are **retained and dual-written**: `git_metadata` is the
  forward-looking superset; the read model prefers `git_metadata.*` and falls
  back to the flat fields. A future required-field migration is the only trigger
  to drop the flat fields, and is out of scope here.

### 4. The task graph stays a derived view — no new edge field

The "task A is blocked by task B" relationship already exists as
`Task.depends_on_task_ids` (= "A waits for these"). Per
[0009](0009-task-graph-as-derived-view.md) the graph is a view over tasks and
their edges, not a competing state machine. Keep the single `depends_on` edge and
derive everything else:

- **blocks** — reverse edges (`t.depends_on_task_ids.includes(self)`);
- **ready** — every `depends_on` task is `done` and self is `planned`/`assigned`;
- **waiting** — has at least one unfinished `depends_on` task.

Critically, the stored `status=blocked` (explicit, human/agent-set) is **distinct
from** the derived `waiting` (dependency not yet `done`). A task may be `planned`
yet not executable because an upstream dependency is unfinished. The board keeps
status columns and overlays a derived `ready` / `waiting(N)` chip on each card; it
does not encode dependency-waiting as a status. No schema change.

### 5. Information architecture

- **One `Work` surface** with a `[ Goals | Tasks ]` segmented switch. Goals mode
  shows the 4-column Goal board; Tasks mode shows the 6-column Task board with a
  `goal` filter (`?goal=<id>`).
- **Goal detail** — a full Notion-style page: meta header, objective,
  success_criteria checklist, **collapsed-by-default** GoalDesign /
  GoalEvaluation / closeout sections, and a `View tasks (N)` entry that jumps to
  the Task board filtered to that goal.
- **Task detail** — a right-side slide-over (peek), expandable to a full page:
  the three tiers (meta / description / acceptance_criteria), the
  `ready`/`waiting` state, and the dependency list.
- **Vision detail** — `summary` plus the **rendered markdown of its
  `source_refs`** docs. The product operates one Vision → many Goals → many
  Tasks; no vision switcher is built yet.
- **Docs rendering is a real gap.** `model.docs` is a hard-coded catalog in
  [readModel.ts](../../apps/agent-dashboard/src/model/readModel.ts) and
  `DocsContext` only lists paths — no markdown is fetched or rendered, and no
  backend route serves doc bodies. Rendering `source_refs` (and a Docs surface)
  requires either a backend `GET /v1/docs/:path` returning markdown, or
  build-time bundling of `docs/**`. This is its own work package.

## Consequences

- **Schema**: two additive enum extensions (`Goal.status` += `review`, `done`),
  one new optional `Task.description`, one new optional `git_metadata` object on
  Goal and Task. **Zero required-field changes and zero enum deletions**, so all
  persisted JSONL and fixtures stay valid and no `schema_version` / `*.v2` file is
  needed — [0017](0017-generic-object-model.md) is upheld.
- **Deprecations (product-level only)**: `Goal.status=complete` (read as `done`),
  `archived` (Goal and Task) — hidden from the board, never newly written, but
  still schema-valid for old rows.
- **Read model**: new pure derivations (`taskGraph`, `ready`/`waiting`); lane
  builders move to the new status sets with legacy folding; a `git_metadata`
  accessor with flat-field fallback.
- **Rust core + CLI** (later WP): add `description` and `git_metadata`
  (`Option<T>` + `#[serde(default)]`), accept `review`/`done`, map producers'
  legacy `complete` → `done`; the `goal close` closeout gate is unchanged.
- **Backend**: a new docs-serving route is required for Vision/Docs rendering.
- The closeout gate and all anti-drift invariants are preserved.

## Affected Modules

- `schemas/goal.schema.json`, `schemas/task.schema.json` and their fixtures.
- `crates/harness-core` (Goal/Task types), `crates/harness-store` (readers),
  `crates/harness-cli` (producers, `goal close` gate, optional docs route).
- `apps/agent-dashboard/src/model/readModel.ts`, `surfaces/`, `types.ts`.
- `docs/dashboard/pages/{vision-overview,goal-document,task-document,graph-kanban}.md`
  are superseded in part by [../dashboard/work-board-design.md](../dashboard/work-board-design.md).

## Validation

```bash
npx pnpm@9.15.4 check   # validate:json, check:schema-fixtures, doc-governance, links
cargo test
```

- Add valid fixtures exercising `Goal.status=review|done`, `Task.description`,
  and `git_metadata` on Goal and Task; keep all existing fixtures green.
- Screenshot acceptance of the unified Work board, Goal full-page detail (with
  per-goal Task board jump), Task slide-over, and Vision doc rendering, per
  [../dashboard/acceptance.md](../dashboard/acceptance.md).

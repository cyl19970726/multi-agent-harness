# Work Board Design — Vision → Goal → Task

This document is the complete design for the **Vision → Goal → Task management
mechanism** of the Agent Workbench: the object structures (with examples), the
unified board, the detail surfaces, and the read-model derivations behind them.

The durable decision and back-compat rules live in ADR
[../decisions/0019-vision-goal-task-workbench-redesign.md](../decisions/0019-vision-goal-task-workbench-redesign.md).
Object meaning stays anchored to [../concept-model.md](../concept-model.md) and
[../data-model.md](../data-model.md); the schemas in [../../schemas/](../../schemas/)
remain the source of truth for fields. Agent/runtime surfaces are out of scope
here.

## 1. Shape

One Vision → many Goals → many Tasks. A Goal belongs to at most one Vision
(`Goal.vision_id`); a Task belongs to at most one Goal (`Task.goal_id`). The
product operates a single active Vision for now; the schema permits more.

```text
Vision ──< Goal ──< Task
  │          │         │
  summary    success   acceptance_criteria
  source_refs criteria  description (markdown)
  (docs)     GoalDesign depends_on_task_ids ──► task graph
             GoalEvaluation                   (derived: ready / waiting / blocks)
             closeout gate (Decision + Eval)
```

## 2. Structures and examples

Legend: ✓ required today · ＋ new (additive-optional, ADR 0019) · ⚠ deprecated
(kept in schema for back-compat, hidden in product).

### 2.1 Vision

`source_refs` is the narrative: it points at the docs that hold the prose
(`docs/prd.md` opens with the `## Vision` section). Vision has no worktree/git
metadata.

| Field | Type | | Notes |
| --- | --- | --- | --- |
| `id` | string | ✓ | |
| `summary` | string | ✓ | one-line direction |
| `source_refs` | string[] | ✓ | doc paths; **rendered** in Vision detail |
| `created_at` | string | ✓ | |

```json
{
  "id": "vision-1",
  "summary": "A generic, domain-neutral multi-agent harness that self-hosts its own development.",
  "source_refs": ["docs/prd.md", "docs/design-basis.md"],
  "created_at": "2026-05-26T00:00:00Z"
}
```

### 2.2 Goal

Board columns = `status` (4 product columns). `complete`/`archived` remain valid
for old rows but are deprecated (see ADR 0019 §1).

| Field | Type | | Notes |
| --- | --- | --- | --- |
| `id` `title` `objective` | string | ✓ | card + detail |
| `status` | enum | ✓ | product: `active \| blocked \| review \| done`; schema also accepts `complete`⚠/`archived`⚠ |
| `success_criteria` | string[] | ✓ | checklist — "what done looks like" |
| `priority` | string | ✓ | `p0/p1/...` card chip |
| `vision_id` | string\|null | | link to Vision |
| `goal_design_id` | string\|null | | → GoalDesign (collapsed section) |
| `closed_by_decision_id` | string\|null | | closeout readiness |
| `owner_agent_id` | string | ✓ | (agent; out of scope here) |
| `git_metadata` | object\|null | ＋ | long-lived integration branch (see §2.4) |
| `created_at` `updated_at` | string | ✓ | |

```json
{
  "id": "goal-2",
  "title": "Adopt generic object model",
  "objective": "Generalize the let-me-try coordination contract into harness core.",
  "owner_agent_id": "leader-1",
  "status": "review",
  "success_criteria": ["Additive-optional schema spine merges green"],
  "priority": "p0",
  "vision_id": "vision-1",
  "goal_design_id": "goal-design-1",
  "closed_by_decision_id": null,
  "git_metadata": { "repo": "multi-agent-harness", "branch": "feature/generic-object-model", "base_branch": "master" },
  "created_at": "2026-05-26T00:00:00Z",
  "updated_at": "2026-05-26T00:00:00Z"
}
```

Goal `done` is **gated**: it requires a closeout `Decision` + `GoalEvaluation`
(or waiver), not a column drag. `review` = tasks resolved, awaiting closeout.

### 2.3 Task

Board columns = `status` (6 product columns). Content is three tiers:
**meta** (the structured fields below) + **description** (new markdown) +
**acceptance_criteria** (checklist).

| Field | Type | | Notes |
| --- | --- | --- | --- |
| `id` `title` `objective` | string | ✓ | `objective` = one-line summary (card) |
| `description` | string | ＋ | **full write-up, markdown** (tier 2) |
| `goal_id` | string\|null | ✓ | filter dimension; per-goal board |
| `parent_task_id` | string\|null | ✓ | decomposition (phase children) |
| `status` | enum | ✓ | product: `planned \| assigned \| running \| blocked \| review \| done`; schema also accepts `archived`⚠ |
| `acceptance_criteria` | string[] | ✓ | checklist (tier 3) |
| `depends_on_task_ids` | string[] | ✓ | the **only** graph edge; drives ready/waiting/blocks |
| `phase` | string\|null | | phase label |
| `scope_refs` | string[] | | files/objects in scope |
| `requires_human_approval` | bool | | approval lock |
| `verdict_decision_id` | string\|null | | link to verdict Decision |
| `owner/assignee/reviewer_agent_id` | string\|null | ✓ | who owns / executes / reviews (agent ids) |
| `git_metadata` | object\|null | ＋ | working branch/worktree/PR (see §2.4) |
| `branch_ref` `pr_ref` `workspace_ref` `owned_paths` | | ✓ | retained flat git fields (dual-written; superseded by `git_metadata`) |
| `created_at` `updated_at` | string | ✓ | |

```json
{
  "id": "task-2",
  "goal_id": "goal-2",
  "parent_task_id": null,
  "title": "Add additive-optional fields",
  "objective": "Extend existing schemas without breaking back-compat.",
  "description": "## Context\nThe spine must stay `additionalProperties:false`.\n\n## Approach\nAdd nullable scalars and arrays; never a new required field.\n\n## Key points\n- old JSONL deserializes unchanged\n- new writers populate the keys",
  "owner_agent_id": "agent-1",
  "assignee_agent_id": "worker-1",
  "reviewer_agent_id": "evaluator-1",
  "status": "review",
  "depends_on_task_ids": ["task-1"],
  "acceptance_criteria": ["Existing fixtures stay valid", "New fields documented in schemas.md"],
  "phase": "phase-0-schema",
  "scope_refs": ["schemas/goal.schema.json", "schemas/task.schema.json"],
  "requires_human_approval": false,
  "verdict_decision_id": null,
  "git_metadata": {
    "repo": "multi-agent-harness",
    "worktree_path": ".worktrees/task-2",
    "branch": "schema/wp-a-object-spine",
    "base_branch": "master",
    "pr_ref": null,
    "commit": null,
    "owned_paths": ["schemas"]
  },
  "branch_ref": "schema/wp-a-object-spine",
  "pr_ref": null,
  "workspace_ref": ".worktrees/task-2",
  "owned_paths": ["schemas"],
  "created_at": "2026-05-26T00:00:00Z",
  "updated_at": "2026-05-26T00:00:00Z"
}
```

### 2.4 `git_metadata` (shared, additive-optional)

On **Goal** and **Task** only. The read model prefers `git_metadata.*` and falls
back to the Task flat fields (`branch_ref`/`pr_ref`/`workspace_ref`/`owned_paths`)
while both exist.

| Field | Type | Notes |
| --- | --- | --- |
| `repo` | string\|null | repository name/slug |
| `worktree_path` | string\|null | local worktree (e.g. `.worktrees/task-2`) |
| `branch` | string\|null | working / integration branch |
| `base_branch` | string\|null | branch it merges into |
| `pr_ref` | string\|null | PR URL or number |
| `commit` | string\|null | head commit |
| `owned_paths` | string[] | paths this unit may change |

### 2.5 Collapsed depth on the Goal page

- **GoalDesign** (`goal_design_id`): `scenario_summary`, `non_goals[]`,
  `risk_and_permission_boundaries`, `required_infra[]`, `task_graph[]`,
  `evidence_plan[]`, `acceptance_gates[]`.
- **GoalEvaluation**: `outcome`, `what_worked`, `what_failed`,
  `reusable_patterns[]`, `anti_patterns[]`, `follow_up_task_ids[]`.

These are the former "proof wall"; in the new IA they are collapsed by default.

## 3. Derived task graph (no schema change)

A single pure read-model function feeds the cards, the per-goal board, and the
graph view. Only `depends_on_task_ids` is stored; everything else is derived.

```ts
taskGraph(tasks): {
  nodes: Task[];
  edges: { from: string /* dependency */, to: string /* dependent */ }[];
  ready:   Set<string>;            // every depends_on is `done`, self planned/assigned
  waiting: Map<string, string[]>;  // taskId -> unfinished dependency ids
}
```

Rules:

- `blocks(t)` = `tasks.filter(x => x.depends_on_task_ids.includes(t.id))`.
- `ready` ⇒ green chip "ready"; `waiting` ⇒ amber chip "waiting (N)".
- **`status=blocked` (stored) ≠ `waiting` (derived).** A `planned` task with an
  unfinished dependency is `waiting`, not `blocked`. The column is always
  `status`; readiness is an overlay chip.

## 4. Surfaces

### 4.1 Work board (`Goals | Tasks` switch)

```text
+-----------------------------------------------------------------------+
| Work        [ Goals | Tasks ]            goal: All ▾   + New           |
+-----------------------------------------------------------------------+
| Goals mode:  active        blocked        review          done        |
|             [GoalCard]…    [GoalCard]…    [GoalCard]…      [GoalCard]… |
|                                                                       |
| Tasks mode:  planned  assigned  running  blocked  review  done        |
|  filter ?goal=goal-2  → only this goal's tasks                        |
|  [TaskCard ⏳waiting(1)] [TaskCard 🟢ready] …                          |
+-----------------------------------------------------------------------+
```

- **GoalCard**: title, Vision chip, priority, task progress (`done/total`),
  closeout-ready dot.
- **TaskCard**: id, title, status, assignee avatar, goal chip, `ready`/`waiting`
  chip, branch/PR icon.
- Same TaskCard + column component is reused by the Goal page's embedded board;
  "embedded" is just the Task board pre-filtered to `?goal=<id>`.

### 4.2 Goal detail (full page)

```text
header: title · status · priority · Vision link · owner
  ├─ Objective
  ├─ Success criteria (checklist)
  ├─ Tasks ── progress + [ View tasks (N) → Task board ?goal=<id> ]
  ├─ ▸ Goal design        (collapsed)
  ├─ ▸ Goal evaluation     (collapsed)
  └─ ▸ Closeout & decision (collapsed; gate state)
sidebar: governance (owner/team/priority/dates) · git_metadata
```

### 4.3 Task detail (slide-over, expandable to full page)

```text
peek header: id · title · status · ready/waiting
  ├─ meta: goal · assignee · reviewer · owner · branch/PR/worktree · deps
  ├─ Description (markdown)
  ├─ Acceptance criteria (checklist)
  └─ Dependencies: depends on […]  ·  blocks […]
```

### 4.4 Vision detail

`summary` + the **rendered markdown** of each `source_refs` doc, with a table of
contents. Depends on the docs-rendering work package (§5).

## 5. Docs rendering (shipped)

Implemented via the **backend route** (the recommended option below), so docs
stay a single source of truth and the Workbench renders live content rather than
bundling a copy:

- `GET /v1/docs?path=docs/...` returns `{ path, content }` with the raw markdown
  body, allow-listed to the `docs/` tree and protected against path traversal
  (`read_allowed_doc` in [../../crates/harness-cli/src/main.rs](../../crates/harness-cli/src/main.rs)).
- The frontend fetches it through `fetchDoc` / `fetchDocRegistry` in
  [../../apps/agent-dashboard/src/api.ts](../../apps/agent-dashboard/src/api.ts)
  and renders the body with the `Markdown` component
  ([../../apps/agent-dashboard/src/components/workbench/Markdown.tsx](../../apps/agent-dashboard/src/components/workbench/Markdown.tsx)).
  The Docs surface builds its tree from `docs/registry.json` (fetched over the
  same allow-listed route). Live source only — the offline fixture has no docs
  server.

The build-time bundling alternative (embedding `docs/**` into the frontend) was
considered and not taken, to avoid a second copy of the docs drifting from the
canonical tree.

## 6. Acceptance

Per [acceptance.md](acceptance.md), screenshots must show:

- the Work board switching `Goals | Tasks`, with Task cards carrying honest
  `ready` vs `waiting(N)` chips distinct from a `blocked` column;
- a Goal page whose design/evaluation/closeout are collapsed by default and whose
  `View tasks` jumps to the Task board filtered to that goal;
- a Task slide-over showing the three tiers (meta / description /
  acceptance_criteria) and its dependencies;
- a Vision page rendering its `source_refs` docs;
- a `done` Goal that still visibly required closeout Decision + GoalEvaluation.

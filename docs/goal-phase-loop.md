# Goal phase loop

This doc assembles the harness's central execution model in one place. The model is scattered today across `harness-core` data types, the `harness-cli` orchestrator, and the workflow runtime. The loop is:

```text
Goal
  -> Goal.phases[]        (sequential agent-planned phases)
    -> per-phase Task DAG (depends_on + disjoint owned_paths)
      -> compile_phase_to_starlark
        -> harness goal run-phases
          -> verdict gate
            -> per-phase landing
              -> next phase
```

A phase is the unit of planning, gating, and landing. A phase **compiles to a Dynamic Agent Workflow** (a `.star` program of `agent()` / `parallel()` / `verdict()` leaves) that the workflow runtime executes. The workflow runtime is documented separately at [`docs/research/dynamic-workflow-runtime-design.md`](research/dynamic-workflow-runtime-design.md), with locked decisions in [`docs/decisions/0022-dynamic-workflow-runtime-json-ir.md`](decisions/0022-dynamic-workflow-runtime-json-ir.md) and [`docs/decisions/0023-starlark-workflow-frontend.md`](decisions/0023-starlark-workflow-frontend.md).

## 1. Goal and phases

A `Goal` carries an ordered list of agent-planned phases in `Goal.phases` (`crates/harness-core/src/lib.rs:376`). Each phase is a `GoalPhase` (`lib.rs:178`):

```rust
pub struct GoalPhase {
    pub id: String,
    pub name: String,
    pub intent: String,
    pub status: GoalPhaseStatus,
    pub acceptance: Option<String>,       // markdown gate condition
    pub verdict_decision_id: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub outputs: Vec<ArtifactSpec>,       // artifacts this phase promises
    pub inputs: Vec<ArtifactSpec>,        // required cross-phase inputs
    pub retry: Option<u32>,               // per-phase retry budget override
    pub landed_commit: Option<String>,    // commit where writable work landed
}
```

Phases are the source of truth for goal progress. The legacy `Goal.stage` field becomes a derived projection: `Goal.effective_stage()` (`lib.rs:400`) derives `Verified` when every phase is `Passed`, `Working` when any phase has started/failed/blocked, and `Draft` when all are `NotStarted`. `GoalPhaseStatus` (`lib.rs:100`) is `NotStarted | InProgress | Passed | Failed | Blocked`.

A legacy goal with empty `phases` runs as a single implicit phase for back-compat.

## 2. Per-phase task DAG

Each phase owns the tasks whose `Task.phase_id` matches the phase's `id` (`lib.rs:1646`). A `Task` (`lib.rs:1608`) declares:

```rust
pub depends_on_task_ids: Vec<String>,   // lib.rs:1618
pub owned_paths: Vec<String>,           // lib.rs:1622
pub phase_id: Option<String>,           // lib.rs:1646
pub outputs: Vec<ArtifactSpec>,         // lib.rs:1656
pub executor: Option<String>,           // lib.rs:1665 — defaults to "codex"
```

Within a phase the tasks form a DAG on `depends_on_task_ids`. The compiler computes longest-path layering (`compile_phase_to_starlark`, `lib.rs:674-704`) and detects cycles (a cycle never converges and is reported as a compile error). Tasks in the same layer whose `owned_paths` are pairwise disjoint are grouped and run in `parallel()`; tasks with overlapping paths are serialized into separate groups (`lib.rs:817-849`). A task with non-empty `owned_paths` is writable and compiles with `writable=True, isolation="worktree"` so concurrent writers never collide on disk (`lib.rs:643-644`, `775-778`).

This is the core concurrency contract: **parallelism comes from disjoint `owned_paths` plus absence of `depends_on` edges**. Overlapping paths force serial execution within a layer.

## 3. ArtifactSpec — outputs, inputs, and gates

`ArtifactSpec` (`lib.rs:139`) makes artifacts first-class:

```rust
pub struct ArtifactSpec {
    pub id: String,
    pub kind: ArtifactKind,       // DesignDoc, Adr, Code, TestReport, MigrationDoc, RegisteredDoc, Screenshot, Other
    pub path: Option<String>,     // repo-relative path; glob ok
    pub purpose: String,
    pub required: bool,           // defaults to true
    pub acceptance: Option<String>,
}
```

**Outputs.** `GoalPhase.outputs` and `Task.outputs` declare what the phase promises to produce. The verdict gate enforces every `required` artifact with a non-empty `path`: it must be present and non-empty in the phase run's evidence (a worker `worktree_diff`) or in the repo working tree (`crates/harness-cli/src/main.rs:1932-1963`). Empty `outputs` reproduces legacy behavior (the implicit `design_md` + acceptance gate).

**Registered-doc gate.** `ArtifactKind::RegisteredDoc` outputs must also be listed in `docs/registry.json` (`main.rs:1990-2030`). This closes the gap where a doc is produced but not registered.

**Cross-phase inputs.** `GoalPhase.inputs` declares artifacts a prior phase must have landed. Before a phase runs, each required input's `path` must exist as a non-empty file in the working tree; otherwise the phase fails fast at start, records a `phase_verdict` `Decision`, and the orchestration stops (`main.rs:2291-2336`).

## 4. compile_phase_to_starlark

`compile_phase_to_starlark` (`lib.rs:651`) is a pure function from `(goal, phase, tasks)` to a Starlark program. It:

1. Filters live tasks for the phase, skipping `Superseded` tasks (`lib.rs:658-662`).
2. Computes longest-path layers over in-phase `depends_on_task_ids` (`lib.rs:674-704`).
3. Groups each layer by disjoint `owned_paths` (`lib.rs:823-838`).
4. Emits `agent(...)` for singletons and `parallel([...])` for groups (`lib.rs:840-849`).
5. Adds a `writable=True, isolation="worktree"` flag for tasks with `owned_paths` (`lib.rs:775-778`).
6. Uses `Task.executor` (default `"codex"`) as the leaf provider (`lib.rs:761-768`).
7. If `phase.acceptance` is non-empty, emits a judge `agent(..., schema={...}, label=verdict-<phase_id>)` followed by `verdict(...)` (`lib.rs:852-919`).

The compiled script starts with a mandatory `workflow(name, design_intent)` header (`lib.rs:811-815`). The output is deterministic: identical task DAGs produce byte-identical scripts, hence a stable content hash used in the compiled file name.

## 5. goal run-phases orchestrator

`harness goal run-phases <goal>` enters `orchestrate_goal_phases` (`main.rs:2235`). It:

1. Loads the goal and refuses if `phases` is empty (`main.rs:2244-2248`).
2. Reuses an in-flight `GoalOrchestrationRun` with `status == Running` as a resume checkpoint, else starts a fresh one (`main.rs:2254-2273`). `GoalOrchestrationRun` (`lib.rs:301`) persists `phase_runs: Vec<OrchestrationPhaseRun>` (`lib.rs:280`) so `--resume` can re-enter without re-spending completed phases.
3. Walks phases in order. A `Passed` phase is skipped (`main.rs:2279-2282`).
4. Checks cross-phase `inputs` fail-fast (`main.rs:2291-2336`).
5. Reuses a prior workflow run id for intra-phase resume when the phase is `InProgress` or `Failed` (`main.rs:2345-2365`).
6. Enters a replan loop: compile, run, gate, land; on failure capture `Knowledge` and ask the reviser to revise the task graph, up to the retry budget (`main.rs:2374-2591`).
7. Stops the orchestration on the first phase that does not pass (`main.rs:2593-2607`).
8. On success, marks the orchestration `Completed` and reconciles the goal's derived stage (`main.rs:2609-2616`).

The CLI accepts `--max-phase-retries <n>` (default 1) and `--dry-run` (`main.rs:3218-3234`). A phase's own `retry` overrides the global default (`main.rs:2381-2385`).

## 6. Verdict gate

A phase passes only when all of the following hold (`main.rs:2439-2441`):

1. The workflow run status is `Completed`.
2. Every task step `ok` is true.
3. Every required artifact (phase + live task `outputs`) is satisfied (`main.rs:2432-2433`, `unmet_required_artifacts` at `main.rs:1932`).
4. Every required `RegisteredDoc` is present in `docs/registry.json` (`main.rs:2438`, `unmet_registered_docs` at `main.rs:1990`).

If `phase.acceptance` is set, the compiled judge leaf returns structured `{"pass": bool, "reason": string}` and `verdict(...)` hard-gates the phase (`lib.rs:901-918`). If no acceptance is set, clauses 1–3 above form the gate.

The orchestrator records the verdict as a `Decision` with `decision_kind = "phase_verdict"` and points `GoalPhase.verdict_decision_id` at it (`main.rs:2493-2509`). This is the durable acceptance record for the phase.

## 7. Per-phase landing

A passing phase lands its writable work onto the goal branch via `land_phase_diffs` (`main.rs:2119`):

1. Collects non-empty worktree diffs from task steps in deterministic order (by leaf `ordinal`, then journaled index) (`main.rs:2126-2136`).
2. Refuses to start unless the repo index and working tree are clean, so unrelated pre-staged work is not swept into the phase commit (`main.rs:2152-2162`).
3. Applies each diff with `git apply --index` (`main.rs:2173-2206`). A failed apply rolls back to the pre-landing HEAD with `git reset --hard` and converts the pass into a clean failure (`main.rs:2193-2204`).
4. Makes one commit: `"phase <id> landed (run-phases)"` (`main.rs:2212-2218`).
5. Returns the new commit sha, which is stored in `GoalPhase.landed_commit` and `OrchestrationPhaseRun.landed_commit` (`main.rs:2519-2532`).

Read-only phases (no writable diffs) land nothing and leave `landed_commit` as `None`. Sequential phases build on prior landings because the next phase's worktrees branch from the updated HEAD.

## 8. Reconcile out-of-band work

`harness goal reconcile-phase` handles work that shipped outside the orchestrator (e.g., a merged PR). `reconcile_phase` (`main.rs:1681`) is a pure store mutation that:

1. Sets the phase `status` to the operator-asserted verdict.
2. Stamps `ended_at` if unset.
3. Records `landed_commit` when provided.
4. Appends a `decision` `Knowledge` entry with provenance tied to the phase.
5. Syncs the goal's derived stage via `sync_goal_stage` (`main.rs:1406`).

This is the escape hatch for human or external-tool landing; the in-band path should be preferred because it preserves the automated artifact and disjoint-path invariants.

## 9. Failure, knowledge, and replan

When a phase fails, the orchestrator appends a `Knowledge` entry (`main.rs:1629-1671`) summarizing which task labels failed and which workflow run produced the failure. If retries remain, it invokes the reviser (`compile_reviser_script`, `lib.rs:1108`) to produce a structured revision (`{"revision": {"supersede": [...], "new_tasks": [...]}}`). `apply_phase_revision` (`main.rs:1766`) marks superseded tasks `Superseded` and appends new tasks scoped to the same phase. A revision that changes nothing stops the loop to avoid infinite churn (`main.rs:2575-2581`). Each retry consumes one from the budget and reruns the freshly compiled script.

## 10. Task <-> workflow step linkage

After a phase run, `write_back_phase_tasks` (`main.rs:1325`) maps each step labelled with a task id onto `Task.status` (`ok` → `Done`, not `ok` → `Blocked`) and appends the step id to `Task.workflow_step_ids`. `link_workflow_steps_to_tasks` (`main.rs:1366`) stamps `WorkflowStep.task_id` and records `VerdictOutcome::Pass` or `CleanFail` on the verdict step (`label = verdict-<phase_id>`). This gives the causal chain `Goal -> Phase -> Task -> WorkflowStep -> ProviderSession`.

## 11. Relation to the workflow runtime

A phase **is** the plan; the workflow runtime **executes** the compiled phase. `compile_phase_to_starlark` emits a Dynamic Agent Workflow — a Starlark program that calls `agent()`, `parallel()`, and `verdict()` primitives provided by `crates/harness-workflow`. The runtime is provider-neutral, schedules leaves under a concurrency cap, journals `WorkflowRun` + `WorkflowStep` rows, and supports deterministic resume via step-identity caching. See the runtime design doc and ADRs cited at the top of this doc.

In the product, every workflow run should carry `goal_id` and `phase_id` so the causal chain is machine-traversable. The compiler already emits `phase=<phase_id>` on every leaf; the runtime stores this on `WorkflowStep.phase`, and `link_workflow_steps_to_tasks` binds it back to `Task` records.

## 12. Source-of-truth summary

| Concept | Canonical location | Projection |
| --- | --- | --- |
| Phase plan | `Goal.phases[]` + `Task` DAG | Compiled `.star` script |
| Phase progress | `GoalPhase.status` | `Goal.stage` (derived) |
| Verdict | `Decision(decision_kind=phase_verdict)` + `GoalPhase.verdict_decision_id` | Dashboard phase timeline |
| Landed code | `GoalPhase.landed_commit` | Git commit `phase <id> landed (run-phases)` |
| Resume | `GoalOrchestrationRun.phase_runs[]` | `--resume` flag |
| Task execution | `Task.workflow_step_ids` -> `WorkflowStep` -> `ProviderSession` | Dashboard step drill-in |

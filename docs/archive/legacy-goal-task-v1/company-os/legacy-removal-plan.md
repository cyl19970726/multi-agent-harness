# Legacy Goal / Task-Graph Removal Plan

## Decision and scope

This is the Wave 2 retirement plan for the superseded `Goal`, `GoalDesign`,
`GoalPhase`, legacy `Task`/Task Graph, and their planning/learning loop. The
end state is **full retirement from active product context and code**, not an
indefinite compatibility layer.

`Mission -> Wave -> executor` remains the coordination hierarchy. Company work
uses `WorkItem`, `Assignment`, `Approval`, and the Company OS records defined
in [the Company OS entry point](../../../company-os/README.md). A generic English use of task is
not a removal target; the target is the typed Task Graph protocol and every
API or UI surface which makes it an active product model.

This audit was run against the working tree on 2026-07-20. Active means
checked-out product source, shipped docs, schemas, fixtures, scripts, plugins,
or the current `.harness` store. It excludes Git history, build outputs, and
other worktrees. Reproduce exact active references with:

```bash
rg -n -i --glob '!target/**' --glob '!node_modules/**' --glob '!docs/archive/**' \
  '(GoalDesign|GoalPhase|TaskGraph|Task Graph|generic-agent-harness|star-goal|star-planner)' \
  AGENTS.md README.md crates apps schemas scripts skills plugins workflows docs examples
```

## Active reference inventory

### Core schemas, records, and store

| Surface | Exact active files / symbols | Removal implication |
| --- | --- | --- |
| Core domain shapes | `crates/harness-core/src/lib.rs`: `PhaseExecutionMode`, `GoalPhase`, `Goal`, `Task`, `GoalDesign`, `GoalEvaluation`, `GoalCase`, `GoalOrchestrationRun`, `MissionProjection::from_goal`; legacy links on `WorkflowRun`, `WorkflowStep`, messages, evidence, decisions, reviews, members, team runs, and provider sessions | Delete types and phase planning only after dependent foreign keys are archived and deliberately replaced where needed. |
| Store API and ledgers | `crates/harness-store/src/lib.rs`: append/read methods for Goal, Task, GoalDesign, GoalEvaluation, GoalCase, GoalOrchestrationRun, plus `mission_projections()` | Remove old ledger access and stop manufacturing Goal-derived Mission projections. |
| Schemas and fixtures | `schemas/{goal.schema.json,task.schema.json,goal-design.schema.json,goal-evaluation.schema.json,goal-case.schema.json}`; `schemas/fixtures/{goal,task,goal-design,goal-evaluation,goal-case}/**` | Delete with their schema checks. `schemas/README.md`, `docs/schemas.md`, and `docs/registry.json` currently index them. |

### CLI, HTTP, MCP, and transport

| Surface | Exact active files / routes | Removal implication |
| --- | --- | --- |
| CLI | `crates/harness-cli/src/main.rs`: `goal`, `phase`, `task`, `goal-design`, `goal-evaluation`, `goal-case`, `autonomy`, `board`, phase compiler/reviser/runner, closeout/learning helpers, and `GoalOrchestrationRun` handling | Remove command families and planning machinery. Do not redirect `goal` to `mission`; freeze returns a migration notice, then commands disappear. |
| HTTP | `crates/harness-cli/src/main.rs`: `/v1/goals`, `/v1/tasks`, `/v1/tasks/{id}/assign`, `/reviewer`, `/review` | Remove writes first, then reads. Add WorkItem APIs only when their Company OS contract exists. |
| Snapshot / SSE | `crates/harness-cli/src/main.rs` snapshot construction; `crates/harness-cli/src/sse.rs` | Drop `goals`, `tasks`, `goal_designs`, `goal_evaluations`, `goal_cases`, and task-derived views from active payloads. |
| MCP | `crates/harness-cli/src/mcp.rs`: Mission compatibility wording and `team_message.task_id` | Remove the link and wording; retain correlation-backed TeamMessage ownership. |
| Tests | `crates/harness-cli/tests/{mission_wave_api.rs,kimi_provider.rs,serve_projects_api.rs}` | Replace compatibility assertions with Mission/Wave and TeamMessage correlation coverage. |

### Dashboard

| Surface | Exact active files | Removal implication |
| --- | --- | --- |
| Public model / navigation | `apps/agent-dashboard/src/{types.ts,api.ts,api/actions.ts,app/selection.ts,app/WorkbenchShell.tsx}` | Delete Goal, Task, phase selection, task actions, and goal/task snapshot handling. |
| Derived data | `apps/agent-dashboard/src/model/{readModel.ts,warnings.ts,workflowSelectors.ts,teamSelectors.ts}` | Delete Goal dual-read, `taskGraph`, task lanes, learning status, and phase selectors. Preserve native Mission/Wave and TeamRun selectors. |
| Rendered UI | `apps/agent-dashboard/src/surfaces/{Surfaces.tsx,Workflows.tsx,TeamWarRoom.tsx}`; `apps/agent-dashboard/src/components/workbench/{OperatorForms.tsx,WorkflowPanels.tsx,tones.ts,atoms.tsx}` | Remove Goal Workbench, Task focus, task board, phase timeline/DAG, and task-assignment controls. Preserve Mission/Wave, MemberRun, TeamRun War Room, and Company OS pages. |
| Tests / fixture | `apps/agent-dashboard/tests/{phase-board-check.mjs,workbench-visual-fixture-check.mjs}`; `apps/agent-dashboard/fixtures/workbench-layout-v2-native-v1/fixture-manifest.json` | Remove phase-board checks; revise visual validation to have no Goal compatibility claim. |

### Workflows, examples, documentation, and skills

| Surface | Exact active files | Removal implication |
| --- | --- | --- |
| Goal-only workflows | `workflows/{goal-workbench-design.star,custom-workflow-phase-runner-acceptance.star}` | Delete; their contract is GoalPhase / Task Graph execution. |
| Acceptance scripts | `scripts/{acceptance-mvp.mjs,acceptance-autonomous-team.mjs,verify-fixes.sh,check-schema-fixtures.mjs,validate-json.mjs}` | Split out goal-learning and phase checks; retain only live execution checks rewritten around Mission/Wave or WorkItem. |
| Examples | `examples/goal-cases/**` | Archive with a manifest or delete; never leave under active `examples/`. |
| Retired Skills | No checked-in `generic-agent-harness`, `star-goal`, or `star-planner` directory remains. References persist in `AGENTS.md`, `docs/VISION.md`, two historical GoalCase files, and `docs/design/internal-workflows/*.js`. | Remove from default instructions and active docs; archive explanation; never reinstall or recreate. |
| Skills to preserve | `skills/star-workflow/**`, `skills/bootstrap-project-workflow/**`, `plugins/kimi-agent-team/skills/**`, `.agents/skills/multi-agent-system-design/**` | Edit only their retired Goal-planner wording; retain their actual capabilities. |

The active documentation/design set needing rewrite or archival is:

```text
AGENTS.md; README.md
docs/{README.md,VISION.md,prd.md,architecture.md,architecture-map.md,concept-model.md,data-model.md,core-modules.md,agent-control-plane.md,agent-integration-model.md,agent-runtime.md,workflow-runtime.md,workflow-git-pr.md,governance-engine.md,goal-learning-loop.md,goal-phase-loop.md}
docs/issues/phase-execution-modes.md
docs/dashboard/{README.md,acceptance.md,design-principles.md,frontend-architecture.md,frontend-design.md,layout-history.md,read-model.md,runbook.md,work-board-design.md}
docs/dashboard/pages/{README.md,goal.md,team-run-console.md,team-run-war-room.md,mission-wave-canvas.md,member-run-focus.md,standing-agent-focus.md}
docs/design-basis.md; docs/design/agent-team-goal-wave-layout.md
docs/design/company-os-v1/{README.md,page-matrix.md,visual-contract.json,prompts/*.md}
docs/design/workbench-layout-v2/{README.md,page-matrix.md,reviews/p0-implementation-review.md,prompts/**/*.md}
docs/integration/{codex.md,codex-source-audit.md}
docs/decisions/{README.md,0006-task-graph-before-workflow-dsl.md,0009-task-graph-as-derived-view.md,0012-dashboard-is-control-plane.md,0017-generic-object-model.md,0019-vision-goal-task-workbench-redesign.md,0024-goal-phase-execution-modes.md,0025-agent-team-run-control-plane.md,0026-mission-wave-architecture.md,0027-company-os-primary-model.md}
docs/vision/task-vs-workflow.svg
```

`docs/company-os/**` is not a deletion target, but its transitional Goal/Task
wording must stop promising compatibility projections.

## Data migration and deletion risks

The project has two independently significant append-only sources. The central
project store is live; the repository-local `.harness` is the migrated source
and still contains local-only history. Counts observed on 2026-07-20 are:

```text
ledger                         central store   migrated repo-local store
goals.jsonl                    258             77
tasks.jsonl                    225             44
goal_designs.jsonl              11              0
goal_evaluations.jsonl            7              absent
goal_cases.jsonl                  absent         absent
goal_orchestration_runs.jsonl      3              absent
```

The central store additionally has 204 Message, 379 Evidence, 81 Proposal,
56 Decision, 3 Review, 2,225 ProviderSession, 1,139 WorkflowRun, and 3,642
WorkflowStep rows. The migrated repository-local source has 28, 39, 12, 14,
1, 911, 386, and 2,003 respectively. These are append-log row counts, not
unique entity counts. R0 snapshots and compares both stores independently;
neither count may be treated as a substitute for the other.

1. `goal_id`, `phase_id`, `task_id`, `current_task_id`, `task_ids`,
   `goal_design_id`, `goal_design_ref`, `follow_up_task_id`, `source_goal_id`,
   `source_task_id`, and `legacy_goal_phase_ids` occur beyond old ledgers.
   They appear in sessions, workflow runs/steps, messages, evidence, reviews,
   decisions, members, TeamRun records, and projections.
2. `MissionProjection::from_goal` currently makes old Goals look like Missions.
   Removing it before export hides provenance.
3. A `Task` is provider delivery bookkeeping. Renaming it to `WorkItem` is
   unsafe: a WorkItem owns business responsibility, source/result documents,
   approvals, and financial effects; a provider turn does not.
4. Goal/phase runners append revised records. Exporting latest rows alone loses
   attempts, failures, supersession history, and evidence lineage.
5. Legacy CLI, HTTP, snapshot, SSE, and MCP callers must get a versioned freeze
   error, never a silent conversion into Mission/Wave or WorkItem.

## Staged freeze, export, and deletion

### R0 — archive contract

Implement a read-only, versioned `legacy-goal-task-v1` exporter before removing
any writer. The immutable archive contains:

- byte-for-byte old ledgers;
- manifest: SHA-256 hashes, line counts, exporter version, project id, source path;
- `latest/` projections for append-only ledgers;
- `edges.jsonl` for every listed foreign key;
- linked Message, Evidence, Proposal, Decision, Review, ProviderSession,
  WorkflowRun/Step, TeamRun/MemberRun/TeamMessage records when they hold a link;
- old schemas, fixtures, GoalCase examples, retired Skill text, and historical
  ADR/design sources required to interpret the records.

The exporter uses a finite ledger + JSON-path foreign-key contract. It never
recursively scans dynamic `args`, `result`, `spec`, or `final_output` payloads.
Missing contract-required interpretation material is explicit in the manifest
as `source_present=false, reason=not_present_in_source`; this is how the absent
retired Skill directories are represented without recreating them.

The exporter never rewrites JSONL, creates a Mission/Wave projection, or coerces
Task rows into WorkItems.

### R1 — freeze creation and default context

- Remove legacy command routes, HTTP writers, Dashboard actions, and MCP inputs.
- Remove Goal compatibility from `mission list` and default snapshots.
- Remove retired Skills and Goal-planning instructions from default entry points.
- Legacy reads return one migration notice with the export location; no synthesis.
- Remove active navigation, example, prompt, and documentation links.

Gate: a new Agent can follow all repository entry points without encountering a
Goal/Task Graph creation path.

### R2 — export and verify every project

Export every configured project, including this repository’s `.harness`, before
deleting a ledger or type. Block deletion on hash, line-count, or referential
closure failure. Archives live outside the active store and docs registry:

```text
~/.harness/archives/<project-id>/legacy-goal-task-v1/<timestamp>/
```

The UI may explicitly open the archive as history; it must never dual-read it
as live work.

### R3 — replace remaining live responsibilities

- Use an execution-scoped `work_ref` or `assignment_correlation_id` for provider
  delivery bookkeeping, not `WorkItem` by default.
- Add WorkItem APIs only after its schema and policy gates exist. Preserve source
  document, owner, executor, reviewer, result/evidence, approvals, and finance
  only where those facts genuinely exist; otherwise retain historical execution.
- Keep Mission/Wave native. Do not convert Goals to Missions after archive export.

### R4 — delete in dependency order

1. Dashboard Goal/Task UI, selectors, actions, fixtures, and visual checks.
2. CLI/HTTP/MCP/SSE Goal and Task surfaces plus Goal/phase runner code.
3. Store methods, old ledgers, core types, schemas, fixtures, and legacy links.
4. Goal-only workflows, scripts, examples, screenshots, registry entries, docs.
5. Historical ADR/design sources from active navigation into the archive, leaving
   a concise active supersession note.

Delete a ledger only after its exact project has a verified archive and every
dependent reference has been checked.

### R5 — prove no compatibility remains

Run acceptance against a freshly initialized project, migrated live project,
and dashboard fixture. Only `docs/archive/**` and the archive manifest may
contain legacy terms.

## Proposed owned-path implementation split

| Lane | Owned paths | Deliverable / gate |
| --- | --- | --- |
| Archive and migration | `crates/harness-cli/src/legacy_export.rs`, `crates/harness-store/**`, `crates/harness-cli/tests/legacy_export.rs`, `docs/archive/**` | Read-only exporter, manifest/edge closure, per-project verification. No core deletion. |
| Core and store removal | `crates/harness-core/src/**`, `crates/harness-store/src/**`, `schemas/{goal*,task.schema.json,fixtures/goal/**,fixtures/task/**}` | Remove types, links, ledgers, schemas only after R2. |
| CLI/API/transport | `crates/harness-cli/src/{main.rs,mcp.rs,sse.rs}`, `crates/harness-cli/tests/**` | Remove routes and install explicit R1 migration errors. |
| Dashboard | `apps/agent-dashboard/src/**`, `apps/agent-dashboard/tests/**`, `apps/agent-dashboard/fixtures/**` | Remove Goal/Task UI and graph; native Mission/Wave/TeamRun remains working. |
| Docs/workflows/Skills | `AGENTS.md`, `README.md`, `docs/**`, `examples/goal-cases/**`, `workflows/**`, `scripts/**`, `skills/**`, `plugins/**` | Active context says Mission/Wave + Company OS only; material archives or deletes. |
| Integration reviewer | Cross-lane; no shared implementation path | Verifies archive closure, no WorkItem coercion, no context leakage, acceptance. |

Use disjoint worktrees. Archive lands before any deletion lane; the reviewer
gates every deletion batch.

## Acceptance commands

The R0 exporter and offline verifier are implemented commands:

```bash
# R0/R2: per-project archive and closure verification
target/debug/harness legacy-goal-task export --project <id> --output <archive-dir>
target/debug/harness legacy-goal-task verify --archive <archive-dir>
shasum -a 256 <archive-dir>/manifest.json

# R1: no legacy creation route remains
! target/debug/harness goal create --title should-fail --objective should-fail
! target/debug/harness phase list --goal should-fail
! target/debug/harness task create --title should-fail --objective should-fail

# R4/R5: native behavior and integrity
cargo test --workspace
npx pnpm@9.15.4 check
npx pnpm@9.15.4 acceptance:mvp
git diff --check

# R5: no active legacy product/default-context reference
! rg -n -i --glob '!target/**' --glob '!node_modules/**' --glob '!docs/archive/**' \
  '(GoalDesign|GoalPhase|TaskGraph|Task Graph|generic-agent-harness|star-goal|star-planner)' \
  AGENTS.md README.md crates apps schemas scripts skills plugins workflows docs examples

# R5: no live legacy ledger in a migrated store
! find <active-store-root> -maxdepth 1 -type f \( \
  -name goals.jsonl -o -name tasks.jsonl -o -name goal_designs.jsonl -o \
  -name goal_evaluations.jsonl -o -name goal_cases.jsonl -o \
  -name goal_orchestration_runs.jsonl \) -print -quit | grep .
```

The final search excludes only `docs/archive/**`: normal README, skill catalog,
docs registry, Dashboard, and runtime must not require archive reading.

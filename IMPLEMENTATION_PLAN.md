# Implementation Plan — Dynamic Workflow Runtime (skill + CLI)

**Goal**: Let any agent (Codex / Claude Code / other) author a JSON workflow spec at
runtime and run it through a Rust runtime that deterministically orchestrates other
agents, with the run visualized live in the Agent Dashboard.

**Locked decisions**
- Trigger = **skill + CLI** (`harness workflow run-spec <spec.json>`). No MCP/plugin now;
  an MCP shim over the same CLI is an optional future addition (sandboxed agents /
  external-project distribution only).
- Dynamic spec = **JSON-IR** (`WorkflowNode { Agent | Phase | Parallel | Pipeline }`),
  schema-validated. Not an embedded JS interpreter.
- Runtime extracted into a **new `crates/harness-workflow` lib crate** (provider-agnostic);
  `harness-cli` depends on it and injects the real `AgentStepFn` delivery driver.

**Reuse (exists today)**: WP2 scheduler + `parallel()` + `AgentStepFn` seam
(`harness-cli/src/workflow.rs`), real driver `workflow_real_agent_step` → neutral delivery
seam (`main.rs:3724`, `run_provider_delivery` `main.rs:7452`), `WorkflowRun`/`WorkflowStep`
objects + store + SSE, Dashboard Workflows surface (`inferWorkflowShape`, `StepCard`,
`AsciiGraph`) and the `TurnDrillIn` live-tool-call component.

Architecture diagram: `docs/design/orchestration-plugin-architecture.{dot,svg,png}`.

---

## Stage 1: Extract `harness-workflow` crate + WorkflowSpec IR + `run-spec` CLI
**Goal**: A new provider-agnostic `harness-workflow` lib crate housing the runtime, plus a
JSON-IR spec the CLI can interpret so an agent authors the workflow shape at runtime.
**Success Criteria**:
- `crates/harness-workflow` exists; runtime moved out of `harness-cli/src/workflow.rs`
  behind the injected `AgentStepFn` seam; `harness-cli` depends on it and injects the real
  driver; `cargo test` green across the workspace.
- `schemas/workflow-spec.schema.json` is a gateable schema with valid/invalid fixtures.
- `WorkflowNode { Agent | Phase | Parallel | Pipeline }` IR + `dispatch_spec()` interpreter.
- `harness workflow run-spec <spec.json>` parses, validates, runs the IR, journals a
  `WorkflowRun` + `WorkflowStep`s.
**Tests**:
- Crate extraction: existing `workflow.rs` unit tests pass unchanged in the new crate.
- IR parse + schema validation (valid + invalid fixtures via `check:schema-fixtures`).
- `dispatch_spec` runs a serial→parallel spec; serial node completes before the barrier.
- Failed required node fails the run but parallel siblings are still collected.
**Status**: Not Started

## Stage 2: Real `pipeline()` + WP3 object fields
**Goal**: Implement streaming `pipeline()` (per-item through stages, no barrier) and add the
run/step fields a dynamic run needs.
**Success Criteria**:
- `pipeline()` runs items through stages with overlapping windows (not a `parallel()` fallback).
- `WorkflowRun.args` (JSON parameterization), `WorkflowRun.agents_spawned`,
  `WorkflowRun.final_output`; `WorkflowStep.result` (structured output, not just summary).
- Dashboard shape correctly labels a pipeline phase.
**Tests**:
- `pipeline` ordering/no-barrier test; an item failing one stage drops to null and skips its rest.
- `args` parameterization flows into node prompts.
- Schema fixtures updated for the new fields.
**Status**: Not Started

## Stage 3: `author-workflow` skill
**Goal**: A skill that teaches an agent to write a valid `WorkflowSpec`, invoke
`harness workflow run-spec`, and read the run back — so the capability is usable by any
shell-capable agent with no plugin.
**Success Criteria**:
- `.agents/skills/author-workflow/SKILL.md` passes `check:skills` and documents: spec shape,
  one worked example, the CLI invocation, reading the run, and the permission note (member
  profile must allow the `harness` binary).
- A Codex member and a Claude member, given the skill, each author + run a real dynamic
  workflow that lands in the store.
**Tests**:
- `pnpm check:skills` + `check:doc-governance` green.
- Acceptance step drives a member through authoring + running a spec end-to-end.
**Status**: Not Started

## Stage 4: Dashboard per-node drill-in
**Goal**: Click a workflow node to see that worker's streamed tool calls, live.
**Success Criteria**:
- `StepCard` is clickable → opens a detail panel wrapping `TurnDrillIn` for the node's
  `provider_session_id`.
- `live_turn_events` is threaded `readModel → Timeline → StepCard → TurnDrillIn` for
  sub-second refresh.
**Tests**:
- `pnpm check:dashboard` (tsc + vite build) green.
- Preview verification: trigger a run, confirm nodes render live and a node drill-in shows
  ordered tool_use/tool_result.
**Status**: Not Started

## Stage 5: End-to-end acceptance + docs + ADR
**Goal**: A scripted acceptance proving the whole loop, plus design docs and a decision record.
**Success Criteria**:
- `acceptance:dynamic-workflow` script: a member authors + runs a 2-provider dynamic
  workflow → dashboard renders it with per-node drill-in → script asserts green.
- `docs/research/dynamic-workflow-runtime-design.md` updated for the spec/IR + skill+CLI path;
  one ADR recording the locked decisions.
**Tests**:
- Acceptance script passes in CI mode (mock providers) and live mode (`--live`).
- `pnpm check` (all governance checks) green.
**Status**: Not Started

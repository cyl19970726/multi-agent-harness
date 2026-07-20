# 0022: Dynamic Workflow Runtime — skill + CLI entry, JSON-IR spec, `harness-workflow` crate

## Status

> **Superseded in part by
> [0023 (Starlark program front-end)](0023-starlark-workflow-frontend.md):** the
> JSON-IR authoring path decided here — the `WorkflowSpec` / `WorkflowNode` /
> `dispatch_spec()` interpreter, the `harness workflow run-spec` CLI arm, and
> `schemas/workflow-spec.schema.json` (+ fixtures + the
> `acceptance-dynamic-workflow` proof) — was **deleted** once Starlark proved a
> strict superset. Starlark (run via `harness workflow run-script`) is now the
> sole dynamic authoring surface alongside the compiled Rust registry. The
> `crates/harness-workflow` crate, the additive `WorkflowRun`/`WorkflowStep`
> fields, the scheduler primitives (`parallel()`/`pipeline()`), and the
> one-journal/one-dashboard contract decided here are **kept**.

Accepted. **Promotes the deferred parts of
[the dynamic-workflow design study](../research/dynamic-workflow-runtime-design.md)**
(its WP5 IR + an author surface) into a shipped path, and builds on
[0011 (Provider-neutral runtime)](0011-provider-neutral-runtime.md),
current schema-governance guidance, and
[0018 (Exec-stream primary substrate)](0018-exec-stream-primary-substrate.md).

(0021 is the latest prior ADR; 0022 is the next free number.)

## Context

The design study (§3) recommended Hybrid **Option C** — ship named, compiled
Rust workflows behind a runtime registry first, and defer the runtime-authored
IR (Option B) to a later WP "once the node shapes are proven." WP1–WP4 landed
that registry runtime: the `WorkflowScheduler` (barrier `parallel()`, the
`AgentStepFn` seam, the `min(16, cores-2)` cap), the `WorkflowRun`/`WorkflowStep`
objects + store + SSE, and the read-only Dashboard Workflows surface.

With the registry proven and the `parallel()` / `pipeline()` node shapes stable,
the remaining goal is to let **any agent author a workflow shape at runtime** —
without a recompile and without each new shape being a binary change. That forces
three decisions the study left open: (1) how an agent *triggers* a runtime
workflow, (2) how the runtime-authored *plan* is represented, and (3) where the
runtime *lives* so it stays provider-agnostic.

## Decision

1. **Trigger = skill + CLI, not MCP/plugin.** The entry point is the existing
   `harness` binary: a new subcommand `harness workflow run-spec <spec.json>`
   alongside `workflow run --name`, plus a `star-workflow` skill
   (`skills/star-workflow/SKILL.md`) that teaches an agent to write a
   valid spec, invoke the CLI, and read the run back. Any shell-capable agent
   (Codex, Claude Code, other) can use it with no plugin install and no new
   transport. An MCP shim over the *same* CLI is an explicitly optional future
   addition — only for sandboxed agents with no shell or for external-project
   distribution — and is **not built now**.

2. **Dynamic spec = JSON-IR, not an embedded JS interpreter.** A runtime-authored
   workflow is a schema-validated JSON document
   (`schemas/workflow-spec.schema.json`) deserialized into
   `WorkflowSpec { name, args, nodes }` where
   `WorkflowNode = Agent | Phase | Parallel | Pipeline`. The
   `dispatch_spec()` interpreter walks the IR and applies the same serial /
   barrier / streaming semantics the registry workflows use. This is the study's
   Option B promoted (§3), and it keeps the study's owner-decision 1 intact: we
   do **not** embed a JS VM (the study, §3.3) — the IR is plain data, not foreign
   code, so the determinism contract and the provider-neutral boundary are
   preserved.

3. **Runtime lives in a new `crates/harness-workflow` lib crate.** The scheduler,
   primitives (`agent()` / `parallel()` / `pipeline()`), the `WorkflowSpec` IR,
   and `dispatch_spec()` are extracted out of `harness-cli/src/workflow.rs` into a
   provider-agnostic crate behind the injected `AgentStepFn` trait object.
   `harness-cli` depends on it and injects the real delivery driver
   (`workflow_real_agent_step` → `run_provider_delivery`), so the runtime never
   names a provider (per [0011](0011-provider-neutral-runtime.md)) and the
   registry `run` path and the dynamic `run-spec` path share one
   `journal_workflow_outcome` so both journal identically.

## Consequences

- **No new object types; additive fields only.** Per
  current schema-governance guidance, the dynamic path reuses
  `WorkflowRun`/`WorkflowStep` and adds optional fields a dynamic run needs:
  `WorkflowRun.args` (JSON parameterization, `{{key}}`-interpolated into node
  prompts), `WorkflowRun.agents_spawned`, `WorkflowRun.final_output`, and
  `WorkflowStep.result` (structured per-step output). Existing registry runs set
  them to defaults.
- **One contract, two front-ends.** Because the trigger is the CLI and the spec
  is data, the dashboard, SSE, store, and per-node `TurnDrillIn` drill-in all see
  a `run-spec` run identically to a registry `run` — no UI fork.
- **`pipeline()` is real streaming, not a `parallel()` fallback.** Items flow
  through stages with no barrier; a stage that fails drops the item and skips its
  remaining stages (the design study's CC-spec failure-drop). The barrier-vs-
  streaming distinction the study flagged as the key behavioral contract is
  honored.
- **Schema is gateable.** `workflow-spec.schema.json` ships with valid/invalid
  fixtures under `check:schema-fixtures`, so a malformed runtime-authored spec is
  rejected before any agent is spawned.
- **MCP stays a future option, not a dependency.** Nothing in the path requires
  an MCP server; the optional shim, if ever built, is a thin wrapper over
  `run-spec` and inherits this contract unchanged.

## Validation

```bash
cargo test --workspace
pnpm check
node scripts/acceptance-dynamic-workflow.mjs   # mock mode, exits 0
```

The end-to-end proof is `scripts/acceptance-dynamic-workflow.mjs`: it authors a
two-provider `WorkflowSpec` (serial `plan` → parallel `audit` barrier → streaming
`synthesize` pipeline), runs it through `harness workflow run-spec --dry-run`,
and asserts the journaled `WorkflowRun` + ordered `WorkflowStep`s carry the
expected serial → parallel → pipeline shape, a populated `final_output`, and are
visible over the live dashboard `/v1/snapshot` API.

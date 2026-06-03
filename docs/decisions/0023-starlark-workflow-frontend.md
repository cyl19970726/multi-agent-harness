# 0023: Starlark program front-end for the workflow runtime

## Status

Accepted, then **superseded in part** by the convergence note below. Originally
added a **third** authoring front-end to the workflow runtime shipped in
[0022 (Dynamic Workflow Runtime — JSON-IR)](0022-dynamic-workflow-runtime-json-ir.md),
and inherits its provider-neutral boundary from
[0011 (Provider-neutral runtime)](0011-provider-neutral-runtime.md) and its
additive-only object contract from [0017 (Generic object model)](0017-generic-object-model.md).

(0022 is the latest prior ADR; 0023 is the next free number.)

## Convergence: Starlark is the SOLE dynamic authoring surface

After shipping all three front-ends, the JSON-IR (0022) and Starlark surfaces
proved redundant: Starlark is a strict superset — every static `agent`/`phase`/
`parallel`/`pipeline` tree the IR could express is a trivially-equivalent
Starlark program, and Starlark additionally expresses the loops / conditionals /
data-driven fan-out the IR cannot. Maintaining two runtime-authoring surfaces (a
schema + fixtures + interpreter for the IR, plus the Starlark evaluator) doubled
the authoring docs and the untrusted-input surface for no added capability.

We therefore **delete the JSON-IR path** and keep Starlark as the only dynamic
authoring surface (alongside the compiled Rust built-in registry):

- `WorkflowSpec` / `WorkflowNode` / `dispatch_spec` and the
  `harness workflow run-spec` CLI arm are removed, along with
  `schemas/workflow-spec.schema.json` (+ fixtures), the JSON examples, and the
  `acceptance-dynamic-workflow` proof.
- The runtime primitives the IR walked (`parallel()` / `pipeline()` /
  `outcome_from_steps` / scheduler / registry / `investigate`) are unchanged —
  Starlark drives them directly.

### `design_intent` is mandatory

Because a Starlark program is now the single authored artifact of record, every
program MUST declare a `workflow(name, design_intent)` header. The
`design_intent` is a free-text explanation of WHY the workflow is shaped the way
it is; the run is **rejected fail-fast** if the header is missing or the intent
is blank / under ~20 characters. The captured intent is persisted on
`WorkflowRun.design_intent` and the raw program text is snapshotted on
`WorkflowRun.spec`, so every dynamic run carries both its rationale and its
reproducible source as a durable audit record. This makes the authoring surface
self-documenting: a run's shape can always be read back together with the reason
it was chosen.

## Context

The runtime now has two authoring surfaces: named, compiled **Rust built-ins**
behind the registry, and the runtime-authored **JSON-IR run-spec** (0022). The
JSON-IR is plain data — `parallel()` / `pipeline()` / `phase` nodes the runtime
walks — which is exactly why it stays hermetic and gateable. But it is also a
*static* shape: an agent declares the fan-out up front and cannot express
loops, conditionals, or data-driven fan-out (iterate over a list the previous
step produced, branch on a result, recurse). That is the expressiveness gap
between a declarative spec and a real **program**.

Claude Code's internal Workflow tool lets an agent write a program with that
control flow. To give a harness-driven agent the same authoring power at runtime
— without a recompile and without widening the JSON-IR into a homegrown
expression language — we need a front-end that accepts an actual program while
keeping the runtime's determinism contract intact.

## Decision

1. **Add a Starlark program front-end alongside the two existing surfaces.** A
   third authoring path: the agent writes a Starlark program (loops,
   conditionals, comprehensions, data-driven fan-out) that calls the same
   `agent()` / `parallel()` / `pipeline()` primitives the JSON-IR and built-ins
   use. This gives runtime-authored workflows the expressiveness of Claude
   Code's internal Workflow tool without a binary change.

2. **Embed a hermetic Starlark interpreter.** We use the existing
   `starlark = "0.14"` (Meta's crate, already a dependency of
   `crates/harness-workflow`). Starlark is chosen precisely because it is
   *designed* to be hermetic: no clock, no random, no filesystem/network, no
   ambient I/O, and deterministic iteration order. The program is pure control
   flow over the workflow primitives; **all** nondeterminism stays in the
   journaled `agent()` leaves — the program decides *shape*, the agent leaves do
   the *work*. This preserves the same determinism guarantee the JSON-IR path
   has, even though the front-end is now Turing-capable.

3. **Only the parser + evaluator are new; the backend is reused unchanged.** The
   evaluator translates primitive calls into the same scheduler dispatch the
   JSON-IR walker already performs, behind the injected `AgentStepFn` seam. The
   `WorkflowScheduler` (barrier `parallel()`, streaming `pipeline()`, the
   concurrency cap), the `WorkflowRun` / `WorkflowStep` objects + store + SSE,
   `journal_workflow_outcome`, and the read-only Dashboard Workflows surface are
   all untouched. The Starlark program is just a third producer of the same
   dispatch sequence.

## Consequences

- **One contract, three front-ends.** Built-ins, JSON-IR, and Starlark all feed
  one scheduler and one journal, so the dashboard, SSE, store, and per-node
  `TurnDrillIn` drill-in see a Starlark run identically to a `run-spec` or a
  registry `run` — no UI fork and no new object types (per 0017).
- **Expressiveness without a custom DSL.** Loops / branches / data-driven
  fan-out come from a real, battle-tested language rather than an ad-hoc
  extension of the JSON-IR, so the IR stays simple, declarative data.
- **Determinism rests on hermeticity.** The guarantee holds only as long as the
  interpreter exposes no nondeterministic globals; the primitive surface given
  to a program must stay pure control flow + the journaled `agent()` leaf.
- **Untrusted-code surface is now bigger.** A program is foreign code, not data.
  This ADR records the *front-end*; it does not yet finalize the sandbox policy.

## Follow-up (not built now)

- **`load()`-gating.** Decide and enforce policy for Starlark's `load()`
  statement (module imports) — default-deny, or an explicit allow-list of
  harness-provided modules — so a runtime-authored program cannot pull in
  arbitrary or escape code.
- **Execution-limit hardening.** Bound the interpreter: step/instruction budget,
  wall-clock and memory ceilings, and recursion/loop limits, so a malformed or
  hostile program cannot wedge the runtime before any agent is spawned. Mirror
  the JSON-IR path's "rejected before any agent is spawned" property.

## Validation

```bash
cargo test --workspace
cargo fmt --check
```

The end-to-end proof mirrors the JSON-IR acceptance: author a Starlark program
that fans out data-driven over a list (loop + `parallel()`) and branches on a
prior step's result, run it through the runtime, and assert the journaled
`WorkflowRun` + ordered `WorkflowStep`s carry the expected shape and are visible
over the live dashboard `/v1/snapshot` API — proving the Starlark front-end
reuses the 0022 backend unchanged.

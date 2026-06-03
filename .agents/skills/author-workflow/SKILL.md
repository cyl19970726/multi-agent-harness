---
name: author-workflow
description: "Use when an agent needs to author a dynamic multi-agent workflow at runtime: write a Starlark program (loops, conditionals, data-driven fan-out) that calls agent()/parallel()/phase()/log() over the harness runtime, declare a mandatory workflow(name, design_intent) header, run it with `harness workflow run-script`, and read the resulting WorkflowRun/WorkflowStep records back from the dashboard snapshot or store."
---

# Author Workflow

Use this skill to make a shell-capable agent (Codex, Claude Code, or any other)
author a workflow at runtime and run it through the harness, with no MCP or
plugin. Starlark is the SOLE dynamic authoring surface: write a `.star` program
and run it through `harness workflow run-script`, which journals a `WorkflowRun`
plus one `WorkflowStep` per agent leaf.

```text
write a .star program  ->  harness workflow run-script <prog.star>  ->  read the run back
```

The runtime is provider-agnostic (`crates/harness-workflow`). Each `agent()` call
names a PROVIDER (`"codex"` or `"claude"`); the CLI spins up a NEW one-shot
ephemeral worker for that call (it does NOT deliver to a pre-existing member) and
journals a `WorkflowRun` plus one `WorkflowStep` per agent call — identical to the
built-in `workflow run --name` path, so the run shows up live on the Agent
Dashboard Workflows surface.

Each ephemeral worker CAN EDIT files (full sandbox + editing tools, not
read-only) and, by default, shares the repo cwd with every sibling call — serial
calls' edits compose naturally on the same tree. When two calls mutate the tree
in parallel and you do not want them to collide, opt one or both into
`isolation="worktree"` (see below).

## When To Use

- The shape of the orchestration is decided at runtime, not baked into a built-in
  registry workflow.
- You want to fan work out (scan, then parallel fix; map/reduce; a data-driven
  fan-out whose width depends on a prior step) and have the run recorded as
  evidence.
- A Lead Agent wants a worker to both design and execute a small multi-agent plan.

Do not use this skill to do the domain work yourself. Use it to author the
program, run it, and then read the recorded run.

## The Mandatory `workflow(name, design_intent)` Header

Every program MUST call `workflow(name, design_intent)` exactly once, before the
rest of the body. It declares:

- `name` — the workflow name (becomes `WorkflowRun.workflow_name`; overrides the
  CLI `--name` / file-stem default).
- `design_intent` — a free-text explanation of WHY the workflow is structured the
  way it is. This is the run's durable rationale.

The run is REJECTED fail-fast if `workflow(...)` is never called, or the
`design_intent` is blank or shorter than ~20 characters:

```text
every workflow must declare a design_intent explaining WHY it is structured this way
```

The captured `design_intent` is persisted on `WorkflowRun.design_intent`, and the
raw program text is snapshotted under `WorkflowRun.spec = {"lang":"starlark","script": <text>}`
for reproducibility.

## Host API

The interpreter is [Starlark](https://github.com/facebook/starlark-rust)
([`crates/harness-workflow/src/starlark_front.rs`](../../../crates/harness-workflow/src/starlark_front.rs)),
the same dialect Bazel uses. It is HERMETIC by design: the script has no clock, no
randomness, and no IO. The orchestration (which agents run, in what order, with what
prompts) is therefore deterministic — the ONLY nondeterminism lives in the journaled
`agent()` leaves.

A program calls these globals (no `import`; they are pre-bound):

| Call | Returns | Meaning |
| --- | --- | --- |
| `workflow(name, design_intent)` | — | REQUIRED header. Declares the run name + the WHY behind its shape. Must run once before the body. |
| `agent(prompt, provider="codex", label=, phase=, model=, isolation=)` | output text | Run ONE ephemeral worker synchronously. `prompt` is positional; the rest are keyword args. `isolation="worktree"` runs it in a throwaway worktree. Capture the return to chain: `scan = agent("...")`. |
| `parallel([dict, ...])` | list of output strings (input order) | Barrier fan-out: run every spec concurrently, block until ALL finish. Each dict needs a `prompt` and may set `provider` (default `"codex"`), `label`, `phase`, `model`, `isolation`. |
| `phase(name)` | — | Set the default phase for the steps that follow. |
| `log(message)` | — | Emit a progress line. |
| `args` | value | The `--args` JSON, injected as a module global (e.g. `args["items"]`). |

Rules every call obeys:

- `provider` is `"codex"` or `"claude"` — the provider whose ephemeral worker
  runs the leaf. There is NO member binding; the provider drives delivery.
- `prompt`, `label`, and `phase` are non-empty strings; optional `model` (any
  non-empty string) overrides the provider's default model.
- The only supported `isolation` value is `"worktree"`.
- Reference `args` inside a prompt with normal Starlark string concatenation
  (e.g. `"audit " + args["area"]`).

### Workspace and `isolation="worktree"`

By default every call edits the SHARED repo cwd. That is what you want for serial
work (a scan call, then a fix call that builds on it). It is NOT safe when several
calls mutate the tree at the same time — the runtime does not auto-prevent
conflicts on the shared tree.

Set `"isolation": "worktree"` on a `parallel()` spec (or `isolation="worktree"` on
an `agent()` call) to run it in its own harness-owned throwaway git worktree under
`.harness/worktrees/`. That call's `git diff` becomes its evidence; the worktree is
NOT auto-merged back and is cleaned up after the call finishes (auto-removed if
unchanged). Use it as the escape hatch whenever a `parallel` block has two or more
slots that EDIT files, so they cannot stomp each other.

## Worked Example: data-driven scan, then parallel fix

A serial scan call (shared cwd) whose output decides the fan-out width: one fix
slot per defect line. Because the fix slots EDIT files in parallel, each opts into
`isolation="worktree"` so they cannot collide. The runnable copy is
[`examples/scan-then-parallel-fix.star`](examples/scan-then-parallel-fix.star):

```python
workflow(
    "scan-then-parallel-fix",
    "Scan once on the shared tree to enumerate defects, then fan out one isolated " +
    "worktree fix per defect so the parallel fixes cannot collide on the same files.",
)

phase("scan")
scan = agent(
    "Scan " + args["area"] + " for defects. Return a numbered list, one per line.",
    provider="codex",
)

phase("fix")
parallel([
    {
        "prompt": "Fix this defect in " + args["area"] + ": " + line + ". Make the minimal change and explain it.",
        "provider": "codex",
        "isolation": "worktree",
    }
    for line in scan.splitlines() if line
])
```

The `scan` phase completes (its edits land on the shared cwd) before the
`parallel` barrier fans out, and each fix slot then runs concurrently in its own
worktree and joins before the run finalizes. The fan-out WIDTH is decided at
runtime from the scan's output — a comprehension over its lines — which no static
shape could express.

## Run It

Write the program to a file, then invoke the CLI. The program's `provider` values
drive delivery, so there is no member binding to pass:

```bash
harness workflow run-script ./scan-then-parallel-fix.star --args '{"area":"checkout flow"}'
```

Useful flags:

| Flag | Effect |
| --- | --- |
| `--name <n>` | Default workflow name; the `workflow(...)` header overrides it. |
| `--args <json>` | Injected as the `args` global. |
| `--dry-run` | Use a mock driver so the program runs end-to-end without spawning agents. |
| `--start-runtime` | Start the provider runtime if it is not already running. |
| `--timeout-ms <ms>` | Per-step delivery timeout (default 3000). |
| `--trace durable\|live` | Retain the heavy per-step turn-event trace (`durable`, default) or stream-only (`live`). |

The command prints the journaled run as JSON, including the new `run` id.

## Read The Run Back

The run and its steps are persisted in the store and exposed on the dashboard.
Read them without raw JSONL reads via the snapshot, which carries `workflow_runs`
and `workflow_steps`:

```bash
harness dashboard snapshot | node -e '
  const s = JSON.parse(require("node:fs").readFileSync(0, "utf8"));
  console.log(JSON.stringify({
    runs: s.workflow_runs,
    steps: s.workflow_steps
  }, null, 2));
'
```

Confirm: a `WorkflowRun` with your `name`, status moving `running -> completed`
(or `failed`), the declared `design_intent`, the snapshotted `spec` script,
`args` echoed back, and one `WorkflowStep` per agent call with its `label`,
`phase`, `provider`, and result. The same run renders on the Agent Dashboard
Workflows surface.

## Permission Note

The agent that runs the program invokes the `harness` binary through its shell, so
its permission profile must allow it:

- The runner's allowed-tool / command policy must permit running the `harness`
  binary (for Claude this is a `Bash(harness ...)` allowance; for Codex the
  sandbox/approval policy must let the shell call through).
- Each agent call spins up a fresh ephemeral worker that CAN EDIT files and runs
  in the repo cwd (or its worktree). A `prompt` that writes files or runs
  destructive/money-moving actions executes for real — scope prompts accordingly
  and reach for `isolation="worktree"` when parallel calls mutate the tree.
- If `harness` is not on the runner's `PATH`, invoke it by absolute path and
  ensure that path is the allowed command.

## Checklist

- [ ] Program declares `workflow(name, design_intent)` once, before the body, with a real (>= ~20 char) design_intent.
- [ ] Program calls only `workflow`/`agent`/`parallel`/`phase`/`log`/`args`; no clock/random/IO assumed.
- [ ] Every agent leaf (`agent()` call / `parallel` spec) has a `provider` of `"codex"` or `"claude"`.
- [ ] Parallel slots that EDIT files use `isolation` `"worktree"`.
- [ ] Ran it: `harness workflow run-script <prog.star>` (no member binding needed).
- [ ] The run is visible in `harness dashboard snapshot` (`workflow_runs` / `workflow_steps`) with its `design_intent`.
- [ ] The runner's profile allows the `harness` binary and what each leaf's prompt does.

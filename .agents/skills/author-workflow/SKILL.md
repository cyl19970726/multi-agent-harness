---
name: author-workflow
description: "Use when an agent needs to author a dynamic multi-agent workflow at runtime: write a JSON WorkflowSpec (Agent/Phase/Parallel/Pipeline IR), validate it against the schema, run it with `harness workflow run-spec`, and read the resulting WorkflowRun/WorkflowStep records back from the dashboard snapshot or store."
---

# Author Workflow

Use this skill to make a shell-capable agent (Codex, Claude Code, or any other)
author a workflow shape at runtime and run it through the harness, with no MCP or
plugin. The capability is a JSON spec plus one CLI command:

```text
write WorkflowSpec JSON  ->  harness workflow run-spec <spec.json>  ->  read the run back
```

The runtime is provider-agnostic (`crates/harness-workflow`). Each `agent` node
names a PROVIDER (`"codex"` or `"claude"`); the CLI spins up a NEW one-shot
ephemeral worker for that node (it does NOT deliver to a pre-existing member) and
journals a `WorkflowRun` plus one `WorkflowStep` per agent node — identical to the
built-in `workflow run --name` path, so the run shows up live on the Agent
Dashboard Workflows surface.

Each ephemeral worker CAN EDIT files (full sandbox + editing tools, not
read-only) and, by default, shares the repo cwd with every sibling node — serial
nodes' edits compose naturally on the same tree. When two nodes mutate the tree
in parallel and you do not want them to collide, opt one or both into
`isolation: "worktree"` (see below).

## When To Use

- The shape of the orchestration is decided at runtime, not baked into a built-in
  registry workflow.
- You want to fan work out across members (scan, then parallel fix; map/reduce;
  a streaming pipeline) and have the run recorded as evidence.
- A Lead Agent wants a worker to both design and execute a small multi-agent plan.

Do not use this skill to do the domain work yourself. Use it to author the spec,
run it, and then read the recorded run.

## Spec Shape

A `WorkflowSpec` is a JSON object validated by
[`schemas/workflow-spec.schema.json`](../../../schemas/workflow-spec.schema.json).
Top level:

| Field | Required | Meaning |
| --- | --- | --- |
| `name` | yes | Non-empty workflow name; becomes `WorkflowRun.workflow_name`. |
| `nodes` | yes (for the top level) | Ordered array of nodes run in sequence. |
| `args` | no | Opaque JSON parameterization; carried onto `WorkflowRun.args` and substituted into node prompts as `{{key}}`. |

Each node is exactly one of four kinds (the IR `WorkflowNode { Agent | Phase | Parallel | Pipeline }`):

| `type` | Required fields | Behavior |
| --- | --- | --- |
| `agent` | `provider`, `prompt` | Spins up one ephemeral `provider` worker. Optional `phase`/`label` group/name the step; optional `model` overrides the provider model; optional `isolation` runs it in a throwaway worktree. |
| `phase` | `name`, `nodes` | A named serial group; its `nodes` run in order. |
| `parallel` | `nodes` | Its `nodes` run concurrently and join at a barrier before the next top-level node. |
| `pipeline` | `stages` | Items stream through `stages` with overlapping windows (no full barrier between stages). |

Rules that keep a spec valid:

- `provider` is `"codex"` or `"claude"` — the provider whose ephemeral worker
  runs the node. There is NO member binding; the spec's provider drives delivery.
- Every node object has exactly the fields its `type` allows; unknown fields are
  rejected (`additionalProperties: false`).
- `prompt`, `name`, and `label` must be non-empty strings.
- Optional `model` (any non-empty string) overrides the provider's default model.
- Optional `isolation` is `"worktree"` (the only supported value).
- Reference `args` inside a prompt with `{{key}}` (e.g. `{{area}}`).

### Workspace and `isolation: "worktree"`

By default every node edits the SHARED repo cwd. That is what you want for serial
work (a scan node, then a fix node that builds on it). It is NOT safe when several
nodes mutate the tree at the same time — the runtime does not auto-prevent
conflicts on the shared tree.

Set `"isolation": "worktree"` on a node to run it in its own harness-owned
throwaway git worktree under `.harness/worktrees/`. That node's `git diff` becomes
its evidence; the worktree is NOT auto-merged back and is cleaned up after the
node finishes (auto-removed if unchanged). Use it as the escape hatch whenever a
`parallel` block has two or more nodes that EDIT files, so they cannot stomp each
other.

## Worked Example: scan, then parallel fix

A serial scan node (shared cwd) whose result the two parallel fix nodes act on.
Because the two fix nodes EDIT files in parallel, each opts into
`isolation: "worktree"` so they cannot collide. The runnable copy is
[`examples/scan-then-parallel-fix.json`](examples/scan-then-parallel-fix.json):

```json
{
  "name": "scan-then-parallel-fix",
  "args": { "area": "checkout flow" },
  "nodes": [
    {
      "type": "phase",
      "name": "scan",
      "nodes": [
        {
          "type": "agent",
          "provider": "codex",
          "prompt": "Scan {{area}} for defects. Return a numbered list of the distinct problems you find, each with the file path and a one-line description.",
          "label": "scan"
        }
      ]
    },
    {
      "type": "parallel",
      "nodes": [
        {
          "type": "agent",
          "provider": "codex",
          "prompt": "Fix the first defect reported by the scan in {{area}}. Make the minimal change and explain it.",
          "phase": "fix",
          "label": "fix-codex",
          "isolation": "worktree"
        },
        {
          "type": "agent",
          "provider": "claude",
          "prompt": "Fix the second defect reported by the scan in {{area}}. Make the minimal change and explain it.",
          "phase": "fix",
          "label": "fix-claude",
          "isolation": "worktree"
        }
      ]
    }
  ]
}
```

The `scan` phase completes before the `parallel` barrier starts (its edits land
on the shared cwd), and both `fix` nodes then run concurrently in their own
worktrees and join before the run finalizes.

## Validate Before Running

`run-spec` parses and rejects an invalid spec, but validate against the schema
first to get a precise error. From the repo root:

```bash
node -e '
  const Ajv = require("ajv/dist/2020").default;
  const fs = require("node:fs");
  const schema = JSON.parse(fs.readFileSync("schemas/workflow-spec.schema.json", "utf8"));
  const spec = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
  const validate = new Ajv({ allErrors: true }).compile(schema);
  if (validate(spec)) { console.log("spec valid"); }
  else { console.error(validate.errors); process.exit(1); }
' .agents/skills/author-workflow/examples/scan-then-parallel-fix.json
```

`schemas/workflow-spec.schema.json` is also gated in CI via the valid/invalid
fixtures under `schemas/fixtures/workflow-spec/`; add a fixture there when you
extend the IR.

## Run It

Write the spec to a file, then invoke the CLI. The spec's `provider` values drive
delivery, so there is no member binding to pass:

```bash
harness workflow run-spec ./scan-then-parallel-fix.json
```

Useful flags:

| Flag | Effect |
| --- | --- |
| `--dry-run` | Use a mock driver so the IR runs end-to-end without spawning agents. |
| `--start-runtime` | Start the provider runtime if it is not already running. |
| `--timeout-ms <ms>` | Per-step delivery timeout (default 3000). |

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

Confirm: a `WorkflowRun` with your `name`, status moving `running -> succeeded`
(or `failed`), `args` echoed back, and one `WorkflowStep` per agent node with its
`label`, `phase`, `provider`, and result. The same run renders on the Agent
Dashboard Workflows surface.

## Permission Note

The agent that runs the spec invokes the `harness` binary through its shell, so
its permission profile must allow it:

- The runner's allowed-tool / command policy must permit running the `harness`
  binary (for Claude this is a `Bash(harness ...)` allowance; for Codex the
  sandbox/approval policy must let the shell call through).
- Each agent node spins up a fresh ephemeral worker that CAN EDIT files and runs
  in the repo cwd (or its worktree). A `prompt` that writes files or runs
  destructive/money-moving actions executes for real — scope prompts accordingly
  and reach for `isolation: "worktree"` when parallel nodes mutate the tree.
- If `harness` is not on the runner's `PATH`, invoke it by absolute path and
  ensure that path is the allowed command.

## Checklist

- [ ] Spec has `name` + `nodes`; every node is a valid `agent`/`phase`/`parallel`/`pipeline`.
- [ ] Every `agent` node has a `provider` of `"codex"` or `"claude"`.
- [ ] Parallel nodes that EDIT files use `isolation: "worktree"`.
- [ ] Spec validates against `schemas/workflow-spec.schema.json`.
- [ ] `harness workflow run-spec <spec.json>` ran (no member binding needed).
- [ ] The run is visible in `harness dashboard snapshot` (`workflow_runs` / `workflow_steps`).
- [ ] The runner's profile allows the `harness` binary and what each node's prompt does.

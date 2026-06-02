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

The runtime is provider-agnostic (`crates/harness-workflow`). The CLI resolves the
spec's member NAMES to real harness member ids and injects the real delivery
driver, then journals a `WorkflowRun` plus one `WorkflowStep` per agent node —
identical to the built-in `workflow run --name` path, so the run shows up live on
the Agent Dashboard Workflows surface.

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
| `agent` | `member`, `prompt` | One delivery to a member. Optional `phase` and `label` group/name the step on the dashboard. |
| `phase` | `name`, `nodes` | A named serial group; its `nodes` run in order. |
| `parallel` | `nodes` | Its `nodes` run concurrently and join at a barrier before the next top-level node. |
| `pipeline` | `stages` | Items stream through `stages` with overlapping windows (no full barrier between stages). |

Rules that keep a spec valid:

- `member` is a NAME (e.g. `"codex"`, `"claude"`), not a harness member id. You
  bind names to ids at run time with `--codex` / `--claude` / `--member name=id`.
- Every node object has exactly the fields its `type` allows; unknown fields are
  rejected (`additionalProperties: false`).
- `prompt`, `name`, `member`, and `label` must be non-empty strings.
- Reference `args` inside a prompt with `{{key}}` (e.g. `{{area}}`).

## Worked Example: scan, then parallel fix

A serial scan node whose result the two parallel fix nodes act on. The runnable
copy is [`examples/scan-then-parallel-fix.json`](examples/scan-then-parallel-fix.json):

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
          "member": "codex",
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
          "member": "codex",
          "prompt": "Fix the first defect reported by the scan in {{area}}. Make the minimal change and explain it.",
          "phase": "fix",
          "label": "fix-codex"
        },
        {
          "type": "agent",
          "member": "claude",
          "prompt": "Fix the second defect reported by the scan in {{area}}. Make the minimal change and explain it.",
          "phase": "fix",
          "label": "fix-claude"
        }
      ]
    }
  ]
}
```

The `scan` phase completes before the `parallel` barrier starts, and both `fix`
nodes run concurrently and join before the run finalizes.

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

Write the spec to a file, then invoke the CLI. The member flags bind the spec's
names to real harness member ids:

```bash
harness workflow run-spec ./scan-then-parallel-fix.json \
  --codex <codex-member-id> \
  --claude <claude-member-id>
```

Useful flags:

| Flag | Effect |
| --- | --- |
| `--codex <id>` / `--claude <id>` | Bind the `codex` / `claude` names. |
| `--member <name>=<id>` | Bind any other name a spec references (repeatable). |
| `--dry-run` | Use a mock driver so the IR runs end-to-end without spawning agents. |
| `--start-runtime` | Start the member runtime if it is not already running. |
| `--timeout-ms <ms>` | Per-step delivery timeout (default 3000). |

At least one member binding is required. The command prints the journaled run as
JSON, including the new `run` id.

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
`label`, `phase`, and result. The same run renders on the Agent Dashboard
Workflows surface.

## Permission Note

The agent that runs the spec invokes the `harness` binary through its shell, so
the member's permission profile must allow it:

- The member's allowed-tool / command policy must permit running the `harness`
  binary (for Claude members this is a `Bash(harness ...)` allowance; for Codex
  members the sandbox/approval policy must let the shell call through).
- Binding `--codex` / `--claude` only resolves NAMES to member ids; it does not
  grant either member new permissions. Each spawned member still runs under its
  own profile, so a `prompt` that writes files or runs money-moving or
  destructive actions needs that member's profile to allow it.
- If `harness` is not on the runner's `PATH`, invoke it by absolute path and
  ensure that path is the allowed command.

## Checklist

- [ ] Spec has `name` + `nodes`; every node is a valid `agent`/`phase`/`parallel`/`pipeline`.
- [ ] `member` values are names bound at run time, not member ids.
- [ ] Spec validates against `schemas/workflow-spec.schema.json`.
- [ ] `harness workflow run-spec <spec.json>` ran with at least one member binding.
- [ ] The run is visible in `harness dashboard snapshot` (`workflow_runs` / `workflow_steps`).
- [ ] The runner's profile allows the `harness` binary, and each member's profile allows what its prompt does.

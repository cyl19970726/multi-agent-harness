# Workflow runtime

This document defines the canonical workflow runtime for `crates/harness-workflow`: a provider-neutral Starlark execution layer that lets the harness run authored multi-agent programs, journal their leaves, and expose the result as `WorkflowRun` / `WorkflowStep` records.

## Vision Link

The product needs a programmable orchestration layer between a high-level goal/task graph and provider-specific worker CLIs. A workflow is useful only after the harness can explain:

```text
Starlark program
  -> deterministic host-control flow
  -> provider-neutral agent leaf specs
  -> ephemeral provider workers
  -> WorkflowStep rows
  -> WorkflowRun terminal summary/output
  -> optional goal-phase landing
```

The workflow runtime is not the durable goal/task/message protocol. It is the execution substrate that a Lead-authored `.star` program and `goal run-phases` both use to fan out provider work and record the result.

## Boundary

| Runtime surface | Owns | Cedes |
| --- | --- | --- |
| `crates/harness-workflow` | Provider-neutral `AgentStepSpec`, `StepResult`, scheduler, `parallel()`, streaming `pipeline()`, built-in registry workflows, Starlark evaluation, outcome shaping | Provider CLI spawning, store writes, project selection, worktree creation/cleanup, goal/task status updates |
| `starlark_front` | Hermetic script evaluation, host globals, required `workflow(...)` metadata, `args` injection, deterministic leaf ordinals, budget/replay control | Provider behavior, filesystem effects, CLI flag parsing |
| `harness-cli workflow run-script` | Reads scripts, builds the real provider driver, appends `WorkflowRun` / `WorkflowStep`, retention/progress/resume policy | Starlark language semantics and scheduler internals |
| `goal run-phases` | Compiles goal phases to Starlark, gates phase results, lands passing writable diffs | The lower-level leaf execution runtime |

The crate was explicitly extracted so it contains no Codex or Claude provider code; the binary injects real delivery through `AgentStepFn`, while tests inject a mock driver (`crates/harness-workflow/src/lib.rs:1`, `crates/harness-workflow/src/lib.rs:3`, `crates/harness-workflow/src/lib.rs:4`, `crates/harness-workflow/src/lib.rs:146`). `AgentStepFn` returns `StepResult` instead of panicking so workflow control flow owns failure handling (`crates/harness-workflow/src/lib.rs:146`, `crates/harness-workflow/src/lib.rs:150`).

The Starlark front-end is the sole dynamic authoring surface: it lets an agent write loops, conditionals, and data-driven fan-out while driving the same ephemeral-worker backend through the injected driver (`crates/harness-workflow/src/starlark_front.rs:1`, `crates/harness-workflow/src/starlark_front.rs:3`, `crates/harness-workflow/src/starlark_front.rs:6`). The interpreter is hermetic: scripts get no clock, randomness, or IO; nondeterminism lives in journaled `agent()` leaves (`crates/harness-workflow/src/starlark_front.rs:10`, `crates/harness-workflow/src/starlark_front.rs:11`).

## Runtime Objects

### `AgentStepSpec`

`AgentStepSpec` is the provider-neutral leaf description produced by `agent()`, `parallel()`, `pipeline()`, and compiled goal phases. It carries:

| Field | Meaning |
| --- | --- |
| `phase` | Declarative grouping for the step |
| `label` | Human-readable step name within the phase |
| `provider` | Provider id such as `codex`, `claude`, or `kimi` |
| `model` | Optional provider model override |
| `effort` | Optional provider reasoning-effort override |
| `fallback_model` | Optional provider fallback model when the adapter supports it |
| `image` | Image file paths passed or described to the worker |
| `add_dir` | Extra directory paths the worker may access |
| `isolation` | Optional isolation mode; only `worktree` is supported |
| `prompt` | Worker prompt |
| `schema` | Optional structured-output schema |
| `writable` | Whether the leaf may edit files / run shell |
| `ordinal` | Deterministic leaf ordinal used by `--resume` |

The fields are defined in `crates/harness-workflow/src/lib.rs:40` through `crates/harness-workflow/src/lib.rs:83`. The only supported isolation constant is `worktree`; an isolated node runs in a throwaway git worktree whose diff is evidence and is not auto-merged by standalone workflow execution (`crates/harness-workflow/src/lib.rs:27`, `crates/harness-workflow/src/lib.rs:29`, `crates/harness-workflow/src/lib.rs:30`).

### `StepResult`

`StepResult` is the runtime result for one leaf. It records phase, label, provider, isolation, success, provider session linkage, output summary, optional started/step ids, details telemetry, optional structured output, and ordinal (`crates/harness-workflow/src/lib.rs:85`, `crates/harness-workflow/src/lib.rs:88`, `crates/harness-workflow/src/lib.rs:89`, `crates/harness-workflow/src/lib.rs:97`, `crates/harness-workflow/src/lib.rs:99`, `crates/harness-workflow/src/lib.rs:101`, `crates/harness-workflow/src/lib.rs:117`, `crates/harness-workflow/src/lib.rs:124`, `crates/harness-workflow/src/lib.rs:132`). `StepResult::step_status()` maps `ok=true` to `WorkflowStepStatus::Completed` and `ok=false` to `Failed` (`crates/harness-workflow/src/lib.rs:135`, `crates/harness-workflow/src/lib.rs:137`).

### `WorkflowOutcome`

`WorkflowOutcome` is the runtime's whole-run value: ordered steps, terminal status, summary, spawned-agent count, and optional final JSON output (`crates/harness-workflow/src/lib.rs:463`, `crates/harness-workflow/src/lib.rs:465`, `crates/harness-workflow/src/lib.rs:466`, `crates/harness-workflow/src/lib.rs:477`). `outcome_from_steps()` is shared by registry workflows and Starlark runs so all front-ends derive status, summary, and final output identically (`crates/harness-workflow/src/lib.rs:657`, `crates/harness-workflow/src/lib.rs:659`). A run with steps but zero successful leaves fails; otherwise partial success completes unless a Starlark `verdict()` overrides the outcome (`crates/harness-workflow/src/lib.rs:665`, `crates/harness-workflow/src/lib.rs:677`).

`step_result_json()` is the machine-facing projection stored on `WorkflowStep.result` and inside final output. It includes phase, label, provider, isolation, ok, provider session id, output summary, structured payload, ordinal, and merged telemetry details such as model, exit code, duration, tokens, failures, and worktree diffs (`crates/harness-workflow/src/lib.rs:624`, `crates/harness-workflow/src/lib.rs:627`, `crates/harness-workflow/src/lib.rs:628`, `crates/harness-workflow/src/lib.rs:643`).

## Host API

Every Starlark program must call:

```python
workflow(name, design_intent, budget_usd=None, success_criterion=None)
```

The header declares the run name and the durable reason for the workflow shape. Missing or too-short `design_intent` rejects the run (`crates/harness-workflow/src/starlark_front.rs:16`, `crates/harness-workflow/src/starlark_front.rs:18`, `crates/harness-workflow/src/starlark_front.rs:68`, `crates/harness-workflow/src/starlark_front.rs:1202`). The CLI persists the captured `design_intent` and snapshots the raw script under `spec = {"lang":"starlark","script": ...}` (`crates/harness-cli/src/main.rs:9473`, `crates/harness-cli/src/main.rs:9475`, `crates/harness-cli/src/main.rs:9733`).

### `agent()`

```python
agent(
    prompt,
    provider="codex",
    label=None,
    phase=None,
    model=None,
    effort=None,
    fallback_model=None,
    image=None,
    add_dir=None,
    isolation=None,
    schema=None,
    writable=False,
)
```

`agent()` runs one ephemeral provider worker synchronously. In text mode it returns the worker's output text. With `schema={...}`, it returns the parsed structured dict, or `None` when no valid JSON object with the required keys was produced (`crates/harness-workflow/src/starlark_front.rs:847`, `crates/harness-workflow/src/starlark_front.rs:849`, `crates/harness-workflow/src/starlark_front.rs:850`, `crates/harness-workflow/src/starlark_front.rs:902`).

Argument semantics:

| Argument | Meaning |
| --- | --- |
| `prompt` | Required worker prompt |
| `provider` | Provider id; defaults to `codex` |
| `label` | Step label; defaults to provider id |
| `phase` | Step phase; defaults to current `phase()` or workflow name |
| `model` | Leaf model override, otherwise CLI `--model`, otherwise provider default |
| `effort` | Leaf effort override, otherwise CLI `--effort`, otherwise provider default |
| `fallback_model` | Passed to providers that support native fallback |
| `image` | List of image paths |
| `add_dir` | List of additional directory paths |
| `isolation` | Optional `worktree` |
| `schema` | Dict converted to JSON schema / structured-output contract |
| `writable` | Enables edit/shell behavior and implies worktree isolation |

The actual host function signature is defined at `crates/harness-workflow/src/starlark_front.rs:854` through `crates/harness-workflow/src/starlark_front.rs:867`. `schema`, `image`, and `add_dir` are validated as dict/list values before the spec is built (`crates/harness-workflow/src/starlark_front.rs:869`, `crates/harness-workflow/src/starlark_front.rs:878`, `crates/harness-workflow/src/starlark_front.rs:882`).

### `parallel()`

```python
parallel([
    {"prompt": "audit api", "provider": "codex", "label": "api"},
    {"prompt": "audit cli", "provider": "claude", "label": "cli"},
])
```

`parallel(specs)` is a barrier fan-out: all specs run concurrently, then the call returns a list in input order. Each spec requires `prompt` and accepts the same leaf fields as `agent()` where relevant (`crates/harness-workflow/src/starlark_front.rs:28`, `crates/harness-workflow/src/starlark_front.rs:31`, `crates/harness-workflow/src/starlark_front.rs:915`, `crates/harness-workflow/src/starlark_front.rs:920`). The runtime extracts plain Rust specs before threading, so no Starlark heap values cross the barrier (`crates/harness-workflow/src/starlark_front.rs:927`).

The scheduler bounds concurrency to `min(16, available_parallelism()-2)` and enforces a 1000-agent lifetime cap (`crates/harness-workflow/src/lib.rs:7`, `crates/harness-workflow/src/lib.rs:161`, `crates/harness-workflow/src/lib.rs:175`, `crates/harness-workflow/src/lib.rs:194`). Results are re-ordered back into input order (`crates/harness-workflow/src/lib.rs:302`).

### `pipeline()`

```python
pipeline(
    args["files"],
    [
        {"prompt": "scan {input}", "label": "scan"},
        {"prompt": "fix according to {input}", "label": "fix", "writable": True},
    ],
)
```

`pipeline(items, stages)` is a streaming fan-out: each item flows through every stage independently, with no barrier between stages. A fast item can reach stage 3 while another remains in stage 1 (`crates/harness-workflow/src/lib.rs:324`, `crates/harness-workflow/src/lib.rs:325`, `crates/harness-workflow/src/starlark_front.rs:944`, `crates/harness-workflow/src/starlark_front.rs:945`). Each stage `prompt` may contain `{input}`, replaced by the original item for stage 1 or by the prior stage's output for later stages (`crates/harness-workflow/src/starlark_front.rs:953`, `crates/harness-workflow/src/starlark_front.rs:954`, `crates/harness-workflow/src/starlark_front.rs:680`, `crates/harness-workflow/src/starlark_front.rs:711`).

`pipeline()` accepts either the canonical list form `pipeline(items, [s1, s2])` or positional stage specs `pipeline(items, s1, s2)` (`crates/harness-workflow/src/starlark_front.rs:958`, `crates/harness-workflow/src/starlark_front.rs:960`, `crates/harness-workflow/src/starlark_front.rs:968`). Pipeline leaves currently advance ordinals but are excluded from `--resume` replay in v1 (`crates/harness-workflow/src/starlark_front.rs:386`, `crates/harness-workflow/src/starlark_front.rs:388`, `crates/harness-workflow/src/starlark_front.rs:1144`, `crates/harness-workflow/src/starlark_front.rs:1147`).

### `phase()`

```python
phase("audit")
```

`phase(name)` sets the default phase for subsequent steps that do not name one explicitly (`crates/harness-workflow/src/starlark_front.rs:45`, `crates/harness-workflow/src/starlark_front.rs:996`). Phase resolution is: explicit step phase, current `phase()`, then workflow name (`crates/harness-workflow/src/starlark_front.rs:133`, `crates/harness-workflow/src/starlark_front.rs:176`).

### `log()`

```python
log("audit complete; starting fix fan-out")
```

`log(message)` records a progress/narration line in the run context (`crates/harness-workflow/src/starlark_front.rs:46`, `crates/harness-workflow/src/starlark_front.rs:1036`). Logs are persisted under `final_output.logs` when the run completes (`crates/harness-workflow/src/starlark_front.rs:1251`, `crates/harness-workflow/src/starlark_front.rs:1260`).

### `output()`

```python
output({"ok": True, "summary": "ready"})
```

`output(value)` declares the run's first-class result. The last call wins, and the value is persisted under `final_output.result` rather than forcing callers to infer the answer from a step summary (`crates/harness-workflow/src/starlark_front.rs:1021`, `crates/harness-workflow/src/starlark_front.rs:1023`, `crates/harness-workflow/src/starlark_front.rs:1028`, `crates/harness-workflow/src/starlark_front.rs:1257`).

### Supporting Globals

`args` is the parsed JSON from `--args`, injected as a module global (`crates/harness-workflow/src/starlark_front.rs:47`, `crates/harness-workflow/src/starlark_front.rs:1190`). `json.encode()` / `json.decode()` are available through the Starlark JSON extension for structured forward-injection (`crates/harness-workflow/src/starlark_front.rs:1180`). `verdict(ok, reason="")` marks intent-relative success or failure; `ok=False` makes the run fail even if every worker executed (`crates/harness-workflow/src/starlark_front.rs:1005`, `crates/harness-workflow/src/starlark_front.rs:1006`, `crates/harness-workflow/src/starlark_front.rs:1228`).

## CLI Surface

The dynamic authoring command is:

```bash
harness workflow run-script <prog.star> [flags]
```

The script path can be positional or supplied as `--script <path>` (`crates/harness-cli/src/main.rs:9604`). The command is routed through `workflow_command`, which also exposes `workflow run`, `get-output`, `list`, `reap`, `gc-worktrees`, and `gc-trace` (`crates/harness-cli/src/main.rs:9333`, `crates/harness-cli/src/main.rs:9336`, `crates/harness-cli/src/main.rs:9393`).

Key `run-script` flags:

| Flag | Meaning |
| --- | --- |
| `<prog.star>` / `--script <path>` | Starlark program to evaluate |
| `--name <name>` | Initial/default workflow name; the Starlark `workflow(...)` header overrides it |
| `--args <json>` | JSON value injected as global `args` |
| `--resume <run_id>` | Re-run identical script and replay prior completed leaves by deterministic ordinal |
| `--trace durable\|live` | Retain provider turn trace durably, or stream live only |
| `--dry-run` | Use mock provider results while journaling the same run/step shape |
| `--start-runtime` | Accepted in options; ephemeral workflow leaves do not require resident runtimes |
| `--timeout-ms <ms>` | Per-node idle timeout; default is 900000 ms |
| `--model <model>` | Run-level default model; leaf `model=` wins |
| `--effort <effort>` | Run-level default effort; leaf `effort=` wins |
| `--max-budget-usd <n>` | Per-run cumulative budget ceiling and per-worker Claude backstop |
| `--progress` | Emit compact NDJSON progress events to stderr |
| `--initiated-by <id>` | Initiator id; defaults to `HARNESS_AGENT_MEMBER_ID` or `operator` |

These flags are parsed in `workflow_run_script_value` (`crates/harness-cli/src/main.rs:9600`, `crates/harness-cli/src/main.rs:9651`, `crates/harness-cli/src/main.rs:9661`, `crates/harness-cli/src/main.rs:9671`, `crates/harness-cli/src/main.rs:9681`, `crates/harness-cli/src/main.rs:9698`). The command prints the selected workflow store to stderr so stdout remains a single JSON result (`crates/harness-cli/src/main.rs:9393`, `crates/harness-cli/src/main.rs:9397`, `crates/harness-cli/src/main.rs:9404`).

`--resume` is intentionally strict: the prior run must exist and carry the identical snapshotted script, otherwise replay is rejected (`crates/harness-cli/src/main.rs:9614`, `crates/harness-cli/src/main.rs:9622`, `crates/harness-cli/src/main.rs:9633`, `crates/harness-cli/src/main.rs:9636`). The replay cache includes only completed steps with ordinals; failed leaves rerun (`crates/harness-cli/src/main.rs:9522`, `crates/harness-cli/src/main.rs:9536`, `crates/harness-cli/src/main.rs:9539`).

## Journaling

A `run-script` invocation journals into the harness store in this order:

```text
append WorkflowRun(status=running)
  -> for each real provider leaf: append WorkflowStep(status=running)
  -> provider worker runs
  -> append terminal WorkflowStep(status=completed|failed)
  -> append terminal WorkflowRun(status=completed|failed)
```

The run id is minted before evaluation so real leaves can journal live step rows as they start (`crates/harness-cli/src/main.rs:9709`, `crates/harness-cli/src/main.rs:9711`). The initial `WorkflowRun` row is appended with status `Running`, args, initiator, script spec, trace retention, host pid, and dry-run marker (`crates/harness-cli/src/main.rs:9713`, `crates/harness-cli/src/main.rs:9716`, `crates/harness-cli/src/main.rs:9722`, `crates/harness-cli/src/main.rs:9729`, `crates/harness-cli/src/main.rs:9733`, `crates/harness-cli/src/main.rs:9741`, `crates/harness-cli/src/main.rs:9744`, `crates/harness-cli/src/main.rs:9747`, `crates/harness-cli/src/main.rs:9749`).

The real driver appends a `WorkflowStep(status=Running)` before spawning the provider, stamped with a `provider_session_id` so the dashboard can attach live turn events mid-flight (`crates/harness-cli/src/main.rs:7214`, `crates/harness-cli/src/main.rs:7222`, `crates/harness-cli/src/main.rs:7225`, `crates/harness-cli/src/main.rs:7232`, `crates/harness-cli/src/main.rs:7241`). When the leaf completes, the driver appends the terminal step immediately; finalize recognizes already-journaled step ids and does not double-write them (`crates/harness-cli/src/main.rs:7292`, `crates/harness-cli/src/main.rs:7297`, `crates/harness-cli/src/main.rs:9841`, `crates/harness-cli/src/main.rs:9845`).

`journal_workflow_outcome()` finalizes the run by appending ordered step ids, terminal status, ended time, summary, agent count, and final output (`crates/harness-cli/src/main.rs:9824`, `crates/harness-cli/src/main.rs:9855`, `crates/harness-cli/src/main.rs:9859`, `crates/harness-cli/src/main.rs:9861`, `crates/harness-cli/src/main.rs:9865`, `crates/harness-cli/src/main.rs:9866`). If `HARNESS_WORKFLOW_ON_COMPLETE` is set, the terminal run is passed to that hook after journaling (`crates/harness-cli/src/main.rs:9548`, `crates/harness-cli/src/main.rs:9551`, `crates/harness-cli/src/main.rs:9867`).

## Store Records

### `WorkflowRun`

`WorkflowRun` is the durable record for one workflow invocation (`crates/harness-core/src/lib.rs:2607`, `crates/harness-core/src/lib.rs:2611`).

| Field | Meaning |
| --- | --- |
| `id` | Run id |
| `workflow_name` | Registered workflow name or Starlark header name |
| `status` | `pending`, `running`, `paused`, `completed`, or `failed` |
| `step_ids` | Ordered step ids for the run |
| `created_at` | Creation time |
| `ended_at` | Terminal time, if finished |
| `summary` | Terminal human summary |
| `args` | Optional JSON args for dynamic runs |
| `agents_spawned` | Count of spawned agent leaves attributed to this run |
| `final_output` | Terminal structured output |
| `initiated_by` | Agent member id or `operator` |
| `design_intent` | Required Starlark design intent for dynamic runs |
| `spec` | Dynamic run spec, usually `{"lang":"starlark","script": ...}` |
| `trace_retention` | `durable`, `live`, or later retention states |
| `host_pid` | Driver process id for abandoned-run reaping |
| `dry_run` | Whether provider execution was mocked |

The fields are defined at `crates/harness-core/src/lib.rs:2612` through `crates/harness-core/src/lib.rs:2675`. `step_ids` order the steps so the journal alone can reconstruct the run (`crates/harness-core/src/lib.rs:2607`, `crates/harness-core/src/lib.rs:2608`). `design_intent` and `spec` are populated for dynamic `run-script` rows, while registry runs may leave them empty (`crates/harness-core/src/lib.rs:2641`, `crates/harness-core/src/lib.rs:2647`).

### `WorkflowStep`

`WorkflowStep` is one agent leaf inside a run (`crates/harness-core/src/lib.rs:2684`, `crates/harness-core/src/lib.rs:2688`).

| Field | Meaning |
| --- | --- |
| `id` | Step id |
| `run_id` | Owning `WorkflowRun.id` |
| `phase` | Declarative phase |
| `label` | Step label |
| `provider_session_id` | Linked provider session, if delivery reached a provider |
| `status` | `queued`, `running`, `completed`, `failed`, or `cached` |
| `output_summary` | Human-facing step output |
| `result` | Structured machine payload from `step_result_json()` |
| `started_at` | Step start time |
| `ended_at` | Terminal time, if finished |
| `task_id` | Goal task id when the goal phase compiler/linker stamps it |
| `verdict_outcome` | Phase verdict marker used by the goal orchestrator |

The fields are defined at `crates/harness-core/src/lib.rs:2689` through `crates/harness-core/src/lib.rs:2713`. `WorkflowStep.run_id` points back to the run; `WorkflowRun.step_ids` preserves ordered membership. `provider_session_id` links the step to the provider session produced by the leaf (`crates/harness-core/src/lib.rs:2684`, `crates/harness-core/src/lib.rs:2686`, `crates/harness-core/src/lib.rs:2690`, `crates/harness-core/src/lib.rs:2694`).

## Provider-Neutral Execution

A workflow leaf names a provider, not a pre-existing `AgentMember`. The real driver spins up one-shot ephemeral workers and reduces them into `StepResult` (`crates/harness-cli/src/main.rs:7321`, `crates/harness-cli/src/main.rs:7325`, `crates/harness-cli/src/main.rs:7615`). The provider registry is the single source of supported provider ids and currently includes Codex, Claude, and Kimi (`crates/harness-cli/src/main.rs:14905`, `crates/harness-cli/src/main.rs:14907`, `crates/harness-cli/src/main.rs:14911`, `crates/harness-cli/src/main.rs:14918`).

Codex leaves run through `codex exec`, receive `--cd`, sandbox selection, JSON stream output, optional `-m`, effort config, images, extra dirs, and optional output schema (`crates/harness-cli/src/main.rs:8388`, `crates/harness-cli/src/main.rs:8419`, `crates/harness-cli/src/main.rs:8421`, `crates/harness-cli/src/main.rs:8423`, `crates/harness-cli/src/main.rs:8431`, `crates/harness-cli/src/main.rs:8437`, `crates/harness-cli/src/main.rs:8440`, `crates/harness-cli/src/main.rs:8446`, `crates/harness-cli/src/main.rs:8449`). Claude leaves run through `claude -p`, stream JSON, use an allowed-tools gate, support `--json-schema`, `--model`, `--effort`, `--fallback-model`, and `--add-dir` (`crates/harness-cli/src/main.rs:8521`, `crates/harness-cli/src/main.rs:8560`, `crates/harness-cli/src/main.rs:8563`, `crates/harness-cli/src/main.rs:8568`, `crates/harness-cli/src/main.rs:8582`, `crates/harness-cli/src/main.rs:8585`, `crates/harness-cli/src/main.rs:8588`, `crates/harness-cli/src/main.rs:8592`, `crates/harness-cli/src/main.rs:8595`). Kimi is registered through the same adapter interface, but its `-p --output-format stream-json` surface is flatter and currently carries no usage/model/cost/structured frame (`crates/harness-cli/src/main.rs:14660`, `crates/harness-cli/src/main.rs:14766`, `crates/harness-cli/src/main.rs:14771`, `crates/harness-cli/src/main.rs:14793`, `crates/harness-cli/src/main.rs:14863`, `crates/harness-cli/src/main.rs:14868`).

The selected project root, not the long-running harness process cwd, is the shared worker cwd and worktree base. The centralized store root remains separate (`crates/harness-cli/src/main.rs:7126`, `crates/harness-cli/src/main.rs:7127`, `crates/harness-cli/src/main.rs:7130`, `crates/harness-cli/src/main.rs:7531`, `crates/harness-cli/src/main.rs:7543`, `crates/harness-cli/src/main.rs:7566`, `crates/harness-cli/src/main.rs:7569`).

## Worktree Isolation

Workflow leaves are read-only by default: `writable=False` is the Starlark default (`crates/harness-workflow/src/starlark_front.rs:866`). A writable leaf may edit files and run shell, and it automatically isolates into a harness-owned throwaway git worktree (`crates/harness-workflow/src/lib.rs:70`, `crates/harness-workflow/src/lib.rs:72`).

Read-only is enforced per provider, not assumed. codex (`--sandbox read-only`) and claude (the `Read,Grep,Glob` tool allowlist) physically prevent a read-only leaf from writing, so they run in the shared cwd. kimi's `kimi -p` has no read-only mode (it rejects every permission flag), so a "read-only" kimi leaf could otherwise edit the live tree. The leaf runner reads each provider's `enforces_read_only` capability and isolates a read-only leaf into a throwaway worktree when its provider can't enforce read-only — so the worktree, not provider trust, is the boundary (`step_needs_isolation`, `provider_enforces_read_only`).

The worktree path is unique per run, node label, and provider session id, so same-label concurrent writable leaves do not collide (`crates/harness-cli/src/main.rs:7409`, `crates/harness-cli/src/main.rs:7415`, `crates/harness-cli/src/main.rs:7419`). The guard removes the worktree and temporary branch on drop (`crates/harness-cli/src/main.rs:7395`, `crates/harness-cli/src/main.rs:7496`, `crates/harness-cli/src/main.rs:7502`, `crates/harness-cli/src/main.rs:7507`). The diff is captured before cleanup and stored as leaf evidence/telemetry (`crates/harness-cli/src/main.rs:7798`, `crates/harness-cli/src/main.rs:7803`, `crates/harness-cli/src/main.rs:7867`).

Writable or isolated leaves require a git-backed project. Non-git projects fail before worktree creation with an actionable message; read-only leaves can still run (`crates/harness-cli/src/main.rs:7646`, `crates/harness-cli/src/main.rs:7652`). `WorktreeGuard::create` also checks git-ness and reports how to recover by making the step read-only or running from a git repo (`crates/harness-cli/src/main.rs:7436`, `crates/harness-cli/src/main.rs:7441`, `crates/harness-cli/src/main.rs:7443`).

Standalone `run-script` writable work is EPHEMERAL. It records the diff and discards the worktree, so in-repo artifacts are not present in the project after the run. Use the goal layer (`goal run-phases`) when writable task output must land on the branch, or retrieve captured text with `harness workflow get-output <run_id> --step <label>`.

Security note: the throwaway worktree only contains writes made inside that checkout. Absolute-path writes escape the worktree boundary and persist wherever that absolute path points, so workflows should not treat worktree isolation as a whole-machine sandbox.

## `run-script` Vs `goal run-phases`

`harness workflow run-script` is the raw dynamic runtime. It evaluates an authored `.star` file, journals a `WorkflowRun` plus `WorkflowStep` rows, and discards writable worktrees after capturing diffs.

`harness goal run-phases` is the goal-layer front-end onto the same runtime. It compiles each goal phase's task DAG into a Starlark program with `compile_phase_to_starlark()` (`crates/harness-core/src/lib.rs:651`). The compiler layers dependencies, groups pairwise-disjoint writable tasks into `parallel([...])`, emits singleton `agent(...)` calls, marks tasks with owned paths as `writable=True, isolation="worktree"`, and adds a structured acceptance judge plus `verdict(...)` when the phase has acceptance criteria (`crates/harness-core/src/lib.rs:640`, `crates/harness-core/src/lib.rs:641`, `crates/harness-core/src/lib.rs:643`, `crates/harness-core/src/lib.rs:645`, `crates/harness-core/src/lib.rs:770`, `crates/harness-core/src/lib.rs:775`, `crates/harness-core/src/lib.rs:790`, `crates/harness-core/src/lib.rs:908`, `crates/harness-core/src/lib.rs:916`).

During `goal run-phases`, the CLI writes the compiled script into the store, runs it through the workflow runtime, gates the outcome, links steps back to tasks, and records a phase verdict decision (`crates/harness-cli/src/main.rs:2396`, `crates/harness-cli/src/main.rs:2399`, `crates/harness-cli/src/main.rs:2410`, `crates/harness-cli/src/main.rs:2439`, `crates/harness-cli/src/main.rs:2471`, `crates/harness-cli/src/main.rs:2493`). If the phase passes, the goal layer lands writable work: it applies each captured worktree diff in deterministic ordinal order and makes one commit named `phase <phase_id> landed (run-phases)` (`crates/harness-cli/src/main.rs:2101`, `crates/harness-cli/src/main.rs:2102`, `crates/harness-cli/src/main.rs:2103`, `crates/harness-cli/src/main.rs:2104`, `crates/harness-cli/src/main.rs:2212`, `crates/harness-cli/src/main.rs:2217`). Passing phase landing is recorded back on the phase and orchestration run as `landed_commit` (`crates/harness-cli/src/main.rs:2517`, `crates/harness-cli/src/main.rs:2519`, `crates/harness-cli/src/main.rs:2530`).

The distinction is deliberate:

| Surface | Executes with | Writable leaf behavior | Landing authority |
| --- | --- | --- | --- |
| `workflow run-script` | Authored Starlark program | Capture diff, discard worktree | None |
| `goal run-phases` | Compiled phase DAG Starlark | Capture diff per task leaf | Goal layer applies and commits passing phase diffs |

## Invariants

1. Starlark is the only dynamic workflow authoring surface.
2. The workflow crate stays provider-neutral; provider CLIs live behind injected drivers/adapters.
3. Scripts must declare `workflow(name, design_intent)` before they are accepted.
4. Host control flow is deterministic; only `agent()` leaves are nondeterministic.
5. `WorkflowRun` and `WorkflowStep` are the canonical journal for workflow execution.
6. A provider leaf names a provider, not a pre-existing durable `AgentMember`.
7. `writable=True` implies throwaway worktree isolation.
8. Standalone workflow worktrees are evidence, not landing.
9. Goal-phase execution may land only after the goal layer's gates pass.
10. The selected project root defines worker cwd and worktree base; the store root defines journal location.

---
name: star-workflow
description: "Use when an agent needs to author a standalone Dynamic Workflow at runtime: write a Starlark program (loops, conditionals, data-driven fan-out) that calls agent()/parallel()/phase()/log() over the harness runtime, declare a mandatory workflow(name, design_intent) header, run it with `harness workflow run-script`, and read the resulting WorkflowRun/WorkflowStep records back from the dashboard snapshot or store."
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
names a PROVIDER (`"codex"`, `"claude"`, or `"kimi"`); the CLI spins up a NEW
one-shot ephemeral worker for that call (it does NOT deliver to a pre-existing
member) and journals a `WorkflowRun` plus one `WorkflowStep` per agent call —
identical to the built-in `workflow run --name` path, so the run shows up live on
the Agent Dashboard Workflows surface.

Each ephemeral worker is READ-ONLY by default and runs in the selected project
root — even on a provider (like kimi) that cannot physically enforce read-only,
provider capability gaps do not silently create worktrees (#190). A worker may
edit only when the call sets `writable=True`; standalone `run-script` then uses a
throwaway worktree and saves the diff as a pending `WorkflowPatch`, so the live
repo is not changed until the patch is applied. A read-only call can still opt
into `isolation="worktree"` when it explicitly needs an isolated checkout.
`write_mode="direct"` is the explicit exception for a simple serial leaf that
should edit the selected project root immediately.

## When To Use

- The shape of the orchestration is decided at runtime, not baked into a built-in
  registry workflow.
- You want to fan work out (scan, then parallel fix; map/reduce; a data-driven
  fan-out whose width depends on a prior step) and have the run recorded as
  evidence.
- A Lead Agent wants a worker to both design and execute a small multi-agent plan.

Do not use this skill to do the domain work yourself. Use it to author the
program, run it, and then read the recorded run.

## Product Boundary

Dynamic Workflow is a standalone executor. Its Starlark program owns internal
steps, fan-out, retries, structured output, and patch decisions. A host may
attach the completed `WorkflowRun`, its artifacts, and its result to an outer
Mission/Wave gate, but the workflow does not create or require a dependency graph,
agent-team assignment, or compatibility lifecycle record.

Do not reconstruct a workflow as a second plan after it runs. Treat the
program, its `WorkflowRun`, `WorkflowStep` records, artifacts, and final
`verdict()`/`output()` as the executor's durable truth.

## Choose The Write Scenario

Pick the landing mode before writing the program. Most bugs in workflow authoring
come from mixing permission (`writable=True`) with landing semantics.

| Scenario | Use | Do not use |
| --- | --- | --- |
| Read-only scan / review / synthesis | Omit `writable`; use `schema=` for any branch decision. | Do not assume a read-only leaf can run arbitrary shell on every provider; Kimi cannot physically enforce read-only, so choose codex/claude when hard read-only enforcement matters (read-only leaves still run on the project root regardless — #190). |
| Failure-aware retry / fallback | Use `return_status=True` and inspect `ok`, `reason`, `detail`, and `structured` before retrying or aborting. | Do not parse prose to guess whether a leaf timed out, failed, or merely returned a negative domain verdict. |
| Normal code implementation | `writable=True`, default worktree landing, `owned_paths=[...]`, leave `persist_changes` unset or `"patch"`. Operator or workflow later applies/rejects the pending `WorkflowPatch`. | Do not rely on `expected_artifacts` to preserve code edits; it is for named files, not source diffs. |
| Workflow-internal apply/reject | Implement in one writable leaf, have that leaf return a compact diff/gate summary, review it with a schema'd read-only gate, then call `apply_patch(label, reason)` or `reject_patch(label, reason)`. | Do not auto-apply without a gate; if the reviewer cannot see the relevant evidence, leave the patch pending for manual review. |
| Simple direct edit | One serial `agent(..., writable=True, write_mode="direct")` against a clean git project. Use for small docs/config edits the operator intends to land in the current tree now. | Do not use direct mode inside `parallel()` / `pipeline()`, and do not split direct edit/repair across multiple writable leaves: after the first leaf dirties the repo, another direct leaf is refused. |
| Parallel fixes | `parallel([{..., "writable": True, "owned_paths": [...]}, ...])`; each slot gets its own worktree and patch. Use unique labels such as `fix:1`, `fix:2`. | Never use `write_mode="direct"` for concurrent mutations. Avoid duplicate labels if humans must apply/reject individual patches. |
| File/artifact generation | `writable=True`, `expected_artifacts=[...]`, and `artifact_manifest([...], artifact_root=..., write_roots=[...])`. | Do not write artifacts to absolute paths or outside declared roots; those writes escape worktree cleanup/accounting. |

Good starting points:
[`examples/direct-doc-edit.star`](examples/direct-doc-edit.star),
[`examples/patch-review-apply.star`](examples/patch-review-apply.star),
[`examples/pending-manual-review.star`](examples/pending-manual-review.star),
[`examples/failure-aware-retry.star`](examples/failure-aware-retry.star),
[`examples/artifact-manifest.star`](examples/artifact-manifest.star),
[`examples/scan-then-parallel-fix.star`](examples/scan-then-parallel-fix.star).

## Example Coverage Map

Each bundled example exists to teach one decision shape. Do not add a new example
unless it covers a scenario this map does not already make obvious.

| Example | Unique purpose | Use when |
| --- | --- | --- |
| `closed-loop.star` | Canonical read-only plan -> draft -> bounded verify/refine -> `output()` -> `verdict()` skeleton. | You are producing an answer/artifact in variables, not editing files. |
| `bug-hunt-verify.star` | Multi-modal bug finding plus skeptic-panel majority verification. | You need a review-quality defect hunt, not a single auditor. |
| `design-tournament.star` | Divergent/convergent design: understand constraints, generate orthogonal proposals, judge/synthesize one winner. | The solution space is wide and design quality varies run-to-run. |
| `assess-verify-synthesize.star` | Streaming `pipeline()` per item: assess -> adversarial verify -> synthesize, with no unnecessary barrier. | Each item should flow through stages independently. |
| `failure-aware-retry.star` | `return_status=True` with explicit timeout/failure/structured-result branching and fallback. | The workflow must retry, degrade, or abort based on how a leaf failed. |
| `direct-doc-edit.star` | The explicit `write_mode="direct"` exception for one small serial live-checkout edit. | The operator intentionally wants the current repo changed now. |
| `patch-review-apply.star` | Default standalone code landing: writable worktree patch, schema'd review, apply/reject/pending decision. | Code should be reviewed before landing in the selected repo. |
| `pending-manual-review.star` | Dedicated pending path: create a patch, make no internal apply/reject call, output operator commands. | Evidence is insufficient for an automated decision but the workflow should complete. |
| `build-and-gate.star` | One writable worker owns edit/test/repair inside its worktree; Starlark gates the worker report. | The edit and gate must share one temporary checkout. |
| `artifact-manifest.star` | Generated file copy-back plus durable artifact manifest tracking. | The important output is a file/report/asset, not only a source diff. |
| `scan-then-parallel-fix.star` | Runtime-determined fan-out into multiple isolated writable patches. | A scan decides how many independent fixes should run concurrently. |

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
([`crates/harness-workflow/src/starlark_front.rs`](../../crates/harness-workflow/src/starlark_front.rs)),
the same dialect Bazel uses. It is HERMETIC by design: the script has no clock, no
randomness, and no IO. The orchestration (which agents run, in what order, with what
prompts) is therefore deterministic — the ONLY nondeterminism lives in the journaled
`agent()` leaves.

**Identifiers must be ASCII** (a Starlark/Python-2 rule): a non-ASCII variable name
like `要点 = agent(...)` fails to PARSE (`invalid input 要`). Non-ASCII text is fully
supported everywhere it belongs — inside string LITERALS: prompts, `label=`, the
`workflow(name, design_intent)` strings, and of course the agents' output. So write a
fully Chinese workflow freely; just keep variable names ASCII (`scan = agent("用中文…")`,
not `扫描 = agent(...)`).

A program calls these globals (no `import`; they are pre-bound):

| Call | Returns | Meaning |
| --- | --- | --- |
| `workflow(name, design_intent, budget_usd=, success_criterion=)` | — | REQUIRED header. Declares the run name + the WHY behind its shape. Optional `budget_usd=N` caps the run's cumulative spend; `success_criterion="..."` declares the bar `verdict()` is judged against. Must run once before the body. |
| `agent(prompt, provider="codex", label=, phase=, model=, effort=, service_tier=, fallback_model=, timeout_s=, image=, add_dir=, expected_artifacts=, persist_changes=, write_mode=, owned_paths=, artifact_root=, write_roots=, auto_apply_on_verdict=False, isolation=, schema=, schema_strict=False, return_status=False, writable=False)` | output text, dict, or status dict | Run ONE ephemeral worker synchronously. `prompt` is positional; the rest are keyword args. `model=` overrides the provider default model; `effort=` overrides reasoning effort; `service_tier=` overrides the Codex CLI service tier; `fallback_model=` sets a provider fallback when supported; `timeout_s=` is a per-leaf wall-clock cap; `expected_artifacts=` validates output files; `persist_changes="discard"` opts out of patch capture; `write_mode="direct"` edits the selected repo for a serial writable leaf; `owned_paths=` guards patch apply. READ-ONLY by default; `writable=True` permits edits and normally uses a throwaway worktree. With `schema={...}` it returns a parsed dict. `schema_strict=True` rejects candidates whose top-level string fields are all empty and requires `schema=`. With `return_status=True`, it returns an inspectable status dict. |
| `parallel([dict, ...])` | list (input order) | Barrier fan-out. Specs may set the same leaf options as `agent()`, including `schema_strict`; `write_mode="direct"` is rejected for concurrent edits. |
| `pipeline(items, stages)` | list (one per item) | No-barrier streaming. Stage specs may set the same leaf options as `agent()`, including `schema_strict`; `write_mode="direct"` is rejected here. |
| `apply_patch(label, reason="")` / `reject_patch(label, reason="")` | — | Declare a workflow-internal decision over a captured patch. The CLI applies or rejects the durable `WorkflowPatch` after the standalone `run-script` journals it, with the same guards as manual `workflow patch apply/reject`. |
| `artifact_manifest(paths, label=, artifact_root=, write_roots=)` | — | Declare durable artifact files to validate into `WorkflowArtifactManifest` rows with exists/size/hash/status. |
| `verdict(ok, reason="")` | — | Declare the run's TYPED outcome. `reason` may be positional or keyword (`verdict(ok, "why")` ≡ `verdict(ok, reason="why")`). `ok=False` finalizes the run `Failed` even if every worker ran — so "workers ran" ≠ "intent satisfied". A closed-loop program's final gate calls this. |
| `output(value)` | — | Declare the run's RESULT — the one unambiguous answer the calling agent reads back. `value` (a string or dict) is persisted verbatim under `final_output.result`, UNCAPPED, so the caller reads one field instead of digging the answer out of a step by label. Last call wins; pass a `schema`'d dict when you want the answer typed (a free-text `agent()` return is the worker's FULL reply — not truncated). |
| `json.encode(value)` / `json.decode(str)` | string / value | Serialize a prior `agent()`'s dict to inject it verbatim into the next prompt (forward-injection), or parse JSON back. |
| `phase(name)` | — | Set the default phase for the steps that follow. |
| `log(message)` | — | Emit a progress line (persisted in the run's `final_output.logs`). |
| `args` | value | The `--args` JSON, injected as a module global (e.g. `args["items"]`). |

Rules every call obeys:

- `provider` is `"codex"`, `"claude"`, or `"kimi"` — the provider whose ephemeral
  worker runs the leaf. There is NO member binding; the provider drives delivery.
  `"kimi"` (Kimi Code) is registry-routed like the others, but its headless `kimi -p`
  surface is leaner: no native schema / effort / service-tier / budget flags and
  a flat reply stream, so `schema=` degrades to text-extraction and `effort=`,
  `service_tier=`, token, and cost come back empty. Kimi also does not physically
  enforce a read-only sandbox, but read-only Kimi leaves still run in the selected
  project root rather than forcing a worktree. Use codex or claude when hard
  read-only enforcement matters; keep schema-gated control-flow leaves on codex
  or claude.
- `prompt`, `label`, and `phase` are non-empty strings; optional `model` (any
  non-empty string) overrides the provider's default model — route a CHEAP model
  to read-only verify/review steps and the strong model to the builder. The value
  is passed VERBATIM to the provider CLI and is NOT validated by the harness (an
  unknown name is rejected by the provider), and the supported set is NOT
  hardcoded — it changes as each provider ships new models. Don't rely on a
  baked-in list; discover the current models from the provider's own CLI:
  - codex — `codex --help` (the `-m/--model` flag) plus `~/.codex/config.toml`
    (`model`, `[profiles.*]`, `[model_providers.*]`) for what's configured.
  - claude — `claude --help`; `--model` takes a latest-model ALIAS
    (e.g. `sonnet` / `opus` / `haiku`) or a full model id.
  - kimi — `kimi provider list` (configured providers, model aliases, and the
    default) and `kimi provider catalog` to discover/import more.
- `effort` overrides the worker's reasoning effort, passed through verbatim to the
  provider — codex accepts `minimal|low|medium|high` (mapped to `-c
  model_reasoning_effort=…`), claude accepts `low|medium|high|xhigh|max` (mapped to
  `--effort …`). Use a low effort on cheap mechanical leaves and a high effort on the
  hard reasoning step. Not validated by the runtime — the provider CLI rejects a
  value it does not know, so use each provider's own levels.
- `service_tier` overrides Codex's speed/cost tier for this leaf, passed through
  verbatim as `-c service_tier=<value>` (for example `default`, `priority`, or
  `flex`, depending on the installed Codex CLI). Leave it unset to inherit
  `~/.codex/config.toml`; claude and kimi ignore it.
- `image` is a list of image file paths attached to the worker. Codex receives
  repeatable `-i <file>` args; claude `-p` has no image flag, so the paths are
  injected into the prompt for the worker to open with the Read tool.
- `add_dir` is a list of extra directory paths the worker may access. Codex and
  claude both receive repeatable `--add-dir <path>` args.
- `expected_artifacts` is a list of repo-relative files the step must produce;
  the runtime rejects absolute paths and `..` components.
- `persist_changes` controls writable diffs: the default saves non-empty diffs as
  pending `WorkflowPatch` rows; set `persist_changes="discard"` only when the diff
  is intentionally throwaway. Use `owned_paths=["src", "docs"]` on code-writing
  leaves so patch apply can reject path escapes. Patch capture only happens for a
  step that both SUCCEEDED (`ok`) and was declared `writable=True` — a failed
  leaf's diff is discarded, and a read-only leaf never produces a patch (the
  writable gate, not isolation, is the guarantee; read-only leaves run on the
  project root even on a provider that can't enforce read-only — #190; see
  [Workspace: read-only by default](#workspace-read-only-by-default-writabletrue-to-edit)).
  Starlark validation
  rejects `auto_apply_on_verdict=True` or `persist_changes="patch"` on a
  `writable=False` leaf, and rejects any `persist_changes` other than
  `"patch"`/`"discard"` or `write_mode` other than `"direct"`/absent, in
  `agent()`, `parallel()`, and `pipeline()` specs alike.
- `write_mode="direct"` is the explicit escape hatch for a simple SERIAL edit that
  should modify the selected project root immediately. It requires
  `writable=True`, a git-backed clean repo, and cannot be used in `parallel()` /
  `pipeline()` specs. It records `direct_diff` evidence and does NOT create a
  pending `WorkflowPatch` because the change is already in the working tree.
- `artifact_root` and `write_roots` are metadata for artifact manifests. Use
  `artifact_manifest([...], artifact_root="out", write_roots=["out"])` when a
  workflow produces files that should be visible and validated after completion.
- `return_status=True` changes the script-visible return into a dict with `ok`,
  `reason`, `detail`, `text`, `structured`, `provider_session_id`, `label`,
  `phase`, `provider`, `isolation`, and `ordinal`. Use it when the workflow needs
  to branch on timeout/failure categories or retry/abort deliberately instead of
  guessing from prose.
- `fallback_model` is an optional fallback model override. Claude receives
  `--fallback-model <model>`; codex has no fallback-model flag, so it is not
  passed to codex.
- The only supported `isolation` value is `"worktree"`.
- Reference `args` inside a prompt with `+` concatenation (`"audit " + args["area"]`)
  for short one-liners, or `.format()` into a triple-quoted string for longer /
  multi-line prompts — see [Writing prompt text](#writing-prompt-text-triple-quote-long-prompts).

### Writing prompt text: triple-quote long prompts

`+` concatenation is fine for a short one-liner, but a long or multi-line prompt
(a role brief, a numbered deliverable list, a report contract) is far more
readable as a **triple-quoted string** — standard Starlark, enabled here, and it
preserves newlines verbatim:

```python
res = agent(
    """You are a payments auditor. Audit {area}.

Look for, in order:
- missing idempotency keys on writes
- unhandled refund / chargeback races
- money paths that skip the ledger

Return a numbered list, one concrete finding per line as `file:line — issue`.""".format(area=args["area"]),
    schema={"items": "the findings, one per line"},
    label="audit",
)
```

`.format(name=value)` injects args (a clean alternative to `"… " + args["area"]
+ " …"`); `'''…'''` is the single-quote form. **The one gotcha: a triple-quoted
string keeps every character between the quotes, including leading indentation.**
So write the body flush-left even when the assignment is indented inside a `def`,
`if`, or comprehension — otherwise the indentation leaks into the prompt:

```python
def build_prompt(task):
    # WRONG — every line after the first carries 8 leading spaces into the prompt
    return """Implement {task}.
        Keep tests green.""".format(task=task)
    # RIGHT — body flush-left; only the first line sits on the return statement
    return """Implement {task}.
Keep tests green.""".format(task=task)
```

(A stray leading newline from opening `"""` on its own line is usually harmless —
strip it with `.strip()` /  `.lstrip("\n")` if a worker is whitespace-sensitive.)

**Starlark does NOT auto-join adjacent string literals.** Python concatenates
`"a" "b"` into `"ab"`; Starlark rejects it as a parse error (`unexpected string
literal … expected one of "+", …`). So to break a long single-line string across
source lines you must use explicit `+` or a triple-quoted block — never bare
adjacent strings:

```python
# WRONG — parse error in Starlark (this is a Python-only convenience)
workflow("x", "first part "
              "second part")
# RIGHT — explicit + , or a triple-quoted string
workflow("x", "first part " +
              "second part")
```

### Workspace: read-only by default, `writable=True` to edit

Every call is READ-ONLY by default and runs in the selected project root — the
worker may read files and run searches but must not edit files or run shell. This
is the safe default for the common case (finders, reviewers, verifiers,
synthesizers all only read). The runtime does not create a worktree merely
because the provider cannot physically enforce read-only; choose codex/claude for
hard read-only enforcement.

A call that must EDIT files or run commands sets `writable=True`. That worker is
automatically run in its own harness-owned throwaway git worktree under
`.harness/worktrees/` (writes land in a discardable checkout, NOT the live repo);
its `git diff` becomes the step's evidence, and the worktree is cleaned up after
(auto-removed if unchanged, never auto-merged directly). A non-empty writable diff
is saved as a pending `WorkflowPatch` unless the step sets
`persist_changes="discard"`. So a `parallel()` block of several `writable` slots is
automatically conflict-free — each gets its own worktree and its own patch.

For a deliberately simple serial edit, use direct mode:

```python
agent(
    "Make the requested one-file docs edit. Do not touch unrelated files.",
    label="direct-doc-edit",
    writable=True,
    write_mode="direct",
)
```

Direct mode separates **permission** from **landing**: `writable=True` gives the
provider edit/shell permission; `write_mode="direct"` says those edits land in
the selected project root immediately. The runtime refuses direct mode if the repo
is dirty before the step, if the project is non-git, or if the spec is inside
`parallel()` / `pipeline()`. Use it for small operator-intended edits where
normal git status/review is enough. Use the default worktree+patch path for code
changes, parallel edits, uncertain prompts, or anything that needs review before
landing.

When a writable leaf PRODUCES FILES that must survive cleanup, declare
`expected_artifacts=["repo/relative/path.ext", ...]`. At step end the CLI copies
each declared non-empty file from the throwaway worktree into the live repo before
cleanup; a missing, empty, absolute, or `..` path marks that step FAILED. An empty
list preserves legacy behavior. Use this for file-producing leaves such as image
generation: it both asserts the output exists and persists it past the ephemeral
worktree.

For code-writing leaves, prefer this shape:

```python
IMPLEMENT = {
    "gate_green": "bool",
    "summary": "what changed and why",
    "files_changed": "repo-relative files changed, one per line",
    "diff_review_notes": "compact diff/gate notes for the reviewer",
}
impl = agent(
    "Implement the focused change and do not touch unrelated files. Before " +
    "reporting, run the relevant gate and include compact diff/gate notes.",
    label="implement",
    writable=True,
    persist_changes="patch",
    owned_paths=["src", "tests"],
    schema=IMPLEMENT,
)
review = agent(
    "Review whether this implementation should be applied, rejected, or left " +
    "pending because evidence is insufficient:\n" + json.encode(impl),
    label="review",
    schema={
        "action": "one of: apply | reject | pending",
        "reason": "why this action is correct",
    },
)

action = review["action"] if type(review) == "dict" else "pending"
reason = review["reason"] if type(review) == "dict" else "review did not return JSON"
if action == "apply":
    apply_patch("implement", review["reason"])
elif action == "reject":
    reject_patch("implement", reason)
else:
    action = "pending"
# action == "pending": make no patch call; the operator applies/rejects later.
```

Manual operator path: `harness workflow patch list`, `harness workflow patch show
<run_id> --step implement`, then `harness workflow patch apply <run_id> --step
implement` or `harness workflow patch reject <run_id> --step implement`. Without
`--allow-dirty`, apply refuses only when a path THIS patch touches already has
local modifications (or a to-be-created file already exists untracked) —
unrelated dirty/untracked files elsewhere in the repo no longer block it.

If the workflow-internal reviewer cannot see enough real evidence in the worker's
structured report, do not call `apply_patch()`; leave the patch pending for manual
inspection with `workflow patch show`.

A `writable` worker runs with **FULL permissions** — codex `--sandbox
danger-full-access`, claude `--permission-mode bypassPermissions` — so it can run
arbitrary shell, **`git add`/`git commit`**, install deps, and reach the network.
(Codex's `workspace-write` was deliberately NOT used: it blocks writes to `.git/`,
so commits failed with "sandbox denied .git".) The throwaway worktree — not an OS
sandbox — is the boundary: a `writable` prompt executes for real, so scope it, and
never point a workflow with destructive/money-moving `writable` steps at a tree you
care about.

Nested harness commands from an ephemeral leaf are store-isolated by default. The
runtime injects a session-local child store/home for `harness ...` commands run
inside a provider leaf, so a worker cannot accidentally create tasks, evidence,
proposals, or decisions in the parent project's central store. If a workflow
intentionally needs a leaf to mutate the canonical harness store, invoke
`run-script` with `HARNESS_WORKFLOW_ALLOW_STORE_MUTATION=1` and record that grant
in the task/evidence; otherwise keep store writes in the Lead process after the
workflow returns.

`isolation="worktree"` is the explicit form of the same thing (a read-only call
that still wants an isolated checkout); `writable=True` implies it. Do not set it
for ordinary read-only reviews or scans.

**The workflow's cwd must be a git repo for `writable` / `isolation="worktree"`
steps** — the throwaway worktree is created with `git worktree add`. In a non-git
directory such a step fails fast with an actionable error. Either run the workflow
from a git repo (`git init`), or keep the step READ-ONLY and retrieve its produced
text with `harness workflow get-output <run_id> --step <label>`. Under a
project-switching `serve`, the worktree base / worker cwd is the run's selected
project root (#147), not necessarily where the binary launched.

A writable / isolated leaf branches from the PROJECT-ROOT checkout's current
`HEAD`, not the cwd that launched `run-script`. The CLI prints the worktree base
(root + branch + short HEAD) when it creates one; keep the selected project root
on the intended branch or a writable leaf can branch from a stale base.

Standalone `run-script` still discards the temporary worktree, but not the diff:
the patch is durable in the store. Apply or reject it explicitly, or have the
workflow declare `apply_patch()` / `reject_patch()` after a review leaf.

## Structured Output: the foundation

A worker called WITHOUT `schema` returns free text — and you cannot reliably
branch on free text. Pass `schema={...}` and the worker is forced to reply with a
single JSON object carrying the schema's TOP-LEVEL KEYS, parsed back into a native
Starlark dict:

```python
res = agent(
    "Audit " + args["area"] + " and report whether it is safe to ship.",
    schema={"ok": "bool", "findings": "findings, one per line"},
)
# res is a real dict with native scalar types: res["ok"] is a bool.
if res == None:
    log("worker produced no valid JSON — skipping")
elif res["ok"]:
    log("clean")
else:
    raw = res["findings"] if type(res["findings"]) == "string" else ""
    for f in [x.strip() for x in raw.splitlines() if x.strip()]:
        log("finding: " + f)
```

The schema's KEYS are the contract; the VALUES (`"bool"`, `"list of strings"`)
are shape hints handed to the worker. The runtime appends a JSON-only
instruction, and if the first reply is not a valid JSON object with those keys it
re-runs the worker ONCE with a corrective nudge. If it STILL fails, the call
returns **`None`** (and the step is journaled as a schema failure). Always handle
`None`.

This is what makes verify / judge / synthesis reliable: a verifier that returns
`{"ok": bool}` is something you can branch on; a verifier that returns a paragraph
is something you have to guess at. Reach for `schema` on every leaf whose output
controls the workflow's CONTROL FLOW.

`parallel()` honours `schema` per-spec: a spec with a schema that parsed yields a
dict in the result list, an unschema'd (or unparsed) spec yields its summary
string — so guard with `type(x) == "dict"` when a fan-out mixes them.

**Field types — known type words COERCE; other hints stay strings.** In the flat
`{"key": "hint"}` form a hint that is a recognised scalar type word — `"bool"`,
`"int"`/`"integer"`, `"number"`/`"float"` — is enforced as that REAL JSON type, so
`{"ok": "bool"}` gives you a real `True`/`False` and `{"n": "int"}` a real integer
(branch directly: `if res["ok"]:`, no string compare). Any OTHER hint (e.g.
`"the file path"`, `"list of strings"`) stays a STRING — the hint just guides the
worker and the runtime enforces the key is present. To get a LIST out of a leaf,
have it return items ONE PER LINE and `.splitlines()` the string field — the
robust, dry-run-safe idiom the examples use (`for x in res["items"].splitlines()`).
For hard array/enum/nested enforcement on a live run, pass a full JSON Schema dict
(`{"type": "object", "properties": {...}, "required": [...]}`) — it is enforced
natively, but `--dry-run`'s mock only fills the flat form, so prefer the
one-per-line idiom in examples that must run under `--dry-run`.

## Default to `pipeline()` over `parallel()`

`parallel()` is a BARRIER — it blocks until every spec finishes. `pipeline()` is
NOT: each item flows through all stages independently, so item A can be at stage 3
while item B is still at stage 1. When you have multi-stage PER-ITEM work (find →
verify each finding; assess → refute each dimension), reach for `pipeline()` first
— the wall-clock is the slowest single CHAIN, not the slowest stage summed over a
barrier. (See [`examples/assess-verify-synthesize.star`](examples/assess-verify-synthesize.star).)

Smell test: if you wrote `a = parallel(...)`, then a middle `transform(a)` that is
just a flatten / map / filter with NO cross-item dependency, then another
`parallel(...)`, you did not need that barrier — fold the transform into a
pipeline stage. A barrier is only correct when stage N genuinely needs ALL of
stage N-1 at once: a dedup/merge across the whole set, an early-exit on the total
count, or a judge that compares the items to each other.

## The Quality Patterns

A workflow earns its keep by CROSS-CHECKING, not by doing one big call. The
patterns below all lean on structured output. Each is a few lines of Starlark.

### verify + repair + stop

Draft the answer, verify it with a SEPARATE schema'd worker, and on failure make
exactly one repair pass — then stop. Bounded, not an open loop. For code edits,
keep the edit/gate/repair loop inside one writable worktree worker, or use the
patch review pattern; do not do `write_mode="direct"` build followed by a second
direct repair leaf, because direct mode requires the repo to be clean before each
direct leaf.

```python
artifact = agent("Draft the answer for " + args["task"], label="draft")
v = agent(
    "Verify this answer for " + args["task"] + ". Did it pass?\n" + artifact,
    schema={"ok": "bool", "problems": "problems, one per line"},
    label="verify",
)
if v != None and not v["ok"]:
    raw = v["problems"] if type(v["problems"]) == "string" else ""
    problems = [p.strip() for p in raw.splitlines() if p.strip()]
    artifact = agent(
        "Repair these problems in the answer:\n- " + "\n- ".join(problems) +
        "\n\nCURRENT ANSWER:\n" + artifact,
        label="repair",
    )
# else: it passed (or verify failed to report) — stop. No unbounded retry loop.
```

### adversarial verify (majority vote)

Do not ask one verifier "is this right?" — spawn N skeptics PER finding, each
prompted to REFUTE it, defaulting to refuted=true when unsure. Keep the finding
only if a MAJORITY do NOT refute. Survival-of-scrutiny beats a single rubber stamp.

```python
N = 3
panel = parallel([
    {
        "prompt": "Try to REFUTE this claim: \"" + finding + "\". If you cannot " +
                  "confirm it, set refuted=true.",
        "schema": {"refuted": "bool"},
        "label": "skeptic",
    }
    for _ in range(N)
])
refuted = 0
for v in panel:
    # Unsure / no-JSON defaults to refuted (conservative).
    if not (type(v) == "dict" and v["refuted"] == False):
        refuted += 1
keep = refuted * 2 < N   # majority did NOT refute
```

### perspective-diverse verify

Same fan-out, but give each verifier a DISTINCT lens instead of N identical
refuters — a finding that survives correctness AND security AND reproduction is
far stronger than one that survives three clones.

```python
lenses = ["is it logically correct?", "is it a security risk?", "does it actually reproduce?"]
checks = parallel([
    {
        "prompt": "Evaluate \"" + finding + "\" strictly on: " + lens,
        "schema": {"ok": "bool", "why": "string"},
        "label": "lens",
    }
    for lens in lenses
])
passed = [c for c in checks if type(c) == "dict" and c["ok"]]
keep = len(passed) * 2 > len(lenses)   # most lenses agree
```

### judge panel

Generate N INDEPENDENT attempts from different angles, score each with parallel
judges, then synthesize from the winner. Use it when the answer is open-ended and
quality varies run-to-run.

```python
angles = ["optimize for clarity", "optimize for performance", "optimize for safety"]
attempts = parallel([
    {"prompt": "Solve " + args["task"] + ", " + a, "label": "attempt"} for a in angles
])
scores = parallel([
    {
        "prompt": "Score this solution 0-10 for " + args["task"] + ":\n" + attempts[i],
        "schema": {"score": "int"},
        "label": "judge",
    }
    for i in range(len(attempts))
])
best, best_score = attempts[0], -1
for i in range(len(attempts)):
    s = scores[i]["score"] if type(scores[i]) == "dict" else -1
    if s > best_score:
        best, best_score = attempts[i], s
agent("Refine and finalize this winning solution:\n" + best, label="synthesize")
```

### loop-until-dry

Keep fanning out finders until K CONSECUTIVE rounds surface nothing new — a
`seen` set turns "find the bugs" into "find them all". Starlark has **no `while`
loop** (`while` is a reserved keyword that does not parse), so bound the hunt with
`for _ in range(MAX_ROUNDS)` and `break` once it converges. (`break` and
`continue` DO work in `for` loops; only `while` is missing.)

```python
seen = {}          # used as a set: finding -> True
dry_rounds = 0
K = 2              # stop after K consecutive empty rounds
MAX_ROUNDS = 8     # no `while` in Starlark: bound the loop, then break on convergence
for _ in range(MAX_ROUNDS):
    rounds = parallel([
        {"prompt": "Find bugs in " + args["area"] + " NOT in this list:\n" +
                   "\n".join(seen.keys()),
         "schema": {"findings": "findings, one per line"}, "label": "finder"}
        for _ in range(2)
    ])
    fresh = 0
    for r in rounds:
        if type(r) == "dict" and type(r["findings"]) == "string":
            for f in [x.strip() for x in r["findings"].splitlines() if x.strip()]:
                if f not in seen:
                    seen[f] = True
                    fresh += 1
    dry_rounds = dry_rounds + 1 if fresh == 0 else 0
    if dry_rounds >= K:
        break
log("converged with " + str(len(seen)) + " distinct findings")
```

### multi-modal sweep

The FIND-side counterpart of perspective-diverse verify: spawn finders that each
search a DIFFERENT WAY — by data flow, by failure mode, by entry point, by file —
each BLIND to what the others surface. One search angle never finds everything; a
sweep of orthogonal angles does. (The bug-hunt-verify example's three lenses are
exactly this.)

```python
angles = [
    "trace the data flow end to end and flag where it can go wrong",
    "enumerate every external call and its failure mode",
    "walk each public entry point and the inputs it fails to validate",
]
finds = parallel([
    {"prompt": "Audit " + area + " by this method ONLY: " + a +
               ". Report concrete issues, one per line as `file:line — issue`.",
     "schema": {"items": "the issues, one per line"}, "label": "sweep"}
    for a in angles
])
```

### completeness critic

End with one agent whose only job is to ask what is MISSING — the cheap final
guard against a confident-but-incomplete result.

```python
gaps = agent(
    "Here is the finding set for " + args["area"] + ":\n- " + "\n- ".join(seen.keys()) +
    "\nWhat important cases are still MISSING?",
    schema={"complete": "bool", "missing": "missing cases, one per line"},
    label="completeness",
)
if gaps != None and not gaps["complete"]:
    raw = gaps["missing"] if type(gaps["missing"]) == "string" else ""
    missing = [x.strip() for x in raw.splitlines() if x.strip()]
    log("gaps remain: " + ", ".join(missing))
```

## Error Tolerance

Workers fail or time out. `agent()` does NOT raise for that — it returns the
worker's (possibly empty) output, and in `schema` mode a failed/timed-out/garbled
worker returns **`None`**. So the script keeps running; YOU decide what a missing
result means:

- Schema'd leaf: check `if res == None: ...` and skip / default conservatively
  (e.g. count a missing skeptic vote as "refuted").
- `parallel()`: the result list is always input-length, but some slots may be
  `None` or a summary string — guard with `type(x) == "dict"` before indexing.

Never assume every slot in a fan-out succeeded; a robust workflow tolerates a
dead leaf and still reaches a verdict.

When the workflow needs to branch on the failure class itself, use
`return_status=True` instead of parsing text:

```python
status = agent(
    "Run a quick verifier for " + args["target"],
    label="verify",
    timeout_s=120,
    return_status=True,
)
if type(status) == "dict" and status["ok"] == False:
    if status["reason"] == "timeout":
        log("verifier timed out; run a cheaper fallback or mark coverage partial")
    else:
        log("verifier failed: " + status["reason"] + " " + status["detail"])
elif type(status) == "dict":
    log("verifier completed")
```

Use this for retry/abort/fallback logic. Use `schema={...}` when the worker
completed and the workflow needs typed domain facts.

## Right-size it, and never cap silently

Scale the STRUCTURE to what was asked. "find any bugs" wants a few finders and a
single verify pass; "thoroughly audit X" wants a larger finder pool, a 3–5-vote
adversarial pass, and a synthesis stage. Do not bring a tournament to a one-line
question, and do not bring a single pass to "be exhaustive." When unsure, lean
thorough for review/audit/research and brief for a quick check.

And when you DO bound coverage — top-N, no retry, sampling, a fixed round count —
`log()` what you dropped. A silent cap reads as "covered everything" when it did
not; a logged one (`log("scanned 50 of 120 files; stopped at the budget")`) keeps
the run honest about what it actually checked.

## When NOT To Use A Workflow

A single-step task is just one `agent()` call — wrapping it in a `workflow(...)`
program adds ceremony and buys nothing. A workflow's entire value is STRUCTURE:
parallelism, cross-checking (verify / adversarial / judge), or loops
(loop-until-dry). If your program has one `agent()` call and no branch, no
fan-out, and no loop, you do not want a workflow — you want that one call. Add the
structure only when the structure is the point.

## Worked Example: the CANONICAL closed-loop skeleton

The shape a non-trivial workflow should follow — it composes ALL the idioms in
one program: a leading typed **plan** injected forward (`json.encode`), a shared
**COMMON** preamble, a bounded **verify → refine loop** against a schema'd bar, a
**schema-gated branch** (control flow keys off `check["passed"]`, not prose), a
cheap model on the read-only verify step, a `budget_usd` ceiling, and a typed
**`verdict()`** so the run's status means *intent met*, not merely *workers ran*.
The runnable copy is [`examples/closed-loop.star`](examples/closed-loop.star); it
runs end-to-end under `--dry-run` (the verdict gate correctly reports `Failed`
when the bar is not met, even though every worker ran), and persists its `log()`
lines + verdict + criterion into the run's `final_output`.

```
harness workflow run-script ./closed-loop.star --args '{"task":"...","bar":"..."}' --max-budget-usd 5
```

A flat fan-out that only finds-and-reports is an ANTI-PATTERN: it cannot tell a
good run from an expensive single agent. Start from this skeleton and drop the
parts you do not need.

## Worked Example: a writable build with a gate loop

The WRITABLE counterpart of the closed loop. Because a `writable=True` worker runs
in its OWN throwaway worktree, a separate implement step and verify step do not
share a tree — so the whole edit → run-the-gate → fix loop lives INSIDE one
writable worker (fed a leading typed design), while the PLAN and the `verdict()`
stay in Starlark. Its build prompt is the internal bar: a role, the injected
design as ground truth, hard constraints (never weaken a test), numbered
deliverables, the exact gate command, and a report contract.
[`examples/build-and-gate.star`](examples/build-and-gate.star) — runs under
`--dry-run`.

## Worked Example: direct docs edit

[`examples/direct-doc-edit.star`](examples/direct-doc-edit.star) shows the
smallest direct-write shape: one serial writable leaf edits the selected project
root with `write_mode="direct"`, then a read-only reviewer checks the requested
target and the workflow declares a verdict. This is for small docs/config edits
where the operator intentionally wants the current checkout changed now. It is
not a review-before-apply flow; direct-mode changes are already in the working
tree.

## Worked Example: failure-aware retry

[`examples/failure-aware-retry.star`](examples/failure-aware-retry.star) shows
the P0 failure-control shape: a primary verifier uses `return_status=True`, the
script checks `ok`, `reason`, `detail`, and `structured`, and only then chooses a
fallback verifier. Use this whenever timeout/failure/malformed output should
change control flow; a negative domain verdict is not the same thing as a dead
leaf.

## Worked Example: worktree patch, review, apply/reject

[`examples/patch-review-apply.star`](examples/patch-review-apply.star) is the
default code-development shape for standalone `run-script`: implement in a
throwaway worktree, capture the diff as a pending `WorkflowPatch`, have a
schema'd review gate inspect the worker's diff/gate summary, then call
`apply_patch()`, `reject_patch()`, or no patch action. If the reviewer lacks
enough evidence, leave the patch pending for manual `workflow patch show` rather
than pretending reject means pending.

[`examples/pending-manual-review.star`](examples/pending-manual-review.star) is
the dedicated pending variant. It creates the same durable patch, but when the
review evidence is incomplete or operator approval is still required, it makes
no `apply_patch()` / `reject_patch()` call and outputs the exact
`workflow patch list/show/apply/reject` commands for the operator. The workflow
finishes; the patch state remains pending.

## Worked Example: artifact manifest

[`examples/artifact-manifest.star`](examples/artifact-manifest.star) shows the
file-producing shape: a writable worker creates a declared repo-relative artifact,
`expected_artifacts` copies it out of the throwaway worktree, and
`artifact_manifest()` records existence, size, hash, and current/missing/stale
status for the dashboard.

## Worked Example: bug hunt with adversarial verify

A quality workflow end to end: diverse schema'd finders fan out, every candidate
finding is cross-examined by a skeptic panel (majority must fail to refute), and
the confirmed set is synthesized. The runnable copy is
[`examples/bug-hunt-verify.star`](examples/bug-hunt-verify.star) — it composes
[structured output](#structured-output-the-foundation),
[adversarial verify](#adversarial-verify-majority-vote), and `None`-tolerant
flattening, and runs end-to-end under `--dry-run`.

## Worked Example: data-driven scan, then parallel fix

A serial read-only scan call whose output decides the fan-out width: one fix
slot per defect line. Because the fix slots EDIT files in parallel, each sets
`writable=True`, which implies its own throwaway worktree and pending patch. The
runnable copy is
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
    schema={"defects": "one defect per line"},
)
defects = []
if type(scan) == "dict" and type(scan["defects"]) == "string":
    defects = [x.strip() for x in scan["defects"].splitlines() if x.strip()]

phase("fix")
fix_results = parallel([
    {
        "prompt": "Fix this defect in " + args["area"] + ": " + defects[i] + ". Make the minimal change and explain it.",
        "provider": "codex",
        "label": "fix:" + str(i + 1),
        "writable": True,
        "persist_changes": "patch",
        "owned_paths": [args["area"]],
    }
    for i in range(len(defects))
])
patch_labels = ["fix:" + str(i + 1) for i in range(len(defects))]
output({"defects": "\n".join(defects), "pending_patch_labels": "\n".join(patch_labels)})
verdict(type(scan) == "dict", reason="created " + str(len(patch_labels)) + " pending patch(es)")
```

The `scan` phase completes before the
`parallel` barrier fans out, and each fix slot then runs concurrently in its own
worktree and joins before the run finalizes. Standalone `run-script` leaves one
pending patch per fix label; worktrees prevent checkout collisions, but patches
can still conflict later when applied to the live repo. The fan-out WIDTH is
decided at runtime from the scan's output — a comprehension over its lines —
which no static shape could express.

## Worked Example: a design tournament (divergent → convergent)

The fullest **divergent-then-convergent** shape — the pattern the real internal
design runs use. Two parallel TYPED probes map the domain + the constraints;
three complete designs are generated from orthogonal philosophies, **each seeded
with the understanding injected forward** (`json.encode`); then a judge scores
them on named dimensions and grafts ONE winner. Every handoff is a multi-field
schema, so each step reads typed fields, not prose.
[`examples/design-tournament.star`](examples/design-tournament.star) — runs under
`--dry-run`.

## Worked Example: assess → adversarial-verify → synthesize (`pipeline`)

Evaluation as an ADVERSARIAL DIALOGUE, streamed with `pipeline()`:
`pipeline(dimensions, assess, verify)` flows each dimension `assess → verify` with
NO barrier; the verifier is fed the assessment (`{input}` forward-injection) and
tries to REFUTE each claim, emitting a corrected verdict; then one report
synthesizes the VERIFIED verdicts. A single assessor over-claims — an independent
refuter that must consolidate a corrected verdict is what makes the synthesis
trustworthy. [`examples/assess-verify-synthesize.star`](examples/assess-verify-synthesize.star)
— runs under `--dry-run`.

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
| `--timeout-ms <ms>` | Per-worker **IDLE** timeout (default 900000 = 15 min). A worker is killed only after this long with NO output — a slow-but-streaming turn runs to completion however long it takes; only a SILENT (wedged provider / auth or network stall) worker is killed. NOT a total wall-clock cap. |
| `--max-budget-usd <amt>` | Per-run spend ceiling; once cumulative cost reaches it, further leaves short-circuit into failed `budget` steps (also settable via `workflow(budget_usd=…)`). |
| `--resume <prior_run_id>` | Re-run the SAME program reusing the prior run's SUCCEEDED leaves (no re-spend); fails if the script changed. |

Use `timeout_s=` for a per-leaf wall-clock cap; `--timeout-ms` remains the global idle-since-last-output timer and resets whenever the worker streams output.
| `--trace durable\|live` | Retain the heavy per-step turn-event trace (`durable`, default) or stream-only (`live`). |
| `--model <m>` | Run-wide default model; a per-call `model=` on an `agent()` overrides it. |
| `--effort <e>` | Run-wide default reasoning effort; a per-call `effort=` on an `agent()` overrides it. |
| `--progress` | Stream a compact NDJSON line per step (phase, label, `running`/`ok`/`failed`) to STDERR as the run executes — the phase-by-phase timeline — while STDOUT stays the single final JSON. |

The command prints the journaled run as JSON to STDOUT, including the new `run`
id and `run.final_output` (see the result-exit note below).

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

### The result exit: how the calling agent gets the answer

You (the calling agent) invoke `run-script` through your shell tool, so you read
its result the way you read any CLI tool: **its stdout becomes your tool result
when the command returns.** `run-script` prints `{"run": {...}, "steps": [...]}`,
and inside `run.final_output`:

- **`result`** — what your `output(value)` declared. **This is the run's answer**;
  read this one field. It is `null` if the script never called `output()`.
- `verdict` — `{ok, reason}` from `verdict()`: did the intent succeed.
- `success_criterion`, `logs` — the declared bar and the `log()` narration.
- `steps[]` — per-leaf `output_summary` (the worker's FULL reply, untruncated), `structured`, and
  telemetry, for audit.

So a foreground call gives you the whole timeline + answer at once:

```bash
harness workflow run-script ./prog.star --args '{...}' \
  | python3 -c 'import sys,json; print(json.load(sys.stdin)["run"]["final_output"]["result"])'
```

For LIVE tracking (which phase is running now), you cannot read a foreground
command's stdout mid-run — the shell tool returns it only on exit. Two options:
add **`--progress`** (NDJSON step events to stderr, which you still see in the
tool result) and/or run it in the **background** and poll `harness dashboard
snapshot` (the journal updates per step live). End any answer-producing program
with `output(...)` so the answer is one field, not a step picked by label.

### Observe a run: status + live progress

Pick by how live you need it:

- **Final result (foreground).** The `run-script` call blocks and returns the whole
  run as its tool result: `run.final_output.result` (the answer), `run.status`
  (`completed` / `failed`), the `verdict`, and `steps[]` (per-leaf `label`, `phase`,
  `provider`, `status`, `output_summary`). One call, whole timeline.
- **By id, any time / any shell.** `harness workflow get-output <run_id>` prints a
  run's ordered steps + status as JSON — `--text` for just the deliverable text,
  `--step <label>` to filter to one leaf. The `<run_id>` (`wfrun-…`) is in the
  run-script output; use this to inspect a run started in the background or another
  session. Each step includes the journaled `result` plus `session_summary`
  (`tool_calls` with counts, `final_message`, `retained`, `truncated`), so a
  "completed but no artifact" leaf is diagnosable: did the worker call the tool,
  and what did its final message say?
- **Live, phase-by-phase.** Add `--progress` for one NDJSON line per step
  (`phase`, `label`, `running` / `ok` / `failed`) on STDERR as it executes, and/or
  background the run and poll `harness dashboard snapshot` (`workflow_runs` +
  `workflow_steps` advance per step).
- **Visual / SSE.** Point a live server at the SAME store
  (`harness serve --store <path>`) and open the dashboard **Workflows** surface:
  every run, a per-step timeline (status, provider, `output_summary`), and "drill in"
  to a step's turn events, updating live over SSE. `--trace durable` (default)
  retains the per-node trace; `--trace live` is stream-only.
- **Completion hook (push — for a backgrounded run).** Set
  `HARNESS_WORKFLOW_ON_COMPLETE` to a shell command and the harness fires it the
  moment a run reaches a terminal status, passing `HARNESS_RUN_ID` /
  `HARNESS_RUN_STATUS` (`completed` / `failed`) / `HARNESS_RUN_NAME` as env vars and
  the full run JSON on stdin. It fires INSIDE the run-owning process at finalization,
  so a backgrounded `run-script &` notifies WITHOUT the caller polling. No-op when
  the var is unset; best-effort (a hook error is logged, never fails the run); keep
  the hook quick (the run waits for it) or self-detach with a trailing `&`. E.g.
  `HARNESS_WORKFLOW_ON_COMPLETE='harness message send --from lead --content "wf $HARNESS_RUN_ID $HARNESS_RUN_STATUS"' harness workflow run-script prog.star &`.
  Scope: this hook is for `run-script` (and the stale-run reaper).

## Permission Note

The agent that runs the program invokes the `harness` binary through its shell, so
its permission profile must allow it:

- The runner's allowed-tool / command policy must permit running the `harness`
  binary (for Claude this is a `Bash(harness ...)` allowance; for Codex the
  sandbox/approval policy must let the shell call through).
- Each agent call spins up a fresh ephemeral worker that is READ-ONLY in the
  selected project root by default (#190). `writable=True` grants edit tools/full
  provider permission; by default those writes go to a throwaway worktree.
  `write_mode="direct"` is the explicit choice to write the selected repo cwd
  immediately. A prompt that writes files or runs destructive / money-moving
  actions executes for real once writable — scope prompts accordingly and keep
  parallel mutations in worktrees.
- Nested `harness ...` commands inside an ephemeral worker write a session-local
  child store by default, not the parent project's central store. Use
  `HARNESS_WORKFLOW_ALLOW_STORE_MUTATION=1 harness workflow run-script ...` only
  when the leaf is explicitly trusted to mutate canonical harness objects.
- If `harness` is not on the runner's `PATH`, invoke it by absolute path and
  ensure that path is the allowed command.

## Checklist

- [ ] Program declares `workflow(name, design_intent)` once, before the body, with a real (>= ~20 char) design_intent.
- [ ] Program calls only `workflow`/`agent`/`parallel`/`pipeline`/`phase`/`log`/`verdict`/`output`/`args`; no clock/random/IO assumed.
- [ ] A program that produces an answer ends with `output(value)` so the caller reads `final_output.result` (a large answer goes through a `schema`'d dict, not capped free text).
- [ ] Every agent leaf (`agent()` call / `parallel` spec) has a `provider` of `"codex"`, `"claude"`, or `"kimi"` (keep `schema=`-gated control-flow leaves on codex/claude).
- [ ] Parallel slots that EDIT files use worktree isolation; never use direct mode in parallel/pipeline.
- [ ] Writable code leaves declare `owned_paths`; leave default patch capture on unless the diff is intentionally throwaway.
- [ ] Store mutation is explicit: ephemeral leaves do not write parent harness
      objects unless the run is intentionally invoked with
      `HARNESS_WORKFLOW_ALLOW_STORE_MUTATION=1`; otherwise the Lead records
      evidence/proposals/decisions after reading the workflow result.
- [ ] Writable file-producing leaves declare `expected_artifacts` and/or `artifact_manifest` with repo-relative paths.
- [ ] If a workflow should decide landing internally, it has a review/gate leaf and calls `apply_patch(label, reason)` or `reject_patch(label, reason)` only for explicit apply/reject; insufficient evidence leaves the patch pending.
- [ ] Every leaf whose output drives control flow uses `schema={...}` and the script handles a `None` (and, in fan-outs, a non-dict) result.
- [ ] Any branch that depends on timeout/provider failure uses `return_status=True` and checks `ok` / `reason` / `detail` rather than parsing prose.
- [ ] The standalone landing decision is explicit: `run-script` creates pending patches unless the script applies or rejects them, while `write_mode="direct"` changes the selected working tree immediately.
- [ ] If the workflow has only one `agent()` call and no branch/fan-out/loop, it is NOT a workflow — collapse it to that one call.
- [ ] Quality steps (verify / adversarial / judge / loop-until-dry / completeness) cross-check rather than trust a single pass, where the task warrants it.
- [ ] Ran it: `harness workflow run-script <prog.star>` (no member binding needed).
- [ ] The run is visible in `harness dashboard snapshot` (`workflow_runs` / `workflow_steps`) with its `design_intent`.
- [ ] The runner's profile allows the `harness` binary and what each leaf's prompt does.

## Maintaining This Skill

When updating this repository's `skills/star-workflow` copy, sync the installed
copies before accepting the task:

```bash
skills/star-workflow/scripts/sync-installed.sh
```

The script updates `$CODEX_HOME/skills/star-workflow` (or `~/.codex/skills` by
default) and `~/.agents/skills/star-workflow`, diffs both installed copies
against the repo copy, and runs `quick_validate.py` when the validator is
available. If an update should not be installed, record that as explicit evidence
and explain why.

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
| `agent(prompt, provider="codex", label=, phase=, model=, isolation=, schema=, writable=False)` | output text, OR a dict (with `schema=`) | Run ONE ephemeral worker synchronously. `prompt` is positional; the rest are keyword args. READ-ONLY by default; `writable=True` lets it edit / run shell AND auto-isolates it into a throwaway worktree. With `schema={...}` it returns a parsed dict (or `None`) — see [Structured Output](#structured-output-the-foundation). Capture the return to chain: `scan = agent("...")`. |
| `parallel([dict, ...])` | list (input order) | Barrier fan-out: run every spec concurrently, block until ALL finish. Each element is the parsed dict (if that spec had a `schema` that parsed) else its output string. Each dict needs a `prompt` and may set `provider` (default `"codex"`), `label`, `phase`, `model`, `isolation`, `schema`, `writable`. |
| `pipeline(items, stages)` | list (one per item) | No-barrier streaming: each item flows through every stage independently. `stages` is a list of dicts `{prompt, provider?, model?, schema?, writable?}` whose `prompt` is a TEMPLATE containing `{input}` — replaced with the item for stage 1, then the prior stage's output for each next stage (forward-injection). Returns each item's LAST stage result. |
| `verdict(ok, reason="")` | — | Declare the run's TYPED outcome. `ok=False` finalizes the run `Failed` even if every worker ran — so "workers ran" ≠ "intent satisfied". A closed-loop program's final gate calls this. |
| `output(value)` | — | Declare the run's RESULT — the one unambiguous answer the calling agent reads back. `value` (a string or dict) is persisted verbatim under `final_output.result`, UNCAPPED, so the caller reads one field instead of digging the answer out of a step by label. Last call wins; pass a `schema`'d dict when you want the answer typed (a free-text `agent()` return is the worker's FULL reply — not truncated). |
| `json.encode(value)` / `json.decode(str)` | string / value | Serialize a prior `agent()`'s dict to inject it verbatim into the next prompt (forward-injection), or parse JSON back. |
| `phase(name)` | — | Set the default phase for the steps that follow. |
| `log(message)` | — | Emit a progress line (persisted in the run's `final_output.logs`). |
| `args` | value | The `--args` JSON, injected as a module global (e.g. `args["items"]`). |

Rules every call obeys:

- `provider` is `"codex"` or `"claude"` — the provider whose ephemeral worker
  runs the leaf. There is NO member binding; the provider drives delivery.
- `prompt`, `label`, and `phase` are non-empty strings; optional `model` (any
  non-empty string) overrides the provider's default model — route a CHEAP model
  to read-only verify/review steps and the strong model to the builder.
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

Every call is READ-ONLY by default — the worker may read files and run searches
but CANNOT edit files or run shell. This is the safe default for the common case
(finders, reviewers, verifiers, synthesizers all only read).

A call that must EDIT files or run commands sets `writable=True`. That worker is
automatically run in its own harness-owned throwaway git worktree under
`.harness/worktrees/` (writes land in a discardable checkout, NOT the live repo);
its `git diff` becomes the step's evidence, and the worktree is cleaned up after
(auto-removed if unchanged, never auto-merged). So a `parallel()` block of several
`writable` slots is automatically conflict-free — each gets its own worktree.

A `writable` worker runs with **FULL permissions** — codex `--sandbox
danger-full-access`, claude `--permission-mode bypassPermissions` — so it can run
arbitrary shell, **`git add`/`git commit`**, install deps, and reach the network.
(Codex's `workspace-write` was deliberately NOT used: it blocks writes to `.git/`,
so commits failed with "sandbox denied .git".) The throwaway worktree — not an OS
sandbox — is the boundary: a `writable` prompt executes for real, so scope it, and
never point a workflow with destructive/money-moving `writable` steps at a tree you
care about.

`isolation="worktree"` is the explicit form of the same thing (a read-only call
that still wants an isolated checkout); `writable=True` implies it.

**The workflow's cwd must be a git repo for `writable` / `isolation="worktree"`
steps** — the throwaway worktree is created with `git worktree add`. In a non-git
directory such a step fails fast with an actionable error. Either run the workflow
from a git repo (`git init`), or keep the step READ-ONLY and retrieve its produced
text with `harness workflow get-output <run_id> --step <label>`.

## Structured Output: the foundation

A worker called WITHOUT `schema` returns free text — and you cannot reliably
branch on free text. Pass `schema={...}` and the worker is forced to reply with a
single JSON object carrying the schema's TOP-LEVEL KEYS, parsed back into a native
Starlark dict:

```python
res = agent(
    "Audit " + args["area"] + " and report whether it is safe to ship.",
    schema={"ok": "bool", "findings": "list of strings"},
)
# res is a real dict with native types: res["ok"] is a bool, res["findings"] a list.
if res == None:
    log("worker produced no valid JSON — skipping")
elif res["ok"]:
    log("clean")
else:
    for f in res["findings"]:
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

**Field types — a `schema` value is enforced as a STRING.** The flat
`{"key": "hint"}` form makes every field a string on live runs (the hint guides
the worker; the runtime enforces the key is present). To get a LIST of items out
of a leaf, have it return them ONE PER LINE and `.splitlines()` the field — the
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

Do the work, verify it with a SEPARATE schema'd worker, and on failure make
exactly one repair pass — then stop. Bounded, not an open loop.

```python
agent("Implement " + args["task"] + " on the shared tree.", label="build")
v = agent(
    "Verify the change for " + args["task"] + ". Did it pass?",
    schema={"ok": "bool", "problems": "list of strings"},
    label="verify",
)
if v != None and not v["ok"]:
    agent(
        "Repair these problems in " + args["task"] + ":\n- " + "\n- ".join(v["problems"]),
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

Keep fanning out finders until K CONSECUTIVE rounds surface nothing new. A
`while` loop plus a `seen` set turns "find the bugs" into "find them all".

```python
seen = {}        # used as a set: finding -> True
dry_rounds = 0
K = 2            # stop after K consecutive empty rounds
while dry_rounds < K:
    rounds = parallel([
        {"prompt": "Find bugs in " + args["area"] + " NOT in this list:\n" +
                   "\n".join(seen.keys()),
         "schema": {"findings": "list of strings"}, "label": "finder"}
        for _ in range(2)
    ])
    fresh = 0
    for r in rounds:
        if type(r) == "dict" and type(r["findings"]) == "list":
            for f in r["findings"]:
                if f not in seen:
                    seen[f] = True
                    fresh += 1
    dry_rounds = dry_rounds + 1 if fresh == 0 else 0
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
    schema={"complete": "bool", "missing": "list of strings"},
    label="completeness",
)
if gaps != None and not gaps["complete"]:
    log("gaps remain: " + ", ".join(gaps["missing"]))
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

## Worked Example: bug hunt with adversarial verify

A quality workflow end to end: diverse schema'd finders fan out, every candidate
finding is cross-examined by a skeptic panel (majority must fail to refute), and
the confirmed set is synthesized. The runnable copy is
[`examples/bug-hunt-verify.star`](examples/bug-hunt-verify.star) — it composes
[structured output](#structured-output-the-foundation),
[adversarial verify](#adversarial-verify-majority-vote), and `None`-tolerant
flattening, and runs end-to-end under `--dry-run`.

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
| `--trace durable\|live` | Retain the heavy per-step turn-event trace (`durable`, default) or stream-only (`live`). |
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
- [ ] Program calls only `workflow`/`agent`/`parallel`/`pipeline`/`phase`/`log`/`verdict`/`output`/`args`; no clock/random/IO assumed.
- [ ] A program that produces an answer ends with `output(value)` so the caller reads `final_output.result` (a large answer goes through a `schema`'d dict, not capped free text).
- [ ] Every agent leaf (`agent()` call / `parallel` spec) has a `provider` of `"codex"` or `"claude"`.
- [ ] Parallel slots that EDIT files use `isolation` `"worktree"`.
- [ ] Every leaf whose output drives control flow uses `schema={...}` and the script handles a `None` (and, in fan-outs, a non-dict) result.
- [ ] If the workflow has only one `agent()` call and no branch/fan-out/loop, it is NOT a workflow — collapse it to that one call.
- [ ] Quality steps (verify / adversarial / judge / loop-until-dry / completeness) cross-check rather than trust a single pass, where the task warrants it.
- [ ] Ran it: `harness workflow run-script <prog.star>` (no member binding needed).
- [ ] The run is visible in `harness dashboard snapshot` (`workflow_runs` / `workflow_steps`) with its `design_intent`.
- [ ] The runner's profile allows the `harness` binary and what each leaf's prompt does.

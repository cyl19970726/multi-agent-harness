# Internal vs External Workflow: What Differs, and Why

A ground-truth comparison of two multi-agent orchestration systems that look
almost identical on the surface and are built on opposite foundations:

- **Internal** — Claude Code's built-in `Workflow` tool. The Lead agent calls it
  with an inline **JavaScript** program; the script runs **in-process** inside
  the agent's own Node runtime, and each `agent()` leaf is a **subagent of the
  same session**.
- **External** — this repo's `harness workflow run-script`. An agent authors a
  **Starlark** program; the harness CLI runs it **out-of-process**, and each
  `agent()` leaf is a **fresh `codex exec` / `claude -p` subprocess**.

> Companion docs: [`external-workflow-gap-analysis.md`](./external-workflow-gap-analysis.md)
> frames the difference as _program quality_ ("external programs were naive").
> This doc frames it as _architecture_ ("here is why the two systems differ at
> all, and what is forced versus chosen"). The catalog
> ([`internal-workflow-catalog.md`](./internal-workflow-catalog.md)) inventories
> the 12 reference programs the patterns come from.

---

## TL;DR

The two systems expose a **near-identical surface** — `agent()`, `parallel()`,
`pipeline()`, `phase()`, `log()`, schema'd structured output, a budget ceiling,
worktree isolation, resume/replay, the same `min(16, cores−2)` concurrency cap
and 1000-agent lifetime cap. Most differences people attribute to "missing
features" are **not capability ceilings** — the runtime closed those (budget,
verdict, resume, native schema, `pipeline()` all landed). What remains is forced
by **three architectural facts**:

1. **In-process JS engine** (internal) **vs out-of-process hermetic Starlark
   CLI** (external).
2. **In-product subagent** (internal) **vs heterogeneous cold CLI subprocess**
   (external).
3. **Turn-scoped ephemeral run** (internal) **vs durably-journaled standing
   artifact** (external).

Every concrete difference below — string-only schema fields, data-template
`pipeline` stages, read-only-by-default + auto-worktree, explicit
provider/model/sandbox, no MCP, the mandatory `design_intent` — falls out of one
of those three. None of them is an accident; each is the price (or the dividend)
of a deliberate architectural choice.

---

## 1. The two execution models at a glance

| | **Internal `Workflow`** | **External `run-script`** |
| --- | --- | --- |
| Authoring language | JavaScript (real closures, `async/await`, `Promise`, `.map/.filter/.flat`) | Starlark (Python-like, hermetic: no clock, no randomness, no IO, no `async`) |
| Where the script runs | In-process, inside the agent's Node runtime | Out-of-process, in the `harness workflow run-script` CLI |
| What an `agent()` leaf is | A subagent **inside the same session** (shares model, tools, MCP) | A **fresh OS subprocess** (`codex exec` / `claude -p`), own process group |
| Provider selection | Implicit — always "this product" | Explicit per leaf: `provider="codex" \| "claude"` |
| Structured output | Subagent **forced to call a `StructuredOutput` tool**, validated at the tool layer, **model retries** on mismatch → arbitrarily nested object | Native CLI flag (`--json-schema` / `--output-schema`); harness normalizes a flat `{key:"hint"}` dict to a JSON Schema where **every field is a `string`** |
| `pipeline()` stage | A **callback** `(prev, item, idx) => agent(...)` — arbitrary code per stage | A **data template** dict `{prompt: "...{input}...", schema?}` — `{input}` is string-substituted |
| Default file access | Edits the **live tree** by default; `isolation:'worktree'` is opt-in & expensive | **Read-only** by default; `writable=True` is required to edit **and auto-isolates** into a throwaway worktree |
| Lifetime / persistence | Ephemeral; lives for the turn/session; journal in transcript dir | Durable `WorkflowRun` + one `WorkflowStep` per leaf in a persistent store; on the Agent Dashboard |
| Resume | `resumeFromRunId`, **same session only** | `--resume <run_id>`, **across process restarts** |
| Mandatory metadata | `meta = { name, description, phases }` literal | `workflow(name, design_intent)` call; `design_intent` ≥ 20 chars or **fail-fast** |
| Orphan cleanup | Tied to session teardown | A **4-hour reaper** marks stale `Running` runs `Failed` |
| Budget | `budget.total / spent() / remaining()`, shared pool | `workflow(budget_usd=)` + `--max-budget-usd`, short-circuits to `budget`-skip steps |
| Concurrency | `min(16, cores−2)`, 1000-agent lifetime cap | **Identical:** `min(16, cores−2)`, 1000-agent lifetime cap |
| `parallel` / `pipeline` semantics | barrier / no-barrier streaming, input order | **Identical:** barrier / no-barrier streaming, input order |

---

## 2. What is actually the **same** (so we don't over-claim)

It is tempting to inflate the gap. Being precise: these are **behaviorally
identical** across both systems, by deliberate design parity:

- **Concurrency model** — a counting semaphore capped at `min(16, cores−2)`, a
  1000-agent lifetime backstop, excess calls queue. (External:
  `harness-workflow/src/lib.rs:176,189`.)
- **`parallel()` is a barrier**, **`pipeline()` is no-barrier streaming**; both
  return results in **input order**; a failed branch becomes `null` / a skip
  rather than rejecting the whole call.
- **Forward-injection in `pipeline()`** — each item flows through every stage
  independently; a fast item races ahead while a slow one lags.
- **Budget as a hard ceiling** with a `remaining()`-style short-circuit, plus the
  "scale fleet to the budget" loop idiom.
- **Worktree isolation** — a git worktree per isolated worker, cleaned up on exit
  (external via RAII `Drop`; `main.rs:4016–4034`).
- **Resume by deterministic ordinal keying** of completed leaves, guarded by a
  script-identity check so changed control flow safely re-runs instead of
  mis-aligning.
- The **same authoring guidance** — "default to `pipeline()` over `parallel()`",
  adversarial-verify, loop-until-dry, judge panels.

The surface parity is intentional: external was built to mirror internal so the
**patterns transfer**. The differences below are the residue that architecture
won't let you erase.

---

## 3. The three root causes

### Root cause 1 — in-process JS engine vs out-of-process hermetic Starlark CLI

Internal's host **is** a JavaScript engine: the Workflow script runs in the same
Node process that hosts the agent loop. So the script gets **real JS**: closures,
`Promise`, `try/catch`, `.map/.filter/.flat`, `while (budget.remaining() > …)`.
The only things removed are `Date.now()` / `Math.random()` / `new Date()` (they
would break resume).

External's host is the `harness workflow run-script` CLI, and the script is a
durable, **reproducible artifact**: it is snapshotted, journaled, resumable across
restarts, runnable headless/cron. Reproducibility **demands determinism**, which
is why Starlark was chosen — it is hermetic by construction (no clock, no
randomness, no IO). The orchestration is therefore a pure function of
`(script, args, journaled leaf results)`; the only nondeterminism lives in the
`agent()` leaves themselves.

**What this forces:**

- **`pipeline()` stages are data, not code.** In external, Starlark values cannot
  cross the Rust thread boundary into worker threads — specs are extracted to
  plain Rust on the eval thread before fan-out (`starlark_front.rs:854`). A stage
  must therefore be a **serializable template** (`{prompt, schema}` with an
  `{input}` placeholder), not a closure. Internal's stage is a first-class
  function because its host can just call it. **This is the single biggest
  ergonomic difference**, and §5 makes it concrete.
- **No `try/catch` around a leaf.** A failing external leaf is journaled and (if
  over budget) becomes a `budget`-skip; internal can wrap a stage in `try/catch`
  and resolve it to `null`.
- **The script can't observe wall-clock or randomness.** Vary work by index, not
  `Math.random()`; stamp timestamps after the run, not during.

### Root cause 2 — in-product subagent vs heterogeneous cold CLI subprocess

Internal's `agent()` spawns a subagent **within the same Claude Code session** —
it reuses the session's model, its tool registry, and **all session-connected MCP
servers** (via `ToolSearch`). There is exactly **one provider** ("this product"),
so `provider` isn't even a parameter.

External's `agent()` execs a **fresh OS subprocess** with no memory of any
session — `codex exec --json --sandbox …` or `claude -p --output-format
stream-json …` (`main.rs:4717,4811`). It is **provider-agnostic by charter**: it
orchestrates _heterogeneous_ vendor CLIs, so each leaf must be an out-of-process
exec with a normalized contract, and the author must **pick the provider per
leaf**.

**What this forces:**

- **Structured output is whatever the CLI's flag supports.** Internal owns the
  model loop, so it can intercept a `StructuredOutput` tool call and **re-prompt
  on a schema miss** — yielding arbitrarily nested validated objects. External can
  only pass `--json-schema` (claude) / `--output-schema <file>` (codex) and parse
  what returns. To map one terse author-friendly dict onto two different vendors'
  schema flags **and** keep the `--dry-run` mock simple,
  `schema_to_json_schema()` (`main.rs:4322`) normalizes every field to a
  **`string`**. Hence the iron external rule: **list-valued returns come back
  one-per-line and you `.splitlines()` them.**
- **Explicit model + sandbox + tool allowlist.** A cold CLI has no inherited
  session config, so external must spell out the `model` string, the sandbox
  (`read-only` vs `workspace-write` for codex), and the tool allowlist
  (`Read,Grep,Glob` vs `Read,Edit,Write,Bash,Grep,Glob` for claude;
  `main.rs:4825–4838`).
- **No MCP.** There is no session, so there are no session-connected MCP tools to
  offer. Internal workflow agents can reach them.
- **Process-group isolation & SIGKILL.** Each worker is its own process group, so
  a timeout kills the whole tree via `libc::kill(-pid, SIGKILL)` — a concern that
  doesn't exist for an in-process subagent.

### Root cause 3 — turn-scoped ephemeral run vs durably-journaled standing artifact

Internal runs **as part of a turn**. Its final value returns to the main loop, it
shows in `/workflows`, and its lifetime is the conversation. It is an
**orchestration primitive _inside_ a session**.

External is a **standing service artifact**: every run is a durable `WorkflowRun`
plus one `WorkflowStep` per leaf, written to a persistent store, surfaced on the
Agent Dashboard, resumable independently of any conversation, and authored to be
**shipped, scheduled, and audited**.

**What this forces / buys:**

- **Mandatory `design_intent`.** A shipped, audited artifact must carry its own
  rationale; external **fail-fast rejects** a run whose `design_intent` is blank
  or < 20 chars (`starlark_front.rs:1097`). Internal's `meta.description` is
  advisory.
- **Script snapshotting + cross-restart resume.** The run persists
  `spec = {lang:"starlark", script:<text>}` so `--resume` can verify
  byte-identity before reusing prior leaves — across process restarts, not just
  within a session.
- **A 4-hour stale-run reaper** (`reap_stale_workflow_runs`, `main.rs:8934`).
  Orphaned `Running` runs (host crashed mid-run) get marked `Failed`. Internal
  has no orphans — session teardown takes everything with it.
- **`verdict(ok, reason)` as a first-class, persisted outcome.** Because the run
  outlives the turn, "did the intent succeed?" must be a durable, typed field on
  the record — not just a value the caller reads and forgets.

### Root cause 4 — the safety default flips (a corollary of 2 + 3)

Because external runs cold CLIs **headless and unattended** (root causes 2 + 3),
its safe default is the **inverse** of internal's:

| | Internal | External |
| --- | --- | --- |
| Default file access | **writable** (edits live tree; trusts in-session subagents) | **read-only** (`Read,Grep,Glob`) |
| To edit | nothing special; `isolation:'worktree'` only to avoid parallel collisions | **`writable=True`** — required, and it **auto-isolates** into a throwaway worktree |

This flip has a real authoring consequence: in external, **a writable worker runs
in its own worktree, so a separate implement-step and verify-step do _not_ share
a tree.** The faithful build loop therefore lives **inside one writable worker**
(implement → run the gate → fix → repeat), while the plan and the verdict stay in
Starlark. Internal can split implement and verify across agents on the shared
live tree. (See [`build-and-gate.star`](../../skills/author-workflow/examples/build-and-gate.star).)

---

## 4. Difference catalog → root cause

| Observable difference | Root cause | Forced or chosen? |
| --- | --- | --- |
| JS closures vs Starlark hermetic dialect | 1 | Forced (determinism for replay) |
| `pipeline` stage = callback vs data template | 1 | Forced (thread/host boundary) |
| `try/catch` around a leaf (internal only) | 1 | Forced |
| Nested schema object vs every-field-`string` | 2 | Forced (vendor flag + dry-run mock) |
| `.splitlines()` idiom for list returns | 2 | Forced (consequence of the above) |
| `provider=` parameter exists at all | 2 | Forced (heterogeneous CLIs) |
| Explicit `model` / sandbox / tool allowlist | 2 | Forced (no inherited session) |
| MCP available (internal only) | 2 | Forced (no session in external) |
| Mandatory `design_intent` | 3 | Chosen (audit discipline) |
| Durable journal + dashboard + cross-restart resume | 3 | Chosen (standing artifact) |
| 4-hour reaper | 3 | Forced (orphans are possible) |
| `verdict()` as a persisted typed outcome | 3 | Chosen |
| Read-only default + `writable` auto-worktree | 4 | Chosen (unattended safety) |
| Concurrency caps, barrier semantics, budget, resume keying | — | **Same by design** |

---

## 5. One scenario, both ways

**Scenario (identical for both):** review the work across N dimensions; for every
finding, run an adversarial verification panel; keep only findings that survive;
synthesize a triaged report. This is the canonical review pattern in the internal
tool's own docs **and** the shape of
[`bug-hunt-verify.star`](../../skills/author-workflow/examples/bug-hunt-verify.star).

### 5a. Internal — JavaScript

```js
export const meta = {
  name: 'review-and-verify',
  description: 'Review across dimensions, adversarially verify each finding, triage',
  phases: [{ title: 'Review' }, { title: 'Verify' }, { title: 'Synthesize' }],
}

const DIMENSIONS = [
  { key: 'logic',   prompt: 'Find logic / off-by-one / boundary bugs in the diff.' },
  { key: 'failure', prompt: 'Find error-handling and resource-leak bugs in the diff.' },
  { key: 'concurrency', prompt: 'Find races and unguarded shared-state bugs in the diff.' },
]

// A pipeline stage is a CALLBACK. Stage 2 fans out a verifier PANEL per finding —
// a parallel() nested INSIDE the stage. The stage returns rich nested objects.
const reviewed = await pipeline(
  DIMENSIONS,
  d => agent(d.prompt, { label: `review:${d.key}`, phase: 'Review', schema: FINDINGS_SCHEMA }),
  (review, d) => parallel(
    review.findings.map(f => () =>
      agent(`Try to REFUTE this finding: ${f.title}. Default to refuted=true if unsure.`,
            { label: `verify:${d.key}`, phase: 'Verify', schema: VERDICT_SCHEMA })
        .then(v => ({ ...f, dim: d.key, refuted: v.refuted }))   // graft verdict onto finding
    )
  )
)

const confirmed = reviewed.flat().filter(Boolean).filter(f => !f.refuted)
const report = await agent(
  `Synthesize a triaged report from these confirmed findings:\n${JSON.stringify(confirmed)}`,
  { phase: 'Synthesize', schema: REPORT_SCHEMA })
return { confirmed: confirmed.length, report }
```

What the JS host gives you for free: `review.findings` is a **real array of
objects**; the stage is a **closure** that can fan out a `parallel()` panel
**per finding** and `.then(...)` to **graft the verdict back onto the object**;
`.flat().filter()` collapses and screens in one line.

### 5b. External — Starlark

```python
# review-and-verify.star
workflow(
    "review-and-verify",
    "Fan out diverse reviewers, adversarially verify EACH finding with a skeptic " +
    "panel (majority must fail to refute), then synthesize a triaged report — so " +
    "only cross-checked findings survive.",
    budget_usd = 8.0,
    success_criterion = "every reported finding survived an adversarial skeptic panel",
)

# Schema fields are enforced as STRINGS, so list returns come back one-per-line.
FINDINGS = {"findings": "each bug, ONE PER LINE, as `<file>:<line> — <bug + failure>`"}
SKEPTIC  = {"refuted": "bool: true unless you can clearly confirm a REAL bug",
            "reason":  "one sentence citing the code"}
REPORT   = {"summary": "2-3 sentence verdict", "must_fix": "blocking bugs, one per line"}

DIMENSIONS = [
    {"key": "logic",       "what": "logic / off-by-one / boundary bugs"},
    {"key": "failure",     "what": "error-handling and resource-leak bugs"},
    {"key": "concurrency", "what": "races and unguarded shared-state bugs"},
]

# --- review: parallel() barrier; stages can't fan out per-finding, so we collect first ---
phase("review")
finds = parallel([
    {"prompt": "Find " + d["what"] + " in the diff. One finding per line as " +
               "`<file>:<line> — <bug + failure>`. Only concrete, code-grounded bugs.",
     "label": "review:" + d["key"], "schema": FINDINGS}
    for d in DIMENSIONS
])

# Flatten one-per-line string findings into a candidate list (the .splitlines() idiom).
candidates = []
for res in finds:
    if type(res) == "dict" and type(res["findings"]) == "string":
        for line in res["findings"].splitlines():
            if line.strip():
                candidates.append(line.strip())
log("collected " + str(len(candidates)) + " candidates")

# --- verify: a skeptic panel per finding — a Starlark loop OVER parallel() calls ---
phase("verify")
SKEPTICS = 3
confirmed = []
for finding in candidates:
    panel = parallel([
        {"prompt": "Try to REFUTE this finding: \"" + finding + "\". Check the real code; " +
                   "set refuted=true unless you can clearly confirm it.",
         "label": "verify", "schema": SKEPTIC}
        for _ in range(SKEPTICS)
    ])
    refuted = 0
    for v in panel:
        if not (type(v) == "dict" and v["refuted"] == False):
            refuted += 1            # missing / non-dict vote counts as refuted (conservative)
    if refuted * 2 < SKEPTICS:      # a MAJORITY did NOT refute
        confirmed.append(finding)
log(str(len(confirmed)) + " of " + str(len(candidates)) + " survived")

# --- synthesize + typed verdict ---
phase("synthesize")
report = agent(
    "Synthesize a triaged report from these CONFIRMED findings (do NOT re-litigate):\n- " +
    "\n- ".join(confirmed),
    schema = REPORT)
verdict(type(report) == "dict",
        reason = "review complete; " + str(len(confirmed)) + " finding(s) confirmed")
```

### 5c. Reading the diff

Same scenario, same fan-out shape, same adversarial-panel idea — but the
**orchestration code diverges at exactly the points the three root causes
predict**:

| Where they diverge | Internal (JS) | External (Starlark) | Root cause |
| --- | --- | --- | --- |
| Per-finding verify panel | A `parallel()` **nested inside the `pipeline` stage callback** | A **Starlark `for`-loop over `parallel()` calls** (a stage template can't fan out per item) | 1 |
| Finding shape | `review.findings` is an **array of objects**; verdict is grafted with `{...f, refuted}` | `findings` is **one string**, `.splitlines()`-ed; the finding stays an opaque line | 2 |
| Collapse + screen | `.flat().filter(Boolean).filter(f => !f.refuted)` | explicit `for` loops accumulating into `confirmed` | 1 |
| Outcome | `return { confirmed, report }` to the caller | `verdict(...)` **persisted on the run record** | 3 |
| Header | `meta = { name, description, phases }` | `workflow(name, design_intent, budget_usd=, success_criterion=)` — intent mandatory | 3 |

The external version is **longer and more explicit** — not because the runtime is
weaker (the concurrency, budget, and verify semantics are identical), but because
its **stages are data**, its **structured returns are flat strings**, and its
**outcome is a durable typed field**. Every extra line traces to a root cause,
not to a missing feature.

---

## 6. What the comparison actually teaches

1. **The gap narrowed to ergonomics, not capability.** Budget, verdict, resume,
   native schema, and `pipeline()` all shipped; the catalog's old "ceiling"
   claims are largely stale. What's left is _how the code reads_, driven by the
   host boundary.
2. **The biggest single ergonomic cost is data-template `pipeline` stages.** When
   a stage must fan out or branch per item, external drops to `parallel()` + a
   Starlark loop. That is the one pattern to internalize when porting an internal
   workflow.
3. **Plan around flat-string structured output.** Design schemas as
   one-field-per-concern with **one-per-line** list values, and `.splitlines()`
   on the way out. Don't reach for nested JSON Schema — it breaks the dry-run mock
   and isn't enforced field-by-field anyway.
4. **The external defaults are safety defaults.** Read-only-by-default and
   `writable`-auto-worktree exist because external runs unattended. Keep
   finders/reviewers/verifiers read-only; put the whole edit→gate→fix loop in
   **one** writable worker.
5. **External buys auditability the internal tool doesn't need.** Mandatory
   `design_intent`, durable journaling, cross-restart resume, and the reaper are
   the dividend of being a standing artifact — lean on them, and always declare a
   real `success_criterion` + `verdict()`.

---

## 7. Implications for the skill

The [`author-workflow`](../../skills/author-workflow/SKILL.md) skill already
teaches the transferable patterns (verify+repair, adversarial verify, judge
panel, loop-until-dry, completeness critic) and the runtime mechanics
(read-only default, `writable` worktree, budget, `verdict`). This doc adds the
**"why" layer** an author should hold in their head while porting an internal
workflow:

- A `pipeline` stage that needs to **fan out or branch per item** → rewrite as
  `parallel()` + a Starlark loop (root cause 1).
- A schema that wants a **nested object or a list field** → flatten to
  one-per-line strings + `.splitlines()` (root cause 2).
- An edit step → make it the **one** `writable` worker and run the gate inside it
  (root cause 4).
- The run's success → a declared `success_criterion` + a typed `verdict()`, not
  just "the workers ran" (root cause 3).

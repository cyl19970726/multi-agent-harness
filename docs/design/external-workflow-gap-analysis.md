# External Workflow Gap Analysis

**Question:** our self-built (external) dynamic-workflow runtime can fan out
codex/claude workers deterministically — but the workflows it actually runs feel
naive next to Claude Code's *internal* Workflow tool, especially in the
**planning** dimension. Where, concretely, are the gaps?

**Method:** mined ~60 real internal Workflow run records persisted under
`~/.claude/projects/<slug>/<session>/workflows/wf_*.json` (each holds the full
embedded orchestration script + result + per-agent progress). Four parallel
readers extracted each run's planning model and control-flow patterns. The
external side is the current `evals/tasks/bug-false-positive/workflow.star` and
the registered `investigate` workflow. All runids cited below are real records.

---

## 1. What the internal corpus actually does (myth vs reality)

**Myth:** "internal workflows are good because the agent plans dynamically."

**Reality:** planning is **overwhelmingly static at the skeleton level**. In
~50 of ~60 records the phase list and fan-out width are hard-coded arrays
(`STAGES` / `PROBES` / `philosophies` / `DIMS`); the orchestration script never
asks a leading agent "what are the steps?" The *plan* is authored at write-time
by the orchestrator (the human/agent writing the `.js`), not generated at run
time. Genuinely dynamic decomposition is **rare** (~3–4 of 60):

- `wf_e37ff5d7-20c` — a leading **Enumerate** scout lists every skill via
  `gh api`; its output is interpolated into the downstream Gather prompts, so
  the reading work is scout-sized (dynamic *content*, fixed lane count).
- `wf_f86139b9-018` (26 agents) / `wf_54170927-b26` (36 agents) — review phase
  emits a variable findings list, then `findings.map(f => agent(...))` spawns
  **one verifier per finding**: the only runs whose fan-out *count* is sized by
  an earlier agent's output.

So the strength is **not** dynamic planning. It lives in four *other*
dimensions:

| Dimension | How internal workflows do it | Evidence (runids) |
| --- | --- | --- |
| **Closed-loop self-check** | act → verify → repair: run the *real* gate, feed errors back to a fix agent, **loop ≤3–6 attempts until green** | `wf_b0be908d-0c2`, `wf_36d72f9a-77c`, `wf_f6900ec3-4f6`, `wf_07defa4e-b3f` (≤6), `wf_fbda5429-b57` (≤5) |
| **Schema-gated branching** | structured output *drives control flow*: `ok` decides continue/stop, `severity` decides whether a Fix phase runs, `winner` enum picks a branch | `wf_07defa4e-b3f` (`blocking = findings.filter(critical‖high)` → conditional Fix), `wf_fbda5429-b57` (`winner` enum branches the impl) |
| **Leading plan, injected forward** | a Design/Decide/Probe agent emits a **typed plan**, then `JSON.stringify`'d **into downstream prompts** as ground truth | `wf_f91ff6a5-fda` (understand → 3-way proposal tournament → judge/synthesis), `wf_fbda5429-b57` (probe → decide → impl), `wf_ca82faa8-2ce` (typed readiness → sequenced WP plan) |
| **Locked shared context** | a `COMMON` preamble bakes the spec, the *verified* code map (`file.rs ~Lxxx`), the exact CI gate commands, and design ground-truth into every agent's prompt | nearly all build pipelines; `COMMON` block ubiquitous |

There is also a **meta-planning outer loop**: the orchestrator iterates the
*script itself* across runs. `wf_c85fa40f-c78` / `wf_f6d4725a-87c` /
`wf_cc39b98e-771` are three attempts at the same workflow; `wf_f86139b9-018`
succeeded where its twin `wf_12073ee5-627` (15 agents) failed. author → run →
observe → fix-script is part of why the internal workflows are good.

Two recurring topologies cover most of the corpus: **gather→synthesize
map-reduce** for research/design (`wf_00be27b8`, `wf_22ce8232`, `wf_2b1cd5d5`,
`wf_b302e7ce`, `wf_e2175003` …) and **serial gated WP build chains** for
delivery (`wf_1265b38e`, `wf_36d72f9a`, `wf_6eabf27a` (7 WPs, ~98 min),
`wf_b0be908d` …), the latter with a uniform `if
out.startsWith('GATE_FAILED') return {stoppedAt}` early-abort cascade.

---

## 2. Where our external workflow's problems actually are

Ranked by impact. Each row contrasts the internal idiom against the current
external reality (`bug-false-positive/workflow.star`: find → verify → report,
flat; `investigate`: scope → audit → done).

### A. Open-loop vs closed-loop — the deepest gap

Internal: `act → verify → repair` until a typed gate passes. External: **`act →
report`, one pass, zero iteration, zero self-repair.** This is exactly the
"planning" that feels missing — the internal workflow *plans to check its own
work and fix it*. The `verify-loop (≤N attempts)` is the single most universal
internal idiom and it is **entirely absent** from every external workflow we
run.

### B. Schema-gated branching is missing

We added `schema=` to the external `agent()`, **but the flow never branches on
it.** Internal flow does: `if blocking.length > 0: Fix`, `if !res.ok: break`,
`winner == 'exec-server' ? … : …`. We treat structured output as *output*, not
as *control*. One critical finding or ten — our graph shape is identical.

### C. No leading planning phase (the part the gap was first felt in)

Every sophisticated internal run has an agent **produce a typed
plan/decision/design first**, then injects it downstream. Our external
workflows **cold-start a fixed fan-out** — there is no understand → decide →
then-act seam. Agent-authored planning simply **does not exist** externally.

### D. No locked shared context

Internal agents share a `COMMON` preamble (authoritative spec + verified code
map + exact gate). Our parallel external agents each get a **bare prompt** with
no shared ground-truth, so parallel results drift and synthesis is incoherent.

### E. No real outcome gate inside the workflow

Internal verification is behavioral / CI-poll / from-zero real-binary
acceptance (`ACCEPT_PASS`, plant-fact-recall, live SSE push). Our external
workflow carries **no real gate** — "verification" is a regex grader that lives
*outside* the workflow, in the eval runner.

---

## 3. Why this ties back to the eval result

The eval showed *workflow: no quality gain at 18.2× cost* on the
`bug-false-positive` task. Earlier this was attributed only to "task too easy."
The gap analysis sharpens it: **our workflow is itself a naive open-loop fan-out
that uses none of the structure that makes internal workflows valuable**
(closed-loop / gated / planned / shared-context). Two consequences:

1. **The skill teaches patterns the examples don't use.** `wf_4bb51aaf-3f9`
   added verify+repair, adversarial-verify, judge-panel, and loop-until-dry to
   the `star-workflow` skill — but our actual `.star` examples use **none** of
   them. The gap is between what is *taught* and what is *written*.
2. **Eval tasks must reward these dimensions.** A task that needs none of
   closed-loop / gating / planning cannot distinguish a good workflow from a
   more expensive single agent. The discriminating task must be one a
   straight-line fan-out *cannot* get right.

---

## 4. Diagnosis in one line

Our external workflow's problem is **not** "can't parallelize." It is that our
workflows are **open-loop, straight-line, context-starved fan-outs**, whereas
internal workflows are **closed-loop, schema-gated programs with a leading plan
and shared authoritative context**. Missing agent-planning (C) is only one
symptom; **closed-loop self-checking (A) is the largest hole.**

---

## 5. Remediation plan ("de-naive the external workflow")

1. **Canonical `.star` examples → closed-loop + gated + leading-plan.** Rewrite
   the example/eval programs to: (i) open with a plan/decompose agent whose
   typed output is injected forward (C, D); (ii) branch on structured output
   (B); (iii) carry a verify→repair loop with a real gate and a max-attempt
   bound (A, E). Mirror `wf_07defa4e-b3f` / `wf_f91ff6a5-fda`.
2. **Skill makes these the mandatory skeleton**, not optional tips: a workflow
   that only fans-out-and-reports should read as an anti-pattern. Carry the
   `COMMON`-preamble and `GATE_FAILED`-style sentinel idioms across.
3. **Eval task that a flat fan-out cannot pass** — an iterate-refine task graded
   on a real outcome gate (e.g. "make this failing suite pass"), so closed-loop
   structure is the only way to score. Add the `single-step-control` negative
   control so ceremony is still penalized.

> Evidence base: ~60 internal `wf_*.json` records across sessions `efd4b176`,
> `e7fe19c6`, `c574dc0f`, and the `multica-workspaces` workspace. Cited runids
> are verbatim from those records.

---

## 6. More structural gaps (F–H), from the full 62-record corpus

The full classification ([internal-workflow-catalog-full.md](internal-workflow-catalog-full.md))
surfaced three structural gaps that A–E did not name. They extend the same
"de-naive the program" axis.

### F. No failure-propagation contract

Across the 12 `serial-gated-delivery` records the **`GATE_FAILED:` first-line
sentinel** is universal: each step inspects the prior step's first line and
short-circuits the whole cascade so broken work never compounds (e.g.
`if (out.startsWith('GATE_FAILED')) return {stoppedAt}` in `wf_b0be908d-0c2`,
`wf_56fc2f22-2d1`, `wf_6eabf27a-e1b`). Our external workflow has **no cross-step
failure contract** — a failed node throws (dropping to `null` in `parallel()`)
or returns prose, and nothing deterministically aborts or degrades downstream.

→ Need a typed/sentinel failure convention + short-circuit so one bad node stops
the chain instead of letting later nodes build on broken work.

### G. No cost-aware worker routing

Internal workflows route cheap read-only work to **cheaper/specialized agent
types**: `agentType:'Explore'` for review/probe (`wf_f86139b9-018`,
`wf_a8874e2e-42a`), `claude-code-guide` for CLI knowledge (`wf_56fc2f22-2d1`,
`wf_fbda5429-b57`), even a `haiku` subagent inside an opus run. Our external
`agent(provider=…)` picks codex|claude but has **no per-node model-tier / cheap-
worker routing** — every node pays full price — and (see §7) no budget ceiling
at all, so one 337-line feature burned 10.9M input tokens (~$20) unbounded.

→ Need per-node model/tier selection (cheap worker for cheap verify/read steps)
*and* a per-run budget ceiling.

### H. Schema-forced output has no failure path

Schema-forcing is itself a **live failure mode**: two internal runs failed/aborted
when a schema-forced agent never emitted its StructuredOutput (`wf_12073ee5-627`
failed @15 agents, `wf_f47ddc8f-ed7` failed @0). Our external runtime added
schema-forcing (#75) by *prompt-instructing* the worker to "reply with a single
JSON object" — a one-shot codex/claude can simply not comply. The miss IS
detected (`schema_failed` marks the step failed with `reason:"schema"`,
`main.rs:4163`) — but there is **no retry or fallback**, and the script's
`agent()` call receives `None`, so a downstream stage that reads the dict
silently gets nothing. (The related `parallel()`-can't-carry-schema defect is
fixed — `dict_schema` reads the per-spec schema at `starlark_front.rs:238`.)

→ Treat "worker did not emit valid schema" as a first-class error with a
retry/fallback, not an assumption that structured output always arrives.

---

## 7. Runtime, safety & grading gaps (from the internal eval `wf_961f46bb`)

A different axis from planning: an internal workflow (`evaluate-external-workflow`,
`wf_961f46bb-0f7`, 9 agents) adversarially evaluated *our* external runtime and
produced a P0–P3 roadmap. Re-verified against current code — several are **already
fixed**, the rest are still open and are real gaps.

| gap | axis | priority | status | evidence (now) |
| --- | --- | --- | --- | --- |
| worker turn has no real wall-clock ceiling | safety | P0 | ✅ **fixed** | `d558dcb` — stdout reader thread + process-group kill + default 3s→5min |
| claude worker model reported as `None` | observability | — | ✅ **fixed** | `parse_worker_model` (#74) |
| `parallel()` can't carry `schema` → fan-out prose-only | grading | P2 | ✅ **fixed** | `dict_schema(&dict,"schema")` `starlark_front.rs:238` (#75) |
| **no cost/token budget ceiling** | safety / cost | **P0** | ❌ **open** | grep `budget\|max_tokens\|max_cost\|spend` in both crates → empty; only backstop is a 1000-agent count |
| **worker sandbox**: `bypassPermissions` + arbitrary Bash on the live repo cwd; `isolation=worktree` opt-in & unused | safety | P1 | ❌ **open** | default cwd = real repo; worktree not default |
| **no stale-run reaper**: a crashed/Ctrl-C'd run stays `status=running` forever | observability | P1 | ❌ **open** | "Stale" handling exists only for resident-daemon / provider-sessions, not `WorkflowRun` |
| **no declared `success_criterion`** on `workflow()` → status conflates "workers ran" with "intent satisfied" | grading | P2 | ❌ **open** | `success_criteria` exists only on `Goal`, not `WorkflowRun`; a verifier saying PROBLEMS but exit 0 still reports `completed` |
| **no resume/replay** → a crash re-spends all tokens | cost | P2 | ❌ **open** | no leaf replay keyed on `step_id` in the workflow path |
| **`log()`/`final_output` not surfaced** in the dashboard | observability | P3 | ❌ **open** | no `final_output`/`logs` refs in `Workflows.tsx` |

**The top open item is the budget ceiling.** Of the two original P0 liveness/safety
defects, the timeout is fixed (`d558dcb`); the **per-run token/cost budget
ceiling is still absent**, which the eval called "the single biggest blocker to
any unattended production use" (a real run hit 10.9M input tokens / ~$20 for a
337-line change). It pairs naturally with gap **G** (cost-aware routing).

---

## 8. The gap map, consolidated

Two axes, both real:

- **Program/authoring axis (A–H):** our `.star` programs are open-loop,
  straight-line, context-starved fan-outs. Fix in the canonical examples + skill.
  Biggest hole: **A (closed-loop self-checking)**.
- **Runtime/safety/grading axis (§7):** the runtime lacks spend/sandbox/reaper/
  grading guardrails. Fix in `harness-workflow` + `harness-cli`. Biggest hole:
  **budget ceiling (P0)**.

A "de-naive the external workflow" effort must move on **both** — a closed-loop
program is still unsafe unattended without a spend ceiling, and a safe runtime
is still naive if its programs only fan-out-and-report.

---

## 9. Expressiveness gaps — can our Starlark even express `evaluate-external-workflow`?

A concrete yardstick: take the actual internal program `evaluate-external-workflow`
(`wf_961f46bb-0f7`) — `pipeline(DIMS, assess, verify)` with rich nested schemas
(`ASSESS`/`VERIFY`/`REPORT`), each stage injecting `JSON.stringify(prior)` forward
— and ask what our external Starlark runtime can reproduce. Verified against
`crates/harness-workflow/src/starlark_front.rs` + `crates/harness-cli/src/main.rs`.

### Hard gaps — cannot be expressed at all

**I. `pipeline()` is not exposed to Starlark.** `workflow_globals` registers only
`workflow`, `agent`, `parallel`, `phase`, `log` (`starlark_front.rs:256–360`).
The program's *entire backbone* is `pipeline(DIMS, assess, verify)` — each
dimension streams through assess→verify with **no barrier** (dimension 2 still
assessing while dimension 1 verifies). Externally you can only `parallel()` (a
barrier), so the streaming two-stage form degrades into two barriered phases.
(The Rust runtime *has* a streaming `pipeline()`; the Starlark front-end just
doesn't surface it.)

**J. `schema=` is a flat key→hint dict, not a real JSON Schema.** Our runtime
takes the schema dict's **top-level keys** as the required reply keys
(`schema_required_keys`, `main.rs:4267`) and validates only that the reply is a
JSON object carrying those keys. The program's schemas are nested JSON Schema —
`score:{enum:[pass,partial,gap]}`, `roadmap:{items:{…priority,item,why}}`,
`required:[…]`. Passing such a `{type:'object', properties:{…}, required:[…]}`
object to our `schema=` would make the worker reply with keys *`type`,
`properties`, `required`* — wholly wrong. So `ASSESS`/`VERIFY`/`REPORT` **cannot
be ported as written**; they must be flattened to `{field: "hint"}`, dropping
the enums, nested item shapes, and required-ness.

### Semantic gaps — expressible but materially weaker

**K. Schema is a prose "Shape hint", validated to top-level keys only — not
API-enforced, no retry.** `schema_instruction` does inline the full compact
schema as a "Shape hint" (`main.rs:4278–4285`), so a capable worker may follow
it — but nothing **enforces** enums/types/nested-required, and there is no
retry-on-mismatch. The internal `agent(schema)` forces a real tool call validated
against the full JSON Schema by the model API, retrying until it conforms. Ours
is prompt-hint + top-level-key check (+ the gap-H no-retry failure path).

**L. No `json.encode` host function for the script.** The program injects
upstream typed output forward with `JSON.stringify(a)` at every stage — its core
mechanism. Starlark scripts have no clean JSON serializer exposed (only Rust-side
`json_to_value`/`value_to_json` marshalling); a script can only `str(dict)` (a
Python-ish repr, not JSON). So "inject the prior agent's structured result into
the next prompt" (gaps C/D) is awkward and lossy externally.

### Ergonomic gaps — different syntax, still expressible

`.then(v => ({...a, verdict:v}))`, `.filter(Boolean)`, template literals `${…}`,
`meta.phases` → Starlark equivalents exist (sequential `v = agent(...)` + dict
merge, list comprehension `[x for x in xs if x]`, `%`/`.format()`,
`workflow()+phase()`). Not blockers, just lower ergonomics.

**Net:** externally we can express "parallel fan-out + flat single-level schema
hint + prose synthesis." We **cannot** express this program's form — it needs
`pipeline()`, real nested JSON-Schema enforcement, and script-level JSON
serialization for forward-injection. Those three are the expressiveness gaps to
close (on top of the authoring gaps A–H and the runtime gaps §7).

> Evidence base: 62 classified internal `wf_*.json` records
> ([internal-workflow-catalog-full.md](internal-workflow-catalog-full.md)) +
> the internal runtime eval `wf_961f46bb-0f7`, re-verified against current
> `crates/harness-workflow` + `crates/harness-cli` + `apps/agent-dashboard`.

---

## 10. Research correction — most gaps are CLI-flag wiring, not ceilings

A research workflow (`research-external-workflow-gap-solutions`, `wf_acee5689-c17`,
11 agents) **actually ran the claude/codex CLIs** and adversarially re-ran each
key command. It overturns several §6/§9 claims: the structured-output gaps are
**not** an architectural ceiling — both CLIs have native schema-constrained output
that we simply never wire. Every status below is *reproduced-from-command*.

### Corrected gap statuses

| gap | old claim | corrected (verified) |
| --- | --- | --- |
| **J** schema not real JSON Schema | architectural ceiling | **solvable-now.** `claude -p --json-schema '<inline>'` → `result.structured_output` (verified `{"capital":"Paris"}`); `codex exec --output-schema <file>` → schema-valid final answer (verified `{"verdict":"fail","score":0}` even against a conflicting prompt). Native on **both**; we just don't pass the flag. |
| **K** prompt-instructed, not enforced | weaker by architecture | **solvable-now.** claude `structured_output` rides the terminal `result` event in the *same* `stream-json --verbose` mode `spawn_claude_ephemeral` already uses (`main.rs:4663`); codex via the already-wired `--output-last-message` (`main.rs:4613`). Wiring only. |
| **H** no retry on bad schema | claimed open | **partly already closed.** One corrective re-prompt for non-conforming structured replies already exists (`main.rs:4140-4153`); only transient crash/timeout retry is missing. Native schema shrinks how often it even fires. |
| **budget** P0 | un-addressable P0 | **reduced to M.** claude `--max-budget-usd` is real but **soft** (10–40× overshoot on tiny caps); codex has **no** native budget flag. Authoritative ceiling = cumulative `StarlarkCtx` tally + per-worker backstop. claude emits exact `total_cost_usd` (we don't parse it yet — `main.rs:4525`). |
| **L** no `json.encode` | open | **solvable-now, ~1 line.** `starlark_front.rs:475` uses bare `GlobalsBuilder::standard()`; switch to `extended_by(&[LibraryExtension::Json])` (starlark-rust 0.14 ships `json.encode/decode`; round-trip empirically proven). |
| **I** `pipeline()` not exposed | open | **open, shape pinned.** `crate::pipeline` exists/tested but `PipelineStage` is a `Send+Sync` closure — a Starlark value/Evaluator cannot cross into it, so it **cannot** be a thin wrapper. Needs **data-template stages** (per-stage prompt/schema/provider + Rust-side forward-injection), effort M. |
| **sandbox** §7 P1 | open | open; fix is cli-flag-wiring + a default flip. claude has **no OS sandbox** (permission-mode only); codex has `-s read-only\|workspace-write\|danger-full-access`. Default codex `read-only` (escalate for editing nodes), claude `acceptEdits` + opt-in Bash, default-isolate editing nodes (`spawn_ephemeral_worker` `main.rs:4070`). |
| **success_criterion** §7 P2 | open | open; `workflow()` takes only free-text `design_intent`; `outcome_from_steps` (`lib.rs:616`) equates "all workers exited" with "intent satisfied". Add a typed `success_criterion` + `assert_verdict(ok,reason)`; the grader's verdict object can now be **native-schema-guaranteed**. |

### Prioritized solution plan

| pri | layer | effort | item |
| --- | --- | --- | --- |
| **P0** | cli-flag-wiring | S | claude worker: pass `--json-schema <spec.schema>`, read `result.structured_output` as primary; keep prompt+`extract_json_object` as fallback |
| **P0** | cli-flag-wiring | S | codex worker: write schema to `session_dir/output-schema.json`, pass `--output-schema <file>` (thread `spec.schema` down from `main.rs:4128`) |
| P1 | starlark-frontend | S | `json.encode/decode` (one-line globals change) → enables forward-injecting a prior `agent()` dict |
| P1 | runtime-tracking | S | parse claude `total_cost_usd` onto `StepResult` (prerequisite for any budget tally) |
| P1 | runtime-tracking | M | cumulative run budget: `workflow(budget_usd=…)` + `--max-budget-usd` CLI + `StarlarkCtx.spent_usd` short-circuit; claude `--max-budget-usd` as per-worker backstop |
| P2 | cli-flag-wiring | M | tighten sandbox defaults + default-isolate editing nodes |
| P2 | runtime-tracking | M | typed `success_criterion` + asserted verdict → `Completed` requires `verdict.ok` |
| P2 | runtime-tracking | S | stale-run reaper: finalize `Running` rows past max-age on startup |
| P3 | starlark-frontend | M | expose `pipeline()` as data-template stages with forward-injection |

### Honest negatives / open questions

- Native schema was shown to **produce** conforming output but **not** to **reject**
  a violating reply — hard-gate behavior unproven; keep prompt+retry fallback until
  a negative test confirms rejection.
- codex `--output-schema` reproduced under `--sandbox read-only`; not re-tested
  under the production `workspace-write`.
- claude `total_cost_usd` may be billed-API cost, not marginal spend on a
  subscription; codex emits **no** dollar figure (estimate from a price table).
- claude `--max-budget-usd` overshoot at realistic ($0.50–$2) caps unmeasured;
  `parallel()` dispatches a whole barrier before tallying (enforcement granularity).
- no stable `step_id` exists, so `--resume`/leaf-replay needs that first (ship the
  cheaper reaper before resume).

**Takeaway:** the biggest "ceiling" I named (structured output) is a **wiring gap** —
two S-effort flag changes (claude `--json-schema`, codex `--output-schema`) close
J/K on both providers. The cheap wins (those two + `json.encode` + cost capture)
are S-effort and unblock the rest.

> Evidence: `wf_acee5689-c17` ran claude 2.1.160 + codex-cli 0.135.0 on this
> machine; every status reproduced-from-command. Full data `/tmp/gap-research.json`.

# Dynamic Workflow Runtime — Design

A design study for a **Rust-native Dynamic Workflow runtime** in our harness,
modeled on Claude Code (CC) Workflows, whose purpose is to **orchestrate
multiple `codex` + `claude-code` agents** under deterministic control flow.
Sections 1–8 are the **original design study** (illustrative Rust only); the
**As-built** block immediately below records what actually shipped and is the
source of truth where the two differ. The locked decisions live in
[ADR 0022](../decisions/0022-dynamic-workflow-runtime-json-ir.md).

## 0. As-built (shipped) — skill + CLI + JSON-IR

> **Superseded in part (see [ADR 0023](../decisions/0023-starlark-workflow-frontend.md)).**
> The JSON-IR specifics in this block — `harness workflow run-spec`,
> `WorkflowSpec`/`WorkflowNode`/`dispatch_spec()`,
> `schemas/workflow-spec.schema.json`, and `scripts/acceptance-dynamic-workflow.mjs`
> — were **deleted** once the Starlark front-end proved a strict superset.
> The dynamic authoring surface is now a **Starlark program** run via
> `harness workflow run-script <prog.star>` (with a mandatory
> `workflow(name, design_intent)` header), authored with the
> [`skills/star-workflow`](../../skills/star-workflow) skill. The runtime
> primitives (`parallel()`/`pipeline()`/scheduler), the `crates/harness-workflow`
> crate, the additive object fields, and the one-journal/one-dashboard contract
> below all **still hold** — Starlark just replaced the JSON-IR as the producer.

The study recommended Hybrid **Option C** (§3): ship named compiled Rust
workflows behind a registry first, defer the runtime-authored IR (Option B) to
WP5. WP1–WP4 landed that registry runtime. Stages 1–5 of the implementation then
**promoted the deferred Option-B IR into a shipped, runtime-authored path** so
any agent can author a workflow shape at runtime. The as-built decisions
(per [ADR 0022](../decisions/0022-dynamic-workflow-runtime-json-ir.md)):

- **Trigger = skill + CLI, not MCP/plugin.** `harness workflow run-spec
  <spec.json>` is the contract, alongside the registry `workflow run --name`. An
  `star-workflow` skill (`skills/star-workflow/SKILL.md`) teaches an
  agent to write a spec, invoke the CLI, and read the run back. No plugin, no new
  transport; an MCP shim over the same CLI is an optional, unbuilt future.
- **Dynamic spec = JSON-IR, not embedded JS.** A spec is a schema-validated JSON
  document (`schemas/workflow-spec.schema.json`) →
  `WorkflowSpec { name, args, nodes }` with
  `WorkflowNode = Agent | Phase | Parallel | Pipeline`. `dispatch_spec()` walks
  the IR; `{{key}}` placeholders interpolate from `args`. This is Option B (§3.B)
  realized as **data, not foreign code** — the study's "no JS VM" decision (§1.1)
  holds.
- **Runtime extracted into `crates/harness-workflow`.** The scheduler,
  `agent()`/`parallel()`/`pipeline()`, the IR, and `dispatch_spec()` moved out of
  `harness-cli/src/workflow.rs` into a provider-agnostic lib crate behind the
  injected `AgentStepFn` seam. `harness-cli` injects the real driver
  (`workflow_real_agent_step` → `run_provider_delivery`), and the registry `run`
  and dynamic `run-spec` paths share one `journal_workflow_outcome`.
- **Additive object fields only** under current schema governance:
  `WorkflowRun.args` / `.agents_spawned` / `.final_output` and
  `WorkflowStep.result`. `pipeline()` is **real streaming** (no barrier; a failed
  stage drops the item and skips its rest), not a `parallel()` fallback.
- **One surface for both paths.** The Dashboard Workflows surface + per-node
  `TurnDrillIn` drill-in render a `run-spec` run identically to a registry run.
- **Proof:** `scripts/acceptance-dynamic-workflow.mjs` authors a two-provider
  serial → parallel → pipeline spec, runs it via `run-spec --dry-run`, and
  asserts the journaled run/steps shape + `final_output` (also over the live
  `/v1/snapshot`).

What changed vs. the study below: owner-decision 3 ("design only", §1) is
superseded — code shipped. Owner-decision 1 (no JS VM) and 2 (Workflow is an
independent object, not yet `Task`/`Goal`-bound) **still hold**. WP5's "promote
to an IR later" (§3 C, §7) is the path that was taken. The §3 Option A/B/C
analysis is retained as the rationale for why the shipped IR is data-only.

---

The remaining sections are the original design study.

Conceptual basis: the owner research report (settled mechanism, taxonomy,
three-plane architecture, two-phase intent→workflow→execution model, evidence
ledger) lives at the plain path
`/.research-cache/dynamic-workflows/report.md` (gitignored, not committed;
cited by section number `§N` below, not re-derived). The CC mechanism spec is
the second input and is cited as "CC spec §N". Harness building blocks are cited
inline as `file:line`. Ties to
[0018](../decisions/0018-exec-stream-primary-substrate.md),
[0011](../decisions/0011-provider-neutral-runtime.md), and
the current schema-governance boundary.

## 1. Goal & scope

**Goal.** A runtime that executes a deterministic orchestration program which
fans work out to **real provider subprocesses** — `codex exec --json` and
`claude -p --output-format stream-json` — concurrently, collects their results
in-process, runs dependent reduce/synthesis steps, and journals every agent call
so a stopped run can resume. It is the Rust realization of the report's
two-phase model (§3.3, §6.4): *first compile intent into an executable
multi-agent program, then schedule + pass data + review + synthesize.*

**Three owner decisions frame this design:**

1. **Rust-native. Do NOT embed a JS interpreter.** CC's orchestration program is
   JavaScript only because CC is itself JS — the script runs in-process in the
   same Node runtime (CC spec §1, §2). Our harness is Rust (`harness-core`,
   `harness-store`, `harness-cli`), provider-neutral
   ([0011](../decisions/0011-provider-neutral-runtime.md)), and exec-stream based
   ([0018](../decisions/0018-exec-stream-primary-substrate.md)). Embedding a JS
   VM to host orchestration logic would import a foreign runtime and a foreign
   determinism contract for no structural gain. The interesting question (§3) is
   therefore *how a Rust harness expresses dynamic control flow at all.*

2. **Workflow is an INDEPENDENT object for now — NOT bound to `Task`/`Goal`.** A
   `WorkflowRun` references neither a `Goal` (`harness-core/src/lib.rs:14`) nor a
   `Task` (`lib.rs:453`) yet. It is a standalone object with its own id,
   lifecycle, and journal. Binding to the `Task` DAG (`Task.depends_on_task_ids`
   `lib.rs:463`) is deferred to a later WP (§7, WP6) so the runtime can be proven
   in isolation first, per the report's framing that Dynamic Workflow is *not*
   the long-lived business DAG (§3.3.2, §4).

3. **Design only.** No code lands from this doc. Rust snippets are illustrative.

**Out of scope:** mid-run human input (the report's human-in-the-loop boundary,
§2.5 — no mid-run user prompt; sign-off splits into multiple runs); binding to
`Goal`/`Task`; a savable user-authored DSL (deferred, §7 WP5); replacing the
autonomy runner (§8).

## 2. The mechanism we copy (condensed)

Settled by the report; **not re-derived here.** The four-form taxonomy (§1, §4),
three-plane architecture (Control / Data / Execution-Observation, §3.3.1), the
artifact data-flow model (§3.3.2), the `agent()` two-level cache/journal anatomy
(§3.3.3), the proven-vs-hypothesis parallel boundary (§3.3.5), the runtime
lifecycle (§3.3.7), and the evidence/runtime-boundary ledger (§2.5) are the
foundation. This section condenses only the **execution semantics** we must
reproduce, from the CC spec.

| Primitive | Exact semantics (CC spec §1–§2) | Failure mode |
| --- | --- | --- |
| `agent(prompt, opts) -> T \| null` | Spawn one isolated subagent; return its final text, or schema-validated/coerced JSON if `opts.schema` set. Journaled by `hash(prompt + opts)`. | Crash/timeout/validation-fail → `null`; never throws. |
| `parallel(thunks) -> [T\|null]` | **Barrier.** Run all thunks up to the concurrency cap; **wait for all** before returning, in input order. | A failed thunk → `null` in its slot; barrier still returns all. |
| `pipeline(items, ...stages) -> [T\|null]` | **No barrier.** Streaming: item A may be at stage 3 while B is at stage 1. A stage returning `null` drops that item. | Stage `null` → item dropped, others continue. |
| `phase(title)` | Declarative grouping marker for progress; zero execution cost. | n/a |

**Scheduler invariants (CC spec §2, §6):** hard concurrency cap
`min(16, cores-2)` concurrent agents; lifetime cap **1000 agents/run**; FIFO
queue for excess; per-agent `timeout`/`budget`; **deterministic resume** —
re-run the script top-to-bottom, return cached results for matching
`(prompt, opts)` journal entries (longest-unchanged-prefix fast path), re-spawn
on first divergence; **null propagation** — failed agents never abort the run.
Non-determinism (`Date.now`, `Math.random`, ambient I/O) is banned *at the
orchestration layer* because it breaks the resume cache key; the agents
themselves stay non-deterministic (§3.3.3, CC spec §3). The barrier-vs-streaming
distinction (CC spec §1) is the single most important behavioral contract to
preserve.

## 3. The Rust expression problem — THE key decision

CC's orchestration program is **arbitrary JS control flow** — `while`, `if`,
`.map`, native dedup/vote/reduce — run in-process (CC spec §5). That is exactly
the report's "plan moved from prompt/context to code/runtime" (§1, §4): *the
script holds the plan.* In Rust we cannot accept arbitrary user code at runtime
without an interpreter. So: **what plays the role of "the script"?** Two
faithful options, plus a hybrid.

### Option A — Rust-code-as-orchestration (each workflow is a compiled `async fn`)

Each workflow is a Rust function that calls runtime primitives directly and uses
native control flow (`join!`, `futures::future::join_all`, `for`, `if`, a
`while` loop) for fan-out/reduce.

```rust
// Illustrative. `rt` is the runtime; primitives mirror CC semantics.
async fn repo_change_with_review(rt: &Wf, args: ReviewArgs) -> Value {
    rt.phase("survey");
    let findings = rt.parallel(args.files.iter().map(|f| {
        let f = f.clone();
        move || rt.agent(format!("audit {f}"), AgentOpts::codex())
    }).collect()).await;                 // barrier

    rt.phase("synthesize");
    let merged = dedup_native(&findings); // plain Rust, no agent spawned
    rt.agent(format!("write report from {merged:?}"), AgentOpts::claude()).await
        .unwrap_or(Value::Null)
}
```

- **Pro:** full expressivity, real `async`/borrow-checked, trivial native
  reduce/vote, zero new language surface, deterministic by inspection.
- **Con:** a *new workflow = a recompile.* Not runtime-dispatchable, not
  user-authored, not directly visualizable as a graph (control flow is opaque
  Rust). The plan is held by **the binary**, not by a runtime artifact.

### Option B — A small orchestration IR the runtime interprets

Define a control-flow-capable step graph — a typed IR with `AgentCall`,
`Parallel`, `Pipeline`, `Phase`, `Loop { until }`, `Cond { when }`,
`DynamicFanOut { over, body }` nodes — that the runtime walks. New workflows are
data (JSON/RON), dispatched at runtime, and renderable as a graph.

```rust
// Illustrative IR (subset).
enum Node {
    Phase(String),
    Agent { prompt: Tmpl, opts: AgentOpts, bind: VarId },
    Parallel(Vec<Node>),                 // barrier
    Pipeline { items: Expr, stages: Vec<Node> }, // streaming
    Loop { until: Expr, body: Box<Node> },
    Cond { when: Expr, then: Box<Node>, els: Option<Box<Node>> },
    FanOut { over: Expr, body: Box<Node> },
}
```

- **Pro:** runtime-dispatchable, visualizable, journalable per node, the plan is
  a **runtime artifact** (closest to CC's "plan in code/runtime", §1, §4); a
  later author/DSL layer can target it.
- **Con:** we must *define and maintain a little language* — `Expr`/`Tmpl`
  evaluation, var scoping, type checking, error semantics — i.e. we reinvent a
  slice of the interpreter the owner told us not to embed. High scope-creep risk
  (§8) and easy to under/over-build before any real workflow exists.

### Option C — Hybrid (RECOMMENDED): built-in named Rust workflows now, IR later

Ship **named, registered Rust workflows** (Option A) behind a runtime registry
keyed by name — `Wf::dispatch("repo-change-with-review", args)` — so workflows
are **selectable at runtime by name** (the dynamic-dispatch the harness wants)
even though each body is compiled. This delivers the entire scheduler, journal,
concurrency cap, SSE, and multi-provider payload **immediately** with zero new
language surface. Then, *only once two or three real workflows exist* and the
node-shape has stabilized, factor the proven control-flow shapes into the
Option-B IR (§7 WP5) — at which point the IR is informed by reality instead of
guessed.

```rust
// Registry gives runtime dispatch without an interpreter (WP1–WP4).
type WfFn = fn(&Wf, Value) -> BoxFuture<'_, Value>;
struct Registry { by_name: BTreeMap<String, WfFn> }
// WfInput { name, args, run_id, resume_from_run_id } selects the fn at runtime.
```

**Why C, tied to "who holds the plan" (§4).** The report's axis is *where the
plan lives.* Option A puts it in the binary (not dispatchable); pure Option B
puts it in an artifact but forces us to build an interpreter first. C keeps the
plan in **compiled Rust** but exposes it through a **named registry + typed
`WfInput`**, so the harness can dispatch, journal, resume, and visualize *runs*
now, and migrate the *plan representation* to the IR later without touching the
scheduler. For a Rust, provider-neutral, dynamically-dispatched harness with
**zero real workflows today**, C is the only option that is both faithful and
incremental ([CLAUDE.md]: incremental over big-bang; boring over clever).

**Recommendation: C** — built-in named Rust workflows behind a runtime registry
now; promote to an IR once the node shapes are proven.

## 4. Mapping CC primitives onto our harness

| CC primitive | Our equivalent | Building block (`file:line`) |
| --- | --- | --- |
| `agent(prompt, opts)` | One exec-stream provider delivery: spawn `codex exec --json` / `claude -p stream-json`, parse NDJSON, reduce to `AgentEvent` + a `ProviderSession` row | `run_provider_delivery` `main.rs:7452`; Claude `run_claude_exec_delivery_real` `main.rs:7685`; Codex `run_codex_exec_process` `main.rs:7201` |
| agent slot lease | Single-owner claim under global `flock`; rejects if a session already blocks the agent | `claim_queued_message_delivery` `harness-store/src/lib.rs:138` (`BlockedBySession` `lib.rs:152`); TTL reclaim `expire_safe_delivery_claims_value` `main.rs:3851` |
| `parallel()` (barrier) | N worker threads draining a bounded queue, join all | new scheduler on std `thread::spawn` + `crossbeam` channels (§3, recommend std; see below) |
| `pipeline()` (streaming) | Per-stage worker pools; item advances to stage+1 on completion, no barrier | same scheduler, stage queues |
| concurrency cap `min(16,cores-2)` | A counting semaphore over worker slots | std-thread permit count (cf. Multica slot semaphore, [multica-architecture.md](multica-architecture.md) `daemon.go:2037-2043`) |
| `phase(title)` / `log()` | A journaled phase marker + `AgentEvent` rows; live via SSE | `AgentEvent` `harness-core/src/lib.rs:628`; `start_sse_watcher` `sse.rs:80`; `SseManager` `sse.rs:35` |
| journal / deterministic resume | Append-only JSONL + per-row `fsync` + latest-wins projection, keyed on step identity | `append_jsonl_unlocked` `lib.rs:261`; `latest_by_id` `lib.rs:324`; lock `acquire_write_lock` `lib.rs:282` |
| `workflow(name, args)` nesting | A child `WorkflowRun` sharing the parent semaphore + agent counter + abort (one level, §3.3.5 hypothesis) | registry dispatch (§3 C); deferred to WP5 |
| `args` / `budget` | `WfInput.args: Value` (must be a JSON value, §3.3.7) + a per-run budget snapshot | new object (§5) |
| 1000-agent lifetime cap | A per-run counter checked before each spawn | new `WorkflowRun.agents_spawned` (§5) |

**tokio vs std — recommend std threads + crossbeam.** The harness has **no
tokio** (Input C: `Cargo.lock` has no `tokio`/`async-*`; `mio` is transitive via
`notify`, which is itself declared-but-unused). The HTTP server is
`std::net::TcpListener` with **`std::thread::spawn` per connection**
(`serve_command` `main.rs:2339`, spawn at `main.rs:2379`); SSE fan-out uses
`crossbeam::channel::bounded` (`sse.rs:10,49`); loops are `thread::sleep` polls
(`run_autonomy_loop` `main.rs:1278`). A bounded parallel-agent scheduler is a
worker pool: N `thread::spawn` workers draining a `crossbeam` bounded queue, the
queue depth **is** the concurrency cap. The store's `flock` +
`BlockedBySession` already makes cross-agent parallelism safe at the persistence
layer (Input C §2), so the scheduler only caps thread count. Pulling in tokio
would mean an async rewrite of the exec-stream `try_wait` poll loops
(`main.rs:7771-7798`, `:7262-7274`) for no benefit at this scale (16 threads).
**Decision: std threads + crossbeam; revisit only if the cap rises far past 16.**

## 5. Proposed Workflow object + run model

Additive under the current schema-governance boundary:
new objects in `harness-core/src/lib.rs`, new `append_*`/reader methods in
`harness-store` (the typed-append pattern at `lib.rs:66-135`), surfaced via SSE
frames (`SseEventFrame` `sse.rs:18`). **No existing object changes; no
`Task`/`Goal` foreign keys yet** (owner decision 2).

```rust
// Illustrative; lands in harness-core later, journaled to JSONL.
struct WorkflowDef { name: String, summary: String }      // registry metadata
struct WorkflowRun {
    id: String,                 // runId (UUID) — journal/SSE key
    def_name: String,           // selects the registered Rust fn (§3 C)
    args: serde_json::Value,    // JSON value, never a string (§3.3.7)
    status: WfRunStatus,        // Pending|Running|Paused|Completed|Failed
    started_at: String, ended_at: Option<String>,
    agents_spawned: u32,        // enforces the 1000 lifetime cap
    final_output: Option<serde_json::Value>,
}
struct WorkflowStep {           // one agent() call == one ProviderSession
    id: String, run_id: String, phase: Option<String>,
    step_key: String,           // hash(prompt + opts subset) — resume cache key
    prompt_preview: String,     // first N chars, for the progress view (§3.3.7)
    provider_session_id: Option<String>, // -> ProviderSession lib.rs:558
    status: WfStepStatus,       // Queued|Running|Complete|Failed|Null|Cached
    result: Option<serde_json::Value>,
}
```

Each `WorkflowStep` **maps onto an existing `ProviderSession`**
(`lib.rs:558`) — the single-execution record (provider, command/args, stdout/
jsonl refs, `exit_code`, timings, `evidence_ids`). The step is the
workflow-layer wrapper that adds `run_id`, `phase`, and the resume `step_key`.

```
WfRunStatus lifecycle
  Pending --start--> Running --all steps terminal--> Completed
                        |                                ^
                        |--stop--> Paused --resume--> ----+
                        |--fatal (cap exceeded / def panic)--> Failed
WfStepStatus
  Queued -> Running -> { Complete | Failed | Null }
  Queued -> Cached            (resume: matching step_key already Complete)
```

Run/step/phase rows append to `workflow_runs.jsonl` / `workflow_steps.jsonl`
exactly like every other object (append+`fsync`, `lib.rs:261`; latest-wins
projection is the **resume primitive**, `lib.rs:324`). The SSE watcher tails
those files by byte offset (`sse.rs:84-99`) and broadcasts new frame variants
(`WfRun`, `WfStep`, `WfPhase`) so the dashboard renders live phase/agent/token
progress, reusing the snapshot+tail machinery unchanged.

## 6. Orchestrating multiple codex + claude — the target scenario

**Scenario.** "Investigate failure X." Fan out across **both providers** — 2
`codex` auditors + 1 `claude` auditor in parallel (barrier) — then a dependent
**`claude` synthesis** step reduces the three findings into one report. This is
the payoff: the runtime claims and spawns three exec-stream deliveries
concurrently under the semaphore, collects results in-process, and runs the
dependent step — none of the auditor transcripts ever enter the synthesis
context except as the runtime-passed `findings` (the report's data-plane,
§3.3.1–§3.3.2).

In the recommended expression form (§3 C — a registered Rust workflow):

```rust
async fn investigate(rt: &Wf, args: Value) -> Value {
    rt.phase("audit");                                   // barrier fan-out
    let findings = rt.parallel(vec![
        thunk(|| rt.agent("audit module A".into(), AgentOpts::codex())),
        thunk(|| rt.agent("audit module B".into(), AgentOpts::codex())),
        thunk(|| rt.agent("audit recent diffs".into(), AgentOpts::claude())),
    ]).await;                          // Vec<Option<Value>>, nulls tolerated

    rt.phase("synthesize");                              // dependent reduce
    let kept: Vec<_> = findings.into_iter().flatten().collect();
    rt.agent(format!("synthesize one report from: {kept:?}"),
             AgentOpts::claude()).await.unwrap_or(Value::Null)
}
```

```
                       WorkflowRun(investigate)  [semaphore cap = min(16,cores-2)]
                                  |
        phase:"audit"  ── parallel() BARRIER ──────────────────────────
            |                      |                      |
   claim+spawn              claim+spawn            claim+spawn        (3 slots,
   codex exec --json        codex exec --json      claude -p          3 worker
   [auditor A]              [auditor B]            stream-json        threads)
     ProviderSession          ProviderSession        ProviderSession
        |                      |                      |
      result A?              result B?              result C   ── nulls tolerated
            \__________________|______________________/
                               | (barrier: all three terminal)
                  findings = [A, B, C]  (in-process, data plane)
                               |
        phase:"synthesize" ── dependent step ──> claim+spawn
                                                 claude -p stream-json
                                                 [synthesis over findings]
                                                   ProviderSession
                                                       |
                                                  final_output  ──> WorkflowRun.Completed
```

Each `claim+spawn` reuses `claim_queued_message_delivery` (`lib.rs:138`) for the
single-owner lease and `run_provider_delivery` (`main.rs:7452`) for execution;
the three run on three worker threads bounded by the semaphore; the barrier is
`join`-all of the three worker handles; the synthesis step is a fourth delivery
that runs only after the barrier. Every spawn increments
`WorkflowRun.agents_spawned` (1000-cap) and journals a `WorkflowStep` keyed by
`step_key` so a stop-and-resume returns the cached `result A/B/C` instead of
re-spawning (CC spec §3).

## 7. Sequenced WP plan

Each WP compiles, passes `npx pnpm@9.15.4 check`, and is independently
committable ([CLAUDE.md]: incremental, working code).

| WP | Deliverable | Size | tokio? |
| --- | --- | --- | --- |
| **WP1** | Minimal runtime: one built-in Rust workflow doing `agent()` ×2 (1 codex + 1 claude) **serial then parallel** over real providers via `run_provider_delivery`; results collected; journaled to `workflow_runs.jsonl`/`workflow_steps.jsonl`. No cap, no SSE yet. | S–M | none (direct calls) |
| **WP2** | Scheduler: std-thread worker pool + `crossbeam` bounded queue; concurrency cap semaphore (`min(16,cores-2)`); 1000-agent lifetime cap; barrier `parallel()` + streaming `pipeline()`; live progress via new `SseEventFrame` variants (`sse.rs:18`). | M | **decision point — recommend std** (§4) |
| **WP3** | `WorkflowDef`/`WorkflowRun`/`WorkflowStep` objects in `harness-core` + `append_*`/readers in `harness-store`; dashboard surfacing (phase/agent/token progress); `/workflows`-style run list + snapshot. | M | none |
| **WP4** | Resume: step-identity keying (`step_key = hash(prompt + opts subset)`), longest-unchanged-prefix fast path, `Cached` step status, `resume_from_run_id` in `WfInput`. Re-uses latest-wins projection (`lib.rs:324`). | M | none |
| **WP5** *(later)* | Promote proven node shapes into the Option-B IR (§3) for runtime-dispatchable / user-authored / visualizable workflows; optional one-level `workflow()` nesting (§3.3.5). | L | none |
| **WP6** *(later)* | Bind `WorkflowRun`/`WorkflowStep` to `Task`/`Goal` (`Task.depends_on_task_ids` `lib.rs:463`; `requires_human_approval` `lib.rs:476`); a workflow run can advance a Task DAG and gate on a Decision/Review. | L | none |

**Critical path:** WP1 → WP2 → WP3 → WP4 are the runtime. WP5/WP6 are deferred
by owner decisions (1: defer DSL; 2: defer Task/Goal binding). The tokio-vs-std
choice is a **WP2 dependency** and is recommended resolved as std (§4).

## 8. Risks & open questions

- **tokio vs std (resolved, revisitable).** Recommended std threads + crossbeam
  (§4) — matches the existing substrate (no tokio; `serve` is thread-per-conn
  `main.rs:2379`) and the 16-thread scale. Risk: if a future cap rises to
  hundreds of concurrent agents, thread-per-agent gets expensive and an async
  rewrite of the exec poll loops (`main.rs:7771-7798`) becomes warranted. Flag,
  don't pre-optimize.
- **Deterministic resume via step-identity keying.** CC keys on
  `hash(prompt + opts)` (CC spec §3); the report notes the native bundle builds a
  **chained** key from prompt + an opts subset (`schema`/`model`/`isolation`/
  `agentType`) + prior key (§3.3.3), and whether `label`/`phase` participate is
  **unproven**. Open: pick the exact `step_key` formula (start with
  `hash(prompt + {model,schema})`, no chaining) and decide whether `phase`
  affects cache granularity — the report explicitly flags this as *not provable
  from public schema* (§3.3.1). Banning orchestration-layer non-determinism is
  trivial in Rust (the workflow `fn` simply must not read clocks/RNG/ambient I/O
  outside `agent()`), but is a convention we must document and lint, not a
  language guarantee.
- **IR scope creep (the main §3 risk).** Building Option B before real workflows
  exist risks an over-/under-fit little language (`Expr`/`Tmpl` eval, scoping,
  typing). Mitigation: Hybrid C — ship registered Rust workflows first, derive
  the IR from ≥2 proven workflows (WP5). Do not start WP5 speculatively.
- **Relation to the existing autonomy runner.** `autonomy_tick_value`
  (`main.rs:1239`) + `run_autonomy_loop` (`main.rs:1278`) are a hardcoded
  observe→plan→decide→deliver loop with a pre-execution approval gate
  (`main.rs:1251`; commits f2dbdc6, dd4c3ff). It is a *workflow expressed in
  Rust* already (Option A in disguise). Open: does the workflow runtime
  **subsume** it (re-express the tick as a registered workflow) or **sit beside**
  it? Recommend *sit beside* through WP4 (don't destabilize a working gated
  loop), then consider subsuming in/after WP6 once Task/Goal binding exists — the
  autonomy gate is exactly the `requires_human_approval` pattern (`lib.rs:476`) a
  Task-bound workflow would generalize.
- **Human-in-the-loop boundary (inherited, §2.5).** No mid-run user input;
  sign-off means splitting into multiple runs. The independent-object design
  (decision 2) keeps this clean now; WP6 must preserve it when binding to Tasks.

# Spike: should the external workflow be authored in a real language?

**Question (from the owner):** "our biggest problem is that we can't author the
external workflow in one of today's mainstream programming languages." Internal
`Workflow` uses **JavaScript**; external `run-script` uses **Starlark**. Is the
Starlark choice the real ceiling, and what would switching cost/buy?

This spike answers with **running code**, not opinion: the same scenario authored
three ways, plus a working feasibility proof for each candidate runtime.

> Reads on top of [`internal-vs-external-workflow.md`](./internal-vs-external-workflow.md)
> (the three root causes) — this spike is specifically about **root cause 1**
> (the authoring host).

---

## TL;DR

- **The intuition is right, and internal proves it.** Internal already runs
  *agent-authored* JavaScript safely by stubbing `Date.now`/`Math.random`. A real
  language + a determinism stub is a solved pattern, not a fantasy.
- **Both candidate runtimes are feasible — proven here, today.** An embedded
  pure-Rust JS engine (Boa) and a Node-subprocess-with-RPC both ran the same
  workflow-shaped script end to end in this environment (§2).
- **But a language swap only fixes root cause 1.** It buys real closures (→
  `pipeline` stages become callbacks, the #1 ergonomic loss) and verbatim porting
  of internal workflows. It does **not** touch root cause 2 (cold-CLI workers →
  flat-string schema, no MCP, explicit provider/sandbox) or root cause 3 (durable
  artifact → `design_intent`, journaling, reaper). Those are independent of the
  authoring language. So: a big authoring win, not a panacea.
- **`deno` is not installed here; `node` v22 is** (with the `--permission`
  sandbox). So the subprocess path would use Node, adding **no new runtime
  dependency**.

---

## 0. What a language swap fixes — and what it can't

| Symptom today | Root cause | Fixed by a real authoring language? |
| --- | --- | --- |
| `pipeline` stage is a data template, not a callback | 1 | **YES** — closures return |
| no `try/catch` around a leaf | 1 | **YES** |
| no `.map/.filter/.flat`, rich loops | 1 | **YES** (mostly there in Starlark too, but JS is richer) |
| schema fields forced to `string`, `.splitlines()` idiom | 2 | NO — it's the vendor-CLI schema bridge |
| no MCP; explicit `provider`/`model`/sandbox per leaf | 2 | NO — cold CLI worker has no session |
| mandatory `design_intent`, durable journal, reaper, `verdict` | 3 | NO — it's the standing-artifact contract |

So this spike is scoped to **closing root cause 1**. That is the single biggest
*authoring-ergonomics* problem (per the gap analysis), which is why it's worth it.

---

## 1. The same scenario, three ways

Scenario: review across N dimensions → keep findings → report. (Full Starlark
version: [`bug-hunt-verify.star`](../../skills/author-workflow/examples/bug-hunt-verify.star).)

### Today — Starlark (`run-script`)

The verify panel can't live *inside* a `pipeline` stage (a stage is a data
template, not a callback), so it's a `for`-loop over `parallel()`, and findings
come back as one-per-line strings to `.splitlines()`:

```python
finds = parallel([{ "prompt": "Find " + d["what"] + " ...", "schema": FINDINGS }
                  for d in DIMENSIONS])
candidates = []
for res in finds:                              # flatten flat-string findings
    for line in res["findings"].splitlines():
        if line.strip(): candidates.append(line.strip())
confirmed = []
for finding in candidates:                     # panel is a loop, not a stage
    panel = parallel([{ "prompt": "refute: " + finding, "schema": SKEPTIC }
                      for _ in range(3)])
    if sum(1 for v in panel if not v["refuted"]) * 2 > 3: confirmed.append(finding)
output(report); verdict(ok, reason="...")
```

### Candidate A — embedded JS (Boa, in-process Rust)

Same host functions, but a real language: the panel fans out *inside* a stage
callback, findings are real arrays of objects:

```js
const reviewed = pipeline(DIMENSIONS, [
  d => agent({ prompt: `Find ${d.what} ...`, schema: FINDINGS }),
  (review, d) => parallel(review.findings.map(f =>           // closure stage!
    () => agent({ prompt: `refute: ${f.title}`, schema: SKEPTIC })
            .then(v => ({ ...f, refuted: v.refuted })))),     // graft onto object
]);
const confirmed = reviewed.flat().filter(f => !f.refuted);
output({ confirmed: confirmed.length, report });
```

### Candidate B — Node subprocess (real TypeScript + RPC)

Byte-for-byte the **internal** authoring surface — `await`, `Promise.all`, npm if
wanted — running in a sandboxed Node process; `agent()` RPCs back to the harness:

```ts
const reviewed = await pipeline(DIMENSIONS,
  d => agent({ prompt: `Find ${d.what} ...`, schema: FINDINGS }),
  (review, d) => Promise.all(review.findings.map(async f =>
    ({ ...f, ...(await agent({ prompt: `refute: ${f.title}`, schema: SKEPTIC })) }))));
const confirmed = reviewed.flat().filter(f => !f.refuted);
output({ confirmed: confirmed.length, report });
```

A and B are essentially the internal program. That is the point: **the patterns
(and the actual internal workflows) port verbatim.**

---

## 2. Feasibility evidence (both ran in this environment)

### A — embedded Boa (pure Rust, no FFI)

`cargo add boa_engine` (built in **27s**), register `agent` as a native function,
run a workflow-shaped script with closures + structured returns:

```rust
let agent = NativeFunction::from_fn_ptr(|_t, args, ctx| {
    let label = args.get(0)...;                 // read the spec
    let obj = JsObject::with_object_proto(ctx.intrinsics());
    obj.set(js_string!("ok"), JsValue::Boolean(true), false, ctx)?;
    obj.set(js_string!("label"), js_string!(label), false, ctx)?;
    Ok(JsValue::from(obj))                       // return a structured dict
});
ctx.register_global_callable(js_string!("agent"), 1, agent)?;
ctx.eval(Source::from_bytes(
  r#"const ok = ["logic","failure","concurrency"].map(d => agent(d))
       .filter(f => f.ok).map(f => f.label);
     JSON.stringify({ confirmed: ok.length, labels: ok });"#))
```

**Output:** `RESULT: {"confirmed":3,"labels":["logic","failure","concurrency"]}` —
real closures + host-fn binding + structured return, in one Rust process.

### B — Node subprocess + stdio JSON-RPC

The workflow script is real JS (`Promise.all`, async `agent()`), with `Date.now`
and `Math.random` stubbed to throw (replay determinism); `agent()` writes a
JSON-RPC line and awaits the host's reply:

```js
Date.now = () => { throw new Error("clock blocked for replay determinism"); };
const agent = (spec) => new Promise(res => { waiters.push(res);
  process.stdout.write(JSON.stringify({ rpc:"agent", spec }) + "\n"); });
const reviews = await Promise.all(dims.map(d => agent({ label:`review:${d}` })));
```

The host (a Rust harness in production; a Node mock here) answers each `agent`
RPC. **Output:** `HOST GOT RESULT:
{"confirmed":3,"labels":["review:logic","review:failure","review:concurrency"]}`.
Node v22's `--permission` model (`perm-model-ok`) supplies the real sandbox.

---

## 3. Complexity / effort / risk

| Dimension | C. Keep Starlark, add callable stages | A. Embed Boa (Rust JS) | B. Node subprocess + RPC |
| --- | --- | --- | --- |
| Authoring language | Starlark (niche) | JavaScript | **TypeScript + npm** |
| Internal-parity (port verbatim) | partial | **high** | **highest** |
| New runtime dependency | none | a Rust crate (`boa_engine`) | Node (already present here) |
| Process model | unchanged (1 process) | 1 process | +1 subprocess per run (cheap next to codex/claude workers) |
| Determinism / replay | unchanged (hermetic) | stub clock/random (internal already does) | stub clock/random in a preamble |
| Sandbox | hermetic by construction | engine has no IO unless host adds it ✅ | Node `--permission` (deny fs/net) |
| The hard part | re-enter interpreter for stage callbacks; keep fan-out thread-safe | bridge **JS Promises ↔ our Rust thread pool** (async `agent`/`parallel`) | design the **RPC bridge** + lifecycle/cancellation/timeout |
| Engine maturity risk | n/a (shipping) | Boa is young; or use mature QuickJS via FFI (`rquickjs`) | V8 via Node — battle-tested |
| Rough effort | **S** (1–2 days) | **M** (the Promise↔thread bridge is the cost) | **M–L** (RPC bridge + sandbox wiring) |
| Migration of existing `.star` | none | rewrite examples (small) | rewrite examples (small) |

Notes:
- **A's core risk is the async bridge.** Our `parallel()`/`pipeline()` run real OS
  threads under a semaphore; a JS engine is single-threaded. Making `agent()`
  return a Promise that resolves off a Rust worker thread (or making `agent()`
  synchronous-blocking like Starlark) is the integration crux. Synchronous-blocking
  is simplest and keeps today's semantics; async matches internal's syntax.
- **B trades in-process simplicity for a real language + ecosystem and an
  off-the-shelf engine we don't maintain.** Since a run already spawns heavyweight
  codex/claude workers, one more Node process is negligible.
- **C is the cheap de-risking step:** it removes the #1 ergonomic loss without a
  language swap, and tells us whether closures-as-stages is "enough" before
  committing to A or B.

---

## 4. Recommendation

1. **Land C first (1–2 days).** Make `pipeline()` accept Starlark callables so a
   stage can fan out per item. This kills the single biggest ergonomic complaint
   with the least risk, and gives a real datapoint on whether a full swap is worth
   it. (Schema/RC2 stays — note that honestly.)
2. **If verbatim internal-parity is the goal, pick B (Node + RPC).** It is the
   purest "use a real existing language" answer: real TypeScript, the npm
   ecosystem, a battle-tested engine and sandbox we don't maintain, and internal
   workflows port nearly verbatim. The cost is the RPC bridge — well-scoped and
   testable. Both PoCs show the moving parts work.
3. **Pick A (embed Boa/QuickJS) only if single-process operation is a hard
   requirement** (no subprocess, no Node dependency). It gives the same authoring
   surface in-process, at the price of the Promise↔thread bridge and a younger
   engine (Boa) or a C-FFI boundary (QuickJS).

Decision criterion in one line: **if "no extra process / no Node dep" matters more
than "real TS + npm + a maintained engine," choose A; otherwise choose B — and
ship C now regardless, because it de-risks the whole question cheaply.**

---

## 5. What this spike did NOT settle

- The async vs synchronous `agent()` decision for A (affects whether the syntax
  matches internal's `await`).
- Cancellation/timeout/streaming (`--progress`) semantics across the RPC boundary
  in B.
- Whether to *replace* Starlark or run a *second* authoring frontend during
  migration. (A second frontend lets existing `.star` runs keep working.)
- None of root cause 2 / 3 — unchanged by any option here.

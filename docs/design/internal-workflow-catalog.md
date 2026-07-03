# Internal Workflow Catalog — 12 annotated reference programs

A study set of **12 real internal Claude Code Workflow programs**, mined from
`~/.claude/projects/<slug>/<session>/workflows/wf_*.json`, chosen to span the
whole pattern space (not 12 lookalikes). Each entry links the **full original
source** (extracted verbatim under [`internal-workflows/`](internal-workflows/))
and analyses its control flow, the transferable idiom, and which **external
workflow gap** (see [external-workflow-gap-analysis.md](external-workflow-gap-analysis.md))
it teaches us to close.

Read this alongside the gap analysis. The gaps, recapped:

- **A. open-loop → closed-loop** (act → verify → repair until a typed gate passes)
- **B. schema-gated branching** (structured output *drives* control flow)
- **C. leading plan, injected forward** (a Design/Decide agent's typed output parameterizes later agents)
- **D. locked shared context** (a `COMMON`/ground-truth preamble every agent shares)
- **E. real outcome gate** (behavioral / CI / from-zero acceptance, not a keyword grader)

### Host API (so the source reads cleanly)

These programs are JavaScript over a runtime that injects:
`phase(title)` (progress group) · `agent(prompt, {label, phase, schema, agentType})`
(returns the agent's text, or a **validated object** when `schema` is given) ·
`parallel([fn,…])` (barrier — awaits all) · `pipeline(items, …stages)`
(no-barrier streaming) · `log(msg)`. `Promise.all` composes sub-pipelines.

### The 12 at a glance

| # | program | agents | pattern | teaches gap |
| --- | --- | --- | --- | --- |
| 1 | [dynamic-workflow-impl](internal-workflows/dynamic-workflow-impl-wf_f6900ec3.js) | 5 | serial stages + `STAGE_RESULT.ok` gate + **1 repair pass** + stop-on-fail | A, B, D |
| 2 | [resident-daemon](internal-workflows/resident-daemon-wf_07defa4e.js) | 5 | design→impl→review→**conditional fix**→**verify loop ≤6** | A, B, C |
| 3 | [resident-agent-impl](internal-workflows/resident-agent-impl-wf_fbda5429.js) | 6 | parallel probe → **DECIDE(winner enum) → branch** → impl | C, B |
| 4 | [workflow-layout-design](internal-workflows/workflow-layout-design-wf_f91ff6a5.js) | 6 | understand → **3-way proposal tournament** → judge/synthesize | C, D |
| 5 | [multica-layout-review](internal-workflows/multica-layout-review-wf_f86139b9.js) | 26 | review → **one verifier per finding** (runtime-sized) → triage | dynamic |
| 6 | [object-model-research](internal-workflows/generic-harness-object-model-research-wf_e37ff5d7.js) | 6 | **Enumerate scout → scout-sized gather** → synthesize | dynamic |
| 7 | [object-model-build](internal-workflows/generic-harness-object-model-build-wf_6eabf27a.js) | 7 | 7-WP serial gated migration, auto-merge, `GATE_FAILED` cascade | A, E |
| 8 | [member-lead-claude-build](internal-workflows/member-lead-claude-build-wf_56fc2f22.js) | 8 | **two concurrent serial tracks** (`Promise.all`) merging one trunk | topology |
| 9 | [evaluate-external-workflow](internal-workflows/evaluate-external-workflow-wf_961f46bb.js) | 9 | assess → **adversarial verify** → synthesize (META: evals US) | B, E |
| 10 | [agent-live-tui-design](internal-workflows/agent-live-tui-design-wf_a8874e2e.js) | 8 | audit → **multi-angle design** → synthesize, audit-ctx injected | C, D |
| 11 | [operator-drives-team](internal-workflows/operator-drives-team-wf_b0be908d.js) | 5 | 4 WP + **real browser/CLI acceptance** (`ACCEPT_PASS`) | E |
| 12 | [xhs-market-research-routes](internal-workflows/xhs-market-research-routes-wf_d792276e.js) | 5 | non-coding map-reduce — **the contrast** (no gate, no loop) | contrast |

---

## Family 1 — Closed-loop build (act → verify → repair)

The single most important family, and the biggest external gap (A). The workflow
does not trust an agent's "done" — it runs the **real gate** and loops a fix
agent until green, with a hard attempt bound and stop-on-fail.

### 1. `dynamic-workflow-impl` · `wf_f6900ec3` · 5 agents · completed (~79 min)

[source](internal-workflows/dynamic-workflow-impl-wf_f6900ec3.js) — five sequential implementation stages, each verified before the next; **stop the chain on failure** so a human can intervene.

**Control-flow skeleton.** A `STAGES[]` array of `{id, prompt}` (the plan is the
array). The loop is the whole engine:

```js
for (const s of STAGES) {
  phase(s.id)
  let res = await agent(s.prompt, { schema: STAGE_RESULT }).catch(…)
  if (res && !res.ok) {                       // B: branch on typed `ok`
    const repair = await agent(COMMON + `REPAIR PASS for ${s.id}… Blockers:\n- ` +
        res.blockers.join('\n- ') + res.verification, { schema: STAGE_RESULT })
    res = repair                               // A: one repair pass
  }
  results.push(res)
  if (!res || !res.ok) { log('STOPPING…'); break }   // stop-on-fail
}
```

`STAGE_RESULT.ok` is *defined* as "true ONLY if every verify command passed"
(schema description, line 19), and each stage prompt lists the exact verify
commands (`cargo test --workspace`, `pnpm check:schema-fixtures`, a smoke run).
A shared `COMMON` preamble (lines 28–32) bakes the branch, the no-`--no-verify`
rule, and the commit discipline into every agent.

**Transferable idiom.** *Typed self-report + one bounded repair + stop-on-fail.*
The agent grades **itself** against named commands and returns a boolean; the
program trusts the boolean, not prose.

**For us (A, B, D).** Our external `.star` returns prose and never loops. The
minimal upgrade: give every "do work" node a `schema` with an `ok` boolean tied
to a concrete gate, then `if not res["ok"]: <repair>` — exactly this loop, in
Starlark.

### 2. `resident-daemon` · `wf_07defa4e` · 5 agents · completed

[source](internal-workflows/resident-daemon-wf_07defa4e.js) — the **most complete single-pass engineering loop** in the corpus: design → implement → adversarial review → *conditional* fix → bounded verify loop.

**Control-flow skeleton.**

```js
phase('Design');    const design = await agent(…, { schema: DESIGN })          // C
phase('Implement'); const impl   = await agent(`…\n${JSON.stringify(design)}`) // C: design injected
phase('Review');    const review = await agent(…, { schema: REVIEW })          // typed findings
const blocking = review.findings.filter(f => f.severity==='critical'||f.severity==='high')
if (blocking.length > 0) { phase('Fix'); await agent(`Fix:\n${JSON.stringify(blocking)}`) } // B
phase('Verify');
for (let attempt = 1; attempt <= 6 && !green; attempt++) {                       // A
  const v = await agent(`run cargo build/test/clippy…`, { schema: VERIFY })
  if (v.build_passed && v.test_passed) { green = true; break }
  await agent(`Fix the failures…\n${v.errors}`)                                  // repair
}
```

Three schemas do three jobs: `DESIGN` (11 required fields — the plan), `REVIEW`
(findings with a `severity` enum), `VERIFY` (booleans + raw `errors`). The
review **severity enum** is what makes the Fix phase conditional (line 118); the
verify **booleans** are what bound the loop (line 143).

**Transferable idiom.** *Plan → build → adversarially-review → fix-only-if-blocking
→ verify-until-green.* Every handoff is typed; every branch reads a field.

**For us (A, B, C).** This is the template a non-naive external coding workflow
should mirror end to end. Note the **adversarial review defaults to skepticism**
("Empty findings list is allowed if it is genuinely clean", line 115) — it does
not invent work.

---

## Family 2 — Plan-first / decision-gated (the "planning" the gap was felt in)

These have a **leading agent that produces a typed plan/decision/design**, which
is then `JSON.stringify`'d into downstream prompts as ground truth. This is gap
**C** — the seam entirely absent from our external workflows.

### 3. `resident-agent-impl` · `wf_fbda5429` · 6 agents · completed

[source](internal-workflows/resident-agent-impl-wf_fbda5429.js) — parallel probe → **decide a winner** → branch implementation on the verdict.

**Control-flow skeleton.**

```js
phase('Probe');
const [execSrv, appSrv, codeMap] = await parallel([ …3 typed probes… ])   // schemas
phase('Decide');
const decision = await agent(`Decide … strictly on these two probe reports:
  EXEC-SERVER:\n${JSON.stringify(execSrv)}\n APP-SERVER:\n${JSON.stringify(appSrv)}`,
  { schema: DECISION })                         // winner ∈ [exec-server|app-server|neither]
phase('Implement');
const impl = await agent(`Implement…\nGROUND TRUTH:\n${JSON.stringify(codeMap)}
  …decision doc:\n${JSON.stringify(decision)}`) // C: decision + code-map injected
```

The `DECISION` schema's `winner` is a 3-value enum (line 51). The implementation
prompt is **assembled from upstream structured output** — the `codeMap` probe
becomes the "GROUND TRUTH" file map the implementer must follow (line 118).

**Transferable idiom.** *Probe in parallel → a judge agent collapses the
evidence into a typed verdict → the verdict branches/seeds the work.* The
"planning" is an explicit agent step whose output is data, not prose.

**For us (C, B).** Our workflows jump straight to fixed fan-out. Adding a
`decide` node that returns `{winner: enum}` and branching the rest of the program
on it is the smallest possible step toward real agent-planning.

### 4. `workflow-layout-design` · `wf_f91ff6a5` · 6 agents · completed

[source](internal-workflows/workflow-layout-design-wf_f91ff6a5.js) — a **generate→evaluate→synthesize tournament**: understand the domain, generate 3 competing designs from orthogonal angles, judge & merge one winner.

**Control-flow skeleton.**

```js
phase('Understand');
const [domain, system] = await parallel([ understandDomain, understandDesignSystem ]) // typed
phase('Propose');
const philosophies = [ {key:'pipeline-centric',…}, {key:'master-detail-timeline',…},
                       {key:'document-narrative',…} ]                                  // 3 angles
const proposals = (await parallel(philosophies.map(p => () =>
    agent(`Propose a COMPLETE layout following:\n${p.brief}\n…domain:${JSON.stringify(domain)}
           …design system:${JSON.stringify(system)}`, { schema: PROPOSAL })            // D: ctx injected
))).filter(Boolean)
phase('Synthesize');
const synthesis = await agent(`You are the design lead. Here are ${proposals.length}
    proposals:\n${JSON.stringify(proposals)}\n Score them on (1)…(5)… then SYNTHESIZE ONE,
    grafting the best ideas from the others.`, { schema: SYNTHESIS })                   // judge
```

Fan-out is **data-driven over the `philosophies` array** (line 92) — dynamic in
shape. Every proposal agent is seeded with the *same* `domain` + `system` JSON
(gap D), so the three designs are comparable rather than drifting. The synthesizer
is an explicit **judge** that must say what it grafted from the runners-up.

**Transferable idiom.** *Diverge from orthogonal seeds sharing one context, then
a judge converges.* Beats one-shot-iterated when the solution space is wide.

**For us (C, D).** This is the divergent-convergent design pattern our skill
*teaches* but no example *uses*. The `JSON.stringify(domain)` injection is the
concrete form of "locked shared context."

### 10. `agent-live-tui-design` · `wf_a8874e2e` · 8 agents · completed

[source](internal-workflows/agent-live-tui-design-wf_a8874e2e.js) — same divergent-convergent shape, one stage deeper: a parallel **audit** of 4 fixed areas feeds an `AUDIT_CTX` into a parallel **design** of 3 fixed angles, then a synthesizer picks the best angle/hybrid. Schemas `AUDIT` / `DESIGN(angle+mockup)` / `FINAL(recommendation+wps[])`, all `additionalProperties:false`. Audits run with `agentType:'Explore'` (cheaper read-only). The lesson layered on #4: **the understanding phase's structured output is the shared context the proposal phase consumes** — `understand → propose → judge` generalizes to `audit → design → judge`.

**For us (C, D).** Confirms the pattern is a reusable *spine* (`understand* →
generate* → judge`), not a one-off; this is what a "design" external workflow
should look like.

---

## Family 3 — Dynamic decomposition (rare, the real high-water mark)

The only programs whose fan-out is sized/seeded by an agent's *runtime* output.

### 5. `multica-layout-review` · `wf_f86139b9` · 26 agents · completed

[source](internal-workflows/multica-layout-review-wf_f86139b9.js) — the only run whose parallel **width is parameterized by an earlier agent's output**: review emits a variable findings list, then one adversarial verifier is spawned **per finding**.

**Control-flow skeleton.**

```js
phase('Review');
const reviews = (await parallel(DIMS.map(d => () =>
    agent(d.prompt, { schema: FINDINGS, agentType: 'Explore' })))).filter(Boolean)
const all = reviews.flatMap(r => r.findings.map(f => ({ ...f, dimension: r.dimension })))
phase('Verify');
const verified = (await parallel(all.map(f => () =>                       // ← width = #findings
    agent(`Adversarially verify this finding… Default real=false if you cannot confirm.
           Finding: ${JSON.stringify(f)}`, { schema: VERDICT })
      .then(v => ({ ...f, ...v }))))).filter(Boolean)
const confirmed = verified.filter(f => f.real)                            // B: boolean filter
phase('Synthesize');
const plan = await agent(`…CONFIRMED findings… group into must_fix/should_fix/optional`,
    { schema: PLAN })
```

The adversarial verifier **defaults to `real=false`** (line 74) — it must
actively confirm a bug from the code or it dies. `confirmed = verified.filter(f
=> f.real)` is the boolean gate that suppresses hallucinated findings; with 26
agents most of the cost is this per-finding cross-examination.

**Transferable idiom.** *Find (cheap, broad) → verify each finding adversarially
(one agent each, default-refute) → keep only confirmed.* Runtime-sized fan-out.

**For us (dynamic, B).** Our eval's `workflow.star` already *imitates* this
(flattened skeptics) but without the default-refute discipline or the
`filter(real)` gate doing real work. This is the canonical "review harness."

### 6. `object-model-research` · `wf_e37ff5d7` · 6 agents · completed

[source](internal-workflows/generic-harness-object-model-research-wf_e37ff5d7.js) — the cleanest **scout-first** decomposition: a leading agent *discovers* the work, and its output sizes the next phase.

**Control-flow skeleton.**

```js
phase('Enumerate');
const skillList = await agent(`Use gh api to enumerate EVERY skill in <repo>… 
    Output ONLY the structured list — this drives the next phase.`)        // scout
phase('Gather');
const [lmtSkills, ourSchema, ourBackend, ourFrontend] = await parallel([
   () => agent(`…read the let-me-try skills… Here is the file list discovered:\n${skillList}…`),
   …                                                                       // C: scout output injected
])
phase('Synthesize');
const plan = await agent(`…INPUT A:${lmtSkills}\nINPUT B:${ourSchema}\n…produce a migration plan`)
```

The scout's text (`skillList`) is interpolated into the Gather prompt (line 23),
so the reading work is **scope-sized by what the scout found** — you cannot write
the Gather prompt until the scout runs.

**Transferable idiom.** *Scout → fan out over what the scout discovered →
reduce.* The honest answer to "we don't know the work-list until we look."

**For us (dynamic, C).** Our hybrid scouting pattern (scout inline, then
pipeline over the result) is exactly this; the external runtime supports it
(`agent()` returns text/objects you can splice into the next prompt) — we just
never do it in the examples.

---

## Family 4 — Serial gated delivery & topology

Author-static phases, but with strong delivery rigor: per-WP gate-before-commit,
`GATE_FAILED` early-abort cascade, auto-merge, and **real** acceptance.

### 7. `object-model-build` · `wf_6eabf27a` · 7 agents · completed (~98 min, the longest)

[source](internal-workflows/generic-harness-object-model-build-wf_6eabf27a.js) — a 7-work-package migration (schema → core → review → gap → learning → closeout → docs). Each WP runs on a clean checkout off latest `master`, **gates** (`cargo test` + `pnpm check` + tsc/vite, ~3 honest attempts), commits, opens+merges its PR, then `git pull --ff-only` so the next WP builds on merged work. A shared `APPROVED` design preamble + `.harness-genplan.md` carry the field anchors. The uniform short-circuit `if (out.startsWith('GATE_FAILED')) return {stoppedAt}` cascades prior results out on the first WP that cannot go green. The governing rule: **additive-optional schema** (new fields default-valued so existing jsonl/fixtures still validate).

**Transferable idiom.** *Sequential, independently-mergeable WPs, each its own
gate, fail-fast cascade.* The "plan" is human-authored; the agents execute and
verify it.

**For us (A, E).** Even without dynamic planning, this is how to make a *long*
external build reliable: a `GATE_FAILED:`-sentinel convention + a per-WP gate the
program checks before proceeding.

### 8. `member-lead-claude-build` · `wf_56fc2f22` · 8 agents · completed (~53 min)

[source](internal-workflows/member-lead-claude-build-wf_56fc2f22.js) — the most elaborate **topology**: two concurrent serial pipelines (5 FE WPs ∥ 3 BE WPs) each independently gating and merging to the **same trunk**.

**Control-flow skeleton.**

```js
const feTrack = (async () => { phase('FE track')
  const wp1 = await agent([RULES, …, FE_GATE, finish('task/fe-wp1', …)].join('\n'))
  if (typeof wp1 === 'string' && wp1.startsWith('GATE_FAILED')) return { stoppedAt:'FE-WP1', … }
  const wp2 = await agent(…); if (wp2.startsWith?.('GATE_FAILED')) return { wp1, stoppedAt:'FE-WP2' }
  …WP3,4,5…
  return { wp1,…,wp5, status:'FE-all-merged' } })()
const beTrack = (async () => { phase('BE track') …WP6,7,8… })()
const [fe, be] = await Promise.all([feTrack, beTrack])     // two pipelines, one barrier
```

Each track is an **IIFE async sub-pipeline**; `Promise.all` runs them
concurrently. `RULES` / `FE_GATE` / `BE_GATE` / `finish()` are composed string
constants — the `COMMON`-context idiom (gap D) taken to its logical end: every
agent is assembled from shared, locked rule blocks. BE WPs that need Claude CLI
knowledge use `agentType: 'claude-code-guide'` (line 112) — **mixed agent type**
inside one workflow.

**Transferable idiom.** *Independent delivery tracks in parallel, each a gated
serial chain, merging the same trunk.* Plus: **compose prompts from shared
constant blocks** so rules/gates are identical across agents.

**For us (topology, D).** Shows the runtime can host real parallel *pipelines*,
not just parallel single agents — and that prompt-assembly-from-constants is how
you keep N agents on the same page.

### 11. `operator-drives-team` · `wf_b0be908d` · 5 agents · completed (~62 min)

[source](internal-workflows/operator-drives-team-wf_b0be908d.js) — 4 sequenced WPs then a **real browser + CLI acceptance** agent. The acceptance step is the lesson:

```js
phase('Acceptance');
const accept = await agent([
  'You are the ACCEPTANCE agent… REAL browser + REAL snapshot — never fake.',
  'Start backend: cargo run … serve …; Start frontend: vite …; drive via agent-browser:',
  '  B. CREATE A TEAM from the UI -> confirm it appears in /v1/snapshot via curl…',
  '  E. OPERATOR MESSAGE … confirm in snapshot the message has sender_kind=operator…',
  'VERDICT: if A-E pass return "ACCEPT_PASS:" with per-check evidence + snapshot deltas.
   If a create path does not actually work, return "ACCEPT_FAIL:" … NEVER fake a pass.',
].join('\n'))
```

Verification is **behavioral and adversarially honest**: a real HTTP/CLI call
must produce the change and `dashboard snapshot` must reflect it; the `RULES`
block forbids hand-writing jsonl to satisfy acceptance (line 21). `ACCEPT_PASS:` /
`ACCEPT_FAIL:` are the machine-readable verdict; small failures are fixed-forward
then re-verified.

**Transferable idiom.** *A terminal acceptance agent that proves behavior
end-to-end against real state, with an anti-faking rule and a sentinel verdict.*

**For us (E).** Our external workflow has no real gate at all. This is what an
in-workflow outcome gate looks like — and the anti-faking discipline is exactly
what an eval needs so a run can't score by fabricating its own evidence.

---

## Family 5 — Evaluation & the contrast

### 9. `evaluate-external-workflow` · `wf_961f46bb` · 9 agents · completed

[source](internal-workflows/evaluate-external-workflow-wf_961f46bb.js) — **meta**: an *internal* workflow that evaluates *our external* runtime, across observability / execution / evaluability / production-readiness. Directly relevant to the eval-harness work.

**Control-flow skeleton** — note `pipeline` (no barrier), so each dimension's
adversarial verify starts the moment its assessment lands:

```js
const assessed = await pipeline(
  DIMS,
  (d) => agent(`${CTX}\n\n${d.prompt}`, { schema: ASSESS }),                  // assess
  (a, d) => agent(`${CTX}\n\nAdversarially CHECK this assessment… try to REFUTE each claimed gap…
      Return holds=true only if it stands.\nASSESSMENT:\n${JSON.stringify(a)}`,
      { schema: VERIFY }).then(v => ({ ...a, verdict: v })))                  // verify (refute)
phase('Synthesize');
const report = await agent(`…Honor the verifiers' corrections where holds=false…
    a PRIORITIZED roadmap (P0..P3)…`, { schema: REPORT })
```

A single big `CTX` constant (line 40) points every agent at the exact evidence
(snapshot API, store jsonl, durable traces, specific source functions) and orders
"ground EVERY claim in real evidence you actually read; do not speculate." The
verifier tries to **refute** (`holds` boolean, line 21); the synthesizer must
honor refutations. This is the three-grader doctrine (code/model/human) realized
as assess→refute→synthesize.

**Transferable idiom.** *Assess on a rubric → an independent agent tries to
refute → synthesize honoring refutations.* Evaluation as adversarial dialogue,
not a single pass.

**For us (B, E).** This is the shape our *eval graders* should take (LLM-judge +
adversarial verify), and the `CTX`-grounding-in-real-artifacts rule is how to
keep judges honest.

### 12. `xhs-market-research-routes` · `wf_d792276e` · 5 agents · completed — THE CONTRAST

[source](internal-workflows/xhs-market-research-routes-wf_d792276e.js) — a **non-coding** market-research workflow (Xiaohongshu/小红书 cultural-IP research). Deliberately included to show what falls away when the domain has no programmatic gate.

```js
phase('Research');
const research = await parallel(dims.map(d => () =>
   agent(`你是…研究员。${d.q}\n【真实数据简报】\n${DATA_BRIEF}\n要求:用WebSearch补充…不要编造数据…`,
         {label:d.label}).then(text => ({dim:d.key, text}))))
phase('Synthesize');
const combined = research.filter(Boolean).map(r=>`### ${r.dim}\n${r.text}`).join('\n\n')
const plan = await agent(`…把市场调研拆成【N条路线】…${combined}…`)   // map-reduce
```

Pure **map-reduce**: parallel breadth → one synthesis. There is **no verify loop,
no schema gate, no adversarial verify, no worktree, no real gate** — correctness
is delegated to *prompt-level discipline* (`DATA_BRIEF` shared context + "cite
real links, never fabricate numbers"). The synthesis step is itself a
meta-planning task (produce 4–6 prioritized research routes).

**Transferable idiom.** *When the output is human-judged prose, the whole closed-
loop/gating apparatus drops away — fan-out + a shared anti-hallucination brief is
enough.* The `DATA_BRIEF` is the gap-D pattern even here.

**For us (contrast).** This delimits *when* the heavy patterns matter: they pay
off for **verifiable** work (code, has a gate), and are overkill for open-ended
research. An eval that wants to show workflow > baseline must pick the former.

---

## How to use this catalog (reading order)

1. **To fix our biggest gap (A, closed-loop):** read #1 then #2 — the
   `STAGE_RESULT.ok` loop and the `verify ≤6` loop are the two shapes to copy.
2. **To add agent-planning (C):** read #3 (decide→branch) and #4 (understand→
   tournament→judge) — both inject a leading agent's typed output forward.
3. **To make verification real (E):** read #11 (browser/CLI `ACCEPT_PASS`) and #9
   (assess→refute→synthesize) — and apply the same anti-faking rule to eval graders.
4. **To see dynamic decomposition (rare):** #5 (per-finding fan-out) and #6
   (scout→sized gather).
5. **To calibrate when NOT to bother:** #12 — the contrast.

Across all 12, four habits recur and are what we should port into the
`star-workflow` skill as the **default skeleton**, not optional tips:
**(i)** a `schema` on every handoff; **(ii)** a `COMMON`/`CTX`/`RULES` constant
shared into every agent; **(iii)** branch on a typed field (`ok` / `severity` /
`winner` / `real` / `holds`); **(iv)** a verify→repair loop or a real acceptance
gate with a `GATE_FAILED:`/`ACCEPT_PASS:` sentinel and stop-on-fail.

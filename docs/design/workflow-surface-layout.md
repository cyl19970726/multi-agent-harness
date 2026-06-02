# Workflow Surface — Layout Design

> Read-only Workflow surface for the agent dashboard. One `SurfaceId` ("workflows")
> owns both the index (list of registered defs + runs) and the run detail, self-splitting
> on a selected `workflowRunId` — exactly the `agents` ↔ `memberId` pattern
> (`WorkbenchShell.tsx:476`). Light theme, semantic color only, hairline borders,
> composed entirely from existing `atoms.tsx` / `ui/*` primitives plus the verbatim
> `TurnDrillIn`.

---

## 0. Decision log: which proposal won, and why

Three proposals were scored on (1) faithfulness to workflow semantics, (2) consistency
with the dashboard, (3) scalability, (4) code-view clarity, (5) empty/loading states.

| Dimension | Pipeline Atlas | Master-Detail Timeline | Run Report |
|---|---|---|---|
| Faithfulness (serial/parallel/barrier, agent, live) | **Strong** (hero DAG animates) | Strong (vertical rail + join bar) | Adequate (nested list) |
| Consistency with dashboard | Weak (bespoke horizontal canvas, only thing not built from atoms) | **Strong** (1:1 AgentDetail) | **Strong** (pure DocumentSurface) |
| Scalability (many runs/steps/parallelism) | Medium (horizontal scroll) | **Strong** (vertical reflow) | **Strong** (vertical reflow) |
| Code view clarity | Strong (Graph/Source tabs) | Strong (two collapsibles) | **Strong** (always-on ASCII + lazy source) |
| Empty/loading | Strong | Strong | **Strong** |

**Chosen base: "Master-Detail Workflow Timeline" (Proposal 2).** It reuses the
AgentsList → AgentDetail muscle memory wholesale, costs zero new navigation model,
reflows vertically (so deep parallelism and long runs scale), and tells the
required-vs-tolerated story explicitly in a Verdict block. Its deliberate choice to
**fold the two-pane runs rail into the master-detail regime** (index doc = the rail,
detail doc = the timeline) is correct: the workspace is empty-first, so a permanent
rail is mostly dead space, and a standing split would be the one surface that breaks
every other surface's pattern.

**Grafted in:**
- From **Pipeline Atlas**: the index **per-run "shape glyph"** (inline `scope ▪ audit ▪▪`
  status pills) so the serial→parallel shape and per-step health read without opening
  a run; the **schematic def preview** on the registered card (so the empty workspace
  can show "what investigate IS" before any run); the live **progressive reveal**
  language (scope pulses → resolves → audit pair fans in → join settles).
- From **Run Report**: the **always-visible ASCII structural restatement** in the Code
  section (cheap, derived from steps already in hand, readable at zero runs) sitting
  *above* the lazy Rust source; the **calm single-column document** discipline; the
  explicit plain-English Verdict gloss.

**Two corrections to ALL three proposals, grounded in the actual Rust model**
(`harness-core/src/lib.rs:1261-1292`, `harness-cli/src/workflow.rs:261`):

1. **`WorkflowStep` has NO `member_id` field.** The persisted step is
   `{ id, run_id, phase, label, provider_session_id?, status, output_summary?, started_at, ended_at? }`.
   "Who ran it" is resolved EXACTLY like the existing chat drill-in: `step.provider_session_id`
   → `snapshot.provider_sessions[].agent_member_id` → the member. When `provider_session_id`
   is null (Running / Failed-before-delivery) there is no member yet — show "—". Wireframes
   in proposals 2 & 3 that printed `member_id` directly on the step are wrong; this doc fixes it.
2. **No `/v1/workflows/{name}/source` endpoint exists today.** The HTTP server has hardcoded
   routes (`main.rs:2525+`). The structural ASCII graph needs ZERO backend work; the **Rust
   source view is an additive enhancement** gated behind a feature check (renders an honest
   "source unavailable" when the endpoint 404s), so the surface ships without a backend dependency.

---

## 1. Information architecture

```
SurfaceId "workflows"
├── (no workflowRunId)  → WorkflowsList   — registered defs + runs index (DocumentSurface, max-w-[940px])
└── (workflowRunId set) → WorkflowRunDetail — one run, top-to-bottom report (DocumentSurface, max-w-[800px])
```

- **Navigation** is selection-state + URL params (NOT react-router), extending
  `selection.ts`:
  - `SurfaceId` gains `"workflows"`.
  - `SelectionState` gains `workflowRunId?: string`.
  - `selectionFromLocation`: read `?surface=workflows` and `?workflowRun=<id>`; setting
    `workflowRun` implies `surface=workflows` (mirror of the `?agent=` rule, `selection.ts:77`).
  - `syncSelectionToLocation`: write `params.set("workflowRun", id)` when set.
  - `surfaceIds` array gains `"workflows"`.
- **SurfaceSwitch** (`WorkbenchShell.tsx:450`) gains a `case "workflows"` that self-splits:
  `selection.workflowRunId ? <WorkflowRunDetail/> : <WorkflowsList/>` — copy of the
  `agents`/`memberId` case at line 472-480.
- **Inspector suppressed**: add `"workflows"` to `noInspector` (`WorkbenchShell.tsx:119`).
  Both views own their full layout up to the doc cap, like agents/tasks/goal/task.
- **Data**: extend `types.ts` `DashboardSnapshot` with `workflow_runs?: WorkflowRun[]`
  and `workflow_steps?: WorkflowStep[]`, plus the two interfaces (mirroring the Rust
  structs verbatim). The registered-def catalog (name + summary) comes from a separate,
  run-independent source (`workflow list` / `GET /v1/workflows`); model it as
  `workflowDefs?: { name: string; summary: string }[]` on the read model.
- **Live**: `api.ts` already upserts snapshot arrays latest-wins-by-id from SSE frames
  (`api.ts:141`). Add the same upsert for `workflow_run` and `workflow_step` SSE event
  kinds (filenames `workflow_runs.jsonl` / `workflow_steps.jsonl` → singular event names,
  confirmed `sse.rs`).
- **New tones** in `tones.ts` (never hardcoded):
  - `workflowRunTone(status)`: `running→running`, `completed→good`, `failed→bad`,
    `pending|paused→idle` (forward-compat, inert — never a primary state).
  - `workflowStepTone(status)`: `running→running`, `completed→good`, `failed→bad`,
    `queued→idle`, `cached→info`.

---

## 2. Workflows INDEX view

`WorkflowsList` — `DocumentSurface className="max-w-[940px]"` (the AgentsList width,
`Surfaces.tsx:608`) inside the 1480px main column. Two stacked regions, whitespace-separated.

### Regions

**Header** — AgentsList-style custom header (`Surfaces.tsx:609-623`): kicker row
(`text-[11px] font-semibold uppercase tracking-wider text-muted-foreground` + `Workflow`
lucide icon + "Workflows"), `h1 text-2xl`, muted description. Header action =
`OperatorActionButton` "Run workflow" (gated on `actionsEnabled`; offline → disabled +
"Connect a live source to enable actions" tooltip). Opens the launch `Dialog` with
codex/claude member `Select`s (reuse `OperatorForms` idiom).

**Region 1 — `DocSection label="Registered"`** (the catalog, ALWAYS present even at zero
runs, so the page is never blank). One bordered card per def (`rounded-lg border
border-border bg-card p-3` — the AgentCurrentTask card idiom):
- left: `Workflow` icon + def name (`text-[13px] font-medium`) + one-line summary
  (muted, `line-clamp-1`: "Serial codex scope, then a parallel codex+claude audit barrier.")
  + `MonoId` of the fn name.
- a **`CollapsibleBlock label="Preview shape"`** (chevron) that renders the SCHEMATIC
  structural graph (declared shape, no live status) — the same nested-list renderer as
  the detail's Structure section, in "schematic" mode. This answers "what will this do"
  on an empty workspace.
- right: inline `OperatorActionButton` "Run" (gated).
- If the catalog fetch is empty/unreachable → `EmptyState(icon=Workflow, title="Workflow
  catalog unavailable", description tied to Load live)`.

**Region 2 — `DocSection label="{n} runs"`** — the grid-of-buttons list idiom copied
from AgentsList (`Surfaces.tsx:679-753`): a shared `grid-template` between header and
rows, `border-b` dividers, `hover:bg-accent/40`, each row a full-width `<button>`
→ `onSelectionChange({ surface:"workflows", workflowRunId: run.id })`.

Columns: `grid-cols-[minmax(0,1.9fr)_minmax(0,1fr)_minmax(0,1.3fr)_minmax(0,1fr)_minmax(0,1.5fr)]`
1. **Run** — `StatusDot(workflowRunTone, pulse when Running)` + `workflow_name` (font-medium)
   over a truncated `MonoId(run.id)` second line.
2. **Status** — `Badge tone={workflowRunTone(status)}` with status text.
3. **Steps** — the grafted **shape glyph**: small inline pills colored by each step's
   `workflowStepTone`, laid out in phase order so the serial→parallel shape + per-step
   health read at a glance: e.g. `▪scope ▪▪audit` (all good), `▪scope ░░audit` (audit
   running), `▫scope ▪▫audit` (scope failed). Derived by grouping this run's
   `workflow_steps` (matched by `run_id`) by phase in `step_ids` order. Tabular.
4. **Duration** — `formatDuration(created_at, ended_at)`; Running rows show "· running"
   with the pulse.
5. **Summary** — the run `summary` truncated (`line-clamp-1`): "investigate completed:
   3/3 steps ok" / "…required serial step (scope) did not succeed"; em-dash while Running.

Order: Running runs pinned on top (they pulse), then terminal runs newest-first.
Empty `workflow_runs` → `EmptyState(icon=Workflow, title="No runs yet", description=
adaptive)` + the same Run button.

### ASCII wireframe — INDEX

```
INDEX  (?surface=workflows)  —  DocumentSurface max-w-[940px]
┌──────────────────────────────────────────────────────────────────────────┐
│ ▣ WORKFLOWS                                              [ ▶ Run workflow ] │
│ Workflows                                                          text-2xl │
│ Registered pipelines and every run. Open a run to see its timeline.        │
│                                                                            │
│ REGISTERED ───────────────────────────────────────────────────────────────│
│ ┌────────────────────────────────────────────────────────────────────────┐│
│ │ ▣ investigate                                              [ ▶ Run ]      ││
│ │   Serial codex scope, then a parallel codex+claude audit barrier.        ││
│ │   ⓜ investigate                                                          ││
│ │   ▸ Preview shape   (CollapsibleBlock → schematic nested-list graph)     ││
│ └────────────────────────────────────────────────────────────────────────┘│
│                                                                            │
│ 3 RUNS ────────────────────────────────────────────────────────────────────│
│  Run                 │ Status     │ Steps           │ Duration │ Summary    │ ← grid header (10px upper)
│ ─────────────────────┼────────────┼─────────────────┼──────────┼────────────│
│  ◉ investigate       │ [Running]  │ ▪scope ░░audit  │ ·running │ scoping…   │ ◉ pulses
│    wfrun-7c…         │            │                 │          │            │
│  ● investigate       │ [Completed]│ ▪scope ▪▪audit  │ 1m12s    │ 3/3 ok     │
│    wfrun-9a…         │            │                 │          │            │
│  ● investigate       │ [Failed]   │ ▫scope ▪▫audit  │ 0m48s    │ scope fail │
│    wfrun-4e…         │            │                 │          │            │
└──────────────────────────────────────────────────────────────────────────┘
glyph: ▪ good · ▫ bad · ░ running   (one pill per step, in phase order)
empty RUNS → EmptyState(Workflow, "No runs yet", "Run a registered workflow above…")
```

---

## 3. RUN DETAIL view

`WorkflowRunDetail` — `DocumentSurface` (centered `max-w-[800px]`, `space-y-7`), a 1:1
echo of the AgentDetail document anatomy. It reads top-to-bottom like a post-run report:
**header → properties → Verdict → Timeline → Code**. (Verdict is hoisted above Timeline
so the why-failed/why-degraded answer lands before the step detail.)

Resolve the run: `model.snapshot.workflow_runs.find(r => r.id === workflowRunId)`. Its
steps: `model.snapshot.workflow_steps.filter(s => s.run_id === run.id)` ordered by
`run.step_ids`. Missing run → `EmptyState(icon=Workflow, title="Workflow run not found",
description="It may not have streamed yet, or the source is offline.")` + back affordance.

### Regions

**HEADER** (`space-y-3`):
- Back affordance: `<button>` `inline-flex items-center gap-1.5 text-[11px] font-semibold
  uppercase tracking-wider text-muted-foreground hover:text-foreground` with a leading
  `ChevronLeft` + "Workflows" → clears `workflowRunId` (copy of `AgentConfigRail`'s back
  button, `Surfaces.tsx:2767`). To its right, a thin prev/next run stepper (`ChevronUp`/
  `ChevronDown` over the runs list + tabular "{i} of {n}") so cross-run scanning survives
  without a standing rail.
- Identity row (`flex flex-wrap items-start justify-between gap-3`): left = `Avatar
  size="lg"` (monogram of `workflow_name`, tone=`workflowRunTone`) + `min-w-0` stack with
  `h1 truncate text-2xl` ("investigate") and a badge row (`mt-1 flex gap-1.5`):
  `Badge tone={workflowRunTone(status)}` + `MonoId(run.id)`. Right (`shrink-0 pt-1`):
  a single gated "Re-run" `OperatorActionButton`.
- `DocProperties` (the `w-32` muted-dt dl, `atoms.tsx:280`): **Status** (StatusDot+label),
  **Verdict** (the `summary` verbatim, or "—" while Running), **Started** (`created_at`
  absolute + relative), **Ended** (`ended_at` or "running…"), **Duration**
  (`formatDuration`), **Steps** ("3 · 1 serial, 2 parallel", derived from inferred shape),
  **Required step** (a click-to-anchor link to the scope step with its pass/fail tone —
  this row makes required-vs-tolerated legible at the top).

**`DocSection label="Verdict"`** (terminal runs only) — a single bordered card
(`rounded-lg border border-status-{tone}/30 bg-status-{tone}/12 p-3`, echoing the
running-banner idiom at `Surfaces.tsx:2882`) tinted by run tone, holding the `summary`
string PLUS an explicit plain-English gloss of the gate logic:
- Failed: "Run failed: the required serial step (scope) did not succeed. The parallel
  audit steps were still collected (tolerated)."
- Completed-with-a-failed-step: "Completed (degraded): {k} of {n} steps failed, but the
  required scope step succeeded, so the run completed."
- Completed clean: "Completed because the required serial step (scope) succeeded. 3/3 ok."
While Running: a muted "Verdict set at completion." line.

**`DocSection label="Timeline"`** — the phase→step structure (§4) + per-step rows (§5).
This is the dominant block.

**`DocSection label="Definition"`** — the dual code view (§6).

### ASCII wireframe — RUN DETAIL

```
RUN DETAIL  (?surface=workflows&workflowRun=wfrun-9a)  —  DocumentSurface max-w-[800px]
┌──────────────────────────────────────────────────────────────────────────┐
│ ‹ WORKFLOWS                                              [ ⌃ ⌄  2 of 3 ]    │ ← back clears id
│                                                                            │
│ ┌──┐ investigate                                          [ Re-run ]       │
│ │IV│ [Completed]  wfrun-9a…                                                │
│ └──┘                                                                        │
│  Status        ● Completed                                                  │
│  Verdict       investigate completed: 3/3 steps ok                         │
│  Started       2026-06-02 10:14:02  · 2h ago                              │
│  Ended         2026-06-02 10:15:14      Duration 1m12s                    │
│  Steps         3 · 1 serial, 2 parallel                                    │
│  Required      ● scope-question (ok) ↗                                      │
│                                                                            │
│ VERDICT ───────────────────────────────────────────────────────────────────│
│ ┌── good-tint ─────────────────────────────────────────────────────────┐  │
│ │ investigate completed: 3/3 steps ok                                    │  │
│ │ Completed because the required serial step (scope) succeeded. 3/3 ok.  │  │
│ └────────────────────────────────────────────────────────────────────────┘│
│                                                                            │
│ TIMELINE ────────────────────────────────────────────────────────────────│
│ │ PHASE · scope                                              [ serial ]    │
│ ●─┐                                                                        │
│   │ ┌──────────────────────────────────────────────────────────────────┐ │
│   │ │ ● scope-question  codex            [Completed]  [required]         │ │
│   │ │ ran by ⓜ codex-7… ↗ · 10:14:02 → 10:14:16 · 14s                   │ │
│   │ │ Modules to audit: lib.rs journal, workflow.rs barrier…            │ │
│   │ │ ▸ ⌗ codex · 14s · 22 events · turn      (TurnDrillIn, lazy)        │ │
│   │ └──────────────────────────────────────────────────────────────────┘ │
│ │ PHASE · audit                                         [ parallel · 2 ]   │
│ ●─┬─ gantt: a-codex  ▮▮▮▮▮▮▮▮                                              │
│   │        a-claude    ▮▮▮▮▮▮▮▮▮▮  (bars overlap = ran concurrently)       │
│   ├─┌────────────────────────────────────────────────────────────────┐   │
│   │ │ ● audit-codex   codex          [Completed]                      │   │
│   │ │ ran by ⓜ codex-7… ↗ · 10:14:18 → 10:14:59 · 41s                 │   │
│   │ │ Audited code paths: claim loop, delivery seam…                  │   │
│   │ │ ▸ ⌗ codex · 41s · 30 events · turn                              │   │
│   │ └────────────────────────────────────────────────────────────────┘   │
│   └─┌────────────────────────────────────────────────────────────────┐   │
│     │ ● audit-claude  claude         [Completed]                      │   │
│     │ ran by ⓜ claude-2… ↗ · 10:14:18 → 10:15:14 · 58s              │   │
│     │ Audited recent diffs: …          ▸ ⌗ claude · 58s · 44 ev · turn│   │
│     └────────────────────────────────────────────────────────────────┘   │
│ ═══ join · barrier — all 2 steps resolved ═══════════════════════════════ │
│                                                                            │
│ DEFINITION ───────────────────────────────────────────────────────────────│
│ ┌ mono ──────────────────────────────────────────────────────────────┐   │
│ │ ● scope-question ──▶ ⟨ ● audit-codex  ∥  ● audit-claude ⟩ ──▶ ⟂ join │   │ ← always-on ASCII
│ └──────────────────────────────────────────────────────────────────────┘ │
│ ▸ View Rust source · workflow.rs        (lazy-fetch, cached, max-h)        │
└──────────────────────────────────────────────────────────────────────────┘
```

---

## 4. Serial / parallel / barrier structure (the Timeline)

**Inferred, never stored** (control flow is not a persisted field). Algorithm:
1. Group steps by `phase`, preserving `step_ids` order.
2. Classify each phase: a phase with one step (or steps whose `[started_at, ended_at]`
   windows do NOT overlap) = **SERIAL** (single node); a phase whose steps have
   **OVERLAPPING** windows = **PARALLEL barrier** (sibling nodes under a join bar).
3. Primary key is phase grouping + step count; window-overlap is the secondary signal.
   For `investigate` this always yields the canonical: scope (serial) → audit (parallel, 2).

**Visual treatment** inside `DocSection label="Timeline"` — a vertical rail
(`border-l border-border` running down the left, echoing a conversation timeline). Each
PHASE is a labeled segment:
- **Phase marker**: small uppercase muted caption "PHASE · scope" / "PHASE · audit" +
  a `Badge tone="idle"` chip to its right reading "serial" or "parallel · 2".
- **SERIAL phase** → a node dot on the rail + ONE `StepRow` card (§5) hanging off it.
- **PARALLEL phase** → the rail forks: N indented sibling `StepRow` cards sharing a left
  bracket, capped by a **JOIN BAR** — a thin full-width hairline row labeled "join ·
  barrier — all {n} steps resolved" whose tone reflects resolution (stays muted/pulsing
  while any step is non-terminal, settles when all are terminal). This is the explicit
  barrier semantic.
- **Inline gantt strip** under a parallel phase (grafted, hand-rolled like
  `AgentSparkline`, `atoms.tsx:195`): one thin horizontal bar per step, left%/width%
  computed from the phase's `min(started_at)..max(ended_at)` window, so overlapping
  windows literally overlap and "ran concurrently" is visible. `bg-status-{tone}/60`
  bars, 4px tall, no chart dep. Degrades to a "ran concurrently" text caption below ~480px.

The same renderer drives the **schematic** mode used by the index "Preview shape"
collapsible and the Code-section ASCII restatement — one structural function, three call
sites, guaranteeing the shape never disagrees with itself.

---

## 5. Per-step: agent + status + output + provider-turn drill-in

Each `StepRow` is a card (`rounded-lg border border-border bg-card p-3`, hover
`border-input`), composed like AgentDetail's current-task card. **Informational, not a
button** — the only interaction is the drill-in. Anatomy:

- **Line 1** (`flex items-start justify-between`): left = `StatusDot(workflowStepTone,
  pulse when Running)` + `step.label` (`text-[13px] font-medium`, "scope-question" /
  "audit-codex") + a role hint parsed from the label ("codex" / "claude"). Right =
  `Badge tone={workflowStepTone(status)}` (Running / Completed / Failed) + the
  **required/tolerated** marker: `Badge tone="info" "required"` on the serial scope step;
  `Badge tone="warn" "tolerated"` next to a Failed audit step.
- **Line 2 — "ran by"** (the agent, RESOLVED THROUGH THE SESSION, not a step field):
  let `session = provider_sessions.find(s => s.id === step.provider_session_id)`; the
  member is `session?.agent_member_id`. Render `Avatar size="sm"` (or `MonoId` fallback)
  + member name as a **clickable link to `?agent=<id>`** (`text-foreground
  hover:text-primary`, the AgentDetail dd-link style). Then the timing window:
  "10:14:02 → 10:14:16 · 14s" (tabular-nums). When `provider_session_id` is null
  (Running, or Failed-before-delivery): "ran by — · started {t} · running…".
- **Line 3 — output**: `step.output_summary` via `<Markdown>` (`line-clamp-3`, the
  at-a-glance provider report); muted "Running…" while None; "No output (step failed
  before delivery)" for a Failed-pre-delivery step.
- **Line 4 — drill-in (the key 1:1 reuse)**: if `step.provider_session_id` resolves
  against `snapshot.provider_sessions`, mount `<TurnDrillIn session={session}
  apiUrl={apiUrl} />` **VERBATIM** (`Surfaces.tsx:3459`). Its cheap one-line summary
  becomes "⌗ {provider} · {duration} · {N} events · turn"; it fetches
  `/v1/provider-sessions/{id}/events` once on first expand, caches locally, renders the
  raw turn in a `max-h-80` panel with loading/error/empty fallbacks — no new fetch
  contract. When `provider_session_id` is null, render a **DISABLED** affordance instead:
  `inline-flex text-[10px] text-muted-foreground` "⌗ no turn yet" with a 40%-opacity
  `ChevronRight`, `cursor-not-allowed` — honest "nothing to drill into yet".

---

## 6. Code view (structural graph + collapsible Rust source)

`DocSection label="Definition"`, stacked (not tabbed — tabs would add vocabulary on a
document page; stacking keeps it a report and lets both be addressable at once):

1. **TOP — always-visible ASCII structural restatement** (grafted from Run Report).
   The SAME inferred shape rendered as a one-line text pipeline in a mono block
   (`rounded-md border border-border bg-muted/30 p-2 font-mono text-[11px]`):
   `● scope-question ──▶ ⟨ ● audit-codex ∥ audit-claude ⟩ ──▶ ⟂ join`. Each node carries
   its step's tone via a leading `StatusDot`, so graph and timeline agree. **Cheap** —
   computed from steps already in hand, no fetch — and it renders from the *declared*
   shape even at zero runs (so it doubles as the index "Preview shape" body).

2. **BOTTOM — lazy Rust source** following the `TurnDrillIn` contract EXACTLY. A
   `CollapsibleBlock`-style flip-chevron row "View Rust source · workflow.rs" (`ChevronRight`/
   `ChevronDown` + `Code` glyph + cheap caption). On first expand only: fetch
   `GET {base}/v1/workflows/{workflow_name}/source` → `{ path, source }`, cache in local
   `useState` (`source!==null` guard, bail if loaded or no `apiUrl`), render into a
   `max-h-96 overflow-y-auto rounded-md border border-border bg-muted/30 p-2 font-mono
   text-[11px] whitespace-pre` block. Loading line (`text-[11px] muted`), error line
   (`text-status-bad`, "HTTP {status}"), and — because this endpoint is **not yet
   implemented** — a faithful "source unavailable (endpoint not present in this build)"
   on 404. No syntax highlighter (light paper, plain mono). The resolved path shows as a
   `MonoId` header (e.g. `workflow.rs`).

   > Backend note: shipping the structural ASCII needs zero backend work. The Rust source
   > view is additive; add a route beside `/v1/provider-sessions/{id}/events` at
   > `main.rs:2549` that returns the compiled workflow fn's source. Until then the panel
   > self-reports unavailable rather than hiding the affordance.

---

## 7. Empty / loading / error states

**Empty is the FIRST-CLASS, most-seen state** (no fixture; runs/steps almost always empty).

- **INDEX empty**: Region 1 "Registered" STILL renders the investigate def card (catalog
  is run-independent), so the page is informative + actionable. Region 2 →
  `EmptyState(icon=Workflow, title="No runs yet", description=adaptive)`:
  - live: "Run a registered workflow above to see its serial→parallel timeline here."
  - offline: "Connect a running harness with Load live, then run a workflow."
  + the gated Run button.
- **Catalog unreachable (offline)**: Region 1 → `EmptyState(title="Workflow catalog
  unavailable", …)`; header "Run workflow" disabled with the standard tooltip.
- **DETAIL — run not found**: `EmptyState(icon=Workflow, title="Workflow run not found",
  description="It may not have streamed yet, or the source is offline.")` + back affordance.
- **DETAIL — fresh run, no steps yet**: header + DocProperties paint from the run row;
  the Timeline renders the SCHEMATIC scaffold (declared shape, no live status) with the
  run header pulsing Running; Verdict shows "Verdict set at completion."; the ASCII graph
  computes from whatever steps exist.
- **Per-block loading/error** all follow the `TurnDrillIn` fallbacks: a muted "loading…"
  line, a `text-status-bad` error line ("HTTP {status}"), faithful empty fallbacks
  ("no events recorded" / "source unavailable" / "no output"). No full-page spinners.
- **Every "nothing here" is an `EmptyState` atom**, never a bare string.
- **Read-only honesty**: every write (Run / Re-run / launch Dialog) is gated via
  `OperatorActionButton` / `ActionButton`, disabled offline with the
  "Connect a live source to enable actions" tooltip — never an enabled no-op.

---

## 8. Live state (SSE)

Single live indicator everywhere: `StatusDot` with `pulse === (tone === "running")` —
on the run identity, the index run row, the phase node dot, and the step card dot.

- `workflow_run` SSE frame → upsert the run row latest-wins by id (`api.ts:141` pattern).
  A new run prepends a pulsing index row; on the detail it flips the header Badge +
  Verdict + Ended/Duration in place and stops the pulse on terminal.
- `workflow_step` SSE frame → upsert the step latest-wins by id. A Running step appears
  in `step_ids` order **before delivery**: in the Timeline its `StepRow` animates in with
  a pulsing dot, "running…", and the **disabled** drill-in (no `provider_session_id`
  yet). When the terminal frame lands (same id), the row updates **IN PLACE** to
  good/bad: dot settles, Badge flips, member resolves (now that `provider_session_id` is
  set), `output_summary` + duration appear, and the **enabled** `TurnDrillIn` mounts.
- The parallel **JOIN BAR** stays muted/pulsing until every step in its phase is
  terminal, then settles — making the barrier wait visible.
- The Timeline therefore **animates the run's progression** (scope pulses → resolves →
  audit pair fans in → join settles → run badge flips), never a batch reveal at end. The
  index "Steps" shape glyph fills in color live too.
- Reuse the existing `FreshnessChip`/live affordance in the header for SSE connection state.

---

## 9. Responsive behavior

Both views live in the centered `DocumentSurface` regime (Inspector suppressed), so
narrow behavior is inherited and gentle — the chief reason the rail was folded away and
the structure is a nested LIST (reflows at any width) rather than an SVG DAG.

- **INDEX**: the grid-table columns collapse on narrow — drop **Summary** then
  **Duration** first via responsive `grid-cols` + `hidden lg:block` (the AgentsList
  column-hiding pattern, `Surfaces.tsx:689`), always keeping Run + Status + the shape
  glyph. Row stays a single tappable button. Header action wraps below the title via
  flex-wrap.
- **DETAIL**: `max-w-[800px]`, fluid to mobile. Identity row is `flex-wrap` (badges drop
  under the title). `DocProperties` `dt` is `w-32` fixed, `dd` flexes. Timeline: the
  parallel fork bracket tightens; the inline gantt strip degrades to a stacked
  "started {t} → ended {t} · ran concurrently" caption below ~480px (the strip needs
  width to be honest). Step cards are full-width and stack; the "ran by" meta wraps. The
  prev/next stepper collapses to just the count. Definition mono panels use
  `overflow-x-auto` so source/ASCII never wrap-corrupt.
- Wide screens center the same document with more whitespace — no second column,
  consistent with the calm-readability goal and the two-regime constraint.

---

## 10. Region → design-system atom/component mapping

| Region / element | Reuses |
|---|---|
| Surface routing, URL params | `selection.ts` (`SurfaceId`+`workflowRunId`, `selectionFromLocation`, `syncSelectionToLocation`); `WorkbenchShell.tsx:450` `SurfaceSwitch` case; `noInspector` list (`:119`) |
| Index wrapper | `DocumentSurface` `className="max-w-[940px]"` (`atoms.tsx:144`) |
| Index header | AgentsList custom header pattern (`Surfaces.tsx:609`); `Workflow` lucide icon; `OperatorActionButton` (`Surfaces.tsx:772`) |
| Registered card | `rounded-lg border border-border bg-card p-3`; `MonoId` (`atoms.tsx:331`); `CollapsibleBlock` "Preview shape" (`atoms.tsx:243`); inline `OperatorActionButton` |
| Runs list | grid-of-buttons idiom (`Surfaces.tsx:679-753`); `DocSection` (`atoms.tsx:163`); `StatusDot` (`atoms.tsx:36`); `Badge` (`ui/badge.tsx`) |
| Index shape glyph | inline `StatusDot`-toned pills (no new atom) |
| Index empty | `EmptyState` (`atoms.tsx:337`) |
| Launch Dialog | `ui/*` `Dialog` + `Select`, `OperatorForms` idiom (`OperatorForms.tsx`) |
| Detail wrapper | `DocumentSurface` (default `max-w-[800px]`, `space-y-7`) |
| Back affordance | AgentConfigRail back button (`Surfaces.tsx:2767`) |
| Identity row | `Avatar size="lg"` (`Avatar.tsx`); `Badge`; `MonoId`; `OperatorActionButton` "Re-run" |
| Properties dl | `DocProperties` (`atoms.tsx:280`) |
| Verdict card | running-banner tint idiom (`Surfaces.tsx:2882`); tone classes only |
| Timeline rail + phase markers | `DocSection`; `border-l` rail; uppercase muted captions; `Badge tone="idle"` chips |
| Join bar | hairline `border` row + tone, derived from step states |
| Inline gantt | hand-rolled bars in the `AgentSparkline` spirit (`atoms.tsx:195`) — no chart dep |
| Step card | current-task card idiom; `StatusDot`; `Badge`; `Avatar size="sm"`; `Markdown` (`Markdown.tsx`); `?agent=` link |
| Provider-turn drill-in | `<TurnDrillIn>` VERBATIM (`Surfaces.tsx:3459`) → `/v1/provider-sessions/{id}/events`; disabled stub when no session |
| Code — ASCII graph | mono block from the shared structural renderer (no fetch) |
| Code — Rust source | `CollapsibleBlock`/`TurnDrillIn` lazy-fetch contract → `GET /v1/workflows/{name}/source` (additive; self-reports unavailable on 404) |
| Tones | NEW `workflowRunTone()` / `workflowStepTone()` in `tones.ts` (the 7 `StatusTone` values only) |
| Data types | NEW `WorkflowRun` / `WorkflowStep` in `types.ts` (mirror `harness-core/src/lib.rs:1261-1292`); `workflow_runs` / `workflow_steps` on `DashboardSnapshot`; SSE upsert in `api.ts` |
| Empty/loading/error | `EmptyState` everywhere; `TurnDrillIn` fallback idiom for async blocks |

---

## 11. Tradeoffs (accepted)

- **No standing two-pane runs rail** — cross-run scanning costs a back-click (mitigated
  by the prev/next stepper + always-present index). Trades the brief's literal "rail +
  timeline split" for full consistency with every surface and a far better empty-first
  story. Revisit toward a bordered-`Section` panel regime only if runs become high-volume.
- **Inferred structure** (phase grouping + window overlap) can mis-read shape under clock
  skew or sub-second windows; phase grouping is the primary key and the Rust source is the
  ground-truth escape hatch. Nails the canonical scope→audit shape; arbitrary future DAGs
  would want a real diagram atom.
- **Nested list, not SVG DAG** — sacrifices many-to-many edge rendering for perfect
  reflow; correct for the 3-node canonical shape, pipeline-deferred otherwise.
- **Shape shown twice** (Timeline rows + Definition ASCII) is intentional redundancy for
  readability; both derive from one renderer so they cannot disagree.
- **Rust source is a backend dependency** the surface ships without — the panel
  self-reports unavailable until `/v1/workflows/{name}/source` exists.
- **Member via session, not step** — a step with no resolvable `ProviderSession` shows
  "ran by —" and a disabled drill-in; acceptable since the journal stamps
  `provider_session_id` only at terminal delivery.
```

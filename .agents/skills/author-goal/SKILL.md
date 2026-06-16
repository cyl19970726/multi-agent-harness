---
name: author-goal
description: "Use when creating or advancing a Goal in this harness: write the markdown sections (description / design / real acceptance), accumulate a knowledge ledger, run multi-agent exploration, and move the goal through its lifecycle. Covers BOTH the manual gated lifecycle (draft → exploring → explored → working → done → verifying → verified) and the knowledge-driven PHASED model (append-only knowledge[] + agent-planned sequential phases[] + a per-phase task graph, executed by `goal run-phases`). To decompose an explored goal into phases + a task DAG, hand off to [[author-planner]]."
---

# Author Goal

A Goal in this harness is **markdown-first**, not a pile of typed fields. Its
substance lives in three rich sections, and it advances through an explicit
lifecycle whose gates force you to actually explore and to define real acceptance
*before* you work. This skill is how you write one and move it through its stages.

The point of the model is to STOP form from crowding out substance. A goal whose
fields are filled but whose design never identified the real problems is a bad
goal. Do not fill sections to look complete — fill them because you explored.

## The shape of a Goal

| Field | Stage written | What goes in it |
| --- | --- | --- |
| `description_md` | draft | What this goal is and WHY. The seed. Short is fine. |
| `explorations[]` | exploring | Raw notes from each explorer / round (multi-agent, multi-round). |
| `knowledge[]` | any | **Append-only ledger of findings** (the TRUTH), each with provenance: `phase_id` / `task_id` / `author` / `timestamp` / `tags` / `source`. Fed by exploration AND task execution. |
| `design_md` | explored | **Key problems FIRST**, then the Big Picture / Overview, then the approach. Either hand-written, or **re-synthesized from `knowledge[]`** via `design-synthesize` (stamps `design_synthesis_at`). |
| `acceptance_md` | before working | The REAL acceptance: criteria + scenario + how to verify *for real*. |
| `phases[]` | when planning | Agent-planned, **sequential** checkpoints; each owns a task DAG (see [[author-planner]]). Optional — a simple goal needs none. |
| `skill_refs[]` | any | Domain skills needed to DO the work (NOT this skill). |
| `stage` | — | Lifecycle position. **Derived from `phases[]` when present** (the stored field is a legacy projection); the stored value is the truth only for goals with no phases. |

`design_md` absorbs what the old GoalDesign split across scenario / non-goals /
required-infra / evidence-plan / acceptance-gates. Write it as prose, not slots.

**Two ways to run a goal.** A *simple* goal uses the **manual lifecycle** below
(you drive the stages by hand). A goal that needs a planned, gated, multi-worker
build uses the **phased model** (`knowledge[]` → `phases[]` → `goal run-phases`,
see "Knowledge-driven phased execution" near the end). They share the same Goal —
phases are additive, and `stage` is derived once a goal has them.

## The lifecycle

```
draft → exploring → explored → working → done → verifying → verified
└── exploration ──┘ └──── work ────┘ └──── acceptance ────┘
```

Back-edges (the CLI allows only these): **any → exploring** (re-open exploration
when understanding is thin) and **verifying → working** (real acceptance failed,
go fix it). Forward moves are one stage at a time.

### The gates (enforced by `goal stage`)

- `exploring → explored` **requires a non-empty `design_md`.** You cannot mark
  exploration complete without a written design.
- `explored → working` **requires a non-empty `acceptance_md`.** You write how the
  goal will be truly accepted BEFORE any work starts.

These two gates are the whole point. They are non-negotiable: the CLI refuses the
transition with an explanatory error if the section is empty.

## Writing each section well

### description_md (draft)
Two short paragraphs: WHAT and WHY. Enough for an explorer to know where to dig.
Do not pre-write the design here — you have not explored yet.

### Exploration (exploring) — multi-agent, multi-round
Exploration is not one pass. Fan out: several explorers, each reading a different
slice of the real code/system, each appending a note. Keep exploring while any
part feels thin. The harness's own workflow runtime is a natural driver — run
parallel `Explore` agents and record each as an exploration note.

```bash
harness goal explore-add --id <goal> --author <who> --notes-file notes.md
# round auto-increments; pass --round N to override
```

### design_md (explored) — key problems FIRST
The tasks are CUT FROM this doc, so a shallow design yields deviated tasks. Go
DEEP and EXHAUSTIVE: identify **all** the key problems (not a tidy few), and make
the architecture **concrete** — actual type/trait signatures, data structures,
file:line, before/after — not prose gestures. If a section reads like a summary,
it is too thin. Deep design is naturally multi-agent: fan out parallel grounded
deep-dives (e.g. via [[author-workflow]], codex-heavy) and synthesize their FULL
output. The design is only as good as the problems it identified. Lead with them:

1. **Key problems** — the genuine, grounded obstacles, each tied to real evidence
   (`file:line`, a command, a measurement). This is where the value is. Example:
   "no canonical event model — `provider_turn_events.jsonl` stores the raw payload
   (main.rs:5193), so a third provider has no normalization target."
2. **Big Picture / Overview** — how the overall architecture/approach changes given
   those problems. Only after the problems are named does the overview make sense.
3. **Approach** — the shape of the work, the decisions, the non-goals.

```bash
harness goal design-set --id <goal> --md-file design.md
```

If, while writing the design, you discover the problems are not actually
understood, go back: `harness goal stage --id <goal> --to exploring`.

Promote a key problem to a tracked `gap` (and later a `task`) when it becomes
actionable — but it lives in `design_md` first, as narrative, not as empty slots.

#### Two ways to fill `design_md`
- **Hand-written** (`design-set --md-file`): you author the prose directly. The
  manual escape hatch; always available.
- **Synthesized from the knowledge ledger** (`design-synthesize`): if you have been
  capturing findings as `knowledge[]` (below), regenerate `design_md` as a derived,
  re-synthesizable view grouped by phase. This is preferred for the phased model,
  where `knowledge[]` is the truth and `design_md` is a projection of it. The
  synthesis is deterministic and **refuses to run on an empty ledger** (capture
  knowledge first).

### The knowledge ledger (knowledge[]) — the durable truth
Findings are not only prose in `design_md`; capture each as an **append-only**
`Knowledge` entry with provenance. The ledger is fed by BOTH exploration and task
execution (a task that learns something appends to it), and it is what
`design-synthesize` rebuilds `design_md` from — so a finding is never lost when the
design is re-written.

```bash
harness goal knowledge-add --goal <goal> --author <who> \
    --notes-file finding.md --tag architecture --tag risk \
    --source exploration            # exploration|task|decision|evidence
    # --phase <phase-id> --task <task-id>  to attach provenance

harness goal design-synthesize --goal <goal>   # knowledge[] → design_md (+ stamp)
```

Use `--source task --task <id>` when a task's execution surfaced the finding;
abandoned approaches stay in the ledger (a later entry can supersede a task).

### acceptance_md (before working) — REAL acceptance
Write this at the `explored → working` gate. It must describe how the goal is
verified *for real*, not a synthetic check. Example: "integrating Kimi Code is
accepted when we actually use Kimi Code to do real work, integrated into the whole
network — not when a unit test passes." Include the criteria, the concrete
scenario, and the steps to run the real verification.

```bash
harness goal acceptance-set --id <goal> --md-file acceptance.md
```

## Driving the lifecycle (CLI)

```bash
# Create (born in draft)
harness goal create --id <goal> --title "..." --objective "..." --owner <agent> \
    --description "what and why" --skill-ref <domain-skill>

# Explore (repeat, multi-agent)
harness goal stage --id <goal> --to exploring
harness goal explore-add --id <goal> --author explorer-a --notes-file a.md
harness goal explore-add --id <goal> --author explorer-b --notes-file b.md

# Synthesize the design, then gate into explored
harness goal design-set --id <goal> --md-file design.md
harness goal stage --id <goal> --to explored          # gate: design_md non-empty

# Write REAL acceptance, then gate into working
harness goal acceptance-set --id <goal> --md-file acceptance.md
harness goal stage --id <goal> --to working           # gate: acceptance_md non-empty

# Work → done → verify against acceptance_md → verified
harness goal stage --id <goal> --to done
harness goal stage --id <goal> --to verifying
harness goal stage --id <goal> --to verified          # only after REAL acceptance passes

# Inspect
harness goal show --id <goal>
```

**Always write markdown via `--md-file <path>`** (or `--<field>-file`). The inline
`--md "…\n…"` form passes a LITERAL backslash-n through the shell — it does NOT
become a newline, so headings/lists render as one mangled blob. Inline is only ever
safe for a single short line with no newlines.

## Knowledge-driven phased execution (the planned path)

Beyond the manual lifecycle, a goal can carry an **agent-planned plan** and be run
by the orchestrator. The model: `knowledge[]` is the truth, `design_md` is a
re-synthesizable view of it, and `phases[]` (sequential) each own a **task DAG**.
The full loop:

```bash
# 1. Capture findings + (re)synthesize the design from them
harness goal knowledge-add --goal <goal> --author <who> --notes-file f.md --tag x
harness goal design-synthesize --goal <goal>

# 2. Plan: decompose design_md → phases + a per-phase task DAG (see author-planner)
harness goal plan <goal>                    # agent-driven; --dry-run plans nothing
#   or hand-author:
harness goal phase-add --goal <goal> --phase-id p1 --name "Build" \
    --intent "…" --acceptance "…"
harness task create --goal <goal> --phase-id p1 --title "…" --objective "…" \
    --owner <who> --owned-path crates/x --design-file slice.md --depends-on <task>

# 3. Inspect the compiled Starlark for one phase (derived, throwaway view)
harness phase compile <goal> --phase p1

# 4. Run the plan: phases SEQUENTIALLY, each gated on its verdict before the next;
#    each task's outcome is written back to Task.status; the goal's derived stage
#    advances; a durable checkpoint is persisted.
harness goal run-phases <goal>              # --dry-run for the mock-worker path
harness goal run-phases <goal> --resume     # re-enter without re-spending done work
harness goal run-phases <goal> --max-phase-retries 2   # replan loop on failure
```

What `run-phases` does per phase: compile the phase's live tasks → a `.star`
(disjoint-`owned_paths` no-dep tasks → `parallel()`, writable tasks → worktree
isolation, phase `acceptance` → a `verdict()` gate), run it on the workflow
runtime, and **only advance when the verdict passes**. On a failure with retries
left it **replans**: appends a `Knowledge` entry for the finding, asks the planner
to revise (supersede dead tasks → `TaskStatus::Superseded` + new ones), recompiles,
and reruns — capped by `--max-phase-retries`.

A **simple goal needs none of this** — one phase, or just the manual lifecycle.
Reach for phases when the work is a real multi-step, multi-worker build that
benefits from gated checkpoints and a living task graph. The decomposition rules
(sequential phases; parallel-iff-disjoint within a phase; each task a mini-goal)
live in [[author-planner]]; the Starlark runtime each phase compiles to is
[[author-workflow]].

## Anti-patterns (reject these)

- **Form over substance.** Sections filled to look complete while the design never
  named the real problems. The gates exist to catch exactly this; do not route
  around them.
- **Designing before exploring.** Writing `design_md` from assumptions instead of
  reading the real code/system. If you have not grounded it in `file:line` /
  evidence, you are not in `explored` yet.
- **Synthetic acceptance.** "A test passes" when the real bar is "the thing works
  in the real network." Write the acceptance you would actually trust.
- **One-shot exploration.** Stopping at one pass when parts are still thin.
  Re-open exploration; it is cheap and the back-edge exists for this.
- **Losing findings to a design rewrite.** Discoveries that live only in `design_md`
  prose vanish when it is re-synthesized. Capture them as `knowledge[]` (the truth);
  let `design-synthesize` rebuild the prose from the ledger.
- **Phases for a one-step goal.** Don't build a task DAG + `run-phases` for work that
  is a single change. Use the manual lifecycle; reach for phases only when gated,
  multi-worker checkpoints earn their keep (the rules are in [[author-planner]]).
- **Editing the compiled `.star` by hand.** It is a derived, throwaway view of the
  task graph; change the tasks and recompile, never the `.star`.

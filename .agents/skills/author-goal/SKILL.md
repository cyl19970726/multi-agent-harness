---
name: author-goal
description: "Use when creating or advancing a Goal in this harness: write the three markdown sections (description / design with key-problems-first / real acceptance), run multi-agent exploration, and move the goal through its lifecycle (draft → exploring → explored → working → done → verifying → verified) with the gates the CLI enforces."
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
| `design_md` | explored | **Key problems FIRST**, then the Big Picture / Overview (how the architecture changes), then the approach. The substantial doc. |
| `acceptance_md` | before working | The REAL acceptance: criteria + scenario + how to verify *for real*. |
| `skill_refs[]` | any | Domain skills needed to DO the work (NOT this skill). |
| `stage` | — | Lifecycle position (see below). |

`design_md` absorbs what the old GoalDesign split across scenario / non-goals /
required-infra / evidence-plan / acceptance-gates. Write it as prose, not slots.

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
The design is only as good as the problems it identified. Lead with them:

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

Long markdown goes through `--<field>-file <path>` (or `--md-file`); the inline
`--<field>` / `--md` form is for short text.

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

# Implementation Plan — Goal Lifecycle Model (markdown-first, staged)

**Goal**: Replace the form-heavy Goal/GoalDesign field soup with a markdown-first
Goal centered on three rich sections + an explicit lifecycle, so the goal's real
substance (grounded design, identified key problems, real acceptance) is captured
instead of lost to ceremony.

**Locked decisions** (agreed with user):
- Markdown sections: `description_md` (draft), `design_md` (after explore: key
  problems FIRST, then Big Picture / Overview / approach), `acceptance_md`
  (written BEFORE work starts; real acceptance — e.g. "actually use Kimi to work,
  integrated into the network").
- Lifecycle (7 stages): `draft → exploring → explored → working → done →
  verifying → verified`. Back-edges: any → `exploring`; `verifying → working`.
- Gates: `exploring→explored` requires non-empty `design_md`; `explored→working`
  requires non-empty `acceptance_md`.
- Exploration is multi-agent / multi-round: `explorations[]` raw notes →
  synthesized into `design_md`.
- `skill_refs[]` on the goal (domain skills to DO the work); SEPARATE from a new
  `author-goal` skill (how to WRITE a goal + transition states).
- Merge GoalDesign INTO Goal (markdown absorbs scenario/non-goal/required-infra/
  evidence-plan/acceptance-gate). Keep the GoalDesign struct for back-compat;
  new goals use the markdown fields.
- Additive only (ADR 0017): new fields `#[serde(default)]`; keep legacy `status`
  alongside new `stage` (map stage→status so old kanban/filters keep working).

## Stage 1: Core model + transition gates (harness-core)
**Goal**: `GoalStage` enum, `Exploration` struct, new additive `Goal` fields, a
pure `can_transition(from,to,goal)` gate function.
**Success**: `cargo test -p harness-core` green; old goal rows still deserialize.
**Tests**: stage default = draft; gate blocks exploring→explored without design_md
and explored→working without acceptance_md; back-edges allowed; stage→status map.
**Status**: Complete (harness-core 41 tests green; workspace builds; clippy clean.
The 4 `resident_daemon` socket failures are a pre-existing macOS `$TMPDIR`-length
issue — confirmed identical on clean master, unrelated to this change.)

## Stage 2: Schema + fixtures
**Goal**: `schemas/goal.schema.json` gains the new optional properties; a valid
fixture exercises them.
**Success**: `pnpm check:schema-fixtures` green.
**Status**: Complete (27 valid / 21 invalid fixtures green; new `with-lifecycle.json`).

## Stage 3: CLI (write the markdown, transition the stage)
**Goal**: `goal create` (md via `--*-file`/stdin + `--skill-ref`), `goal
explore-add`, `goal design-set`, `goal acceptance-set`, `goal stage <to>`
(gate-enforced), `goal show` (renders sections + stage).
**Success**: `cargo test` green; a goal can be driven draft→verified via CLI with
gates refusing illegal transitions.
**Status**: Complete (e2e verified: gates reject draft→working, explored-without-design, working-without-acceptance; explore-add auto-increments round; harness-cli 145 pass, only the pre-existing macOS resident_daemon socket flakes fail).

## Stage 4: `author-goal` skill
**Goal**: a skill teaching how to write description/design(key-problems-first)/
acceptance and how to pass each gate to transition stages.
**Success**: `pnpm check:skills` + `check:doc-governance` green.
**Status**: Not Started

## Stage 5: Dashboard (stage bar + 3 markdown sections)
**Goal**: Goal detail surface renders a stage flow bar + the three markdown
sections + the explorations list (markdown rendered).
**Success**: `pnpm check:dashboard` green; in-browser screenshot shows it.
**Status**: Not Started

## Stage 6: Migrate the two real goals
**Goal**: re-express `goal-self-hosting` and `goal-provider-neutral` in the new
model — design_md carries the grounded key problems (the gaps stay linked),
acceptance_md the real acceptance — and verify in the frontend.
**Success**: both goals show their markdown + stage in the dashboard.
**Status**: Not Started

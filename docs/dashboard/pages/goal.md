# Goal Workbench Page

status: active contract for `goal-goal-workbench-v1`
owner_role: product-design
canonical_for: selected Goal detail, phase-first execution surface, Goal proof chain
route_or_surface: `?surface=goal&goal=<goal-id>`
workflow_evidence: `wfrun-1783013150649-0`, `evidence-1783013226770-p11384-0`

## Purpose

Goal detail is the primary workbench for a harness-operated Goal. It is not a
filtered task board. The page must let an operator reconstruct why the Goal
exists, what spec is being executed, which phases and tasks are active, what
evidence exists, what review or decision is missing, and what action moves the
Goal forward.

The Work page remains the Goal collection and index. Once a Goal is selected,
phase work, task detail, and proof state should stay inside the Goal context.

## Canonical Objects

- `Goal`: durable outcome, owner, status, design, acceptance, phases, knowledge.
- `GoalPhase`: sequential execution and gate model.
- `Task`: phase-owned or unphased/follow-up work item.
- `Message`: assignment and report proof.
- `AgentMember`: assignee, reviewer, runtime, current work.
- `Evidence`: checks, screenshots, workflow runs, reports, diffs, artifacts.
- `Proposal`, `Review`, `Decision`: acceptance packet and Leader outcome.
- `GoalEvaluation`: closeout learning and follow-up source.

## Workflow Proof

The page must expose this chain without requiring raw JSON:

```text
Goal spec
  -> phases
  -> phase plan
  -> compiled workflow
  -> live workflow run
  -> Message assignment
  -> AgentMember work/report
  -> Evidence
  -> Proposal/Review
  -> Decision
  -> GoalEvaluation
```

Missing links are first-class state. A missing assignment message, missing
evidence, missing review, missing decision, or missing evaluation must appear as
a visible proof-chain gap with an owner and next action.

## Selected Direction

Use a phase-first Goal Workbench:

- Header: Goal identity, short spec, acceptance summary, current blocker, next
  action, derived lifecycle summary, proof health.
- Main body: vertical Phase Spine. Each phase owns its plan, compiled Starlark
  workflow, latest workflow run, gate, progress, inline step details,
  evidence/review/decision state, and outputs.
- Inspector: selected task, evidence, gate, or decision packet without leaving
  the Goal page.
- Bottom: unphased and follow-up tasks, separated from phase tasks.

The legacy `draft -> exploring -> explored -> working -> done -> verifying ->
verified` stage bar is demoted to a derived lifecycle summary for phased goals.
It is not the main navigation or proof surface.

## Layout Contract

### Desktop

Viewport target: `1440x900` or wider.

```text
+----------------------------------------------------------------------------+
| Goal Workbench: goal-content-model-v1                       Status: working |
+----------------------------------------------------------------------------+
| +-----------------------------+ +--------------------+ +------------------+ |
| | Goal Spec / Acceptance      | | Phase Progress     | | Proof Health     | |
| | outcome summary             | | current phase      | | msg/evidence/dec | |
| | acceptance summary          | | blocked/review     | | evaluation state | |
| +-----------------------------+ +--------------------+ +------------------+ |
| Derived lifecycle: working (derived from phases, not the gate source)        |
| Next action: Critic review for Phase 3                         Owner: Lead   |
+--------------------------------------+-------------------------------------+
| Phase Spine                          | Inline Inspector                    |
| +---+ +----------------------------+ | +---------------------------------+ |
| | 1 | | GoalDesign accepted        | | | Selected task / evidence / gate | |
| | ok| | plan -> workflow passed    | | | assignment message              | |
| +---+ | evidence: goal_design      | | | AgentMember report              | |
|       +----------------------------+ | | evidence refs                   | |
| +---+ +----------------------------+ | | proposal / review / decision    | |
| | 2 | | Assignment accepted        | | +---------------------------------+ |
| | ok| | compiled workflow run      | |                                     |
| +---+ +----------------------------+ |                                     |
| +---+ +----------------------------+ |                                     |
| | 3 | | Implementation acceptance | |                                     |
| | ! | | workflow running / gate    | |                                     |
| +---+ +----------------------------+ |                                     |
+--------------------------------------+-------------------------------------+
| Unphased / Needs Triage      | Follow-up / After Decision                  |
+----------------------------------------------------------------------------+
```

Desktop scroll ownership:

- Page scrolls vertically.
- Phase Spine and Inspector may each have internal scroll only after the first
  viewport is filled.
- No horizontal page overflow is allowed.

### Tablet

Viewport target: around `834x1112` or `900x1180`.

```text
+------------------------------------------------+
| Goal Workbench: goal-content-model-v1           |
| Status: working       Current: Phase 3          |
+------------------------------------------------+
| +--------------------------------------------+ |
| | Goal Spec / Acceptance                     | |
| | outcome, non-goals, acceptance summary     | |
| +--------------------------------------------+ |
| +--------------------+ +---------------------+ |
| | Phase Progress     | | Proof Health        | |
| | 2/5 gates accepted | | msg ok / ev warning | |
| +--------------------+ +---------------------+ |
| Derived lifecycle: working -> verifying         |
| Next action: Critic review for Phase 3          |
+------------------------------------------------+
| Phase Spine                                     |
| +---+ +--------------------------------------+ |
| | ok| | Phase 1 GoalDesign                   | |
| +---+ | workflow passed / 3 steps done       | |
|       +--------------------------------------+ |
| +---+ +--------------------------------------+ |
| | ok| | Phase 2 Assignment                   | |
| +---+ | workflow compiled / messages sent    | |
|       +--------------------------------------+ |
| +---+ +--------------------------------------+ |
| | ! | | Phase 3 Implementation               | |
| +---+ | live run / 1 gate gap                | |
|       +--------------------------------------+ |
+------------------------------------------------+
| Inspector drawer opens over lower page          |
| Unphased / Follow-up Tasks                      |
+------------------------------------------------+
```

Tablet behavior:

- Inspector becomes a drawer or collapsible panel.
- Phase cards stay readable without horizontal scrolling.
- The header must still show Goal identity, phase progress, proof health, and
  next action in the first viewport.

### Mobile

Viewport target: around `390x844`.

```text
+------------------------------+
| Goal Workbench               |
| goal-content-model-v1        |
| working / Phase 3            |
+------------------------------+
| Goal Spec                    |
| outcome summary              |
| acceptance: screenshots      |
| gate: review pending         |
+------------------------------+
| Progress                     |
| phases: 2/5 accepted         |
| proof: msg ok / ev warning   |
| next: critic review          |
+------------------------------+
| Lifecycle projection         |
| working -> verifying         |
+------------------------------+
| Phase Spine                  |
| +--+ +---------------------+ |
| |ok| | GoalDesign          | |
| +--+ | accepted / 3 done   | |
|      +---------------------+ |
| +--+ +---------------------+ |
| |ok| | Assignment          | |
| +--+ | messages sent       | |
|      +---------------------+ |
| +--+ +---------------------+ |
| |! | | Implementation      | |
| +--+ | active / blocked    | |
|      +---------------------+ |
+------------------------------+
| Tap step -> expand inline    |
| Unphased / Follow-up         |
+------------------------------+
```

Mobile behavior:

- The global Workbench rail moves to bottom navigation so the Goal document owns
  the full viewport width.
- Step detail expands inline inside the phase by default; a full task document
  remains available as a secondary explicit action because `Task` remains the
  internal assignment/evidence object.
- Expanded task content must preserve visible Goal/phase context.
- No long table or multi-column task status board may force horizontal
  scrolling.

## Phase Spine

Each phase block must show:

- phase id/name, status, intent, and gate acceptance;
- current entry/exit condition when available;
- phase plan steps by meaningful groups: ready, waiting, running, blocked,
  review, done;
- compiled Starlark workflow generated from the current phase plan;
- latest workflow run status, step counts, attempts, verdict, and a direct link
  to the workflow run detail for realtime execution data;
- phase plan sequence that can expand each step inline;
- evidence/review/decision state for the phase gate;
- declared outputs and landed commit when present;
- next action and owner.

Kanban is not a phase-internal interaction. The Work page may still expose a
goal-scoped status projection, but inside a phase the primary question is:
what is the plan, what workflow did the plan compile into, what is the live run
doing, and what proof is missing for the gate.

The Goal page must not expose `Task Graph` as a product concept, tab, or
advanced operator view. Dependencies and owned-path grouping remain compiler
inputs, but the user-facing surface calls them phase plan steps and compiled
workflow branches.

## Task Grouping

Task grouping rules:

- `Task.phase_id` is the primary join key.
- Phase plan steps render only in their phase.
- Unphased tasks render in `Unphased / Needs Triage`.
- Follow-up tasks render in `Follow-up / After Decision` when identifiable from
  GoalEvaluation, Decision, or parent/follow-up refs.
- A task detail panel must show assignment message, assignee, reviewer, owned
  paths, acceptance criteria, reports, evidence refs, proposal/review/decision
  refs, and runtime/member state when present.

## Proof Chain

Proof health is shown in the header and phase inspector. It checks:

- GoalDesign evidence or object exists;
- task assignment message exists before implementation/report;
- AgentMember report exists when work is claimed;
- evidence refs exist before review/decision;
- proposal or review packet exists for implementation acceptance;
- Leader decision exists;
- GoalEvaluation exists before closeout, or a valid waiver exists.

Every failed check must expose a next action and avoid implying the Goal is done
from task activity alone.

## Screenshot Gate

Implementation acceptance for this page requires actual browser evidence:

- Desktop screenshot at about `1440x900` or wider.
- Tablet screenshot at about `834x1112` or `900x1180`.
- Mobile screenshot at about `390x844`.
- Console check for runtime errors and React warnings.
- Horizontal overflow check at every viewport.
- Interaction check opening at least one phase task detail without losing Goal
  context.

`npx pnpm@9.15.4 check:dashboard` is required but not sufficient. Without real
screenshots, console output, overflow proof, and interaction verification, the
Goal Workbench implementation is not accepted.

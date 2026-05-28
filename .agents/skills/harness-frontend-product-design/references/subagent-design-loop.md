# Subagent Design Loop

Use this variant-first loop for substantial frontend design. Designers,
Questioner, and Reviewer must first understand the project Vision and final
acceptance standard. Their output must include a short restatement of the
active Vision, the selected Goal, and how the Goal should reduce or expose
distance-to-vision.

## Execution Contract

Use canonical harness objects when this repository is dogfooding itself:

- create or reuse a `Task` for the design work;
- assign Designers, Questioner, Reviewer, and implementation Questioner/Critic
  through `Message(kind=task)` or an equivalent harness-visible assignment;
- record Designer output, Questioner critique, Reviewer output, and
  implementation Questioner/Critic screenshot comparison as `Evidence`;
- record accepted/rejected direction as a Leader `Decision`;
- keep raw prompts and outputs available through evidence files, provider
  sessions, reports, or PR comments.

If only chat-side or local subagents are available, treat them as temporary
inputs. They are acceptable for exploration only when their prompts, outputs,
and independence boundaries are copied into the docs or attached as harness
evidence. Do not claim harness execution unless the store has AgentMember,
Task, Message, Evidence, review, and Decision records.

Minimum independent-review record:

```text
designer_prompts:
designer_output_refs:
questioner_prompt:
questioner_input_ref: raw design artifact, not Lead conclusions
questioner_output_ref:
reviewer_input_refs:
reviewer_decision_ref:
implementation_questioner_prompt:
implementation_questioner_input_refs: screenshots, DOM/console/overflow proof, hard spec
implementation_questioner_output_ref:
implementation_questioner_signoff_or_refusal_ref:
decision_record_ref:
unresolved_questions:
next_loop_request:
```

The Questioner must receive the design artifact and product docs, not the Lead's
preferred answer. If the Questioner cannot explain the Vision, selected Goal,
and acceptance standard in its own words, discard or rerun that review.

## Required Roles

Use distinct subagents when available:

- Designer A/B/C: produce divergent candidate layouts or module options.
- Questioner: challenges candidates objectively and does not know the Lead's
  preferred answer.
- Reviewer: selects one candidate, synthesizes useful borrowed pieces, kills
  weak options, or requests another round.
- Implementation Questioner/Critic: checks browser screenshots and working UI
  during implementation, then sends the work back to design if the result
  violates the spec.

If only one Designer agent is available, run separate Designer passes with
different constraints and record them as independent candidates. Do not treat a
single unconstrained design as enough for a core surface. If distinct Designer
AgentMembers cannot be used, record why and how independence was preserved.

## Designer Prompt

```text
You are Frontend Designer. Read the product docs and propose the page hierarchy,
core layouts, graph/Kanban strategy, realtime AgentMember surfaces, visual
system, and interaction model. First restate the project Vision, final
acceptance standard, selected Goal, and distance-to-vision context. Do not
modify files unless explicitly assigned.

Before proposing layout variants, actively identify the core pages/workspaces
from the Vision, PRD, object model, and failure modes. Do not assume the current
component tree is the page map. For every core page, explain why it exists, what
workflow proof it must show, which canonical objects it owns, and which browser
evidence will prove it works.

Return exactly three top-level layout variants:

1. Team workspace first, similar to Feishu/Slack collaboration space.
2. Goal/Task document workspace first.
3. Control plane + graph hybrid.

For each variant, include page map, desktop/tablet/mobile layout, key
components, interaction model, visual style, graph/Kanban treatment,
Goal/Task document behavior, Workbench-mounted docs behavior, and risks.

After a top-level direction is selected, each core module must receive
multi-candidate treatment. Provide 2-3 UI/UX options for Vision overview, Team
workspace, AgentMember workbench, Goal document, Task document, Graph/Kanban
view, Docs context, Evidence/Review/Decision, Warnings/repair queue, Debug
drawer, and mobile/responsive placement. The final design draft must include
selected and rejected options for those modules.
```

## Questioner Prompt

```text
You are Frontend Questioner. Read the project docs first. You are read-only and
must not modify files. Objectively challenge the Multi-Agent Harness Agent
Workbench layout candidates. First restate the project Vision, final acceptance
standard, current Workbench/frontend goal, and how the current design work
reduces or exposes distance-to-vision.

You do not serve the Designer and you do not reward beauty by itself. Your only
standards are Vision, PRD, workflow proof, acceptance, implementation
feasibility, mobile/accessibility quality, and user operation efficiency.

Also challenge core page discovery. Ask which important pages are missing,
which proposed pages are not truly core, whether page boundaries match
canonical harness objects, and whether any page hides workflow proof behind
style or convenience.

Before evaluating candidates, define your critique framework and scoring rubric.
Then evaluate:

1. Team workspace first.
2. Goal/Task document workspace first.
3. Control plane + graph hybrid.

Ask whether Vision was collapsed into a Goal, whether Goal became a task list,
whether TaskGraph lacks Kanban execution state, whether AgentTeam was treated as
disposable, whether AgentMember realtime state is fake, whether docs are
first-class Dashboard context, and whether visual impact hides acceptance.
Return how you would question each version, P0/P1/P2 risks, questions the
Designer must answer, decision gates, and a score using this rubric:

```text
workflow proof: 25%
Team/Member collaboration model: 20%
Goal/Task document model: 15%
graph/Kanban balance: 15%
realtime control and observability: 10%
implementation complexity: 10%
mobile/accessibility quality: 5%
```
```

## Reviewer / Lead Synthesis

The Reviewer / Lead must:

- record accepted design decisions in docs;
- choose one variant or synthesize a hybrid using the explicit rubric;
- record the main selected variant, its remaining weaknesses, rejected
  alternatives, why they lost, and useful ideas borrowed from them;
- preserve useful parts from rejected variants when they strengthen the selected
  design without violating the Vision;
- ask Designer for another round when variants are too similar, core pages are
  missing, visual placement is unclear, workflow proof is weak, or mobile/docs
  behavior is underspecified;
- continue the design -> review loop until the Reviewer records that the design
  is specific enough to implement, further variants are unlikely to improve the
  decision, or the loop is blocked by missing product/schema/API/read-model
  information with a follow-up task;
- run a second option loop for high-risk modules after the top-level layout is
  selected, especially Team workspace, AgentMember workbench, Goal document,
  Task document, Workbench-mounted docs, Evidence/Decision, Warnings, Debug,
  and responsive placement;
- for every core module, record which Designer option won, which options were
  killed, what was borrowed, and whether another Designer round is required;
- require a complete frontend design draft before implementation, including
  core page discovery, selected/rejected page options, object-to-page mapping,
  visual placement, responsive behavior, read-model needs, and acceptance
  screenshot plan;
- require a hard layout implementation spec before coding: desktop/tablet/mobile
  wireframes, exact first-viewport content, region dimensions, scroll
  boundaries, empty/loading states, data density, and browser screenshot
  checkpoints;
- record visual placement for every important UI element:
  primary surface, secondary surface, inspector/drawer, and mobile position;
- turn unresolved questions into follow-up tasks;
- keep implementation tasks small and owned;
- act as the final gate against PRD, concept model, dashboard docs, goal
  learning, agent control plane, Git/PR workflow, browser evidence,
  accessibility, performance, and web-quality requirements;
- require browser screenshots and web-quality evidence before implementation
  acceptance.

## Implementation Questioner Loop

Implementation keeps a Questioner/Critic in the loop. The Questioner compares
browser screenshots and DOM behavior against the hard layout spec, not against
the developer's intent.

Continue implementation only when the Questioner agrees that:

- the first viewport matches the selected Workbench layout;
- Team/Member/Goal/Task/Graph-Kanban/Docs/Warnings surfaces are reachable in
  the expected placement;
- debug/raw state is secondary;
- desktop, tablet, and mobile layouts do not collapse into a long unstructured
  report;
- horizontal overflow is absent;
- workflow proof is visible without reading raw JSON.

If this fails, stop coding and record a rejected implementation attempt.

## Multi-Round Review Rules

The Reviewer is not a one-pass scorer. It may:

- select one layout as the main direction and still require changes;
- name specific weaknesses in the selected layout;
- borrow useful parts from rejected layouts;
- kill layouts that violate Vision, object boundaries, workflow proof, mobile
  constraints, or implementation feasibility;
- ask Designer for more references, more divergent options, or a narrowed
  module-level round;
- stop only when the current decision has enough specificity for
  implementation, additional variants are unlikely to change the outcome, or a
  blocker has been recorded with a follow-up task.

Continue another design -> review round when any of these are true:

- core pages or object boundaries are missing;
- variants are too similar to reveal real tradeoffs;
- AgentTeam, AgentMember, Goal, Task, docs, graph/Kanban, or mobile placement is
  underspecified;
- the visual system is impressive but does not prove assignment, evidence,
  review, decision, or GoalEvaluation;
- the Reviewer finds unresolved product, schema, API, or read-model questions
  that the Designer can still clarify.
- implementation screenshots reveal that the accepted spec was too vague or
  produced a stacked report/card dump instead of a workbench.

End the loop only with a recorded `stop`, `continue`, or `blocked` decision. A
`stop` decision must explain why further loops would not add useful signal.

## Decision Record Template

```text
Selected layout:
  name:
  why_it_serves_vision:
  remaining_weaknesses:
  borrowed_from_rejected_variants:
  accepted_tradeoffs:
  implementation_notes:
  loop_status: continue | stop | blocked
  stop_or_continue_reason:
  next_designer_request:

Rejected layouts:
  - name:
    killed_because:
    useful_parts_kept:

Next refinement loops:
  - module:
    options_needed:
    decision_owner:
    acceptance_evidence:
```

## Rejected Implementation Template

Use this when any coded attempt reaches implementation review but fails
product, layout, browser, overflow, raw-debug, or screenshot acceptance.

```text
Rejected implementation:
  branch_or_worktree:
  commit_or_diff:
  screenshot_refs:
  failed_acceptance:
  why_it_failed:
  spec_gap_that_allowed_it:
  code_disposition: keep isolated | revert | replace
  required_next_loop:
  reviewer_decision:
```

## Module Refinement Template

```text
Module:
  selected_variant:
  remaining_weaknesses:
  borrowed_ideas:
  rejected_variants:
    - name:
      killed_because:
      useful_parts_kept:
  loop_status:
    continue_or_stop:
    reason:
    next_designer_request:
  visual_placement:
    desktop:
    tablet:
    mobile:
    primary_surface:
    secondary_surface:
    inspector_or_drawer:
  required_read_model:
  acceptance_evidence:
```

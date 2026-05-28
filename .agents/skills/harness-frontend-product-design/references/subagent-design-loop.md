# Subagent Design Loop

Use this variant-first loop for substantial frontend design. Both design
subagents must first understand the project Vision and final acceptance
standard. Their output must include a short restatement of the active Vision,
the selected Goal, and how the Goal should reduce or expose
distance-to-vision.

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

Return exactly three layout variants:

1. Team workspace first, similar to Feishu/Slack collaboration space.
2. Goal/Task document workspace first.
3. Control plane + graph hybrid.

For each variant, include page map, desktop/tablet/mobile layout, key
components, interaction model, visual style, graph/Kanban treatment,
Goal/Task document behavior, Dashboard-mounted docs behavior, and risks.

After a top-level direction is selected, propose 2-3 page-level UI/UX options
for every high-risk core page. The final design draft must include page specs
for Vision overview, Team workspace, AgentMember workbench, Goal document, Task
document, Graph/Kanban view, Docs context, Evidence/Review/Decision,
Warnings/repair queue, and Debug drawer.
```

## Questioner Prompt

```text
You are Frontend Questioner. Read the project docs first. You are read-only and
must not modify files. Objectively challenge the Multi-Agent Harness Agent
Dashboard layout candidates. First restate the project Vision, final acceptance
standard, current Dashboard/frontend goal, and how the current design work
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

## Lead Synthesis

The Lead must:

- record accepted design decisions in docs;
- choose one variant or synthesize a hybrid using the explicit rubric;
- record rejected alternatives and why they lost;
- preserve useful parts from rejected variants when they strengthen the selected
  design without violating the Vision;
- run a second option loop for high-risk modules after the top-level layout is
  selected, especially Team workspace, AgentMember workbench, Goal document,
  Task document, Dashboard-mounted docs, Evidence/Decision, Warnings, Debug,
  and responsive placement;
- require a complete frontend design draft before implementation, including
  core page discovery, selected/rejected page options, object-to-page mapping,
  visual placement, responsive behavior, read-model needs, and acceptance
  screenshot plan;
- record visual placement for every important UI element:
  primary surface, secondary surface, inspector/drawer, and mobile position;
- turn unresolved questions into follow-up tasks;
- keep implementation tasks small and owned;
- act as the final gate against PRD, concept model, dashboard docs, goal
  learning, agent control plane, Git/PR workflow, browser evidence,
  accessibility, performance, and web-quality requirements;
- require browser screenshots and web-quality evidence before implementation
  acceptance.

## Decision Record Template

```text
Selected layout:
  name:
  why_it_serves_vision:
  accepted_tradeoffs:
  implementation_notes:

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

## Module Refinement Template

```text
Module:
  selected_variant:
  rejected_variants:
    - name:
      killed_because:
      useful_parts_kept:
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

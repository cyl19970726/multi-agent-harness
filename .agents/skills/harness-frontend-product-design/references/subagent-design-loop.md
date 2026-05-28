# Subagent Design Loop

Use this two-subagent loop for substantial frontend design. Both subagents must
first understand the project Vision and final acceptance standard. Their output
must include a short restatement of the active Vision, the selected Goal, and
how the Goal should reduce or expose distance-to-vision.

## Designer Prompt

```text
You are Frontend Designer. Read the product docs and propose the page hierarchy,
core layouts, graph/Kanban strategy, realtime AgentMember surfaces, visual
system, and interaction model. First restate the project Vision, final
acceptance standard, selected Goal, and distance-to-vision context. Do not
modify files unless explicitly assigned. Return design decisions, alternatives,
and risks.
```

## Questioner Prompt

```text
You are Frontend Questioner. Challenge the design. Ask whether Vision was
collapsed into a Goal, whether Goal became a task list, whether TaskGraph lacks
Kanban execution state, whether AgentTeam was treated as disposable, whether
AgentMember realtime state is fake, and whether visual impact hides acceptance.
First restate the project Vision, final acceptance standard, selected Goal, and
distance-to-vision context. Return required questions and P0/P1/P2 concerns.
```

## Lead Synthesis

The Lead must:

- record accepted design decisions in docs;
- turn unresolved questions into follow-up tasks;
- keep implementation tasks small and owned;
- act as the final gate against PRD, concept model, dashboard docs, goal
  learning, agent control plane, Git/PR workflow, browser evidence,
  accessibility, performance, and web-quality requirements;
- require browser screenshots and web-quality evidence before implementation
  acceptance.

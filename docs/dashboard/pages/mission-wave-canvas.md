# Mission / Wave Canvas Page Spec

```text
status: implemented
owner_role: product-design
canonical_for: Mission planning, ordered Wave execution, and Wave gate decisions
route_or_surface: Missions -> Mission -> selected Wave
```

## User Problem

The Host needs to see and steer a durable objective as a simple ordered
sequence: what was accepted, what is executing now, what must be decided, and
what comes next. Earlier Goal/Phase and graph-heavy views obscure this with
planning machinery. The primary user question is:

> What is the next meaningful Wave decision for this Mission, and what does
> the evidence from the current executor attempt justify?

The Canvas keeps Mission intent and ordered Waves central. It embeds concise
executor controls but does not turn the Mission into a Team transcript,
workflow debugger, or legacy dependency graph.

## Canonical Data And Semantics

Required data:

- native `Mission`: title, objective, status, provenance, closeout summary, and
  project context;
- ordered native `Wave`: index, title, objective, executor kind, exit criteria,
  status, artifacts, outcomes, deviations, re-plan notes, and gate state;
- executor projection appropriate to each selected Wave:
  `AgentTeamRun` attempts / `WorkflowRun` / host outcome;
- for agent-team Waves: compact `MemberRun` state, assignment-correlated
  activity, attempt outcome, and linked artifacts/checks; and
- explicit gate record: `accepted | revise | blocked`, accepted run when any,
  accepted-by actor, note, and timestamp.

The only native hierarchy is:

```text
Mission -> ordered Wave -> executor
executor = agent_team | dynamic_workflow | host
```

A Wave is a lightweight integration boundary, not an implementation phase,
dependency graph, or provider session. Retired coordination records are not
loaded into the active Mission/Wave authoring or Dashboard path.

For an agent-team Wave, TeamRun completion makes an attempt available for
review. It never updates `Wave.gate_status` by implication. A Host gate action
must explicitly choose `accepted`, `revise`, or `blocked` and name the
completed attempt if accepted.

## Layout Contract

The visual reference is the approved
`mission-wave-canvas/running-gate-pending--desktop` expected design in
[`../../design/execution-workbench-v3/`](../../design/execution-workbench-v3/README.md).

### Desktop — `1440x1000`

Use the shared Workbench shell: 230px product sidebar, about 800px center
canvas, 340px Context Rail.

```text
+----------------------+--------------------------------------+------------------+
| Product sidebar      | Mission header                       | Context Rail     |
| Missions / Agents    | objective · status · actions         | Mission brief    |
| Workflows / Knowledge+--------------------------------------+ Needs You        |
| Active context tree  | Wave 1 · accepted (compact)          | Selected Wave    |
|                      +--------------------------------------+ Agent Team or    |
|                      | Wave 2 · expanded/current             | workflow compact |
|                      | objective / executor / exit criteria  | Gate & outcome   |
|                      | compact executor controls              |                  |
|                      | attempt / outputs / gate readiness    |                  |
|                      +--------------------------------------+                  |
|                      | Re-plan after Wave 2                  |                  |
|                      +--------------------------------------+                  |
|                      | Wave 3 · planned (compact)            |                  |
+----------------------+--------------------------------------+------------------+
```

Waves remain a vertical ordered flow, never a graph, Gantt, kanban, or
dependency graph canvas. The current/selected Wave expands by default. Accepted
Waves are concise read-only history; planned Waves are collapsed until selected.
The re-plan band appears only where plan-versus-actual changed a later Wave.

The expanded Wave shows objective, executor kind, exit criteria, current gate
state, output/evidence summary, and the executor-specific compact control.
For `agent_team`, this is TeamCompact plus MemberCompact controls and attempt
lineage. For `dynamic_workflow`, it is WorkflowRun/step outcome context. For
`host`, it is the declared host outcome/artifacts. None is forced into a
universal executor object.

### Tablet — `900x1180`

- Collapse the product sidebar; retain Mission title, current Wave, and status.
- Keep the ordered Wave flow in the main column.
- The selected Wave remains expanded; nonselected Waves collapse to objective,
  executor, and gate chip.
- Context Rail becomes a contextual sheet/inline region. `Needs You`,
  `Selected Wave`, and `Gate & Outcome` remain immediately accessible.
- Gate controls stay adjacent to the selected Wave's evidence summary, not at
  the end of an unrelated page.

### Mobile — `390x844`

- Top bar exposes back-to-Missions, Mission status, and Context action.
- Display an ordered vertical list with one expanded Wave at a time.
- Each compact Wave shows number, title, executor, status, gate chip, and one
  outcome/next-action line.
- The expanded Wave has its executor compact control and gate readiness; its
  detailed Team/Member surfaces open as separate routes.
- Context uses a bottom sheet ordered `Needs You`, `Gate & Outcome`, `Selected
  Wave`, `Mission Brief`, then executor summary. No horizontal overflow.

## Context Rail Modules

1. **MissionBrief** — durable objective, scope/provenance, current status, and
   closeout information when present.
2. **NeedsYou** — only real pending approvals, blockers, or explicit decisions;
   no decorative zero-state card.
3. **SelectedWavePanel** — objective, executor kind, exit criteria, state,
   deviations, artifacts, and open-executor action.
4. **ExecutorCompact** — AgentTeam, Dynamic Workflow, or Host variant. It
   expresses the executor's own truth without erasing their different models.
5. **GateOutcome** — candidate attempt/result, criteria satisfaction, missing
   proof, historical gate decision, and allowed gate actions.

`WaveMicro`, `WaveCompact`, and `WavePanel` are density variants of the same
object; the Team and Member controls use the same principle. Modules may
collapse but do not become user-authored dashboard widgets in the first release.

## Actions

Implemented now:

- Create a Mission, create ordered Waves, and select a Wave.
- Open a selected Wave's Team War Room, MemberRun Focus, WorkflowRun, host
  output, artifact, or check.
- Gate the selected Wave as `accepted`, `revise`, or `blocked` when its
  executor outcome and required evidence are available.
- Retry an agent-team Wave by creating a new TeamRun attempt linked to the same
  Wave; do not mutate an old attempt away or create a fake replacement Wave.
- Close a Mission with an explicit outcome after all Waves are accepted.

Target follow-up, not an implemented contract:

- reorder or edit a planned Wave after creation; and
- mutate a structured re-plan delta after plan-versus-actual evidence. The
  current implementation records the re-plan note when a Wave is created.

The gate action explicitly selects a completed attempt for acceptance. It must
reject a running/incomplete attempt and state why. No button may imply that
finishing a TeamRun has already accepted the Wave.

## Empty, Loading, And Failure States

- **New Mission/no Waves:** show the Mission objective and one clear action to
  define the first ordered Wave; do not show a blank graph canvas.
- **No selected Wave:** select the current active Wave, otherwise the first
  planned Wave; explain if all Waves are closed.
- **No executor run:** render its plan/exit criteria and a truthful `not yet
  started` executor state.
- **Executor failed/blocked:** retain the Wave's context and show the relevant
  attempt/outcome and Next decision: revise, retry, or block. Do not infer a
  gate result.
- **Missing evidence:** GateOutcome lists the missing criterion/proof and keeps
  accept unavailable; artifacts may legitimately be absent when they are not a
  Wave requirement.
- **Read error/offline:** retain a clearly stale last projection and scoped
  retry; do not erase the ordered flow with a generic empty state.

## Screenshot Acceptance

For `mission-wave-canvas--running-gate-pending--desktop`:

- use the registered native fixture, route, and `1440x1000` viewport for
  baseline, implementation, and comparison;
- first viewport contains Mission header, three ordered Waves, accepted prior
  Wave, expanded running Wave, planned next Wave, and a visible re-plan band;
- expanded agent-team Wave contains a compact Team control with member state,
  attempt, outputs/evidence, and Gate section;
- gate readiness visibly reads as pending/review and remains distinct from the
  Team attempt's completion/running state;
- Sidebar, main canvas, and Context Rail follow the approved shared visual
  system; no legacy dependency graph, Gantt, kanban, or generic analytics layout appears;
- all material divergence from the approved expected image is recorded in the
  Workbench Layout V2 visual contract with defects or intentional deviations.

## Explicit Boundaries

- Mission/Wave is the only active coordination hierarchy for new work.
- A Wave needs neither a legacy dependency graph nor universal member/task ownership.
- Team, Workflow, and Host executor controls share shell components but retain
  separate canonical records.
- `MemberRun` remains a run-scoped team instance, not a StandingAgent page.
- TeamRun completion is evidence about an attempt only. Wave gate acceptance,
  revision, or block is a separate durable parent decision.

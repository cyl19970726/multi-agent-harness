# ADR 0026: Mission/Wave Product Architecture

## Status

Accepted product direction; implementation migration planned.

This decision supersedes ADR 0024 as the future product orchestration model and
amends ADR 0025. Existing GoalPhase code, commands, ledgers, and stored data
remain compatibility surfaces until the migration below is implemented and
verified.

## Context

The historical harness grew around a complete governance chain:
Goal -> GoalPhase -> Task Graph -> Message -> Evidence -> Proposal -> Review ->
Decision -> GoalEvaluation. That model is implemented, but it was not the
multi-agent product that operators began using.

PR #207 introduced the first intended-for-use Agent Team control plane:
cross-provider MemberRuns, message delivery, normalized actions, delegation,
Kimi ACP sessions, plugins/MCP, and a live workbench. Applying the historical
GoalPhase and Task Graph model to this surface creates two competing sources of
truth and makes simple multi-wave work unnecessarily expensive to understand.

The product is better understood as tools for resident Host Agents. A Host
installs a plugin and chooses Dynamic Workflow, Agent Team, or direct Host
execution for each stage of a larger outcome. Future Standing Agents + Docs
will use the same tools for long-lived business operation.

## Decision

### Product hierarchy

The canonical hierarchy is:

```text
Mission -> ordered Wave -> executor
```

`Mission` replaces `Goal` as the product term. A Mission is the durable intent
and desired-outcome container.

The naming choice is deliberate:

- `Objective` remains the concrete desired result inside a Mission or Wave;
- `Outcome` is what execution produced, so it cannot name the container;
- `Initiative` sounds like portfolio/project management and is too broad for a
  resident Agent's executable unit;
- `Run` names an execution attempt, not durable intent;
- `Mission` naturally contains several Waves and fits both one-off Host work
  and future long-lived business operation without implying a Task Graph.

`Wave` replaces `GoalPhase` as the product term and future orchestration
object. A Wave is intentionally small:

```text
Wave
  id
  mission_id
  index
  title
  objective
  exit_criteria?
  status
  executor_kind: agent_team | dynamic_workflow | host
  executor_run_ids[]
  accepted_run_id?
  plan_note?
  outcome_summary?
  artifact_refs[]
  gate_status: pending | accepted | revise | blocked
  gate_note?
  accepted_by?
  accepted_at?
  created_at / updated_at
```

A Wave does not own or require a Task Graph. It delegates execution semantics
to its executor.

### Executor semantics

- `agent_team`: creates one or more AgentTeamRun attempts. Work identity is a
  `TeamMessage(kind=assignment)` plus its `correlation_id`. Member actions,
  blockers, handoffs, reviews, and delegations correlate to that assignment.
- `dynamic_workflow`: points at WorkflowRun attempts. The workflow owns its
  internal steps, fan-out, retry, and structured result.
- `host`: the resident Host executes directly. It may use provider-native
  subagents. Those children remain provider implementation detail unless hooks
  expose honest observational events.

`executor_run_ids[]` separates the logical Wave from retries or replacement
runs. `accepted_run_id` identifies the attempt the gate accepted.

### Minimal acceptance

Wave completion is a lightweight gate, not the historical global object chain.
The gate records who accepted the Wave, when, a short note, outcome summary,
and artifact references when proof is useful. An Agent Team Wave normally
reaches the gate through assignment -> handoff -> optional review_result.

The repository may continue to use its stricter historical evidence and review
objects while it self-hosts the migration. Those are current repository
governance, not required concepts in the new Agent Team product model.

### Shared infrastructure, distinct semantics

Dynamic Workflow, Agent Team, Host execution, and future Standing Agents share
provider-neutral runtime/session control, capability snapshots, permission and
budget ceilings, artifact references, event transport, hooks, plugins/MCP, and
Dashboard projections.

They do not become one universal Agent or Run object. A WorkflowStep is a
one-shot graph node, a MemberRun is a collaborator inside a TeamRun, a Host
subagent is provider-controlled, and a Standing Agent has durable business
identity and knowledge.

### Thinking

Thinking is optional transient live state. If a provider exposes it, the
adapter sanitizes, truncates, and rate-limits it into a non-durable live channel.
It expires or is overwritten, cannot be replayed, never enters JSONL/snapshots,
never counts as evidence, and is never forwarded to peers.

Explicit plans, tool actions, artifacts, blockers, handoffs, and outcomes remain
durable.

## Options Rejected

### Keep Goal as the product term

Rejected because Goal is already coupled to GoalDesign, GoalPhase, Task Graph,
and the historical self-hosting governance chain in current code and docs.
Keeping the word while changing all of its semantics would make migration copy
and operator expectations ambiguous.

### Keep GoalPhase and render it as Wave

Rejected because GoalPhase already owns task DAGs, compiled workflows, retries,
landing commits, and derived Goal stages. Calling it Wave preserves the exact
complexity the product is removing.

### Create Wave above GoalPhase

Rejected because it creates two phase/gate/status models and an ambiguous source
of truth.

### Make Task Graph mandatory inside Agent Team

Rejected because assignment messages already express work, delivery, blocker,
handoff, and review semantics. Teams may use internal planning, but the harness
does not require a graph.

### Make Host subagents canonical harness tasks

Rejected because provider-native children are controlled by the Host/provider.
Hooks may observe them, but the harness must not claim lifecycle or acceptance
authority it does not possess.

## Consequences

- Product UI and docs use Mission/Wave, while compatibility surfaces label
  Goal/GoalPhase explicitly.
- Agent Team target schemas eventually remove Task Graph joins such as
  `AgentTeamRun.task_ids`, `MemberRun.current_task_id`, and Team object
  `task_id` fields in favor of assignment-message correlation.
- Wave needs attempt lineage and a lightweight gate.
- Current self-hosting governance can remain stricter than the exported product
  model during migration.
- The architecture map becomes the canonical cross-module diagram.

## Non-destructive Migration

1. **Docs:** make this ADR and `docs/architecture-map.md` canonical. Mark ADR
   0024, the GoalPhase loop, and run-centric UI sections transitional.
2. **Additive contracts:** introduce Mission/Wave schemas and projection code;
   dual-read existing Goal/GoalPhase data without rewriting JSONL history.
3. **Agent Team joins:** add `mission_id`, `wave_id`, and
   `assignment_message_id`/correlation joins. Keep old task fields readable but
   stop requiring them for new runs.
4. **Runtime:** route Wave executor selection to Agent Team, Dynamic Workflow,
   or Host. Prove retries through `executor_run_ids[]` and `accepted_run_id`.
5. **Thinking:** remove durable thinking writes after the transient channel is
   available; preserve old rows as history but exclude them from new snapshots.
6. **CLI/API/Dashboard/plugins/skills:** promote Mission/Wave names and keep
   temporary Goal/phase aliases with deprecation output.
7. **Removal:** delete GoalPhase-specific orchestration only after fixtures,
   migrations, stored-data reads, live Wave runs, governance, and Dashboard
   acceptance pass. Record the removal in a later ADR.

## Affected Contracts

- `Goal`, `GoalPhase`, Task phase joins, and goal orchestration runtime;
- AgentTeamRun, MemberRun, TeamMessage, MemberAction, and DelegationRun joins;
- WorkflowRun attachment to an outer Mission/Wave;
- CLI, MCP tools, plugins, skills, Dashboard read models, schemas, fixtures,
  stored JSONL compatibility, governance registry, and acceptance suites.

## Validation

The design stage passes when canonical docs contain no claim that Wave attaches
to GoalPhase, no required Task Graph remains in Mission/Wave or Agent Team, the
thinking boundary is transient, and governance/doc checks pass. Runtime
acceptance belongs to the follow-up implementation Mission.

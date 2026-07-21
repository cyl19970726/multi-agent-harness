# ADR 0025: Agent Team Run Control Plane

## Status

Accepted as the v0 Agent Team substrate.

Superseded in part by ADR
[0026](0026-mission-wave-architecture.md) for top-level product hierarchy,
Mission/Wave terminology, and thinking policy. This ADR remains canonical for
the v0 Agent Team object set, delegation guardrails, and host/tooling split.
ADR [0032](0032-provider-native-session-is-execution-truth.md) supersedes its
durable provider-activity mirroring: provider transcript/tool/command/file
events stay in the native session, while Harness keeps coordination facts.

## Context

Agent Team exists for work that cannot be reduced to a one-shot function call.
A sub-agent returns a result and disappears. An Agent Team member is a living
collaborator with its own mailbox, runtime state, and responsibility lane until
its attempt completes or is cancelled. The separate Wave gate then decides
whether a completed attempt is accepted, revised, or blocked.

The v0 implementation goal is not to solve the whole Mission/Wave product. It
is to prove the first real `agent_team` executor substrate:

- wave-scoped collaboration across providers;
- explicit assignment, handoff, blocker, and review messages;
- observable member actions and delegation;
- shared dashboard, CLI, and host-tool read model.

The native Mission-first Console is now implemented for this branch: it creates
Missions and ordered Waves, creates/retries linked AgentTeamRun attempts, and
shows the selected project's durable updates over SSE. This does not make
Dynamic Workflow or Host a routed Agent Team control plane.

## Decision

### Layering

Within the canonical Mission/Wave model, this ADR owns the `agent_team`
executor branch:

```text
Mission
  -> Wave(executor_kind=agent_team)
    -> AgentTeamRun
      -> MemberRun
        -> DelegationRun
```

Other executor kinds (`dynamic_workflow`, `host`) are defined by ADR 0026 and
reuse shared runtime infrastructure without adopting Agent Team semantics.

### v0 object model

```text
AgentTeamRun    id, mission_id?, wave_id?, objective, status, budget_limit_usd?,
                host{surface, thread_id}, member_run_ids[],
                created_at / started_at / ended_at

MemberRun       id, team_run_id, name, role, provider, model?,
                status(starting|idle|queued|running|waiting|reviewing|
                       blocked|completed|failed|stopped),
                native_session?, current_task_id?,
                worktree_ref?, owned_paths[], created_at / ended_at

TeamMessage     id, team_run_id, task_id?, from, to[], kind,
                correlation_id, causation_id?, evidence_refs[],
                deliveries[{ member_id, policy, status, attempt, updated_at }]

MemberAction    id, seq, team_run_id, member_run_id, task_id?,
                type(plan_updated|message_sent|message_received|
                     tool_started|tool_completed|file_changed|
                     command_started|command_completed|test_started|
                     test_completed|delegation_started|
                     delegation_completed|review_started|
                     review_completed|waiting_for_input|
                     waiting_for_approval|blocked|error|completed),
                status(started|progress|succeeded|failed|cancelled),
                title, summary, evidence_refs[],
                started_at / ended_at

DelegationRun   id, team_run_id, parent_member_run_id, parent_task_id?,
                mode(provider_native|harness_worker|dynamic_workflow),
                provider, provider_child_thread_id?, workflow_run_id?,
                objective, status, evidence_ids[]

TeamRunEvent    id, seq, team_run_id,
                source{kind(host|member|delegation), member_run_id?,
                       delegation_run_id?},
                entity_type, entity_id, operation(created|updated|completed),
                summary, occurred_at
```

Rules:

- `AgentTeamRun` is one execution attempt for one Wave and becomes read-only
  history when the attempt terminates. A Wave may retry with a new run and its
  gate identifies the accepted attempt.
- `MemberRun` is an execution instance, not a standing durable employee record.
- `TeamMessage` separates message semantics from per-recipient delivery state.
- `MemberAction` and `TeamRunEvent` are transitional for provider-derived work
  events. Their target scope is Harness-owned coordination, control requests /
  acknowledgements, and lifecycle facts; they do not mirror provider activity.
- A native attempt links both `mission_id` and `wave_id`. Optional identifiers
  remain at the Store/API boundary only for reading imported records; unlinked
  runs are excluded from active Agent Team product navigation and authoring.
- Attempt completion (`reviewing -> completed`) is separate from the Wave gate;
  only a completed attempt can be accepted by that parent Wave.

### Assignment-message correlation

Ownership inside an Agent Team Wave is explained by message correlation, not by
an exposed legacy dependency graph:

```text
TeamMessage(kind=assignment)
  -> correlation_id
  -> Harness blocker / handoff / review / PendingInteraction
  -> explicit outcome + artifacts/check refs
  -> NativeSessionRef for member execution detail
```

This is the target proof chain for lane ownership inside the run.

Automatic handoff reuses its assignment's `correlation_id`. Manual CLI, HTTP,
and MCP sends now accept `correlation_id` and `causation_id`: explicit
correlation must identify an Assignment in the same run, causation must identify
a message in the same run, and a causation-only reply inherits its cause's
correlation. Invalid or cross-run lineage is rejected before a message append.
Messages that omit both fields retain an opaque generated correlation and make
no claim of assignment ownership.

### Delegation guardrails

Two delegation modes are valid:

- `provider_native`: the member invokes its provider's native subagent
  capability. The harness captures attribution when hooks or artifacts expose
  it, but does not pretend to control the child lifecycle.
- `harness_worker` / `dynamic_workflow`: the harness launches the child itself,
  so it enforces delegation depth, path subset, permission ceiling, and budget
  limits.

If a provider capability exists but the adapter has not verified or wired the
observation path, the harness degrades honestly to `dynamic_workflow` or
documents the observation gap instead of pretending unified control.

### Packaging And Call Surfaces

The resident `harness serve` process owns store, event stream, read model, and
MCP server. Provider-specific plugins are thin host-native packages over it.

The call surface split remains:

| Layer | Role | Why |
| --- | --- | --- |
| Plugin | distribution | packaging and install only |
| MCP | primary call surface | machine-readable tool schema and native host approval UX |
| Skill | teaches method | when to form a team, how to split waves, delivery contracts |
| CLI | plumbing and fallback | executable body for debug, monitors, CI |
| Hook | observation nerves | event-triggered injection and light interception, never the primary call surface |

### Thinking Policy

The accepted target is that thinking is not part of the durable Agent Team
object model.

- A provider may expose transient live reasoning to the host UI.
- The harness may surface that live-only signal when available.
- It is never persisted as `MemberAction`, `TeamRunEvent`, or canonical
  evidence.
- It is never replayed or forwarded to other members.

Persist Harness-owned control and coordination facts, artifact/check
references, blockers, handoffs, and explicit outcomes instead. Provider-native
activity remains readable through the member's native session binding.

New Kimi adapter writes do not append provider reasoning as durable
`MemberAction(type=thinking)` rows. Active stores are cleaned rather than
retaining those rows as a compatibility contract. The Console receives a sanitized `member_activity` preview
only through project-scoped SSE: it carries an expiry, is never tailed from a
ledger, never appears in a snapshot, and is not replayed after reconnect.

Provider adapters may normalize native events in memory for live display, but
must not persist a second event stream. A member handoff is durable only when
it is explicitly promoted into a Harness `TeamMessage`/outcome; ordinary final
assistant text remains part of the native session.

Provider model names are execution constraints, not cosmetic metadata. Codex
maps a requested member model to `codex exec -m`; Kimi maps it after
`session/new` through ACP `session/set_config_option(configId=model)`. An
unavailable Kimi alias must fail before prompting rather than silently falling
back to the user's default model.

## Consequences

- ADR 0025 remains the canonical v0 substrate for the `agent_team` executor.
- Mission/Wave product hierarchy is owned by ADR 0026; retirement of the old
  coordination stack is owned by ADR 0028.
- A future dashboard or host surface should explain Agent Team ownership through
  assignment-message correlation and wave context rather than through a
  first-class legacy dependency graph concept.
- Residual internal fields such as `current_task_id` are removal debt, not an
  active product contract, ownership model, or reason to retain old UI.

## Non-goals

- Standing agent organizations, long-lived employee directories, or cross-Mission
  inboxes.
- Treating private reasoning as durable execution history or evidence.
- Claiming full observability for provider-native subagents when hooks are
  missing.
- Replacing `dynamic_workflow` or `host` with Agent Team semantics.

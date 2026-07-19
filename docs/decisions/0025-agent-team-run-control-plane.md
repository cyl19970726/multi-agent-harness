# ADR 0025: Agent Team Run Control Plane

## Status

Accepted as the v0 Agent Team substrate.

Superseded in part by ADR
[0026](0026-mission-wave-architecture.md) for top-level product hierarchy,
Mission/Wave terminology, and thinking policy. This ADR remains canonical for
the v0 Agent Team object set, delegation guardrails, and host/tooling split.

## Context

Agent Team exists for work that cannot be reduced to a one-shot function call.
A sub-agent returns a result and disappears. An Agent Team member is a living
collaborator with its own mailbox, runtime state, and responsibility lane until
the Wave gate closes.

The v0 implementation goal is not to solve the whole Mission/Wave product. It
is to prove the first real `agent_team` executor substrate:

- wave-scoped collaboration across providers;
- explicit assignment, handoff, blocker, and review messages;
- observable member actions and delegation;
- shared dashboard, CLI, and host-tool read model.

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
AgentTeamRun    id, objective, status, wave_index?, budget_limit_usd?,
                host{surface, thread_id}, member_run_ids[],
                created_at / started_at / ended_at

MemberRun       id, team_run_id, name, role, provider, model?,
                status(starting|idle|queued|running|waiting|reviewing|
                       blocked|completed|failed|stopped),
                provider_session_id?, acp_session_id?, current_task_id?,
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
- `MemberAction` stores explicit work facts. It does not store private
  reasoning.
- `TeamRunEvent` is a single ordered durable event log with sanitized payloads.

### Assignment-message correlation

Ownership inside an Agent Team Wave is explained by message correlation, not by
an exposed Task Graph:

```text
TeamMessage(kind=assignment)
  -> correlation_id
  -> MemberAction / blocker / handoff / review_result / delegation
  -> artifacts, checks, summaries, explicit outcome
```

This is the target proof chain for lane ownership inside the run.

The current v0 implementation is incomplete here. The automatic member
handoff reuses its assignment's `correlation_id`, but the manual CLI/API/MCP
send path creates a new correlation id and does not yet accept an existing
`correlation_id` or `causation_id`. Until that additive input lands, clients
must keep the assignment message id/correlation in the message body when they
need to express the relationship; they must not claim that progress, blocker,
or review messages are structurally correlated by the store.

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

Persist explicit actions, artifacts, summaries, blockers, and outcomes instead.

Current v0 code still appends provider reasoning as durable
`MemberAction(type=thinking)` rows. That behavior predates this policy and is a
known migration gap. New product surfaces must not treat those rows as
evidence, forward them to peers, or describe them as the final contract; the
runtime must stop writing them after the transient channel exists, as specified
by ADR 0026.

## Consequences

- ADR 0025 remains the canonical v0 substrate for the `agent_team` executor.
- Mission/Wave product hierarchy and compatibility migration are owned by ADR
  0026, not by this file.
- A future dashboard or host surface should explain Agent Team ownership through
  assignment-message correlation and wave context rather than through a
  first-class Task Graph concept.
- Current runtime fields such as `current_task_id` may remain during migration,
  but they are compatibility seams, not the preferred product explanation.

## Non-goals

- Standing agent organizations, long-lived employee directories, or cross-Mission
  inboxes.
- Treating private reasoning as durable execution history or evidence.
- Claiming full observability for provider-native subagents when hooks are
  missing.
- Replacing `dynamic_workflow` or `host` with Agent Team semantics.

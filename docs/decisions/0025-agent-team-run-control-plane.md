# ADR 0025: Agent Team Run Control Plane

```text
status: accepted historical substrate; current semantics amended by ADRs 0032,
  0034, 0044, and 0050
owner_role: architecture
canonical_for: origin of the AgentTeamRun/MemberRun control-plane boundary;
  current responsibility, lifecycle, and delivery rules live in the amending ADRs
```

## Status And Reading Rule

This ADR introduced the first real Agent Team control-plane substrate. Its old
Wave-owned run and Assignment-message proof chains are no longer product
contracts and are deliberately not reproduced here. Git history preserves the
v0 proposal; the governed failure reconstruction is in
Agent Team Shared Task List research.

Read the current contracts instead:

- [ADR 0032](0032-provider-native-session-is-execution-truth.md): the Provider
  native session is execution truth; Harness does not mirror transcript, tool,
  command, file, subagent, or thinking streams.
- [ADR 0034](0034-host-plan-waves-and-mission-teams.md): Mission is durable
  intent, Wave is versioned Host plan/judgment, and an independent AgentTeamRun
  may span several Waves.
- [ADR 0044](0044-durable-team-supervision-and-typed-mail.md): one durable
  Supervisor generation owns runtime control and delivery claims.
- [ADR 0050](0050-agent-team-work-board-and-message-boundary.md): Work owns
  responsibility and state; WorkDelivery notifies a runtime; TeamMessage is
  authored conversation only.

If this ADR conflicts with one of those documents, the amending ADR wins.

## Context

Agent Team exists for work that cannot be reduced to one function call. A
Member is an addressable, multi-turn collaborator with a durable MemberRun,
mailbox route, Workspace, provider-native session, and explicit lifecycle.
Provider-native subagents remain implementation details of the Member that
invoked them unless Harness actually creates a separate MemberRun.

The v0 milestone proved that several Provider members could be coordinated
through one Store and inspected through CLI and Dashboard. It also exposed two
modeling errors:

1. binding each run to one Wave confused Host planning with execution
   lifetime; and
2. treating an Assignment Message correlation as responsibility forced the
   Host to reconstruct a task board from conversation history.

Those errors motivated ADRs 0034 and 0050 rather than compatibility layers.

## Retained Decision

### Agent Team control-plane boundary

The current retained object boundary is:

```text
AgentTeam                         reusable team identity
AgentTeamRun                      one supervised execution space
  -> MemberRun                    addressable runtime/session binding
  -> Work                         durable responsibility and state
       -> WorkOperation           crash-atomic replay row
            -> WorkEvent          append-only semantic transition
            -> WorkDelivery       runtime notification/outbox delta
  -> TeamMessage                  authored conversation
  -> PendingInteraction           Provider turn actually paused for input
  -> control acknowledgements, explicit outcome, artifact/check references
```

Rules:

- An AgentTeamRun may be standalone or related to a Mission. It is not owned by
  one Wave, and Wave advance does not close the run or its members.
- A MemberRun is one Harness execution binding, not a Standing Agent or company
  employee identity. Its provider-native session remains the execution record.
- Work plus its WorkOperation/WorkEvent history is responsibility truth.
  WorkDelivery is transport for a Work version; neither a delivery receipt nor
  Provider completion changes Work state.
- TeamMessage carries question, answer, planning, explanation, review
  discussion, or peer coordination. `work_id` is an optional conversational
  link. Correlation and causation preserve reply lineage, not ownership.
- PendingInteraction exists only when a real Provider turn is paused for an
  answer or authorization. Ordinary Host/Member discussion remains Message.
- Harness lifecycle/activity rows record only Harness-owned coordination,
  control, outcome, and evidence facts. They never impersonate provider-native
  execution history.

There is no Assignment Message reader or ownership fallback. An Execution
Space containing the removed Agent Team Assignment rows must be archived/reset
or passed through an explicit future offline converter before current Works
commands run.

### Delegation guardrails

Two implementation classes remain distinct:

- `provider_native`: a Member invokes its Provider's native subagent. Harness
  may project honest attribution when the Provider exposes it, but does not
  claim child lifecycle control.
- Harness-created Member/child Team or Dynamic Workflow: Harness owns the
  created runtime or step and therefore enforces identity, Workspace/path,
  permission, budget, delivery, and lifecycle boundaries.

A provider capability without a verified adapter path is reported as
unsupported or unobserved. It is never promoted into a synthetic MemberRun.

### Packaging and call surfaces

The resident Harness service owns Store, read models, event stream, and MCP.
Provider plugins remain thin distribution packages.

| Layer | Role |
| --- | --- |
| Plugin | install and Provider-native packaging |
| MCP | machine-readable Host call surface |
| Skill | optional operating method, never architecture authority |
| CLI | complete plumbing, debugging, automation, and fallback |
| Hook | bounded observation/injection, never canonical state ownership |

CLI, HTTP, MCP, Dashboard, and Plugin must call the same application/store
semantics rather than each inventing a responsibility projection.

### Thinking and native execution policy

Thinking is not a durable Agent Team object. A sanitized, expiring live preview
may be shown when a Provider exposes one, but it is never persisted, replayed,
forwarded to peers, or treated as evidence. Provider transcript, tools,
commands, files, native subagents, and turn history stay in Provider-native
storage. Harness persists only its own coordination, Work state, controls,
outcomes, and evidence references.

Provider model/effort selection is an execution constraint with a real native
receipt, not display metadata. Version-specific support remains governed by the
reviewed Provider integration profile.

## Consequences

- Agent Team has a stable multi-Provider execution boundary without making
  Mission/Wave an executor container.
- The shared Works board replaces reconstruction of responsibility from mail.
- Conversation remains durable and correlated without becoming a scheduler.
- Member runtime/session lifetime remains independent from Wave advance,
  TeamRun completion, Mission closeout, or a single Provider turn.
- Dynamic Workflow and Host-native subagents remain separate executor models.

## Non-goals

- no universal Goal, Plan Gate, Task Graph, or Wave executor ownership;
- no Assignment Message compatibility path;
- no Provider transcript or thinking copy;
- no automatic semantic acceptance from Provider completion or transport
  receipt; and
- no claim of Harness lifecycle control over provider-native subagents.

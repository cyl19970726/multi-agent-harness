# Agent Runtime

This document defines the provider-neutral Agent Runtime Object Model
(A-ROM). Provider-specific files under `docs/integration/` explain how a
concrete provider implements this contract.

## Vision Link

The product needs persistent agent members that can be created, messaged,
observed, reviewed, and closed. A provider turn is useful only after the harness
can relate it to a member, task, message, evidence, proposal, and decision.

Final acceptance for this mechanism:

```text
create AgentMember
  -> start AgentRuntime
  -> send Message(kind=task)
  -> record delivery and provider session
  -> reduce provider events into harness state
  -> receive report/evidence/proposal
  -> close or recover runtime
```

## Key Questions

| Question | Runtime answer |
| --- | --- |
| Who is the agent? | `AgentMember` durable identity, role, skills, permissions, team, and workspace policy. |
| What is running? | `AgentRuntime` process/session/control endpoint and health. |
| What did the provider do? | `ProviderSession` plus `AgentEvent` stream. |
| How does a member receive work? | `MessageDelivery` maps harness messages to provider turns or native inputs. |
| What happens when busy? | Harness-owned queue policy decides enqueue, interrupt, reject, or fail. |
| How is context built? | Harness packages bounded task context, evidence refs, skill refs, and permissions per delivery. |
| How are providers swapped? | Providers implement the same interfaces and cannot own harness state. |

## A-ROM Objects

| Object | Owns | Refuses |
| --- | --- | --- |
| `AgentMember` | identity, role, prompt refs, skill refs, permission profile, team, current projections | provider transcript as identity |
| `AgentRuntime` | lifecycle, pid/socket/control endpoint, protocol and delivery health | task ownership or decisions |
| `MessageDelivery` | message to provider request correlation and terminal delivery state | hidden chat assignment |
| `ProviderSession` | one provider interaction and reproducible request/output refs | canonical task state |
| `AgentEvent` | normalized provider/runtime/hook events | raw provider-specific semantics |
| `ProviderChildThread` | provider-native subagent or child thread visibility | durable harness member identity by default |
| `PermissionProfile` | allowed tools, approval policy, sandbox, live/destructive boundaries | prompt-only safety |
| `WorkspaceRef` | cwd, worktree, branch, environment, owned paths | implicit global workspace |

## Provider Interfaces

```text
AgentProvider
  create_runtime(member, workspace, permissions)
  close_runtime(runtime)
  health(runtime)
  deliver(message, context)
  interrupt(runtime, reason)
  read_events(runtime, cursor)

MessageDelivery
  package_context(message, task, evidence_refs, skill_refs, permissions)
  send(provider_request)
  correlate_response(response_or_event)
  record_delivery(status, provider_session)

EventReducer
  provider_event -> AgentEvent
  AgentEvent -> member/task/message/proposal/evidence projections

WorkspaceProvider
  prepare_workspace(task)
  attach_branch_or_pr(task)
  inspect_changed_paths(task)
  cleanup_or_archive(task)
```

Codex, Claude Code, OpenClaw, a Permission Agent, or a future cloud provider
should implement these boundaries without changing `Goal`, `Task`, `Message`,
`Evidence`, `Proposal`, or `Decision` semantics.

## Queue And Context Policy

The harness owns delivery policy:

| Member state | Message policy |
| --- | --- |
| `idle` | deliver next eligible message |
| `running` | enqueue normal messages; allow explicit interrupt only by policy |
| `waiting_for_input` | deliver clarification or decision messages |
| `waiting_for_approval` | deliver approval decision or keep queued |
| `blocked` | queue or reassign, depending on Leader decision |
| `closed` / `error` | fail delivery and create evidence/blocker |

Provider context is ephemeral. Harness state is durable. Each delivery should
include only the bounded context needed for that turn: task objective,
acceptance criteria, relevant messages, evidence refs, skill refs, owned paths,
workspace refs, and permission profile.

## Provider-Specific Docs

Use this split:

```text
docs/agent-runtime.md        # provider-neutral A-ROM and interfaces
docs/integration/README.md   # integration rules and template
docs/integration/codex.md    # Codex implementation
docs/integration/<name>.md   # future provider implementation
```

Do not let the first provider implementation define the generic runtime.

## Invariants

1. Harness store is canonical; provider transcript is evidence.
2. Hooks and provider notifications are event inputs, not the message bus.
3. A runtime can fail while the member identity remains recoverable.
4. Provider-native subagents are visible child threads, not harness members
   unless explicitly promoted.
5. Dashboard reads normalized harness state, not raw provider state directly.

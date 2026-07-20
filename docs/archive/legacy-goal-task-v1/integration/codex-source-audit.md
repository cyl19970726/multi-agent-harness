# Codex Source Audit

This document records Codex source findings that influence the Codex provider
adapter. It is not the provider-neutral runtime contract and it is not the
canonical Codex integration design.

Use this split:

```text
docs/agent-runtime.md              # provider-neutral A-ROM and interfaces
docs/integration/codex.md          # canonical Codex provider integration
docs/integration/codex-source-audit.md  # source findings and refresh notes
```

When a source finding changes a durable product decision, promote the decision
to `docs/decisions/` and update the canonical integration doc.

## Audit Questions

The Codex source audit exists to answer these questions:

| Question | Why it matters |
| --- | --- |
| What is the stable external integration layer? | Prevents us from patching provider internals that will be hard to upgrade. |
| Does Codex already expose a gateway or daemon we can reuse? | Determines whether harness needs its own Provider Gateway. |
| How do threads, turns, hooks, skills, plugins, and native subagents differ? | Prevents runtime, packaging, and provider-native collaboration from being collapsed into one feature. |
| Which provider events are lossless enough for Dashboard state? | Determines what can become `AgentEvent`, `Proposal`, `Evidence`, or report messages. |
| Which Codex state is audit evidence rather than source of truth? | Keeps harness store canonical. |

## High-Level Finding

Codex has useful integration surfaces, but no single built-in abstraction that
is the same thing as a harness `AgentMember`.

```text
Harness AgentMember
  -> Harness Provider Gateway
  -> Codex app-server transport
  -> Codex thread / turn
  -> Codex notifications, hooks, thread reads
  -> normalized harness store
```

The harness should integrate at the external protocol boundary — today the
documented headless exec-stream (`codex exec --json`, ADR 0018); the app-server
boundary audited here is the retained fallback — observe hooks and provider
state, and keep its own durable objects. It should not patch Codex internal
processors for V1.

## Source Findings

| Codex area | Finding | Harness consequence |
| --- | --- | --- |
| app-server transport | Codex routes client connections through transport events, request processors, and outbound notifications. | Build a provider client around the external protocol instead of automating TUI/PTY output. |
| app-server client | Codex has internal client semantics for request/response and notification streams. | `CodexProtocolClient` should preserve terminal events, assistant output, plan/diff deltas, command output, and errors. |
| app-server daemon | Codex has daemon lifecycle concerns: socket paths, pid files, managed binary, and operation locks. | Harness supervisor must track pid, socket, startup timeout, restart, and close state explicitly. |
| thread and turn processors | Work is delivered through threads and turns. | Harness `Message(kind=task)` maps to `turn/start` after a real provider thread exists. |
| rollout / thread store | Codex can read persisted thread state. | Use `thread/read` or provider state as reconciliation evidence, not as canonical harness state. |
| hooks | Codex hooks cover session, prompt, tool, permission, stop, and subagent lifecycle. | Hooks are lifecycle observers and guardrails; they can backfill evidence but cannot mark a failed app-server delivery as delivered. |
| skills | Skills instruct Codex how to operate a workflow or project. | Skills are operational guidance, not runtime state. |
| plugins | Plugins bundle skills, hooks, apps, and MCP servers. | Plugin packaging should come after CLI/API/schema contracts stabilize. |
| native subagents | Codex can spawn provider-native child agents/threads. | Treat them as `ProviderChildThread` unless explicitly promoted to harness `AgentMember`. |
| permissions / sandbox | Codex has approval, sandbox, and command policy surfaces. | Harness `PermissionProfile` must remain explicit and visible in Dashboard. |
| MCP and command tools | Codex can expose or consume structured tools. | Future project adapters should prefer structured CLI/MCP access over prompt-only instructions. |
| cloud/batch job concepts | Codex has job/item concepts for batch execution. | Useful reference, but not a replacement for harness `Goal -> TaskGraph -> Message` semantics. |

## Gateway Finding

The source audit did not find a Codex-internal "gateway" abstraction that
matches our needs. Codex has transport acceptors, request processors, daemon
helpers, and clients; the harness still needs a provider gateway of its own.

Harness Provider Gateway responsibilities:

- start, health-check, interrupt, restart, and close provider runtimes;
- manage Unix socket or later WebSocket transport;
- run the JSON-RPC state machine;
- correlate `Message` delivery with provider thread/turn ids;
- reduce notifications and hook observations into harness objects;
- reconcile with provider thread state when notifications are incomplete;
- expose provider-neutral operations to CLI/API/Dashboard.

The gateway is an adapter, not a source of truth.

## Thread And Message Finding

Codex threads and turns are provider execution state. Harness messages are the
durable communication ledger. The app-server (fallback) delivery path is — the
primary substrate is headless `codex exec --json` per ADR 0018:

```text
queued harness Message
  -> initialize
  -> thread/start or thread/resume
  -> turn/start
  -> notification stream
  -> ProviderSession + AgentEvent + report/evidence/proposal candidates
```

Important constraints:

- the harness must not invent provider thread ids;
- `turn/start` should be sent only after a real provider thread is known;
- delivery success requires terminal provider evidence or reconciliation;
- failed protocol calls should create failed provider sessions with request and
  output fixtures;
- provider transcript text can support a report but cannot replace harness
  messages or decisions.

## Hooks Finding

Hooks are necessary for timely Dashboard visibility and policy enforcement, but
they are not enough to create a persistent agent product.

Use hooks for:

- session and turn lifecycle telemetry;
- prompt envelope validation;
- command/check evidence candidates;
- stop/report backfill;
- permission and tool-use guardrails;
- provider-native subagent start/stop telemetry.

Do not use hooks as:

- the canonical message bus;
- the only runtime lifecycle mechanism;
- the only proof of task delivery;
- the owner of task graph or Leader decisions.

## Native Subagent Finding

Codex native subagents are provider-child execution units inside a provider
thread. They can be useful, but they are not the same product object as harness
agent members.

```text
Harness AgentMember
  durable identity, role, permissions, queue, task ownership, evidence, review

Codex native subagent
  provider child thread, provider role, provider status, transcript evidence
```

Dashboard should show native subagents under the owning `AgentMember` as
provider child threads. Promotion to first-class harness member must be an
explicit Leader action.

## Permission Finding

Codex provider permissions should be treated as implementation detail plus
evidence. Harness permissions must remain explicit at the harness layer because
tasks, worktrees, PR review, destructive commands, and project adapters need a
provider-neutral policy.

Minimum harness fields influenced by Codex:

- permission profile;
- workspace roots and owned paths;
- approval state;
- allowed tools or command classes;
- sandbox/network policy;
- reviewer or approver for dangerous actions.

## Refresh Protocol

Refresh this audit when any of these changes:

- Codex app-server method names, event names, or transport behavior;
- Codex hook event names or hook payloads;
- Codex skill/plugin packaging;
- Codex native subagent APIs;
- Codex permission, sandbox, or approval behavior;
- harness provider gateway implementation.

Every refresh should end with one of three outcomes:

1. no canonical docs need changes;
2. update [codex.md](../../../integration/codex.md) because provider behavior changed;
3. create or update an ADR in [../decisions/](../decisions/) because the
   product decision changed.

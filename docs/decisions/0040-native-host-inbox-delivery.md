# ADR 0040: Native Host Inbox Delivery

```text
status: active
date: 2026-07-28
extends: ADR 0039 ordinary member planning and durable mailbox delivery
```

## Context

Members can send `TeamMessage` records to the reserved `host` recipient while
the Lead is working, waiting for user input, or no longer running. Durable mail
alone does not make the Lead process addressable. In particular, a Codex
Desktop task opened by the user is not owned by Harness merely because its
thread id is known.

The product must therefore distinguish:

1. control-plane receipt by Harness;
2. delivery into the exact provider-native Host task;
3. Host intake/transport acknowledgement;
4. a semantic answer, decision, or acceptance.

Collapsing these steps would make an Inbox row look like proof that the Host
model saw or acted on it.

## Decision

### Exact native Host binding

Every addressable TeamRun binds the Lead using the existing pair:

```text
AgentTeamRun.host_surface
AgentTeamRun.host_thread_id
```

The pair identifies one provider-native Host task. `host_thread_id` is not a
display label and must not be guessed from the current project. Hooks, MCP
clients, and Dashboard projections may aggregate Host mail only across runs
whose pair exactly matches the calling task.

The CLI supports:

```bash
firm team-run create ... \
  --host-surface codex-app \
  --host-thread-id <native-session-id>

firm team-run bind-host --id <run> \
  --surface codex-app \
  --thread-id <native-session-id>

firm team-run host-inbox \
  --surface codex-app \
  --thread-id <native-session-id> \
  --json
```

Changing the binding is an append-only TeamRun revision guarded by
compare-and-append. It cannot overwrite a concurrent lifecycle or membership
change.

### Busy, idle, and offline are delivery capabilities

They are not a new universal Host state machine:

| Situation | Real capability | Harness behavior |
| --- | --- | --- |
| Host has an active turn | A normal message must not interrupt it | Keep durable mail; deliver at the provider's next safe boundary |
| Codex Host reaches `Stop` | The current hook may continue the same native task | Inject a bounded continuation once; never loop on `stop_hook_active` |
| Host task is open but idle and Harness owns a live provider connection | The adapter may start a new native turn and wait for its terminal receipt | Deliver through that connection; mark intake only after the adapter receipt |
| Host task is open but Harness does not own its connection (current Codex Desktop plugin case) | No background callback exists | Keep mail actionable; surface it at `UserPromptSubmit` or the next `SessionStart` |
| Host is offline | No native turn can be started | Keep mail durable until the exact task resumes |

“Idle” by itself is not authority to spawn another app-server and attach to a
thread already owned by Codex Desktop. Until a supported live connection is
registered, the Desktop plugin is a safe-boundary pull adapter, not a push
daemon.

### Codex safe-boundary delivery

The Firm Codex hook reads the common hook `session_id` and only queries
the matching `codex-app` Host binding.

- `SessionStart` publishes the binding instructions and actionable Inbox.
- `UserPromptSubmit` adds actionable mail as developer context.
- `Stop` uses Codex's native `decision: "block"` continuation semantics when
  new mail exists. The continuation prompt contains bounded message metadata,
  the correlation, and exact read/ACK commands.
- A `Stop` payload with `stop_hook_active=true` never continues again.

Reading does not ACK. The Host ACKs only after the message has entered its
working context. ACK remains transport intake, not semantic acceptance.

### Provider-neutral contract

Future Claude, Kimi, or other Host adapters implement the same four facts:

```text
exact native binding
durable actionable Inbox
real safe-boundary or live-connection delivery receipt
separate semantic reply/decision
```

Provider-specific hooks and transports may differ. An adapter must report
safe-boundary pull when it cannot wake an idle task; it must not claim push
delivery by polling a ledger or by successfully writing to an unrelated
process.

## Consequences

- A Host can work without mid-turn interruption and still receive Member mail
  before the Codex task stops.
- Multiple Host tasks in one project no longer consume each other's Inbox.
- Offline mail is not lost, but idle Desktop tasks are not falsely advertised
  as asynchronously wakeable.
- `delivered + manual_ack` still means Harness accepted Member-to-Host mail.
  The explicit Host ACK proves intake; a correlated reply or decision proves
  semantics.
- A later managed Host Gateway may add real idle push without changing
  `TeamMessage` or Agent Team relations.

## Acceptance

1. Exact surface/thread filtering returns only the bound runs.
2. A Member-to-Host message remains actionable until explicit Host ACK.
3. Codex `Stop` continues once with bounded mail and never loops.
4. Member hooks never receive the Lead Inbox.
5. Missing native identity fails open and does not scan every active TeamRun.
6. CLI, HTTP, and MCP expose the same exact-binding aggregate read.
7. Documentation never claims an unowned Codex Desktop or Claude Code session
   can be woken in the background.

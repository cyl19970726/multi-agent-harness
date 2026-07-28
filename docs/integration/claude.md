# Claude Integration

This document defines the Claude-specific implementation of Star Harness Agent
Team members. Provider-neutral runtime contracts live in
[agent-runtime.md](../agent-runtime.md); native session ownership lives in
[native-session-storage.md](native-session-storage.md).

## Mode boundary

Claude has one Agent Team execution mode:

| Product surface | Mode | Lifecycle |
| --- | --- | --- |
| Agent Team Member | `claude_agent_sdk` | persistent streaming mailbox and native session |
| Dynamic Workflow / bounded execution | `claude_cli` | one-shot `claude -p` process |
| Historical Team records | `claude_cli` | readable, never startable |

`claude_agent_sdk` is both the default and the only accepted mode for a new
Claude Team member:

```bash
--member "Builder:Feature owner:claude"
--member "Builder:Feature owner:claude/agent-sdk"
```

`claude/cli` is rejected on Team creation and add-member. Missing Node, runner,
or Agent SDK dependencies fail explicitly; Harness never silently falls back
to a one-shot mode.

This boundary matches Codex:

```text
Agent Team     Codex app-server       Claude Agent SDK streaming
Workflow       codex exec             claude -p
```

## Runtime shape

The runner lives in `apps/claude-member-runner/`. Rust starts one Node process
per MemberRun and exchanges NDJSON control frames over stdio.

```text
Harness Host process
  ├─ durable Mission / Wave / TeamMessage / MemberRun
  ├─ process-local live-control registry
  └─ Claude member runner
       ├─ Agent SDK query with AsyncIterable mailbox
       ├─ provider-native session
       └─ ~/.claude/projects/**/<session>.jsonl
```

Harness persists only coordination, native-session binding, explicit outcomes,
checks, artifact references, and control facts. Claude owns transcript, tool
activity, commands, file events, subagents, turn lifecycle, and reasoning.
Thinking is never copied into Harness.

The SDK streaming message shape must match the installed SDK declarations:

```ts
{
  type: "user",
  message: { role: "user", content: [...] },
  parent_tool_use_id: null
}
```

Do not replace this with the superficially similar `{type, content}` shape.

## Host lifecycle

The Host is the Team Lead and owns the Member runtime lifecycle:

1. Create or add a Claude MemberRun with an Assignment.
2. Start the TeamRun in the Host server process.
3. Deliver ordinary Host or peer messages through the Member mailbox.
4. Interrupt only the current SDK query when needed.
5. Continue on the same native session after interrupt.
6. Close the Member explicitly when its runtime is no longer needed.
7. Resume later only from an explicit provider-owned session id.

Public controls:

```text
team_run_add_member
team_run_send_message
team_run_status / team_run_inbox / team_run_events
team_run_interrupt_member
team_run_close_member
team_run_create(resume_native_session_id=...)
```

`Interrupt` calls `query.interrupt()`. The runner retires the current query,
then resumes the same session for subsequent mailbox input. It does not mean
“remove this Member.”

`Close` sends the runner's explicit close command. The runner emits
`member_closed` and exits. Normal mailbox idleness never closes a production
member. `HARNESS_CLAUDE_AGENT_SDK_IDLE_GRACE_MS` exists only to give
deterministic foreground integration tests a bound.

Live controls are currently process-local. A close/interrupt request must go
through the same `harness serve` or MCP Host process that started the TeamRun.
After that process exits, Harness can still reconstruct coordination and resume
the native session, but it does not pretend to own an orphaned provider
process. Starting the TeamRun in a new Host process reattaches every unclosed
Member to its recorded native session; subsequent live controls must reach that
new supervisor.

## Messages and interactions

Ordinary collaboration uses `TeamMessage`:

- Host → Member assignment or follow-up;
- Member → Host question, blocker, progress, review request, or handoff;
- Member → Member peer coordination.

Claude uses the same provider-neutral
`collaborate-as-agent-team-member` contract as Codex app-server and Kimi ACP.
The first SDK turn receives a self-contained collaboration envelope with the
TeamRun, MemberRun, Assignment correlation, roster, Inbox, peer-message, and
Host-handoff commands, so correctness does not depend on a provider-specific
Claude Skill fork. When the Star Harness Skill is also installed, it must match
that canonical contract rather than redefine mailbox semantics.

A Claude Member sends Host mail explicitly with `harness team-run send
--to host`. Harness stores it immediately in the Host Inbox as delivered mail
requiring manual ACK. This does not interrupt the Host's current turn; the Host
reads it at the next safe boundary, ACKs transport separately, and sends a
causation-linked semantic response when needed.

This repository does not claim that a Claude Code/Desktop Host session owned by
another process can be background-woken. A future Claude Host adapter must own
the live Agent SDK streaming connection before it reports idle push delivery.
Without that connection, it uses the same exact native binding and
safe-boundary pull contract as [ADR 0040](../decisions/0040-native-host-inbox-delivery.md).

Provider-paused questions and approvals use `PendingInteraction`. A provider
`completed` status alone is not proof that an answer, approval, or semantic
handoff occurred.

Claude does not expose the same content-steer primitive as Codex app-server in
this adapter. Send ordinary content as a queued TeamMessage. SDK permission or
model mutation is a provider control, not a substitute for team conversation.

## Workspace and permissions

Provider cwd resolves as:

```text
MemberRun.worktree_ref > AgentTeamRun.execution_root > project_root
```

It never resolves to `store_root`. Changing cwd changes which project
instructions, skills, plugins, and MCP configuration Claude discovers and is
therefore an execution boundary.

Team members currently run with the approved broad tool posture needed for
unattended development. `owned_paths` is a collaboration and review declaration,
not an OS containment boundary. Real isolation requires a Git worktree,
container, or other system boundary. The Host should assign disjoint worktrees
or owned paths and make integration conflicts explicit.

## Native session and Desktop

The runner binds the real `system(init).session_id`, tags the provider session
with TeamRun/MemberRun identity, and stores only a `NativeSessionRef` in
Harness. Native history is read on demand from Claude's own project session
store.

Claude Desktop does not automatically list sessions created by an external
process. A session can be opened through Claude's provider-owned import path:

```bash
open "claude://resume?session=<native_session_id>"
```

This does not change Harness storage ownership; it is only a Desktop view of
the same provider-native session.

## Capability and version governance

The current `claude_agent_sdk` profile intentionally remains
`review_required`. Deterministic runner tests cover mailbox delivery,
interrupt/resume, explicit close, and session binding, but they do not replace
a proportional live-provider canary.

Never install, upgrade, downgrade, or switch Claude Code or the Agent SDK
without explicit Human confirmation naming the candidate version. After an
approved change:

1. run `harness member providers --fail-on-review`;
2. run mode-specific deterministic tests;
3. run a proportional live canary;
4. update the reviewed-version set only when the evidence supports it.

## Validation

Repository gates:

```bash
node --test apps/claude-member-runner/test/*.test.mjs
cargo test -p harness-cli --test claude_agent_sdk_member
cargo test -p harness-cli
npx pnpm@9.15.4 acceptance:mission-wave
```

Minimum live canary:

1. create one Claude SDK MemberRun;
2. bind and resolve its native session;
3. deliver a late Host message after a completed turn;
4. interrupt a live turn and verify same-session continuation;
5. close from the Host and observe runner termination;
6. reconstruct coordination from Harness and execution from Claude storage.

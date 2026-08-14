# Claude Integration

```text
status: implementation reference; Work/WorkDelivery target pending ADR 0050
```

This document defines the Claude-specific implementation of Star Harness Agent
Team members. Provider-neutral runtime contracts live in
agent-runtime.md; native session ownership lives in
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

### Model and reasoning controls

Harness passes the neutral requested `model` and `effort` to each Agent SDK
query and records the SDK session receipt separately. The current reviewed SDK
path does not expose a service-tier receipt, so a requested service tier is
marked `unsupported` rather than silently treated as effective. Provider-native
session history remains the execution truth.

## Runtime shape

The runner lives in `apps/claude-member-runner/`. Rust starts one Node process
per MemberRun and exchanges NDJSON control frames over stdio.

```text
Harness Host process
  ├─ durable Mission / Wave / TeamMessage / MemberRun
  ├─ durable Team Supervisor lease + delivery claims
  ├─ process-local SDK control handles owned by that generation
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

1. Create or add a Claude MemberRun and create/assign its initial Work.
2. Start the TeamRun in the Host server process.
3. Deliver ordinary Host or peer messages through the Member mailbox.
4. Interrupt only the current SDK query when needed.
5. Continue on the same native session after interrupt.
6. Close the Member explicitly when its current runtime is no longer needed.
7. Reopen the same MemberRun later from its recorded provider-owned session id.
8. Deactivate only when the coordination identity must end permanently.

Public controls:

```text
team_run_add_member
team_run_send_message
team_run_status / team_run_inbox / team_run_events
team_run_interrupt_member
team_run_close_member
team_run_reopen_member
team_run_create(resume_native_session_id=...)
```

`Interrupt` calls `query.interrupt()`. The runner retires the current query,
then resumes the same session for subsequent mailbox input. It does not mean
“remove this Member.”

`Close` sends the runner's explicit close command. The runner emits
`member_closed` and exits while the MemberRun, frozen mailbox, and native
session locator remain. Reopen increments the runtime generation, starts a new
runner, and resumes that exact session. Normal mailbox idleness never closes a
production member. `HARNESS_CLAUDE_AGENT_SDK_IDLE_GRACE_MS` exists only to give
deterministic foreground integration tests a bound.

Physical SDK handles remain process-local. A close/interrupt request must route
through the lease's loopback locator to the Harness service holding the current
durable Supervisor generation. That service fences the lease again immediately
before the SDK operation. After it exits or loses its lease, Harness retains
coordination and the native-session locator but does not pretend to own an
orphaned process. Starting the TeamRun after lease expiry or release acquires a
higher generation, reattaches every unclosed Member to its recorded native
session, and owns all subsequent claims and live controls.

The owner verifies the runner/SDK stream before claiming queued mail. A failed
probe leaves mail queued and reconnects the recorded session first. Close
intent is latched durably before the runner is torn down, preventing a stale
receiver or later lease generation from resurrecting the Member.

## Messages and interactions

Responsibility uses Work assignment/claim, WorkEvent and WorkDelivery. Ordinary
conversation uses `TeamMessage` for Host/Member follow-up and peer coordination.
Blocking, submission, request changes and acceptance are Work operations; a
linked Message may explain them.

Claude uses the same provider-neutral
`collaborate-as-agent-team-member` contract as Codex app-server and Kimi ACP.
The first SDK turn receives a self-contained collaboration envelope with the
TeamRun, MemberRun, active Work/version, roster, Inbox, peer-message, and Work
submission commands, so correctness does not depend on a provider-specific
Claude Skill fork. When the Star Harness Skill is also installed, it must match
that canonical contract rather than redefine mailbox semantics.

The envelope supplies `HARNESS_BIN`, the exact Harness executable selected by
the Host. A Claude Member sends Host mail explicitly with `"$HARNESS_BIN"
team-run send
--to host`. Harness stores it immediately in the Host Inbox as delivered mail
requiring manual ACK. This does not interrupt the Host's current turn; the Host
reads it at the next safe boundary, ACKs transport separately, and sends a
causation-linked semantic response when needed.

This repository does not claim that a Claude Code/Desktop Host session owned by
another process can be background-woken. A future Claude Host adapter must own
the live Agent SDK streaming connection before it reports idle push delivery.
Without that connection, it uses the same exact native binding and
safe-boundary pull contract as [ADR 0040](../../decisions/0040-native-host-inbox-delivery.md).

Provider-paused questions and answers use correlated Messages. A provider
`completed` status alone is not proof that an answer, approval, Work
submission, or Host acceptance occurred.

A provider API failure mid-turn (HTTP 401/403/5xx, blocked egress, expired
token) is recorded as a failed `provider_error` action naming the terminal
reason and HTTP status — never as a completed Work, and no submission is
fabricated for it. The SDK's `result.subtype` stays `"success"` on such turns
(issue #293), so the runner forwards `is_error`, `terminal_reason`, and
`api_error_status` explicitly. The persistent member usually survives and stays
`idle` for the next message; a clean Host close after an error round still
produces `member_closed` rather than a transport error.

Claude does not expose the same content-steer primitive as Codex app-server in
this adapter. Send ordinary content as a queued TeamMessage. SDK permission or
model mutation is a provider control, not a substitute for team conversation.

## Native Continuation

Claude Code 2.1.139 and later exposes native `/goal`. It is a session-scoped
continuation loop: setting a goal starts work immediately, a Stop hook evaluates
the condition after each cycle, one goal may be active per session, and the
goal is restored when that session resumes. `/goal` can inspect status and
`/goal clear` ends the loop. Goal activation does not widen tool permissions.

This is a provider-native capability, not yet an adapter-wired Team contract.
The current `claude_agent_sdk` integration therefore remains `host_driven`.
Harness must not activate `/goal` and also feed the AsyncIterable mailbox as a
competing top-level driver for the same Work. Before promotion to
`provider_driven`, a mode/version canary must prove exclusive cycle ownership,
mail injection or queuing, interruption, resume, terminal reason, and
permission continuity.

The durable Work, WorkEvent/WorkDelivery, Workspace, submission, and Host
acceptance remain Harness-owned regardless of the selected driver. See
Member Continuation Model and
[Claude Code goal docs](https://code.claude.com/docs/en/goal).

## Workspace and permissions

Provider cwd resolves as:

```text
MemberRun.provider_cwd_hint > AgentTeamRun.execution_root > project_root
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
Harness. The same `system(init)` event supplies `claude_code_version`; this is
the execution-mode version stored in the Member profile and native-session
reference. An unrelated `claude --version` binary on `PATH` must not stand in
for the SDK runtime.

Native history is read on demand from Claude's own project session store. The
SDK can enumerate and resume it:

```text
listSessions({ dir: <member-cwd> })
claude --resume <native-session-id>
```

Claude Desktop does not list an externally created Agent SDK session
automatically. The provider-owned import path was live-verified on Claude Code
2.1.220 / Agent SDK 0.3.220:

```bash
open "claude://resume?session=<native-session-id>"
```

Desktop imports it as `local_<native-session-id>`. A sequential SDK resume after
import appended coherently to the same native session in the canary. Concurrent
SDK and Desktop generation was not tested, so the operating rule is strict:
Desktop is observation-only while Harness owns the Member's execution driver.
Import is always an explicit operator action; member startup never opens
Desktop automatically.

`firm member-run open-native --id <member-run-id>` performs that explicit
macOS import. `--print-only` returns the target without opening an application.
The Dashboard exposes the same provider URI only for a bound
`claude_agent_sdk` session. Harness continues storing the original SDK session
id; `local_...` is a deterministic Desktop presentation id, not another
Harness-owned transcript or lifecycle.

## Account capacity and runtime context

A reviewed adapter version does not mean the account can execute. Wave 2 proved
the gap: local auth metadata reported logged-in while the SDK returned
`403 Request not allowed`, because the Harness process had no `HTTP(S)_PROXY`
and this host's direct egress is blocked; the identical request succeeded
through the proxy (`apps/claude-member-runner/FINDINGS.md` §F).

```bash
firm member preflight --provider claude --json            # metadata only
firm member preflight --provider claude --canary --json   # a real request
```

Without `--canary` the state stays `unknown` with
`evidence_source: auth_metadata`: a credential file or env key proves a
credential exists, never that a request would succeed. The report always
includes the non-secret proxy/base-URL runtime context so a `403` is diagnosed
as missing proxy rather than mistaken for an account limit.

That precedence also governs the start guard. A recorded structured `401`/`403`
is merged into the live probe, not substituted for it: while the Harness
process has no `HTTP(S)_PROXY`, capacity stays `unknown`, the missing-proxy
diagnosis is preserved, and the member is **not** gated — the recorded
rejection is kept in `detail` as evidence. Once a proxy is configured the same
failure does implicate the credential and blocks.

Claude rate limits are never surfaced: the Agent SDK terms do not permit a
third-party product to offer claude.ai login or rate limits without prior
approval, so the snapshot's `windows` stays empty by contract. See
[provider-capacity.md](provider-capacity.md).

## Capability and version governance

The adapter remains version-specific. Deterministic runner tests cover mailbox
delivery, interrupt/resume, explicit close, execution-environment propagation,
session binding, and SDK-reported version capture. The 2026-07-28 canary proved
two Host rounds on native session
`ec91628d-a514-4d40-ae9c-7f73ecf3c40f`, correct project/store selection,
Member-to-Host conversation, Work submission, same-session continuation, and
explicit Host close.
That session is enumerable by SDK `listSessions`; Desktop visibility requires
the explicit provider-owned import described above.

Claude Code and Agent SDK maintenance follows ADR 0031's Agent-managed,
one-Provider-at-a-time update loop. Do not hot-replace an active
MemberRun/native session. After a change:

1. run `firm member providers --fail-on-review`;
2. run mode-specific deterministic tests;
3. run a proportional live canary;
4. update the reviewed-version set only when the evidence supports it.

The adapter and reviewed 2.1.220 live evidence exist independently of local
availability. With this repository's locked Agent SDK 0.3.220 dependencies
restored, the current provider audit detects Claude Code `2.1.220` and reports
`current`. A missing SDK package beside the configured runner must still report
`unavailable`; an unrelated `claude` binary must never substitute for the SDK
runtime probe.

## Validation

Repository gates:

```bash
node --test apps/claude-member-runner/test/*.test.mjs
cargo test -p firm-cli --test claude_agent_sdk_member
cargo test -p firm-cli
npx pnpm@9.15.4 acceptance:mission-wave
```

Minimum live canary:

1. create one Claude SDK MemberRun;
2. bind and resolve its native session;
3. deliver a late Host message after a completed turn;
4. interrupt a live turn and verify same-session continuation;
5. close from the Host and observe runner termination;
6. reconstruct coordination from Harness and execution from Claude storage.
7. verify `listSessions({dir})` resolves the bound id without claiming Desktop
   sidebar visibility.

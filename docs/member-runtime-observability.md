# Member Runtime Observability
This is the canonical contract for observing an `AgentMember`, Agent Team
`MemberRun`, or Workflow step without creating a second provider history.
ADR 0032 is implemented: provider-native sessions own chat, turns, tools,
commands, file activity, native children, and resume state.

## Truth model

```text
Harness coordination truth
  assignment / delivery / pending interaction / control ack
  explicit outcome / artifact / check / Host Wave decision
                     +
NativeSessionRef
  provider / execution_mode / native_session_id / locator
  provider + adapter versions / availability / resume support
                     |
Provider adapter reads native store on demand
                     |
NativeActivityProjection (ephemeral, sanitized, rebuildable)
```

Harness never persists the provider transcript, stdout/stderr, NDJSON stream,
tool lifecycle, command output, file-event stream, or reasoning as an
alternative execution record. Thinking may appear only as sanitized transient
live state and is never replayed or evidence.

## Operator questions

| Question | Authoritative signal |
| --- | --- |
| Was work assigned? | Harness Assignment message and correlation |
| Is a delivery attempt active? | latest `MessageDelivery.execution_status` |
| Is the runtime executable? | `AgentRuntimeHealth` process, endpoint, protocol, and delivery probes |
| What is the agent doing? | on-demand provider-native activity projection |
| Is input or approval required? | Harness `PendingInteraction` |
| Can execution resume? | `NativeSessionRef.supports_resume` plus availability/version checks |
| What supports the Host decision? | explicit outcome, artifact/check references, and Host Wave update/advance |

Process-alive is not execution-ready. A green runtime requires positive protocol
and delivery probes; unknown or stale layers render amber.

## Durable versus ephemeral data

Durable Harness data:

- runtime identity and health;
- TeamRun `execution_root`, optional member `worktree_ref`, and the launch-time
  `workspace_snapshot` containing actual cwd, Git HEAD/branch, and only the
  instruction/skill directory paths Harness discovered relative to that cwd;
- delivery claim, status, terminal source, and native session reference;
- assignment, handoff, blocker, review, and Host/Lead/Policy interaction;
- steer/interrupt/stop/resume request and acknowledgement;
- explicit outcome summaries, artifacts, checks, and Host Wave decisions.

Ephemeral provider projection:

- assistant messages for live viewing;
- tool/command/file activity summaries;
- token and timing telemetry when the native store exposes it;
- native child activity;
- sanitized live thinking preview.

Member Focus joins this projection on read. Its compact activity view must show
at least representative provider-native message and tool anchors alongside the
Harness Assignment/Handoff; hiding every native row behind `Full record` makes
a healthy bound Session look empty. Native rows are visibly labeled and remain
read-through projections, never Harness copies.

A missing, stale, or incompatible native session is shown honestly. The UI must
not silently substitute a Harness copy.

The workspace snapshot is path and revision metadata, not a configuration
archive. Harness does not persist instruction or skill contents, credentials,
environment dumps, provider transcript/tool streams, or thinking. Legacy rows
without these optional fields remain valid and render as unavailable.
Discovery is observational metadata: a listed root does not prove that a
particular provider version read every file below it. Provider-specific loading
behavior remains a version- and execution-mode-specific adapter claim.

## Provider adapter obligations

Every execution mode publishes a capability snapshot and implements the subset
it claims:

```text
discover_native_session(launch_receipt)
read_native_activity(ref) -> bounded projection + truncated
resume_native_session(ref, input)
steer_or_send(ref, input)
interrupt(ref, reason)
inspect_version_compatibility(ref)
```

Codex app-server, Codex exec, Kimi ACP/CLI, and Claude CLI are distinct modes.
A provider release triggers compatibility review when the observed version no
longer matches the adapter profile. Unsupported controls remain visibly
unsupported; adapters must not simulate acknowledgements.

## Interaction routing

Provider questions and permission requests cross a governance boundary and are
promoted to `PendingInteraction`. Lead may answer clarification questions;
Policy or a human authority resolves permission/destructive-action requests.
The adapter resumes or continues the same native session when supported and
records only the interaction decision and control acknowledgement in Harness.

## Dashboard behavior

Team Activity interleaves two visually distinct sources:

- Harness coordination events, durable and replayable;
- provider-native activity, labelled with provider/mode and availability.

Reconnect reloads Harness state and re-reads native activity. It does not replay
a hidden Harness provider-event ledger. Provider read errors currently render
an honest unavailable/empty state. Retry/resume/fresh-start controls remain a
planned Member Focus extension; today explicit resume is selected through the
TeamRun retry/create CLI, MCP, or HTTP input.

The Team and Member views also expose the registered project/store roots from
Workspace selection, TeamRun execution root, member worktree override, and
actual launch snapshot without conflating any of them.

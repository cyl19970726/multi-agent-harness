# Member Runtime Observability
This is the canonical contract for observing an `AgentMember` or Agent Team
`MemberRun` without creating a second provider history. Historical Workflow
steps are legacy archive evidence only.
ADR 0032 is implemented: provider-native sessions own chat, turns, tools,
commands, file activity, native children, and resume state.

## Truth model

```text
Harness coordination truth
  Work / WorkEvent / WorkDelivery / identity-first Message / CanonicalMessageDelivery
  MessageSubscription / Supervisor / transport receipt
  correlated question/reply / stable Agent route / control acknowledgement
  explicit outcome / artifact / check / Host Work decision
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
| What Work exists and who owns it? | latest Work projection plus append-only WorkEvents |
| Did a Host-pushed Work version reach the runtime? | WorkDelivery claim and `provider_received` receipt |
| Did a Member pull ready Work itself? | atomic `claimed` WorkEvent plus successful bound-runtime command result; no loopback delivery |
| Did the Member accept responsibility? | Work `claimed` or `started` event, not WorkDelivery state |
| Who owns live control? | latest active `TeamSupervisorLease` generation and owner heartbeat |
| Who sent the input? | immutable Message sender Actor plus bound AgentMember/AgentSession; never a caller-supplied display identity |
| Is a delivery attempt active? | latest queued/claim/provider-receipt/failure projection |
| Is the runtime executable? | provider-process health, endpoint, protocol, and delivery probes |
| What is the agent doing? | on-demand provider-native activity projection |
| Is input required? | unresolved correlated question `Message` |
| Can execution resume? | `NativeSessionRef.supports_resume` plus availability/version checks |
| What supports the Host decision? | explicit outcome, artifact/check references, and the Work decision trail |

Process-alive is not execution-ready. A green runtime requires positive protocol
and delivery probes; unknown or stale layers render amber.

## Durable versus ephemeral data

Durable Harness data:

- runtime identity and health;
- current Team Supervisor generation, owner locator/heartbeat, reconnect state,
  authenticated Message sender AgentMember/Session, subscriptions, delivery
  claims/provider receipts/failures, and
  canonical per-recipient `CanonicalMessageDelivery`;
- TeamRun `execution_root`, optional member `provider_cwd_hint`, and the launch-time
  `provider_environment_observation` containing actual cwd, Git HEAD/branch, and only the
  instruction/skill directory paths Harness discovered relative to that cwd;
- Work, append-only state transition, WorkDelivery, terminal source, and
  native-session reference;
- Work blocker, submission, requested changes, Host acceptance, and
  Host/Lead/Policy conversation or interaction;
- steer/interrupt/close/resume request and acknowledgement;
- explicit outcome summaries, artifacts, checks, and Host Work decisions.

Ephemeral provider projection:

- assistant messages for live viewing;
- tool/command/file activity summaries;
- token and timing telemetry when the native store exposes it;
- native child activity;
- sanitized live thinking preview.

Member Focus joins this projection on read. Its compact activity view must show
at least representative provider-native message and tool anchors alongside the
Harness Work transitions and linked conversation; hiding every native row behind `Full record` makes
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

Codex app-server, Kimi ACP, and Claude Agent SDK streaming are the executable
Agent Team modes. Codex exec, Kimi CLI, and Claude CLI describe retired
one-shot or historical records and cannot be selected as Team fallbacks. A
provider release triggers compatibility review when the observed version no
longer matches the adapter profile. Unsupported controls remain visibly
unsupported; adapters must not simulate acknowledgements.

Persistent Team adapters also prove provider transport health before delivery
claim, route cross-process controls through the current Supervisor generation,
reattach the same native session after lease rollover, and latch explicit Close
before teardown. An uncertain claim remains visible until reconciled; it is
never replayed merely because a process restarted.

Provider acceptance is mode-specific but must identify the active cycle:

- Codex: the `turn/start` response; only an explicit `turn/started` event may
  move the active turn scope. Item or terminal frames from an older interrupted
  turn are ignored for the new cycle.
- Claude: the Agent SDK's delivery receipt.
- Kimi: the first frame for the active ACP `session/prompt` (session update,
  provider request, or terminal response), identified by the prompt request id.
  ACP does not expose a separate prompt-start acknowledgement.

The adapter marks delivery at that boundary, not when the whole turn finishes.
This lets a bound Member send a Work-linked question or peer message during a
long-running turn while keeping crash recovery honest.

Provider output and Harness outcome are also distinct. The adapter never turns
final assistant text into an automatic Work submission, Message, or Host
acceptance. The Member explicitly submits the latest Work version with result
and evidence references; interim and final narration remain solely in the
provider-native session.

## Interaction routing

Provider questions are `provider_interaction_request` Messages and Lead answers
are causation-linked `provider_interaction_response` Messages. The AgentSession
permission ceiling is frozen before provider start;
in-ceiling actions proceed directly and out-of-ceiling actions fail closed.
Protected external effects still require their Company-level approval policy.
The adapter resumes or continues the same native session when supported and
records only the correlated Message decision and provider-control receipt in
Harness. It never creates a PendingInteraction ledger.

## Dashboard behavior

Team Activity interleaves two visually distinct sources:

- Harness coordination events, durable and replayable;
- provider-native activity, labelled with provider/mode and availability.

Reconnect reloads Harness state and re-reads native activity. It does not replay
a hidden Harness provider-event ledger. Provider read errors currently render
an honest unavailable/empty state. Member Focus exposes only mode-backed
controls. Interrupt stops the current turn; Close explicitly ends the Member
runtime; Resume must use the bound provider-native session. The Host can
perform the same lifecycle operations through CLI, HTTP, and Dashboard
application logic.

Team and Member views show delivery claim, provider receipt or failure, and the
semantic Work claim/start event as distinct facts. `provider_received` is a
successful transport receipt; it is not responsibility acceptance. A
stale/missing Supervisor disables live controls without hiding durable mail or
changing Member status locally.

The Team and Member views also expose the selected Execution Space, Project
Binding root, TeamRun execution root, member worktree override, and actual
launch snapshot without conflating any of them.

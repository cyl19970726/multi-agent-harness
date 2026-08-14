# Pi Integration

```text
status: implementation reference; persistent Agent Team mode implemented
provider version reviewed: 0.83.0
adapter contract: pi-rpc-v1
adapter reviewed at: 2026-08-05
```

This document defines how Star Harness integrates Pi as a persistent Agent
Team member. Provider-neutral lifecycle and mailbox semantics remain in
Agent Runtime; this file records only Pi-specific
transport, session, delivery, privacy, and capability boundaries.

Pi is the first binding of the provider-neutral Team runtime adapter
(`crates/firm-cli/src/runtime_adapter.rs`): the member loop (wake → claim →
cycle → settle) is shared, and Pi compiles the semantic intents into its RPC
primitives. Its executable capability report is published as
`runtime_capability_bindings` on the `firm member providers` report.

## Current Mode Boundary

| Executor | Mode | Status |
| --- | --- | --- |
| Agent Team Member | `pi_rpc` | implemented persistent JSONL-over-stdio mode |
| Dynamic Workflow / bounded execution | Pi print mode | outside this Team adapter |
| Historical Team record | native Pi session JSONL | readable when the recorded file is available |

New Pi MemberRuns use `pi --mode rpc`. Harness does not fall back from
`pi_rpc` to a one-shot process when persistent startup or resume fails.

## Runtime And Native Session

One live Pi MemberRun owns one child process and one native Pi session file:

```text
MemberRun + active Work or queued TeamMessage
  -> pi --mode rpc --thinking off --session-dir <managed directory>
  -> get_state
  -> set_auto_compaction(false)
  -> prompt
  -> agent_settled
  -> next Work or TeamMessage starts another prompt on the same process
  -> explicit Host Close terminates the process but retains the session file
```

Harness launches Pi with `--no-context-files` and `--no-extensions`. The
member prompt carries the Harness collaboration envelope explicitly, avoiding
implicit project instruction loading and unreviewed extension behavior.

`get_state` supplies the absolute `sessionFile`. Harness stores that path as
the MemberRun's `NativeSessionRef` with execution mode `pi_rpc`; Pi's JSONL is
the sole transcript, tool-call, and provider-turn record. Harness stores only
coordination facts, receipts, action summaries, and the native-session binding.

Resume uses `pi --session <recorded session file>`. A missing file is not
reconstructed from Harness events and does not become a synthetic transcript.

## Turn And Delivery Semantics

Pi RPC accepts a `prompt` command and emits structured lifecycle, message, and
tool events. Harness treats `agent_settled` as the terminal acknowledgement for
one provider round. A successful prompt response alone is not semantic
completion.

Ordinary Host and peer mail has a `NextRound` boundary. Messages queued while
Pi is busy remain durable in the Harness queue and wake a later provider
round; they are never compiled into Pi's native `steer` channel. Only an
explicit Steer control command (`POST /v1/team-runs/{run}/members/{member}/steer`)
compiles into current-cycle injection at the cycle control boundary. The
profile therefore reports:

```text
interaction_mode: EndRoundAndFollowUp
ordinary_message_boundary: NextRound
```

Every completed round gets a provider receipt of the form
`pi:<native-session-path>:round-<n>`. The WorkDelivery active for that prompt
is completed exactly once. TeamMessages accepted into that prompt receive the
same round receipt. A later Host follow-up creates a new round and never
rewrites the already-completed WorkDelivery receipt.

An agent message must include an explicit result report. Provider completion
does not itself prove Host acceptance, artifact validity, or review approval.

## Controls

- **Interrupt:** Harness sends Pi's `abort` RPC command for the current turn.
- **Close:** Harness terminates the owned process group and records the
  control acknowledgement. Closing retains the native session for resume.
- **Resume:** supported only from the recorded, locally available native Pi
  session after the privacy scan described below.
- **Plan and Goal:** emulated through ordinary correlated Markdown messages;
  Harness does not expose a Pi-native Plan Gate or continuation Goal.
- **Mid-turn Steer:** supported for explicit Steer control commands. The
  adapter compiles the command body into a Pi `steer` frame at the cycle
  control boundary and acknowledges the control as `steer_accepted`; ordinary
  mail never takes this path. `follow_up` (queue at Pi's native boundary) is
  implemented at the RPC level and likewise unused by ordinary mail.

## Permission Enforcement

The member's canonical permission ceiling is compiled into the spawned
process — a mapped string that never reaches launch is not enforcement:

- `read_only` → `pi --tools read,grep,find,ls`
- `workspace_write` → `pi --tools read,grep,find,ls,write,edit` — no `bash`:
  a shell escapes the workspace boundary and the adapter cannot verify it.
- `full_access` → no `--tools` flag (Pi default toolset). The profile records
  `security_enforcement_locus: none_verified` for this case instead of
  pretending an adapter boundary exists.

The MemberRun's profile snapshot carries the resolved
`security_enforcement_locus` for the ceiling actually applied.

## Thinking Privacy Contract

Harness product policy permits thinking only as sanitized transient live
state; it cannot be persisted or replayed as evidence. Pi 0.83.0 can persist
thinking content in its native session, so persistent Team sessions always
launch with:

```text
--thinking off
```

A non-`off` `reasoning_effort` request is reported as unsupported, not copied
into the effective controls. Before resuming an existing Pi session, Harness
scans every JSONL entry recursively and fails closed if it finds a content
block whose `type` is `thinking` or a `thinkingSignature` field. Ordinary
`thinking_level_change` metadata is not treated as reasoning content.

This enforcement is why the profile may truthfully report
`thinking_transient_only: true`: the adapter prevents new persisted thinking
and refuses to replay a native session that already contains it.

## Capability Snapshot

| Capability | Adapter claim |
| --- | --- |
| Persistent bidirectional transport | supported via RPC JSONL |
| Native-session discovery | `get_state.data.sessionFile` |
| Same-session multi-round work | supported |
| Ordinary mail during a busy turn | durable, delivered next round |
| Structured tool events | supported for live projection |
| Built-in tools | compiled from the permission ceiling into `--tools` (see Permission Enforcement) |
| Interrupt | supported via `abort` |
| Explicit close | supported by owned process termination |
| Resume | supported after file availability and thinking scan |
| Mid-turn steer | supported for explicit Steer control commands |
| Native boundary queue (`follow_up`) | RPC-implemented; unused by ordinary mail |
| Native queue observation | `get_state` steering/follow-up/pending-count snapshot |
| Native Plan/Goal mode | not claimed |
| Native subagent observation | not claimed |
| Background-task observation | not claimed |

Provider brand, RPC capability, adapter coverage, and product permission are
separate claims. Built-in tool availability does not authorize protected
external effects.

## Failure And Recovery Boundaries

The adapter fails explicitly when Pi cannot start, the handshake times out,
RPC emits malformed frames, the process exits, a turn never settles, the
native session disappears, or resume encounters persisted thinking. Unknown
or incomplete provider behavior is not translated into a successful Work or
message receipt.

An interrupted MemberRun may be reopened against the same native session when
the session passes the privacy scan. An uncertain WorkDelivery claim is
reconciled from its durable lease and receipt state; it is never blindly
replayed with a replacement receipt.

## Validation Gates

Deterministic acceptance covers:

- RPC launch arguments, including enforced `--thinking off` and the compiled
  `--tools` allowlist for the member's permission ceiling;
- Work completion followed by a queued Host follow-up on the same runtime
  over the canonical Message/Delivery path;
- two distinct round receipts without WorkDelivery conflict or disconnect,
  read from the canonical per-binding delivery records;
- TeamMessage delivery proof on the second round;
- both halves of the busy-turn contract: an ordinary Message stays in the
  Harness queue, and an explicit Steer command compiles into a native `steer`
  frame and is acknowledged;
- executable capability bindings on the provider report (steer supported,
  continuation intents and `reconcile_effect` honestly unsupported);
- native-session binding and profile truth;
- rejection of sessions containing persisted thinking.

The opt-in live canary runs real Pi 0.83.0, performs the RPC handshake and a
tool-writing prompt, waits for `agent_settled`, and verifies the native session
contains no persisted thinking. Run it with:

```bash
cargo test -p firm-cli --test pi_canary --features pi-canary -- --nocapture
```

The deterministic integration test is:

```bash
cargo test -p firm-cli --test pi_team_member -- --nocapture
```

Provider upgrades must return the adapter to `review_required` until the
mode-specific deterministic gates and a proportional live canary pass. See
[ADR 0031](../../decisions/0031-interactive-provider-modes-and-version-drift.md)
and [Native Session Storage](native-session-storage.md).

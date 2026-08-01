# Kimi ACP Agent Team runtime

```text
status: implementation reference
owner_role: provider-integration
canonical_for: Kimi persistent Agent Team execution, controls, mailbox boundary, and resume
```

This document defines Kimi's persistent Agent Team mode. The provider overview,
installation, bounded Workflow mode, native record parsing, and permission
limitations remain in [Kimi integration](kimi.md). Provider-neutral runtime
rules remain in [Agent runtime](../agent-runtime.md).

## Planning and continuation

Kimi ACP may expose native plan updates. They remain a Member-internal aid, not
a Harness Plan lifecycle:

```text
Host TeamMessage(message): "Return a Markdown plan first; do not execute"
  -> Member may use Kimi native planning in its session
  -> Member TeamMessage(message): Markdown plan
  -> Host TeamMessage(message): revise or execute
```

Kimi does not currently have a reviewed native continuation controller in this
adapter. It uses the provider-neutral `host_driven` path: Harness delivers one
eligible mailbox envelope at a time and Kimi keeps its native session as
execution truth. This is not an emulated Goal; no Harness Goal object exists.
For 0.31.0 the capability snapshot truthfully reports `goal_mode=native`, while
the separate execution driver remains `host_driven` because ACP does not yet
provide the reviewed Goal inspection and control operations Harness would need
to delegate cycle ownership.
Raw ACP plan, thought, and tool streams remain provider-native. Only ordinary
Host/Member coordination is persisted.

A provider-owned pause that actually blocks the session is a
`PendingInteraction`; ACP `completed` is not semantic Host acceptance. See
[ADR 0039](../decisions/0039-ordinary-member-planning-and-durable-mailbox-delivery.md)
and the [Member Continuation Model](../member-continuation-model.md).

## Mode and compatibility boundary

| Product surface | Mode | Status |
| --- | --- | --- |
| Agent Team Member | `kimi_acp` | persistent bidirectional Team mode |
| Dynamic Workflow / bounded execution | `kimi_exec` | one-shot `kimi -p` mode |
| Historical Team record | `kimi_exec` | readable, never startable |

Harness never silently falls back from ACP to one-shot print mode.
`ProviderCapabilities::kimi_exec()` describes bounded Workflow execution and
must not be used to infer Team capability.

The installed Kimi Code probe reports `0.31.0`. After the Human-approved
upgrade, deterministic adapter checks and a live ACP canary reviewed this
version for prompt delivery, model/reasoning selection, native-session resume,
next-round batched mail, and cooperative Interrupt. `kimi-acp-v1` therefore
reports `current` for those slices. ACP defines `session/cancel` as a JSON-RPC
notification without a request id. The first live canary incorrectly sent it
as a request and received `-32601 Method not found`; inspection of the installed
0.31.0 implementation and a corrected notification canary identified the
framing defect in Harness.

## Model and reasoning controls

Kimi ACP advertises model and configuration options per session. Harness maps
neutral `model` to ACP `model` and neutral `effort` to the advertised
`thinking` option. A successful `session/set_config_option` response is the
receipt for an explicit request. When no value was requested, the advertised
`currentValue` is the receipt for the session default.

Harness records that distinction in `MemberRun.provider_controls`; it does not
claim a separate read-back or send an invented `effort` wire field. Service
tier is `unsupported` until ACP advertises a reviewed equivalent. The locally
configured K3 alias advertises `low`, `high`, and `max`; Harness sends the
requested value through `session/set_config_option` and retains the native
receipt in the Member control snapshot.

## Runtime sequence

```text
MemberRun + correlated Assignment
  -> Kimi ACP process over stdio
  -> initialize
  -> session/new, or session/resume for a known compatible session
  -> session/load only when an older ACP server does not implement resume
  -> session/prompt for one eligible mailbox envelope
  -> explicit Host Close ends the Member runtime
```

Health is reported separately for process, ACP protocol, native session, and
mailbox delivery. Provider-native activity stays in the Kimi session; Harness
retains only the native binding and explicit coordination facts.

## Kimi 0.31 capability adoption

The upstream capability inventory is larger than the currently exposed Team
surface. Harness adopts it in layers:

| Kimi capability | Harness posture |
| --- | --- |
| `session/resume` | implemented and preferred for exact-session reattachment |
| `session/set_config_option` | implemented for model, thinking effort, and mode receipts |
| `session/update` / `session/request_permission` | implemented for transient activity and durable `PendingInteraction` routing |
| image and embedded resource prompt blocks | supported upstream; add only through a typed, bounded Member input contract rather than embedding arbitrary blobs in `TeamMessage` |
| `session/list` | supported upstream; useful next for recovery diagnostics, never for guessing which session to resume |
| ACP MCP forwarding | supported upstream; pass only explicitly approved MCP descriptors and never copy credentials into Harness state |
| native Goals and custom/background/nested agents | usable inside the Kimi Member; remain provider-native execution details until separately reviewed control/observation contracts exist |
| `session/cancel` | implemented as an ACP notification; reviewed for cooperative Interrupt in installed 0.31.0 |
| `session/close`, audio prompts, terminal reverse-RPC | unsupported upstream; Harness must not advertise them |

This lets Kimi gain native capability without expanding the Harness object
model. A new feature becomes a product control only after the exact installed
mode returns a real receipt and the permission/privacy boundary is defined.
Explicit Host Close remains available: it durably latches runtime-shutdown
intent and terminates the Harness-owned ACP process without claiming either a
native session close or conflating Close with cooperative Interrupt. Explicit
Reopen starts a higher adapter generation and resumes the recorded ACP session.

## Busy-turn delivery boundary

Kimi ACP does not currently expose a reviewed Harness mid-turn steer operation.
Its ordinary-message boundary is `next_round_batched`:

- `team-run send` first proves only durable `TeamMessage` acceptance; it does
  not prove that the active Kimi prompt absorbed it;
- messages arriving during `session/prompt` remain queued and must be delivered
  exactly once, in order, by a later prompt on the same compatible session;
- urgent scope correction remains durable ordinary mail for the next safe
  boundary. Interrupt can stop the current turn through the reviewed
  `session/cancel` notification, but it does not inject the correction into
  that turn; the queued message still belongs to the next safe provider round;
- a handoff produced before a newer correction is delivered cannot satisfy the
  newer assignment/revision chain. The corrected round needs its own receipt
  and a handoff that restates the binding.

[Issue #274](https://github.com/cyl19970726/multi-agent-harness/issues/274)
is the live dogfood trail for this contract. CLI and Dashboard label durable
acceptance separately from provider receipt. The Supervisor pumps queued mail
after the terminal prompt boundary; two corrections are rendered in order in
one next-round envelope and receive one native prompt receipt. A Handoff is
fenced while same-correlation mail remains `queued` or `claimed`, so the
pre-correction result cannot satisfy the updated work chain.

## Restart and resume

A Harness communication, adapter, permission, model/effort, or Plugin contract
change replaces the affected runtime generation; it does not replace the
Organization Agent or silently Close the Member. The Supervisor drains or
interrupts the active prompt, reconciles delivery claims, resumes the compatible
native session with `session/resume` when available, drains attach-time
`session/update` frames, and falls back to `session/load` only for an older
server that returns method-not-found. It then sends a fresh correlated canary.

Recovery distinguishes three delivery states:

- `queued`: no provider side effect exists; reconnect, then claim normally;
- `claimed`: acceptance is uncertain; expose reconciliation instead of
  replaying the content;
- `delivered` without a correlated Handoff: resume the same native session and
  ask the Member to inspect native state/workspace and complete or restate the
  work. Do not redeliver the Assignment as a new attempt.

Acceptance must repeat the busy-turn scenario after Supervisor replacement:
two ordered deferred messages reach the same MemberRun/session exactly once,
the Member produces a corrected handoff, and Host ACKs it. An incompatible
native session remains historical provider evidence while the durable Standing
Agent and Company Work continue through an explicit new session.

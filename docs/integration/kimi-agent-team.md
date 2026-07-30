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

The installed Kimi Code probe reports `0.29.1`, while `kimi-acp-v1` is reviewed
only for `0.27.0`; the adapter therefore reports `review_required`. Do not
promote 0.29.1 without protocol/schema regeneration, deterministic checks, a
proportional live canary, and explicit Human confirmation under ADR 0031.

## Model and reasoning controls

Kimi ACP advertises model and configuration options per session. Harness maps
neutral `model` to ACP `model` and neutral `effort` to the advertised
`thinking` option. A successful `session/set_config_option` response is the
receipt for an explicit request. When no value was requested, the advertised
`currentValue` is the receipt for the session default.

Harness records that distinction in `MemberRun.provider_controls`; it does not
claim a separate read-back or send an invented `effort` wire field. Service
tier is `unsupported` until ACP advertises a reviewed equivalent. A successful
setting receipt does not change the separate `review_required` compatibility
status of installed Kimi 0.29.1.

## Runtime sequence

```text
MemberRun + correlated Assignment
  -> Kimi ACP process over stdio
  -> initialize
  -> session/new or session/load
  -> session/prompt for one eligible mailbox envelope
  -> session/cancel for an explicit reviewed interrupt
  -> explicit Host Close ends the Member runtime
```

Health is reported separately for process, ACP protocol, native session, and
mailbox delivery. Provider-native activity stays in the Kimi session; Harness
retains only the native binding and explicit coordination facts.

## Busy-turn delivery boundary

Kimi ACP does not currently expose a reviewed Harness mid-turn steer operation.
Its ordinary-message boundary is `next_round_batched`:

- `team-run send` first proves only durable `TeamMessage` acceptance; it does
  not prove that the active Kimi prompt absorbed it;
- messages arriving during `session/prompt` remain queued and must be delivered
  exactly once, in order, by a later prompt on the same compatible session;
- urgent stop or scope reduction is an explicit Interrupt followed by one
  bounded queued correction, not ordinary mail mislabeled as live steer;
- a handoff produced before a newer correction is delivered cannot satisfy the
  newer assignment/revision chain. The corrected round needs its own receipt
  and a handoff that restates the binding.

[Issue #274](https://github.com/cyl19970726/multi-agent-harness/issues/274)
is the live dogfood evidence for the remaining gap. The current persistent
Supervisor polls again after a terminal prompt boundary, but CLI/Dashboard do
not yet expose the deferral clearly and the stale-handoff fence is incomplete.
Until `work-agentos-kimi-mid-turn-delivery-v1` is accepted, wait for the safe
boundary or explicitly interrupt, send one merged correction, and verify both
the terminal ACP receipt and the Member's restatement.

## Restart and resume

A Harness communication, adapter, permission, model/effort, or Plugin contract
change replaces the affected runtime generation; it does not replace the
Organization Agent or silently Close the Member. The Supervisor drains or
interrupts the active prompt, reconciles delivery claims, resumes the compatible
native session with `session/load`, and sends a fresh correlated canary.

Acceptance must repeat the busy-turn scenario after Supervisor replacement:
two ordered deferred messages reach the same MemberRun/session exactly once,
the Member produces a corrected handoff, and Host ACKs it. An incompatible
native session remains historical provider evidence while the durable Standing
Agent and Company Work continue through an explicit new session.

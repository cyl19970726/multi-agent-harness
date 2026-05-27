# 0008: Persistent Codex Agent Runtime

## Decision

The first provider integration is Codex, and the target MVP runtime is
persistent Agent Members backed by `codex app-server`, not only one-shot
`codex exec`.

Use one Codex app-server process per Agent Member in V1.

Codex does not own or poll the harness mailbox. Harness delivery is pushed by a
provider gateway: it selects the latest queued `Message` for a member, starts or
probes that member's app-server runtime, creates or resumes the provider
thread, and sends the message as a `turn/start` request.

That gateway must claim or lease the message before provider side effects. The
decision is not "call `turn/start` whenever a queued row exists"; it is
"harness owns mailbox, provider gateway safely injects claimed messages into
Codex turns."

## Consequences

Each member gets its own prompt, worktree, provider thread, runtime state, and
event stream. `codex exec` and `codex review` remain fallback paths for
one-shot work, CI smoke tests, and PR review.

Skills teach Codex how to operate in this workflow. App-server notifications
and hooks feed `AgentEvent`, `Proposal`, `Evidence`, messages, and Dashboard
updates. Plugins are deferred until CLI/API/schema contracts are stable.

This decision is not MVP-complete until:

- delivery claim/lease is atomic with latest-message selection;
- unresolved provider sessions block later normal delivery;
- closed, closing, and retired members reject delivery and runtime restart;
- the delivered turn has a stable harness envelope;
- Dashboard warnings use the same projection as the dispatcher.

See [../integration/codex.md](../integration/codex.md) and
[../integration/codex-message-delivery.md](../integration/codex-message-delivery.md)
for the message delivery contract, plus
[../integration/codex-source-audit.md](../integration/codex-source-audit.md).

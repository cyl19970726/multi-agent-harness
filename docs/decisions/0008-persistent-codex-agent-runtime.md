# 0008: Persistent Codex Agent Runtime

## Decision

The first provider integration is Codex, and the target MVP runtime is
persistent Agent Members backed by `codex app-server`, not only one-shot
`codex exec`.

Use one Codex app-server process per Agent Member in V1.

## Consequences

Each member gets its own prompt, worktree, provider thread, runtime state, and
event stream. `codex exec` and `codex review` remain fallback paths for
one-shot work, CI smoke tests, and PR review.

Skills teach Codex how to operate in this workflow. App-server notifications
and hooks feed `AgentEvent`, `Proposal`, `Evidence`, messages, and Dashboard
updates. Plugins are deferred until CLI/API/schema contracts are stable.

See [../integration/codex.md](../integration/codex.md) and
[../integration/codex-source-audit.md](../integration/codex-source-audit.md).

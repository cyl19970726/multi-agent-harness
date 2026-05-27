# 0011: Provider-Neutral Runtime Before Provider Implementations

## Decision

Codex is the first provider implementation, not the generic runtime contract.

## Consequences

The provider-neutral Agent Runtime Object Model lives in
[../agent-runtime.md](../agent-runtime.md). Provider-specific docs live under
[../integration/](../integration/).

Future providers such as Claude Code, OpenClaw, cloud agents, or Permission
Agents must implement the runtime contract without redefining core object
semantics.

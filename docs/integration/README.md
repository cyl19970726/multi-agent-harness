# Provider Integrations

This directory contains provider-specific integration documents. It should not
define the generic runtime contract. The provider-neutral contract lives in
[../agent-runtime.md](../agent-runtime.md).

To integrate a new agent, provider, or platform, start from the canonical
[Agent Integration Model](../agent-integration-model.md): it defines the three
pillars (base configuration, environment, platform adaptation), the
provider-neutral launch spec, and the step-by-step integration checklist that
produces a doc from the template below. The primary integration substrate is
headless exec-stream, per
[../decisions/0018-exec-stream-primary-substrate.md](../decisions/0018-exec-stream-primary-substrate.md).

## Vision Link

Star Harness must support Codex first — with Claude Code and Kimi now
registered as further exec-stream providers — while leaving room for others
such as OpenClaw, cloud-hosted agents, or a Permission Agent. Provider
integrations are successful only when they preserve the native
`Mission -> ordered Wave -> executor` contract and the executor's own honest
runtime records. Provider integrations must not reintroduce the retired
Goal/GoalPhase planning stack.

## Integration Boundary

```text
docs/agent-runtime.md        # provider-neutral A-ROM and interfaces
docs/integration/README.md   # provider documentation rules
docs/integration/host-agent-mcp.md
                                 # Host MCP control contract and Codex setup
docs/integration/codex.md    # Codex implementation
docs/integration/codex-message-delivery.md
                                 # Codex mailbox and turn delivery detail
docs/integration/claude.md       # Claude Code integration
docs/integration/kimi.md     # Kimi (Moonshot) integration
docs/integration/<name>.md   # future provider implementation
```

Provider docs answer how a concrete provider implements:

- runtime creation and close;
- message delivery;
- delivery claim/lease and duplicate-prevention semantics;
- event ingestion and reduction;
- queue and context constraints;
- permissions, sandbox, and approvals;
- native subagent or child-thread behavior;
- evidence, proposal, and report extraction;
- Dashboard-visible health and failure modes;
- fallback modes and unsupported capabilities.

## Provider Template

Each provider doc should answer:

```text
Provider
  capability_summary:
  runtime_model:
  message_delivery:
  claim_and_retry_model:
  event_sources:
  reducer_mapping:
  queue_policy_constraints:
  context_packaging_constraints:
  permission_model:
  workspace_model:
  native_multi_agent_features:
  evidence_and_report_extraction:
  dashboard_health_signals:
  fallback_modes:
  unsupported_or_risky_surfaces:
  validation_gates:
```

## Current And Planned Providers

| Provider | Doc | Status | Role |
| --- | --- | --- | --- |
| Host control | [host-agent-mcp.md](host-agent-mcp.md) | MCP implemented | Codex/Kimi/Claude-style Host contract; independent from the Team Member provider. |
| Codex | [codex.md](codex.md) | planned / implemented in slices | First provider: headless exec-stream primary (app-server retained as fallback design) + hooks + skills + plugin path. |
| Codex message delivery | [codex-message-delivery.md](codex-message-delivery.md) | planned / implemented in slices | Persistent member mailbox, dispatcher, queue policy, and delivery proof. |
| Codex source audit | [codex-source-audit.md](codex.md) | planned / reference | Source-level notes that support Codex integration decisions. |
| Claude Code | [claude.md](claude.md) | planned / implemented in slices | On-demand provider via claude CLI, native subagent-to-child-thread mapping. |
| Kimi (Moonshot) | [kimi.md](kimi.md) | Team Member start implemented through ACP | Current executable Agent Team member adapter. Other Kimi execution slices remain separately scoped. |
| OpenClaw / cloud agent | not yet created | idea | Future remote or cloud-hosted provider implementation. |
| Permission Agent | not yet created | idea | Future approval/safety specialist or provider-side permission service. |

Do not create empty provider docs before there is a real provider risk,
implementation plan, or integration task. Provider placeholders belong in this
README until they need their own file.

## Invariants

1. Provider docs cannot redefine core object semantics.
2. Provider transcripts and hooks are evidence sources, not harness state.
3. First-provider convenience must not become generic architecture.
4. Unsupported provider features must be explicit so Dashboard and CLI can show
   honest capability state.
5. Provider docs must distinguish target contract, implemented slices, and
   acceptance gaps. A working hot path is not the same as a gateable provider.
6. Host-provider support and Team Member-provider support are separate
   capabilities and must never be inferred from each other.

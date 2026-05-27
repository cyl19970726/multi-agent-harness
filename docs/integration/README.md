# Provider Integrations

This directory contains provider-specific integration documents. It should not
define the generic runtime contract. The provider-neutral contract lives in
[../agent-runtime.md](../agent-runtime.md).

## Vision Link

Multi-Agent Harness must support Codex first while leaving room for other
providers such as Claude Code, OpenClaw, cloud-hosted agents, or a Permission
Agent. Provider integrations are successful only when they implement the same
harness objects and workflow without changing `Goal`, `Task`, `Message`,
`Evidence`, `Proposal`, or `Decision` semantics.

## Integration Boundary

```text
docs/agent-runtime.md        # provider-neutral A-ROM and interfaces
docs/integration/README.md   # provider documentation rules
docs/integration/codex.md    # Codex implementation
docs/integration/<name>.md   # future provider implementation
```

Provider docs answer how a concrete provider implements:

- runtime creation and close;
- message delivery;
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
| Codex | [codex.md](codex.md) | planned / implemented in slices | First persistent provider, app-server + hooks + skills + plugin path. |
| Claude Code | not yet created | idea | Future provider implementation when there is concrete integration work. |
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

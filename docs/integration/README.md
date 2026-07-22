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
docs/integration/native-session-storage.md
                                 # provider-native storage/read/resume contract
docs/integration/codex.md    # Codex implementation
docs/integration/codex-message-delivery.md
                                 # Codex mailbox and turn delivery detail
docs/integration/claude.md       # Claude Code integration
docs/integration/kimi.md     # Kimi (Moonshot) integration
docs/integration/<name>.md   # future provider implementation
```

Provider docs answer how a concrete provider implements:

- each concrete execution mode (`exec`, ACP, app-server, SDK), never only the
  provider brand;
- runtime creation and close;
- message delivery;
- delivery claim/lease and duplicate-prevention semantics;
- event ingestion and reduction;
- native session discovery, read projection, availability, and resume;
- queue and context constraints;
- permissions, sandbox, and approvals;
- native subagent or child-thread behavior;
- evidence, proposal, and report extraction;
- Dashboard-visible health and failure modes;
- fallback modes and unsupported capabilities.

Every claim must distinguish four layers:

```text
provider-native capability
  -> selected execution-mode capability
  -> adapter-wired capability
  -> product policy allowed capability
```

Receiving a provider event is not proof that its semantic operation succeeded.
In particular, `tool completed`, `question answered`, and `action approved` are
separate states.

## Provider Template

Each provider doc should answer:

```text
Provider
  capability_summary:
  provider_version:
  adapter_contract_version:
  reviewed_provider_versions:
  adapter_reviewed_at:
  compatibility_status:
  execution_modes:
  selected_execution_mode:
  native_vs_adapter_capabilities:
  runtime_model:
  message_delivery:
  claim_and_retry_model:
  event_sources:
  native_session_store:
  native_session_binding:
  native_activity_projection:
  reducer_mapping:
  tool_manifest_and_special_semantics:
  reverse_rpc_methods:
  pending_interaction_routing:
  provider_vs_semantic_completion:
  cancel_interrupt_resume_close:
  queue_policy_constraints:
  context_packaging_constraints:
  permission_model:
  workspace_model:
  native_multi_agent_features:
  background_task_semantics:
  context_compaction_and_instruction_sources:
  persistence_privacy_and_redaction:
  auth_quota_and_rate_limit_failures:
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
| Codex + Kimi live acceptance | [live-agent-team-acceptance-2026-07-21.md](live-agent-team-acceptance-2026-07-21.md) | accepted live evidence | Real retry lineage, assignment/handoff correlation, interrupted-run recovery, and no durable thinking. |
| OpenClaw / cloud agent | not yet created | idea | Future remote or cloud-hosted provider implementation. |
| Permission Agent | not yet created | idea | Future approval/safety specialist or provider-side permission service. |

Do not create empty provider docs before there is a real provider risk,
implementation plan, or integration task. Provider placeholders belong in this
README until they need their own file.

## Invariants

1. Provider docs cannot redefine core object semantics.
2. Provider-native sessions are the sole truth for per-agent transcript, tool,
   command, file, turn, and resume state. Harness stores only a mode-aware
   binding plus coordination facts; hooks and native readers feed ephemeral
   projections rather than a duplicate ledger.
3. First-provider convenience must not become generic architecture.
4. Unsupported provider features must be explicit so Dashboard and CLI can show
   honest capability state.
5. Provider docs must distinguish target contract, implemented slices, and
   acceptance gaps. A working hot path is not the same as a gateable provider.
6. Host-provider support and Team Member-provider support are separate
   capabilities and must never be inferred from each other.
7. Each MemberRun snapshots a mode-specific `ProviderIntegrationProfile`.
8. Provider questions, approvals, and plan reviews become durable
   `PendingInteraction` rows. Thinking never does.
9. Unknown reverse-RPC methods fail closed and surface as adapter gaps; they
   must not be translated into successful tool completion.
10. A provider adapter must document native-store discovery, availability,
    privacy/retention, resume, missing-session behavior, and version drift in
    addition to its tool list and reverse-RPC methods.

The Host reads and resolves pending interactions with the
`team_run_resolve_interaction` MCP tool (or the equivalent CLI/API route),
passing the provider's exact option id. Authority is enforced by route: Lead
accepts `host|lead`, Human accepts `operator|human`, and Policy accepts only
`policy`. Dashboard controls therefore cannot turn a policy decision into an
ungoverned operator click.

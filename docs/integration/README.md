# Provider Integrations

This directory contains provider-specific integration documents. It should not
define the generic runtime contract. The provider-neutral contract lives in
[../agent-runtime.md](../agent-runtime.md).

To integrate a new agent, provider, or platform, start from the canonical
[Agent Integration Model](../agent-integration-model.md): it defines the three
pillars (base configuration, environment, platform adaptation), the
provider-neutral launch spec, and the step-by-step integration checklist that
produces a doc from the template below. Execution mode is selected by executor
contract. New Agent Team members use the provider's persistent, bidirectional
Team mode: Codex app-server, Kimi ACP, or Claude Agent SDK streaming. Bounded
exec/CLI modes remain Dynamic Workflow and historical-read substrates; they
are not Team fallbacks. See
[ADR 0031](../decisions/0031-interactive-provider-modes-and-version-drift.md).
The selected mode's continuous-execution contract is defined by the
[Member Continuation Model](../member-continuation-model.md).

## Vision Link

Star Harness must support Codex first — with Claude Code and Kimi now
registered as further exec-stream providers — while leaving room for others
such as OpenClaw, cloud-hosted agents, or a Permission Agent. Provider
integrations are successful only when they preserve Mission intent,
Host-plan Waves, Mission-linked independent teams, and each execution
capability's honest native records. Provider integrations must not reintroduce the retired
Goal/GoalPhase planning stack.

## Integration Boundary

```text
docs/agent-runtime.md        # provider-neutral A-ROM and interfaces
docs/integration/README.md   # provider documentation rules
docs/integration/host-agent-mcp.md
                                 # Host MCP control contract and Codex setup
docs/integration/native-session-storage.md
                                 # provider-native storage/read/resume contract
docs/integration/provider-capacity.md
                                 # capacity/auth preflight contract and truth matrix
docs/integration/codex.md    # Codex implementation
docs/integration/codex-message-delivery.md
                                 # Codex mailbox and turn delivery detail
docs/integration/claude.md       # Claude Code integration
docs/integration/kimi.md     # Kimi (Moonshot) integration
docs/integration/kimi-agent-team.md
                                 # Kimi ACP persistent Team runtime and delivery
docs/integration/<name>.md   # future provider implementation
```

Provider docs answer how a concrete provider implements:

- each concrete execution mode (`exec`, ACP, app-server, SDK), never only the
  provider brand;
- runtime creation and close;
- default execution driver and native continuation capabilities;
- continuation inspection, replace/clear, cycle boundaries, and permission
  continuity;
- message delivery;
- delivery claim/lease and duplicate-prevention semantics;
- event ingestion and reduction;
- native session discovery, read projection, availability, and resume;
- queue and context constraints;
- permissions, sandbox, and approvals;
- native subagent or child-thread behavior;
- evidence, proposal, and report extraction;
- Dashboard-visible health and failure modes;
- runtime account capacity and its reviewed evidence source, kept separate from
  adapter compatibility — see
  [provider-capacity.md](provider-capacity.md);
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
  default_execution_driver:
  native_continuation_capabilities:
  continuation_inspection_and_controls:
  continuation_permission_scope:
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
| Codex | [codex.md](codex.md) | adapter implemented; installed 0.145.0 current | `codex_app_server` is the only new Codex Team mode; bounded `codex_exec` belongs to Workflow and historical reads. |
| Codex message delivery | [codex-message-delivery.md](codex-message-delivery.md) | implemented in slices | Persistent member mailbox, dispatcher, queue policy, and delivery proof. |
| Claude Code | [claude.md](claude.md) | adapter implemented; locked SDK 0.3.220 reports Claude Code 2.1.220 current | `claude_agent_sdk` is the only new Claude Team mode and 2.1.220 is adapter-reviewed; `claude_cli` remains Workflow/historical only. |
| Kimi (Moonshot) | [kimi.md](kimi.md) · [ACP Team runtime](kimi-agent-team.md) | adapter implemented; installed 0.31.1 current for reviewed slices | `kimi_acp` is the Team mode. Prompt delivery, K3/max controls, generation-crossing same-session resume, bounded full-access receipts, next-round batched mail, and the ACP `session/cancel` notification are reviewed. |
| Provider live acceptance | [live-agent-team-acceptance-2026-07-21.md](live-agent-team-acceptance-2026-07-21.md) | accepted + blocked live evidence | Historical acceptance plus the 2026-07-30 two-pass Codex/Claude/Kimi persistent-Team matrix: Host/Peer mail, real Codex Steer/Interrupt, Kimi next-round receipts, Supervisor restart on the same native sessions, Organization projection, and explicit Close. |
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
11. One MemberRun/native session/writable Workspace has one top-level execution
    driver. An adapter must never start a Harness cycle while a provider-native
    continuation loop owns that same work.
12. Provider-native continuation is optional. Absence of Goal mode degrades to
    `host_driven`; it does not make the provider an invalid Agent Team member.
13. One latest-wins Team Supervisor generation owns a live TeamRun's provider
    transports, delivery claims, reconnect, and real controls. The adapter must
    verify transport health before claim and fence every routed operation.
14. Team mail has typed actor provenance. Delivery claim, provider receipt,
    recipient ACK, semantic response, and Host acceptance are distinct facts.
15. Explicit Close is latched and ends one runtime generation. Idle, Work
    submission, Wave/Team/Mission completion, and service restart never imply Close;
    explicit Reopen alone may resume the same MemberRun/native session, while
    Retire is permanent.

The Host reads and resolves pending interactions with the
`team_run_resolve_interaction` MCP tool (or the equivalent CLI/API route),
passing the provider's exact option id. Authority is enforced by route: Lead
accepts `host|lead`, Human accepts `operator|human`, and Policy accepts only
`policy`. Dashboard controls therefore cannot turn a policy decision into an
ungoverned operator click.

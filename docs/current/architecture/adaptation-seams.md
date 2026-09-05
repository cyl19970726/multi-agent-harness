# Provider Adaptation Seams

A positive overview of how Star Harness adapts heterogeneous coding agents
(Codex, Claude Code, Kimi Code, Pi, DeepSeek Harness). It is a reading map:
the canonical contracts stay in [agent-runtime.md](agent-runtime.md),
[agent-integration-model.md](agent-integration-model.md),
[member-continuation-model.md](member-continuation-model.md),
[provider-event-projection.md](provider-event-projection.md), and
[native-session-storage.md](../integration/native-session-storage.md); this
page only says where each seam is and why it is cut there. Names below are the
types and packages at master `875adb05`.

## 1. The one conclusion

Star Harness does not unify the five providers' execution models. It unifies
only how the outside world safely schedules, controls, observes, and recovers
them. Every public abstraction expresses a semantic intent plus a
postcondition; each provider package compiles that into its own primitives and
hands back a receipt. Provider memory, reasoning, and tool-event streams stay
on the native side (ADR 0032); Harness reads projections on demand.

The public model is derived from facts that cannot be eliminated when a coding
agent is scheduled safely, not from any one provider's API:

| Irreducible fact | Separation principle | Public object |
| --- | --- | --- |
| It has a provider-native memory | Memory vs control: the native session records what happened; `AgentSession` records whether, by whom, and with what permission it may be controlled | `AgentSession` ↔ `NativeSessionRef` |
| It executes in some live runtime | Session vs process: a vanished process is not a vanished session; resume produces a new adapter-process epoch, not a live reattach | process-local runtime handle (provider package only) |
| It can only be controlled at observable cycle boundaries | Work vs cycle: one Work spans many cycles; a provider `completed` ends one cycle | `ExecutionCycle` |
| It may start its own next cycle | Continuation vs current activity: interrupting a cycle does not forbid a future one | `NativeContinuation` projection |
| Scheduling authority is not supervision authority | A provider may own the next-cycle decision; the NodeDaemon always owns observation, isolation, interrupt, recovery, and fencing | `ExecutionDriver` + NodeDaemon supervision |
| External control can time out, lose receipts, or crash mid-effect | Text vs control: every provider effect is a durable command; an uncertain effect is reconciled, never replayed blindly | `RuntimeCommand` with phase × certainty × postcondition |

What is abstracted (Harness owns the semantics): capability declaration and
admission; launch spec and permission ceiling; lifecycle intents and receipts;
execution-cycle boundaries; native-session binding; interaction and permission
callbacks; member collaboration capability; observation projection;
conformance. What is deliberately not abstracted: provider wire vocabulary
(Thread, Turn, prompt, followup), transcripts and tool events, provider-internal
subagents and background tasks, the mechanics of native continuation, native
session file formats, and the Human ↔ Host conversation, which happens in the
provider's own surface.

## 2. Three layers

| Layer | Owns | Packages |
| --- | --- | --- |
| Outer — Harness coordination (durable, provider-neutral) | identity, Work, Message, RuntimeCommand facts and policy | `firm-core` (domain types, immutable ids), `firm-store` (canonical facts, CAS, lease, append/rebuild), `firm-application` (Role Actions and Views, typed outcomes, application ports), `firm-cli` NodeDaemon composition (`machine_authority`, `team_supervision`, `runtime_composition`) plus CLI/HTTP/Dashboard adapters, `firm-fabric` (cross-machine transport) |
| Middle — provider-neutral runtime contract (process-local) | how a persistent coding-agent runtime is opened, fed, controlled, observed, quiesced, and released, with no provider wire vocabulary | `firm-runtime-supervisor` (the single monotonic wake/claim → drive → settle loop), `firm-runtime-contract` (`cycle`, `control`, `receipt_and_terminal`, `provider_capabilities`, `collaboration_capability`, `conformance`), `firm-runtime-host` (process groups, NDJSON, timeouts), `firm-provider-events` (persisted readers → read-only v3 event records) |
| Inner — provider-native adaptation | transport, permission mapping, session, receipt and terminal observation for one provider; never writes the Store, never accepts Work, never runs a second supervisor | `firm-provider-codex` (`codex_app_server`), `firm-provider-claude` (`claude_agent_sdk`), `firm-provider-kimi` (`kimi_acp`), `firm-provider-pi` (`pi_rpc`), `firm-provider-deepseek` (`deepseek_sdk`) |

The middle layer's six abstractions (durable session binding, live runtime
handle, `ExecutionCycle` as the provider-neutral "turn", `NativeContinuation`,
`ExecutionDriver`, and NodeDaemon supervision) are laid out in
[agent-integration-model.md](agent-integration-model.md) under "The native
session projector".

## 3. Nine seams

Each row: what the outer layer owns, what the middle contract requires the
adapter to prove, and what the provider-native side supplies.

| Seam | Harness owns | Neutral contract | Provider-native counterpart |
| --- | --- | --- | --- |
| S1 Capability declaration and admission | `ProviderIntegrationProfile` snapshot per MemberRun; `review_required` is a refusal, not a warning | fourteen `SemanticCapability` values, each bound to provider + execution mode + provider version + adapter revision with a status and evidence string; `CapabilityResolver`, `AdmissionDecision`, `RuntimeBindingFence` | exact provider version, protocol, primitive availability ("provider supports" ≠ "adapter supports") |
| S2 Launch spec and permission compilation | provider-neutral `LaunchSpec`; `PermissionCeiling` frozen at AgentSession creation; explicit cwd | `prompt_ref`, `skill_refs`, `permission`, `writable_roots`, workspace, MCP, resume, `execution_driver` | sandbox/approval policy, permission mode, ACP allow, Pi argv, DSH sandbox policy; `security_enforcement_locus` says who really enforces the ceiling |
| S3 Lifecycle intents and receipts | durable `RuntimeCommand`: prepare → effect → settle; the fence is constructible only from an admitted command | `RuntimeAdapter`: `open_or_resume`, `execute_control`, `observe`, `inspect_effect`, `reconcile`, `close_runtime`, `quiesce`, `release`; closed `ControlIntent`; layered receipts (`EffectReceipt`, `QuiesceReceipt`, `MemberRuntimeCloseReceipt`) | thread/turn, session, SDK handle control primitives; process groups, stdio, runner command frames |
| S4 `ExecutionCycle` | Supervisor wake/claim → drive → settle; durable `ProviderCycleCorrelation` | `TeamRuntimeAdapter::run_cycle`: input-acceptance receipt, control injection, terminal boundary, `ExecutionCycleOutcome`; the typed-timeout and receipt-honesty narrowing is planned under SPEC-TYPED-CYCLE-OUTCOME-01 (DEV-156, not yet implemented) | one native turn / prompt / query / followup and its terminal frame |
| S5 Native session binding and recovery | `NativeSessionRef`; resume only by native id; Close keeps the session, Reopen raises the adapter-process epoch (ADR 0065) | `AgentSession` + provider-session epoch + `control_state`; `availability`, `supports_resume` verified per version | thread id, session id, session file, DSH SessionId; each provider's own JSONL store |
| S6 Interaction and permission callbacks | provider questions become correlated Messages (`ProviderInteractionRequest` / `Response`); no second permission object | `interaction_mode`, `ordinary_message_boundary`, `ProviderInteractionType`, `control_topology`; in-ceiling tool approvals auto-acknowledged idempotently, out-of-ceiling fails closed | `requestUserInput`, `request_permission`, SDK permission callbacks |
| S7 Member collaboration capability | members act on Harness only through authenticated `firm` Role Actions submitted to the current Supervisor | `CollaborationCapabilityEnvelope` bound to exact TeamRun/MemberRun/Session/Daemon/Supervisor generations; a reviewed delivery mechanism per provider proving the capability reaches the agent tool boundary | direct tool environment, SDK tool environment, ACP, RPC, DSH Cordis shell environment |
| S8 Observation projection | the runtime plane (`AgentSession.control_state`, receipts) is canonical; the session-history plane is provider-native truth read through the NodeDaemon's official readers | `ProviderNativeEventRecord` v3 with source generation, ordering key, and content availability; live callbacks are payload-less wake hints | rollout / session / wire / pi_sessions / DSH JSONL |
| S9 Conformance | one shared admission and lifecycle discipline every adapter must pass | `conformance.rs`: the fenced `RuntimeAdapter` trait, `preflight_effect` (fence plus capability-closure admission), `CompositionLifecycle` (quiesce verified before a composition swap), `OneShotDisposer`. The cycle-level rules — receipt before terminal, no stale-idle terminal, transport loss after receipt → Unknown, empty output is not success, Close only on exact owned evidence — are stated in [agent-runtime.md](agent-runtime.md) and exercised by the four-provider native-control seam test harness in `firm-cli`; their shared provider-parameterised assertions are planned under SPEC-TYPED-CYCLE-OUTCOME-01 (DEV-156) | each provider's scripted fixtures |

## 4. How the five providers fill the seams

Capability statuses as each package declares them in `capability_bindings()`
(adapter self-report, not brand capability):

| `SemanticCapability` | Codex | Claude Code | Kimi Code | Pi | DeepSeek Harness |
| --- | --- | --- | --- | --- | --- |
| open_or_resume · start_cycle · interrupt_current_cycle · observe · close_runtime | Supported | Supported | Supported | Supported | Supported |
| inject_current_cycle | Experimental (turn/steer) | Unsupported | Unsupported | Supported (steer) | Unsupported |
| queue_at_native_boundary | Unsupported | Unsupported | Unsupported | Supported (follow_up) | Unsupported |
| inspect_effect | Unsupported | Unsupported | Unsupported | Degraded | Unsupported |
| reconcile_effect | Unsupported | Unsupported | Unsupported | Unsupported | Unsupported |
| inspect / inhibit / resume_continuation | Experimental (thread/goal) | Unsupported | Unsupported | Unsupported | Unsupported |
| quiesce | Degraded | Degraded | Degraded | Supported (read-only provable; full access returns Unknown) | Degraded |
| release | Degraded | Degraded | Degraded | Supported (owned process-group disposer) | Degraded |
| permission_enforcement | Supported (provider-native policy) | Degraded (bypass is not a sandbox) | Degraded (adapter auto-approval) | Degraded (full access unverified) | Degraded (DSH sandbox policy) |

Every managed provider profile is `host_driven`; the `external_interactive`
Host profile is `user_driven`; no profile is `provider_driven` and no provider
has passed that admission gate, so the continuation controls remain
experimental at best (S1) and the one-driver invariant is enforced by the
outer layer, not by a provider. Pi additionally declares one binding outside
the shared fourteen (`observe_native_queue`, Supported).

Native session locators (S5): Codex `codex_rollout` (thread id; rollout
JSONL; `thread/resume`), Claude `claude_project_session` (session id from the
`session_bound` frame; project JSONL; resume by session id), Kimi
`kimi_code_session` (session id from `session/new`; wire JSONL;
`session/resume`, fallback `session/load`), Pi `pi_session` (session file
from `get_state`; managed root only; `--session <file>`), DeepSeek
`deepseek_harness_session` (`session_bound.sessionId`; DSH zstd JSONL read by
the official reader; resume must return the same SessionId).

## 5. Where the cost sits

Precision is concentrated in the fence, settlement, and permission seams (S1,
S3, S4, S7) and in the outer layer's durable records; observation (S8) and the
prompt/skill vocabulary are projections. The acceptance tiers in the
development loop follow the same line: a change that alters an admission,
fence, settlement, or permission decision, a durable schema, a lease or epoch
rule, or an invariant statement is a kernel-tier change; the rest is
projection-tier. See ADR 0064 (HostAttention as a delivery ledger) and
ADR 0065 (the two runtime epochs) for the two model statements this overview
relies on.

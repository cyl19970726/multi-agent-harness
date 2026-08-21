# Provider Runtime Package Crosswalk

```text
status: DEV-58 implementation crosswalk; target entries are non-operative until landed
issue: https://github.com/cyl19970726/multi-agent-harness/issues/499
starting_revision: 68fd9d33178f48d107a3a28d8580079263ff3e55
```

This crosswalk records the package-ownership migration for current Agent Team
and Host provider runtimes. It does not restore Dynamic Workflow. Historical
Workflow records remain export, verify, and restore-read evidence only.

## Execution surfaces

| Surface | Current implementation truth at the starting revision | Target owner |
| --- | --- | --- |
| Agent Team runtime | Persistent `codex_app_server`, `claude_agent_sdk`, `kimi_acp`, and `pi_rpc` bindings in `firm-cli` | provider packages implementing `firm-runtime-contract`, driven by a provider-neutral supervisor |
| Interactive Host | Provider-native Host session outside a Harness transcript ledger | Host/application composition; no Team fallback |
| Headless Host | Kimi ACP exact-session resume and Claude CLI exact-session resume in `host_binding`; Codex is rejected because read-only resume is not proven; Pi is unsupported | optional `HostRuntimeBinding` per provider, owned separately from Team lifecycle |
| Direct `/v1/agents/*` delivery | Still-active compatibility routes use the old one-shot provider registry and message-delivery ledger | explicit compatibility application port until a separate accepted retirement decision; never represented as Dynamic Workflow or a Team binding |
| Historical provider modes | `codex_exec`, `claude_cli`, `kimi_exec`, and old Pi records remain readable in profiles/native-session metadata | historical decoder only, except the exact current Host/direct-delivery call sites named above |
| Dynamic Workflow | Runtime, crate, writers, commands, API, Dashboard, plugin, Skill, and examples are retired | no target runtime owner |

The direct-delivery row is important: code movement must not silently delete a
documented current route, but its existence also does not authorize a new
executor model. The migration first isolates it from Team and Host contracts;
retirement requires its own accepted product decision and route-level
acceptance update.

## Symbol ownership

| Current source | Current responsibility | Classification | Target |
| --- | --- | --- | --- |
| `runtime_adapter_contract.rs` | provider-neutral lifecycle intents, fences, receipts, capability admission, conformance tests | current Team contract | `firm-runtime-contract` (extracted in DEV-58) |
| `runtime_adapter.rs` | Team wake/claim/cycle/settle loop plus runtime control | current Team supervisor mixed with CLI/application state | provider-facing language lives in `firm-runtime-contract`; monotonic shared round progression now lives in `firm-runtime-supervisor` over `SupervisorApplicationPort`, while the CLI composition implements the narrow durable Work/Message/Store/RuntimeCommand round port pending application-module relocation |
| `runtime_adapter_capabilities.rs` | Team runtime selector, capability report, permission compilation | current catalog plus provider policy mixed together | canonical descriptor extracted to `firm-application`; provider-specific capability reports and permission compilers remain to move |
| `provider_adapter.rs` | current provider-native control seam and standalone NodeSession control | current control contract mixed with implementations | runtime contract/application control plus provider bindings |
| `provider_adapters.rs` | old Codex/Claude/Kimi/Pi one-shot registry | current compatibility delivery and formerly the Claude Host path; historical names; unsupported Pi stubs | renamed and narrowed to the explicit `CompatibilityDeliveryBinding` registry for Codex/Claude/Kimi; Pi's unsupported effect stub deleted; Claude Host bypasses this registry through its typed Host binding |
| `codex_claude_adapters.rs` | Codex/Claude one-shot parsing, process launch, delivery, runtime-control facts | current Claude Host/direct delivery plus historical Codex paths; mixed coordination writes | provider transport/bindings; application owns durable facts |
| `provider_ephemeral.rs` | generic child-tree teardown and NDJSON runner plus unused orphan-pidfile parameters | current Host/compatibility transport; orphan registration had no caller | `firm-runtime-host` (extracted in DEV-58); CLI retains only error translation |
| `process_reaper.rs` | snapshot/read-model helpers and legacy Team tolerance | application/read-model; filename is misleading | application projection package/module |
| `codex_app_server.rs`, `codex_team_runtime.rs` | Codex native protocol and Team binding | current Team provider | app-server transport/protocol, complete Team binding, and deterministic tests extracted to `firm-provider-codex`; CLI retains a narrow application callback/error adapter |
| `claude_agent_sdk.rs`, `claude_team_runtime.rs`, `apps/claude-member-runner` | Claude Rust bridge, Team binding, Node runner | current Team provider | runtime process, reviewed runner protocol/version gate, complete Team binding, and tests extracted to `firm-provider-claude`; CLI retains a narrow application error/trait adapter; versioned Node runner asset remains in `apps/claude-member-runner` |
| `kimi_acp.rs`, `kimi_team_runtime.rs` | Kimi ACP transport and Team binding | current Team provider; ACP transport also serves current Host | ACP process/protocol, complete Team binding, and tests extracted to `firm-provider-kimi`; CLI retains a narrow application callback/error adapter and Host consumes the same provider package through its own binding |
| `pi_rpc/` | Pi RPC client and Team binding | current Team provider | RPC process/session/prompt/steer/abort, native-session validation, permission argv admission, complete Team binding, and client tests extracted to `firm-provider-pi`; CLI retains a narrow application callback/error adapter |
| `main_tests/workflow_runtime_tests.rs` and child directory | 114 source files no longer referenced by any test root after retirement | unreachable Dynamic Workflow residue | deleted in DEV-58 |
| `main_tests/codex_exec.rs` | compiled historical parser/retirement characterization | historical read/retirement evidence | move beside the historical decoder or delete when equivalent archive verification proves coverage |

## Dependency target

```text
firm-core <- firm-runtime-contract
firm-runtime-contract <- firm-runtime-supervisor
firm-runtime-contract <- firm-provider-{codex,claude,kimi,pi}
firm-core/store/runtime-contract/providers <- firm-application
firm-application/providers/transports <- firm-cli
```

Forbidden edges:

- `firm-core` to provider, Store implementation, CLI, or Node runner;
- runtime contract to provider wire vocabulary;
- provider package to authoritative Store writes or Host acceptance;
- Host binding to Team lifecycle fallback;
- historical mode to provider effects;
- any package to a restored Dynamic Workflow writer or runtime registry.

## Migration order

1. Extract the provider-neutral runtime contract and its conformance tests.
2. Delete unreachable post-retirement tests and classify every remaining
   Workflow/provider-exec reference.
3. Establish canonical provider descriptors with distinct Team, Host,
   compatibility, event-decoder, probe, and historical-read capabilities.
4. Extract the shared Team supervisor behind narrow ports.
5. Move Codex, Claude, Kimi, and Pi one at a time without version upgrades.
6. Remove the temporary CLI contract re-export and provider-native protocol
   code from `firm-cli`.
7. Run retirement, provider, repository, and clean-archive gates at one exact
   revision before independent Review.

## Completion evidence

This document describes a migration, not a shipped claim. Completion requires:

- the crate graph and source paths proving every target owner;
- catalog completeness tests proving intentional unsupported gaps;
- Dynamic Workflow retirement manifest verification;
- provider behavior and permission characterization for Team and Host paths;
- no current provider protocol implementation in `firm-cli`;
- canonical repository checks at the submitted SHA and an independent Review
  of that same SHA.

Landed DEV-58 milestones:

- `e78b533c`: extracted `firm-runtime-contract`, moved its conformance tests,
  and deleted 114 unreachable post-retirement Workflow test sources;
- `c7aec277`: established `firm-application` as the canonical four-provider
  descriptor and made Team selection/provider reporting derive from it;
- current transport slice: extracts provider-neutral process-group teardown,
  idle/wall timeouts, NDJSON collection, and stderr draining to
  `firm-runtime-host`; removes unused orphan-pidfile and live-filename arguments;
- current Codex slice: extracts the app-server process, JSON-RPC protocol,
  thread/turn/Goal control, capacity decoder, and protocol tests to
  `firm-provider-codex`; durable AgentSession/RuntimeCommand ownership remains
  above the provider package;
- current supervisor-language slice: makes capability evidence, cycle
  observations/receipts/outcomes, and provider-structured terminal failures
  owned by `firm-runtime-contract` instead of the CLI loop; provider-native
  interrupt/close plans and their executable control port now live there too,
  while RuntimeCommand preparation/settlement stays in the application layer;
- current cycle-control slice: replaces provider-visible durable steer state
  with opaque `SteerRequest` tokens and keeps admissions/API replies inside the
  supervisor. Providers can observe content and return receipts, but cannot
  settle RuntimeCommands or answer callers directly;
- current adapter-port slice: moves the executable `TeamRuntimeAdapter` trait
  itself into `firm-runtime-contract`; the CLI supervisor currently consumes
  it with its local error adapter pending the provider-package error split;
- current Kimi slice: extracts Kimi ACP process/session/prompt/cancel/close,
  binary resolution, reverse-RPC ordering, and 20 protocol tests to
  `firm-provider-kimi`. Application callback errors cross the package boundary
  with an explicit supervisor-lease-loss bit so fencing failures retain type;
- current Pi slice: extracts Pi RPC process/session/prompt/steer/abort,
  thinking-free native-session validation, flush proof, permission argv
  admission, and client tests to `firm-provider-pi`; the same structured
  callback-error bridge preserves supervisor fencing failures;
- current Claude slice: extracts the reviewed Agent SDK runner transport,
  session/version fencing, complete `TeamRuntimeAdapter`/`RuntimeAdapter`
  implementation, close lifecycle, and tests to `firm-provider-claude`.
  `firm-cli` retains only a 184-line application adapter that preserves typed
  supervisor callback errors and delegates the provider contract;
- current Codex Team slice: extracts the complete app-server Team lifecycle,
  native Goal supervision, provider reverse-request boundary, steer/interrupt,
  close/quiesce/release behavior, and deterministic tests to
  `firm-provider-codex`. `firm-cli` retains only a 182-line application adapter
  for durable provider-interaction callbacks and typed supervisor lease loss;
- current Kimi Team slice: extracts the complete ACP Team lifecycle,
  prompt/cancel/close behavior, reverse-request ordering boundary, live-event
  projection, and deterministic tests to `firm-provider-kimi`. `firm-cli`
  retains only a 177-line application adapter; the temporary provider
  `test-support` feature is removed because test-only process construction is
  once again package-local;
- current Pi Team slice: extracts the complete RPC Team lifecycle,
  prompt/steer/abort, queue observation, close/quiesce/release, provider-native
  control primitive, and flush proof to `firm-provider-pi`. `firm-cli` retains
  only a narrow application callback/error adapter; its duplicate Pi native
  control implementation is removed;
- current execution-surface slice: replaces the catalog's shared mode shape
  with distinct `TeamRuntimeBinding`, `HostRuntimeBinding`,
  `CompatibilityDeliveryBinding`, and effect-free `HistoricalProviderMode`
  types. The current `/v1/agents/*` registry is explicitly compatibility-only,
  has no Pi unsupported stub, and is no longer used by Claude headless Host;
  Kimi Host continues through its separate ACP exact-session path;
- current supervisor slice: adds the provider-neutral
  `firm-runtime-supervisor` package. It owns the one monotonic round loop and
  consumes only `SupervisorApplicationPort`; the executable composition keeps
  durable Work/Message/Store/RuntimeCommand preparation and settlement inside
  that application port. Provider packages and the supervisor package have no
  dependency on CLI or Store implementations.

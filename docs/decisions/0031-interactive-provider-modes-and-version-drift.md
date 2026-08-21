# ADR 0031 — Interactive provider modes and adapter version drift

```text
status: accepted_and_implemented
date: 2026-07-21
scope: Agent Team Member chat, steering, interruption, and provider upgrades
```

ADR [0032](0032-provider-native-session-is-execution-truth.md) additionally
requires every interactive mode to use its provider-native session store for
history and resume rather than a Harness transcript mirror.

ADR [0056](0056-correlated-message-and-session-permission-cutover.md)
supersedes this ADR's PendingInteraction routing and per-request permission
workflow. Current provider questions use correlated Messages; AgentSession
permissions are frozen before launch and out-of-ceiling callbacks fail closed.

## Context

An Agent Team Member is not only a one-shot executor. Operators and the Lead
need to send follow-up messages, steer active work, answer provider requests,
and interrupt a turn without fabricating terminal state. Providers expose
different control surfaces, and those surfaces change across releases.

Codex `exec --json` is a good non-interactive batch stream, but it cannot accept
same-turn input. Current Codex app-server exposes persistent threads,
`turn/steer`, `turn/interrupt`, streamed items, approvals, and thread resume.
Kimi ACP exposes its own session prompt, reverse requests, and cancellation
protocol. These are distinct execution modes, not interchangeable provider
labels.

## Decision

### Chat and control semantics

The shared product actions are:

| Product action | Active interactive turn | Idle interactive session | Non-interactive turn |
| --- | --- | --- | --- |
| Send message | steer current turn | start follow-up turn | queue for next round |
| Interrupt | provider interrupt, then await terminal acknowledgement | no-op | unsupported unless a real process handle is controlled |
| Stop member | interrupt active turn, close adapter runtime, then mark stopped | close runtime, then mark stopped | only after observed process termination |
| Provider question | durable PendingInteraction routed to Lead/Human/Policy | same | explicit blocker/follow-up only |

The Dashboard composer must show which result occurred: **Steered now**,
**Started follow-up**, or **Queued for next round**. An interrupt control enters
an **Interrupting** state and becomes terminal only after provider confirmation
or an explicit recovery attestation.

### Codex mode selection

- `codex_app_server` is the default and only mode for new Codex Agent Team
  Members. It provides chat,
  same-turn steer, approvals, and interrupt. Its provider thread id is the
  native-session binding. Restart-time `thread/resume` is implemented through
  an explicit resume binding; capability state remains mode/version specific.
- `codex_exec` was a bounded one-shot substrate for the now-retired Dynamic Workflow and
  legacy non-Team paths. Historical Team records that name it remain readable,
  but Harness rejects new Team creation or start attempts for that mode.
- The two modes retain separate ProviderIntegrationProfiles and acceptance
  gates because Workflow execution capability is not Team capability.
- `codex_exec` honestly reports `interaction_mode=unsupported` and
  `supports_cancel=false`; `codex_app_server` reports only the controls its
  live adapter now exercises.

### Claude mode selection

- `claude_agent_sdk` is the default and only mode for new Claude Agent Team
  Members. Streaming input owns one mailbox and native session; the Host can
  deliver later messages, call the SDK's real interrupt, close the runtime, or
  explicitly resume the provider-owned session.
- `claude_cli` (`claude -p`) was a bounded substrate for the now-retired Dynamic Workflow and a
  readable historical execution mode. Harness rejects it for new Team members
  because an empty queue ends the process and there is no live lifecycle
  control channel.
- Missing SDK runner dependencies fail explicitly. There is no fallback to
  `claude_cli`.
- The adapter remains `review_required` until a proportional live canary
  validates the installed Claude version. Deterministic runner tests do not
  update the reviewed-version set.

### Version drift governance

Every execution-mode profile records:

- detected provider version;
- adapter contract version;
- exact provider versions reviewed against that contract;
- adapter review date;
- compatibility status and explanation.

`harness member providers` probes installed versions. `--fail-on-review` is the
CI/periodic-audit gate. A new unreviewed version becomes `review_required`; it
does not silently become compatible or incompatible. Review must regenerate
provider schemas/capability snapshots and run mode-specific deterministic and
live acceptance before adding the new version to the reviewed set.

Dashboard exposes the same source-review compatibility state on MemberRun.
Provider execution is fail-closed at every start, resume, reopen, recovery,
rebind, preflight, and provider-list boundary. An installed version that is
`review_required` can run only when the selected project/execution store has an
active operational admission for the exact scoped key:

```text
(project_id, store_id,
 provider, execution_mode, provider_version, adapter_contract_version)
```

The admission is append-only and records canonical Project Binding / Execution
Space identity, actor, evidence, time, and `strict | advisory` policy. Scope is
never derived from a path hash. Moving or migrating ledger bytes does not move
authority: the destination scope needs its own admission. Both policies are
exact-key decisions and may bridge only `review_required`. `strict` records
explicit operational authorization and clears the operational review flag.
`advisory` permits execution but preserves `needs_review=true`, emits its policy
and admission source, and continues to fail `member providers --fail-on-review`.
Neither policy can excuse an unavailable
or failed version probe, a known incompatible version, an unknown adapter
contract, or a different mode/version/contract. Source-reviewed versions and
operational admissions remain separate fields and outputs: admission never
adds a version to `reviewed_provider_versions` and never claims source review.

Operators create an admission with:

```text
harness --project <project-binding-id-or-path> provider admit \
  --provider <name> \
  --execution-mode <mode> \
  --provider-version <installed-version> \
  --adapter-contract-version <contract> \
  --evidence <ref> \
  [--policy strict|advisory] [--actor <id>] [--json]
```

`--project` is a global flag and must appear before `provider`. Whenever store
resolution selects an Execution Space (through `--space`, `FIRM_SPACE`, or the
active-space marker), this command requires that flag on the current invocation.
`FIRM_PROJECT`, `ACTIVE_PROJECT`, and the space's default Project Binding are
ambient execution context, not explicit authorization for an append-only trust
decision. Omitting the flag fails before probing or writing. The resulting scope
is exactly the flag-selected Project Binding id plus the canonical selected
Execution Space id; the command never silently substitutes the space default.
When no Execution Space exists and resolution lands unambiguously on a legacy
Project Store, the existing Project Binding default remains accepted and the
scope is `project-store:<project-id>`.

`provider admit` only records an observed tuple whose refreshed compatibility
status is `review_required`. A source-reviewed `current` tuple needs no
operational admission and the command refuses it without writing a record;
unavailable, incompatible, and failed probes remain non-admittable.

The command independently probes the installed version and verifies the
registered mode and adapter contract before appending. It does not install,
build, upgrade, downgrade, or edit provider/adapter source. The default policy
is `strict`.

Revocation and supersession append terminal rows that name the one current
active predecessor and a reason. Replay validates the full causal ledger and
fails closed on duplicate ids, unknown/non-current predecessors, policy/scope
drift, forks, or invalid ordering. An idempotent command replay does not append
a duplicate row: evidence refs are treated as a sorted, deduplicated set, while
policy, actor, scope, and the exact four-part tuple must still match. The
command reports whether it created or reused the durable record. A revoked or
superseded key no longer authorizes execution;
historical evidence remains readable. Execution-space migration copies and
verifies the admission ledger bytes, but stale source scope grants no authority
in the destination.

### Agent-managed update cadence

Provider discovery and provider installation are separate operations:

- Harness may check Codex, Claude Code, and Kimi version drift at most once per
  calendar day by default. A manual diagnostic check is still allowed when a
  provider fails unexpectedly.
- Discovery never installs, upgrades, downgrades, or changes the reviewed
  version set. It only reports `current | review_required | incompatible |
  unavailable`.
- Compatibility admission is also not installation: it records an operator
  decision in the selected store and has no provider source/build side effect.
- When several releases appear during one day, propose one selected candidate
  per provider rather than chasing every intermediate release.
- Provider maintenance is Agent-managed and does not require per-version Human
  confirmation. Before changing one Provider, record its current and candidate
  versions, installation channel, adapter/mode risk, acceptance commands and a
  tested rollback path.
- Change only one Provider at a time. Never hot-replace the runtime behind an
  active MemberRun or native session; the accepted old runtime may finish, and
  the candidate applies to newly started sessions.
- Authentication, payment, license acceptance, new credentials, permission
  expansion and other protected operations still require the appropriate
  Human or policy approval.
- After installation, keep the adapter `review_required` until its
  mode-specific deterministic checks and a proportional live canary pass. Only
  then may documentation and `reviewed_provider_versions` be updated.
- Roll back on install failure, protocol/schema mismatch, deterministic
  regression or failed live canary. Preserve the failed evidence and record the
  candidate as unaccepted rather than repeatedly retrying it in active work.

The normal operating target is therefore one reviewable Provider update window
per day, not continuous or simultaneous upgrades. Agent-managed means the Host
owns this loop and evidence; it does not mean bypassing Provider security
prompts or treating an unreviewed binary as compatible.

## Consequences

- Provider name no longer determines chat or interruption capability; execution
  mode does.
- Agent Team UI can remain shared while buttons are capability-driven.
- Codex app-server is the only new Agent Team mode, not a selectable batch
  alternative or a hidden fallback from `codex_exec`.
- Release monitoring becomes reproducible and suitable for scheduled checks.
- Daily monitoring is read-only; Provider binary changes use the staged,
  Agent-managed review and rollback loop.
- Provider protocol vocabulary alone never proves Harness lifecycle control.
- Version review also covers native-store discovery/read/resume compatibility;
  a stream parser passing is not enough.
- Operational admission is a scoped exception, not a shortcut for promoting a
  provider version into source-reviewed compatibility.

## Acceptance

- installed Codex and Kimi versions probe as `current` when they match reviewed
  versions;
- a fake/new version produces `review_required` and `--fail-on-review` fails;
- an exact active admission can authorize only that `review_required` tuple;
  version, execution-mode, adapter-contract, and store mismatches still block;
- an advisory admission never authorizes unavailable, incompatible, unknown,
  or probe-failed compatibility;
- MemberRun snapshots and Dashboard expose compatibility state;
- deterministic acceptance proves Codex `turn/steer`/`turn/interrupt`,
  streamed activity, and each Kimi operation that the exact reviewed version
  actually implements. ACP `session/cancel` must be sent as a notification
  without a JSON-RPC request id, and Interrupt acceptance must be backed by the
  terminal prompt response rather than an invented cancel response;
- provider request routing is durable through PendingInteraction;
- restart-time Codex `thread/resume`/`exec resume` and Kimi
  `session/resume` (with an explicit older-server `session/load` fallback) use
  `NativeSessionRef` bindings and fail rather than silently opening a fresh
  session.

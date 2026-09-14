# ADR 0032: Provider-native session is the per-agent execution truth

## Status

Accepted and implemented as the storage, read, and resume boundary.

ADR 0050 replaces this document's Assignment/Handoff message examples with
Agent Team Work, WorkDelivery, Work submission/acceptance, and ordinary
Work-linked conversation. The provider-native session boundary remains active.
ADR 0056 replaces this document's PendingInteraction examples with correlated
provider question/reply Messages and a frozen AgentSession permission ceiling.

Amended 2026-09-15 (CORE4-N3) against the checked-out code: the availability
enum, the read-projection type name, the per-provider resume verbs, and the
thinking clause below are corrected in place. The shipped read model is
`PersistedSessionReadResponse` / `ProviderNativeEventRecord`, documented in
[docs/current/architecture/provider-event-projection.md](../current/architecture/provider-event-projection.md).

This ADR amends ADR 0010, ADR 0025, ADR 0030, and ADR 0031 where they imply
that Harness must mirror a provider transcript, tool lifecycle, command stream,
or file-event stream into durable Harness records.

## Context

Codex, Kimi, Claude Code, and similar coding agents already own a native
session store. That store contains the provider's conversation, tool calls,
turn lifecycle, and provider-specific state needed to resume the agent. Copying
the same stream into Harness creates two histories that can diverge, expands
privacy and retention scope, and makes provider upgrades look like generic
Harness schema migrations.

Harness still needs durable state above one agent: why a team exists, which
Work a member owns, who may answer a provider question, which Work version was
submitted and accepted, and what outcome and artifacts were returned. Those
are Harness facts and cannot be
delegated to a provider's private transcript.

## Decision

### Three truth layers

```text
Harness coordination truth
  AgentTeam / TeamMembership / AgentMember / TeamRun / MemberRun / AgentSession
  Work / WorkOperation / WorkEvent / WorkDelivery
  Message / MessageSubscription / CanonicalMessageDelivery
  RuntimeCommand / outcome / artifact refs / Host acceptance
                     |
Provider-native execution truth
  native session / turns / chat / tools / commands / file events / native children
```

The provider-native session is the sole source of truth for one agent's native
conversation and execution stream. Harness does not continuously copy that
stream into JSONL ledgers.

Harness is canonical for coordination and responsibility. It persists:

- flat AgentTeam membership, TeamRun/MemberRun/AgentSession identity, role,
  constraints, and lifecycle state owned by Harness;
- canonical Work responsibility, ordered operations/events, exact execution
  binding, WorkDelivery, submission, and explicit Host acceptance;
- identity-first Messages, subscriptions, correlated provider question/reply,
  and per-recipient CanonicalMessageDelivery;
- durable RuntimeCommands, control acknowledgements, explicit outcome
  summaries, check results, artifact references, and hashes.

Harness does not persist:

- the provider transcript or a second copy of provider chat;
- provider tool-call, command, file-change, token, or reasoning streams;
- provider-native child-agent history merely to populate Team Activity;
- a second provider-session event log that claims equal authority to the native
  store.

An explicit Harness outcome summary is not a transcript copy. It is a
coordination fact authored at the TeamRun boundary or deliberately cited by a
Host acceptance decision.

### Native session binding

`AgentSession` and its exact MemberRun/runtime-generation binding use the
implemented `NativeSessionRef` contract:

```text
provider
execution_mode
native_session_id
native_locator_kind
provider_version
adapter_contract_version
availability = available | stale | missing | incompatible | unknown
supports_resume
last_verified_at
parent_native_session_id?   # retry/resume lineage when the provider exposes it
```

`native_locator_kind` describes how the provider adapter resolves the session;
it need not expose a private absolute path to every caller. The binding is a
reference and compatibility snapshot, not a mirrored session body.

The retired Harness session mirror, its ledger/schema, and the former
`MemberRun` session-id fields have been removed. New adapters must bind the
mode-aware reference and must not recreate a generic mirrored session object.

### Read projection and Dashboard

The Dashboard builds a joined projection:

```text
Harness coordination records
  + provider adapter reads NativeSessionRef on demand
  -> response-local read model (ephemeral, rebuildable, non-authoritative)
```

`NativeActivityProjection` was this ADR's placeholder name. No Rust type and no
durable record was ever built for it. A same-named Dashboard TypeScript
interface does survive (`apps/agent-dashboard/src/types.ts:555`, returned by
`fetchNativeMemberActivity` at `apps/agent-dashboard/src/api.ts:273-283`), but
it is dead code: nothing in the app calls that function, and the
`/v1/member-runs/{id}/native-activity` route it fetches is retired and answers
`410 Gone` pointing at the AgentWorkspace `persisted_session_projection`
(`crates/firm-cli/src/main_modules/http_get_routes.rs:533-545`).

The shipped read model is `PersistedSessionReadResponse`, a page of
`ProviderNativeEventRecord`
(`crates/firm-node-daemon/src/daemon_protocol.rs:57-62`,
`crates/firm-provider-events/src/persisted_model.rs:257-259`); its contract is
[docs/current/architecture/provider-event-projection.md](../current/architecture/provider-event-projection.md).

The adapter may normalize native events in memory for display. A bounded cache
is allowed only when it is deletable, rebuildable, explicitly non-evidence, and
never used to resume or accept Work. Provider unavailability produces an
honest `missing`, `stale`, or `incompatible` state; it does not fall back to a
secret Harness transcript.

Team Activity therefore contains two visibly different record classes:

- durable Harness coordination records; and
- live or on-demand native provider activity, labelled with provider source and
  availability.

Thinking remains stricter than ordinary native activity: nothing durable is
written for it. No Harness store holds a reasoning record — `firm-store` never
reads or writes `ProviderNativeEventRecord` at all — and reasoning is never
replayed, forwarded as coordination, or used as evidence.

What does happen, precisely: the response-local projection is rebuilt from the
provider's own session file on every read and may carry reasoning fragments
(`SessionSemanticKind::Reasoning`) to the local Operator
(`crates/firm-provider-events/src/persisted/projector.rs:200`, `:236-237`,
`:318-319`, `:355`, `:655-659`). Team-managed Pi is launched with thinking off
and its projector emits no reasoning fragment at all, keeping any persisted
thinking block inside the untouched `native_event` (`projector.rs:470-473`).

### Resume

Resume is provider- and execution-mode-specific:

1. resolve the MemberRun's `NativeSessionRef` through its provider adapter;
2. verify provider version, adapter contract, availability, permissions, and
   workspace identity;
3. invoke that mode's verified provider-native resume operation. On this
   checkout: Codex app-server `thread/resume`
   (`crates/firm-provider-codex/src/lib.rs:319-323`); Kimi ACP `session/resume`,
   falling back to `session/load` only on a JSON-RPC method-not-found
   (`crates/firm-provider-kimi/src/lib.rs:518-533`); Claude the Agent SDK
   `resume` option (`apps/claude-member-runner/src/member-runner.mjs:157`); Pi
   `--session <file>` (`crates/firm-provider-pi/src/lib.rs:286-288`); DeepSeek
   Harness `ctx.agents.resume` with the exact `SessionId`, reached through the
   runner's injected `runtime.resume` wrapper
   (`apps/deepseek-member-runner/bin/deepseek-member-runner.mjs:25`,
   `apps/deepseek-member-runner/src/member-runner.mjs:40`). The legacy one-shot
   compatibility paths use `codex exec resume` and `--resume`;
4. record a Harness control request/acknowledgement and resume lineage, without
   copying the resumed transcript;
5. fail honestly when the native session is missing or incompatible.

A retry must explicitly choose `fresh` or `resume`. A new attempt never mutates
away the earlier TeamRun or its native session binding.

### Portability and retention

Provider cleanup may make a native session unavailable. Harness retains the
coordination history and references but must show that native detail can no
longer be opened or resumed.

A portable export is an explicit user operation, not an automatic mirror. It
must name its scope, redaction policy, encryption/retention policy, and whether
it is evidence or only an archive. Exporting thinking is prohibited.

## Migration outcome

The migration is destructive by design: obsolete local session mirrors are not
read, copied by project migration, exported as active evidence, or exposed by
compatibility endpoints. Codex, Kimi, and Claude adapters discover provider
session ids, read native activity on demand, resume through their verified
native mode, and expose compatibility/availability honestly. Harness retains
only coordination facts, correlated Message routing, explicit outcomes,
artifact/check references, delivery control state, RuntimeCommands, and Host
acceptance.

## Consequences

- Provider adapters own native-store discovery, reading, resume, compatibility,
  and missing-session behavior.
- Harness schemas become smaller and less coupled to provider event vocabularies.
- Agent Team acceptance combines Harness coordination truth with provider-native
  execution records; neither layer impersonates the other.
- Dashboard remains one coherent operator surface without becoming a second
  provider transcript database.
- Provider version review must cover native storage and resume compatibility,
  not only tool names and streaming frames.

## Acceptance

- A MemberRun can open its native provider session through a mode-aware binding.
- Codex, Kimi, and Claude each prove native-store discovery and resume or report
  an explicit unsupported/missing state.
- New runs create no Harness transcript, stdout/JSONL mirror, or provider tool /
  command / file `MemberAction` rows.
- Team Activity still shows canonical Work, Messages, delivery/control
  acknowledgements, outcomes, artifacts, and Host acceptance from Harness.
- Native activity disappears or becomes unavailable when its provider session
  cannot be read, without changing canonical Work or Host acceptance.
- No thinking enters Harness persistence, caches, exports, or evidence.

## Non-goals

- Standardizing every provider's native transcript format.
- Making provider sessions company documents or cross-member message buses.
- Inferring Work responsibility, RuntimeCommand success, or Host acceptance
  from native chat.
- Promising resume for a mode before its adapter proves it.

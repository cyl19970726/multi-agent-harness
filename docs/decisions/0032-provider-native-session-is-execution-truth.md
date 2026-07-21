# ADR 0032: Provider-native session is the per-agent execution truth

## Status

Accepted as the target storage and resume boundary.

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

Harness still needs durable state above one agent: why a team exists, what a
member was assigned, who may answer a provider question, which attempt
finished, what outcome and artifacts were returned, and which Wave attempt was
accepted. Those are Harness facts and cannot be delegated to a provider's
private transcript.

## Decision

### Three truth layers

```text
Company OS truth
  Document / WorkItem / Approval / Finance / Organization
                     |
Harness coordination truth
  Mission / Wave / AgentTeamRun / MemberRun binding
  Assignment / TeamMessage / PendingInteraction / outcome / artifact refs / gate
                     |
Provider-native execution truth
  native session / turns / chat / tools / commands / file events / native children
```

The provider-native session is the sole source of truth for one agent's native
conversation and execution stream. Harness does not continuously copy that
stream into JSONL ledgers.

Harness is canonical for coordination and responsibility. It persists:

- Mission, Wave, executor attempt, MemberRun identity, role, constraints, and
  lifecycle state owned by Harness;
- assignment, handoff, blocker, review, and cross-member/Host messages whose
  existence matters outside the provider session;
- `PendingInteraction`, because routing an Agent question or permission request
  to Lead, Policy, or Human crosses a governance boundary;
- explicit outcome summaries, check results, artifact references, hashes, and
  Wave gates;
- control requests and acknowledgements such as steer, interrupt, stop, resume,
  and recovery attestations.

Harness does not persist:

- the provider transcript or a second copy of provider chat;
- provider tool-call, command, file-change, token, or reasoning streams;
- provider-native child-agent history merely to populate Team Activity;
- a second provider-session event log that claims equal authority to the native
  store.

An explicit Harness outcome summary is not a transcript copy. It is a
coordination fact authored or accepted at the TeamRun/Wave boundary.

### Native session binding

The target `MemberRun` contract contains a `NativeSessionRef` (schema name may
be `ProviderSessionBinding` when implemented) with at least:

```text
provider
execution_mode
native_session_id
native_locator_kind
provider_version
adapter_contract_version
availability = available | stale | missing | incompatible
supports_resume
last_verified_at
parent_native_session_id?   # retry/resume lineage when the provider exposes it
```

`native_locator_kind` describes how the provider adapter resolves the session;
it need not expose a private absolute path to every caller. The binding is a
reference and compatibility snapshot, not a mirrored session body.

The current `ProviderSession` schema and `MemberRun.provider_session_id` /
`acp_session_id` fields are transitional implementation surfaces. They do not
authorize Harness-owned transcript, stdout, or JSONL copies. The implementation
must converge on one mode-aware native session binding.

### Read projection and Dashboard

The Dashboard builds a joined projection:

```text
Harness coordination records
  + provider adapter reads NativeSessionRef on demand
  -> NativeActivityProjection (ephemeral, rebuildable, non-authoritative)
```

The adapter may normalize native events in memory for display. A bounded cache
is allowed only when it is deletable, rebuildable, explicitly non-evidence, and
never used to resume or accept a Wave. Provider unavailability produces an
honest `missing`, `stale`, or `incompatible` state; it does not fall back to a
secret Harness transcript.

Team Activity therefore contains two visibly different record classes:

- durable Harness coordination records; and
- live or on-demand native provider activity, labelled with provider source and
  availability.

Thinking remains stricter than ordinary native activity: Harness does not read
it into a durable projection, persist it, replay it, forward it, or use it as
evidence. A provider may expose a sanitized transient live preview under the
existing thinking policy.

### Resume

Resume is provider- and execution-mode-specific:

1. resolve the MemberRun's `NativeSessionRef` through its provider adapter;
2. verify provider version, adapter contract, availability, permissions, and
   workspace identity;
3. invoke the provider-native resume operation (`thread/resume`,
   `session/load`, `--resume`, or the verified equivalent);
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

## Migration

The current implementation still writes provider-derived `MemberAction` and
`TeamRunEvent` rows for tool, command, file, and streamed activity, and some
paths retain transcript/stdout/JSONL references as Harness session state. These
are known migration debt, not the target contract.

Migration order:

1. add the mode-aware native session binding and provider-native readers;
2. implement and verify Codex, Kimi, and Claude resume independently;
3. make Dashboard dual-source and label native projections honestly;
4. stop all new provider-derived action/event and transcript-copy writes;
5. reduce `MemberAction`/`TeamRunEvent` to Harness-owned coordination facts;
6. remove obsolete provider-event fields, ledgers, and old local data after
   migration checks. No backward-compatibility reader is required for obsolete
   local provider-event copies.

Until steps 1-4 ship, documentation and UI must state that the current durable
activity stream is transitional and must not claim the target boundary is
implemented.

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
- Team Activity still shows assignments, interactions, handoffs, outcomes,
  control acknowledgements, artifacts, and gates from Harness.
- Native activity disappears or becomes unavailable when its provider session
  cannot be read, without changing the accepted Wave record.
- No thinking enters Harness persistence, caches, exports, or evidence.

## Non-goals

- Standardizing every provider's native transcript format.
- Making provider sessions company documents or cross-member message buses.
- Inferring assignment, approval, or Wave acceptance from native chat.
- Promising resume for a mode before its adapter proves it.

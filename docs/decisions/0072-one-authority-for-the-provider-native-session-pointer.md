# ADR 0072: One authority for the provider-native session pointer

```text
status: accepted; type unification, identity predicates and locator table implemented by the 2026-09-15 N2a slice; the authority projection is specified here and implemented by the follow-up N2b slice
date: 2026-09-15
amends: ADR 0032 native session binding; ADR 0065 two runtime epochs (unchanged, clarified)
canonical_for: which record owns the provider-native session pointer; how many NativeSessionRef types exist; where a native_locator_kind comes from
```

## Context

ADR 0032 made the provider's native session store the sole execution truth and
gave Harness one reference to it: `NativeSessionRef`. It did not say which
Harness record *owns* that reference. Three records ended up holding a copy:

| Copy | Location | Written by |
| --- | --- | --- |
| `AgentSession.native_session_ref` | trust journal | `bind_agent_session_native_session` |
| `MemberRun.native_session` | trust journal (member_run aggregate) | `bind_member_run_native_session` |
| `ProviderRuntimeProjection.native_session` | legacy `member_runs.jsonl` | `ledger.save_member_run` |

Nothing named one of them the authority, so the system compensated with a
validator whose only job was to reject disagreement
(`validate_member_session_admission_unlocked`), and the settle path wrote the
three copies in three separate Store lock acquisitions, ledger row first,
best-effort, with a documented `MEMBER_RUN_DUAL_LEDGER_COMMIT_INCOMPLETE`
caveat. Readers then made real decisions from whichever copy was nearest:
Reopen's resumability check, the detached-recovery-close postcondition, the
provider-interaction-response staleness fence, and reviewer-runtime eligibility
all read a *projection*, not the trust journal.

Worse, the "one" reference was two types. `agentfirm_api::NativeSessionRef`
required `availability` and denied unknown fields;
`team_runtime::NativeSessionRef` defaulted `availability` to `Unknown` and
accepted unknown fields. `harness_core::NativeSessionRef` resolved to the
second. A hand-written converter mapped one to the other field by field, and
carried two duplicated identity predicates plus a third, deliberately
asymmetric one — three spellings of "same session?" that could drift apart.

`native_locator_kind` had the same shape of problem. It is part of
`same_identity_as` and part of the persisted-session read fingerprint, so it is
identity, not a label. Three independent producers spelled it:

| Producer | codex | kimi | claude | deepseek | pi |
| --- | --- | --- | --- | --- | --- |
| adapter `native_locator_kind()` | `codex_rollout` | `kimi_code_session` | `claude_project_session` | `deepseek_harness_session` | `pi_session` |
| `provider_native_session_ref` | same | same | same | same | **`provider_native_session`** |
| `--resume-member` seeding | same | same | same | **`provider_native`** | **`provider_native`** |

A `--resume-member` seed for Pi therefore carried a kind no adapter produces,
so the pointer could never match the session it named.

## Decision

**One type.** `team_runtime::NativeSessionRef` and
`NativeSessionAvailability` are re-exports of the `agentfirm_api` types, not
copies. The unified type keeps `deny_unknown_fields` — every row in the real
stores satisfies it, so a foreign field is a contract break rather than a
tolerated extra — and defaults `availability` to `Unknown`, so a legacy
`member_runs.jsonl` row that omits it still decodes as honestly unknown. The
hand converter and the duplicated predicate are deleted. Exactly two
comparisons remain: `NativeSessionRef::same_identity_as`, and the one named
asymmetric admission comparison `native_session_admits_resume_seed`, which
accepts a version-less resume seed against a version-carrying observation and
rejects the reverse.

**One authority.** After an AgentSession binds a native reference,
`AgentSession.native_session_ref` is authoritative. `MemberRun.native_session`
and the `member_runs.jsonl` copy are *projections* of it: they are written from
the AgentSession value inside the same Store transaction, and a projection that
would disagree is refused rather than silently reconciled. Deciding readers
consult the AgentSession when one exists; the compare-both validators stay, but
their meaning changes from "two peers must agree" to "the projection must equal
the authority".

Before a session exists, `MemberRun.native_session` carries **requested**
semantics: the pointer a `--resume-member` seed or `resume_native_session_id`
asked for. This is the #845 pre-Open attachment path and it stays working — a
requested pointer is an intent, never an execution claim. An
`external_interactive` Host never has an AgentSession at all, so its MemberRun
pointer is never promoted to authority.

**One locator-kind table.** `harness_core::native_locator` holds one entry per
reviewed (provider, execution_mode) pair. Each adapter's
`native_locator_kind()` returns its own entry, and every seeding path reads the
same table, so a seeded pointer carries the kind its adapter will produce. An
unregistered pair resolves to `None` and the caller fails closed with a named
error instead of substituting a placeholder.

The NodeDaemon-owned Codex session adapter keeps its own `codex_thread` kind
alongside the Team-runtime `codex_rollout`: they are different adapters, and
collapsing them would silently change the identity of every row already written
under either name.

## Consequences

- One pointer type means the trust journal and the ledger projection cannot
  disagree structurally, only in value — and value disagreement is now a
  refusal, not a repair.
- ADR 0065 is unchanged. The two runtime epochs stay independent; this ADR
  decides who owns the native-session *pointer*, not who owns a generation.
- The `MEMBER_RUN_DUAL_LEDGER_COMMIT_INCOMPLETE` diagnostic remains a
  fail-closed report, not a recovery journal. Naming an authority means a
  partial write is now repairable by re-projecting from the AgentSession
  instead of requiring an operator to choose between two peers.
- Honest scope: the type unification, the two identity predicates, and the
  locator table are implemented. The authority projection and the reader
  redirection are specified above and land in the follow-up slice; until then
  the three copies are still written in separate transactions and this document
  is ahead of the checkout on that one point.

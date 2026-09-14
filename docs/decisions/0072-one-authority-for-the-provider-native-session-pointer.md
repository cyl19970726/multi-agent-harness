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

A `--resume-member` seed for Pi or DeepSeek Harness therefore carried a kind no
adapter produces, so the pointer could never match the session it named. This is
not hypothetical: 12 such objects in 8 rows of the
`dev109-final-20260827-6d71ea3e` Execution Space carry
`native_locator_kind: "provider_native"` — one `deepseek_harness` and one `pi`
MemberRun, each with `provider_version: null`, `availability: "unknown"` and
`parent_native_session_id == native_session_id`, the exact signature of the
resume seed. They are the defect's fingerprint on disk, and the shape is pinned
as a fixture.

Nothing existing is stranded by the fix, for three reasons worth stating
because they are the actual argument: no read path consults the table (all of
its call sites are write/seed paths); `native_locator_kind` stays an unvalidated
`String` on decode and `same_identity_as` compares the same six fields as
before, so those rows decode and compare exactly as they did; and every one of
the 12 sits under `native_session` and never under `native_session_ref`, so no
AgentSession ever bound them and they remain valid pre-session *requested*
intents. They were already unmatchable against their own adapters — that is the
defect, not a regression introduced here.

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
reviewed (provider, execution_mode) pair. The Team runtime adapters declare
theirs as the associated const `TeamRuntimeAdapter::NATIVE_LOCATOR`, and the
trait's provided `native_locator_kind()` returns that entry's kind — so an
adapter cannot answer with a literal, and the guarantee is in the type system
rather than in a comment. Every seeding path reads the same table, so a seeded
pointer carries the kind its adapter will produce. An unregistered pair resolves
to `None` and the caller fails closed with a named error instead of substituting
a placeholder.

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
  locator table landed in the N2a slice; the authority projection and the reader
  redirection land in N2b. Until N2b merges, the three copies are still written
  in separate transactions and this document is ahead of the checkout on that
  one point.

## How far "projection" goes for the `member_runs.jsonl` copy

`MemberRun.native_session` in the trust journal is a true projection: N2b writes
it from the AgentSession value in the same atomic ledger rewrite, through the
paired-aggregate commit.

The legacy `member_runs.jsonl` copy is **not** a derivable display projection,
and the difference is load-bearing. Those two MemberRun rows are one record in
two files, and `current_member_lifecycle_validation_mismatch_fields` fails
closed on the next read when they disagree:

```text
MEMBER_RUN_MATERIALIZATION_MISMATCH: … legacy/canonical projection differs …
for fields native_session … incomplete cross-file commit requires explicit
inspection, not automatic replay
```

Writing only the canonical half therefore makes every later admission of that
TeamRun refuse. This was not reasoned from the design; it was observed — three
trust-kernel tests failed that way when N2b first tried to leave the legacy row
to be derived later (`work_bound_before_first_open_is_claimable_after_native_session_attaches`
and two arms of `lost_work_live_requires_full_fence`).

So the decision is:

- the legacy row is appended **under the same write lock**, exactly as every
  other MemberRun writer in this Store does
  (`transition_current_team_member_lifecycle`,
  `compare_and_advance_member_run_generation`), accepting the **pre-existing**
  `MEMBER_RUN_DUAL_LEDGER_COMMIT_INCOMPLETE` caveat. A cross-file pair cannot be
  made atomic without a journal this Store deliberately does not keep, and that
  is a property of every MemberRun write here rather than something this ADR
  introduces;
- there is **no second escape hatch on the shared trust-commit primitive**: the
  two trust aggregates land in one atomic rewrite, and only the established
  dual-ledger append follows it;
- `derive_member_runs_jsonl_native_session` stays as the explicit repair verb
  for a row left stale by a failure between the two writes, and is asserted to
  be a no-op when the row already agrees, so it is safe to call at any time.

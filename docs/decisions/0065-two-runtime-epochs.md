# ADR 0065: Two Runtime Epochs — MemberRun Owns the Adapter-Process Epoch, AgentSession Records the Provider-Session Epoch

```text
status: accepted; Owner decision 2026-09-05 (SPEC-ADAPTATION-REFACTOR-01 D-B, Review 04 Pass)
date: 2026-09-05
amends: ADR 0049 (clarifies runtime_generation); docs/current/architecture/agent-runtime.md "Team Host runtime" and "Work delivery" wording
canonical_for: the meaning of MemberRun.runtime_generation and AgentSession.runtime_generation and the scope of their relation
```

## Context

`docs/current/architecture/agent-runtime.md` said that `MemberRun` is an
internal diagnostic projection with no runtime authority, while the code makes
`MemberRun.runtime_generation` part of the `RuntimeBindingFence` that every
provider effect must prove
(`crates/firm-runtime-contract/src/provider_capabilities.rs`), requires
exactly one current active MemberRun before any Work delivery, and advances
that generation on Reopen, recovery, and non-clean runtime replacement
(`crates/firm-store/src/trust_kernel/trust_members.rs`,
`crates/firm-cli/src/main_modules/team_recovery_work.rs`). The review in #765
(finding F4) and the four review rounds of SPEC-ADAPTATION-REFACTOR-01
established the facts, verified at master `84b93001`:

- The code already states the model in
  `crates/firm-cli/src/main_modules/runtime_effects.rs`:
  "`MemberRun.runtime_generation` fences the Team-owned adapter process.
  `AgentSession.runtime_generation` fences the machine-owned provider
  session. They are deliberately independent: Team Close/Reopen replaces the
  adapter generation while retaining the same AgentSession, native transcript,
  and WorkExecutionBindings. Conflating the counters strands every legitimate
  same-session Reopen as soon as the MemberRun advances."
- A session row is minted in exactly two production places. The Team path
  (`member_orchestration.rs`) copies the current MemberRun generation into
  `AgentSession.runtime_generation` and embeds it in the session id. The
  standalone session-start route (`http_trust_routes.rs`) mints
  `runtime_generation: 1` with no MemberRun and no NodeDaemon.
- Production code never advances the session field afterwards; a session row
  is reused across Reopens while it is not Closed. `AgentSession` carries no
  `member_run_id` and is `deny_unknown_fields`; lookup is keyed on
  `(agent_member_id, execution_space_id, non-Closed)` and a second current
  session is refused. Every MemberRun is created at generation 1, and TeamRun
  completion closes no session.
- `external_interactive` Hosts have a MemberRun epoch and no AgentSession at
  all; that epoch is a live fence for their Work commands.
- A checked-in test (`managed_binding_uses_one_exact_identity_and_independent_generations`)
  asserts member generation 2 against session generation 1 as design intent.

## Decision

**Adopt the two-epoch model as the stated architecture, in the code's own
words, without any schema change.**

```text
MemberRun.runtime_generation     adapter-process epoch. The Team-owned adapter
                                 process / coordination authority generation.
                                 Reopen, recovery, and non-clean replacement
                                 advance it. external_interactive Hosts have it
                                 without a session. It is fence authority.
AgentSession.runtime_generation  provider-session epoch. The machine-owned
                                 provider session generation, immutable per
                                 row, embedded in the session id.
Deliberately independent         Close/Reopen advances the adapter-process
                                 epoch and keeps the same AgentSession, native
                                 transcript, and WorkExecutionBindings.
```

Mint fact (Team-path sessions only): the row's provider-session epoch equals
the minting MemberRun's adapter-process epoch at mint time.

Invariant, scoped to the MemberRun that minted the row: the row's epoch equals
that MemberRun's epoch at mint and is at most its current epoch. It is a
documented statement, not a Store assertion; the Store has no join key that
identifies the minting MemberRun, and the Owner withdrew the proposed
mint-record ledger. Three situations are outside the invariant and are legal:

1. A session reused by a later MemberRun of the same AgentMember (which starts
   at generation 1), for example a session that survived TeamRun completion.
2. `external_interactive` Hosts, which have no AgentSession.
3. Sessions minted by the standalone session-start route, which have no
   minting MemberRun.

The Result settlement exception in `agent-runtime.md` ("same provider-session
epoch, higher adapter-process epoch") is a direct consequence of this model.

ADR 0049 stands; its `runtime_generation` is the adapter-process epoch. The
field names stay as they are; documentation uses the epoch names.

Rejected: making `AgentSession.runtime_generation` the sole epoch (it would
require building a session-epoch advance, re-encoding the session id, a new
Supervisor dedup key, and a rule for sessionless Hosts, and could not be
rolled back under `deny_unknown_fields`); keeping the "diagnostics only"
wording (every recovery fix rediscovers the truth).

## Consequences

- `agent-runtime.md` no longer contradicts the fence: MemberRun carries the
  adapter-process epoch and coordination status; TeamRun stays a projection.
- The generation-related defects are fixed by mechanism, each in its own
  kernel-tier slice and without a dogfood freeze: a Host verb that releases a
  binding whose adapter-process and provider-session epochs are both provably
  superseded (#799, #734 option c), the native-session-attach admission
  asymmetry (#745, #583), the `RecoveryRequired` session exit (#755), the
  completed-run Supervisor lifecycle (#812), and the dual MemberRun ledger's
  read cost and crash journal (#821, #764).
- Any future change to either epoch's meaning, or to the mint sites, is a
  kernel-tier change and amends this ADR.

# Master Recovery Design — 2026-08-08

```text
status: active recovery design
baseline_under_recovery: origin/master@3b7ba16184d89a09c053f8b30b82ff0ca8b3b431
semantic_boundary_for_phase_1: c0e8aad
execution_policy: temporary Codex subagents only; no Harness Member, Mission, or Work records
```

## Purpose

Restore one buildable, testable and honestly described baseline before resuming Harness dogfood.
Phase 1 is deliberately a recovery change, not a feature-integration change. It removes the
incoherent P1 overlay, completes the firm rename at current active boundaries, repairs active CI and
documentation consumers, and establishes exact-revision build provenance.

The current repair is executed by isolated Codex subagents. Harness Members and Mission/Work state
are not execution or acceptance evidence until the recovered candidate has passed independent review
and is ready for a separate dogfood run. This temporary policy must not be described as Harness
self-hosting acceptance.

## Baseline facts

- `c0e8aad` is the consistent semantic boundary for the six P1-touched source files:
  `firm-cli`'s `main.rs`, `host_dispatcher.rs`, `member_probe.rs`, and `supervisor_daemon.rs`, plus
  `firm-core/src/lib.rs` and `firm-store/src/lib.rs`.
- The `member_probe` module did not exist at that boundary. Phase 1 removes it and its dangling
  integrations.
- Phase 1 does not restore the proposed seventh `WorkStatus::Orphaned`, universal acceptance
  evidence, P1-1/P1-2 behavior, reconcile behavior, or member-probe behavior.
- P0 is not fully preserved on current master. The tree at `c27a017` contained #387 P0-1 bind-host
  increments and P0-3 provider-admit work, but those increments are absent after reconstruction to
  the `c0e8aad` boundary and must be reintroduced later from their contracts and tests.
- #387 P0-2 commit `ae7d8c2` never entered master. Master contains only a headless-dispatch
  placeholder; Phase 1 must not present it as a durable dispatcher.
- The rename is not a license for global replacement. `HARNESS_HOME`, `.harness`, historical ADR
  text, wire/fixture compatibility examples, and Rust dependency aliases remain where they are
  intentional compatibility or provenance boundaries.

## Phase 1 scope and acceptance

### 1. Restore the pre-P1 contract

Keep the six source files semantically aligned with `c0e8aad`, apart from the completed crate/binary
rename, formatting, compile-only quarantine of the unwired #415 path, and exact build provenance.
Remove all dangling P1-1/P1-2 and member-probe types, fields, commands, tests and imports. Do not
introduce serialized compatibility claims while doing so.

### 2. Complete the active firm rename

The canonical package and executable are `firm-cli` and `target/debug/firm`. Active CI, package
scripts, dashboard runtime checks, Docs v2 smoke tests, Company OS finance/operator/org/work smoke
tests, installer sources, native-session boundary docs, current skills, plugin mirrors, AGENTS and
the documentation registry must agree. A user-facing `harness` compatibility command or environment
name may remain when it is an intentional supported boundary.

Deleted historical designs and archives are not restored or made current by changing their path.
Active checks that only validate removed historical review/PRD artifacts are retired from CI or
rewritten against a genuinely current contract.

One stable Company OS trademark JSON fixture is explicitly promoted into
`docs/current/company-os/fixtures` because active dashboard/operator acceptance consumes the data.
Its manifest records historical blob `fcbdc293841529faa95c5481788957a68b882865`, deletion commit
`8dff83b`, current ownership and checksum. This is governed deterministic test data, not product
authority: no deleted design document or archive path is restored, and the fixture cannot establish
current product claims by itself.

### 3. Exact, store-less build identity

`firm --build-info` must return without resolving or opening a project/store. A known `git_rev` is a
full 40-character commit SHA; `unknown` is the only fallback. The same full/unknown contract applies
to `/v1/meta`, dashboard doctor, and the Vite dashboard build so short/full mismatches cannot produce
a false stale-build diagnosis. Archive builds receive an explicit exact SHA.

Dirty-tree verification and exact committed-candidate verification are reported separately. A dirty
build can prove behavior but cannot prove that its embedded HEAD equals its uncommitted contents.

### 4. Read safety and compatibility boundary

No Phase 1 command may inspect the active Harness store. Host-attention and related read paths are
treated as potentially mutating because initialization, reconciliation, migration or projections may
write. Tests use only disposable temporary homes/stores.

Persisted-ledger compatibility remains a required later acceptance gate, but Phase 1 does not claim
it complete. Representative-ledger replay, ordinary transitions, or an explicit manifested cutover
must be designed and executed against isolated copies before dogfood or release.

### 5. Phase 1 gates

The dirty candidate must pass:

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace -- --test-threads=1
cargo clippy --workspace --all-targets -- -D warnings
pnpm check
native-session boundary, plugin mirror/install, skill install, package, and governance gates
```

After a local recovery commit, export that exact commit with `git archive` into a new temporary
directory and rebuild/test it with a separate `CARGO_TARGET_DIR` and the full candidate SHA supplied
as build provenance. This proves the committed tree, not the dirty working tree. Remote CI, push,
merge and persisted-ledger compatibility remain outside Phase 1 acceptance.

## #415 risks held for later work

The old multi-team daemon is not safe to expose merely by restoring its CLI wiring:

- startup unconditionally unlinks the socket path and can remove a live daemon's socket;
- daemon scan cadence is coupled to client timeout, so an unknown result can trigger blind fallback
  and competing supervisors;
- lease reads use `.ok().flatten()` in adoption paths and therefore fail open on store errors;
- status/stop socket helpers have existed since #399 but were never publicly wired; current daemon
  status/stop commands refer to another daemon surface and team-run status does not aggregate the
  multi-team daemon truth.

Phase 1 may explicitly quarantine the three old integration tests so the unwired source continues to
compile. The quarantine must name #415 and remain visible in test output and this design. Phase 3 must
re-enable and expand them only after the hazards above are resolved. No #415 runtime behavior is
implemented in Phase 1.

## Required follow-up order

Recovery after Phase 1 is serial and contract-first:

```text
schema
  -> restore #387 P0-1 bind-host increments and P0-3 provider admit
  -> reconstruct P1-1 and P1-2 without Orphaned/universal evidence
  -> extract and verify a provider-neutral probe library
  -> harden and publicly wire #415
  -> implement #387 P0-2 as a durable, leased dispatcher
  -> reconcile
  -> enforce branch protection and exact-head green checks
```

Each step needs its own behavior tests on the same revision and must keep the Phase 1 baseline green.
Historical feature branches are patch references, not acceptance evidence.

## Non-goals

- no #387 or #415 feature implementation;
- no active-store read, migration, replay or write;
- no claim of persisted-ledger compatibility, live provider acceptance, or Harness self-hosting;
- no force-push, PR, merge or master mutation;
- no resurrection or cosmetic retarget of deleted archives;
- no bulk rename of compatibility environment names, storage directories, ADR history, fixtures, or
  Rust dependency aliases.

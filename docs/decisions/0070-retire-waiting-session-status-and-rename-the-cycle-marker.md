# ADR 0070: Retire the Unreachable `waiting` Session Status and Rename the Harness Cycle Marker

```text
status: accepted; implemented by the 2026-09-15 N1 cutover
date: 2026-09-15
amends: the AgentSession wire shape named by ADR 0049 and ADR 0065
canonical_for: which AgentSession lifecycle values exist; what the open-cycle marker is and is not
```

## Context

`AgentSession` is the durable coordination record for one NodeDaemon-owned
provider session. ADR 0032 draws the line it must not cross: the provider's
native session store is the sole truth for transcript, tool calls and turn
identity, and Harness owns coordination records only. Two fields on that
struct had drifted across that line in opposite directions.

**`AgentSessionStatus::Waiting` was unreachable.** No writer anywhere in the
tree produced it. What it did produce was surface area: the Store's transition
table carried four edges that named it (`Active -> Waiting` and
`Waiting -> Active | Idle | Closed`), a post-write arm cleared the cycle on it,
and four command-eligibility lists plus the CLI's projection table treated it
as a drivable lane. Every one of those was a rule about a state the system
cannot enter, and each one silently widened what a reader would admit if a row
ever claimed it.

**`current_turn_id` was named after something it never held.** The Store
synthesizes the value itself on the `-> Active` write — it was literally
`format!("provider-turn:{session_id}:{version}")` — and clears it when the
cycle ends. Every reader in the tree uses it as a presence fact: the
terminal-boundary predicate, the drained-lane proof, the reattach quiescence
fence, the Team admission `safe_cold` check, the two drain settlements and the
`expected_cycle_ref` precondition all ask only *is a cycle open on this lane*.
None can resolve, resume or cancel anything with it, because it is not a
provider turn id and no provider has ever seen it. A Harness-owned counter
wearing a provider-truth name is exactly the confusion ADR 0032 exists to
prevent.

At `406f5f9e` the repository and every real store agree:

| Fact | Evidence |
| --- | --- |
| No code constructs `AgentSessionStatus::Waiting` | only match arms and one test matrix named it |
| No persisted session was ever `waiting` | 2,162 `agent_session` projections across 33 real trust journals: 0 `waiting` |
| ...across every store we hold | 1,641 in `~/.harness` (24 spaces), 503 in `.visual-evidence` (3 spaces), 18 in the two-mac evidence journals under `docs/` |
| Every persisted row carries the old field spelling | 2,162 of 2,162 carry `current_turn_id`; `AgentSession` is `deny_unknown_fields` |
| The marker never leaves Harness | its only writer is the Store's own `-> Active` arm; no adapter reads or sends it |

## Decision

1. **`waiting` is deleted, not reserved.** `AgentSessionStatus` is `cold`,
   `idle`, `active`, `interrupted`, `recovery_required`, `closed`. The four
   dead transition edges, the post-write arm, and every reader arm that
   admitted a command on a waiting lane are gone with it.

   Because no row anywhere carries `"waiting"`, the enum **drops** the wire
   value rather than reserving it: a document carrying it now fails decoding
   instead of decoding into a drivable lane. This is the same call ADR 0067
   made for `provider_driven`, for the same reason — a reserved value nothing
   ever wrote could only arrive from a forged or corrupted document, and
   refusing the decode is the fail-closed reading.
   `schemas/fixtures/agent-session/invalid/retired-waiting-lifecycle.json`
   pins that refusal on the JSON Schema side and
   `crates/firm-core/tests/agent_session_wire_contract.rs` pins it on the Rust
   serde side.

   **The "waiting" fact is not lost.** A lane blocked on provider input is
   `Active` with `RuntimeActivity::WaitingInput` — a bounded control
   observation in `control_state`, which is where an observation belongs. It
   was never a durable lifecycle, which is why nothing ever wrote one.

2. **`current_turn_id` becomes `current_cycle_marker`.** It is an opaque
   marker of the one execution cycle a lane currently has open, synthesized by
   Harness as `harness-cycle:{session_id}:{version}` and cleared when the cycle
   ends. It never carries a provider turn id, and no surface may resolve,
   resume or cancel a provider turn from it. The shared predicate renames with
   it: `AgentSession::is_at_terminal_turn_boundary` becomes
   `is_at_terminal_cycle_boundary`, and the operator strings it feeds say
   "terminal cycle boundary" and "open cycle" instead of naming a turn.

   **Semantics do not change.** Same writer, same clearing rule, same readers,
   same meaning — only the name, the synthesized prefix and the label in the
   messages. This is deliberately a rename and nothing more.

3. **`#[serde(alias = "current_turn_id")]` is retained and load-bearing.**
   `AgentSession` is `deny_unknown_fields` and 100% of persisted rows spell the
   field the old way, so without the alias every existing trust journal would
   stop decoding. No writer emits the old spelling again: the JSON Schema
   declares only `current_cycle_marker`, and a round-trip test proves a legacy
   row decodes onto the new field and reserializes under the new name. The
   alias is a read tolerance, like the five ADR 0069 froze, not a dual write.

4. **Historical evidence is not rewritten.** The four `*-trust.jsonl` evidence
   journals under `docs/current/operations/evidence/` keep the exact bytes
   they recorded, including `current_turn_id` and the `provider-turn:` prefix.
   They are records of what ran, and (3) is what keeps them readable.

## Consequences

- A binary older than this cutover cannot read a row written after it: the old
  struct is `deny_unknown_fields` with no alias for the new spelling, so a
  downgrade after any new session write is one-way. This matches the existing
  DEV-230 downgrade boundary documented in `operations.md`.
- The transition table shrinks from thirteen edges to nine. Four RuntimeCommand
  eligibility lists lose their `waiting` arm — the Store's `StopSession` guard
  and its quiesce/release/close/drain guard, plus the CLI admission preflight's
  two counterparts — and the CLI's desired-lifecycle projection loses three
  arms (`Waiting -> Active | Idle | Closed`).
- Operator-visible strings change wording: "not at a terminal turn boundary"
  becomes "not at a terminal cycle boundary", "still has an open turn" becomes
  "still has an open cycle", and the predecessor-drain conflict says "an open
  cycle". The typed `TrustErrorCode` values are untouched, so the generated
  member-trust error contract is unchanged.
- **The TeamRun hold fingerprint keeps its `in_turn` key, deliberately.** The
  field it reflects is now `current_cycle_marker`, but that key is a durable
  hash input, not vocabulary: the fingerprint is written into the TeamRun ledger
  as a `team-run-canonical-state:` evidence ref on a `team_supervisor_no_progress`
  hold (`supervisor_daemon/recovery.rs`), read back by a later NodeDaemon
  generation — possibly a different binary — and compared for equality against a
  freshly recomputed one. `canonical_json_fingerprint` hashes object keys, so
  renaming it would invalidate every hold written before the cutover exactly
  once: adoption would be re-enabled on a run whose canonical rows had not
  changed, burning a fresh TeamSupervisor generation (#671, #704) on the
  heartbeat-starvation path (#836), and breaking the "clears only when a
  canonical row changes" contract in `operations.md`. The renaming rule for this
  decision therefore stops at the wire: field names and enum values move, hashed
  key spellings do not. `canonical_state_document_keys_are_frozen_durable_hash_inputs`
  is the trip-wire.
- ADR 0049 and ADR 0065 are amended in their wire-shape references only. Their
  Close/Reopen/Retire semantics and their two-epoch generation rule are
  untouched: this ADR renames a field and deletes an unreachable enum value,
  and changes no authority, fence or lifecycle rule.

## Alternatives rejected

- **Give `waiting` a writer.** Nothing needed one in the whole history of the
  system: the fact it would record already exists as
  `RuntimeActivity::WaitingInput`, one layer down, where it does not multiply
  lifecycle edges.
- **Reserve `waiting` as a legacy wire value.** Zero rows wrote it, so a
  reserved value could only ever arrive from a forged or corrupted document.
  ADR 0067 settled this pattern already.
- **Keep the field name and fix only the documentation.** The name is what
  readers see first, and it asserted provider-native truth on a
  Harness-synthesized value. A comment under a misleading name loses to the
  name.
- **Rename the field without an alias, or dual-write both spellings.** Without
  the alias every persisted store stops decoding on the first read. A dual
  write would create a second spelling for one fact, which is the ambiguity
  the rename exists to remove.

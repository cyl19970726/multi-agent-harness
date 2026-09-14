# ADR 0067: Retire the NativeContinuation Control Plane

```text
status: accepted; implemented by the 2026-09-14 C1 cutover
date: 2026-09-14
supersedes: the provider-driven half of ADR 0041
canonical_for: which execution drivers exist; which continuation surface is retained and why
```

## Context

ADR 0041 modelled continuation as two independent axes and gave the execution
driver three managed values: `host_driven`, `provider_driven`, and the declared
external `user_driven` exception. The `provider_driven` value came with a
control plane: three semantic capabilities (`inspect_continuation`,
`inhibit_continuation`, `resume_continuation`), two `ControlIntent` variants,
six `RuntimeCommandKind` values, a `RuntimeDriverRef::ProviderContinuation`
authority shape, and a Store fence that admitted a provider continuation as the
next-cycle driver.

None of it ever executed. At `24a6ab5a` the repository and the live dogfood
stores agree:

| Fact | Evidence |
| --- | --- |
| No production code constructed `ProviderDriven` | only tests and fixtures named the variant |
| No persisted AgentSession was provider-driven | `~/.harness`: 5233 `host_driven`, 16 `user_driven`, 0 `provider_driven` |
| No persisted driver ref was a provider continuation | 0 `"kind":"provider_continuation"` rows |
| No continuation was ever armed | 0 `{"state":"armed"}` activations |
| No RuntimeCommand row carried a continuation kind | persisted kinds are `author_message`, `start_cycle`, `open_runtime`, `resume_native_session`, `close_member`, `interrupt_current_cycle`, `dispatch_provider`, `start_session` only |
| Every provider declared the controls unusable | Codex `Experimental` (never canaried), Claude / DeepSeek / Kimi / Pi `Unsupported` |

The three-driver model was therefore a standing invitation to build a second
scheduler, not a shipped capability. ADR 0041's own record says why that is
dangerous: the live Codex canary that motivated it produced two concurrent
top-level turns writing one worktree.

## Decision

1. **Two execution drivers, not three.** `MemberExecutionDriver` is
   `host_driven` for every managed runtime and `user_driven` for the declared
   `external_interactive` Host exception. `RuntimeDriverRef::ProviderContinuation`
   is deleted with it.

   Because no row anywhere carries `"provider_driven"` or
   `"provider_continuation"`, the enum **drops** the variants rather than
   reserving their wire values: a document carrying them now fails decoding
   (`AgentSessionControlState` is `deny_unknown_fields` and the enums have no
   `other` arm) instead of silently decoding into a managed driver. The former
   `provider-driven-armed` valid fixture is kept as an **invalid** fixture so
   both the JSON Schema gate and Rust serde prove that refusal.

2. **The control plane is deleted.** `ControlIntent::{InhibitContinuation,
   ResumeContinuation}`, `ControlIntent::validate()`,
   `validate_continuation_exact()`, `SemanticCapability::{InspectContinuation,
   InhibitContinuation, ResumeContinuation}` (`ALL` 14 → 11), the fifteen
   provider `CapabilityBinding` declarations, and the Codex/Kimi/Pi adapter
   control arms are gone.

3. **The command kinds are frozen, not deleted.**
   `RuntimeCommandKind::{InspectContinuation, InhibitContinuation,
   ResumeContinuation}` join `ActivateContinuation`,
   `ReplaceContinuationCondition` and `ClearContinuation` in
   `FROZEN_RUNTIME_COMMAND_KINDS`. Their wire values stay readable and exactly
   replayable; a new prepare is rejected with `RUNTIME_COMMAND_KIND_FROZEN`
   before any classification, capability or fence check. This is the
   fail-closed option: freezing rejects earlier than a capability lookup would,
   and it keeps historical replay byte-identical.

   The kind → `required_capability` maps keep their arms. Those strings
   (`continuation.inhibit`, …) are server-owned permission names, not the
   deleted `SemanticCapability` names, and the already-frozen sibling kinds
   keep theirs for the same reason.

4. **The activation projection is retained — it is the drained-lane proof.**
   `AgentSession.control_state.continuation` keeps its `definition`,
   `activation` and `observed_at` fields. 1,597 + 503 persisted `agent_session`
   rows already carry them under `deny_unknown_fields`, and — more
   importantly — `NativeContinuationActivation::Armed` is read by six
   fail-closed guards that must keep refusing:

   - both `RuntimeBindingFence` driver arms (a host cycle is refused while a
     continuation is armed);
   - `interrupted_runtime_is_terminated` (DEV-171), the proof that a killed
     runtime is really gone before `Interrupted -> Idle`;
   - `lane_is_quiescent`, the reattach precondition;
   - the user-driven admission rule (an external runtime must be disarmed);
   - `safe_cold` in Team admission and `LaneTerminationProof` (DEV-232).

   Nothing arms it any more; every writer sets `Disarmed`. Keeping the readers
   means that if anything ever did, host drive, reattach, Close and user-driven
   admission would all still refuse. Deleting them would convert six refusals
   into six silent allows. `schemas/fixtures/agent-session/valid/host-driven-armed-continuation.json`
   keeps the armed wire shape under both the JSON Schema gate and Rust serde,
   so the shape those six guards read cannot stop decoding by accident.

   `continuation.definition.{phase, continuation_ref, revision}` stay for the
   same reason: they are what the retained `expected_continuation_ref` /
   `expected_continuation_phase` RuntimeCommand preconditions are proven
   against. Those preconditions are kept — they are persisted fields of a
   `deny_unknown_fields` struct and they fence *any* command, not only the
   retired kinds.

5. **The quiesce step is retained, and it is not uniform across adapters.**
   `QuiesceStep::InhibitContinuation` and `QuiesceReceipt.continuation_inhibited`
   are unchanged — removing them would change the shape of every serialized
   `QuiesceReceipt`. Claude, Pi, Kimi and DeepSeek satisfy that step from
   `activation == Disarmed`: they have no native continuation to control, so
   the retained projection is the whole proof. Codex is different and stays
   different: on both the Close and the quiesce path it reads
   `thread/goal/get` and, when the observed Goal is `active`, writes
   `thread/goal/set(paused)` before proving the thread idle. That write is
   terminal-control safety — it stops a Goal from starting a successor turn
   and racing the terminal observation — not a scheduling operation, and it is
   the one continuation write this ADR deliberately keeps.

6. **Read-only observation stays.** Codex `observe_continuation`
   (`thread/goal/get`) keeps feeding the activation projection, and
   `inhibit_active_goal_for_terminal_control` stays on the Close path: a member
   can still activate a native Goal inside its own session, and Close must
   pause it before the terminal observation or the Goal can start a successor
   turn and race Close. Observation is not control, and it grants no authority:
   the projection's activation is copied from the durable record, never
   inferred from what the provider reports.

   The Codex `CODEX_ONE_DRIVER_VIOLATION` guard that refuses a host
   `start_cycle` while the observed continuation is armed is likewise **kept**,
   for the same reason as the Store fences in (4). It is a refusal, not a
   feature of the retired plane.

## Consequences

- Capability fingerprints change for all five providers. A
  `ProviderIntegrationProfile`'s `capability_fingerprint` is
  `canonical_json_fingerprint` over its `capability_bindings`, so removing
  three bindings changes every provider's fingerprint.

  The live dogfood is unaffected because a profile is **snapshotted per
  MemberRun when the member starts**: `MemberRun.provider_profile_snapshot`
  keeps the fingerprint it was created with, and `RuntimeBindingFence`
  compares a command's `capability_fingerprint` against
  `AgentSession.control_state.capability_fingerprint` — both sides of that
  comparison come from the same frozen snapshot, not from the new binary's
  binding list. A running member therefore keeps driving on its old
  fingerprint across a binary upgrade; the new fingerprint appears only when a
  profile is refreshed (`profile_capability_fingerprint` is recomputed on
  refresh) and a new Session is opened under it. 2,517 persisted
  `"capability":"inspect_continuation"` / `"inhibit_continuation"` strings in
  existing MemberRun snapshots stay readable because
  `ProviderCapabilityBinding.capability` is a `String`, not the enum.

- ADR 0041 is superseded in its `provider_driven` half only. Its
  one-top-level-driver invariant, its Workspace-lease rule, its completion-policy
  axis, and its "provider satisfaction is not Host acceptance" boundary all
  remain active — the first of those is now *stronger*, because with one
  managed driver there is no longer a legitimate way to express two.

- Hard Invariant 8 in `AGENTS.md` loses the `provider_driven` option. The
  sentence "Never activate a provider-native goal and also issue an ordinary
  Harness start for the same work" is reworded rather than dropped: a
  provider-native goal is now unambiguously an internal execution aid (Hard
  Invariant 9), and the Harness start is the only start there is.

## Alternatives rejected

- **Delete the continuation projection too.** It is 2,100 persisted rows under
  `deny_unknown_fields` and, more decisively, it is the field six drained-lane
  guards read. Deleting it trades a documentation cleanup for six lost
  refusals.
- **Delete the RuntimeCommand kinds.** Historical replay must stay exact, and
  the repository already has a freeze mechanism whose whole purpose is this
  case. Deleting would make an old envelope undecodable rather than rejected.
- **Reserve `provider_driven` as a legacy wire value.** Nothing wrote it, so a
  reserved value could only ever arrive from a forged or corrupted document.
  Failing the decode is the fail-closed reading.

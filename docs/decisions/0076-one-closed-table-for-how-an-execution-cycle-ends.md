# ADR 0076: One closed table for how an ExecutionCycle ends

```text
status: Accepted — Owner 2026-09-15 (core 6); implementation tracked by Task CORE6-X1-20260915
date: 2026-09-15
amends: no ADR. This adds a table where there was none; `InterruptCause`, `CycleTimeouts` and
        `ExecutionCycleOutcome` keep every field and meaning they had. (The zero-producer
        `InterruptCause::AdapterPolicy`, which SPEC-TYPED-CYCLE-OUTCOME-01 froze as a reviewed
        escape hatch, is projected onto `InterruptedByProvider` here and its retirement is a
        separate decision, tracked by the core-6 contract-trim Task.)
canonical_for: the closed set of ways one ExecutionCycle may end; how each ending maps to a
        member_actions action_type, a provider_status, and a durable settlement; and the two
        Harness↔provider turn misalignments this closes
baseline: master aaf278ec; the current-state audit of the loop, the five adapters and the
        September dogfood stores that motivates it
```

## Context

`ExecutionCycle` is the Harness-side "turn". It is aligned with each provider's native
turn at exactly two points: **acceptance** (the exact provider input-acceptance receipt)
and **terminal** (the exact provider terminal reference). That alignment is right and is
not what this ADR changes.

What was missing is a closed answer to "how did this cycle end". Before this ADR:

- **The `Ok` half had no name.** An ending had to be reconstructed by crossing three
  orthogonal axes on `ExecutionCycleOutcome` — terminal observed × `provider_terminal_failure`
  × `interrupt` — plus `close_requested_by_harness`. No enum said what the combinations mean,
  so every reader recombined them by hand.
- **The `Err` half had no name at all.** Anything an adapter could not express in the outcome
  left `run_cycle` as `Err(Self::Error)` carrying a free-form string, and the shared loop
  funnelled *every* one of them into a single action row: `action_type = "provider_error"`,
  summary "provider round N failed; inspect the provider-native session for details". A
  transport death, an acceptance timeout, a one-driver refusal and a protocol violation were
  the same row.
- **`provider_status`, the one machine-readable column on that row, was populated by one
  provider.** `take_cycle_terminal_failure` had a `None` default and only Codex overrode it,
  so on Claude, DeepSeek, Kimi and Pi a failed cycle carried no machine-readable reason.
- **`ProviderTerminalFailure.reason` is an unbounded `String` fed by provider text**: Codex
  `codexErrorInfo` variant keys, Claude/DeepSeek runner `terminalReason`, Kimi ACP stop
  reasons, Pi `errorMessage` prose. Only the quota/auth classifier ever matched it, and only
  for two states.

The September dogfood stores show the cost. Across 305 `member_actions` rows the
`action_type` column carried **16 distinct free string values**; of 76 recorded terminals,
**4 carried no reason at all** because the Claude adapter's terminal reference is a
catch-all with a synthetic cycle counter. Two independent places recorded "how it ended"
and neither was a closed set.

The audit also found two places where the Harness cycle and the provider turn genuinely
disagree, not just describe things differently. They are fixed here because a closed table
is what makes them visible:

1. **A control that races a normal completion was dropped.** Claude and DeepSeek hard-coded
   `interrupt: None, close_requested_by_harness: false` on the `turn_complete` arm. A Host
   Interrupt or Close issued while the turn was finishing therefore vanished from the
   outcome, and the shared loop — still holding the pending control effect — failed the whole
   member with `RuntimeRecoveryRequired: control lacked verified terminal acknowledgement`.
2. **An empty terminal was classified two ways.** Codex, Kimi and Pi returned
   `final_text == ""` with no failure, which feeds the zero-output circuit breaker. Claude
   and DeepSeek reported `ProviderTerminalFailure { reason: "empty_final_report" }` for the
   same observable fact — and because `decide_team_round` disqualifies the zero-output branch
   whenever a terminal failure is present, that actively **reset** the unproductive-round
   streak. A member producing nothing three rounds running opened the breaker on three
   providers and never on the other two.

## Decision

### 1. `CycleEnding` is the closed table

`crates/firm-runtime-contract/src/cycle_ending.rs:201` defines twelve variants. Every
ending of `run_cycle`, `Ok` and `Err` alike, maps to exactly one:

| ending | means | settles as |
| --- | --- | --- |
| `Completed` | trusted terminal, no failure, authored output or tool calls | Applied / Satisfied |
| `EmptyOutput` | trusted terminal, no failure, no authored text and no tool call | Applied / Satisfied |
| `InterruptedByHost` | trusted terminal reached because the Host interrupted | Applied / Satisfied |
| `InterruptedByProvider { reason }` | the provider ended the turn as interrupted with no Harness request | Applied / Satisfied |
| `Closed` | trusted terminal reached under a Harness Close request | Applied / Satisfied |
| `ProviderFailed { code, detail, http_status }` | the provider reported a failed turn | Applied / Satisfied command, `Unsatisfied` postcondition |
| `NotStarted { code }` | refused **before** the input crossed the provider boundary | Rejected / NotApplied |
| `AcceptanceTimeout` | the input was written but never acknowledged | Rejected / NotApplied |
| `TransportLost { detail }` | transport or child died before a terminal | RecoveryRequired / Unknown |
| `ControlSettleTimeout` | an issued Interrupt/Close was never settled | RecoveryRequired / Unknown |
| `TerminalUnobserved { code, detail }` | a terminal was reported but cannot be trusted | RecoveryRequired / Unknown |
| `HostAborted { detail }` | the **Harness** ended the cycle itself | RecoveryRequired / Unknown |

`CycleEnding::settlement` (`cycle_ending.rs:343`) takes whether the acceptance receipt
exists, because the last four are genuinely two-valued: a transport that dies *before*
acceptance never applied anything and is replay-safe, while the same death *after*
acceptance is "accepted, outcome unproven" (invariant I2) and never "not applied". The
first six cannot occur without a receipt; the two Rejected rows cannot occur with one.

Three variants beyond the shape the core-6 audit proposed, each for a stated reason:

- **`NotStarted`** — Codex's five `CODEX_ONE_DRIVER_VIOLATION` pre-send refusals, its
  closed-runtime refusal, its armed-continuation refusal, and Kimi's "already has an active
  session/prompt" all happen before anything is written to the provider. That is materially
  different from `AcceptanceTimeout`: nothing was sent, so a replay is safe, and the loop
  already settles it Rejected/NotApplied. Folding it into `ProviderFailed` would claim the
  provider failed when the Harness refused.
- **`HostAborted`** — `CycleControl::fatal_error` and a failing `on_input_accepted` callback
  (a durable Harness settle) are the Harness's own exits from `run_cycle`. Neither is a
  provider ending, and calling either `ProviderFailed` would be a false attribution.
- **`TerminalUnobserved`** — "the provider reported a failed turn" and "we cannot trust what
  we saw" are different facts with different settlements. A provider failure is an *observed*
  terminal with an `Unsatisfied` postcondition; an unobservable terminal is `Unknown` and
  becomes explicit recovery work. `CLAUDE_AGENT_SDK_PROTOCOL_ERROR`,
  `KIMI_CYCLE_TERMINAL_MISMATCH`, `CODEX_RUNTIME_POSTCONDITION_UNKNOWN`,
  `PI_CYCLE_SETTLEMENT_UNKNOWN`, `DEEPSEEK_HARNESS_PROTOCOL_ERROR` and the two
  `UNEXPECTED_CLOSE` refusals live here, not under `ProviderFailed`.

### 2. Precedence inside the `Ok` outcome

Close, interrupt, failure and emptiness can all be true of one outcome, so
`CycleEnding::from_outcome` (`cycle_ending.rs:260`) applies one stated order:

```text
ProviderFailed > Closed > InterruptedByHost > InterruptedByProvider > EmptyOutput > Completed
```

A reported provider failure outranks everything because it is the only fact that makes the
postcondition `Unsatisfied`. Close outranks Interrupt because Close is why the runtime is
ending. `CycleEnding` is a **summary**, never a replacement: `close_requested_by_harness`,
`interrupt` and `provider_terminal_failure` stay on the outcome, because
`verified_terminal_control_ack` still needs each of them separately.

### 3. `ProviderFailed.code` is a small closed enum; the provider's text is kept

`ProviderFailureCode` (`cycle_ending.rs:52`) is derived from today's producers only:
`QuotaExhausted`, `Unauthorized`, `OutputLimit`, `Refusal`, `TurnRequestLimit`,
`RunnerError`, `TurnFailed`. `classify` (`cycle_ending.rs:83`) reads the exact HTTP status
and the closed reason vocabularies already frozen in `provider_capacity.rs`, and **never**
substring-scans free text — for the same reason that classifier refuses to: a member writing
"fixed the 403 handler" must not classify its own account. Unrecognised provider text maps
to `TurnFailed`, which means *unclassified*, never *fine*; the original text always travels
as `detail`. `CycleRefusalCode` and `TerminalUnobservedCode` are the same shape for their
two variants.

### 4. One fixed action-type table, and `provider_status` on all five providers

`CycleEnding::action_type` (`cycle_ending.rs:317`) is the single source for the
`member_actions.action_type` column. The column stays a **string on the wire**; the values
are frozen, and the four pre-ADR-0076 spellings keep their meaning so historical rows stay
comparable:

| ending | action_type |
| --- | --- |
| `Completed` | `turn_completed` |
| `EmptyOutput` | `empty_provider_round` |
| `InterruptedByHost` / `InterruptedByProvider` | `interrupted` |
| `Closed` | `closed` |
| `ProviderFailed` | `provider_error` |
| `TransportLost` | `transport_lost` |
| `AcceptanceTimeout` | `input_never_accepted` |
| `ControlSettleTimeout` | `control_settle_timeout` |
| `NotStarted` | `cycle_not_started` |
| `HostAborted` | `host_aborted` |
| `TerminalUnobserved` | `terminal_unobserved` |

`CycleEnding::provider_status` (`cycle_ending.rs:386`) fills that column for **every**
ending. A provider failure keeps the existing `provider_terminal:{reason}:{http}` shape byte
for byte, so `ProviderTerminalFailure::parse` and the capacity classifier keep working
unchanged. Every other ending uses the distinct `cycle_ending:{wire}` prefix, which `parse`
deliberately does not match: a transport loss is not a provider terminal and must never be
classified as one.

`TeamRuntimeAdapter::take_cycle_terminal_failure` (`cycle.rs:403`) stops being an
adapter-implemented hook and becomes a shared default derived from
`take_cycle_ending` (`cycle.rs:393`). Adapters record an ending; the structured failure —
and therefore a populated `provider_status` — follows on all five.

### 5. Exhaustiveness is a compile-time property, per adapter

Each adapter owns a small closed enum naming its own `Err` endings and one wildcard-free
`ending()` match into the table: Codex `cycle_ending.rs:57`, Claude `:43`, DeepSeek `:44`,
Kimi `:55`, Pi `:46`. Adding an ending to an adapter is a compile error until it is placed
in the table, and each adapter's test holds a second wildcard-free match so the mapping
itself cannot drift silently.

Where an adapter's underlying client collapses several facts into one message string, the
client records the typed fact instead of the caller parsing prose: Codex and Pi gained a
typed RPC-failure classification, so their acceptance timeouts are recorded as
`AcceptanceTimeout` rather than as generic RPC-timeout strings; Kimi's ACP client and Pi's
RPC client record into the same per-adapter enum the runtime lifts onto the cycle.

### 6. The ending is persisted on the cycle correlation

`ProviderCycleCorrelation.ending` is an additive serde field carrying the frozen wire value
(`completed`, `empty_output`, `provider_failed:quota_exhausted`, …). It is written for every
cycle, including clean completions, which previously left no machine-readable trace of how
they ended. Pre-ADR-0076 rows carry no key and read back as `None`.

### 7. The two misalignments

- **A requested control that races a normal terminal is SETTLED by that terminal.** The
  control asked the turn to end; it ended at its exact native boundary. Claude and DeepSeek
  now carry the requested Interrupt/Close on the outcome together with its `abort` receipt —
  claimed only when the interrupt frame actually crossed the provider boundary, so a control
  that was never delivered still fails closed.
- **An empty terminal is `EmptyOutput` on all five providers and counts toward the circuit
  breaker everywhere.** `empty_final_report` is no longer a provider terminal failure.
  `decide_team_round` derives zero output from the ending rather than re-deriving it from
  text, so the same observable fact can no longer be recorded two ways.

## Consequences

- An operator reading one action row learns how the cycle ended and whether the input
  crossed the provider boundary, on every provider, without opening the provider-native
  session. The shared loop's catch-all row is gone.
- Six new `action_type` values appear in `member_actions` (`transport_lost`,
  `input_never_accepted`, `control_settle_timeout`, `cycle_not_started`, `host_aborted`,
  `terminal_unobserved`) and `closed` now also appears for a cycle that ended under a Close.
  Anything reading that column as a closed set must be updated; the four historical values
  are unchanged.
- `provider_status` now carries a second prefix, `cycle_ending:`. Readers that parse it as a
  `ProviderTerminalFailure` are unaffected — that parse still returns `None` for the new
  prefix, which is the honest answer.
- Claude and DeepSeek members whose empty rounds previously reset the streak will now open
  the unproductive-round circuit breaker after three, like the other three providers. That is
  the intended behavior change, not a regression.
- The acceptance test ADR 0056 names as
  `a_silent_provider_turn_is_a_provider_error_and_stays_reconstructable` is renamed
  `a_silent_provider_turn_is_an_empty_round_and_stays_reconstructable`, because that is what
  the row now is. Its guards are unchanged and one is added: a silent turn still publishes no
  fabricated completion, no handoff message and no Work submission, its native session stays
  resumable, and it must now NOT be recorded as a provider error. ADR 0056 stays as written.
- `CycleEnding` says nothing semantic. No variant claims Work completion or Host acceptance;
  provider satisfaction still never implies Host acceptance (invariant I6).

## Alternatives rejected

- **Classify by parsing the error message.** The strings are provider-authored and
  unstable; this is the disease, not the cure.
- **One shared failure enum for all five adapters.** Their failure vocabularies genuinely
  differ, and a shared enum would hide which provider can produce what. Per-adapter enums
  converging on one table keeps both facts.
- **Fold protocol violations into `ProviderFailed`.** It would claim the provider reported a
  failure it did not report, and would settle an unprovable terminal as `Unsatisfied`
  instead of `Unknown`.
- **Make `member_actions.action_type` a typed column.** The column is persisted history;
  freezing the values and generating them from one table gives the same closure without a
  migration.

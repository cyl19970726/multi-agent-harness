# Live Codex and Kimi Agent Team acceptance — 2026-07-21

```text
status: accepted live-provider evidence
owner_role: execution-foundation
canonical_for: first native Codex + Kimi AgentTeamRun acceptance and interrupted-attempt recovery
```

> Historical implementation evidence: this run predates ADR 0032 and therefore
> includes provider-derived MemberAction/TeamRunEvent mirrors. It remains valid
> evidence for transport, correlation, interruption recovery, and gates, but it
> is not the target storage contract. A new acceptance must prove native-session
> reads/resume with no mirrored provider tool/command/file history.

## 2026-07-22 Codex app-server live control addendum

The interactive adapter was subsequently verified against installed
`codex-cli 0.145.0-alpha.18` with native records:

- Mission `mission-1784651480593-p38526-0`;
- Wave `wave-1784651488050-p39605-0`;
- accepted TeamRun `team-run-1784651499664-p38249-0`;
- MemberRun `member-run-1784651499664-p38249-1`;
- provider thread `019f8584-f91d-7b61-9945-26b6780bfa95`.

The member ran a real app-server turn, received an operator message through
`turn/steer`, emitted structured command actions and a correlated final
handoff, and reached `completed`. The accepted Wave and closed Mission name the
attempt. Reasoning was eligible only for transient live SSE and no thinking row
was written. This addendum proves live steer for the reviewed installed
version; deterministic tests separately cover `AskUserQuestion` resume and
Codex/Kimi cooperative interruption.

## Scope

This record proves real provider transport and native Store reconstruction. It
does not claim that a deterministic fixture is live evidence, that assignment
receipt validates file contents, or that the Harness controls provider-native
subagents.

- Mission: `mission-1784634958783-p62756-0`
- Wave: `wave-1784634972607-p64405-0`
- selected attempt: `team-run-1784635821706-p13532-0`
- earlier preserved attempt: `team-run-1784635307471-p88869-0`

## Provider reality

| MemberRun | Provider/model | Provider session | Outcome |
| --- | --- | --- | --- |
| `member-run-1784635821706-p13532-1` | Codex `gpt-5.6-sol` | `019f8495-ab12-72a1-a0c9-694d418a60ec` | completed |
| `member-run-1784635821706-p13532-2` | Kimi `kimi-code/kimi-for-coding` | `session_49da875c-295f-4d86-bab1-7627c6ddcb53` | completed |

The requested historical `k2.5` alias was not configured by Kimi Code 0.27.0.
The run used the lowest configured coding tier, displayed by the local Kimi
configuration as **K2.7 Coding**, rather than silently falling back to K3 or
mutating user configuration.

## Attempt lineage and recovery

Attempt 1 started both real providers. Kimi emitted observable tool actions but
then requested interactive input and attempted further delegation, which was
outside the bounded audit. The Host stopped the foreground process to protect
quota. Process inspection confirmed no `team-run start`, `codex exec`, or Kimi
ACP process remained, but the append-only Store correctly still said `running`:
a status mutation alone had not observed the external interruption.

The implementation now supports an explicit recovery attestation:

```text
team-run cancel --confirm-provider-stopped --reason ... --cancelled-by ...
```

It preserved Attempt 1 as `cancelled`, marked its unfinished members `stopped`,
and recorded `interrupted/cancelled` MemberActions plus Host events. It did not
delete the attempt or claim a completed outcome.

Attempt 2 used a bounded transport-only prompt: no tools, subagents, file
inspection, or questions. Both members completed in one round. Each assignment
has its own correlation; each member returned a causation-linked `handoff` to
the Host; the Store contains explicit progress and completion actions.

## Verified native facts

- Wave attempt order contains the cancelled attempt followed by the completed
  retry.
- Both Assignment messages moved from queued to delivered with attempt `1`.
- Codex and Kimi MemberRuns have real provider session identifiers and terminal
  timestamps.
- Both handoffs name their originating assignment as `causation_id` and reuse
  its `correlation_id`.
- Dashboard snapshot joins Mission, Wave, selected TeamRun, both MemberRuns,
  assignments, handoffs, and MemberActions.
- No `thinking`, `thinking_preview`, or provider `reasoning` field occurs in
  `team_messages.jsonl`, `member_actions.jsonl`, `team_run_events.jsonl`, or
  `member_runs.jsonl`.

## Acceptance boundary

This proves Codex exec transport, Kimi ACP transport, native attempt lineage,
assignment/handoff correlation, transitional durable action projection, interrupted-run
recovery, Dashboard projection, and the non-persistence of thinking. The
evidence references named by the members were assignment-provided references;
their contents were deliberately not revalidated by this quota-bounded smoke
test.

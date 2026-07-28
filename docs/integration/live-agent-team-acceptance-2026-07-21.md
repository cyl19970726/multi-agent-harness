# Live Codex and Kimi Agent Team acceptance — 2026-07-21

```text
status: accepted live-provider evidence
owner_role: execution-foundation
canonical_for: first native Codex + Kimi AgentTeamRun acceptance and interrupted-attempt recovery
```

> Historical implementation evidence: this run predates ADR 0032 and therefore
> includes provider-derived MemberAction/TeamRunEvent mirrors. It remains valid
> evidence for transport, correlation, interruption recovery, and gates, but it
> is not the target storage contract. The 2026-07-22 acceptance below replaces
> its storage claim with native-session reads and no mirrored provider history.

## 2026-07-28 persistent lifecycle and Workspace addendum

Mission `mission-1785227648994-p85779-0` revalidated the current persistent
Team modes after the single-execution-driver change.

| Provider mode | TeamRun / MemberRun | Native session | Accepted facts |
| --- | --- | --- | --- |
| Codex `codex_app_server` 0.145.0 | `team-run-1785228644132-p55579-0` / `member-run-1785228644132-p55579-1` | `019fa7eb-9d46-7160-b2d9-a894c66c906d` | two Host rounds on one thread, host-driven execution, explicit Host close |
| Claude `claude_agent_sdk` 2.1.220 | `team-run-1785230417407-p72711-0` / `member-run-1785230417407-p72711-1` | `ec91628d-a514-4d40-ae9c-7f73ecf3c40f` | exact `HARNESS_BIN`, correct project/store resolution, handoff, second-round continuation, explicit Host close, SDK `listSessions` discovery |
| Kimi `kimi_acp` 0.29.1 | `team-run-1785230571586-p78529-0` / `member-run-1785230571586-p78529-1` | none | requested `k2.5` rejected before bind because that alias is absent from operator configuration |

The Kimi attempt is blocked evidence, not an accepted provider canary. Harness
did not switch to another configured model, edit `~/.kimi-code/config.toml`, or
change the installed provider version without Human approval.

The Claude native file contains 36 JSONL records and the installed Agent SDK's
`listSessions({dir})` resolves the exact id, cwd, branch, first prompt, and last
summary. Claude's documented product boundary says Agent SDK sessions do not
enter the Claude Desktop/session-picker history. This acceptance therefore
claims provider-native storage and resume, not Desktop sidebar visibility.

The live run also exposed and fixed an execution-root edge case: a `serve`
started from an unregistered external worktree must retain its exact
`ProjectContext`; nested Member commands use `HARNESS_PROJECT` as an executable
root selector and `HARNESS_PROJECT_ID` as identity. Members invoke the Host's
exact `HARNESS_BIN`, not a stale binary found on `PATH`.

One correlation defect remains deliberately assigned to the following Wave:
second-round handoff causation must name the triggering follow-up message rather
than always naming the initial Assignment. The lifecycle canary is accepted
without pretending that message-lineage defect is complete.

## 2026-07-22 provider-native storage acceptance

The post-ADR 0032 acceptance is Mission
`mission-1784634958783-p62756-0`, Wave
`wave-1784664060823-p62885-0`, accepted TeamRun
`team-run-1784664071054-p64939-0`.

Two real members completed one bounded, tool-free assignment:

| MemberRun | Reviewed provider/model | Native session | Result |
| --- | --- | --- | --- |
| `member-run-1784664071054-p64939-1` | Codex `0.145.0-alpha.18` / `gpt-5.6-sol` | `019f8644-bfc8-7912-beb3-00ce0d15cb0d` | completed |
| `member-run-1784664071054-p64939-2` | Kimi `0.27.0` / `kimi-code/kimi-for-coding` | `session_cdb47f15-3a65-40ad-a6a6-b71db69b89c5` | completed |

Each assignment has its own correlation id and each handoff reuses that
correlation while naming the assignment as causation. Both `NativeSessionRef`s
are `available` and the on-demand native-activity API reconstructed two items
from each provider-owned store. The project Store contains no
`provider_sessions.jsonl`, `provider_turn_events.jsonl`, `provider-sessions/`,
provider stdout/stderr mirror, or Harness NDJSON transcript.

Kimi `k2.5` was not present in the operator's installed model configuration.
The acceptance therefore used the configured low coding tier
`kimi-code/kimi-for-coding`; it does not claim a K2.5 run.

A separate preserved three-provider attempt,
`team-run-1784663785080-p43197-0`, proved Codex and Kimi again and reached a
real Claude `2.1.181` native session. Claude generation was blocked by the
operator's expired OAuth token (`401`), so Wave
`wave-1784663768283-p32736-0` is explicitly `blocked`, not accepted. The adapter
now preserves Claude's native session locator and provider error even on this
failure path; deterministic tests cover successful Claude native read/resume
and failure behavior without transcript mirroring.

## 2026-07-22 Codex app-server live control addendum

The interactive adapter was subsequently verified against installed
`codex-cli 0.145.0-alpha.18` with native records:

- Mission `mission-1784651480593-p38526-0`;
- Wave `wave-1784651488050-p39605-0`;
- accepted TeamRun `team-run-1784651499664-p38249-0`;
- MemberRun `member-run-1784651499664-p38249-1`;
- provider thread `019f8584-f91d-7b61-9945-26b6780bfa95`.

The member ran a real app-server turn, received an operator message through
`turn/steer`, emitted native structured command activity and a correlated final
Harness handoff, and reached `completed`. The accepted Wave and closed Mission
name the attempt. Reasoning was eligible only for transient live SSE and no
thinking row was written. This addendum proves live steer for the reviewed
installed version; deterministic tests separately cover `AskUserQuestion`
resume and Codex/Kimi cooperative interruption.

## Post-acceptance provider-version audit

On 2026-07-22 the installed Codex CLI had advanced to
`0.145.0-alpha.27`, while the reviewed live acceptance above remains pinned to
`0.145.0-alpha.18`. `harness member providers --fail-on-review` therefore
reported Codex as `review_required`, exactly as ADR 0031 requires. This does not
invalidate the historical run, and it is not permission to add alpha.27 to the
reviewed set without mode-specific protocol and live acceptance. Installed
Claude `2.1.181` and Kimi `0.27.0` still probed as `current` in the same audit.

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
- Codex and Kimi MemberRuns have real provider-native session identifiers and terminal
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

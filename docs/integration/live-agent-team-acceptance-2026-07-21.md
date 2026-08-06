# Live Codex and Kimi Agent Team acceptance — 2026-07-21

```text
status: accepted live-provider evidence
owner_role: execution-foundation
canonical_for: first native Codex + Kimi AgentTeamRun acceptance and interrupted-attempt recovery
```

> Historical implementation evidence: this run predates ADR 0032 and ADR 0050
> and therefore
> includes provider-derived MemberAction/TeamRunEvent mirrors. It remains valid
> evidence for transport, correlation, interruption recovery, and gates, but
> its Assignment/Handoff records are not the current responsibility or
> acceptance contract. The 2026-07-22 acceptance below replaces
> its storage claim with native-session reads and no mirrored provider history.

## 2026-07-30 consecutive persistent-Team release acceptance

Mission `mission-firm-dogfood-20260729-v1` ran the canonical
[Agent Team Dogfood Loop](../product/agent-team-dogfood-loop.md) against the
trusted-development profile. The accepted live execution tree used Company
Store `agent-company`, Execution Space `firm-dogfood`, Project Binding
`multi-agent-harness`, and repository integration HEAD `1895f5f` before the
final clean rebase onto `master`.

The first attempt was deliberately rejected rather than normalized:

- Wave 9 `wave-1785348433136-p52326-0` and TeamRun
  `team-run-1785348596155-p55046-0` exposed two release-blocking defects:
  Codex 0.145.0 returns the selected model at `/result/model`, while the
  adapter read only the older nested response; and the Company projection
  treated append-only TeamMessage revisions as separate logical assignments.
- Repair Wave 10 `wave-1785349130290-p17767-0` fixed both boundaries and passed
  the focused Rust suites, repository checks, and
  `acceptance:mission-wave`.
- The failed TeamRun and its native provider sessions remain preserved. It was
  not counted as a green run.

Two fresh matrices then passed consecutively with no intervening repair:

| Accepted Wave | TeamRun | Codex native session | Claude native session | Kimi native session |
| --- | --- | --- | --- | --- |
| `wave-1785349414671-p30876-0` | `team-run-1785349414903-p30865-0` | `019faf1e-a9f8-7ea1-bf5d-4d84adfe280c` | `c17d30f8-70d4-4938-addb-392f6dd45229` | `session_c7190a7e-a17a-4fd0-af2f-edd648072b26` |
| `wave-1785350178229-p29071-0` | `team-run-1785350184788-p29140-0` | `019faf2a-50a7-7da1-821b-ff083fb8002d` | `4378478d-64e6-4681-a057-9b6c9f07636c` | `session_3cecbab1-9013-443c-8236-1dc8c887bf86` |

Both matrices proved the following from Harness records and the matching
provider-owned sessions:

- Codex `codex_app_server` 0.145.0, Claude `claude_agent_sdk` 2.1.220,
  and Kimi `kimi_acp` 0.29.1 each bound a fresh native session. Kimi remains
  `review_required` because its reviewed-version set still names 0.27.0; the
  live result does not silently promote the adapter.
- Each Member sent correlated Host progress and one Peer-ring message.
  Claude accepted ordinary busy mail in-turn with an SDK receipt. Kimi kept
  store-visible mail queued until a later native prompt, then delivered the
  four-message batch with one terminal `kimi-acp-prompt:4` receipt boundary.
- Host issued a real Codex Steer and Interrupt against one app-server turn.
  Bounded native-session forensics found the Steer content, the matching
  `turn_aborted`, and a later Host follow-up on the same Codex thread.
- Host stopped the generation-1 Supervisor only after all Members were idle.
  The expired lease became non-current without closing a Member. A
  generation-2 Supervisor resumed the exact same MemberRuns and native session
  ids, delivered one new correlated message to each provider, and received a
  fresh Handoff from each.
- Kimi prompt receipt counters restart with a new Supervisor transport. The
  receipt is therefore a terminal fact for that delivery claim, not a
  cross-Supervisor globally increasing sequence. Native session identity plus
  Supervisor generation proves continuity.
- The linked `agent-wcw-development` Standing Agent projected exactly one
  logical Assignment in each accepted TeamRun. Its mailbox count equalled the
  distinct addressed TeamMessage ids (`6/6` after each restart), with no
  identity conflict or duplicate physical-revision row.
- Host acknowledged every Host-bound message, explicitly closed all three
  runtimes, observed all MemberRuns reach `stopped`, completed each TeamRun,
  and only then accepted the Wave. Wave or Team completion itself did not
  imply Close.

No P0/P1 defect remains open from this matrix. Two lower-risk findings are
tracked with owner and exact retest conditions:

- [#266](https://github.com/cyl19970726/multi-agent-harness/issues/266)
  is a P2 test-infrastructure race in the bind/drop/spawn free-port helper.
  The serialized integration test and acceptance suite are green; acceptance
  requires two default-parallel suite passes plus a concurrent-spawn stress
  probe after a race-free handoff is implemented.
- [#267](https://github.com/cyl19970726/multi-agent-harness/issues/267)
  is a P2 terminal-delivery projection gap: mail for a Member that fails before
  native binding stays durable but remains incorrectly actionable. Its owner
  is Agent Team runtime/Store delivery; acceptance requires CLI, Dashboard and
  `--all` to agree on a non-actionable terminal state without deleting history.

The bounded forensic review found no tool failure, timeout, instrument
forking, repository write, or hidden transcript mirror in either accepted
Codex lane. It did expose three reusable review rules: a provider Handoff is
not Host acceptance; `inbox --all` visibility is not delivery; and an API
`interrupt_requested` result must be paired with the provider-native terminal
event before it is accepted as an interrupt.

### Post-clippy representation canary

Repair Wave `wave-1785351218971-p10701-0` removed the final
`clippy::large-enum-variant` release blocker by boxing only the
`CompanyActor::Agent(StandingAgent)` payload. It did not add an allow-list
exception or change the serialized Standing Agent contract. Focused Store,
Company execution-link, CLI, Dashboard, documentation-governance and Plugin
parity checks passed, followed by the complete CLI suite, `pnpm check`,
`acceptance:mission-wave`, and `cargo clippy --all-targets -- -D warnings`.

A fresh read-only three-provider canary then ran on integration HEAD
`d4dc461554ef18a2b4c7fca9e02ef42e3634aa2b`:

| TeamRun / MemberRun | Provider mode | Native session |
| --- | --- | --- |
| `team-run-1785351412922-p21722-0` / `member-run-1785351412922-p21722-1` | Codex `codex_app_server` | `019faf3c-f5ba-7d80-aeb9-8ede919f907f` |
| `team-run-1785351412922-p21722-0` / `member-run-1785351412922-p21722-2` | Claude `claude_agent_sdk` | `d9dbb6b0-0b58-489f-b1d0-3d3325a4ef16` |
| `team-run-1785351412922-p21722-0` / `member-run-1785351412922-p21722-3` | Kimi `kimi_acp` | `session_86a1fdf0-565c-4a70-b3cd-3202338733a2` |

Each Member sent correlated Host progress, a Peer message, and a terminal
Handoff. Terminal provider receipts proved the Peer ring in all directions;
store visibility alone was not counted as delivery. The linked
`agent-wcw-development` actor retained its `execution_agent_member_ref`, role,
capabilities, permissions, membership and compact Organization projection.
The projection contained exactly one Standing Assignment, three distinct
addressed messages and no identity conflict. The Host acknowledged all eight
Host-bound messages, explicitly closed each idle runtime, observed all three
MemberRuns reach `stopped`, and only then completed the TeamRun.

## 2026-07-28 persistent lifecycle and Workspace addendum

Mission `mission-1785227648994-p85779-0` revalidated the current persistent
Team modes after the single-execution-driver change.

| Provider mode | TeamRun / MemberRun | Native session | Accepted facts |
| --- | --- | --- | --- |
| Codex `codex_app_server` 0.145.0 | `team-run-1785228644132-p55579-0` / `member-run-1785228644132-p55579-1` | `019fa7eb-9d46-7160-b2d9-a894c66c906d` | two Host rounds on one thread, host-driven execution, explicit Host close |
| Claude `claude_agent_sdk` 2.1.220 | `team-run-1785230417407-p72711-0` / `member-run-1785230417407-p72711-1` | `ec91628d-a514-4d40-ae9c-7f73ecf3c40f` | exact `FIRM_BIN`, correct project/store resolution, handoff, second-round continuation, explicit Host close, SDK `listSessions` discovery |
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
`ProjectContext`; nested Member commands use `FIRM_PROJECT` as an executable
root selector and `FIRM_PROJECT_ID` as identity. Members invoke the Host's
exact `FIRM_BIN`, not a stale binary found on `PATH`.

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
`0.145.0-alpha.18`. `firm member providers --fail-on-review` therefore
reported Codex as `review_required`, exactly as ADR 0031 requires. This does not
invalidate the historical run, and it is not permission to add alpha.27 to the
reviewed set without mode-specific protocol and live acceptance. Installed
Claude `2.1.181` and Kimi `0.27.0` still probed as `current` in the same audit.

## Scope

This record proves real provider transport and native Store reconstruction. It
does not claim that a deterministic fixture is live evidence, that assignment
receipt validates file contents, or that the Firm controls provider-native
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
- Both initial-round handoffs name their originating assignment as
  `causation_id` and reuse its `correlation_id`. Persistent multi-round adapters
  now keep that correlation while later handoffs name the exact follow-up
  TeamMessage that triggered their round.
- Dashboard snapshot joins Mission, Wave, selected TeamRun, both MemberRuns,
  assignments, handoffs, and MemberActions.
- No `thinking`, `thinking_preview`, or provider `reasoning` field occurs in
  `team_messages.jsonl`, `member_actions.jsonl`, `team_run_events.jsonl`, or
  `member_runs.jsonl`.

## Persistent lineage canary — 2026-07-28

Wave 4 added one read-only Codex app-server canary after deterministic
lineage tests:

- TeamRun `team-run-1785234011645-p94238-0`;
- MemberRun `member-run-1785234011646-p94238-1`;
- native thread `019fa83d-59bb-7922-91e1-9ae69352282a`;
- Assignment correlation `corr-1785234011670-p94238-5`; and
- Host follow-up `tmsg-1785234060005-p96204-0`.

The same native thread completed two rounds. The first explicit Handoff points
to the Assignment; the second points to the exact Host follow-up while keeping
the Assignment correlation. Exactly two Handoffs remain. The Member had used
the collaboration CLI to submit each one, so the Adapter correctly treated
those explicit records as authoritative and did not append duplicate copies of
the final provider replies. The Host then explicitly stopped the idle member
and completed the TeamRun.

An earlier exploratory run,
`team-run-1785233710585-p85372-0`, is preserved as cancelled evidence: it
revealed the former duplicate-Handoff behavior and was intentionally stopped
before the replacement canary verified the fix.

## Standing Agent identity canary — 2026-07-28

Wave 5 verified the explicit Organization-to-execution identity join without
collapsing their lifecycles:

- Company OS StandingAgent and reusable AgentMember
  `agent-org-runtime-dogfood`;
- Mission-linked reusable team `team-org-runtime-dogfood`;
- TeamRun `team-run-1785235941106-p35827-0`;
- MemberRun `member-run-1785235941106-p35827-1`;
- Assignment correlation `corr-1785235941131-p35827-5`;
- acknowledged Handoff `tmsg-1785235997733-p37319-0`; and
- Codex app-server thread `019fa85b-0b68-7270-aab4-e09dc01fbb3c` on reviewed
  Codex `0.145.0`.

Creating the TeamRun from its independent Team definition automatically
preserved `MemberRun.agent_member_id`. Before provider execution, the Company
OS snapshot already projected the exact Assignment. After the first provider
turn, the same projection included the idle MemberRun, native-session locator,
Mission, TeamRun, correlation, and Team/Member navigation target. The Member
submitted exactly one explicit correlated Handoff, the Host acknowledged it,
and the runtime remained idle and resumable until the Host sent an explicit
Close through the same supervising service. Only then did the MemberRun become
`stopped`; the Host subsequently completed the TeamRun.

The canary was read-only. Organization availability remained its declared
`available` value throughout and was not derived from `running`, `idle`, or
`stopped`. No membership or authority row changed when execution completed.
Unlinked MemberRuns remain absent from the Standing Agent projection.

## Acceptance boundary

This proves Codex exec transport, Kimi ACP transport, native attempt lineage,
assignment/handoff correlation, transitional durable action projection, interrupted-run
recovery, Dashboard projection, and the non-persistence of thinking. The
evidence references named by the members were assignment-provided references;
their contents were deliberately not revalidated by this quota-bounded smoke
test.

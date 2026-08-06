# Company OS Foundation Builders Host takeover

Date: 2026-08-05

Mission: `mission-company-os-recursive-implementation-v1`

Team: `team-company-os-foundation-builders-v1`

Recovered TeamRun: `team-run-1785854370624-p96105-0`

Integration branch: `codex/company-os-recursive-implementation-v1`

## Recovery decision

The Host rebound the TeamRun from Codex task
`019fac11-56f5-7cd1-9f67-d6d3d8406d4b` to
`019fcd94-aada-70d3-843e-4fb88abbb741`, then attempted to resume all five
Kimi ACP MemberRuns against their existing provider-native sessions.
Supervisor generations 2 and 3 both reached a new provider request but
received no provider response. A separate `qwen/qwen3.8-max` canary also
produced no output with all Team runtimes stopped. The runtimes were therefore
closed without deleting their MemberRuns, native session bindings, Works,
branches, or dirty worktrees.

The resumed sessions are historical execution evidence. They are not evidence
that the residual implementation ran. The replacement Core, Runtime, Docs, and
UI workers were Host-native implementation aids. Each used session forensics to
find the predecessor continuation point and did not impersonate the closed
MemberRun.

## Preserved and integrated commit chain

The independent conflict review proved that the four tip commits were not all
standalone. The Host integrated the complete dependency chains in this order:

1. UI contract and implementation: `55edf10`, `44be4a4`
2. Recursive topology, delivery recovery, and Host attention foundation:
   `d4bfeae`, `f55990a`, `6435d12`
3. Work lifecycle test foundation and persistent Team-scoped Work:
   `e26926a`, `da287b1`
4. Durable AgentMember identity and root Lead bootstrap: `7dda6de`
5. Durable identity Dashboard projection: `d96cc9b`
6. Work-to-Host-attention integration and migration safety: `e12efd6`

The two textual conflicts in `crates/firm-store/src/lib.rs` were resolved by
preserving the complete import union and both Runtime and identity test blocks.

## Accepted implementation boundaries

- `Work.team_id` is durable Team responsibility. `team_run_id` is the current
  execution attempt, while creator, owner, and source provenance remain stable.
- Team scope promotion and same-Team execution retarget are explicit guarded
  transitions through the one Work operation/event/delivery funnel.
- Submitted and Blocked Work derive deterministic Host attention from that same
  funnel without manufacturing Team messages.
- Host attention uses an unlocked helper while the store lock is held. A
  deterministic reconciliation path repairs the gap between the Work event
  fsync and attention fsync without duplicate rows.
- An unresolved attention bound to the old TeamRun prevents Work retarget until
  the exact old Host delivery is acknowledged.
- `host_attentions.jsonl` is copied and byte-verified during Execution Space
  migration.
- DurableAgentMember is distinct from MemberRun and provider-native Session.
  Root Lead authority is explicit and compatibility convergence refuses
  ambiguity.
- Dashboard organization reads durable identity without treating runtime state
  as identity. Durable identity wins over a same-id compatibility row; Team
  Host authority comes from `AgentTeam.host_member_id`, with a legacy TeamRun
  fallback only when the durable field is absent.
- Recursive Organization and Team Works use execution snapshot truth and expose
  unavailable delegation and Docs handoff capabilities honestly.

## Verification

- `cargo fmt --all -- --check`
- `git diff --check`
- `cargo test -p firm-core`
- `cargo test -p firm-store --test team_work_delivery_lifecycle`
- `cargo test -p firm-store`
- focused topology, Codex receipt, identity, execution-space migration, and
  snapshot tests
- schema fixtures: 40 valid and 40 invalid
- `pnpm check:dashboard`, including type checks, browser/a11y checks, four
  responsive viewports, and production build
- `cargo test --workspace -- --test-threads=1`: all suites passed
- `pnpm acceptance:mission-wave`: passed, including MCP 3/3, Mission/Wave 4/4,
  TeamRun API 39/39, TeamRun start 10 passed with 2 documented historical
  ignores, and the Dashboard gate

## Wave 2 Host-native completion

The four carried-forward control-plane Works were implemented by new
Host-native workers after session-forensics handoff. Their commits are separate
from the closed Kimi MemberRuns and retain replacement-worker provenance:

- runtime invalidation and scoped live convergence: `e7b3c19`
- Dashboard bounded self-healing and stale-response exclusion: `2b6666a`
- Kimi empty-round circuit breaking and model-control validation: `a4317f9`

The Runtime now emits scoped invalidation for snapshot-visible execution
ledgers and selected Company Store ledgers, including append, torn-write,
truncate, and atomic-replace cases. Company invalidation fans out only to
Execution Spaces bound to the same Company; an unknown explicit Space fails
closed. The subscription is established before the initial marker so the
marker-to-GET window does not lose invalidation.

The Dashboard treats invalidation as a GET hint rather than source truth. It
coalesces dirty scopes, prevents delayed bounded Team responses from
overwriting a newer full Company view, recovers stale Space and Company
selectors independently, and resynchronizes on reconnect, visibility regain,
online regain, quiet-open streams, and generation changes.

The provider loop stops after three consecutive rounds with no durable output.
An empty terminal action is recorded as `empty_provider_round`, capacity stays
unknown, active Work remains in progress, and already received delivery
evidence is retained rather than replayed. A real durable output resets the
circuit. A model change accepts only controls valid for the new model and does
not silently inherit stale effective settings.

Independent audit found no remaining P0 or P1 live-convergence gap. The full
integration gate passed after one existing persistent-Codex timing test first
observed one provider round instead of two; the exact isolated rerun passed,
and a subsequent complete `acceptance:mission-wave` run passed all gates.

During the read-only audit, one helper omitted the intended temporary selector
and accidentally created empty TeamRun `team-run-1785865864775-p42963-0` with
MemberRun `member-run-1785865864775-p42963-1`. It has no Work, assignment,
native session, or execution claim. The Host cancelled it and retained the
ledger row as honest audit evidence.

## Remaining explicit risks

The persistent Work cutover currently validates an independently read Company
Store while holding the execution-store lock. That staged design has a
cross-store time-of-check/time-of-use window. A production cutover needs an
external migration fence or transaction protocol plus a concurrency test; the
current validator must not be represented as cross-store atomicity.

The accepted live-convergence slice intentionally leaves these P2 boundaries:

- externally creating a new Company does not live-refresh the Company picker;
- selecting a Company changes the global active Company and can affect other
  tabs or CLI clients;
- SSE has no durable cursor or `Last-Event-ID`; a scoped full snapshot is the
  safety strategy after reconnect;
- same-size typed-delta atomic replacement still depends on the older tailer;
- direct ledger deletion does not immediately emit invalidation;
- a single combined real-runtime SSE plus real-browser in-process test is still
  absent;
- freshness is shown globally rather than per projection domain.

The original Kimi-owned Work rows remain evidence of their original
assignments. Host takeover commits and checks are recorded here and in the Wave
outcome instead of being falsely attributed to those closed MemberRuns.

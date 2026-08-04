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

The two textual conflicts in `crates/harness-store/src/lib.rs` were resolved by
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
- `cargo test -p harness-core`
- `cargo test -p harness-store --test team_work_delivery_lifecycle`
- `cargo test -p harness-store`
- focused topology, Codex receipt, identity, execution-space migration, and
  snapshot tests
- schema fixtures: 40 valid and 40 invalid
- `pnpm check:dashboard`, including type checks, browser/a11y checks, four
  responsive viewports, and production build
- `cargo test --workspace`: all suites passed except one parallel-only Kimi
  prompt timing timeout in `team_run_api`; the complete file then passed 36/36
  with `--test-threads=1`
- `pnpm acceptance:mission-wave`: passed, including MCP 3/3, Mission/Wave 4/4,
  TeamRun API 36/36, TeamRun start 10 passed with 2 documented historical
  ignores, and the Dashboard gate

## Remaining work and explicit risks

The following recovered Works were not implemented by this takeover slice and
must remain visible for a later Wave or successor TeamRun:

- external Work and Company Store live invalidation/SSE coverage
- bounded Dashboard resync after missed events, reconnect, visibility regain,
  or a stale-open stream
- independent live-projection coverage audit
- provider zero-output/quota circuit breaking and model-control validation

The persistent Work cutover currently validates an independently read Company
Store while holding the execution-store lock. That staged design has a
cross-store time-of-check/time-of-use window. A production cutover needs an
external migration fence or transaction protocol plus a concurrency test; the
current validator must not be represented as cross-store atomicity.

The original Kimi-owned Work rows remain evidence of their original
assignments. Host takeover commits and checks are recorded here and in the Wave
outcome instead of being falsely attributed to those closed MemberRuns.

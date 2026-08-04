# Company OS Recursive Organization — Live Cross-Branch Conflict Map & Integration Order

Review lane: `IndependentArchitectureReviewer` (read-only integration review).
TeamRun: `team-run-1785854370624-p96105-0` · Work: `work-1785855218448-p84955-2` v2.
Snapshot time: 2026-08-04 ~22:55 +0800. Supersedes the unfinished map from
`work-company-os-integration-conflict-map-v1` (TeamRun `team-run-1785845092639-p58829-0`,
started but never submitted; that session is historical evidence only).

## Fixed reference points

| Ref | Commit | Meaning |
|---|---|---|
| Frozen integration base (PR 302 HEAD) | `7296354` | `origin/codex/nested-org-agent-teams-spec-v1`, tip = docs proposals for recursive org |
| `origin/master` | `52c0864` | PR 301 merged; PR 302 not yet merged |
| Shared lane base above PR 302 | `b9e91cd` | `feat(agent-team): add recursive topology foundation (ADR 0051)`, parent `7296354` |

`b9e91cd` is contained in Core, Docs, and Runtime branches and is the correct
merge base between them; it contributes no cross-lane conflicts.

## Member commit ledger

| Lane | Member (run) | Worktree | Branch | Tip | Unique commits vs base | Status |
|---|---|---|---|---|---|---|
| Core | CoreKernelBuilder (`…-p96105-1`) | `company-os-core-kernel-v1` | `codex/team-delivery-recovery-v1` | `c9a8f8a` | `c9a8f8a` fix(agent-team): recover stranded WorkDelivery claims and align codex receipt timing (on `b9e91cd`) | committed; current Host-attention Work not yet started in files (clean tree) |
| Docs | DocsInfrastructureBuilder (`…-p96105-2`) | `company-os-docs-infrastructure-v1` | `codex/company-os-agent-member-identity-v1` | `b9e91cd` | none yet | in_progress, no unique commits, clean tree |
| Runtime | RuntimeDeliveryBuilder (`…-p96105-3`) | `company-os-runtime-delivery-v1` | `codex/company-os-runtime-delivery-v1` | `e3d9eef` | `e3d9eef` test(agent-team): pin WorkDelivery busy/close/reopen/retire lifecycle (on `b9e91cd`) | committed + uncommitted in-flight edits |
| UI | CompanyOSUXBuilder (`…-p96105-4`) | `company-os-ui-contract-v1` | `codex/company-os-ui-contract-v1` | `e2e4f94` | `e2e4f94` docs: contract recursive Org, Docs, and Works UI (on `7296354`, does NOT contain `b9e91cd`) | committed + uncommitted in-flight edits |
| Review | IndependentArchitectureReviewer (`…-p96105-5`) | `company-os-independent-review-v1` | `codex/company-os-independent-review-v1` | `7296354` | n/a (this document) | read-only lane |

Unused: `codex/company-os-recursive-implementation-v1` (worktree
`company-os-recursive-implementation-v1`) is clean at `7296354` with zero
unique commits — no member currently owns it.

## In-flight (uncommitted) snapshots

Verified via non-mutating `git stash create` snapshots merged with `git merge-tree`.

Runtime (snapshot `cdff842`, 48 insertions):
- `crates/harness-cli/src/main.rs` +2 lines each in `create_team_run` (~11794), `add_team_run_member` (~11923), `team_run_work_command` (~13341), `create_team_work_value` (~23646)
- `crates/harness-core/src/lib.rs` +34 in `pub struct Work` / `impl Work` / `WorkEventKind` (3070–3183) + 2 in tests
- `crates/harness-store/src/lib.rs` +2 in `mod tests` (~5318)
- `crates/harness-cli/src/company_os_api.rs` +2 in `projection_tests`
- `crates/harness-store/tests/team_work_delivery_lifecycle.rs` +2

UI (snapshot `f93c3e7`):
- `apps/agent-dashboard/src/app/selection.ts` +27, `apps/agent-dashboard/src/types.ts` +12
- untracked (not in snapshot, new files): `src/model/orgSelectors.ts` (234 ln), `src/model/teamWorksSelectors.ts` (180 ln)

Core and Docs worktrees: clean.

## File/hunk ownership matrix (vs `7296354`)

| File | Core | Docs | Runtime | UI |
|---|---|---|---|---|
| `crates/harness-cli/src/main.rs` | b9e91cd 66ln + c9a8f8a 15893–23085, 38900+ | b9e91cd 66ln (shared) | b9e91cd 66ln (shared) + in-flight 11794/11923/13341/23646 | — |
| `crates/harness-core/src/lib.rs` | b9e91cd +337 (shared) | shared | shared + in-flight Work struct 3070–3183 | — |
| `crates/harness-store/src/lib.rs` | b9e91cd +218 (shared) + c9a8f8a 1444, 6539 | shared | shared + in-flight tests 5318 | — |
| `crates/harness-cli/tests/team_topology.rs` | shared (new) | shared | shared | — |
| `crates/harness-cli/tests/codex_work_receipt.rs` | c9a8f8a (new) | — | — | — |
| `crates/harness-cli/tests/fake_provider/mod.rs` | c9a8f8a | — | — | — |
| `crates/harness-store/tests/team_work_delivery_lifecycle.rs` | — | — | e3d9eef (new, 650ln) + in-flight | — |
| `crates/harness-cli/src/company_os_api.rs` | — | — | in-flight | — |
| `docs/product/agent-team-works.md` | c9a8f8a | — | — | — |
| `schemas/agent-team.schema.json` + `schemas/fixtures/agent-team/*` | shared | shared | shared | — |
| `docs/design/company-os-v6/recursive-org-docs-works-v1/*` | — | — | — | e2e4f94 (exclusive) |
| `docs/registry.json` | — | — | — | e2e4f94 (exclusive) |
| `apps/agent-dashboard/src/**` | — | — | — | in-flight + untracked (exclusive) |

## Collision findings

All pairwise and sequential `git merge-tree` simulations (committed tips AND
in-flight snapshots) are CLEAN — exit 0, zero conflicting paths:

- Core × Runtime(committed), Core × UI, Runtime × UI, Docs × Core
- Core × Runtime(in-flight `cdff842`), Core × UI(in-flight `f93c3e7`), Runtime(in-flight) × UI(in-flight)
- Sequential: (Core+UI) → +Runtime(in-flight) → +Docs: clean; Docs tip is an ancestor of the merged tip.

No blocking collision exists at this snapshot. Non-blocking risks messaged to peers:

1. **Work-kernel semantic collision (Core ↔ Runtime).** Runtime is extending
   `pub struct Work` / `WorkEventKind` for Team-scoped promotion; Core's
   in-progress Host-attention Work also needs Work-transition fields. Field
   names must be agreed before either commits more of `harness-core`
   (messaged: `tmsg-1785855688906-p84114-1` to Core, `tmsg-1785855716881-p94911-1` to Runtime).
2. **main.rs adjacency (Core ↔ Runtime).** Runtime's `create_team_work_value`
   hunk (~23646) sits ~500 lines from Core's `close_team_member_value` edits
   (22894–23085). Clean today; after Core lands, Runtime must rebase onto the
   post-Core tip (line numbers shift by hundreds of lines).
3. **Anticipated Docs overlap.** Docs has no unique commits yet but its
   AgentMember-identity Work will need `harness-core`, `harness-store`,
   `main.rs`, `schemas/agent-member.schema.json` — all Core/Runtime-owned
   except the agent-member schema. Docs merges last and must announce touched
   structs before editing (messaged: `tmsg-1785855717239-p95150-1`).
4. **UI lane is fully disjoint** — docs/design, registry.json, and dashboard
   sources are touched by no other lane. No message required.

## Recommended integration order (against frozen PR 302 HEAD `7296354`)

1. **UI** `codex/company-os-ui-contract-v1` (commit its two untracked
   selector files + selection/types edits first). Zero overlap; docs+dashboard only.
2. **Core** `codex/team-delivery-recovery-v1` @ `c9a8f8a`, plus its upcoming
   Host-attention commits. Largest committed harness delta; self-contained tests.
3. **Runtime** `codex/company-os-runtime-delivery-v1` @ `e3d9eef` + in-flight
   Team-scope Work commits. **Rebase onto the post-Core tip, not onto `b9e91cd`.**
   Expected: no textual conflicts at today's hunks; resolve any drift in
   `main.rs`/`harness-store` tests by keeping both sides (disjoint functions).
4. **Docs** `codex/company-os-agent-member-identity-v1` last; rebase onto the
   integrated tip of 1–3 immediately before its first real commit.

Validation order after each merge step (and once at the combined tip):
1. `cargo fmt --check`
2. `cargo test -p harness-store` (team_work_delivery_lifecycle, topology fixtures)
3. `cargo test -p harness-core`
4. `cargo test -p harness-cli` (team_topology, codex_work_receipt)
5. `cargo test --workspace`
6. Dashboard: `pnpm -C apps/agent-dashboard typecheck`/build (UI step only)
7. Gate: `pnpm acceptance:mission-wave`
8. Only then merge PR 302 branch (with the integrated member work) toward master.

## Executable Host integration sequence

```bash
BASE=7296354   # frozen PR 302 HEAD
INT=codex/nested-org-agent-teams-spec-v1   # PR 302 branch == $BASE today

# 1) UI lane commits in-flight work, then:
git merge-base --is-ancestor $BASE codex/company-os-ui-contract-v1  # true today
git checkout $INT && git merge --ff-only codex/company-os-ui-contract-v1 || git merge --no-ff codex/company-os-ui-contract-v1

# 2) Core (already contains b9e91cd; verify then merge)
git merge-base --is-ancestor $BASE codex/team-delivery-recovery-v1
git merge --no-ff codex/team-delivery-recovery-v1

# 3) Runtime rebase onto integrated tip, then merge
git -C <runtime-worktree> rebase $INT      # after Runtime commits its in-flight edits
git merge --no-ff codex/company-os-runtime-delivery-v1

# 4) Docs last, same pattern once it has commits
git -C <docs-worktree> rebase $INT
git merge --no-ff codex/company-os-agent-member-identity-v1

# validation per step, then full gate
cargo fmt --check && cargo test --workspace
pnpm acceptance:mission-wave
```

Next reviewer actions: re-snapshot when Core starts its Host-attention edits
(harness-core Work struct is the watch point), and when Docs announces its
first struct touches.

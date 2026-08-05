# Company OS Wave 3 Independent Release Audit

Status: independent candidate audit complete. All four recorded P1s are closed
on committed candidates and the three implementation Works are Host-accepted.
Release integration, live projection reconciliation, remote delivery, and
post-integration gates have not been performed by this reviewer.

Reviewer: `IndependentReleaseReviewer`
(`member-run-1785898628397-p33412-4`, provider-native session
`019fcfdb-7b98-73c0-82c9-44f7f41a61e3`).

TeamRun: `team-run-1785898628238-p33412-0`.

Review Work: `work-company-os-wave3-release-audit-v1`.

Fixed Wave 3 base: `8539a336dbf2da79d3d86504191db68710930e39`.

Snapshot date: 2026-08-05 Asia/Shanghai. Remote facts were refreshed after
`origin/master` moved during this review and are bound to the hashes below.
They must be refreshed again immediately before integration and delivery.

## Independent judgment

The three Wave 3 implementation candidates pass their independently reviewed
boundaries. They are not yet a release: the exact integration sequence, live
append-only repair, final integrated matrix, fast-forward delivery, remote
readback, and new CI remain Host release gates. Four P1 findings were recorded;
all are closed on committed candidates and all ownership/closure facts are
Work-linked:

1. **Closed in `5565999e8a09367bc73c1f53801b375d57f409ee`.** The
   cancelled `review_required` Kimi 0.32 recovery run had been actively driven
   through four provider-native sessions. The candidate now freshly probes and
   refuses non-current adapters before start, HTTP Work rebind, closed-member
   reopen, Supervisor recovery, delivery claim, process start, or native attach.
   Owner: Runtime Protocol replacement
   `member-run-1785899073688-p33412-16`. Finding messages:
   `tmsg-1785899093745-p4171-1`, `tmsg-1785899217429-p97508-1`;
   independent closure: `tmsg-1785901505292-p52268-1` and
   `tmsg-1785901513960-p58077-1`; exact-hash correction:
   `tmsg-1785901663023-p55698-1`.
2. **Closed in `3b269d4bc3e9625562eee8bb5768b1a4cd1bf344`.** The live
   Host HTTP rebound used to replace both failed Claude Wave 3
   members erased `Work.team_id` from the complete WorkOperation projections.
   Owner: AtomicityBuilder `member-run-1785898628239-p33412-1`. Messages:
   `tmsg-1785899266100-p28994-1`, `tmsg-1785899266800-p29724-1`;
   independent closure recommendation: `tmsg-1785901127960-p89286-1`.
3. **Closed in `5ca256355dc1566726f718c8d49368071c5ecfc3`.** The first
   live-browser candidate checked the server Company default only
   after selecting the already-default Company A, then switched to Company B
   without a readback. That assertion could pass while a non-default page
   selection still mutated the CLI/global default. Owner: DashboardE2EBuilder
   `member-run-1785899074195-p33412-18`. Message:
   `tmsg-1785900252970-p51874-1`; resolution:
   `tmsg-1785900625813-p13828-0`. Independent rerun of
   `pnpm check:dashboard:runtime-e2e` passed all 17 checks, including the
   strengthened Company B -> server-default Company A assertion.
4. **Closed in `3b269d4bc3e9625562eee8bb5768b1a4cd1bf344`.** The first
   cutover-fence draft claimed neither side is mutable after a crash
   between the Company fence and `TeamScopePromoted`, but the source-linked
   compatibility Work can still advance after the process releases both
   locks. Company ownership is frozen, so the state is non-dual, but an
   intervening Work version can strand the recorded intended promotion. Owner:
   AtomicityBuilder `member-run-1785898628239-p33412-1`. Message:
   `tmsg-1785900416858-p62518-1`; causal review pass:
   `tmsg-1785901127950-p89275-0`.

There is no open or unowned P0/P1 at this snapshot. Candidate closure is not
final integrated acceptance; every causal check must be rerun after conflict
resolution at the integration hash.

## Remote and ancestry baseline

Read-only remote inspection produced:

| Ref | Exact object | Finding |
| --- | --- | --- |
| `origin/master` | `a2fc58b3a446c7814d555e16990d443ff1dd8a04` | Current remote master from the final `git ls-remote`; it moved during review from `5b56f86`. |
| PR 302 head | `729635451d73c24710cd1c6138cc04713495bff6` | Open PR `codex/nested-org-agent-teams-spec-v1` -> `master`; CI rows shown by GitHub are green but predate Wave 3. |
| PR 302 merge ref | `ec7ac8d97df7150e7f7579a2b0bc4c3b3c31dc6d` | Stale test merge of `7296354` into old base `52c0864`, not current master. GitHub reports mergeability/merge-state `UNKNOWN`. |
| Wave 3 base | `8539a336dbf2da79d3d86504191db68710930e39` | Contains PR 302 head as an exact ancestor. |

`7296354` is an ancestor of `8539a33`; it is not an ancestor of current
`origin/master`. Neither `8539a33` nor `origin/master` is an ancestor of the
other. Their merge base is `52c086467360b26b66b920158d3daa671326b7a9`.

Current master adds six commits beyond the shared base:

- `2f9405e`: ADR 0051 single-intent spine and Host scheduling skill changes;
- `6dc7c90`: decision-shaped Work board reads and MCP/API tests;
- `6617929`: Runtime/Dashboard build provenance surface;
- `5b56f86`: recovery rebind of runs and Works without recreation;
- `1eab685`: retired Goal-stack removal under ADRs 0028/0051; and
- `a2fc58b`: CLI cheatsheet and anti-drift coverage.

The Wave 3 base adds the PR 302 ancestry plus Company OS implementation commits
through `8539a33`. It does not contain those six master commits.

### Baseline merge simulation

`git merge-tree --write-tree 8539a33 a2fc58b` reports one textual
conflict only: `docs/decisions/README.md`. The release resolution must retain
both the implemented nested-Team ADR 0051 history and current master's
single-intent-spine ADR entry; deleting either side would falsify history.

The single textual conflict does not imply low semantic risk. Current master
overlaps Wave 3 ownership in:

- `crates/harness-cli/src/main.rs`, `company_os_api.rs`, `mcp.rs`, and tests;
- `crates/harness-core/src/lib.rs` and `crates/harness-store/src/lib.rs`;
- Runtime/SSE and recovery tests;
- Dashboard `api.ts`, `WorkbenchShell.tsx`, `types.ts`, Vite configuration,
  browser checks, and provenance UI; and
- Work/ADR/skill contracts.

Candidate simulations against exact master `a2fc58b` report:

- Atomicity `3b269d4` and Dashboard `5ca2563`: the same single textual
  `docs/decisions/README.md` conflict;
- Runtime `5565999`: two textual conflicts,
  `docs/decisions/README.md` and `crates/harness-cli/src/main.rs`; and
- Atomicity plus Dashboard, and Runtime plus Dashboard before master, merge
  textually clean.

The Runtime `main.rs` resolution is semantic, not choose-a-side: retain its
fresh provider preflight and recovery mutations (`mut members`, preflight
before mutation, and `started_at` refresh) while also retaining master's
Goal-stack deletion, `board-summary`, cheatsheet, help, and anti-drift surface.
Pairwise textual cleanliness does not remove the requirement to rerun the
causal matrix after that resolution.

## Wave 3 native-session provenance

Harness coordination records are used only for bindings, Work transitions,
and control acknowledgements. Transcript/tool/edit claims below come from the
provider-owned session files.

| MemberRun | Role | Provider/session | Execution evidence |
| --- | --- | --- | --- |
| `member-run-1785898628239-p33412-1` | AtomicityBuilder | Codex 0.145.0 / `019fcfdb-7d8e-78a1-8b02-3e03e9823ad1` | Current reviewed Codex session; Work started v1 -> v2. Candidate `3b269d4`; independent core/store/CLI checks pass. |
| `member-run-1785898628286-p33412-2` | original RuntimeProtocolBuilder | Claude 2.1.220 / `fdaa100b-faa8-429e-8142-55d941d06a55` | 13-line native record, zero tool calls or edits; HTTP 403 before Work start; closed and preserved. |
| `member-run-1785898628333-p33412-3` | original DashboardE2EBuilder | Claude 2.1.220 / `e5b90cad-07b6-4346-b74a-737f561c6ec8` | 9-line native record, zero tool calls or edits; HTTP 403 before Work start; closed and preserved. |
| `member-run-1785898628397-p33412-4` | IndependentReleaseReviewer | Codex 0.145.0 / `019fcfdb-7b98-73c0-82c9-44f7f41a61e3` | This audit; implementation paths remain read-only. |
| `member-run-1785898854634-p33412-10` | temporary Runtime replacement | Codex 0.145.0 / `019fcfdd-f745-7e51-b921-4defcd95893f` | Created but never assigned Work; superseded and coordination-closed. One session-meta row at the first snapshot. |
| `member-run-1785898855139-p33412-12` | temporary Dashboard replacement | Codex 0.145.0 / `019fcfdd-f9b1-7c01-9a7d-8ffc62225e66` | Created but never assigned Work; superseded and coordination-closed. One session-meta row at the first snapshot. |
| `member-run-1785899073688-p33412-16` | stable Runtime replacement | Codex 0.145.0 / `019fcfe1-4dd4-7670-9710-b5217949cf1d` | Work rebound by explicit Host event v1 -> v2, started v2 -> v3, submitted v3 -> v4, and Host-accepted v4 -> v5. Candidate `5565999`. |
| `member-run-1785899074195-p33412-18` | stable Dashboard replacement | Codex 0.145.0 / `019fcfe1-50b8-7e92-9254-d23c6f3ee919` | Work rebound by explicit Host event v1 -> v2, started v2 -> v3, corrected through review, submitted with `8e23241` plus `5ca2563`, and Host-accepted at v7. |

The replacement sessions have new native ids and do not resume the failed
Claude sessions. The temporary Codex sessions have no Work authorship. The
stable replacement Work events preserve the original stable owner identity and
change only `active_member_run_id`; this is coordination provenance, not a
claim that the failed Claude sessions implemented anything.

## Cancelled Kimi 0.32 recovery run

Historical TeamRun `team-run-1785868856532-p24714-0` is cancelled. Its five
MemberRuns are closed; Supervisor generation 2 is released. The four executing
provider-native sessions are preserved and must never be rebound into Wave 3:

| Role/session suffix | Normalized recovery prompts | Execs | Edits | Edited files |
| --- | ---: | ---: | ---: | ---: |
| Core / `session_e3278c50-501c-4956-b34a-f4b9b5a6daf3` | 132 | 82 | 25 | 3 |
| Docs / `session_dcd8474a-d521-4567-8d55-7211a72ec2ff` | 80 | 102 | 11 | 5 |
| Runtime / `session_fbc72bc6-fe75-42a1-989c-ddc93defc232` | 133 | 107 | 21 | 4 |
| UI / `session_d3011e2b-3a23-4b4f-8cfe-f09c383be342` | 133 | 81 | 29 | 10 |
| Reviewer / `session_4b3557c9-abb2-4c71-a4c2-a981b53df0b8` | 0 | 0 | 0 | 0 |

These are normalized provider-native events measured from each Kimi
`wire.jsonl`, not Harness message counts. A bounded drill shows the same full
`RUNTIME RECOVERY` envelope repeatedly injected at adjacent records. The
provider version bound in Harness is Kimi 0.32.0 with compatibility status
`review_required`. Therefore the historical failure was active execution under
an unreviewed adapter, followed by repeated same-session recovery drive. The
later Host closure/cancellation is correct preservation, but does not prove a
preventive gate.

The Kimi worktrees remain evidence and are not Wave 3 implementation lanes:

- Core dirty: 1,207 insertions / 23 deletions across `main.rs`, store, and the
  plugin hook.
- Runtime dirty: 493 insertions / 1 deletion across `main.rs` and `sse.rs`.
- UI dirty: 198 insertions / 14 deletions plus untracked freshness source/test.
- Docs and independent-review worktrees are clean.

No dirty Kimi change may be copied, committed, reset, or attributed to a Wave 3
Member. Preserve the worktrees until the Host records an explicit archival
decision.

### Stopped-worktree commit classification

| Commit | Classification | Independent reason |
| --- | --- | --- |
| `ae5bfe1` | Preserve, do not integrate in Wave 3 | Clean historical commit, but it expands the compatibility/durable identity union and compatibility Team-member path. That conflicts with the current root invariant not to extend the compatibility join and is outside the three Wave 3 implementation Works. It needs a separate architecture decision if reconsidered. |
| `ed8ba90` | Superseded as release documentation | Describes the independently-read cross-store validator as the migration machinery and does not close the TOCTOU boundary that Wave 3 explicitly owns. AtomicityBuilder's accepted protocol must write the truthful replacement contract. Do not cherry-pick this commit as-is. |
| `a87ebc2` | Superseded and factually stale against `8539a33` | Claims `parent_team_id` and `host_member_id` are absent TARGET wire fields, while `8539a33` implements both in `AgentTeam`, schema, topology validation, and Dashboard projection. Do not integrate. |

This classification preserves authorship and history without assigning any of
the commits to Wave 3.

## Live Work provenance and projection closure

The four Wave 3 Works were created at version 1 and the reviewer/atomicity
Works started normally at version 2. The failed Claude Runtime and Dashboard
Works were explicitly rebound to stable Codex replacement MemberRuns, then
started by those bound runtimes.

The rebound operation exposed a new P1: both Runtime and Dashboard Works had
`team_id=team-company-os-foundation-builders-v1` and a serialized
`created_by_member_id` at v1. Their HTTP-operator v2 WorkOperation snapshots
omit both keys entirely; the current bound-runtime start then serializes both
as null at v3. The unrebound Atomicity and Audit Works retain the durable Team
id. The `rebound` event records the old/new MemberRun ids and stable owner but
no intentional Team-scope transition. This is not a display-only discrepancy
because WorkOperation persists the complete resulting Work projection. The
missing keys also make a current-version clone-only regression insufficient:
acceptance must exercise or refuse a stale/mixed-schema writer.

Atomicity candidate `3b269d4` supplies the required closure evidence:

1. deterministic regression showing rebind/rebound preserves all unrelated
   Work fields including `team_id`, creator, source, criteria, and evidence;
2. a supported append-only reconciliation/repair for the two already-affected
   Works, without editing store rows; and
3. readback by both TeamRun and durable Team scope after restart.

The reviewer used the candidate binary read-only to reconstruct both live
Works with `team_id=team-company-os-foundation-builders-v1`; the historical raw
rows remain unchanged. The Host must run the explicit versioned
`reconcile-projection` path after integration and verify both scopes. Tests do
not silently repair live state, and this reviewer did not mutate the Works.

## Live conflict map

| Lane | Worktree/branch | Current base/candidate | Master semantic overlap | State |
| --- | --- | --- | --- | --- |
| Atomicity | `company-os-wave3-atomicity` / `codex/company-os-wave3-atomicity` | `3b269d4bc3e9625562eee8bb5768b1a4cd1bf344` | `harness-core`, `harness-store`, `team_run_work_command`, and WorkOperation/rebind semantics overlap current master/PR 310. Candidate includes the authorized `reconcile-projection` CLI hunk and same-MemberRun higher-generation rebind primitive. Against `a2fc58b`, textual conflict is only `docs/decisions/README.md`. | committed; Work Host-accepted v4 |
| Runtime | `company-os-wave3-runtime` / `codex/company-os-wave3-runtime` | unique `5565999e8a09367bc73c1f53801b375d57f409ee`; dependencies `b21917b` (semantic PR 310 replay) then `d5f468f` (patch-identical Atomicity replay) | `main.rs`, SSE/provider and TeamRun tests overlap master. Against `a2fc58b`, textual conflicts are `main.rs` and `docs/decisions/README.md`. Final integration already gets PR 310 from master and Atomicity from its lane, so transplant only commits strictly above `d5f468f`. | committed; Work Host-accepted v5 |
| Dashboard | `company-os-wave3-dashboard` / `codex/company-os-wave3-dashboard` | `5ca256355dc1566726f718c8d49368071c5ecfc3` (`8e23241` implementation + `5ca2563` audit correction) | Dashboard API, Workbench, types, Vite/browser checks overlap provenance changes. Against `a2fc58b`, textual conflict is only `docs/decisions/README.md`; semantic composition with `6617929` still needs integrated checks. | committed; Work Host-accepted v7 |
| Audit | `company-os-wave3-review` / `codex/company-os-wave3-review` | this report only | `docs/research` only; merge after implementation/report resnapshot. | in progress |

All candidate hashes and exact current-master textual conflicts are frozen
above. Any remote movement invalidates this map.

## Acceptance matrix

| Boundary | Required independent proof | Snapshot result |
| --- | --- | --- |
| Cross-store cutover | Concurrent conflicting transitions are fenced/refused; retry/restart is idempotent; no dual mutable responsibility; rebound preserves Team scope. | **PASS on Atomicity candidate** — independent `harness-core` 65/65 + 13/13 and `harness-store` 54/54 + 15/15 + 4/4 pass. The one-way fence, crash/retry, crash-gap Work advance, sparse mixed-writer recovery/refusal, explicit append-only reconciliation, and higher-generation rebind tests are present. Final integrated rerun and live reconciliation remain required. |
| Provider compatibility | `review_required` cannot start, reopen/resume, recover/rebind, or rebound delivery before native drive; reviewed Codex/Claude modes still work. | **PASS on Runtime candidate** — five independent exact-tree regressions prove Kimi 0.32 refusal before ACP across start, HTTP rebound, reopen and recovery; Claude refusal before runner; reviewed recovery preserves one stable MemberRun/session and no duplicate Work. Final integrated rerun remains required. |
| Runtime recovery | Honest cursor or explicitly bounded snapshot contract; same-size replace and deletion invalidate/recover without stale scope or duplicate Work. | **PASS on Runtime candidate** — contract explicitly rejects durable cursor/`Last-Event-ID` claims and uses authoritative scoped snapshot on open/reconnect; independent replace/delete/reconnect regression passed. Final integrated rerun remains required. |
| Company selection | Tab-local selection does not mutate CLI/global Company; externally created Company appears; stale scope cannot overwrite current scope. | **PASS on Dashboard candidate** — independent real-browser rerun selected non-default B and read server default A, refreshed external C without changing tab scope, and rejected delayed B after switching to A. Final integrated rerun remains required. |
| Domain freshness | Works, Docs, Org, and Runtime/read-model freshness are independently truthful and accessible. | **PASS on Dashboard candidate** — real browser exposed one accessible scoped group and four independent domain states; Work/Docs/Org invalidations left unrelated domains live. Final integrated rerun remains required. |
| Real Runtime/browser E2E | Real in-process Runtime/SSE plus real browser; external Work, Docs, Org writes; reconnect, visibility/background, stale selector; no fixture-only substitute. | **PASS on Dashboard candidate** — `pnpm check:dashboard:runtime-e2e` independently passed 17/17. Source builds and spawns the real `harness serve`, proxies through Vite, drives Chromium, writes native isolated stores, kills/restarts Runtime, and performs no snapshot/SSE/business-row fixture injection. Final integrated rerun remains required. |
| Master/PR integration | Candidate contains current master and exact PR 302 head ancestry; merge conflicts semantically resolved; full gates at final hash. | **PENDING HOST INTEGRATION.** Exact `a2fc58b` ancestry and two-file maximum conflict map recorded; no integrated hash exists yet. |
| Remote delivery | Authorized fast-forward update, remote hash readback, PR 302 recomputed CI/merge state. | Not authorized/executed by reviewer. |

## Exact integration and delivery sequence

The release must preserve the exact PR 302 ancestry and must not force-push.
Rebasing the entire `8539a33` history onto master would rewrite PR 302's three
commits and destroy the exact-ancestor proof. Use a release integration branch
that merges master once, then rebase only the new Wave 3 lane commits.

Host sequence with all three implementation Works now committed and accepted:

```bash
BASE=8539a336dbf2da79d3d86504191db68710930e39
MASTER=a2fc58b3a446c7814d555e16990d443ff1dd8a04
PR302=729635451d73c24710cd1c6138cc04713495bff6
INT=codex/company-os-recursive-implementation-v1
RUNTIME_DEP=d5f468f07ff66af5b2b50a52b3113273633d24ff # after PR 310 + Atomicity replays

# 0. Abort if the remote moved; substitute no guessed hash.
git ls-remote origin refs/heads/master refs/heads/codex/nested-org-agent-teams-spec-v1
test "$(git rev-parse "$INT")" = "$BASE"

# 1. Preserve both histories. Resolve docs/decisions/README.md by retaining
# both truthful ADR entries, then commit the merge. The Runtime transplant later
# also conflicts in main.rs: compose provider preflight/recovery with master's
# Goal deletion, board-summary, cheatsheet and help surface; choose neither side.
git switch "$INT"
git merge --no-ff "$MASTER"

# 2. For each lane, rebase only commits above the fixed Wave 3 base onto the
# current integration tip, review conflicts, then fast-forward integrate.
# Ordered dependency: Atomicity -> Runtime -> Dashboard -> independent report.
git -C <atomicity-worktree> rebase --onto "$INT" "$BASE" codex/company-os-wave3-atomicity
git merge --ff-only codex/company-os-wave3-atomicity
# Current master already contributes PR 310 and Atomicity was integrated first.
# Exclude both Runtime-lane dependency replays and transplant only its unique
# Wave 3 commits above d5f468f.
git -C <runtime-worktree> rebase --onto "$INT" "$RUNTIME_DEP" codex/company-os-wave3-runtime
git merge --ff-only codex/company-os-wave3-runtime
git -C <dashboard-worktree> rebase --onto "$INT" "$BASE" codex/company-os-wave3-dashboard
git merge --ff-only codex/company-os-wave3-dashboard
git -C <review-worktree> rebase --onto "$INT" "$BASE" codex/company-os-wave3-review
git merge --ff-only codex/company-os-wave3-review

# 3. Prove both remote lines remain ancestors before any delivery.
git merge-base --is-ancestor "$PR302" HEAD
git merge-base --is-ancestor "$MASTER" HEAD
git diff --check "$MASTER"..HEAD

# 4. Run the focused matrix from this report plus the repository release gate.
# Record commands and actual counts at the final hash.
pnpm acceptance:mission-wave

# 5. Reviewer recommends a dry-run first. Only an explicitly authorized Host
# may perform the actual fast-forward push that updates existing PR 302.
git push --dry-run origin HEAD:codex/nested-org-agent-teams-spec-v1
git push origin HEAD:codex/nested-org-agent-teams-spec-v1

# 6. Read remote truth back; do not infer delivery from local push output.
git ls-remote origin refs/heads/master refs/heads/codex/nested-org-agent-teams-spec-v1
gh pr view 302 --repo cyl19970726/multi-agent-harness \
  --json headRefOid,baseRefOid,mergeable,mergeStateStatus,statusCheckRollup,url
```

If either remote hash moved, if the push is non-fast-forward, or if PR 302 no
longer names that head branch, stop and recompute. Never use `--force` or
`--force-with-lease` for this delivery.

## Reproducible audit commands

Audit and conflict commands executed successfully:

```bash
git ls-remote origin refs/heads/master refs/pull/302/head refs/pull/302/merge
gh pr view 302 --repo cyl19970726/multi-agent-harness \
  --json number,title,state,url,baseRefName,baseRefOid,headRefName,headRefOid,mergeable,mergeStateStatus,commits,statusCheckRollup
git merge-base 8539a33 origin/master
git merge-base --is-ancestor 7296354 8539a33
git merge-tree --write-tree 8539a33 origin/master
git cherry -v 8539a33 codex/company-os-agent-member-identity-v1
git cherry -v 8539a33 codex/company-os-runtime-delivery-v1
git cherry -v 8539a33 codex/company-os-ui-contract-v1
python3 /Users/hhh0x/.codex/skills/session-forensics/scripts/session_metrics.py \
  <the eleven exact Wave3-and-cancelled-Kimi provider-native files> \
  --json-out /tmp/wave3-session-metrics.json
git merge-tree --write-tree 8539a33 a2fc58b
git merge-tree --write-tree 3b269d4 a2fc58b
git merge-tree --write-tree 5565999 a2fc58b
git merge-tree --write-tree 5ca2563 a2fc58b
```

Independent candidate checks:

```bash
# Atomicity candidate 3b269d4
cargo test -p harness-core       # 65 unit + 13 Company OS passed
cargo test -p harness-store      # 54 unit + 15 Company OS + 4 delivery passed
cargo check -p harness-cli       # passed

# Runtime candidate 5565999; each exact filter ran one test and passed
cargo test -p harness-cli --test team_run_api \
  review_required_kimi_032_blocks_initial_start_and_http_work_rebind_before_acp -- --exact
cargo test -p harness-cli --test team_run_api \
  installed_kimi_upgrade_blocks_reopen_and_recovery_without_reusing_native_session -- --exact
cargo test -p harness-cli --test team_run_api \
  reviewed_recovery_redelivers_same_stable_member_without_duplicate_work_or_session -- --exact
cargo test -p harness-cli --test claude_agent_sdk_member \
  review_required_agent_sdk_package_is_refused_before_fake_runner_execution -- --exact
cargo test -p harness-cli --test serve_sse_projects \
  typed_ledger_replace_delete_and_reconnect_recover_only_the_selected_scope -- --exact
cargo test -p harness-cli --test provider_capacity_preflight # 9/9 passed
cargo test -p harness-cli --test team_run_start               # 10 passed, 2 historical claude_cli tests ignored
cargo test -p harness-cli --test claude_agent_sdk_member      # 10/10 passed

# Dashboard candidate 5ca2563
pnpm check:dashboard:runtime-e2e  # 17/17 passed
pnpm check:dashboard              # full component/browser/a11y/build gate passed
```

Final report branch: `codex/company-os-wave3-review`. Its commit is recorded in
the submitted Review Work, avoiding a self-referential commit hash in this
file.

## Remaining risks at this snapshot

- The live store has demonstrated projection loss on rebound. Candidate reads
  recover it, but the existing rows still require the explicit versioned
  append-only reconciliation plus dual-scope readback after integration.
- The provider gate passes on `5565999`, but the Runtime rebase has a semantic
  `main.rs` conflict with current master; only final-hash causal reruns prove
  the composed gate.
- Master moved during review. The exact `a2fc58b` map has one baseline/Atomicity/
  Dashboard conflict and two Runtime conflicts, but every lane still overlaps
  master semantically.
- The stopped Kimi worktrees contain large dirty implementations. Accidental
  staging, copying, cleanup, or authorship reassignment would destroy evidence
  or contaminate Wave 3 provenance.
- PR 302's published merge ref remains stale against old master and its GitHub
  mergeability is unknown. Remote delivery is not proven until fast-forward
  readback and new CI complete at the delivered head.

# Company OS Wave 3 Independent Release Audit

Status: independent integrated-candidate audit complete. All five recorded P1s
are closed, the three implementation Works are Host-accepted, and repaired
release candidate `22e09bf915bac835c284d399522bf34935490465` passes the
post-integration gate. Live projection reconciliation, report integration,
remote delivery/readback, and new PR CI remain Host actions.

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

The three Wave 3 implementation candidates and repaired integration candidate
pass their independently reviewed boundaries. They are not yet delivered: live
append-only repair, report integration, authorized fast-forward delivery,
remote readback, and new CI remain Host release gates. Five P1 findings were
recorded; all are closed and all ownership/closure facts are Work-linked:

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
5. **Closed in `22e09bf915bac835c284d399522bf34935490465`.** The first
   integrated head `2a50d88f7a4a9ca5370e6ad0577921c8f184e19b`
   contained current master but not exact PR 302 head `7296354`; pushing it to
   the existing PR branch would have required a forbidden non-fast-forward
   update. Owner: Host. Finding and repair recommendation:
   `tmsg-1785902137385-p72571-0`. The repaired line begins at `8539a33` and
   merge commit `77f1b2aa501ae5eb085bdede1146d9ff37f3927c` has exact
   parents `8539a33` and `a2fc58b`; both required ancestor checks now pass.
   Host repair response: `tmsg-1785902902930-p74133-1`.

There is no open or unowned P0/P1 at this snapshot. The causal matrix was rerun
on the exact repaired integration hash. Provider completion was not used as
acceptance evidence.

## Remote and ancestry baseline

Read-only remote inspection produced:

| Ref | Exact object | Finding |
| --- | --- | --- |
| `origin/master` | `a2fc58b3a446c7814d555e16990d443ff1dd8a04` | Current remote master from the final `git ls-remote`; it moved during review from `5b56f86`. |
| PR 302 head | `729635451d73c24710cd1c6138cc04713495bff6` | Open PR `codex/nested-org-agent-teams-spec-v1` -> `master`; CI rows shown by GitHub are green but predate Wave 3. |
| PR 302 merge ref | `ec7ac8d97df7150e7f7579a2b0bc4c3b3c31dc6d` | Stale test merge of `7296354` into old base `52c0864`, not current master. GitHub reports mergeability/merge-state `UNKNOWN`. |
| Wave 3 base | `8539a336dbf2da79d3d86504191db68710930e39` | Contains PR 302 head as an exact ancestor. |
| Repaired release candidate | `22e09bf915bac835c284d399522bf34935490465` | Local `codex/company-os-recursive-release-v1`; contains exact master and PR 302 head ancestry. Not pushed. |

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

- `crates/firm-cli/src/main.rs`, `company_os_api.rs`, `mcp.rs`, and tests;
- `crates/firm-core/src/lib.rs` and `crates/firm-store/src/lib.rs`;
- Runtime/SSE and recovery tests;
- Dashboard `api.ts`, `WorkbenchShell.tsx`, `types.ts`, Vite configuration,
  browser checks, and provenance UI; and
- Work/ADR/skill contracts.

Candidate simulations against exact master `a2fc58b` report:

- Atomicity `3b269d4` and Dashboard `5ca2563`: the same single textual
  `docs/decisions/README.md` conflict;
- Runtime `5565999`: two textual conflicts,
  `docs/decisions/README.md` and `crates/firm-cli/src/main.rs`; and
- Atomicity plus Dashboard, and Runtime plus Dashboard before master, merge
  textually clean.

The Runtime `main.rs` resolution is semantic, not choose-a-side: retain its
fresh provider preflight and recovery mutations (`mut members`, preflight
before mutation, and `started_at` refresh) while also retaining master's
Goal-stack deletion, `board-summary`, cheatsheet, help, and anti-drift surface.
Pairwise textual cleanliness does not remove the requirement to rerun the
causal matrix after that resolution.

### Repaired integration truth

The Host's first rebased integration `2a50d88` failed the exact PR 302
ancestor check. Directly merging `7296354` into that line produced 15 textual
documentation conflicts, so the lower-risk repair rebuilt from `8539a33`.
The accepted local release lineage is:

| Object | Purpose |
| --- | --- |
| `77f1b2aa501ae5eb085bdede1146d9ff37f3927c` | Merge commit with exact parents `8539a33` and `a2fc58b`; preserves PR 302 and master histories. |
| `ea4acf7`, `44fda1a` | Resolve the ADR collision: master retains ADR 0051 Single Intent; Nested Agent Teams becomes ADR 0052 with references updated. |
| `207355841ceadff8d60b7c829f19075bdadb6a07` | Atomicity integration; stable patch id equals candidate `3b269d4`. |
| `108b1e848c9322f54247d8c8d6b2df2031e74c23` | Runtime-only integration; stable patch id equals exact candidate `5565999e8a09367bc73c1f53801b375d57f409ee`. |
| `76392f94ff574de18b7eb6308c9a56cbbb70515c`, `5a4716ecc056d3fab2c23b8006b7ea9091bdc393` | Dashboard implementation/correction; stable patch ids equal `8e23241` and `5ca2563`. |
| `0934f4867229147c53c45993daf37e7fe9226c36` | Integration-only correction: refuse Team-scoped use of a TeamRun-local `--since` cursor, and supply `/v1/meta` in the self-heal browser harness. |
| `22e09bf915bac835c284d399522bf34935490465` | Pins the successful MCP rebind fixture to reviewed fake Kimi rather than inheriting an installed `review_required` version. |

All four implementation stable patch ids match their integrated counterparts
exactly. The release candidate and this report contain only Git-resolved
Runtime object ids, never a guessed expansion of a short hash.

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
| Atomicity | `company-os-wave3-atomicity` / `codex/company-os-wave3-atomicity` | `3b269d4bc3e9625562eee8bb5768b1a4cd1bf344` | `firm-core`, `firm-store`, `team_run_work_command`, and WorkOperation/rebind semantics overlap current master/PR 310. Candidate includes the authorized `reconcile-projection` CLI hunk and same-MemberRun higher-generation rebind primitive. Against `a2fc58b`, textual conflict is only `docs/decisions/README.md`. | committed; Work Host-accepted v4 |
| Runtime | `company-os-wave3-runtime` / `codex/company-os-wave3-runtime` | unique `5565999e8a09367bc73c1f53801b375d57f409ee`; dependencies `b21917b` (semantic PR 310 replay) then `d5f468f` (patch-identical Atomicity replay) | `main.rs`, SSE/provider and TeamRun tests overlap master. Against `a2fc58b`, textual conflicts are `main.rs` and `docs/decisions/README.md`. Final integration already gets PR 310 from master and Atomicity from its lane, so transplant only commits strictly above `d5f468f`. | committed; Work Host-accepted v5 |
| Dashboard | `company-os-wave3-dashboard` / `codex/company-os-wave3-dashboard` | `5ca256355dc1566726f718c8d49368071c5ecfc3` (`8e23241` implementation + `5ca2563` audit correction) | Dashboard API, Workbench, types, Vite/browser checks overlap provenance changes. Against `a2fc58b`, textual conflict is only `docs/decisions/README.md`; semantic composition with `6617929` still needs integrated checks. | committed; Work Host-accepted v7 |
| Release | `company-os-recursive-implementation-v1` / `codex/company-os-recursive-release-v1` | `22e09bf915bac835c284d399522bf34935490465` | Conflicts are resolved with both histories retained; ADRs are 0051 Single Intent and 0052 Nested Teams. Four candidate patch ids match exactly; integration-only cursor and test-fixture corrections are explicit. | clean local candidate; both ancestry checks and final gate pass; not pushed |
| Audit | `company-os-wave3-review` / `codex/company-os-wave3-review` | this report only | `docs/research` only; merge after implementation/report resnapshot. | in progress |

All candidate, integration, and exact current-master hashes are frozen above.
Any remote movement invalidates this map.

## Acceptance matrix

| Boundary | Required independent proof | Snapshot result |
| --- | --- | --- |
| Cross-store cutover | Concurrent conflicting transitions are fenced/refused; retry/restart is idempotent; no dual mutable responsibility; rebound preserves Team scope. | **PASS on integrated `22e09bf`** — `firm-core` 65 + 13 and `firm-store` 58 + 15 + 4 all pass. The one-way fence, crash/retry, crash-gap Work advance, sparse mixed-writer recovery/refusal, append-only reconciliation, and higher-generation rebind are present. Live reconciliation remains a Host data action. |
| Provider compatibility | `review_required` cannot start, reopen/resume, recover/rebind, or rebound delivery before native drive; reviewed Codex/Claude modes still work. | **PASS on integrated `22e09bf`** — serial `team_run_api` passes 45/45, including Kimi 0.32 pre-ACP refusals and reviewed stable recovery; `team_run_start` passes 10 with 2 documented historical `claude_cli` ignores. Claude live sessions remain honest 403 history, not a positive live-provider claim. |
| Runtime recovery | Honest cursor or explicitly bounded snapshot contract; same-size replace and deletion invalidate/recover without stale scope or duplicate Work. | **PASS on integrated `22e09bf`** — bounded snapshot contract remains explicit; `serve_sse_projects` passes 7/7, including typed replace/delete/reconnect selected-scope recovery. |
| Company selection | Tab-local selection does not mutate CLI/global Company; externally created Company appears; stale scope cannot overwrite current scope. | **PASS on integrated `22e09bf`** — real browser selected non-default B while server default remained A, refreshed external C, and rejected a delayed stale response. |
| Domain freshness | Works, Docs, Org, and Runtime/read-model freshness are independently truthful and accessible. | **PASS on integrated `22e09bf`** — exact-tree dashboard gate exposes and exercises all four independent domain states. |
| Real Runtime/browser E2E | Real in-process Runtime/SSE plus real browser; external Work, Docs, Org writes; reconnect, visibility/background, stale selector; no fixture-only substitute. | **PASS on integrated `22e09bf`** — independent isolated Runtime/Chromium rerun passes 17/17 and the full dashboard TypeScript/browser/a11y/production-build gate passes. No snapshot/SSE/business-row fixture injection. |
| Master/PR integration | Candidate contains current master and exact PR 302 head ancestry; merge conflicts semantically resolved; full gates at final hash. | **PASS on local release candidate `22e09bf`** — exact master and PR 302 ancestor checks both exit 0, four implementation patch ids match, ADR 0051/0052 coexist, `git diff --check` passes, and `pnpm acceptance:mission-wave` passes. |
| Remote delivery | Authorized fast-forward update, remote hash readback, PR 302 recomputed CI/merge state. | Not authorized/executed by reviewer. |

## Exact integration and delivery sequence

The first rebased integration proved why the release must preserve exact PR 302
ancestry: it could not fast-forward the existing PR branch. The repaired local
branch now has the correct topology and must not be rebased again.

Verified current state and remaining Host sequence:

```bash
MASTER=a2fc58b3a446c7814d555e16990d443ff1dd8a04
PR302=729635451d73c24710cd1c6138cc04713495bff6
REL=codex/company-os-recursive-release-v1
REL_HEAD=22e09bf915bac835c284d399522bf34935490465

# 0. Abort if the remote moved; substitute no guessed hash.
git ls-remote origin refs/heads/master refs/heads/codex/nested-org-agent-teams-spec-v1
test "$(git rev-parse "$REL")" = "$REL_HEAD"

# 1. Reprove both histories and the exact-tree gate.
git switch "$REL"
git merge-base --is-ancestor "$PR302" HEAD
git merge-base --is-ancestor "$MASTER" HEAD
git diff --check "$MASTER"..HEAD
pnpm acceptance:mission-wave

# 2. Integrate both audit-only commits after this Work submits. Replace the
# placeholder with the exact new report commit from the Work artifact refs.
git cherry-pick 1f88429ad19be4b3defa923cef1f69948e6d7a1e \
  <updated-report-commit-from-Work>
git diff --check "$MASTER"..HEAD
pnpm check:docs-governance
git merge-base --is-ancestor "$PR302" HEAD
git merge-base --is-ancestor "$MASTER" HEAD

# 3. Run the explicit versioned append-only Work projection reconciliation and
# dual TeamRun/Team-scope readback described in this report. Do not edit rows.

# 4. Only an explicitly authorized Host may update the existing PR branch.
git push --dry-run origin HEAD:codex/nested-org-agent-teams-spec-v1
git push origin HEAD:codex/nested-org-agent-teams-spec-v1

# 5. Read remote truth back; do not infer delivery from local push output.
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
git show -s --format='%H %P' 77f1b2a
git merge-base --is-ancestor a2fc58b 22e09bf
git merge-base --is-ancestor 7296354 22e09bf
git diff --check a2fc58b..22e09bf
# `git show <commit> | git patch-id --stable` for all four candidate/integrated pairs
```

Independent candidate checks:

```bash
# Atomicity candidate 3b269d4
cargo test -p firm-core       # 65 unit + 13 Company OS passed
cargo test -p firm-store      # 54 unit + 15 Company OS + 4 delivery passed
cargo check -p firm-cli       # passed

# Runtime candidate 5565999; each exact filter ran one test and passed
cargo test -p firm-cli --test team_run_api \
  review_required_kimi_032_blocks_initial_start_and_http_work_rebind_before_acp -- --exact
cargo test -p firm-cli --test team_run_api \
  installed_kimi_upgrade_blocks_reopen_and_recovery_without_reusing_native_session -- --exact
cargo test -p firm-cli --test team_run_api \
  reviewed_recovery_redelivers_same_stable_member_without_duplicate_work_or_session -- --exact
cargo test -p firm-cli --test claude_agent_sdk_member \
  review_required_agent_sdk_package_is_refused_before_fake_runner_execution -- --exact
cargo test -p firm-cli --test serve_sse_projects \
  typed_ledger_replace_delete_and_reconnect_recover_only_the_selected_scope -- --exact
cargo test -p firm-cli --test provider_capacity_preflight # 9/9 passed
cargo test -p firm-cli --test team_run_start               # 10 passed, 2 historical claude_cli tests ignored
cargo test -p firm-cli --test claude_agent_sdk_member      # 10/10 passed

# Dashboard candidate 5ca2563
pnpm check:dashboard:runtime-e2e  # 17/17 passed
pnpm check:dashboard              # full component/browser/a11y/build gate passed

# Exact repaired release candidate 22e09bf
cargo test -p firm-core -p firm-store
# core: 65 + 13; store: 58 + 15 + 4; all passed
cargo test -p firm-cli --test team_run_api -- --test-threads=1
# 45/45 passed
pnpm acceptance:mission-wave
# MCP 4/4; Mission/Wave 4/4; TeamRun API 45/45;
# Team start 10 passed + 2 documented historical ignores;
# full Dashboard TypeScript/browser/a11y/real Runtime 17/17/build passed
```

One preliminary `cargo test --workspace` used default parallel test threads and
hit an ephemeral-port collision: `team_run_api` was 44/45 and
`kimi_model_switch_uses_only_the_new_models_advertised_effort_controls` failed
before `firm serve` became ready with `Address already in use`. The complete
suite then passed 45/45 under the repository's serial acceptance setting. This
is recorded as test-harness flake evidence, not hidden or treated as a product
failure.

Final report branch: `codex/company-os-wave3-review`. Its commit is recorded in
the submitted Review Work, avoiding a self-referential commit hash in this
file.

## Remaining risks at this snapshot

- The live store has demonstrated projection loss on rebound. Candidate reads
  recover it, but the existing rows still require the explicit versioned
  append-only reconciliation plus dual-scope readback after integration.
- The provider gate and scoped Runtime contract pass at integrated `22e09bf`.
  Claude's two original live sessions remain honest HTTP 403 evidence, and
  installed Kimi 0.32 remains `review_required`; neither is a positive live
  provider canary claim.
- Master moved during review and the first integration rewrote PR 302 ancestry.
  The repaired line fixes both, but any new remote movement or rebase invalidates
  the ancestry and gate evidence.
- Default-parallel `team_run_api` can collide on an ephemeral port; the official
  serial acceptance passed. This is a non-blocking test-harness reliability
  risk, not an excuse to weaken or skip the serial gate.
- The stopped Kimi worktrees contain large dirty implementations. Accidental
  staging, copying, cleanup, or authorship reassignment would destroy evidence
  or contaminate Wave 3 provenance.
- PR 302's published merge ref remains stale against old master and its GitHub
  mergeability is unknown. Remote delivery is not proven until the report and
  live repair evidence are integrated, fast-forward readback matches, and new
  CI completes at the delivered head.

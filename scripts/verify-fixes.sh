#!/usr/bin/env bash
# Real regression verification for the Issue #107 Gap 5 (canonical event model),
# Issue #139 (workflow ergonomics + codex reliability), and the goal-planning-model
# (knowledge-driven phased planning S1-S8) fixes. Prints a PASS/FAIL matrix and
# exits nonzero on any failed check.
#
#   scripts/verify-fixes.sh            # deterministic tier: fmt/clippy/test + dashboard build
#   scripts/verify-fixes.sh --real     # + live tier: REAL codex workflow, serve,
#                                       #   normalized endpoints, live SSE frames
#
# The deterministic tier needs no codex/network and covers every fix at the
# unit/build level (it IS the CI gate plus the dashboard typecheck+build). The
# --real tier additionally proves the fixes on a live codex turn end-to-end:
#   P1  real `harness workflow run-script` (no --dry-run):
#         #5 schema coercion + #2 final-message structured + #6 positional verdict
#         + #4 bare-positional pipeline + #1 stdin (no wedge) + Gap 5 A2 cmd leaf
#   P2  serve the run's store and curl the normalized endpoints:
#         Gap 5 A2 (codex command -> tool_call+tool_result), F0 (historical
#         normalized + retained), A3 (a real claude session -> multi-block
#         expansion, if one exists locally), B (live `provider_turn_event_normalized`
#         frames during a fresh delivery)
# Frontend (Stage C) NormalizedTurnTui is build-checked here; its browser drill-in
# is a manual preview step (point the dashboard at the printed serve URL).
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
REAL=0
[ "${1:-}" = "--real" ] && REAL=1
PORT="${VERIFY_PORT:-8796}"
HARNESS="$REPO_ROOT/target/debug/harness"
TMP="$(mktemp -d)"
STORE="$TMP/store"
SERVE_PID=""
SSE_PID=""
MP_SERVE_PID=""

PASS=0
FAIL=0
SKIP=0
ok()   { echo "  PASS  $1"; PASS=$((PASS + 1)); }
bad()  { echo "  FAIL  $1"; FAIL=$((FAIL + 1)); }
skip() { echo "  SKIP  $1"; SKIP=$((SKIP + 1)); }
section() { echo; echo "== $1 =="; }

# Free a port held by a serve we started, by pid AND by listener (belt-and-
# suspenders: a serve started behind a subshell can outlive its $!).
free_serve_port() {
  local pid="$1" port="$2"
  [ -n "$pid" ] && kill "$pid" 2>/dev/null
  if [ -n "$port" ] && command -v lsof >/dev/null 2>&1; then
    for lp in $(lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null); do
      kill "$lp" 2>/dev/null
    done
  fi
}
cleanup() {
  [ -n "$SSE_PID" ] && kill "$SSE_PID" 2>/dev/null
  free_serve_port "$SERVE_PID" "${PORT:-}"
  free_serve_port "$MP_SERVE_PID" "${MPPORT:-}"
  rm -rf "$TMP"
}
trap cleanup EXIT

# check_json FILE PYEXPR -> "1" if the expression (over the loaded JSON `d`) is truthy, else "0".
check_json() { python3 -c "import json; d=json.load(open('$1')); print(1 if ($2) else 0)" 2>/dev/null || echo 0; }

# ---------------------------------------------------------------------------
section "Deterministic checks (no codex required)"

cargo fmt --all --check >/dev/null 2>&1
[ $? -eq 0 ] && ok "cargo fmt --all --check" || bad "cargo fmt --all --check"

cargo clippy --all-targets -- -D warnings >/dev/null 2>"$TMP/clippy.log"
[ $? -eq 0 ] && ok "cargo clippy --all-targets -D warnings" || bad "cargo clippy (see $TMP/clippy.log)"

cargo test --workspace -- --skip resident_daemon >"$TMP/test.log" 2>&1
TEST_RC=$?
[ $TEST_RC -eq 0 ] && ok "cargo test --workspace" || bad "cargo test --workspace (see $TMP/test.log)"
# Confirm the specific fix tests actually ran (guards against a rename silently dropping coverage).
for t in \
  schema_to_json_schema_coerces_known_type_hints \
  extract_codex_final_message_returns_terminal_message_not_joined \
  pipeline_accepts_bare_positional_stages_not_just_a_list \
  verdict_accepts_a_positional_reason \
  codex_normalize_command_execution_with_output_emits_call_and_result \
  claude_normalize_user_tool_results_expand_in_order_and_retain_raw \
  historical_normalized_events_normalize_durable_trace_and_report_retained \
  one_line_can_broadcast_multiple_frames \
  broadcast_is_isolated_per_project \
  offsets_and_broadcasts_independent_across_projects \
  reconcile_phase_sets_status_landed_commit_knowledge_and_syncs_stage \
  landed_commit_round_trips_and_legacy_defaults_to_none \
  orchestrate_lands_writable_phase_diff_and_records_landed_commit \
  orchestrate_readonly_phase_lands_nothing \
  orchestrate_fails_phase_when_diff_cannot_be_applied \
  land_phase_diffs_refuses_when_repo_tree_is_dirty \
  land_phase_diffs_rolls_back_partial_apply_leaving_no_orphan ; do
  grep -q "test .*$t ... ok" "$TMP/test.log" && ok "unit: $t" || bad "unit: $t did not run/pass"
done
# goal-multi-project deterministic regression coverage — one representative test
# per dimension MUST run in the workspace test command (the suite runs against
# temp harness homes/project roots, never the real ~/.harness). A rename that
# silently drops a dimension's coverage fails the gate here. The "[dim]" tag makes
# a failure name the project dimension involved.
for pair in \
  "project-id:project_id_for_path_outside_home_is_stable_hash" \
  "registry:init_writes_registry_metadata_and_active_marker" \
  "resolution-precedence:project_flag_selects_by_id" \
  "resolution-precedence:legacy_cwd_walk_up_is_warned_fallback" \
  "workflow-cwd:writable_node_roots_worktree_at_project_root_not_harness_cwd" \
  "persistent-cwd:codex_delivery_without_worktree_runs_in_project_root_not_harness_cwd" \
  "persistent-cwd:claude_delivery_sees_the_selected_projects_claude_md_marker" \
  "serve-api:snapshot_with_project_param_reads_that_store_only" \
  "sse-isolation:sse_streams_are_isolated_per_project" \
  "dashboard-switch:serve_and_cli_from_different_cwds_converge_after_switch" \
  "global-policy:global_writable_node_fails_with_actionable_non_git_message" \
  "migration:migrate_preserves_records_and_payloads_and_marks_old_store" ; do
  dim="${pair%%:*}"; t="${pair#*:}"
  grep -q "test .*$t ... ok" "$TMP/test.log" && ok "mp[$dim]: $t" || bad "mp[$dim]: $t did not run/pass"
done

npx tsc -p apps/agent-dashboard/tsconfig.json --noEmit >/dev/null 2>&1
[ $? -eq 0 ] && ok "dashboard tsc --noEmit" || bad "dashboard tsc --noEmit"

npx vite build --config apps/agent-dashboard/vite.config.ts --outDir "$TMP/web" --emptyOutDir >/dev/null 2>&1
[ $? -eq 0 ] && ok "dashboard vite build" || bad "dashboard vite build"

# ---------------------------------------------------------------------------
section "Planning-model loop (goal-planning-model S1-S8, --dry-run, no codex)"
cargo build -p harness-cli >/dev/null 2>&1
if [ ! -x "$HARNESS" ]; then
  bad "build harness-cli for planning checks"
else
  PM="$TMP/pm"
  H() { "$HARNESS" --store "$PM" "$@"; }
  H goal create --id pm --title "verify planning" --owner lead --priority p1 >/dev/null 2>&1
  # S2: the design_md-requires-knowledge gate fires on an empty ledger.
  if H goal design-synthesize --goal pm >/dev/null 2>&1; then
    bad "S2 gate: design-synthesize should reject an empty knowledge ledger"
  else
    ok "S2 gate: design-synthesize rejects empty knowledge"
  fi
  # S2: knowledge-add -> design-synthesize regenerates design_md from the ledger.
  H goal knowledge-add --goal pm --author lead --tag arch \
    --notes "codegen-to-Starlark beats a native DAG executor" >/dev/null 2>&1
  H goal design-synthesize --goal pm >"$TMP/pm_syn.json" 2>/dev/null
  [ "$(check_json "$TMP/pm_syn.json" "d.get('design_synthesis_at') is not None and 'Design' in (d.get('design_md') or '')")" = "1" ] \
    && ok "S2 design-synthesize from knowledge" || bad "S2 design-synthesize"
  # S3: phase + tasks -> deterministic .star with parallel() + verdict().
  H goal phase-add --goal pm --phase-id p1 --name Build \
    --intent "two disjoint crates build in parallel then verify" \
    --acceptance "compiles and smoke passes" >/dev/null 2>&1
  H task create --id pk1 --goal pm --title A --objective a --owner lead --phase-id p1 --owned-path crates/a >/dev/null 2>&1
  H task create --id pk2 --goal pm --title B --objective b --owner lead --phase-id p1 --owned-path crates/b >/dev/null 2>&1
  H phase compile pm --phase p1 >"$TMP/pm_c.json" 2>/dev/null
  CPATH="$(python3 -c "import json;print(json.load(open('$TMP/pm_c.json'))['path'])" 2>/dev/null)"
  { [ -n "$CPATH" ] && grep -q 'parallel(\[' "$CPATH"; } && ok "S3 compile -> parallel() for disjoint tasks" || bad "S3 compile parallel()"
  { [ -n "$CPATH" ] && grep -q 'verdict(' "$CPATH"; } && ok "S3 compile -> verdict() gate" || bad "S3 compile verdict()"
  CH1="$(python3 -c "import json;print(json.load(open('$TMP/pm_c.json'))['hash'])" 2>/dev/null)"
  H phase compile pm --phase p1 >"$TMP/pm_c2.json" 2>/dev/null
  CH2="$(python3 -c "import json;print(json.load(open('$TMP/pm_c2.json'))['hash'])" 2>/dev/null)"
  [ -n "$CH1" ] && [ "$CH1" = "$CH2" ] && ok "S3 compile deterministic (identical content hash)" || bad "S3 determinism"
  # S4/S8: run-phases dry-run -> gate -> advance -> checkpoint + verdict decision + step link.
  H goal run-phases pm --dry-run >"$TMP/pm_run.json" 2>/dev/null
  [ "$(check_json "$TMP/pm_run.json" "d.get('status')=='completed' and d.get('stage')=='verified'")" = "1" ] \
    && ok "S4 run-phases (dry-run) -> completed + stage verified" || bad "S4 run-phases"
  H goal show --goal pm >"$TMP/pm_goal.json" 2>/dev/null
  [ "$(check_json "$TMP/pm_goal.json" "bool(d['phases']) and all(p['status']=='passed' and p.get('verdict_decision_id') for p in d['phases'])")" = "1" ] \
    && ok "S8 phases passed + verdict_decision_id set" || bad "S8 verdict_decision_id"
  [ -f "$PM/goal_orchestration_runs.jsonl" ] && ok "S4 orchestration checkpoint persisted" || bad "S4 checkpoint"
  [ "$(check_json "$PM/decisions.jsonl" "True" 2>/dev/null || echo 0)" = "1" ] || true
  grep -q '"decision_kind":"phase_verdict"' "$PM/decisions.jsonl" 2>/dev/null \
    && ok "S8 phase_verdict Decision recorded" || bad "S8 phase_verdict Decision"
  python3 -c "import json;ss=[json.loads(l) for l in open('$PM/workflow_steps.jsonl')];import sys;sys.exit(0 if any(s.get('task_id') for s in ss) and any(s.get('verdict_outcome') for s in ss) else 1)" 2>/dev/null \
    && ok "S8 WorkflowStep.task_id + verdict_outcome populated" || bad "S8 WorkflowStep link"
  # S5: a re-run skips the already-passed phase (resume primitive).
  H goal run-phases pm --dry-run >"$TMP/pm_run2.json" 2>/dev/null
  [ "$(check_json "$TMP/pm_run2.json" "d.get('ran')==[] and d.get('skipped')==['p1']")" = "1" ] \
    && ok "S5 resume skips already-passed phase" || bad "S5 resume skip"

  # goal-phase-artifacts s3-gate: the deterministic REQUIRED-ARTIFACT gate.
  # A phase whose task declares a `required` output the (dry-run mock) worker
  # never produces must FAIL — today, with no manifest, it would PASS. The same
  # phase PASSES when the declared path points at a file that exists & is
  # non-empty (here a real committed repo file, since the dry-run worker writes
  # no diff and the process cwd is the repo root = the gate's working-tree root).
  PART="$TMP/part"
  HA() { "$HARNESS" --store "$PART" "$@"; }
  HA goal create --id art --title "verify artifacts" --owner lead --priority p1 >/dev/null 2>&1
  HA goal knowledge-add --goal art --author lead --tag arch --notes "gate enforces declared outputs" >/dev/null 2>&1
  HA goal design-synthesize --goal art >/dev/null 2>&1
  # Phase 1: a required output that is NEVER produced -> the gate must FAIL it.
  HA goal phase-add --goal art --phase-id p1 --name Build \
    --intent "produce the promised report" --acceptance "report exists" \
    --output "id=report,kind=test_report,path=docs/never-produced-by-dryrun.md,purpose=the report,required=true" >/dev/null 2>&1
  HA task create --id ak1 --goal art --title A --objective a --owner lead --phase-id p1 --owned-path crates/a \
    --output "id=report,kind=test_report,path=docs/never-produced-by-dryrun.md,purpose=the report,required=true" >/dev/null 2>&1
  HA goal run-phases art --dry-run >"$TMP/art_fail.json" 2>/dev/null
  [ "$(check_json "$TMP/art_fail.json" "d.get('status')=='failed' and d.get('failed_phase')=='p1'")" = "1" ] \
    && ok "s3-gate: phase FAILS when a required artifact is absent" || bad "s3-gate absent-artifact should fail"
  # The verdict rationale names the missing artifact.
  grep -q 'docs/never-produced-by-dryrun.md' "$PART/decisions.jsonl" 2>/dev/null \
    && ok "s3-gate: verdict rationale names the missing artifact" || bad "s3-gate missing-artifact rationale"
  # Phase 2 (fresh goal): a required output whose path EXISTS in the repo root
  # (the gate's working-tree fallback) -> the same gate PASSES.
  HA goal create --id art2 --title "verify artifacts present" --owner lead --priority p1 >/dev/null 2>&1
  HA goal phase-add --goal art2 --phase-id p1 --name Build \
    --intent "deliver a file that already exists" --acceptance "file exists" \
    --output "id=manifest,kind=code,path=Cargo.toml,purpose=workspace manifest,required=true" >/dev/null 2>&1
  HA task create --id ak2 --goal art2 --title B --objective b --owner lead --phase-id p1 --owned-path crates/b \
    --output "id=manifest,kind=code,path=Cargo.toml,purpose=workspace manifest,required=true" >/dev/null 2>&1
  HA goal run-phases art2 --dry-run >"$TMP/art_pass.json" 2>/dev/null
  [ "$(check_json "$TMP/art_pass.json" "d.get('status')=='completed'")" = "1" ] \
    && ok "s3-gate: phase PASSES when the required artifact is present" || bad "s3-gate present-artifact should pass"
fi

# ---------------------------------------------------------------------------
section "Phase landing + reconcile (goal-phase-landing, no codex)"
# Drive the built harness through the L1 reconciliation path (out-of-band work):
# `goal reconcile-phase` trues a phase's status to reality, stamps landed_commit,
# appends a decision-sourced Knowledge entry, and SYNCS the goal's derived stage.
# The L2 durable-landing path (apply each writable diff + one commit, recording
# landed_commit) needs a real worktree diff a dry-run worker never produces, so it
# is covered by the unit tests asserted in the "tests that MUST run" list above
# (orchestrate_lands_writable_phase_diff_and_records_landed_commit et al.) and is
# additionally exercised end-to-end in the --real tier below.
cargo build -p harness-cli >/dev/null 2>&1
if [ ! -x "$HARNESS" ]; then
  bad "build harness-cli for phase-landing checks"
else
  PL="$TMP/pl"
  HP() { "$HARNESS" --store "$PL" "$@"; }
  HP goal create --id pl --title "verify landing" --owner lead --priority p1 >/dev/null 2>&1
  HP goal phase-add --goal pl --phase-id p1 --name Build \
    --intent "ship A out-of-band" --acceptance "A landed" >/dev/null 2>&1
  HP goal phase-add --goal pl --phase-id p2 --name Verify \
    --intent "ship B out-of-band" --acceptance "B landed" >/dev/null 2>&1
  # Baseline: a phase-driven goal whose phases are all not_started derives `draft`,
  # NOT the contradictory raw working bar (the lie L1 fixes).
  HP goal show --goal pl >"$TMP/pl_before.json" 2>/dev/null
  [ "$(check_json "$TMP/pl_before.json" "bool(d['phases']) and all(p['status']=='not_started' and p.get('landed_commit') is None for p in d['phases'])")" = "1" ] \
    && ok "reconcile: fresh phase-driven goal starts all not_started/unlanded" || bad "reconcile baseline"
  # Reconcile p1 -> passed with a landed commit + a note. The command's own JSON
  # reports the recorded landed_commit AND the freshly-derived effective stage.
  HP goal reconcile-phase --goal pl --phase p1 --to passed \
    --landed-commit deadbee1 --note "shipped via PR #999" >"$TMP/pl_rec1.json" 2>/dev/null
  [ "$(check_json "$TMP/pl_rec1.json" "d.get('status')=='passed' and d.get('landed_commit')=='deadbee1' and d.get('effective_stage')=='working' and d.get('knowledge_id')")" = "1" ] \
    && ok "reconcile p1 -> passed (landed_commit recorded, derived stage advances to working)" \
    || bad "reconcile p1 status/landed_commit/effective_stage"
  # The mutation is durable on the goal: p1 carries status+landed_commit, the
  # persisted stage synced to the derived `working`, and a `reconcile`-tagged
  # decision-sourced Knowledge entry with phase provenance was appended.
  HP goal show --goal pl >"$TMP/pl_mid.json" 2>/dev/null
  [ "$(check_json "$TMP/pl_mid.json" "next(p for p in d['phases'] if p['id']=='p1')['status']=='passed' and next(p for p in d['phases'] if p['id']=='p1')['landed_commit']=='deadbee1' and d['stage']=='working'")" = "1" ] \
    && ok "reconcile: phase status + landed_commit persisted; goal stage synced to working" \
    || bad "reconcile p1 persistence/stage sync"
  [ "$(check_json "$TMP/pl_mid.json" "any(k.get('phase_id')=='p1' and k.get('source')=='decision' and 'reconcile' in (k.get('tags') or []) and 'deadbee1' in (k.get('notes_md') or '') for k in d.get('knowledge',[]))")" = "1" ] \
    && ok "reconcile: appended a decision-sourced reconcile Knowledge entry (provenance p1)" \
    || bad "reconcile p1 knowledge provenance"
  # Reconcile p2 too -> all phases passed -> derived stage advances to `verified`
  # and the goal status flips to done (the all-phases-passed terminal).
  HP goal reconcile-phase --goal pl --phase p2 --to passed \
    --landed-commit deadbee2 >"$TMP/pl_rec2.json" 2>/dev/null
  [ "$(check_json "$TMP/pl_rec2.json" "d.get('effective_stage')=='verified'")" = "1" ] \
    && ok "reconcile p2 -> all passed -> derived stage advances to verified" || bad "reconcile p2 effective_stage"
  HP goal show --goal pl >"$TMP/pl_final.json" 2>/dev/null
  [ "$(check_json "$TMP/pl_final.json" "d['stage']=='verified' and d['status']=='done' and all(p['status']=='passed' and p['landed_commit'] for p in d['phases'])")" = "1" ] \
    && ok "reconcile: goal stage=verified status=done, every phase passed+landed" || bad "reconcile terminal state"
  # An unknown phase id is rejected (clear error, no mutation, nonzero exit).
  if HP goal reconcile-phase --goal pl --phase nope --to passed >/dev/null 2>&1; then
    bad "reconcile: unknown phase id should fail"
  else
    ok "reconcile: unknown phase id is rejected"
  fi
fi

# ---------------------------------------------------------------------------
section "Multi-project serve API + #89 convergence (goal-multi-project, no codex)"
# Isolated HOME/HARNESS_HOME so this never touches the developer's real ~/.harness.
# Deterministic, no codex/claude: fake provider shims (scripts/multi-project-demo/
# fake-provider.sh) intercept the bare-name spawns and record the cwd they ran in.
# Proves, against ONE serve for the whole scenario:
#   * GET /v1/projects lists both projects + _global; POST /v1/projects/switch
#     flips the active project; a CLI from a DIFFERENT cwd then resolves the SAME
#     central store (~/.harness/projects/<id>), preserving the #89 sibling-
#     convergence invariant across a project switch;
#   * a writable WORKFLOW leaf in project A roots its worktree under A and reads
#     A's AGENTS.md marker "alpha" (not the harness process cwd);
#   * a persistent MEMBER delivery in project B runs in B's project_root and reads
#     B's AGENTS.md marker "beta";
#   * the GLOBAL `_global` (~/, non-git) project runs read-only nodes but rejects
#     writable/worktree nodes with an actionable message;
#   * migrating a legacy repo-local `.harness` into the central store preserves
#     every record + payload and marks (does not delete) the old store.
# shellcheck disable=SC1091
. "$REPO_ROOT/scripts/multi-project-demo/fake-provider.sh"
cargo build -p harness-cli >/dev/null 2>&1
if [ ! -x "$HARNESS" ]; then
  bad "build harness-cli for multi-project checks"
else
  MPHOME="$TMP/mp-home"
  MPHH="$MPHOME/.harness"
  mkdir -p "$MPHH"
  MPPORT="${VERIFY_MP_PORT:-8797}"
  # Avoid a busy port: a stale serve on $MPPORT would answer our curls from ITS
  # store, cascading into confusing wrong-project failures. Step to the next free
  # port (bounded) so the demo is self-healing across back-to-back runs.
  port_busy() { command -v lsof >/dev/null 2>&1 && lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1; }
  for _ in $(seq 1 20); do port_busy "$MPPORT" && MPPORT=$((MPPORT + 1)) || break; done
  # Fake provider shims on PATH so workflow/member spawns never touch real codex.
  MPBIN="$TMP/mp-fakebin"
  MP_CWD="$TMP/mp-provider-cwd.txt"
  MP_MARKER="$TMP/mp-provider-marker.txt"
  install_fake_providers "$MPBIN" "$MP_CWD" "$MP_MARKER"
  # Drive harness against the isolated home; clear inherited store overrides.
  MH() { env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= "$HARNESS" "$@"; }
  # As MH, but with the fake providers ahead of any real ones on PATH.
  MHP() { env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= PATH="$MPBIN:$PATH" "$HARNESS" "$@"; }
  ROOT_A="$MPHOME/repo-a"; ROOT_B="$MPHOME/repo-b"
  mkdir -p "$ROOT_A" "$ROOT_B"
  # Unique per-project AGENTS.md markers prove the worker read the SELECTED
  # project's tree. A is a git repo so a writable leaf can isolate a worktree
  # there (and HEAD carries AGENTS.md into the checkout the worker reads).
  printf 'PROJECT-A-AGENTS-alpha\n' >"$ROOT_A/AGENTS.md"
  printf 'PROJECT-B-AGENTS-beta\n'  >"$ROOT_B/AGENTS.md"
  ( cd "$ROOT_A" && git init -q && git config user.email v@v.v && git config user.name v \
      && git add -A && git commit -qm init ) >/dev/null 2>&1
  ( cd "$ROOT_A" && MH init >/dev/null 2>&1 )
  ( cd "$ROOT_B" && MH init >/dev/null 2>&1 )  # repo-b is active after init
  ID_A="$(python3 -c "import json;
reg=json.load(open('$MPHH/projects/registry.json'))
print(next(p['id'] for p in reg['projects'] if p['path'].endswith('repo-a')))" 2>/dev/null)"
  ID_B="$(python3 -c "import json;print(json.load(open('$MPHH/projects/registry.json'))['current_project_id'])" 2>/dev/null)"
  if [ -z "$ID_A" ] || [ -z "$ID_B" ] || [ "$ID_A" = "$ID_B" ]; then
    bad "MP setup: two distinct projects registered"
  else
    ok "MP setup: two distinct projects registered ($ID_A, $ID_B)"
    # Start serve from repo-a's cwd against the isolated home. `exec` replaces the
    # shell with the serve process so $! is the REAL serve pid (a `( cd && ... ) &`
    # subshell would leave $! pointing at the subshell, leaking the serve child on
    # teardown and holding the port for the next run).
    env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= \
      bash -c "cd '$ROOT_A' && exec '$HARNESS' serve --addr '127.0.0.1:$MPPORT' --no-truncate" \
      >"$TMP/mp-serve.log" 2>&1 &
    MP_SERVE_PID=$!
    UP=0
    for _ in $(seq 1 50); do
      curl -fs "http://127.0.0.1:$MPPORT/health" >/dev/null 2>&1 && { UP=1; break; }
      sleep 0.2
    done
    if [ $UP -ne 1 ]; then
      bad "MP serve did not come up (see $TMP/mp-serve.log)"
    else
      ok "MP serve up on :$MPPORT"
      # GET /v1/projects lists both + _global.
      curl -fs "http://127.0.0.1:$MPPORT/v1/projects" >"$TMP/mp-projects.json" 2>/dev/null
      [ "$(check_json "$TMP/mp-projects.json" "set(['$ID_A','$ID_B','_global']).issubset({p['id'] for p in d['projects']})")" = "1" ] \
        && ok "MP GET /v1/projects lists both projects + _global" || bad "MP /v1/projects enumeration"
      # POST switch to A.
      curl -fs -X POST "http://127.0.0.1:$MPPORT/v1/projects/switch" \
        -H 'Content-Type: application/json' -d "{\"project\":\"$ID_A\"}" >"$TMP/mp-switch.json" 2>/dev/null
      [ "$(check_json "$TMP/mp-switch.json" "d.get('ok') is True and d['result']['current']=='$ID_A'")" = "1" ] \
        && ok "MP POST /v1/projects/switch -> A" || bad "MP switch"
      # CLI from an UNRELATED cwd converges on A's central store.
      MPELSE="$MPHOME/elsewhere/deep"; mkdir -p "$MPELSE"
      MPSRC="$( cd "$MPELSE" && env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= \
        "$HARNESS" --store-source goal list 2>&1 | sed -n 's/.*store-source:.*root=//p' )"
      case "$MPSRC" in
        *"/projects/$ID_A") ok "MP #89 convergence: CLI from other cwd -> A central store" ;;
        *) bad "MP #89 convergence: CLI resolved '$MPSRC' (expected .../projects/$ID_A)" ;;
      esac

      # Dashboard project picker (goal-multi-project P6, dashboard-browser-check):
      # against the SAME live serve, prove SSE channel isolation (a client
      # subscribed to B never sees a live event appended to A, but DOES see B's),
      # which is the guarantee the picker relies on when it re-points the stream on
      # switch. The Playwright UI leg degrades to SKIP when Playwright is absent.
      STORE_A="$MPHH/projects/$ID_A"
      STORE_B="$MPHH/projects/$ID_B"
      if node "$REPO_ROOT/apps/agent-dashboard/tests/project-picker-check.mjs" \
        --base "http://127.0.0.1:$MPPORT" \
        --project-a "$ID_A" --store-a "$STORE_A" \
        --project-b "$ID_B" --store-b "$STORE_B" \
        >"$TMP/mp-picker.log" 2>&1; then
        ok "MP dashboard picker checks (SSE isolation A/B; see $TMP/mp-picker.log)"
      else
        bad "MP dashboard picker checks (see $TMP/mp-picker.log)"
      fi

      # --- Workflow leaf in A (cwd-routing, P3/P4) --------------------------
      # A writable leaf run --project A from a cwd that is NOT A: its worktree
      # roots UNDER A, the fake claude is spawned there, records its cwd, and
      # reads A's AGENTS.md "alpha". This is the deterministic stand-in for the
      # operator's live-codex leaf — same routing, no provider.
      : >"$MP_CWD"; : >"$MP_MARKER"
      ( cd "$MPELSE" && env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= \
        PATH="$MPBIN:$PATH" "$HARNESS" --project "$ROOT_A" workflow run-script \
        "$REPO_ROOT/scripts/multi-project-demo/leaf-writable.star" --timeout-ms 15000 \
        >"$TMP/mp-leaf.json" 2>"$TMP/mp-leaf.err" )
      LEAF_CWD="$(cat "$MP_CWD" 2>/dev/null)"
      LEAF_MARKER="$(cat "$MP_MARKER" 2>/dev/null)"
      ROOT_A_REAL="$( cd "$ROOT_A" && pwd -P )"
      case "$LEAF_CWD" in
        "$ROOT_A_REAL"/*|"$ROOT_A_REAL") ok "MP workflow leaf in A ran under A ($LEAF_CWD)" ;;
        *) bad "MP workflow leaf cwd '$LEAF_CWD' is not under A ($ROOT_A_REAL)" ;;
      esac
      [ "$LEAF_MARKER" = "PROJECT-A-AGENTS-alpha" ] \
        && ok "MP workflow leaf read A's AGENTS.md marker alpha" \
        || bad "MP workflow leaf read marker '$LEAF_MARKER' (expected alpha)"

      # --- Persistent member delivery in B (cwd-routing, P3) -----------------
      # A persistent codex member delivered --project B (from a cwd that is NOT B)
      # runs in B's project_root: the fake codex records B's root and reads B's
      # AGENTS.md "beta". Delivery may report failure (the shim is not a real
      # turn) — the cwd + marker are recorded regardless, which is what we assert.
      : >"$MP_CWD"; : >"$MP_MARKER"
      MEMB="$( cd "$MPELSE" && env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= \
        PATH="$MPBIN:$PATH" "$HARNESS" --project "$ROOT_B" agent create --name mp-worker \
        --role worker --provider codex 2>/dev/null | python3 -c 'import sys,json;print(json.load(sys.stdin).get("id",""))' 2>/dev/null )"
      if [ -z "$MEMB" ]; then
        bad "MP could not create a codex member in B"
      else
        ( cd "$MPELSE" && env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= \
          PATH="$MPBIN:$PATH" "$HARNESS" --project "$ROOT_B" agent send --to "$MEMB" --from lead \
          --content "report your cwd" >/dev/null 2>&1 )
        ( cd "$MPELSE" && env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= \
          PATH="$MPBIN:$PATH" "$HARNESS" --project "$ROOT_B" agent deliver --agent "$MEMB" \
          --start-runtime --timeout-ms 8000 >/dev/null 2>&1 )
        DELIV_CWD="$(cat "$MP_CWD" 2>/dev/null)"
        DELIV_MARKER="$(cat "$MP_MARKER" 2>/dev/null)"
        ROOT_B_REAL="$( cd "$ROOT_B" && pwd -P )"
        [ "$DELIV_CWD" = "$ROOT_B_REAL" ] \
          && ok "MP member delivery in B ran in B's project_root ($DELIV_CWD)" \
          || bad "MP member delivery cwd '$DELIV_CWD' != B project_root ($ROOT_B_REAL)"
        [ "$DELIV_MARKER" = "PROJECT-B-AGENTS-beta" ] \
          && ok "MP member delivery read B's AGENTS.md marker beta" \
          || bad "MP member delivery read marker '$DELIV_MARKER' (expected beta)"
      fi
    fi
    free_serve_port "$MP_SERVE_PID" "$MPPORT"
    MP_SERVE_PID=""
  fi

  # --- GLOBAL (~/, non-git) project policy (P5) -------------------------------
  # The reserved `_global` project is rooted at HOME (not a git repo). Read-only
  # nodes run; writable/worktree nodes are rejected with an actionable message.
  ( cd "$MPHOME" && MH --project _global init >/dev/null 2>&1 )
  cat >"$TMP/mp-global-write.star" <<'STAR'
workflow("mp-global-write", "a writable node against the non-git global project must fail loud")
phase("edit")
agent("edit", provider = "claude", writable = True, label = "editor")
STAR
  ( cd "$TMP" && MHP --project _global workflow run-script "$TMP/mp-global-write.star" \
    --timeout-ms 8000 >"$TMP/mp-global-write.json" 2>/dev/null )
  [ "$(check_json "$TMP/mp-global-write.json" "d['steps'][0]['status']=='failed' and 'not a git repository' in (d['steps'][0].get('output_summary') or '') and '_global' in (d['steps'][0].get('output_summary') or '')")" = "1" ] \
    && ok "MP _global writable node fails loud (non-git, actionable)" || bad "MP _global writable policy"
  cat >"$TMP/mp-global-read.star" <<'STAR'
workflow("mp-global-read", "a read-only node against the non-git global project runs")
phase("scan")
agent("read and report", provider = "claude", label = "reader")
STAR
  ( cd "$TMP" && MHP --project _global workflow run-script "$TMP/mp-global-read.star" \
    --dry-run >"$TMP/mp-global-read.json" 2>/dev/null )
  [ "$(check_json "$TMP/mp-global-read.json" "d['run']['status']=='completed' and d['steps'][0]['status']=='completed'")" = "1" ] \
    && ok "MP _global read-only node runs successfully" || bad "MP _global read-only node"

  # --- Migration of a legacy repo-local .harness (P7) -------------------------
  # Seed a repo with a repo-local .harness store, migrate it, and prove every
  # record + payload is copied to the central store with NO data loss, and the old
  # store is MARKED (not deleted). All under the isolated home.
  MPLEG="$MPHOME/legacy-repo"; mkdir -p "$MPLEG/.harness/provider-sessions/sess-x"
  printf '%s\n' '{"id":"g-legacy","title":"legacy goal"}'   >"$MPLEG/.harness/goals.jsonl"
  printf '%s\n' '{"id":"m-legacy","name":"legacy member"}'  >"$MPLEG/.harness/members.jsonl"
  printf 'legacy-payload\n' >"$MPLEG/.harness/provider-sessions/sess-x/codex.json"
  ( cd "$MPLEG" && MH project migrate >"$TMP/mp-migrate.json" 2>/dev/null )
  MIG_OK="$(check_json "$TMP/mp-migrate.json" "d.get('migrated') is True and d['records_after']==d['records_before'] and d['records_before']>=2")"
  MIG_ID="$(python3 -c "import json;print(json.load(open('$TMP/mp-migrate.json'))['project_id'])" 2>/dev/null)"
  MIG_DST="$MPHH/projects/$MIG_ID"
  if [ "$MIG_OK" = "1" ]; then
    ok "MP migrate preserved record count (no data loss; records_after==records_before)"
  else
    bad "MP migrate record-count preservation (see $TMP/mp-migrate.json)"
  fi
  { [ -s "$MIG_DST/goals.jsonl" ] && [ -s "$MIG_DST/members.jsonl" ] \
    && [ "$(cat "$MIG_DST/provider-sessions/sess-x/codex.json" 2>/dev/null)" = "legacy-payload" ]; } \
    && ok "MP migrate copied ledgers + provider-session payload to central store" \
    || bad "MP migrate central-store contents incomplete"
  { [ -s "$MPLEG/.harness/MIGRATED_TO_CENTRAL" ] && [ -s "$MPLEG/.harness/goals.jsonl" ]; } \
    && ok "MP migrate marked (did not delete) the old local store" \
    || bad "MP migrate old-store marker/retention"
fi

# ---------------------------------------------------------------------------
if [ $REAL -eq 1 ]; then
  section "Live checks (real codex + serve on :$PORT)"

  if ! command -v codex >/dev/null 2>&1; then
    skip "codex CLI not found — skipping all live checks"
  else
    cargo build -p harness-cli >/dev/null 2>&1
    [ -x "$HARNESS" ] || { bad "build harness-cli"; }

    # P1: real (non-dry-run) workflow.
    mkdir -p "$STORE"
    "$HARNESS" workflow run-script "$REPO_ROOT/scripts/verify-fixes.star" \
      --store "$STORE" --timeout-ms 300000 >"$TMP/p1.json" 2>"$TMP/p1.log"
    if [ "$(check_json "$TMP/p1.json" "d.get('run',{}).get('status')=='completed'")" = "1" ]; then
      ok "P1 workflow completed (real codex, no dry-run)"
    else
      bad "P1 workflow did not complete (see $TMP/p1.log)"
    fi
    # #5 + #2 + #6: verdict ok is True ONLY if the schema'd dict carried a real bool+int.
    [ "$(check_json "$TMP/p1.json" "d['run']['final_output']['verdict']['ok'] is True")" = "1" ] \
      && ok "#5/#2 schema coercion -> real bool+int (verdict ok)" || bad "#5/#2 schema coercion / final-message structured"
    [ "$(check_json "$TMP/p1.json" "d['run']['final_output']['result']['typed']['ok'] is True and d['run']['final_output']['result']['typed']['n']==7")" = "1" ] \
      && ok "#5 typed.ok is bool True and typed.n is int 7" || bad "#5 typed value types"
    [ "$(check_json "$TMP/p1.json" "d['run']['final_output']['result']['pipe_len']==1")" = "1" ] \
      && ok "#4 bare-positional pipeline ran" || bad "#4 bare-positional pipeline"
    [ "$(check_json "$TMP/p1.json" "'reason' in d['run']['final_output']['verdict']")" = "1" ] \
      && ok "#6 positional verdict reason captured" || bad "#6 positional verdict reason"
    # #1: every codex leaf finished (a stdin wedge would leave a step not-ok / time out).
    [ "$(check_json "$TMP/p1.json" "all(s['ok'] for s in d['run']['final_output']['steps'])")" = "1" ] \
      && ok "#1 all codex leaves finished (no stdin wedge)" || bad "#1 a codex leaf stalled"
    # #7: both SAME-LABEL writable nodes ('wdup') got their own worktree and succeeded.
    [ "$(check_json "$TMP/p1.json" "len([s for s in d['run']['final_output']['steps'] if s['label']=='wdup'])==2 and all(s['ok'] for s in d['run']['final_output']['steps'] if s['label']=='wdup')")" = "1" ] \
      && ok "#7 same-label parallel writable nodes both got a worktree" || bad "#7 same-label writable nodes collided"

    CMD_SID="$(python3 -c "import json; d=json.load(open('$TMP/p1.json')); print(next((s['provider_session_id'] for s in d['run']['final_output']['steps'] if s['label']=='cmd'), ''))" 2>/dev/null)"

    # P2: serve the run's store.
    "$HARNESS" serve --addr "127.0.0.1:$PORT" --store "$STORE" >"$TMP/serve.log" 2>&1 &
    SERVE_PID=$!
    UP=0
    for _ in $(seq 1 20); do
      curl -fs "http://127.0.0.1:$PORT/v1/snapshot" >/dev/null 2>&1 && { UP=1; break; }
      sleep 0.3
    done
    if [ $UP -ne 1 ]; then
      bad "serve did not come up on :$PORT (see $TMP/serve.log)"
    else
      ok "serve up on :$PORT"

      # Gap 5 A2: codex command_execution -> tool_call + tool_result.
      curl -fs "http://127.0.0.1:$PORT/v1/provider-sessions/$CMD_SID/normalized-events" >"$TMP/a2.json" 2>/dev/null
      [ "$(check_json "$TMP/a2.json" "any(e['kind']=='tool_call' for e in d['events']) and any(e['kind']=='tool_result' and (e.get('tool_result') or {}).get('content') for e in d['events'])")" = "1" ] \
        && ok "A2 codex command -> tool_call + non-empty tool_result" || bad "A2 codex command fidelity"
      [ "$(check_json "$TMP/a2.json" "all(e.get('raw_provider_event') is not None for e in d['events'])")" = "1" ] \
        && ok "A2 raw_provider_event retained on every event" || bad "A2 raw retention"

      # F0: historical normalized endpoint reports retained + normalizes.
      curl -fs "http://127.0.0.1:$PORT/v1/sessions/$CMD_SID/normalized-events" >"$TMP/f0.json" 2>/dev/null
      [ "$(check_json "$TMP/f0.json" "d.get('retained') is True and len(d.get('events',[]))>0")" = "1" ] \
        && ok "F0 historical normalized endpoint (retained=true)" || bad "F0 historical normalized endpoint"

      # A3: a REAL claude session (if one exists locally) -> multi-block expansion,
      # one tool_result per tool_call, all correlated.
      A3SID="$(python3 -c "
import glob,os
best=''; bestn=0
for f in sorted(glob.glob('$REPO_ROOT/.harness/provider-sessions/*/claude.stream-json.ndjson'), reverse=True):
    sid=os.path.basename(os.path.dirname(f)); n=sum(1 for _ in open(f))
    if n>bestn: best,bestn=sid,n
print(best)" 2>/dev/null)"
      if [ -n "$A3SID" ]; then
        grep -F "\"$A3SID\"" "$REPO_ROOT/.harness/provider_sessions.jsonl" >>"$STORE/provider_sessions.jsonl" 2>/dev/null
        cp -r "$REPO_ROOT/.harness/provider-sessions/$A3SID" "$STORE/provider-sessions/" 2>/dev/null
        curl -fs "http://127.0.0.1:$PORT/v1/provider-sessions/$A3SID/normalized-events" >"$TMP/a3.json" 2>/dev/null
        [ "$(check_json "$TMP/a3.json" "(lambda evs: (lambda calls,res: len(res)>0 and len(res)==sum(1 for e in evs if e['kind']=='tool_call') and res<=calls)({(e.get('tool_call') or {}).get('id') for e in evs if e['kind']=='tool_call'}, {(e.get('tool_result') or {}).get('tool_call_id') for e in evs if e['kind']=='tool_result'}))(d['events'])")" = "1" ] \
          && ok "A3 claude multi-block: tool_results correlate 1:1 to tool_calls" || bad "A3 claude multi-block expansion"
      else
        skip "A3 — no local claude session to normalize"
      fi

      # B: live `provider_turn_event_normalized` frames during a fresh delivery.
      AG="$("$HARNESS" --store "$STORE" agent create --name verify-live --role worker --provider codex 2>/dev/null | python3 -c "import sys,json;print(json.load(sys.stdin).get('id',''))" 2>/dev/null)"
      if [ -n "$AG" ]; then
        curl -sN --max-time 120 "http://127.0.0.1:$PORT/v1/events" >"$TMP/sse.txt" 2>/dev/null &
        SSE_PID=$!
        sleep 1
        MSG="$("$HARNESS" --store "$STORE" message send --from "$AG" --to "$AG" --content "Run the shell command: ls crates ; then summarize in one sentence." 2>/dev/null | python3 -c "import sys,json;print(json.load(sys.stdin).get('id',''))" 2>/dev/null)"
        "$HARNESS" --store "$STORE" agent deliver --agent "$AG" --message "$MSG" --start-runtime --timeout-ms 180000 >/dev/null 2>&1
        sleep 2
        N="$(grep -c '^event: provider_turn_event_normalized$' "$TMP/sse.txt" 2>/dev/null || echo 0)"
        [ "${N:-0}" -ge 1 ] && ok "B live SSE emitted $N provider_turn_event_normalized frame(s)" || bad "B live SSE normalized frames (got ${N:-0})"
      else
        skip "B — could not create a codex agent"
      fi
    fi

    # P3: the planning-model LIVE loop — plan -> compile -> run a real codex
    # worker in a worktree -> gate -> advance the goal. This is the goal-level
    # acceptance ("accepted only when the full loop runs live").
    PML="$TMP/pml"; mkdir -p "$PML"
    HL() { "$HARNESS" --store "$PML" "$@"; }
    HL goal create --id live --title "live planning loop" --owner lead --priority p1 >/dev/null 2>&1
    HL goal phase-add --goal live --phase-id p1 --name Build \
      --intent "create a marker file proving the live planning loop ran" >/dev/null 2>&1
    HL task create --id mk --goal live --title "make marker" \
      --objective "create the marker file" --owner lead --phase-id p1 --owned-path verify_marker.txt \
      --design "Create a file named verify_marker.txt at the repo root whose only line is PLANNING_LIVE_OK." >/dev/null 2>&1
    HL goal run-phases live --timeout-ms 300000 >"$TMP/pml_run.json" 2>"$TMP/pml.log"
    [ "$(check_json "$TMP/pml_run.json" "d.get('status')=='completed' and d.get('stage')=='verified'")" = "1" ] \
      && ok "P3 planning live loop completed (real codex run-phases)" || bad "P3 planning live loop (see $TMP/pml.log)"
    [ -f "$PML/goal_orchestration_runs.jsonl" ] \
      && ok "P3 live orchestration checkpoint persisted" || bad "P3 live checkpoint"
    grep -q '"workflow_name":"phase-p1"' "$PML/workflow_runs.jsonl" 2>/dev/null \
      && ok "P3 phase compiled + dispatched a real workflow run" || bad "P3 phase workflow run"

    # P4: goal-phase-landing (L2) DURABLE LANDING e2e. A real run-phases whose
    # writable task creates a file must LEAVE that file COMMITTED on the branch —
    # not lost to the dropped worktree. Point harness at a DEDICATED temp git repo
    # via --project so the landing commit (`phase <id> landed (run-phases)`) lands
    # THERE, keeping this repo clean. The orchestrator's repo_root is the project
    # root, so landing applies the writable task's worktree diff + makes one commit
    # and records landed_commit on the phase.
    PLR="$TMP/land-store"; mkdir -p "$PLR"
    PLREPO="$TMP/land-repo"
    mkdir -p "$PLREPO"
    ( cd "$PLREPO" && git init -q && git config user.email v@v.v && git config user.name v \
        && git commit -q --allow-empty -m init ) >/dev/null 2>&1
    PLREPO_REAL="$( cd "$PLREPO" && pwd -P )"
    HD() { env HARNESS_ROOT= HARNESS_PROJECT= "$HARNESS" --store "$PLR" --project "$PLREPO" "$@"; }
    HD goal create --id land --title "live landing" --owner lead --priority p1 >/dev/null 2>&1
    HD goal phase-add --goal land --phase-id p1 --name Build \
      --intent "create a committed marker proving durable landing" \
      --acceptance "landed_durable.txt is committed on the branch" >/dev/null 2>&1
    HD task create --id ld --goal land --title "make landing marker" \
      --objective "create the landing marker file" --owner lead --phase-id p1 \
      --owned-path landed_durable.txt \
      --design "Create a file named landed_durable.txt at the repo root whose only line is LANDING_DURABLE_OK." >/dev/null 2>&1
    HD goal run-phases land --timeout-ms 300000 >"$TMP/land_run.json" 2>"$TMP/land.log"
    [ "$(check_json "$TMP/land_run.json" "d.get('status')=='completed'")" = "1" ] \
      && ok "P4 durable-landing run-phases completed (real codex)" || bad "P4 durable-landing run-phases (see $TMP/land.log)"
    # The writable file is COMMITTED on the branch and present in the working tree.
    if git -C "$PLREPO_REAL" cat-file -e HEAD:landed_durable.txt 2>/dev/null && [ -f "$PLREPO_REAL/landed_durable.txt" ]; then
      ok "P4 writable file committed on the branch + present in working tree (not lost to worktree)"
    else
      bad "P4 landed file missing from HEAD/working tree (see $TMP/land.log)"
    fi
    # The HEAD commit IS the canonical per-phase landing commit.
    LAND_SUBJ="$(git -C "$PLREPO_REAL" log -1 --pretty=%s 2>/dev/null)"
    [ "$LAND_SUBJ" = "phase p1 landed (run-phases)" ] \
      && ok "P4 HEAD is the per-phase landing commit" || bad "P4 landing commit subject '$LAND_SUBJ'"
    # landed_commit was recorded on the phase and equals the landing commit sha.
    HD goal show --goal land >"$TMP/land_goal.json" 2>/dev/null
    LAND_HEAD="$(git -C "$PLREPO_REAL" rev-parse HEAD 2>/dev/null)"
    [ "$(check_json "$TMP/land_goal.json" "next((p for p in d['phases'] if p['id']=='p1'), {}).get('landed_commit')=='$LAND_HEAD'")" = "1" ] \
      && ok "P4 landed_commit recorded on the phase == landing commit sha" || bad "P4 landed_commit recording"
  fi
else
  section "Live checks"
  skip "live tier not run (pass --real to exercise real codex + serve + SSE)"
fi

# ---------------------------------------------------------------------------
echo
echo "== verify-fixes: $PASS passed, $FAIL failed, $SKIP skipped =="
[ $FAIL -eq 0 ]

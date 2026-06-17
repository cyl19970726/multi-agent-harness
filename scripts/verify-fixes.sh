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

cleanup() {
  [ -n "$SSE_PID" ] && kill "$SSE_PID" 2>/dev/null
  [ -n "$SERVE_PID" ] && kill "$SERVE_PID" 2>/dev/null
  [ -n "$MP_SERVE_PID" ] && kill "$MP_SERVE_PID" 2>/dev/null
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
  offsets_and_broadcasts_independent_across_projects ; do
  grep -q "test .*$t ... ok" "$TMP/test.log" && ok "unit: $t" || bad "unit: $t did not run/pass"
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
fi

# ---------------------------------------------------------------------------
section "Multi-project serve API + #89 convergence (goal-multi-project, no codex)"
# Isolated HOME/HARNESS_HOME so this never touches the developer's real ~/.harness.
# Proves: GET /v1/projects lists both projects + _global; POST /v1/projects/switch
# flips the active project; a CLI from a DIFFERENT cwd then resolves the SAME
# central store (~/.harness/projects/<id>), preserving the #89 sibling-convergence
# invariant across a project switch.
cargo build -p harness-cli >/dev/null 2>&1
if [ ! -x "$HARNESS" ]; then
  bad "build harness-cli for multi-project checks"
else
  MPHOME="$TMP/mp-home"
  MPHH="$MPHOME/.harness"
  mkdir -p "$MPHH"
  MPPORT="${VERIFY_MP_PORT:-8797}"
  # Drive harness against the isolated home; clear inherited store overrides.
  MH() { env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= "$HARNESS" "$@"; }
  ROOT_A="$MPHOME/repo-a"; ROOT_B="$MPHOME/repo-b"
  mkdir -p "$ROOT_A" "$ROOT_B"
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
    # Start serve from repo-a's cwd against the isolated home.
    ( cd "$ROOT_A" && env HOME="$MPHOME" HARNESS_HOME="$MPHH" HARNESS_ROOT= HARNESS_PROJECT= \
      "$HARNESS" serve --addr "127.0.0.1:$MPPORT" --no-truncate >"$TMP/mp-serve.log" 2>&1 ) &
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
    fi
    [ -n "$MP_SERVE_PID" ] && kill "$MP_SERVE_PID" 2>/dev/null
    MP_SERVE_PID=""
  fi
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
  fi
else
  section "Live checks"
  skip "live tier not run (pass --real to exercise real codex + serve + SSE)"
fi

# ---------------------------------------------------------------------------
echo
echo "== verify-fixes: $PASS passed, $FAIL failed, $SKIP skipped =="
[ $FAIL -eq 0 ]

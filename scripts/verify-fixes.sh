#!/usr/bin/env bash
# Real regression verification for the Issue #107 Gap 5 (canonical event model)
# and Issue #139 (workflow ergonomics + codex reliability) fixes. Prints a
# PASS/FAIL matrix and exits nonzero on any failed check.
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
  one_line_can_broadcast_multiple_frames ; do
  grep -q "test .*$t ... ok" "$TMP/test.log" && ok "unit: $t" || bad "unit: $t did not run/pass"
done

npx tsc -p apps/agent-dashboard/tsconfig.json --noEmit >/dev/null 2>&1
[ $? -eq 0 ] && ok "dashboard tsc --noEmit" || bad "dashboard tsc --noEmit"

npx vite build --config apps/agent-dashboard/vite.config.ts --outDir "$TMP/web" --emptyOutDir >/dev/null 2>&1
[ $? -eq 0 ] && ok "dashboard vite build" || bad "dashboard vite build"

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
  fi
else
  section "Live checks"
  skip "live tier not run (pass --real to exercise real codex + serve + SSE)"
fi

# ---------------------------------------------------------------------------
echo
echo "== verify-fixes: $PASS passed, $FAIL failed, $SKIP skipped =="
[ $FAIL -eq 0 ]

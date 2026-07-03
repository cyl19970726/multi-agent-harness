#!/usr/bin/env bash
# Acceptance: a fresh user can INSTALL the authoring skill kit (star-workflow
# + star-goal + star-planner) and RUN the harness. Models the external-user
# journey with checkable outcomes; exits nonzero on any failed check.
#
#   scripts/acceptance-skill-install.sh            # local: install + build + serve + run
#   scripts/acceptance-skill-install.sh --remote   # also: raw URL 200 + anonymous public clone
#
# Local checks need no network (install from this repo, dry-run worker). The
# --remote checks need the repo PUSHED + PUBLIC (they exercise the curl|bash path
# a stranger uses).
set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RAW_URL="https://raw.githubusercontent.com/cyl19970726/multi-agent-harness/master/scripts/install-skill.sh"
CLONE_URL="https://github.com/cyl19970726/multi-agent-harness.git"
REMOTE=0
[ "${1:-}" = "--remote" ] && REMOTE=1

PASS=0
FAIL=0
ok()  { echo "  ✓ $1"; PASS=$((PASS + 1)); }
bad() { echo "  ✗ $1"; FAIL=$((FAIL + 1)); }

WORK="$(mktemp -d)"
SV=""
cleanup() { [ -n "$SV" ] && { kill "$SV" 2>/dev/null; wait "$SV" 2>/dev/null; }; rm -rf "$WORK"; }
trap cleanup EXIT

echo "== A1: install the skill kit into a clean project (from this repo) =="
PROJ="$WORK/proj"
mkdir -p "$PROJ"
if bash "$REPO_ROOT/scripts/install-skill.sh" --agent both --dest "$PROJ" >/dev/null 2>&1; then
  ok "install-skill.sh --agent both succeeded"
else
  bad "install-skill.sh exited nonzero"
fi
# All three shipped skills must land as REAL files (never a symlink) under BOTH
# the Claude (.claude/skills) and Codex (.agents/skills) roots, each with its
# SKILL.md and Codex agents/openai.yaml.
for name in star-workflow star-goal star-planner; do
  for d in .claude/skills .agents/skills; do
    if [ -f "$PROJ/$d/$name/SKILL.md" ] && [ ! -L "$PROJ/$d/$name" ]; then
      ok "$d/$name installed as real files"
    else
      bad "$d/$name missing or a symlink"
    fi
    [ -f "$PROJ/$d/$name/agents/openai.yaml" ] \
      && ok "$d/$name/agents/openai.yaml copied (Codex config)" \
      || bad "$d/$name/agents/openai.yaml missing"
  done
done
# star-workflow additionally ships runnable examples/.
[ "$(ls "$PROJ/.claude/skills/star-workflow/examples" 2>/dev/null | wc -l | tr -d ' ')" -ge 3 ] \
  && ok "star-workflow examples copied" || bad "star-workflow examples missing"

echo "== A2: build the harness binary =="
BIN="$REPO_ROOT/target/debug/harness"
if [ ! -x "$BIN" ]; then
  ( cd "$REPO_ROOT" && cargo build -q -p harness-cli ) >/dev/null 2>&1 || true
fi
[ -x "$BIN" ] && ok "harness binary present" || bad "harness binary missing (cargo build failed?)"

ROOT="$WORK/store"
STAR="$WORK/acc.star"
cat > "$STAR" <<'STAREOF'
workflow("acceptance", "scan then a two-way parallel audit, dry-run for acceptance")
phase("scan")
s = agent("scope the audit")
phase("audit")
parallel([{"prompt": "audit a: " + s}, {"prompt": "audit b: " + s, "provider": "claude"}])
STAREOF

echo "== A3: run a workflow (dry-run, no spend) is journaled =="
if [ -x "$BIN" ]; then
  OUT="$(HARNESS_ROOT="$ROOT" "$BIN" workflow run-script "$STAR" --dry-run 2>/dev/null)"
  if printf '%s' "$OUT" | python3 -c "import json,sys; d=json.load(sys.stdin); sys.exit(0 if d['run']['status']=='completed' and len(d.get('steps',[]))==3 else 1)" 2>/dev/null; then
    ok "run-script --dry-run completed with 3 steps"
  else
    bad "run-script did not complete as expected"
  fi
  [ -s "$ROOT/workflow_runs.jsonl" ] && [ -s "$ROOT/workflow_steps.jsonl" ] \
    && ok "run journaled to the store" || bad "store rows missing"
else
  bad "skipped run (no binary)"
fi

echo "== A4: serve exposes the run via the API =="
if [ -x "$BIN" ]; then
  PORT=8791
  HARNESS_ROOT="$ROOT" "$BIN" serve --addr "127.0.0.1:$PORT" >/dev/null 2>&1 &
  SV=$!
  sleep 1.5
  curl -fsS -m 5 "http://127.0.0.1:$PORT/v1/workflows" >/dev/null 2>&1 \
    && ok "serve API responds (/v1/workflows)" || bad "serve API down"
  if curl -fsS -m 5 "http://127.0.0.1:$PORT/v1/snapshot" 2>/dev/null \
      | python3 -c "import json,sys; d=json.load(sys.stdin); sys.exit(0 if len(d.get('workflow_runs',[]))>=1 else 1)" 2>/dev/null; then
    ok "the run is readable from /v1/snapshot"
  else
    bad "run not visible in snapshot"
  fi
  kill "$SV" 2>/dev/null; wait "$SV" 2>/dev/null; SV=""
else
  bad "skipped serve (no binary)"
fi

if [ "$REMOTE" = "1" ]; then
  echo "== A5: anonymous download + install (repo must be public) =="
  code="$(curl -fsSL -o "$WORK/dl.sh" -w "%{http_code}" "$RAW_URL" 2>/dev/null || true)"
  [ "$code" = "200" ] && ok "raw install-skill.sh reachable (HTTP 200)" \
    || bad "raw script HTTP ${code:-000} (repo private / not pushed?)"
  if GIT_TERMINAL_PROMPT=0 git clone -q --depth 1 "$CLONE_URL" "$WORK/anon" 2>/dev/null \
      && [ -f "$WORK/anon/skills/star-workflow/SKILL.md" ]; then
    ok "anonymous public clone carries skills/star-workflow"
  else
    bad "anonymous public clone failed"
  fi
  if [ -s "$WORK/dl.sh" ]; then
    bash "$WORK/dl.sh" --agent both --dest "$WORK/anonproj" >/dev/null 2>&1 || true
    [ -f "$WORK/anonproj/.claude/skills/star-workflow/SKILL.md" ] \
      && [ -f "$WORK/anonproj/.agents/skills/star-workflow/SKILL.md" ] \
      && ok "anonymous one-liner install works end to end" || bad "anonymous one-liner install failed"
  fi
fi

echo ""
echo "acceptance: $PASS passed, $FAIL failed"
[ "$FAIL" = "0" ]

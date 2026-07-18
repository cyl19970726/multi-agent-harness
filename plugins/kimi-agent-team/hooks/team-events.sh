#!/usr/bin/env bash
# team-events.sh — Kimi Code SessionStart/Stop hook for the Agent Team plugin.
#
# Injects a one-line summary of the active AgentTeamRun (run id / status /
# member count / un-ACKed deliveries / Team Console URL) into the host
# session, so the host is always aware of live team state and the web entry
# point. Registered for SessionStart and Stop with a 10s timeout.
#
# Contract: fail-open. Missing harness CLI, bad JSON, no python3, or any
# other failure must exit 0 silently and never block the session.

set -uo pipefail

# Drain the hook event payload from stdin (required; the payload is unused —
# both SessionStart and Stop get the same one-line injection).
cat >/dev/null 2>&1 || true

harness_bin="${HARNESS_BIN:-harness}"
command -v "$harness_bin" >/dev/null 2>&1 || exit 0

list_json="$("$harness_bin" team-run list --json 2>/dev/null)" || exit 0
[[ -n "$list_json" ]] || exit 0

console_url="${TEAM_CONSOLE_URL:-http://127.0.0.1:8787/team-console}"

LIST_JSON="$list_json" CONSOLE_URL="$console_url" python3 - 2>/dev/null <<'PY'
import json, os, sys

raw = os.environ.get("LIST_JSON", "").strip()
if not raw:
    sys.exit(0)
try:
    data = json.loads(raw)
except ValueError:
    sys.exit(0)

# Accept either a top-level array or {"runs": [...]}.
runs = data.get("runs", data) if isinstance(data, dict) else data
if not isinstance(runs, list):
    sys.exit(0)

ACTIVE = {"planning", "running", "waiting", "reviewing", "blocked"}
active = [r for r in runs if isinstance(r, dict) and r.get("status") in ACTIVE]
if not active:
    sys.exit(0)

run = active[-1]
run_id = run.get("id", "?")
status = run.get("status", "?")
members = run.get("member_run_ids") or run.get("members") or []
unacked = run.get("unacked_count", run.get("unacked", 0)) or 0
objective = str(run.get("objective") or "")[:60]
console = os.environ.get("CONSOLE_URL", "http://127.0.0.1:8787/team-console")

parts = ["[agent-team] active run {}".format(run_id), "status={}".format(status)]
if members:
    parts.append("members={}".format(len(members)))
if unacked:
    parts.append("unacked={}".format(unacked))
parts.append("console={}".format(console))
if objective:
    parts.append('objective="{}"'.format(objective))
print(" ".join(parts) + " — /agent-team:status for the cockpit view")
PY

exit 0

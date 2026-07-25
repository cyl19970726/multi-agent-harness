#!/usr/bin/env bash
# Fail-open lifecycle telemetry plus SessionStart active-run orientation.
set -uo pipefail

payload="$(cat 2>/dev/null || true)"
harness_bin="${HARNESS_BIN:-harness}"
command -v "$harness_bin" >/dev/null 2>&1 || exit 0

# Forward bound lifecycle events to Harness. Core ingestion owns sanitization;
# unbound raw hook payloads are deliberately not persisted by this plugin.
if [[ -n "${HARNESS_AGENT_MEMBER_ID:-}" ]]; then
  args=(hook record --agent "$HARNESS_AGENT_MEMBER_ID")
  if [[ -n "${HARNESS_AGENT_RUNTIME_ID:-}" ]]; then
    args+=(--runtime "$HARNESS_AGENT_RUNTIME_ID")
  fi
  printf '%s' "$payload" | "$harness_bin" "${args[@]}" >/dev/null 2>&1 || true
fi

event_name="$(printf '%s' "$payload" |
  sed -n 's/.*"hook_event_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' |
  head -1)"
case "$event_name" in
  SessionStart|session_start|"")
    ;;
  *)
    exit 0
    ;;
esac

list_json="$("$harness_bin" team-run list --json 2>/dev/null)" || exit 0
[[ -n "$list_json" ]] || exit 0

LIST_JSON="$list_json" python3 - 2>/dev/null <<'PY'
import json
import os

try:
    data = json.loads(os.environ.get("LIST_JSON", ""))
except ValueError:
    raise SystemExit(0)

runs = data.get("runs", data) if isinstance(data, dict) else data
if not isinstance(runs, list):
    raise SystemExit(0)

active_states = {"planning", "running", "waiting", "reviewing", "blocked"}
active = [run for run in runs
          if isinstance(run, dict) and run.get("status") in active_states]
if not active:
    raise SystemExit(0)

run = active[-1]
run_id = run.get("id", "?")
status = run.get("status", "?")
members = run.get("member_run_ids") or run.get("members") or []
project = run.get("project_id") or os.environ.get("HARNESS_PROJECT") or ""
parts = [
    "[star-harness]",
    f"active TeamRun={run_id}",
    f"status={status}",
    f"members={len(members)}",
]
if project:
    parts.append(f"project={project}")
parts.append("use `harness team-run status --id " + str(run_id) + "`")
print(" ".join(parts))
PY

exit 0

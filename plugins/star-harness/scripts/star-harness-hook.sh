#!/usr/bin/env bash
# Fail-open lifecycle telemetry plus bounded Host Inbox orientation.
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
    show_active_run=1
    ;;
  UserPromptSubmit|user_prompt_submit)
    show_active_run=0
    ;;
  *)
    exit 0
    ;;
esac

# A bound Member session owns its own Inbox. Never inject the Lead Inbox into a
# Member merely because both use the same provider plugin.
[[ -n "${HARNESS_AGENT_MEMBER_ID:-}" ]] && exit 0

list_json="$("$harness_bin" team-run list --json 2>/dev/null)" || exit 0
[[ -n "$list_json" ]] || exit 0

LIST_JSON="$list_json" python3 - 2>/dev/null <<'PY' |
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
active = [
    run
    for run in runs
    if isinstance(run, dict) and run.get("status") in active_states
]
# Hooks have a short timeout and are orientation only. Prefer the most recent
# active runs; the canonical CLI remains the complete view.
for run in active[-5:]:
    run_id = str(run.get("id", ""))
    if not run_id:
        continue
    status = str(run.get("status", "?"))
    members = run.get("member_run_ids") or run.get("members") or []
    project = str(run.get("project_id") or os.environ.get("HARNESS_PROJECT") or "")
    print("\t".join((run_id, status, str(len(members)), project)))
PY
while IFS=$'\t' read -r run_id status member_count project; do
  [[ -n "$run_id" ]] || continue

  if [[ "$show_active_run" == "1" ]]; then
    parts="[star-harness] active TeamRun=$run_id status=$status members=$member_count"
    [[ -n "$project" ]] && parts="$parts project=$project"
    printf '%s\n' "$parts use \`harness team-run status --id $run_id\`"
  fi

  inbox_json="$("$harness_bin" team-run inbox --id "$run_id" \
    --member-run-id host --json 2>/dev/null)" || continue
  [[ -n "$inbox_json" ]] || continue
  INBOX_JSON="$inbox_json" TEAM_RUN_ID="$run_id" python3 - 2>/dev/null <<'PY'
import json
import os
import re

try:
    data = json.loads(os.environ.get("INBOX_JSON", ""))
except ValueError:
    raise SystemExit(0)

messages = data.get("messages", data) if isinstance(data, dict) else data
if not isinstance(messages, list) or not messages:
    raise SystemExit(0)

run_id = os.environ.get("TEAM_RUN_ID", "?")
print(f"[star-harness] Needs you: TeamRun={run_id} pending_host_messages={len(messages)}")
for message in messages[:3]:
    if not isinstance(message, dict):
        continue
    sender = message.get("from_member_id", "?")
    kind = message.get("kind", "message")
    message_id = message.get("id", "?")
    correlation = message.get("correlation_id", "?")
    body = re.sub(r"\s+", " ", str(message.get("body", ""))).strip()
    if len(body) > 120:
        body = body[:117] + "..."
    print(
        f"- from={sender} kind={kind} message={message_id} "
        f"correlation={correlation}: {body}"
    )
if len(messages) > 3:
    print(f"- ... and {len(messages) - 3} more")
print(
    "  read/ack with `harness team-run inbox --id "
    + run_id
    + " --member-run-id host --json`"
)
PY
done

exit 0

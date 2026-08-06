#!/usr/bin/env bash
# Fail-open lifecycle telemetry plus native-thread-scoped Host Inbox delivery.
set -uo pipefail

payload="$(cat 2>/dev/null || true)"
firm_bin="${FIRM_BIN:-harness}"
command -v "$firm_bin" >/dev/null 2>&1 || exit 0

hook_fields="$(
  HOOK_PAYLOAD="$payload" python3 - 2>/dev/null <<'PY'
import json
import os

try:
    value = json.loads(os.environ.get("HOOK_PAYLOAD", "") or "{}")
except ValueError:
    value = {}
items = []
for key in ("hook_event_name", "session_id", "turn_id", "stop_hook_active"):
    item = value.get(key, "")
    if isinstance(item, bool):
        item = "true" if item else "false"
    items.append(str(item).replace("|", " ").replace("\n", " "))
print("|".join(items))
PY
)"
IFS='|' read -r event_name session_id turn_id stop_hook_active <<<"$hook_fields"
stop_hook_active="${stop_hook_active:-false}"

# Resolve the native surface before either Member telemetry or Host Inbox
# handling. The same discriminator also supplies the explicit provider identity
# for bound Member hook events; hook ingestion must never guess Codex merely
# because no provider flag was passed.
if [[ -n "${HARNESS_HOST_SURFACE:-}" ]]; then
  host_surface="$HARNESS_HOST_SURFACE"
elif [[ -n "${KIMI_PLUGIN_ROOT:-}" ]]; then
  host_surface="kimi-cli"
elif [[ -n "${CLAUDE_PLUGIN_ROOT:-}" ]]; then
  host_surface="claude-code"
else
  host_surface="codex-app"
fi
case "$host_surface" in
  codex-app|codex*) hook_provider="codex" ;;
  claude-code|claude*) hook_provider="claude" ;;
  kimi-cli|kimi*) hook_provider="kimi" ;;
  *) hook_provider="${FIRM_PROVIDER:-codex}" ;;
esac

# Export firm host binding so downstream `firm team-run create` /
# `firm team-run start` can auto-bind when these env vars are present and
# unambiguous. Hook subprocesses inherit them; the user's shell must source
# them separately for standalone CLI use.
export STAR_FIRM_HOST_SURFACE="$host_surface"
export STAR_FIRM_HOST_THREAD_ID="$session_id"

# Forward bound lifecycle events to Harness. Core ingestion owns sanitization;
# unbound raw hook payloads are deliberately not persisted by this plugin.
if [[ -n "${FIRM_AGENT_MEMBER_ID:-}" ]]; then
  args=(hook record --provider "$hook_provider" --agent "$FIRM_AGENT_MEMBER_ID")
  if [[ -n "${FIRM_AGENT_RUNTIME_ID:-}" ]]; then
    args+=(--runtime "$FIRM_AGENT_RUNTIME_ID")
  fi
  printf '%s' "$payload" | "$firm_bin" "${args[@]}" >/dev/null 2>&1 || true
  # Codex Stop requires JSON stdout. A Member owns its own Inbox and must never
  # receive the Lead Inbox simply because both use the same provider plugin.
  if [[ "$event_name" == "Stop" && -n "$turn_id" ]]; then
    printf '{}\n'
  fi
  exit 0
fi

# External interactive Member inbox push. A user-driven interactive session
# (execution_mode=external_interactive) binds with FIRM_TEAM_RUN_ID +
# FIRM_MEMBER_RUN_ID and receives its queued coordination mail as native
# context; Harness never drives this Member, so this hook is its only push
# channel. Driven Members (FIRM_AGENT_MEMBER_ID above) own their inbox
# through the Supervisor and must never enter this branch.
if [[ -n "${FIRM_MEMBER_RUN_ID:-}" ]]; then
  member_run_id="$FIRM_MEMBER_RUN_ID"
  team_run_id="${FIRM_TEAM_RUN_ID:-}"
  if [[ -z "$team_run_id" || -z "$session_id" ]]; then
    # Without the run binding or native identity there is no safe mailbox to
    # inject, so fail open rather than reading every run.
    if [[ "$event_name" == "Stop" && -n "$turn_id" ]]; then
      printf '{}\n'
    fi
    exit 0
  fi
  case "$event_name" in
    SessionStart|session_start)
      show_binding=1
      ;;
    UserPromptSubmit|user_prompt_submit)
      show_binding=0
      ;;
    Stop|stop)
      show_binding=0
      ;;
    *)
      exit 0
      ;;
  esac

  # Environment variables are routing hints, not proof that this session owns
  # an external MemberRun. Verify the durable binding before reading any mail
  # so a stale or mistyped id cannot intake a driven Member's Inbox.
  member_detail_json="$("$firm_bin" member-run show \
    --id "$member_run_id" --json 2>/dev/null)" || {
    if [[ "$event_name" == "Stop" && -n "$turn_id" ]]; then
      printf '{}\n'
    fi
    exit 0
  }
  if ! MEMBER_DETAIL_JSON="$member_detail_json" TEAM_RUN_ID="$team_run_id" \
    MEMBER_RUN_ID="$member_run_id" python3 - 2>/dev/null <<'PY'
import json
import os

try:
    detail = json.loads(os.environ.get("MEMBER_DETAIL_JSON", "") or "{}")
except ValueError:
    raise SystemExit(1)
member = detail.get("member_run", {}) if isinstance(detail, dict) else {}
profile = member.get("provider_profile", {}) if isinstance(member, dict) else {}
valid = (
    member.get("id") == os.environ.get("MEMBER_RUN_ID")
    and member.get("team_run_id") == os.environ.get("TEAM_RUN_ID")
    and profile.get("execution_mode") == "external_interactive"
    and profile.get("execution_driver") == "user_driven"
    and member.get("coordination_status", "active") == "active"
    and member.get("status") not in {"completed", "failed", "stopped"}
)
raise SystemExit(0 if valid else 1)
PY
  then
    if [[ "$event_name" == "Stop" && -n "$turn_id" ]]; then
      printf '{}\n'
    fi
    exit 0
  fi

  member_inbox_json="$("$firm_bin" team-run inbox \
    --id "$team_run_id" --member-run-id "$member_run_id" --json 2>/dev/null)" || {
    if [[ "$event_name" == "Stop" && -n "$turn_id" ]]; then
      printf '{}\n'
    fi
    exit 0
  }

  if [[ "$event_name" == "Stop" || "$event_name" == "stop" ]]; then
    # A user-driven external session must remain free to stop by default.
    # Operators who explicitly want queued mail to continue the same native
    # task may opt in for this session. This is cooperative hook behavior, not
    # a Harness lifecycle-control claim.
    case "${HARNESS_EXTERNAL_AUTO_CONTINUE:-false}" in
      1|true|TRUE|yes|YES|on|ON) external_auto_continue=true ;;
      *) external_auto_continue=false ;;
    esac
    if [[ "$external_auto_continue" != "true" ]]; then
      [[ "$host_surface" != "kimi-cli" ]] && printf '{}\n'
      exit 0
    fi
    if [[ "$stop_hook_active" == "true" ]]; then
      [[ "$host_surface" != "kimi-cli" ]] && printf '{}\n'
      exit 0
    fi
    if [[ "$host_surface" == "codex-app" && -z "$turn_id" ]]; then
      printf '{}\n'
      exit 0
    fi
    continuation_json="$(
      INBOX_JSON="$member_inbox_json" TEAM_RUN_ID="$team_run_id" \
      MEMBER_RUN_ID="$member_run_id" python3 - 2>/dev/null <<'PY'
import json
import os
import re

try:
    entries = json.loads(os.environ.get("INBOX_JSON", ""))
except ValueError:
    entries = []
messages = [m for m in entries if isinstance(m, dict)] if isinstance(entries, list) else []
if not messages:
    print("{}")
    raise SystemExit(0)

team_run_id = os.environ.get("TEAM_RUN_ID", "?")
member_run_id = os.environ.get("MEMBER_RUN_ID", "?")
lines = [
    "Firm Member Inbox received new coordination mail while this Member was busy.",
    "Process it now in the same native task. Read the full message before deciding; "
    "reply in its correlation when needed, then ACK only after it has entered your working context.",
]
for message in messages[:5]:
    body = re.sub(r"\s+", " ", str(message.get("body", ""))).strip()
    if len(body) > 180:
        body = body[:177] + "..."
    lines.append(
        f"- from={message.get('from_member_id', '?')} "
        f"kind={message.get('kind', 'message')} message={message.get('id', '?')} "
        f"correlation={message.get('correlation_id', '?')}: {body}"
    )
if len(messages) > 5:
    lines.append(f"- ... and {len(messages) - 5} more; use `firm team-run inbox "
                 f"--id {team_run_id} --member-run-id {member_run_id} --json`.")
lines.append(
    f"Use `firm team-run ack --id {team_run_id} --message-id <message-id> "
    f"--member-id {member_run_id}` after intake. Do not treat transport ACK as semantic acceptance."
)
print(json.dumps({"decision": "block", "reason": "\n".join(lines)}))
PY
    )"
    if [[ "$host_surface" == "kimi-cli" ]]; then
      continuation_reason="$(
        CONTINUATION_JSON="$continuation_json" python3 - 2>/dev/null <<'PY'
import json
import os
try:
    value = json.loads(os.environ.get("CONTINUATION_JSON", "") or "{}")
except ValueError:
    value = {}
print(value.get("reason", "") if value.get("decision") == "block" else "")
PY
      )"
      if [[ -n "$continuation_reason" ]]; then
        printf '%s\n' "$continuation_reason" >&2
        exit 2
      fi
      exit 0
    fi
    if [[ -z "$continuation_json" ]]; then
      continuation_json='{}'
    fi
    printf '%s\n' "$continuation_json"
    exit 0
  fi

  INBOX_JSON="$member_inbox_json" TEAM_RUN_ID="$team_run_id" \
  MEMBER_RUN_ID="$member_run_id" SHOW_BINDING="$show_binding" python3 - 2>/dev/null <<'PY'
import json
import os
import re

try:
    entries = json.loads(os.environ.get("INBOX_JSON", ""))
except ValueError:
    entries = []
messages = [m for m in entries if isinstance(m, dict)] if isinstance(entries, list) else []

team_run_id = os.environ.get("TEAM_RUN_ID", "?")
member_run_id = os.environ.get("MEMBER_RUN_ID", "?")
if os.environ.get("SHOW_BINDING") == "1":
    print(f"[firm] External member binding: team_run={team_run_id} member_run={member_run_id}")
if not messages:
    raise SystemExit(0)
print(f"[firm] Member mail: TeamRun={team_run_id} pending_member_messages={len(messages)}")
for message in messages[:3]:
    body = re.sub(r"\s+", " ", str(message.get("body", ""))).strip()
    if len(body) > 120:
        body = body[:117] + "..."
    print(
        f"- from={message.get('from_member_id', '?')} "
        f"kind={message.get('kind', 'message')} message={message.get('id', '?')} "
        f"correlation={message.get('correlation_id', '?')}: {body}"
    )
if len(messages) > 3:
    print(f"- ... and {len(messages) - 3} more")
print(
    f"  read with `firm team-run inbox --id {team_run_id} "
    f"--member-run-id {member_run_id} --json`; ACK only after intake"
)
PY
  exit 0
fi

case "$event_name" in
  SessionStart|session_start)
    show_binding=1
    ;;
  UserPromptSubmit|user_prompt_submit)
    show_binding=0
    ;;
  Stop|stop)
    show_binding=0
    ;;
  *)
    exit 0
    ;;
esac

if [[ -z "$session_id" ]]; then
  # Only Codex Stop requires structured stdout. Without native identity there
  # is no safe mailbox to inject, so fail open rather than reading every run.
  if [[ "$event_name" == "Stop" && -n "$turn_id" ]]; then
    printf '{}\n'
  fi
  exit 0
fi

inbox_json="$("$firm_bin" team-run host-inbox \
  --surface "$host_surface" --thread-id "$session_id" --json 2>/dev/null)" || {
  if [[ "$event_name" == "Stop" && -n "$turn_id" ]]; then
    printf '{}\n'
  fi
  exit 0
}

if [[ "$event_name" == "Stop" || "$event_name" == "stop" ]]; then
  # Stop is the provider-reviewed safe boundary for an external Host task.
  # Codex identifies it with turn_id and consumes structured JSON. Claude Code
  # consumes the same decision=block shape without turn_id. Kimi shell hooks
  # block with exit 2 and read the reason from stderr. All three expose
  # stop_hook_active to cap continuation at one pass.
  if [[ "$stop_hook_active" == "true" ]]; then
    [[ "$host_surface" != "kimi-cli" ]] && printf '{}\n'
    exit 0
  fi
  if [[ "$host_surface" == "codex-app" && -z "$turn_id" ]]; then
    printf '{}\n'
    exit 0
  fi
  continuation_json="$(
    INBOX_JSON="$inbox_json" HOST_SURFACE="$host_surface" python3 - 2>/dev/null <<'PY'
import json
import os
import re

try:
    entries = json.loads(os.environ.get("INBOX_JSON", ""))
except ValueError:
    entries = []
messages = []
for entry in entries if isinstance(entries, list) else []:
    if not isinstance(entry, dict):
        continue
    run_id = str(entry.get("team_run_id", ""))
    for message in entry.get("messages", []):
        if isinstance(message, dict):
            messages.append((run_id, message))
if not messages:
    print("{}")
    raise SystemExit(0)

lines = [
    "Firm Host Inbox received new coordination mail while this Host was busy.",
    "Process it now in the same native task. Read the full message before deciding; "
    "reply in its correlation when needed, then ACK only after it has entered your working context.",
]
for run_id, message in messages[:5]:
    body = re.sub(r"\s+", " ", str(message.get("body", ""))).strip()
    if len(body) > 180:
        body = body[:177] + "..."
    lines.append(
        f"- TeamRun={run_id} from={message.get('from_member_id', '?')} "
        f"kind={message.get('kind', 'message')} message={message.get('id', '?')} "
        f"correlation={message.get('correlation_id', '?')}: {body}"
    )
if len(messages) > 5:
    lines.append(f"- ... and {len(messages) - 5} more; use `firm team-run host-inbox "
                 f"--surface {os.environ.get('HOST_SURFACE', '?')} "
                 f"--thread-id <session-id> --json`.")
lines.append(
    "Use `firm team-run ack --id <team-run-id> --message-id <message-id> "
    "--member-id host` after intake. Do not treat transport ACK as semantic acceptance."
)
print(json.dumps({"decision": "block", "reason": "\n".join(lines)}))
PY
  )"
  if [[ "$host_surface" == "kimi-cli" ]]; then
    continuation_reason="$(
      CONTINUATION_JSON="$continuation_json" python3 - 2>/dev/null <<'PY'
import json
import os
try:
    value = json.loads(os.environ.get("CONTINUATION_JSON", "") or "{}")
except ValueError:
    value = {}
print(value.get("reason", "") if value.get("decision") == "block" else "")
PY
    )"
    if [[ -n "$continuation_reason" ]]; then
      printf '%s\n' "$continuation_reason" >&2
      exit 2
    fi
    exit 0
  fi
  if [[ -z "$continuation_json" ]]; then
    continuation_json='{}'
  fi
  printf '%s\n' "$continuation_json"
  exit 0
fi

INBOX_JSON="$inbox_json" HOST_SURFACE="$host_surface" HOST_SESSION_ID="$session_id" \
SHOW_BINDING="$show_binding" python3 - 2>/dev/null <<'PY'
import json
import os
import re

try:
    entries = json.loads(os.environ.get("INBOX_JSON", ""))
except ValueError:
    entries = []
if not isinstance(entries, list):
    entries = []

surface = os.environ.get("HOST_SURFACE", "?")
session_id = os.environ.get("HOST_SESSION_ID", "?")
if os.environ.get("SHOW_BINDING") == "1":
    print(f"[firm] Host native binding: surface={surface} thread={session_id}")
    print(
        "  New TeamRuns must use `--host-surface "
        + surface
        + " --host-thread-id "
        + session_id
        + "`; bind an existing run with `firm team-run bind-host`."
    )

for entry in entries:
    if not isinstance(entry, dict):
        continue
    run_id = str(entry.get("team_run_id", "?"))
    messages = entry.get("messages", [])
    if not isinstance(messages, list) or not messages:
        continue
    print(f"[firm] Needs you: TeamRun={run_id} pending_host_messages={len(messages)}")
    for message in messages[:3]:
        if not isinstance(message, dict):
            continue
        body = re.sub(r"\s+", " ", str(message.get("body", ""))).strip()
        if len(body) > 120:
            body = body[:117] + "..."
        print(
            f"- from={message.get('from_member_id', '?')} "
            f"kind={message.get('kind', 'message')} message={message.get('id', '?')} "
            f"correlation={message.get('correlation_id', '?')}: {body}"
        )
    if len(messages) > 3:
        print(f"- ... and {len(messages) - 3} more")
    print(
        "  read with `firm team-run inbox --id "
        + run_id
        + " --member-run-id host --json`; ACK only after intake"
    )
PY

exit 0

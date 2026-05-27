#!/usr/bin/env bash
set -euo pipefail

payload="$(cat)"

if [[ -n "${HARNESS_ROOT:-}" && -n "${HARNESS_AGENT_MEMBER_ID:-}" ]]; then
  harness_bin="${HARNESS_BIN:-harness}"
  args=(hook record --agent "$HARNESS_AGENT_MEMBER_ID")
  if [[ -n "${HARNESS_AGENT_RUNTIME_ID:-}" ]]; then
    args+=(--runtime "$HARNESS_AGENT_RUNTIME_ID")
  fi
  if [[ -n "${HARNESS_TASK_ID:-}" ]]; then
    args+=(--task "$HARNESS_TASK_ID")
  fi
  printf '%s' "$payload" | "$harness_bin" "${args[@]}" >/dev/null
  exit 0
fi

data_root="${PLUGIN_DATA:-}"
if [[ -z "$data_root" ]]; then
  exit 0
fi

mkdir -p "$data_root/unbound-events"
stamp="$(date +%s%3N 2>/dev/null || date +%s)"
event_name="$(printf '%s' "$payload" | sed -n 's/.*"hook_event_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -1)"
if [[ -z "$event_name" ]]; then
  event_name="unknown"
fi
printf '%s\n' "$payload" > "$data_root/unbound-events/${stamp}-${event_name}.json"

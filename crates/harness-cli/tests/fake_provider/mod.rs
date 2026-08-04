//! Shared test helper: a FAKE provider binary (`codex` / `claude`) that records
//! the cwd it was spawned in, so persistent-delivery cwd tests can prove the
//! harness spawns the worker in the SELECTED project's `project_root` — not the
//! harness process cwd — without invoking a real provider (goal-multi-project P3,
//! Stage 3).
//!
//! The harness spawns providers by BARE NAME (`Command::new("codex")` /
//! `Command::new("claude")`), so prepending a dir holding an executable shim to
//! `PATH` intercepts the spawn. The shim writes its `$PWD` to a known file and
//! emits one harmless NDJSON line so `run_ndjson_child` has something to read,
//! then exits. We assert on the recorded cwd, not on delivery success.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::harness_env::clear_inherited_native_harness_env;

/// Create a `bin/` dir containing an executable shim named `provider` (e.g.
/// `codex` or `claude`) that, when run, writes its current working directory to
/// `cwd_marker` and emits a single NDJSON line on stdout. Returns the `bin/` dir
/// to prepend to `PATH`.
///
/// `which <provider>` (used by the harness when starting the runtime) also
/// resolves to this shim, so `--start-runtime` reports the provider as available.
pub fn install_provider_shim(base: &Path, provider: &str, cwd_marker: &Path) -> PathBuf {
    install_provider_shim_capturing(base, provider, cwd_marker, None)
}

/// Like [`install_provider_shim`] but, when `capture_file` is `Some((name, dst))`,
/// the shim also copies the content of `<cwd>/<name>` (if present) to `dst`. This
/// proves a provider can READ a project-root file (e.g. `CLAUDE.md`) from the cwd
/// the harness spawned it in — the whole point of cwd routing.
pub fn install_provider_shim_capturing(
    base: &Path,
    provider: &str,
    cwd_marker: &Path,
    capture_file: Option<(&str, &Path)>,
) -> PathBuf {
    let bin_dir = base.join(format!("fakebin-{provider}"));
    fs::create_dir_all(&bin_dir).expect("mk fake bin dir");
    let shim_path = bin_dir.join(provider);
    // POSIX shell shim. `pwd -P` resolves symlinks so the recorded path matches a
    // canonicalized project root. The NDJSON line keeps the reader happy; its
    // content is irrelevant to the cwd assertion.
    let mut script = String::from("#!/bin/sh\n");
    script.push_str(&format!(
        "pwd -P > {marker}\n",
        marker = shell_single_quote(&cwd_marker.display().to_string()),
    ));
    if let Some((name, dst)) = capture_file {
        // Copy the named file from the cwd to `dst` iff it exists (cat is run
        // relative to the shim's cwd — the project root the harness chose).
        script.push_str(&format!(
            "if [ -f {name} ]; then cat {name} > {dst}; fi\n",
            name = shell_single_quote(name),
            dst = shell_single_quote(&dst.display().to_string()),
        ));
    }
    script.push_str("printf '%s\\n' '{\"type\":\"fake\"}'\nexit 0\n");
    fs::write(&shim_path, script).expect("write shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim_path).expect("stat shim").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim_path, perms).expect("chmod shim");
    }
    bin_dir
}

/// Read the cwd a shim recorded, trimmed. Panics if the marker was never written
/// (the provider shim never ran), which itself is a useful failure signal.
pub fn read_recorded_cwd(cwd_marker: &Path) -> PathBuf {
    let raw = fs::read_to_string(cwd_marker)
        .unwrap_or_else(|e| panic!("provider shim never recorded a cwd at {cwd_marker:?}: {e}"));
    let trimmed = raw.trim();
    assert!(!trimmed.is_empty(), "recorded cwd was empty");
    PathBuf::from(trimmed)
}

/// Single-quote a string for safe inclusion in a POSIX shell script.
fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Create a `bin/` dir holding a fake `kimi` executable speaking just enough
/// line-delimited ACP JSON-RPC (stdio) for `team-run start` integration tests.
///
/// The shim answers `initialize` / `session/new` with canned results, and for
/// `session/prompt` streams (in order): one `agent_thought_chunk` (eligible
/// only for the volatile live preview; never journaled), one `tool_call` + terminal
/// `tool_call_update`, one `agent_message_chunk` carrying a `## RESULT` /
/// `## SUMMARY` report, then the terminal `{"result":{"stopReason":...}}`
/// response. `FAKE_KIMI_RESULT` (done|blocked|failed, default done) selects
/// the RESULT word so tests can drive both run outcomes. Prepend the returned
/// dir to PATH so [`resolve_kimi_bin`] picks the shim over a real install.
pub fn install_kimi_acp_shim(base: &Path) -> PathBuf {
    let bin_dir = base.join("fakebin-kimi");
    fs::create_dir_all(&bin_dir).expect("mk fake kimi bin dir");
    let shim_path = bin_dir.join("kimi");
    // printf format strings: `\\n` emits a literal backslash-n (a JSON escape
    // inside string values); a trailing `\n` emits the record newline.
    let script = r###"#!/bin/sh
# Fake `kimi acp` (Agent Team v0 tests): line-delimited JSON-RPC over stdio.
result="${FAKE_KIMI_RESULT:-done}"
ask="${FAKE_KIMI_ASK:-0}"
version="${FAKE_KIMI_VERSION:-0.0.0}"
if [ -n "${FAKE_KIMI_ENV_MARKER:-}" ]; then
  env | grep '^HARNESS_' | sort > "$FAKE_KIMI_ENV_MARKER"
fi
if [ "$1" != "acp" ]; then
  echo "fake kimi: only 'acp' is implemented" >&2
  exit 2
fi
session_id="session_fake_$$"
mode="default"
prompt_count=0
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{},"authMethods":[],"agentInfo":{"name":"fake-kimi","version":"%s"}}}\n' "$id" "$version"
      ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"%s","configOptions":[{"type":"select","id":"model","currentValue":"k2.5","options":[{"value":"k2.5","name":"K2.5"},{"value":"qwen/qwen3.8-max","name":"Qwen 3.8 Max"}]},{"type":"select","id":"thinking","currentValue":"high","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"},{"value":"max","name":"Max"}]}]}}\n' "$id" "$session_id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}}\n' "$session_id"
      ;;
    *'"method":"session/load"'*)
      session_id=$(printf '%s' "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
      if [ -n "${FAKE_KIMI_ATTACH_MARKER:-}" ]; then
        printf 'load %s\n' "$session_id" >> "$FAKE_KIMI_ATTACH_MARKER"
      fi
      if [ "${FAKE_KIMI_LOAD_REPLAY:-0}" = "1" ]; then
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"STALE_HISTORY_REPLAY"}}}}\n' "$session_id"
      fi
      printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":[{"type":"select","id":"model","currentValue":"k2.5","options":[{"value":"k2.5","name":"K2.5"},{"value":"qwen/qwen3.8-max","name":"Qwen 3.8 Max"}]},{"type":"select","id":"thinking","currentValue":"high","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"},{"value":"max","name":"Max"}]}]}}\n' "$id"
      ;;
    *'"method":"session/resume"'*)
      if [ "${FAKE_KIMI_RESUME_UNSUPPORTED:-0}" = "1" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"Method not found"}}\n' "$id"
        continue
      fi
      session_id=$(printf '%s' "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
      if [ -n "${FAKE_KIMI_ATTACH_MARKER:-}" ]; then
        printf 'resume %s\n' "$session_id" >> "$FAKE_KIMI_ATTACH_MARKER"
      fi
      printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":[{"type":"select","id":"model","currentValue":"k2.5","options":[{"value":"k2.5","name":"K2.5"},{"value":"qwen/qwen3.8-max","name":"Qwen 3.8 Max"}]},{"type":"select","id":"thinking","currentValue":"high","options":[{"value":"low","name":"Low"},{"value":"high","name":"High"},{"value":"max","name":"Max"}]}]}}\n' "$id"
      ;;
    *'"method":"session/set_config_option"'*)
      if [ -n "${FAKE_KIMI_CONTROL_MARKER:-}" ]; then
        printf '%s\n' "$line" >> "$FAKE_KIMI_CONTROL_MARKER"
      fi
      case "$line" in
        *'"configId":"model"'*'"value":"qwen/qwen3.8-max"'*)
          # A model-switch receipt carries the NEW model's option set. Its
          # thinking values intentionally exclude the old K3-only `max` so
          # tests can prove stale controls are not inherited across models.
          if [ "${FAKE_KIMI_MODEL_SWITCH_NO_REFRESH:-0}" = "1" ]; then
            printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
          else
            printf '{"jsonrpc":"2.0","id":%s,"result":{"configOptions":[{"type":"select","id":"model","currentValue":"qwen/qwen3.8-max","options":[{"value":"k2.5","name":"K2.5"},{"value":"qwen/qwen3.8-max","name":"Qwen 3.8 Max"}]},{"type":"select","id":"thinking","currentValue":"on","options":[{"value":"on","name":"On"},{"value":"off","name":"Off"}]}]}}\n' "$id"
          fi
          ;;
        *'"configId":"model"'*'"value":"k2.5"'*)
          printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
          ;;
        *'"configId":"thinking"'*'"value":"low"'*|*'"configId":"thinking"'*'"value":"high"'*|*'"configId":"thinking"'*'"value":"max"'*)
          printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
          ;;
        *'"configId":"mode"'*'"value":"plan"'*|*'"configId":"mode"'*'"value":"default"'*)
          mode=$(printf '%s' "$line" | sed -n 's/.*"value":"\\([^"]*\\)".*/\\1/p')
          printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
          ;;
        *)
          printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32602,"message":"unexpected fake model selection"}}\n' "$id"
          ;;
      esac
      ;;
    *'"method":"session/prompt"'*)
      prompt_id="$id"
      prompt_count=$((prompt_count + 1))
      if [ -n "${FAKE_KIMI_PROMPT_MARKER:-}" ]; then
        printf '%s\n' "$line" >> "$FAKE_KIMI_PROMPT_MARKER"
      fi
      if [ "$prompt_count" = "1" ] && [ -n "${FAKE_KIMI_FIRST_PROMPT_READY:-}" ]; then
        : > "$FAKE_KIMI_FIRST_PROMPT_READY"
      fi
      if [ "$prompt_count" = "1" ] && [ -n "${FAKE_KIMI_FIRST_PROMPT_RELEASE:-}" ]; then
        while [ ! -e "$FAKE_KIMI_FIRST_PROMPT_RELEASE" ]; do
          sleep 0.02
        done
      fi
      if [ "${FAKE_KIMI_WAIT:-0}" = "1" ]; then
        continue
      fi
      if [ "$mode" = "plan" ]; then
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"plan","entries":[{"content":"Inspect the Work contract","status":"completed"},{"content":"Implement only after Host review","status":"pending"},{"content":"Run focused checks","status":"pending"}]}}}\n' "$session_id"
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"1. Inspect the Work contract\\n2. Implement only after Host review\\n3. Run focused checks\\n"}}}}\n' "$session_id"
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
        continue
      fi
      if [ -n "${FAKE_KIMI_REJECT_BEFORE_UPDATE_MARKER:-}" ] && [ ! -e "${FAKE_KIMI_REJECT_BEFORE_UPDATE_MARKER}" ]; then
        # Immediate non-retryable rejection with NO preceding session/update:
        # the provider never accepted the prompt, so Harness must not publish
        # a provider receipt for it and must leave the delivery replayable.
        : > "${FAKE_KIMI_REJECT_BEFORE_UPDATE_MARKER}"
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"provider API 429: rate limited before the turn started"}}\n' "$id"
        continue
      fi
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hidden reasoning"}}}}\n' "$session_id"
      if [ "${FAKE_KIMI_KEEP_WORK_ACTIVE:-0}" = "1" ] && [ "$prompt_count" = "1" ]; then
        # Mirror a real member's first durable action: after the provider has
        # accepted the prompt, start its assigned Work. An outputless terminal
        # response then makes the supervisor continue that same responsibility
        # immediately, reproducing the quota-loop failure deterministically.
        sleep 0.1
        work_json=$("$HARNESS_BIN" --project "$HARNESS_PROJECT_ID" team-run work list \
          --team-run-id "$HARNESS_TEAM_RUN_ID" \
          --member-run-id "$HARNESS_MEMBER_RUN_ID")
        work_id=$(printf '%s\n' "$work_json" | sed -n 's/.*"id": "\([^"]*\)".*/\1/p' | sed -n '1p')
        work_version=$(printf '%s\n' "$work_json" | sed -n 's/.*"version": \([0-9][0-9]*\).*/\1/p' | sed -n '1p')
        "$HARNESS_BIN" --project "$HARNESS_PROJECT_ID" team-run work start \
          --team-run-id "$HARNESS_TEAM_RUN_ID" \
          --work-id "$work_id" \
          --member-run-id "$HARNESS_MEMBER_RUN_ID" \
          --expected-version "$work_version" >/dev/null
      fi
      if [ "${FAKE_KIMI_QUOTA_ERROR:-0}" = "1" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"provider API 403: quota exceeded"}}\n' "$id"
        continue
      fi
      if [ -n "${FAKE_KIMI_PROMPT_ERROR_ONCE_MARKER:-}" ] && [ ! -e "${FAKE_KIMI_PROMPT_ERROR_ONCE_MARKER}" ]; then
        # One non-retryable provider failure after partial content streamed:
        # the terminal session/prompt response is a JSON-RPC error. Harness
        # must record a provider_error round, not a partial Handoff.
        : > "${FAKE_KIMI_PROMPT_ERROR_ONCE_MARKER}"
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32000,"message":"provider API 403: usage limit reached"}}\n' "$id"
        continue
      fi
      if [ "${FAKE_KIMI_EMPTY_TERMINAL:-0}" = "1" ] && [ "${FAKE_KIMI_REAL_ON_PROMPT:-0}" != "$prompt_count" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
        continue
      fi
      if [ -n "${FAKE_KIMI_PEER_ACK_CONFIG:-}" ] && [ -s "${FAKE_KIMI_PEER_ACK_CONFIG}" ] && [ "$prompt_count" = "2" ]; then
        # Two-peer convergence: the named member answers its follow-up round
        # with acknowledgement-only peer mail (informational, no explicit
        # response intent). The config file holds "<from member run>\n<to member run>".
        ack_from=$(sed -n '1p' "${FAKE_KIMI_PEER_ACK_CONFIG}")
        ack_to=$(sed -n '2p' "${FAKE_KIMI_PEER_ACK_CONFIG}")
        if [ "${HARNESS_MEMBER_RUN_ID:-}" = "$ack_from" ]; then
          sleep 0.1
          work_id=$("$HARNESS_BIN" --project "$HARNESS_PROJECT_ID" team-run work list \
            --team-run-id "$HARNESS_TEAM_RUN_ID" \
            --member-run-id "$HARNESS_MEMBER_RUN_ID" \
            | sed -n 's/.*"id": "\([^"]*\)".*/\1/p' | sed -n '1p')
          "$HARNESS_BIN" --project "$HARNESS_PROJECT_ID" team-run send \
            --id "$HARNESS_TEAM_RUN_ID" \
            --from "$HARNESS_MEMBER_RUN_ID" \
            --to "$ack_to" \
            --kind message \
            --body "ACK: noted, no reply needed" \
            --work-id "$work_id" \
            --correlation-id "corr-peer-$work_id" \
            > "${FAKE_KIMI_PEER_ACK_MARKER:?}" 2>&1
        fi
      fi
      if [ -n "${FAKE_KIMI_CRASH_ONCE_MARKER:-}" ] && [ ! -e "$FAKE_KIMI_CRASH_ONCE_MARKER" ]; then
        : > "$FAKE_KIMI_CRASH_ONCE_MARKER"
        exit 7
      fi
      if [ "${FAKE_KIMI_MESSAGE_DURING_TURN:-0}" = "1" ]; then
        # Give the Harness reader a deterministic chance to consume the first
        # ACP frame and publish its WorkDelivery receipt before this bound
        # member authors a Work-linked conversation message from the turn.
        sleep 0.1
        work_id=$("$HARNESS_BIN" --project "$HARNESS_PROJECT_ID" team-run work list \
          --team-run-id "$HARNESS_TEAM_RUN_ID" \
          --member-run-id "$HARNESS_MEMBER_RUN_ID" \
          | sed -n 's/.*"id": "\([^"]*\)".*/\1/p' | sed -n '1p')
        "$HARNESS_BIN" --project "$HARNESS_PROJECT_ID" team-run send \
          --id "$HARNESS_TEAM_RUN_ID" \
          --from "$HARNESS_MEMBER_RUN_ID" \
          --to host \
          --kind message \
          --work-id "$work_id" \
          --body "PROGRESS: explicit Work-linked update during active ACP turn" \
          > "${FAKE_KIMI_MESSAGE_MARKER:?}" 2>&1
      fi
      if [ "$ask" = "1" ]; then
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"12:ask-user","title":"AskUserQuestion","kind":"other","status":"in_progress"}}}\n' "$session_id"
        printf '{"jsonrpc":"2.0","id":700,"method":"session/request_permission","params":{"sessionId":"%s","options":[{"optionId":"q0_opt_0","name":"Use native contract","kind":"allow_once"},{"optionId":"q0_skip","name":"Skip","kind":"reject_once"}],"toolCall":{"toolCallId":"12:ask-user","title":"AskUserQuestion","content":[{"type":"content","content":{"type":"text","text":"Which implementation should be used?"}}]}}}\n' "$session_id"
      elif [ "$ask" = "approval" ] || [ "$ask" = "approval_twice" ]; then
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"13:bash","title":"Bash","kind":"execute","status":"in_progress"}}}\n' "$session_id"
        printf '{"jsonrpc":"2.0","id":701,"method":"session/request_permission","params":{"sessionId":"%s","options":[{"optionId":"tool_allow_once","name":"Allow once","kind":"allow_once"},{"optionId":"tool_reject_once","name":"Reject","kind":"reject_once"}],"toolCall":{"toolCallId":"13:bash","title":"Bash","content":[{"type":"content","content":{"type":"text","text":"Run the requested command?"}}]}}}\n' "$session_id"
      elif [ "$ask" = "approval_reject_only" ]; then
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"15:bash","title":"Bash","kind":"execute","status":"in_progress"}}}\n' "$session_id"
        printf '{"jsonrpc":"2.0","id":703,"method":"session/request_permission","params":{"sessionId":"%s","options":[{"optionId":"tool_reject_once","name":"Reject","kind":"reject_once"}],"toolCall":{"toolCallId":"15:bash","title":"Bash","content":[{"type":"content","content":{"type":"text","text":"A provider request with no allow option"}}]}}}\n' "$session_id"
      elif [ "$ask" = "unknown" ]; then
        printf '{"jsonrpc":"2.0","id":704,"method":"session/request_permission","params":{"sessionId":"%s","options":[],"toolCall":{"toolCallId":"16:unknown","title":"UnknownCapability","content":[]}}}\n' "$session_id"
      else
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"tool-1","title":"fake_edit","kind":"edit","status":"in_progress"}}}\n' "$session_id"
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"tool-1","status":"completed"}}}\n' "$session_id"
        if [ "${FAKE_KIMI_CONCATENATED_REPORT:-0}" = "1" ]; then
          printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"ordinary narration with no trailing newline"}}}}\n' "$session_id"
        fi
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"## RESULT\\n%s\\n## SUMMARY\\nfake member finished round\\n"}}}}\n' "$session_id" "$result"
        # FAKE_KIMI_STOP_REASON exercises non-`end_turn` terminal reasons
        # (max_tokens/refusal/max_turn_requests). FAKE_KIMI_NULL_ERROR_KEY
        # reproduces servers that serialize every field, so a SUCCESSFUL
        # response still carries `"error": null`.
        stop_reason="${FAKE_KIMI_STOP_REASON:-end_turn}"
        if [ "${FAKE_KIMI_NULL_ERROR_KEY:-0}" = "1" ]; then
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"%s"},"error":null}\n' "$id" "$stop_reason"
        else
          printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"%s"}}\n' "$id" "$stop_reason"
        fi
      fi
      ;;
    *'"id":700'*'"optionId":"q0_opt_0"'*)
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"12:ask-user","status":"completed"}}}\n' "$session_id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"## RESULT\\n%s\\n## SUMMARY\\nfake member received Lead answer\\n"}}}}\n' "$session_id" "$result"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt_id"
      ;;
    *'"id":701'*'"optionId":"tool_allow_once"'*)
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"13:bash","status":"completed"}}}\n' "$session_id"
      if [ "$ask" = "approval_twice" ]; then
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call","toolCallId":"14:bash","title":"Bash","kind":"execute","status":"in_progress"}}}\n' "$session_id"
        printf '{"jsonrpc":"2.0","id":702,"method":"session/request_permission","params":{"sessionId":"%s","options":[{"optionId":"tool_allow_always","name":"Always allow","kind":"allow_always"},{"optionId":"tool_allow_once_second","name":"Allow once","kind":"allow_once"},{"optionId":"tool_reject_once_second","name":"Reject","kind":"reject_once"}],"toolCall":{"toolCallId":"14:bash","title":"Bash","content":[{"type":"content","content":{"type":"text","text":"Run another requested command?"}}]}}}\n' "$session_id"
      else
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"## RESULT\\n%s\\n## SUMMARY\\nfake member received Policy approval\\n"}}}}\n' "$session_id" "$result"
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt_id"
      fi
      ;;
    *'"id":702'*'"optionId":"tool_allow_always"'*)
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"14:bash","status":"completed"}}}\n' "$session_id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"## RESULT\\n%s\\n## SUMMARY\\nfake member received two Policy acknowledgements\\n"}}}}\n' "$session_id" "$result"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt_id"
      ;;
    *'"id":703'*'"optionId":"tool_reject_once"'*)
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"tool_call_update","toolCallId":"15:bash","status":"failed"}}}\n' "$session_id"
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"## RESULT\\n%s\\n## SUMMARY\\nfake member observed fail-closed Policy denial\\n"}}}}\n' "$session_id" "$result"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt_id"
      ;;
    *'"id":704'*)
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"## RESULT\\n%s\\n## SUMMARY\\nfake member observed fail-closed Human resolution\\n"}}}}\n' "$session_id" "$result"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$prompt_id"
      ;;
    *'"method":"session/cancel"'*)
      if [ -n "${FAKE_KIMI_CANCEL_MARKER:-}" ]; then
        printf '%s\n' "$line" >> "$FAKE_KIMI_CANCEL_MARKER"
      fi
      if printf '%s' "$line" | grep -q '"id":'; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"Method not found"}}\n' "$id"
        continue
      fi
      if [ -n "${prompt_id:-}" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"cancelled"}}\n' "$prompt_id"
      fi
      ;;
  esac
done
exit 0
"###;
    fs::write(&shim_path, script).expect("write fake kimi shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim_path).expect("stat shim").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim_path, perms).expect("chmod shim");
    }
    bin_dir
}

/// Add a deterministic `codex exec --json` shim to `bin_dir`. The stream
/// includes a reasoning item and a final report so Agent Team tests can prove
/// reasoning stays transient while only the explicit outcome is durable.
pub fn install_codex_team_shim(bin_dir: &Path) -> PathBuf {
    fs::create_dir_all(bin_dir).expect("mk fake codex team bin dir");
    let shim_path = bin_dir.join("codex");
    let script = r###"#!/bin/sh
if [ -n "${FAKE_CODEX_ENV_MARKER:-}" ]; then
  env | grep '^HARNESS_' | sort > "$FAKE_CODEX_ENV_MARKER"
fi
if [ "$1" = "--version" ]; then
  printf '%s\n' 'codex-cli 0.145.0-alpha.18'
  exit 0
fi
if [ "$1" = "app-server" ]; then
  thread_id="thread_fake_codex_app_server"
  turn_id="turn_fake_codex_app_server"
  turn_seq=0
  # Capacity fixtures. Assign the defaults here rather than inline in the
  # printf: `${VAR:-{"a":1}}` terminates at the FIRST `}` of the default, which
  # silently corrupts a JSON literal.
  account_json="${FAKE_CODEX_ACCOUNT_JSON}"
  if [ -z "$account_json" ]; then
    account_json='{"account":{"type":"chatgpt","email":"fake@example.com","planType":"pro"},"requiresOpenaiAuth":true}'
  fi
  rate_limits_json="${FAKE_CODEX_RATE_LIMITS_JSON}"
  if [ -z "$rate_limits_json" ]; then
    rate_limits_json='{"rateLimits":{"limitId":"codex","primary":{"usedPercent":7,"windowDurationMins":10080,"resetsAt":1786161121},"secondary":null,"rateLimitReachedType":null,"spendControlReached":false}}'
  fi
  while IFS= read -r line; do
    id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
    case "$line" in
      *'"method":"initialize"'*)
        printf '{"id":%s,"result":{"userAgent":"fake-codex"}}\n' "$id"
        ;;
      *'"method":"account/read"'*)
        printf '{"id":%s,"result":%s}\n' "$id" "$account_json"
        ;;
      *'"method":"account/rateLimits/read"'*)
        printf '{"id":%s,"result":%s}\n' "$id" "$rate_limits_json"
        ;;
      *'"method":"thread/start"'*)
        if [ -n "${FAKE_CODEX_THREAD_MARKER:-}" ]; then
          printf 'thread/start %s\n' "$line" >> "$FAKE_CODEX_THREAD_MARKER"
        fi
        reasoning_effort=$(printf '%s' "$line" | sed -n 's/.*"model_reasoning_effort":"\([^"]*\)".*/\1/p')
        service_tier=$(printf '%s' "$line" | sed -n 's/.*"serviceTier":"\([^"]*\)".*/\1/p')
        reasoning_json=null
        service_json=null
        if [ -n "$reasoning_effort" ]; then reasoning_json="\"$reasoning_effort\""; fi
        if [ -n "$service_tier" ]; then service_json="\"$service_tier\""; fi
        printf '{"id":%s,"result":{"model":"gpt-5.6-sol","reasoningEffort":%s,"serviceTier":%s,"thread":{"id":"%s"}}}\n' "$id" "$reasoning_json" "$service_json" "$thread_id"
        ;;
      *'"method":"thread/resume"'*)
        if [ -n "${FAKE_CODEX_THREAD_MARKER:-}" ]; then
          printf 'thread/resume %s\n' "$line" >> "$FAKE_CODEX_THREAD_MARKER"
        fi
        if [ -n "${FAKE_CODEX_RESUME_MARKER:-}" ]; then
          printf '%s\n' "$line" >> "$FAKE_CODEX_RESUME_MARKER"
        fi
        thread_id=$(printf '%s' "$line" | sed -n 's/.*"threadId":"\([^"]*\)".*/\1/p')
        reasoning_effort=$(printf '%s' "$line" | sed -n 's/.*"model_reasoning_effort":"\([^"]*\)".*/\1/p')
        service_tier=$(printf '%s' "$line" | sed -n 's/.*"serviceTier":"\([^"]*\)".*/\1/p')
        reasoning_json=null
        service_json=null
        if [ -n "$reasoning_effort" ]; then reasoning_json="\"$reasoning_effort\""; fi
        if [ -n "$service_tier" ]; then service_json="\"$service_tier\""; fi
        printf '{"id":%s,"result":{"model":"gpt-5.6-sol","reasoningEffort":%s,"serviceTier":%s,"thread":{"id":"%s","turns":[]}}}\n' "$id" "$reasoning_json" "$service_json" "$thread_id"
        ;;
      *'"method":"thread/name/set"'*)
        if [ -n "${FAKE_CODEX_NAME_MARKER:-}" ]; then
          printf '%s\n' "$line" >> "$FAKE_CODEX_NAME_MARKER"
        fi
        printf '{"id":%s,"result":{}}\n' "$id"
        ;;
      *'"method":"thread/goal/set"'*)
        if [ -n "${FAKE_CODEX_PLAN_MARKER:-}" ]; then
          printf 'goal_set %s\n' "$line" >> "$FAKE_CODEX_PLAN_MARKER"
        fi
        printf '{"id":%s,"result":{"goal":{"objective":"fake work","status":"active"}}}\n' "$id"
        ;;
      *'"method":"turn/start"'*)
        plan_mode=0
        case "$line" in *'"collaborationMode":{"mode":"plan"'*) plan_mode=1 ;; esac
        if [ -n "${FAKE_CODEX_PLAN_MARKER:-}" ]; then
          printf 'turn plan_mode=%s %s\n' "$plan_mode" "$line" >> "$FAKE_CODEX_PLAN_MARKER"
        fi
        turn_seq=$((turn_seq + 1))
        turn_id="turn_fake_codex_app_server_${turn_seq}"
        response_turn_id="$turn_id"
        if [ "${FAKE_CODEX_REBIND_EVENT_TURN:-0}" = "1" ]; then
          response_turn_id="turn_start_response_${turn_seq}"
        fi
        printf '{"id":%s,"result":{"turn":{"id":"%s","status":"inProgress","items":[]}}}\n' "$id" "$response_turn_id"
        if [ "$plan_mode" = "1" ]; then
          if [ "${FAKE_CODEX_STALE_COMPLETION_ON_SECOND_PLAN:-0}" = "1" ] && [ "$turn_seq" = "2" ]; then
            printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"turn_fake_codex_app_server_1","status":"completed","items":[{"id":"stale-plan-app-1","type":"plan","text":"STALE PLAN MUST BE IGNORED"}]}}}\n' "$thread_id"
          fi
          printf '{"method":"turn/plan/updated","params":{"threadId":"%s","turnId":"%s","plan":[{"step":"Revision %s: inspect the Assignment contract","status":"completed"},{"step":"Implement only after Host approval","status":"pending"},{"step":"Run focused checks","status":"pending"}]}}\n' "$thread_id" "$turn_id" "$turn_seq"
          printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed","items":[{"id":"plan-app-%s","type":"plan","text":"Revision %s\\n1. Inspect the Work contract\\n2. Implement only after Host review\\n3. Run focused checks"}]}}}\n' "$thread_id" "$turn_id" "$turn_seq" "$turn_seq"
          continue
        fi
        printf '{"method":"item/started","params":{"threadId":"%s","turnId":"%s","item":{"id":"command-app-1","type":"commandExecution","command":"cargo check","commandActions":[],"cwd":"/tmp","status":"inProgress"}}}\n' "$thread_id" "$turn_id"
        if [ "${FAKE_CODEX_AUTO_COMPLETE:-0}" = "1" ] || { [ "${FAKE_CODEX_AUTO_COMPLETE_AFTER_STEER:-0}" = "1" ] && [ "$turn_seq" -gt "1" ]; }; then
          printf '{"method":"item/agentMessage/delta","params":{"threadId":"%s","turnId":"%s","itemId":"message-app-1","delta":"## RESULT\\ndone\\n## SUMMARY\\nexecuted approved plan\\n"}}\n' "$thread_id" "$turn_id"
          printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed","items":[{"id":"message-app-1","type":"agentMessage","text":"## RESULT\\ndone\\n## SUMMARY\\nexecuted approved plan\\n"}]}}}\n' "$thread_id" "$turn_id"
          if [ "${FAKE_CODEX_EXIT_AFTER_FIRST_TURN:-0}" = "1" ] && [ "$turn_seq" = "1" ]; then
            # FAKE_CODEX_EXIT_ONCE_MARKER (optional): only the first spawned
            # process exits, so a test can model a single transport loss and
            # then let the resumed process keep running turns.
            if [ -z "${FAKE_CODEX_EXIT_ONCE_MARKER:-}" ] || [ ! -f "${FAKE_CODEX_EXIT_ONCE_MARKER}" ]; then
              if [ -n "${FAKE_CODEX_EXIT_ONCE_MARKER:-}" ]; then
                : > "${FAKE_CODEX_EXIT_ONCE_MARKER}"
              fi
              exit 0
            fi
          fi
        elif [ "${FAKE_CODEX_INTERRUPT_WITHOUT_REQUEST:-0}" = "1" ]; then
          printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"interrupted","items":[]}}}\n' "$thread_id" "$turn_id"
        elif [ "${FAKE_CODEX_ASK:-0}" = "1" ]; then
          printf '{"id":700,"method":"item/tool/requestUserInput","params":{"threadId":"%s","turnId":"%s","itemId":"ask-app-1","questions":[{"id":"implementation","header":"Contract","question":"Which implementation should be used?","options":[{"label":"Use native contract","description":"Use the provider-native path."},{"label":"Stop","description":"Do not continue."}]}]}}\n' "$thread_id" "$turn_id"
        fi
        ;;
      *'"method":"turn/steer"'*)
        printf '{"id":%s,"result":{"turnId":"%s"}}\n' "$id" "$turn_id"
        printf '{"method":"item/agentMessage/delta","params":{"threadId":"%s","turnId":"%s","itemId":"message-app-1","delta":"## RESULT\\ndone\\n## SUMMARY\\nsteered app-server member\\n"}}\n' "$thread_id" "$turn_id"
        printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed","items":[{"id":"message-app-1","type":"agentMessage","text":"## RESULT\\ndone\\n## SUMMARY\\nsteered app-server member\\n"}]}}}\n' "$thread_id" "$turn_id"
        ;;
      *'"method":"turn/interrupt"'*)
        printf '{"id":%s,"result":{}}\n' "$id"
        printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"interrupted","items":[]}}}\n' "$thread_id" "$turn_id"
        ;;
      *'"id":700'*'"answers"'*)
        printf '{"method":"item/agentMessage/delta","params":{"threadId":"%s","turnId":"%s","itemId":"message-app-ask","delta":"## RESULT\\ndone\\n## SUMMARY\\nreceived Lead answer\\n"}}\n' "$thread_id" "$turn_id"
        printf '{"method":"turn/completed","params":{"threadId":"%s","turn":{"id":"%s","status":"completed","items":[]}}}\n' "$thread_id" "$turn_id"
        ;;
    esac
  done
  exit 0
fi
printf '%s\n' '{"type":"thread.started","thread_id":"thread_fake_codex_team"}'
printf '%s\n' '{"type":"turn.started"}'
printf '%s\n' '{"type":"item.completed","item":{"id":"reason-1","type":"reasoning","text":"hidden codex reasoning"}}'
printf '%s\n' '{"type":"item.started","item":{"id":"command-1","type":"command_execution","command":"cargo check","status":"in_progress"}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"command-1","type":"command_execution","command":"cargo check","status":"completed","aggregated_output":"ok","exit_code":0}}'
printf '%s\n' '{"type":"item.completed","item":{"id":"message-1","type":"agent_message","text":"## RESULT\ndone\n## SUMMARY\nfake codex member finished round\n"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}'
exit 0
"###;
    fs::write(&shim_path, script).expect("write fake codex team shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim_path).expect("stat shim").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim_path, perms).expect("chmod shim");
    }
    bin_dir.to_path_buf()
}

/// Deterministic Claude Code stream-json shim for provider-native Agent Team
/// coverage. It emits a real session id, hidden thinking, one tool call, and a
/// terminal report; only the session binding and report may become durable.
pub fn install_claude_team_shim(bin_dir: &Path) -> PathBuf {
    fs::create_dir_all(bin_dir).expect("mk fake claude team bin dir");
    let shim_path = bin_dir.join("claude");
    let script = r###"#!/bin/sh
if [ -n "${FAKE_CLAUDE_ENV_MARKER:-}" ]; then
  env | grep '^HARNESS_' | sort > "$FAKE_CLAUDE_ENV_MARKER"
fi
if [ "$1" = "--version" ]; then
  printf '%s\n' '2.1.181 (Claude Code)'
  exit 0
fi
session_id="session_fake_claude_native"
resume=""
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--resume" ]; then shift; resume="$1"; session_id="$1"; fi
  shift
done
printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$session_id\",\"model\":\"fake-claude\"}"
printf '%s\n' '{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hidden claude reasoning"},{"type":"tool_use","id":"tool-claude-1","name":"Read","input":{"file_path":"README.md"}}]}}'
printf '%s\n' '{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-claude-1","content":"provider-owned output"}]}}'
printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"session_id\":\"$session_id\",\"result\":\"## RESULT\\ndone\\n## SUMMARY\\nfake claude member finished round\"}"
exit 0
"###;
    fs::write(&shim_path, script).expect("write fake claude team shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim_path).expect("stat shim").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim_path, perms).expect("chmod shim");
    }
    bin_dir.to_path_buf()
}

/// Claude stream-json shim that opens a native session and then reports an API
/// failure on stdout, matching the real CLI's authentication-error shape.
pub fn install_claude_failure_shim(bin_dir: &Path) -> PathBuf {
    fs::create_dir_all(bin_dir).expect("mk fake claude failure bin dir");
    let shim_path = bin_dir.join("claude");
    let script = r###"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '%s\n' '2.1.181 (Claude Code)'
  exit 0
fi
session_id="session_fake_claude_failed"
printf '%s\n' "{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"$session_id\",\"model\":\"fake-claude\"}"
printf '%s\n' "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"api_error_status\":401,\"session_id\":\"$session_id\",\"result\":\"Failed to authenticate. API Error: 401 Invalid authentication credentials\"}"
exit 0
"###;
    fs::write(&shim_path, script).expect("write fake claude failure shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim_path).expect("stat shim").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim_path, perms).expect("chmod shim");
    }
    bin_dir.to_path_buf()
}

/// One spawned `harness` invocation's result.
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
}

/// Drive a full persistent delivery through the real `harness` binary against a
/// SELECTED project, with a fake provider on `PATH`. Every step runs from
/// `process_cwd` (deliberately != the project root) to prove the worker cwd
/// derives from the selected project, not the harness process cwd.
///
/// `envs` are the base env pairs (HOME / HARNESS_HOME from `TempHome::envs()`);
/// `fake_bin` is prepended to PATH so the provider shim intercepts the spawn.
pub struct DeliveryDriver {
    bin: PathBuf,
    project_root: PathBuf,
    process_cwd: PathBuf,
    envs: Vec<(String, String)>,
    fake_bin: PathBuf,
}

impl DeliveryDriver {
    pub fn new(
        project_root: &Path,
        process_cwd: &Path,
        envs: Vec<(String, String)>,
        fake_bin: &Path,
    ) -> Self {
        Self {
            bin: PathBuf::from(env!("CARGO_BIN_EXE_harness")),
            project_root: project_root.to_path_buf(),
            process_cwd: process_cwd.to_path_buf(),
            envs,
            fake_bin: fake_bin.to_path_buf(),
        }
    }

    fn run(&self, args: &[&str]) -> CliOutput {
        let path = format!(
            "{}:{}",
            self.fake_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(&self.bin);
        command
            .arg("--project")
            .arg(&self.project_root)
            .args(args)
            .current_dir(&self.process_cwd)
            .envs(self.envs.iter().cloned())
            .env("PATH", path);
        clear_inherited_native_harness_env(&mut command);
        let out = command.output().expect("run harness");
        CliOutput {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            success: out.status.success(),
        }
    }

    /// `harness --project <root> init` (registers + activates the project).
    pub fn init_project(&self) {
        let out = self.run(&["init"]);
        assert!(out.success, "init failed: {}", out.stderr);
    }

    /// Create a member for `provider`. When `worktree` is `Some`, pins the member's
    /// workspace via `--worktree`. Returns the new member id.
    pub fn create_member(&self, provider: &str, worktree: Option<&Path>) -> String {
        let mut args = vec![
            "agent",
            "create",
            "--name",
            "worker",
            "--role",
            "worker",
            "--provider",
            provider,
        ];
        let worktree_str;
        if let Some(wt) = worktree {
            worktree_str = wt.display().to_string();
            args.push("--worktree");
            args.push(&worktree_str);
        }
        let out = self.run(&args);
        assert!(out.success, "agent create failed: {}", out.stderr);
        let value: serde_json::Value = serde_json::from_str(&out.stdout)
            .unwrap_or_else(|e| panic!("create stdout not JSON ({e}): {}", out.stdout));
        value["id"]
            .as_str()
            .expect("member id in create output")
            .to_string()
    }

    /// Queue a message for `member_id`.
    pub fn send_message(&self, member_id: &str, content: &str) {
        let out = self.run(&[
            "agent",
            "send",
            "--to",
            member_id,
            "--from",
            "lead",
            "--content",
            content,
        ]);
        assert!(out.success, "agent send failed: {}", out.stderr);
    }

    /// Deliver queued messages to `member_id`, starting the runtime. Returns the
    /// delivery output (delivery may report failure since the shim is not a real
    /// provider; the cwd is recorded regardless).
    pub fn deliver(&self, member_id: &str) -> CliOutput {
        self.run(&[
            "agent",
            "deliver",
            "--agent",
            member_id,
            "--start-runtime",
            "--timeout-ms",
            "5000",
        ])
    }
}

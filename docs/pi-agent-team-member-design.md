# Pi Agent Team Member Integration Design

## Overview

Integrate [pi](https://pi.dev) (the coding agent harness, v0.83.0) as a first-class
Agent Team Member provider in Star Harness, alongside Codex, Claude, and Kimi.

Pi uses **RPC mode** (`pi --mode rpc`) for headless process integration: strict
JSONL (LF-delimited) over stdin/stdout. This is the closest analog to
`codex_app_server` and `kimi_acp`.

---

## 1. Architecture

```
Harness run_member_orchestration()
  └─ run_pi_team_member()              ← new function in main.rs
       └─ PiRpcClient::spawn()         ← new module pi_rpc.rs
            └─ pi --mode rpc            ← child process, JSONL over stdio
```

No Node.js bridge runner needed — pi RPC mode is designed for process integration.

## 2. Pi vs Existing Providers

| Dimension | Codex app-server | Kimi ACP | Claude Agent SDK | **Pi RPC** |
|-----------|-----------------|----------|-----------------|------------|
| Transport | JSON-RPC over stdio | JSON-RPC over stdio | JSONL via Node runner | **JSONL over stdio** |
| Stdio framing | `\n` lines | `\n` lines | `\n` lines | `\n` only (strict JSONL) |
| Session identity | `thread.id` (server-assigned) | `sessionId` (ACP) | `sessionId` (SDK) | **session JSONL file path** |
| Session discovery | in `thread/start` response | in `session/new` response | `session_bound` event | **`get_state` → `sessionFile`** |
| Resume | `thread/resume` RPC | `session/load` RPC | `resumeSessionId` param | **`--session <path>` CLI flag** |
| Turn start | `turn/start` | `session/prompt` | `deliver` command | **`prompt` RPC command** |
| Turn boundary event | app-server events | `stopReason` | `turn_complete` | **`agent_settled`** |
| Cancel | `turn/interrupt` | `session/cancel` | `interrupt` command | **`abort` RPC command** |
| Close | kill process group | kill process group | `close` command | **kill process group** |
| Steer mid-turn | `turn/steer` | n/a (no mid-turn steer) | n/a | **`steer` RPC command** |
| Multi-turn in one prompt | yes | yes | yes | **yes (auto tool-call loop)** |
| Built-in tools | codex tools | kimi tools | claude tools | **read/write/edit/bash/grep/find/ls** |

## 3. Pi RPC Protocol (Key Points)

### 3.1 Startup

```bash
pi --mode rpc \
   --provider <provider> \
   --model <provider/model> \
   --session-dir <harness-managed-dir> \
   --thinking <off|low|medium|high> \
   --no-context-files \
   --no-extensions
```

**Key flags:**
- `--no-context-files` (**required**): Pi auto-loads `AGENTS.md`/`CLAUDE.md` from
  cwd and parent dirs. In a Harness worktree, the project's own `AGENTS.md` would
  conflict with Harness's member instructions. We must disable context file
  loading and include all instructions in the prompt text.
- `--no-extensions` (**required for v1**): Project extensions expecting TUI mode
  will crash or misbehave in RPC mode. We disable extension discovery and
  rely on pi's built-in tools (`read/write/edit/bash/grep/find/ls`). A future
  version may selectively enable MCP-bearing extensions.
- `--thinking` maps from the member's `reasoning_effort` requested level:
  `off→off, low→low, medium→medium, high→high`.
- `--session-dir` points to a Harness-managed directory so pi persists sessions
  there (never in `~/.pi/agent/sessions/`).
- No `--no-session`: we WANT pi to persist sessions for resume support.

Pi starts and waits for stdin commands. It emits **NO initial event** on stdout.

### 3.2 Session Discovery

Send `get_state` immediately after spawn (HANDSHAKE_TIMEOUT = 15s):

```json
→ {"id": "req-1", "type": "get_state"}
← {"id": "req-1", "type": "response", "command": "get_state", "success": true,
   "data": {
     "sessionFile": "/abs/path/to/session.jsonl",
     "sessionId": "abc123",
     "model": {...},
     "thinkingLevel": "off",
     "isStreaming": false,
     "isCompacting": false,
     "autoCompactionEnabled": true,
     "steeringMode": "all",
     "followUpMode": "one-at-a-time",
     "messageCount": 0,
     "pendingMessageCount": 0
   }}
```

**Critical fields:**
- `sessionFile` — absolute path to the JSONL session file. Store as
  `native_session_id` in `MemberRun.native_session.native_session_id`.
- `autoCompactionEnabled` — if `true`, immediately send
  `{"type": "set_auto_compaction", "enabled": false}` to prevent pi from
  triggering unexpected compaction during long prompts.
- `isStreaming` — can be used to double-check agent state before sending
  a new prompt (safety check: assert `false`).

If `get_state` times out or fails, the pi process is dead/misconfigured —
return an error with stderr tail.

### 3.3 Sending Work (Turn)

```json
→ {"id": "req-2", "type": "prompt", "message": "<full work contract text>"}
← {"id": "req-2", "type": "response", "command": "prompt", "success": true}
  ... streaming events ...
← {"type": "agent_settled"}
```

The `prompt` response (`success: true`) means the prompt was accepted/queued.
Streaming events follow asynchronously.

### 3.4 Event Stream During a Prompt

```
agent_start                     ← agent begins processing
  turn_start                    ← first LLM turn
    message_start
      message_update            ← may contain:
        text_start / text_delta / text_end
        thinking_start / thinking_delta / thinking_end
        toolcall_start / toolcall_delta / toolcall_end
    message_end
    tool_execution_start        ← for each tool call
    tool_execution_update        ← streaming output (primarily bash; may fire for other tools)
    tool_execution_end
  turn_end                      ← FIRST turn done (carries: message, toolResults)
  turn_start                    ← second turn (if tools were called → LLM continues)
    ...
  turn_end                      ← LAST turn done
agent_end                       ← agent run complete (may still retry/compact)
agent_settled                   ← FULLY DONE: no auto-retry/compaction/queued follow-up
```

**Key terminal signal:** `agent_settled` means the agent is truly idle.
Between `agent_end` and `agent_settled`, pi might:
- Auto-compact and retry the prompt
- Process queued steering/follow-up messages
Only after `agent_settled` is it safe to send the next `prompt`.

**Extracting final text:** The last `turn_end` before `agent_settled` carries
`message` (a full `AssistantMessage`) and `toolResults`. Extract text from
`message.content` blocks where `type == "text"`. This is more reliable than
accumulating `text_delta` events, which may interleave with tool calls.

### 3.5 Abort / Interrupt

```json
→ {"id": "req-3", "type": "abort"}
← {"id": "req-3", "type": "response", "command": "abort", "success": true}
```

After abort, pi completes the current in-flight operation and emits
`agent_settled`. The turn text up to the abort point is preserved.

### 3.6 Steer (Mid-Turn Message)

```json
→ {"type": "steer", "message": "New instruction to inject mid-turn"}
← {"type": "response", "command": "steer", "success": true}
```

Steer messages are delivered after the current assistant turn finishes its
tool calls, before the next LLM call. This is how we inject Host/peer messages
while the member is working.

### 3.7 Resume (Crash Recovery)

```bash
pi --mode rpc --session <path-to-previous-session.jsonl> [other flags]
```

Pi loads the session file and continues. The `sessionFile` from `get_state`
will return the same path. Harness stores this path in
`native_session.native_session_id`.

### 3.8 Model / Provider Selection

Can be set at startup (CLI flags) or via RPC commands:

```json
→ {"type": "set_model", "provider": "anthropic", "modelId": "claude-sonnet-4-20250514"}
```

Or at startup:
```bash
pi --mode rpc --model anthropic/claude-sonnet-4-20250514
```

### 3.9 Important: No `set_system_prompt` RPC Command

Pi RPC has no `set_system_prompt` command. Pi's system prompt is baked in at
startup (default + AGENTS.md + APPEND_SYSTEM.md). All member-specific
instructions must be included in the `prompt` message text.

## 4. Pi Binary Resolution

Following the Kimi pattern (`resolve_kimi_bin`):

```rust
fn resolve_pi_bin() -> String {
    // 1. PI_BIN env override
    if let Ok(explicit) = std::env::var("PI_BIN") {
        if !explicit.trim().is_empty() {
            return explicit;
        }
    }
    // 2. pi on PATH (which pi)
    // 3. npm global: ~/.nvm/versions/node/*/bin/pi
    // 4. fallback: "pi" (let Command error on spawn failure)
    "pi".into()
}
```

## 5. Pi RPC Client Module (`pi_rpc.rs`)

### 5.1 Structure

```rust
pub(crate) struct PiRpcClient {
    child: Child,
    stdin: ChildStdin,
    next_request_id: u64,
    /// Response waiters: request id → oneshot sender
    pending: Arc<Mutex<HashMap<String, Sender<serde_json::Value>>>>,
    /// Streaming events from reader thread (notifications + non-response frames)
    events: Receiver<serde_json::Value>,
    reader: Option<JoinHandle<()>>,
    stderr_tail: Arc<Mutex<String>>,
    /// Absolute path to pi's session JSONL file
    session_file: Option<PathBuf>,
}

pub(crate) struct PiSpawnOptions<'a> {
    pub cwd: &'a Path,
    pub model: Option<&'a str>,
    pub thinking: Option<&'a str>,
    pub provider: Option<&'a str>,
    pub resume_session_file: Option<&'a Path>,
    pub session_dir: &'a Path,
    pub collaboration_env: &'a [(String, String)],
}

pub(crate) struct PiTurnOutcome {
    pub final_text: String,
    pub interrupted: bool,
    pub close_requested_by_harness: bool,
}
```

### 5.2 Key Methods

#### `spawn(options) -> CliResult<Self>`
1. Resolve pi binary via `resolve_pi_bin()`
2. Build `Command`:
   ```bash
   pi --mode rpc \
      --session-dir <options.session_dir> \
      [--model <options.model>] \
      [--provider <options.provider>] \
      [--thinking <options.thinking>] \
      [--session <options.resume_session_file>] \
      --no-context-files \
      --no-extensions
   ```
3. Set cwd, collaboration env vars
4. `isolate_provider_child_process_group(&mut cmd)`
5. Spawn child, wrap stdin/stdout/stderr
6. Start reader thread (JSONL lines → events channel or pending response dispatch)
7. Start stderr drain thread
8. Send `{"id":"pi-init","type":"get_state"}` with HANDSHAKE_TIMEOUT:
   - Extract `session_file` from response
   - If `autoCompactionEnabled == true`, send
     `{"type":"set_auto_compaction","enabled":false}`
   - Log model, thinkingLevel for observability
9. Return client with session_file populated

#### `prompt(text, idle_timeout, on_update, should_cancel) -> CliResult<PiTurnOutcome>`
1. Send `{"type": "prompt", "message": text}`, wait for `success: true` response
2. Loop reading events from the events channel:
   - `turn_end` → extract text from `message.content` blocks (type=="text").
     Store in `last_turn_text` (replaces, not appends — only the LAST turn's
     text matters for the final report).
   - `tool_execution_start` → call `on_update` for live activity projection
     (toolName, args → preview string).
   - `tool_execution_end` → call `on_update` for completion.
   - Check `should_cancel()` → if true, send `{"type": "abort"}`, set
     `interrupted = true`, continue reading until `agent_settled`.
   - `agent_settled` → break loop.
   - `agent_end` → reset idle timer (this event proves the child is alive).
   - On idle timeout (no event for `idle_timeout`) → send `abort`, then
     kill process tree, return error.
   - On events channel disconnect → transport dead, return error.
3. Return `PiTurnOutcome { final_text: last_turn_text, interrupted, close_requested }`

Note: `message_update` events with `text_delta` are NOT used for text
accumulation. They are useful for live streaming to a UI but error-prone
for final text extraction (tool calls interleave with text blocks). Use
`turn_end.message.content` instead.

#### `abort() -> CliResult<()>`
Send `{"type": "abort"}` and wait for response.

#### `ensure_transport_alive() -> CliResult<()>`
Check if reader thread has ended or child has exited.

#### `shutdown(self)` / `Drop`
Kill process group, join reader thread.

### 5.3 Reader Thread

Same pattern as `codex_app_server.rs` and `kimi_acp.rs`:
- Read lines from child stdout (Rust `BufRead::lines()` — compatible with pi's
  strict `\n`-only framing).
- Parse JSON.
- If frame has `"type": "response"` → dispatch to pending waiter by `id` field.
- Otherwise → event/notification → send to events channel.
- On stdout EOF → drop events sender, clear pending map.

**Extension UI requests:** If pi has extensions enabled (we disable them via
`--no-extensions`), extension UI methods like `select`/`confirm` emit
`{"type": "extension_ui_request", "id": ..., "method": "select"}` on stdout.
These are NOT responses — they're requests FROM pi TO the client. Since we
disable extensions, we should never see these. If one appears, log and ignore
(do not block the events channel).

## 6. Team Member Orchestration (`run_pi_team_member`)

Follows the established `run_codex_member` / `run_kimi_member` / 
`run_claude_agent_sdk_team_member` pattern.

### 6.1 Main Loop

```
fn run_pi_team_member(ledger, objective, member, context) -> MemberOutcome {
    // 1. Validate supervisor lease
    // 2. Build collaboration envelope / environment
    // 3. Fence lease before spawning provider process
    // 4. PiRpcClient::spawn(...)
    //    - model from member.model
    //    - resume from member.native_session.native_session_id
    //    - session_dir from store root + /pi_sessions/<member_id>/
    // 5. Extract session_file → write to member_row.native_session
    // 6. Register live_control handle
    // 7. Save member_row (status = Idle)
    // 8. Enter main loop:
    //    a. wait_for_idle_member_wake() — wait for Work / Messages / Close
    //    b. Build prompt_text:
    //       - If Work: work_contract_prompt(objective, member, work, envelope)
    //       - If Messages: message list formatted as TEAM MESSAGES
    //    c. pi_client.prompt(prompt_text, idle_timeout, on_update, should_cancel)
    //       - on_update: project tool events to live_sink
    //       - should_cancel: check live_control for interrupt/close
    //    d. On agent_settled:
    //       - Parse turn_text → ## RESULT / ## SUMMARY
    //       - Record Action
    //       - Save member_row status
    //       - Loop back to step a
    // 9. On Close or error → return MemberOutcome
}
```

### 6.2 Prompt Format (Work Contract)

Same format as other providers:

```
## PI TEAM MEMBER INSTRUCTIONS

You are {member.name}, role: {member.role}.
Your owned paths: {owned_paths}

## WORK CONTRACT

**Work ID**: {work.id}
**Title**: {work.title}
**Expected outputs**: {work.description}

## OBJECTIVE
{objective}

## REPORT FORMAT
When done, report with:
## RESULT
[DONE | BLOCKED | NEEDS_REVIEW]

## SUMMARY
[Concise summary of what you did and why]

## BLOCKERS (if any)
...

## TASK
{work.contract}
```

### 6.3 Live Activity Projection

Map pi events to activity previews (same pattern as Codex's `project_codex_team_event_live`):

| Pi Event | Live Preview |
|----------|-------------|
| `tool_execution_start` (toolName="bash", args.command) | "Bash: `<command>`" |
| `tool_execution_start` (toolName="edit", args.path) | "Edit: `<file>`" |
| `tool_execution_start` (toolName="write", args.path) | "Write: `<file>`" |
| `tool_execution_start` (toolName="read", args.path) | "Read: `<file>`" |
| `tool_execution_start` (toolName="grep") | "Grep" |
| `tool_execution_start` (toolName="find") | "Find" |
| `tool_execution_start` (toolName="ls") | "Ls" |
| `tool_execution_end` | (clear or update to completed) |

Note: `message_update.toolcall_start` fires BEFORE the tool actually executes
(when the LLM decides to call it). `tool_execution_start` fires when the tool
BEGINS executing. Use `tool_execution_start` for live projection, matching the
Codex pattern where `item.started` drives the preview.

### 6.4 Interrupt Flow

1. Operator/Lead sends `MemberControlCommand::Interrupt`
2. `should_cancel` closure returns `true`
3. `pi_client.abort()` is called
4. Pi emits `agent_settled` with whatever text accumulated
5. Member returns to Idle, turn is recorded as `interrupted`

### 6.5 Close Flow

1. Host sends `MemberControlCommand::Close`
2. `should_cancel` returns `true` with `close_requested = true`
3. Pi is aborted, then process group killed
4. `member_row.coordination_status = Closed`, `status = Stopped`

### 6.6 Resume (Crash Recovery)

When the orchestration loop detects transport failure and `member_row.native_session`
is `Some`:
1. Record `member_disconnected` event
2. Wait 250ms
3. Re-enter loop → `PiRpcClient::spawn()` with `resume_session_file` set
   to the stored `native_session_id` (session file path)
4. Pi loads session, picks up where it left off
5. Continue with remaining Work

## 7. ProviderAdapter (One-Shot Delivery)

For the `provider_adapter` trait (used by Dynamic Workflow), pi uses print mode:

```bash
pi -p "<prompt>" --provider <p> --model <m> --thinking off --no-session
```

Pi print mode outputs the assistant's final text to stdout, then exits.
Exit code 0 = success.

```rust
struct PiAdapter;

impl ProviderAdapter for PiAdapter {
    fn name(&self) -> &'static str { "pi" }
    
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            streaming: true,
            resume: true,
            mid_turn_approval: false,
            subagents: false,       // pi has no built-in subagents
            mcp: false,             // pi has no built-in MCP (can be added via extensions)
            hooks: false,
            schema: false,
            cost: false,
            enforces_read_only: false,
        }
    }
    
    fn live_ndjson_file_name(&self) -> &'static str {
        "pi.stream-json.ndjson"
    }
    
    fn map_permission(&self, perm: LaunchPermission) -> &'static str {
        match perm {
            // pi print mode: --tools limits which tools are available
            LaunchPermission::ReadOnly => "--tools read,grep,find,ls",
            LaunchPermission::WorkspaceWrite => "",
            LaunchPermission::FullAccess => "",
        }
    }
    
    fn start_runtime(&self, store: &HarnessStore, member: &AgentMember) -> CliResult<AgentRuntime> {
        start_pi_runtime(store, member)
    }
    
    fn run_delivery(&self, ...) -> CliResult<DeliveryOutcome> {
        run_pi_delivery(store, member, runtime, message, delivery_id, timeout_ms, project)
    }
    
    fn spawn_ephemeral(&self, ctx: &EphemeralSpawnContext) -> CliResult<EphemeralSpawn> {
        spawn_pi_ephemeral(ctx.session_dir, ctx.session_id, ...)
    }
}
```

## 8. ProviderIntegrationProfile

```rust
if provider == "pi" && matches!(requested_mode, Some("pi_rpc") | None) {
    return ProviderIntegrationProfile {
        provider: "pi".to_string(),
        execution_mode: "pi_rpc".to_string(),
        execution_driver: MemberExecutionDriver::HostDriven,
        provider_version: None,
        adapter_contract_version: Some("pi-rpc-v1".to_string()),
        reviewed_provider_versions: vec!["0.83.0".to_string()],
        compatibility_status: ProviderCompatibilityStatus::Unknown,
        adapter_reviewed_at: Some("2026-08-11".to_string()),
        compatibility_note: Some(
            "Pi RPC-mode persistent Agent Team member. Session is a JSONL file; \
             resume via --session <path>. Built-in tools: read, write, edit, \
             bash, grep, find, ls. Agent_settled is the turn-completion signal."
                .to_string(),
        ),
        interaction_mode: ProviderInteractionMode::PauseAndResume,
        ordinary_message_boundary: OrdinaryMessageBoundary::InTurn,
        plan_mode: ProviderFeatureMode::Emulated,
        goal_mode: ProviderFeatureMode::Emulated,
        tool_event_fidelity: ProviderEventFidelity::Structured,
        artifact_event_fidelity: ProviderEventFidelity::Structured,
        supports_cancel: true,       // abort RPC command
        supports_resume: true,       // --session <path> CLI flag
        observes_native_subagents: false,
        observes_background_tasks: false,
        thinking_transient_only: true,  // thinking is in message_update, not persisted separately
    };
}
```

## 9. Changes to `provider_registry`

```rust
fn provider_registry() -> &'static [&'static dyn ProviderAdapter] {
    &[&CodexAdapter, &ClaudeAdapter, &KimiAdapter, &PiAdapter]
}
```

## 10. Changes to `run_member_orchestration`

Add a `pi` branch in the provider dispatch:

```rust
} else if current.provider.eq_ignore_ascii_case("pi")
    && matches!(execution_mode, Some("pi_rpc") | None)
{
    run_pi_team_member(ledger, objective, &current, &context)
}
```

## 11. Session Directory Layout

```
<harness_store_root>/
  pi_sessions/
    <member_run_id>/
      <timestamp>.jsonl      ← pi auto-creates this
```

On resume: pass `--session <abs-path-to-jsonl>` to pi.

## 12. Files Changed

| File | Change |
|------|--------|
| `crates/harness-cli/src/pi_rpc.rs` | **NEW** — Pi RPC client |
| `crates/harness-cli/src/main.rs` | Add `mod pi_rpc`; add `PiAdapter`; add `run_pi_team_member`; add pi branch in `run_member_orchestration`; add pi profile in `team_member_provider_profile_for_mode`; add `resolve_pi_bin`; add `start_pi_runtime`; add `run_pi_delivery`; add `spawn_pi_ephemeral`; register in `provider_registry` |
| `crates/harness-cli/tests/fake_provider/` | Pi fake shim for deterministic tests |
| `crates/harness-cli/tests/pi_team_member.rs` | **NEW** — Deterministic tests |

## 13. Design Decisions Resolved

1. **`--no-context-files` is REQUIRED.** Pi loads AGENTS.md/CLAUDE.md from cwd
   and parent dirs. In a Harness worktree, the project's own AGENTS.md would
   conflict with Harness's member instructions. All instructions must be in
   the prompt text. (Resolved: §3.1)

2. **`--no-extensions` is REQUIRED for v1.** Project extensions expecting TUI
   mode will crash in RPC mode. Future versions may selectively enable
   MCP-bearing extensions. (Resolved: §3.1)

3. **Disable auto-compaction at startup.** `get_state` returns
   `autoCompactionEnabled`. If `true`, send `set_auto_compaction` to disable.
   Compaction during a prompt would add unpredictable latency and may lose
   context that the Host expects to remain. (Resolved: §3.2)

4. **Extract final text from the last `turn_end.message`, not `text_delta`
   accumulation.** `turn_end` carries the full `AssistantMessage` object with
   all `content` blocks. Extract `type: "text"` blocks from the last
   `turn_end` before `agent_settled`. This avoids ordering bugs from
   interleaved tool calls and thinking blocks. (Resolved: §3.4)

5. **`reasoning_effort` → `--thinking` mapping.** The member's requested
   `reasoning_effort` maps directly: `off→off, low→low, medium→medium,
   high→high, xhigh→high, max→high`. Pi supports up to `high` in the
   `--thinking` flag. (Resolved: §3.1)

6. **Rust `BufRead::lines()` IS compatible.** Pi uses `\n` as the sole record
   delimiter. Rust's `BufRead::lines()` splits on `\n` only, unlike Node's
   `readline` which also splits on U+2028/U+2029. No special handling needed.
   (Resolved: confirmed)

## 14. Open Questions / Risks

1. **Pi session format stability across versions.** Pi's session JSONL format
   may change. We must pin `reviewed_provider_versions` and re-verify on each
   pi upgrade (same as Kimi's version-gated capabilities).

2. **Pi `--session` in RPC mode.** The `--session <path|id>` CLI flag is
   listed as a general option but not explicitly in the RPC mode docs. We
   must verify it works in RPC mode for resume. If not, we can pass
   `--session-dir <parent-dir>` and let pi auto-discover the session.

3. **`tool_execution_update` semantics.** The docs say it fires "e.g., bash
   output as it arrives" but don't specify which tools emit it. We handle it
   generically for all tools, but should not rely on it for non-bash tools.

4. **Mid-turn steer for Agent Team.** Pi's `steer` RPC command could deliver
   Host/peer messages during a turn. But pi's steering modes
   (`one-at-a-time` vs `all`) and its interaction with `agent_settled` need
   real-provider validation before claiming `interaction_mode: PauseAndResume`
   with steer support.

5. **pi version detection.** Unlike Kimi (`kimi --version`) and Codex
   (`codex --version`), we need to verify `pi --version` output format for
   `apply_provider_version`. Current version is `0.83.0` (plain semver).

6. **Session file existence on resume.** If a previous session file is deleted
   or corrupted, passing `--session <path>` may cause pi to error. We should
   check file existence before resume and fall back to a fresh session.

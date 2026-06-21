# Kimi (Moonshot) integration

本文档定义 Multi-Agent Harness 如何集成 Kimi Code（Moonshot）。重点是把
Kimi 变成 harness 里的第三个 registry-routed provider：可以创建、投递消息、
观察状态、回收运行时，并把 Kimi Code 的 flat stream-json 输出转成 harness 的
`AgentEvent`、`ProviderSession`、`Evidence`、`Message` 和 `Decision`。

Provider-neutral runtime contracts live in [../agent-runtime.md](../agent-runtime.md).
This file should explain only how Kimi implements those contracts. Shared object
semantics such as `Task`, `Message`, `Evidence`, `Proposal`, and `Decision` must
not be redefined here.

## 核心结论

V1 主方案是：

```text
AgentMember(provider=kimi)
  -> AgentRuntime(kimi CLI, request-response shape, one spawned per delivery)
  -> provider session
  -> Message delivery through kimi CLI with injected harness context
  -> kimi-native flat stream parsing
  -> harness store and Agent Dashboard
```

也就是说：

- `kimi` CLI 是按需 provider 执行形式（非持久 app-server）；
- Kimi 已作为第三个 provider 进入 `provider_registry()`，顺序为 Codex、Claude、Kimi
  (`crates/harness-cli/src/main.rs:14905-14915`)；
- 每次消息投递会通过 harness 的消息上下文生成 Kimi prompt；
- Kimi 的真实 headless CLI surface 是 `kimi -p <prompt> --output-format stream-json`
  加可选 `--model` 和 `-S/--session`，已按 v0.18 行为写入代码注释
  (`crates/harness-cli/src/main.rs:14432-14437`,
  `crates/harness-cli/src/main.rs:14562-14567`)；
- `kimi -p` 不能携带 permission flags；`--plan`、`--auto`、`--yolo` 都会被拒绝，
  所以 delivery 路径不传权限 flag (`crates/harness-cli/src/main.rs:14607-14612`)；
- Kimi stream-json 是 flat NDJSON，不是 Claude-shaped：
  `{"role":"assistant","content":...}` 加
  `{"role":"meta","type":"session.resume_hint",...}`
  (`crates/harness-cli/src/main.rs:14349-14357`)；
- schema、cost、resume 等能力在 core capability preset 中按 degraded/unknown 处理，
  不是正向支持声明 (`crates/harness-core/src/lib.rs:4929-4950`)。

## 为什么 Kimi CLI 是主方案

Kimi Code 官方 CLI 提供 `kimi` 命令行工具。相比直接走 Moonshot HTTP API：

- **local-process shape**：`kimi -p` 与 Claude/Codex 的按需 CLI delivery 模型一致，
  harness 可以复用 ProviderAdapter、ProviderSession、NDJSON tee、event ingest 等基础设施；
- **registry-routed provider**：Kimi 通过 `ProviderAdapter` trait 实现接入，provider
  lookup 走 `provider_adapter(name)`，不是新增散落 match arms
  (`ProviderAdapter` trait `crates/harness-cli/src/main.rs:13397`;
  `provider_registry()` + `provider_adapter()` `crates/harness-cli/src/main.rs:14906-14916`)；
- **real CLI surface is small**：headless 模式只依赖 `-p`、`--output-format stream-json`、
  可选 `--model`、可选 `--session`，避免伪造 Claude-only flags；
- **honest degradation**：Kimi v0.18 的 `-p` stream 不返回 Claude `result`、`usage`、
  `model` frame，因此 schema/cost/model usage 走 harness fallback，而不是声称原生支持。

## Provider Runtime 模型

V1 使用 on-demand-spawned `kimi` CLI per AgentMember delivery，在一个 runtime 标识下持续
记录 sessions。

runtime 字段最小集合：

```text
AgentRuntime
  id
  agent_member_id
  provider = kimi
  status = Running (或 Suspended/Closed)
  pid = None (kimi 按需启动，无持久进程 pid)
  control_endpoint = "kimi-runtime://{dir}" (指向运行时目录)
  command = "kimi"
  args = [] (每次 delivery 动态注入参数)
  started_at / ended_at
  last_event_at
```

`start_kimi_runtime` 创建 runtime 目录，探测 `resolve_kimi_bin()` 得到的二进制是否存在，
并记录 `pid: None`、`command: "kimi"`、`control_endpoint: kimi-runtime://...`
(`crates/harness-cli/src/main.rs:14520-14559`)。

健康检查分三层：

```text
endpoint: runtime directory exists + kimi binary probe
session: most recent ProviderSession exists + has terminal status
delivery: latest message delivery has proof of receipt from Kimi
```

Process 层不适用（Kimi 按需启动，无持久进程）。

## Install and login

Operator prerequisite:

```bash
# Install Kimi Code using Moonshot's current installer/package instructions.
# The harness expects a `kimi` executable.
kimi login
kimi -p "ping" --output-format stream-json
```

Binary resolution order is implemented by `resolve_kimi_bin()`:

1. `KIMI_CODE_BIN` env override, if non-empty;
2. bare `kimi` on `PATH`;
3. default install path `~/.kimi-code/bin/kimi`;
4. bare `kimi` as the final fallback, so spawn failure is explicit.

Source: `crates/harness-cli/src/main.rs:14317-14345`.

## Message Delivery

每次投递消息时，harness 构造一个包含：

- 当前任务上下文（goal/task/evidence/decision）；
- 消息队列（inbox 消息）；
- harness developer instructions（角色、权限、安全）。

然后调用 Kimi CLI：

```bash
kimi -p "{structured_prompt}" --output-format stream-json
```

可选参数：

```bash
kimi -p "{structured_prompt}" --output-format stream-json --model <model>
kimi -p "{structured_prompt}" --output-format stream-json --session <session_id>
```

`run_kimi_exec_delivery_real` 会把 developer instructions 折叠进 prompt，因为 Kimi 没有
Claude 的 `--append-system-prompt`；resume 使用 `--session <id>`；model 使用 `--model <model>`
(`crates/harness-cli/src/main.rs:14587-14606`)。

Kimi delivery 明确不传这些 Claude-only 或非真实 headless flags：

```text
--verbose
--permission-mode
--allowedTools
--json-schema
--mcp-config
--add-dir
--effort
```

Source: `crates/harness-cli/src/main.rs:14562-14567`,
`crates/harness-cli/src/main.rs:14607-14612`.

Kimi 执行后返回：

- raw flat NDJSON frames；
- `ProviderSession` 记录，provider 为 `kimi`；
- 可选 resumable session id（来自 `session.resume_hint`）；
- `Evidence`（source_type = `kimi_delivery_session`）；
- `DeliveryOutcome.summary`（来自 assistant content）；
- no native usage/model/cost/structured frame in `-p` mode，走 degraded fallback
  (`crates/harness-cli/src/main.rs:14658-14763`)。

## Event Sources

Kimi 产生的事件通过以下源进来：

1. **Kimi stdout flat NDJSON** — 直接解析 `kimi -p --output-format stream-json` 输出：
   - assistant reply frame: `{"role":"assistant","content":"..."}`
   - resume hint frame:
     `{"role":"meta","type":"session.resume_hint","session_id":"...","command":"kimi -r ..."}`
   - no Claude `system.init`
   - no Claude terminal `result`
   - no `usage` / `model` frame in `-p` mode

2. **ProviderSession record** — harness-store 记录的会话：
   - provider = `kimi`
   - command = `kimi`
   - args = `["-p", "--output-format", "stream-json", ...]`
   - provider_thread_id = parsed resumable session id when present
   - jsonl_ref = `kimi.stream-json.ndjson`

3. **Evidence ingest** — Kimi delivery session output:
   - source_type = `kimi_delivery_session`
   - source_ref = `provider-session:{resolved_session_id}`
   - summary = Kimi stream-json delivery summary

Source: `crates/harness-cli/src/main.rs:14687-14733`.

## Reducer Mapping

Kimi 事件 -> harness objects：

```text
(provider = "kimi")
  role=="assistant"              -> AgentEvent { event_type: "assistant_message" }
  type=="session.resume_hint"    -> AgentEvent { event_type: "session_resume_hint" }
  other role                     -> AgentEvent { event_type: role }
  unknown frame                  -> AgentEvent { event_type: "unknown" }
  parsed session.resume_hint     -> ProviderSession id/session status
```

Kimi uses kimi-native parsing:

- `parse_kimi_frames` parses one JSON frame per non-empty NDJSON line;
- `extract_kimi_reply_text` concatenates every assistant frame's content;
- `extract_kimi_session_id` reads `session_id` from `type=="session.resume_hint"`;
- `infer_kimi_status` treats clean exit with frames as success, clean empty output as stale,
  and non-zero exit as failed.

Source: `crates/harness-cli/src/main.rs:14360-14430`.

The durable ingest path is `ingest_kimi_stream_json`, which explicitly says it mirrors Claude ingest
but operates on flat kimi-native frames and stamps rows with provider `kimi`
(`crates/harness-cli/src/main.rs:15177-15245`).

Queue discipline（来自 harness，不由 provider 定义）：

- 投递前：消息锁定在 `delivery_status = queued`
- 投递中：更新为 `delivery_status = in_progress`
- 投递后：若成功则 `delivery_status = delivered`；若失败重试或 `failed`
- claim/lease 原子性由 harness-store 负责，provider adapter 只执行 delivery

## Permission Model

Kimi interactive CLI exposes standalone permission flags:

```text
--plan
--auto
-y / --yolo
```

The adapter keeps a `map_permission` implementation for trait conformance and possible future
interactive/ACP invocation:

```text
ReadOnly        -> --plan
WorkspaceWrite  -> --auto
FullAccess      -> --yolo
```

Source: `crates/harness-cli/src/main.rs:14779-14790`.

But the real `kimi -p` headless delivery path does **not** use them. Kimi v0.18 rejects permission
flags combined with `--prompt` / `-p`, so `spawn_kimi_ephemeral` and `run_kimi_exec_delivery_real`
pass no permission flag. This means kimi has **no read-only mode at all**: a leaf the workflow
declares read-only can still edit the live tree (observed in dogfooding — a read-only kimi leaf
edited two checked-in docs).

Writable vs read-only boundaries are therefore enforced **structurally by the harness**, not by a
Kimi CLI flag. Kimi declares `enforces_read_only = false` in `ProviderCapabilities::kimi_exec()`
(unlike codex `--sandbox read-only` and claude's `Read,Grep,Glob` tool allowlist), and the workflow
leaf runner reads that capability: a read-only leaf whose provider can't enforce read-only is run in
a throwaway git worktree anyway, so any writes land in a discardable checkout instead of the live
repo (`provider_enforces_read_only` / `step_needs_isolation`,
`crates/harness-cli/src/main.rs`). On a non-git project there is no worktree to isolate into, so the
leaf degrades to the shared cwd with a printed warning that its writes are not contained.

Source: `crates/harness-cli/src/main.rs:14471-14478`,
`crates/harness-cli/src/main.rs:14607-14612`.

Provider config remains provider-neutral:

```json
{
  "provider": "kimi",
  "provider_config": {
    "approval_policy": "none" | "prompt_required",
    "workspace_policy": "workspaceWrite" | "readOnly",
    "service_tier": "free" | "pro" | "team"
  }
}
```

## Workspace Model

Kimi and Claude both use an isolated runtime/session directory:

```text
{harness_root}/runtimes/{member_id}/
  state.json

{harness_root}/provider-sessions/{delivery_id}/
  kimi.stream-json.ndjson
  kimi.stderr                 # only when stderr is non-empty
```

`run_kimi_delivery` writes `kimi.stream-json.ndjson`, records the path in
`ProviderSession.jsonl_ref`, and writes stderr to `kimi.stderr` when present
(`crates/harness-cli/src/main.rs:14667-14733`,
`crates/harness-cli/src/main.rs:14740-14751`).

## Native Multi-Agent Features

Kimi Code v0.18 is treated as a single provider member execution surface.

Doctrine:

> Child threads stay under the parent member, not promoted to members.

For Kimi V1:

- no native Kimi subagent lifecycle is claimed;
- no provider child thread creation is claimed;
- `session.resume_hint` is a resumable session hint, not a child agent;
- any multi-agent behavior must still flow through harness `AgentMember`, `Task`,
  `Message`, `Evidence`, and `Decision` records.

This matches `ProviderCapabilities::kimi_exec()`, where `subagents`, `mcp`, `hooks`,
`schema`, `cost`, and `resume` are all false/degraded until proven
(`crates/harness-core/src/lib.rs:4929-4950`).

## Evidence and Report Extraction

Kimi output contains flat assistant text plus optional meta frames. Harness converts them as:

```text
Kimi flat NDJSON
  ├─ role=="assistant" content    -> delivery summary + AgentEvent
  ├─ session.resume_hint          -> ProviderSession resume token / ProviderMeta
  ├─ raw NDJSON file              -> Evidence jsonl_ref
  └─ stderr                       -> transcript_ref / stderr_ref when non-empty
```

Evidence lifecycle:

1. Raw evidence: capture `kimi.stream-json.ndjson` as-is.
2. Indexed evidence: append `Evidence { source_type: "kimi_delivery_session" }`.
3. Optional graduation: future structured extraction may promote reply content to Proposal or Decision.
4. Schema extraction: degraded to caller text-extract fallback because `kimi_exec().schema = false`.

`spawn_kimi_ephemeral` sets `tokens`, `model`, `structured`, and `cost_usd` to `None`
because Kimi `-p` stream-json carries no usage/model/cost frame
(`crates/harness-cli/src/main.rs:14497-14516`).

## Dashboard Health Signals

Dashboard reads `runtime_health` / session records computed by harness-cli:

```json
{
  "endpoint": {
    "status": "pass" | "fail" | "unknown",
    "message": "kimi binary resolved" | "kimi binary unavailable"
  },
  "session": {
    "status": "pass" | "warn" | "fail",
    "message": "last Kimi session succeeded" | "no sessions yet" | "last Kimi session failed"
  },
  "delivery": {
    "status": "pass" | "warn" | "fail",
    "message": "last message delivered" | "delivery pending"
  },
  "checked_at": "2026-06-20T00:00:00Z"
}
```

Codex has process/socket/protocol/delivery. Kimi has no persistent process and no socket protocol,
so the meaningful layers are binary endpoint, session, and delivery.

## Capabilities and Cost

Kimi capability preset:

```text
streaming         true
resume            false (degraded/unknown)
mid_turn_approval false
subagents         false
mcp               false
hooks             false
schema            false (text-extract fallback)
cost              false (token-estimate fallback)
```

Source: `crates/harness-core/src/lib.rs:4929-4950`.

The registry tests assert that Kimi is registered, reports `ProviderCapabilities::kimi_exec()`,
uses `kimi.stream-json.ndjson`, and keeps schema/cost/resume false until proven
(`crates/harness-cli/src/main.rs:17029-17049`).

`provider_price_per_mtok("kimi")` currently returns placeholder estimate `(0.60, 2.50)`.
The source warns this is only a workflow spend bound, not billing truth, and must be confirmed
against Moonshot pricing or a future live usage frame before spend decisions are trusted
(`crates/harness-core/src/lib.rs:1532-1548`).

## Fallback Modes

若 Kimi CLI 不可用或失败：

1. **No fallback to Moonshot HTTP API** — V1 keeps the CLI provider shape. HTTP API fallback would
   need a separate adapter/work package.

2. **Message queueing on delivery failure** — 消息留在 `delivery_status = failed` 或
   `delivery_status = queued`，下次 `agent deliver` 重试。

3. **Health downgrade** — 若 `resolve_kimi_bin()` 找不到 runnable binary，endpoint health
   降级，Dashboard 显示 Kimi unavailable。

4. **Schema fallback** — schema-mode nodes consume the assistant reply through harness text extraction,
   because Kimi `-p` has no `--json-schema` support in the implemented surface.

5. **Cost fallback** — cost uses harness token-estimate and placeholder price bounds because Kimi
   `-p` stream-json has no usage/cost frame.

6. **Resume fallback** — only a parsed `session.resume_hint.session_id` is exposed as resumable.
   Synthetic fallback session ids are not surfaced as resume tokens
   (`crates/harness-cli/src/main.rs:14675-14685`).

7. **Reconciliation hook** — 可通过 `agent reconcile` 手工修复状态（与 Codex / Claude 同）。

## Unsupported or Risky Surfaces

相比 Codex 和 Claude：

1. **No permission flags with `-p`** — Kimi v0.18 rejects permission flags with prompt mode.
   Harness must enforce boundaries through worktree/task ownership.

2. **No native schema frame** — no Kimi `--json-schema` equivalent is passed in V1.

3. **No native cost/usage frame** — `tokens`, `model`, and `cost_usd` are `None` in Kimi delivery.

4. **Resume is degraded** — `session.resume_hint` exists in the flat stream, but core capability still
   marks resume false until the end-to-end resume contract is proven.

5. **No MCP/hooks/subagents claim** — capability preset marks them false.

6. **Stale comments must not override parser truth** — any old comment claiming Claude-shaped Kimi
   output is superseded by the live v0.18 parser tests and kimi-native reducer. The regression test
   proves Claude reply extraction fails on real Kimi frames
   (`crates/harness-cli/src/main.rs:16945-16980`).

## Validation Gates

实现 Kimi 集成的 validation 清单：

- [ ] `kimi` installed and `kimi login` completed for the operator account.
- [ ] `kimi -p "ping" --output-format stream-json` emits flat NDJSON with assistant content.
- [ ] `agent create --provider kimi --start` creates a runtime with provider `kimi`.
- [ ] `agent deliver` spawns `kimi -p --output-format stream-json`.
- [ ] No permission flags are passed on the `-p` path.
- [ ] Optional `--model` is passed when launch spec has a model.
- [ ] Optional `--session` is passed only when a real resume id is available.
- [ ] `kimi.stream-json.ndjson` is written under the provider session directory.
- [ ] Assistant reply extraction works for string content and array block content.
- [ ] `session.resume_hint` is parsed into the provider session thread/resume field.
- [ ] schema/cost/resume remain degraded until a follow-up proves them live.
- [ ] `supported_provider_names()` includes `kimi`.
- [ ] Codex and Claude paths remain regression-clean.

## Sequencing with Other Work Packages

Kimi is the third registry-routed provider after Codex and Claude. The relevant sequencing is:

1. **Provider registry** — Kimi is registered through `provider_registry()` and resolved through
   `provider_adapter(name)`, not hard-coded dispatch (`crates/harness-cli/src/main.rs:14905-14915`).

2. **Kimi-native parser** — flat NDJSON parser and reducer are required because Claude parser cannot
   extract Kimi replies (`crates/harness-cli/src/main.rs:14349-14417`,
   `crates/harness-cli/src/main.rs:16968-16980`).

3. **Delivery implementation** — `run_kimi_delivery` records ProviderSession, Evidence, NDJSON,
   stderr, status, and summary under provider `kimi` (`crates/harness-cli/src/main.rs:14641-14763`).

4. **Capability honesty** — core keeps Kimi degraded except streaming until live behavior proves
   resume/schema/cost/MCP/hooks/subagents (`crates/harness-core/src/lib.rs:4929-4950`).

5. **Future hardening** — once Kimi live usage, schema, resume, or tool control become stable,
   update `ProviderCapabilities::kimi_exec()`, parser tests, integration docs, and dashboard health
   expectations together.

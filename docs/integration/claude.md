# Claude Integration

本文档定义 Star Harness 如何集成 Claude Code。重点是把 Claude 变成
harness 里的持久 `AgentMember` provider：可以创建、投递消息、观察状态、回收
运行时，并将 Claude 原生 session 作为 transcript、tool activity 与 resume
真相；Harness 只保存 binding、协调、显式 outcome 和 artifact/check refs。

Provider-neutral runtime contracts live in [../agent-runtime.md](../agent-runtime.md).
This file should explain only how Claude implements those contracts. Shared
object semantics such as `Task`, `Message`, `Evidence`, `Proposal`, and
`Decision` must not be redefined here.

## 核心结论

V1 主方案是：

```text
AgentMember(provider=claude)
  -> AgentRuntime(claude CLI, request-response shape, one spawned per delivery)
  -> provider session
  -> Message delivery through claude CLI with injected harness context
  -> Claude native session (execution truth and resume)
  -> ephemeral output projection + Harness coordination store
  -> Agent Dashboard joined view
  -> optional Claude Code plugin packaging after contracts stabilize
```

Agent Team 也使用同一原生真相边界：`provider=claude` 默认
`execution_mode=claude_cli`。adapter 以内存方式消费
`claude -p --output-format stream-json --verbose`，从 `system(init)` 绑定真实
session id；显式重试通过 `resume_native_session_id` 调用 `--resume`。工具、
命令、文件活动与对话不写入 `MemberAction`，Member 详情页通过
`GET /v1/member-runs/{id}/native-activity` 读取 Claude 自己的 project JSONL。
thinking 在 reader 层直接丢弃。

也就是说：

- `claude CLI` 是按需 provider 执行形式（非持久 app-server）；opt-in 的
  resident 模式（`HARNESS_CLAUDE_RESIDENT=1`，ADR-0021）可把
  `claude --input-format stream-json` 进程保持常驻、逐 turn 喂 stdin frame；
- 每次消息投递会通过 harness 的消息上下文生成 Claude 输入 prompt；
- Claude 执行输出由 adapter 临时解析；只有显式 handoff、outcome、artifact /
  check ref、PendingInteraction 与控制确认进入 Harness；
- 子代理（native Claude subagents via threads）自动成为 child threads，
  而非升级为新 members；
- fallback 和 CI/review helper 与 Codex 类似但使用 claude API；
- runtime health 可观测但形状不同（无持久 pid，有会话标识）。

## 为什么 Claude CLI 是主方案

Claude Code 官方 CLI 提供了 `claude` 命令行工具和 APIs 给外部产品做集成。
相比于 Claude Agent SDK 或 HTTP API：

- **local-process shape**：`claude` 命令行保持与 Codex 类似的启动模式，便于
  harness 的一致性（ADR-0008: one-process-per-member 虽然 Claude 是按需，
  但记录 session id 和事件同样清晰）；
- **native subagent + thread handling**：Claude 的 native subagents（Task
  tool 子代理）出现在 stream-json 转录帧里，harness
  可以把它们映射到 `ProviderChildThread` 对象而无需额外的多层代理；
- **no persistent server overhead**：避免了专属的 app-server 进程与调度复杂度，
  符合"按需计算"的云原生 philosophy；
- **statement-driven delivery**：每次消息投递是一个独立的 claude 调用，
  输入是任务 + 上下文，输出是可解析的事件流，便于 idempotent 重试。

## Provider Runtime 模型

V1 使用 on-demand-spawned claude CLI per AgentMember delivery，在一个
runtime 标识下持续记录 sessions。

runtime 字段最小集合：

```text
AgentRuntime
  id
  agent_member_id
  provider = claude
  status = Running (或 Suspended/Closed)
  pid = None (claude 按需启动，无持久进程 pid)
  control_endpoint = "claude-runtime://{dir}" (指向运行时目录)
  command = "claude"
  args = [] (每次 delivery 动态注入参数)
  started_at / ended_at
  last_event_at
```

健康检查分三层（不同于 Codex 的四层）：

```text
endpoint: runtime directory exists + last_session within acceptable time
session: NativeSessionRef resolves and native terminal state is readable
delivery: latest message delivery has proof of receipt from Claude
```

Process 层不适用（Claude 按需启动，无持久进程）。

## Message Delivery

每次投递消息时，harness 构造一个包含：
- 当前任务上下文（goal/task/evidence/decision）
- 消息队列（inbox 消息）
- harness 系统 prompt（角色、权限、安全）

然后调用 claude CLI（`run_claude_exec_delivery_real`，
`crates/harness-cli/src/main.rs`）：

```bash
claude -p "{harness_message_envelope}" --output-format stream-json --verbose \
  [--resume {prior_session_id}] [--append-system-prompt {developer_instructions}] \
  [--model {model}] [--json-schema {schema}] \
  --permission-mode {mode} [--allowedTools {t1,t2}] [--mcp-config {path}] \
  [--add-dir {root}]
```

Claude 执行后由 adapter 绑定真实 session id，供原生 `--resume` 使用，并
提供临时 activity projection。只有明确的跨 actor 消息、结果摘要、artifact /
check 引用或治理动作会被提升为 Harness 记录。

## Event Sources

Claude 产生的事件通过以下源进来：

1. **Claude native session / stdout stream-json** — 通过 provider adapter
   读取并在内存中归一化；不复制成 Harness execution ledger
   - `system`（subtype `init`）：新会话打开，携带 `session_id`（resume 用）
   - `assistant` / `user`：消息帧（content blocks：text/tool_use/tool_result）
   - `stream_event`：细粒度增量事件（按 subtype 归约）
   - `result`：终态帧，携带最终 assistant 文本、usage/cost/model、可选
     schema-validated `structured_output`
   - native subagents（Task tool）出现在转录帧里，不是独立事件类型

2. **NativeSessionRef（target）** — Harness 记录的 mode-aware 引用
   - provider = "claude"
   - provider_thread_id：从 `system(init)` 帧解析的真实 session id
     （下一次 delivery 用 `--resume` 延续）
   - availability / provider version / adapter contract / resume capability

3. **Explicit promotion** — Harness 只保存 assignment、handoff、outcome、
   artifact/check refs、PendingInteraction 与控制确认；完整 session 留在 Claude。

## Reducer Mapping

Claude 事件 → harness objects（`ingest_claude_stream_json` reducer）：

```text
(provider = "claude")
  system(init)   → ProviderSession { provider_thread_id = session_id } + AgentEvent { stream_system_init }
  stream_event   → AgentEvent { event_type = subtype }
  result         → AgentEvent { stream_result }；status = Succeeded（无 error）/ Failed
  无 result 帧    → status = Stale（有事件）/ Failed（空输出或进程失败）
  assistant text → DeliveryOutcome.summary（report 内容）+ Evidence
```

Queue discipline（来自 harness，不由 provider 定义）：

- 投递前：消息锁定在 `delivery_status = queued`
- 投递中：claim 行记录为 `delivery_status = acknowledged`（原子 claim/lease）
- 投递后：若成功则 `delivery_status = delivered`；若失败重试或 `failed`
- claim/lease 原子性：harness-store 的 `claim_queued_message_delivery`
  必须在事件入库前原子提交

## Permission Model

Claude 权限映射到 harness `provider_config`：

```json
{
  "provider": "claude",
  "provider_config": {
    "approval_policy": "none" | "prompt_required",
    "workspace_policy": "workspaceWrite" | "readOnly",
    "service_tier": "free" | "pro" | "team"
  }
}
```

权限语义：
- `approval_policy = "prompt_required"`：每条消息需要 Lead 审批后投递
- `workspace_policy = "workspaceWrite"`：Claude 可写入运行时目录（如生成文件、日志）
- `service_tier`：影响 rate limits 和推理模型选择
- 权限落到 CLI 层是 `--permission-mode` + `--allowedTools`：read-only =
  `Read,Grep,Glob`（无 Edit/Write/Bash），即 claude 的
  `enforces_read_only = true` 能力（对比 kimi 无法物理只读）

Codex 的 `sandbox_policy` / `service_tier` 具有相似语义。不在 core
schema 中绑定到 provider 特定值（provider-neutral in data model；
mapping 在 CLI 层）。

## Workspace Model

Claude 和 Codex 都假设一个隔离的工作目录。下列 Harness delivery mirror
是 ADR 0032 之前的当前实现，必须迁移为 Claude 原生 session reader：

```text
{harness_root}/runtimes/{member_id}/       # runtime 目录标记（无持久 pid）

{harness_root}/provider-sessions/{delivery_id}/
  claude.stream-json.ndjson  # 该次 delivery 的完整 NDJSON 流（jsonl_ref）
  claude.stderr              # 仅当 stderr 非空时写入
```

会话延续通过 `--resume`：delivery 从 `system(init)` 帧解析真实 session id，
目标写入 `NativeSessionRef`；下一次对同一 member 的 delivery
用 `--resume <session_id>` 延续同一对话（记忆跨 delivery 保留）。worker cwd
取 member.worktree_ref → project root → process cwd（Claude 从 cwd 发现
CLAUDE.md / .claude/）。

## Native Multi-Agent Features

Claude native subagents（threads 中的代理生成）自动成为
`ProviderChildThread` 对象，而**不升级为 `AgentMember`**。

Doctrine（来自 design spec §4.2(G)）：

> Child threads stay **under** the parent member, not promoted to members.

映射：
- subagent/agent_spawn 形状的事件（`provider_child_thread_from_event`）→
  `ProviderChildThread { agent_path, thread_id, parent_session_id, status }`；
  常规 stream ingest 里 `provider_child_thread_id` 为 None（ADR-0011：
  subagents 单独处理）
- `ProviderChildThread` 在 Dashboard 中显示为"Member runtime panel"的一部分
- 不创建新的 `AgentMember`（除非显式提升）
- 若子代理需要与其他 members 通信，必须通过 parent 代理中转

这与 Codex 的 `subagent/collab_agent_spawn` 相同的原则。

## Evidence and Report Extraction

Claude 输出包含结构化或非结构化内容（code、analysis、plans）。
Harness 把它们梯度化为 `Evidence`、`Proposal`、`Decision`：

```text
Claude output
  ├─ structured JSON (plan/proposal) → Evidence + Proposal (自动)
  ├─ code artifact (file/script)    → Evidence (category: "code_artifact")
  ├─ narrative analysis             → Evidence (category: "agent_output")
  └─ explicit decision              → Decision (若包含 decision_kind 标记)
```

Evidence 生命周期（与 Codex 同）：
1. Raw evidence：capture Claude stdout as-is（`claude.stream-json.ndjson`）
2. Indexed evidence：`source_type = "claude_delivery_session"`
3. Optional graduation：若有结构化标记，升级为 Proposal 或 Decision

## Dashboard Health Signals

实现层的 `runtime_health` 行是 provider-neutral 字段
`{process_alive, socket_exists, protocol_probe, delivery_probe, checked_at}`；
下面的 endpoint/session/delivery 是 Claude 语义下的目标三层映射：

```json
{
  "endpoint": {
    "status": "pass" | "unknown",
    "message": "runtime directory exists"
  },
  "session": {
    "status": "pass" | "warn" | "fail",
    "message": "last session < 5min old" | "no sessions yet"
  },
  "delivery": {
    "status": "pass" | "warn" | "fail",
    "message": "last message delivered" | "delivery pending"
  },
  "checked_at": "2026-05-31T12:34:00Z"
}
```

Codex 有四层（process/socket/protocol/delivery）；Claude 有三层
（endpoint/session/delivery），因为无持久进程。

Dashboard 呈现方式（已 provider-neutral）：
- 绿色 = pass
- 琥珀色 = unknown/warn
- 红色 = fail
- 灰色 = not applicable（provider 不支持该层）

## Fallback Modes

若 claude CLI 不可用或失败：

1. **No fallback to sync claude API** — 与 Codex 的 `codex exec` 类似，
   我们优先维持按需模式。若要用 HTTP API fallback，需在 WP9+ 实现。

2. **Message queueing on delivery failure** — 消息留在 `delivery_status = failed`
   或 `delivery_status = queued`，下次 `agent deliver` 重试。

3. **Health downgrade** — 若 claude CLI 不可用，`endpoint.status = "fail"`，
   Dashboard 显示"Claude not available"。

4. **Reconciliation hook** — 可通过 `agent reconcile` 手工修复状态。

## Unsupported or Risky Surfaces

相比 Codex：

1. **No interrupt / thread pause** — Claude 按需执行，无中途中断机制
   （`turn/interrupt` 只存在于 Codex app-server fallback 契约）。要停止执行，
   必须不再投递消息。

2. **No harness-managed hooks** — harness 未接 Claude hooks；策略通过
   prompt/flag 注入，不如 Codex hook 桥实时。

3. **No mid-turn control channel** — `claude -p` 的 stream-json 是单向
   stdout 流（harness 边读边 tee 做实时观察），没有双向协议连接，无法在
   turn 中途注入输入或审批（resident 模式也只是逐 turn 喂 stdin）。

4. **Subagent lifecycle different** — Claude subagents 是 stateless
   generation（每次 turn 可能产生不同的 subagent），不是持久线程。
   Harness 记录 `ProviderChildThread` 但不能 "resume" 同一个 subagent。

5. **File access is tool-mediated and flag-gated** — Claude 通过自身工具
   （Read/Edit/Write/Bash）访问 cwd 与 `--add-dir` 目录；harness 用
   `--permission-mode` + `--allowedTools` 做物理边界（不像 Codex 用
   `--sandbox`）。

## Validation Gates

实现 Claude 集成的 validation 清单（与 WP6-WP8 对应）：

- [ ] `agent create --provider claude --start` 创建 runtime，记录 session
- [ ] `agent deliver` 投递消息，claude 处理，session 更新
- [ ] Events 解析正确，session/turn/item events 入库
- [ ] Subagent spawn 记录为 `ProviderChildThread`，不升级为 member
- [ ] Evidence 梯度化：CLI output 作为 raw evidence，可选升级为 proposal
- [ ] `agent reconcile` 恢复一致性（与 Codex 同）
- [ ] Health signals 正确（endpoint/session/delivery，无 process 层）
- [ ] `--provider [codex|claude]` 在 CLI help 中可见
- [ ] Codex 路径保持回归干净（无 provider 特定代码泄露）
- [ ] Provider-neutral doctrine 保持（ADR-0011）：core schema 中不出现
  Claude 字面值

## Sequencing with Other Work Packages

WP8 依赖 WP6（provider enum + dispatch）和 WP7（Claude runtime/delivery）。
WP8 目标是：

1. **Event ingest parser** — Claude 特定的事件解析逻辑
2. **Child-thread mapper** — Subagent spawn → `ProviderChildThread`
3. **Integration docs** — 本文档，定义 Claude 与 harness 的边界
4. **CLI help update** — `--provider` 参数文档化
5. **Registry & governance** — docs/registry.json + check-doc-links 更新

一旦 WP8 通过，Claude 成为"一流"provider，与 Codex 功能同等（虽然语义
有差异——那是 provider 特定的，不影响 harness objects）。

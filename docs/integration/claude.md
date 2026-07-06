# Claude Integration

本文档定义 Star Harness 如何集成 Claude Code。重点是把 Claude 变成
harness 里的持久 `AgentMember` provider：可以创建、投递消息、观察状态、回收
运行时，并把执行过程转成 harness 的 `AgentEvent`、`Proposal`、`Evidence`、
`Message` 和 `Decision`。

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
  -> claude agent output parsing
  -> harness store and Agent Dashboard
  -> optional Claude Code plugin packaging after contracts stabilize
```

也就是说：

- `claude CLI` 是按需 provider 执行形式（非持久 app-server）；
- 每次消息投递会通过 harness 的消息上下文生成 Claude 输入 prompt；
- Claude 执行输出被解析为 harness 的事件、提议、证据、决策；
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
- **native subagent + thread handling**：Claude 的 native subagents（通过
  `--thread` 和 `--subagents` 参数）直接产生 session/turn 事件，harness
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
session: most recent ProviderSession exists + has terminal event
delivery: latest message delivery has proof of receipt from Claude
```

Process 层不适用（Claude 按需启动，无持久进程）。

## Message Delivery

每次投递消息时，harness 构造一个包含：
- 当前任务上下文（goal/task/evidence/decision）
- 消息队列（inbox 消息）
- harness 系统 prompt（角色、权限、安全）

然后调用 claude CLI：

```bash
claude --provider-session {session_id} --message "{structured_prompt}"
  --thread {optional_thread_id} --output-format json
```

Claude 执行后返回：
- 新的 `ProviderSession` 记录
- `AgentEvent` 流（start/progress/complete）
- 可选的 `ProviderChildThread` 产生（subagents）
- `Message` 回复（harness 解析 stdout）
- `Evidence` / `Proposal` / `Decision` graduation（从 claude output）

## Event Sources

Claude 产生的事件通过以下源进来：

1. **Claude stdout JSON** — 直接解析 claude 输出，转成 `AgentEvent`
   - session-start：新会话打开
   - turn-start：消息开始处理
   - item-received：Claude 接收到 harness context
   - generation-started：Claude 开始生成
   - generation-completed：Claude 完成生成
   - turn-completed：消息处理完成
   - subagent-spawn：若使用 native subagents

2. **ProviderSession record** — harness-store 记录的会话
   - provider_session_id：Claude 分配的 session ID
   - provider = "claude"
   - thread_id：如果使用 Claude threads
   - terminal_source：证据来源（e.g., "claude_session_output"）

3. **Evidence ingest** — Claude 输出的实证
   - category = "agent_output" / "subagent_output"
   - content：Claude 生成的文本 / 代码 / 分析
   - related_message_id：回复的原始消息

## Reducer Mapping

Claude 事件 → harness objects（`ingest_provider_output` dispatcher）：

```text
(provider = "claude")
  session-start → ProviderSession { id, provider_session_id, thread_id, terminal_source, created_at }
  turn-completed → ProviderSession { last_turn_id, last_event_at }
  subagent-spawn → ProviderChildThread { agent_path, agent_role, thread_id, parent_session_id }
  generation-completed → Evidence { category: "agent_output", content, session_source }
  item/* → AgentEvent { event_kind, provider, payload }
```

Queue discipline（来自 harness，不由 provider 定义）：

- 投递前：消息锁定在 `delivery_status = queued`
- 投递中：更新为 `delivery_status = in_progress`
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
    "service_tier": "free" | "pro" | "team",
    "thread_reuse": true | false
  }
}
```

权限语义：
- `approval_policy = "prompt_required"`：每条消息需要 Lead 审批后投递
- `workspace_policy = "workspaceWrite"`：Claude 可写入运行时目录（如生成文件、日志）
- `service_tier`：影响 rate limits 和推理模型选择
- `thread_reuse`：是否复用一个持久 thread，还是每次投递创建新的

Codex 的 `sandbox_policy` / `service_tier` 具有相似语义。不在 core
schema 中绑定到 provider 特定值（provider-neutral in data model；
mapping 在 CLI 层）。

## Workspace Model

Claude 和 Codex 都假设一个隔离的工作目录：

```text
{harness_root}/runtimes/{member_id}/
  sessions/
    {session_id}/
      context.json         # 投递时的任务+消息上下文
      output.json          # claude 返回的结构化输出
      transcript.md        # 完整对话记录（如果 thread_reuse=true）
  state.json               # runtime 当前状态（pid None，status，last_event_at）
```

若 `thread_reuse = true`：Claude 持久化同一个 thread，harness 记录
thread_id，每次投递 append 到同一 thread 的历史。否则每次投递创建新 thread。

## Native Multi-Agent Features

Claude native subagents（threads 中的代理生成）自动成为
`ProviderChildThread` 对象，而**不升级为 `AgentMember`**。

Doctrine（来自 design spec §4.2(G)）：

> Child threads stay **under** the parent member, not promoted to members.

映射：
- Claude `subagent_spawn` 事件 → `ProviderChildThread { agent_path, thread_id, parent_session_id, status }`
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
1. Raw evidence：capture Claude stdout as-is
2. Indexed evidence：`terminal_source = "claude_session_output"`
3. Optional graduation：若有结构化标记，升级为 Proposal 或 Decision

## Dashboard Health Signals

Dashboard 读取 `runtime_health` 对象（由 harness-cli 计算）：

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
   （Codex 有 `turn/interrupt`）。要停止执行，必须不再投递消息。

2. **No persistent hooks** — Claude 本身无 hooks；harness 通过
   prompt injection 表达策略，不如 Codex hooks 实时。

3. **No websocket live events** — Claude 执行是同步的（或异步但返回一个
   结果），不像 Codex app-server 有实时事件流。harness 通过轮询或
   session 记录观察状态。

4. **Subagent lifecycle different** — Claude subagents 是 stateless
   generation（每次 turn 可能产生不同的 subagent），不是持久线程。
   Harness 记录 `ProviderChildThread` 但不能 "resume" 同一个 subagent。

5. **No local file direct access** — Claude 没有直接的本地文件系统访问
   （Codex 有 `/cwd` api）。所有文件交换都通过 prompt 注入和输出解析。

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

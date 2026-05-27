# Codex Integration

本文档定义 Multi-Agent Harness 如何集成 Codex。这里的重点不是“让
Codex 能跑一次任务”，而是把 Codex 变成 harness 里的持久
`AgentMember` provider：可以创建、投递消息、观察状态、回收运行时，并把
执行过程转成 harness 的 `AgentEvent`、`Proposal`、`Evidence`、`Message`
和 `Decision`。

Provider-neutral runtime contracts live in [../agent-runtime.md](../agent-runtime.md).
This file should explain only how Codex implements those contracts. Shared
object semantics such as `Task`, `Message`, `Evidence`, `Proposal`, and
`Decision` must not be redefined here.

## 核心结论

V1 主方案是：

```text
AgentMember(provider=codex)
  -> AgentRuntime(codex app-server, one process per member)
  -> provider thread
  -> Message delivery through app-server turn/start
  -> app-server notifications + Codex hooks
  -> harness store and Agent Dashboard
  -> optional Codex plugin packaging after contracts stabilize
```

也就是说：

- `codex app-server` 是持久 provider runtime；
- hooks 是生命周期观测、治理、实时状态回写和兜底；
- skills 是 Codex 如何使用 harness/project CLI 的操作指南；
- plugin 是分发和产品化包装层，负责把 harness-managed hooks / skills / MCP
  工具稳定安装进目标项目；
- `codex exec` / `codex review` 是 fallback 和 CI/review helper，不是
  常驻 AgentMember 的主 runtime。

## 为什么 app-server 是主方案

Codex app-server 官方定位就是给外部产品做深度集成：client 可以
`initialize`、`thread/start` 或 `thread/resume`、`turn/start`，并持续读取
thread/turn/item 事件。官方 app-server 文档也说明 `turn/start` 后应通过事件
流观察 `item/*`、`thread/status/changed` 和 `turn/completed` 等生命周期。

对 harness 来说，这正好对应我们需要的控制面：

- 一个 `AgentMember` 可以绑定一个长期 provider thread；
- Leader 可以通过 `Message` 把任务送进该 thread；
- Dashboard 可以看到 runtime pid、socket、thread id、当前 task、事件流；
- provider 事件可以转成 durable harness objects；
- 后续可以通过 `turn/interrupt`、`thread/archive`、`thread/read` 做停止、
  回收和 reconciliation。

直接使用 `codex exec` 做不到这一点。`codex exec resume` 可以恢复本地
transcript，适合一次性自动化任务，但它不是长期在线的 agent instance，也很难
稳定映射到 `AgentMember.status`、实时 proposal、turn event 和队列化消息。

## Provider Runtime 模型

V1 使用 one app-server process per AgentMember。

原因：

- failure domain 清晰，一个 member 崩溃不影响其他 member；
- prompt、cwd、worktree、permission profile、provider thread 都能隔离；
- Dashboard 可以直接显示 pid/socket/thread id；
- close/restart/reconcile 语义简单；
- shared app-server pool 会提前引入调度、隔离、订阅和权限复杂度。

V1 runtime 最小字段：

```text
AgentRuntime
  id
  agent_member_id
  provider = codex
  status
  pid
  control_endpoint = unix://...
  command / args
  started_at / ended_at
  last_event_at
```

健康检查分四层：

```text
process: pid alive
socket: unix control socket exists
protocol: initialize succeeds
delivery: turn/start reaches terminal provider event or reconciles from rollout/hook
```

只有 process/socket 通过不代表真正的 AgentMember 可执行。MVP 验收必须至少
包含 protocol 和 delivery 层。

## 是否需要 Gateway

检查 Codex 源码后，`gateway` 不是 Codex app-server 的核心架构概念。源码中
`gateway` 的命中主要是示例文本、HTTP `BAD_GATEWAY`、network proxy 以及
MCP 测试命名；app-server 自身更接近下面这个结构：

```text
transport acceptor
  -> TransportEvent(ConnectionOpened / ConnectionClosed / incoming message)
  -> message processor
  -> thread / turn / request processors
  -> outbound router
  -> per-connection writer
```

因此如果 harness 需要 gateway，它应该是我们自己的 Provider Gateway，而不是
依赖 Codex 内部已有 gateway。

Harness Provider Gateway 的职责：

- 管理 `AgentRuntime` 生命周期：start、health、restart、close；
- 统一 app-server transport：Unix socket / local WebSocket / future remote
  WebSocket；
- 处理 JSON-RPC state machine：`initialize`、`initialized`、
  `thread/start|resume`、`turn/start`、`turn/interrupt`；
- 把 provider notification 转成 harness event；
- 对接 hooks 和 rollout reconciliation，补齐最终 report；
- 维护 provider capability：是否支持 skills、hooks、review、dynamic tools、
  remote control、command exec；
- 暴露稳定的 harness API：`create member`、`send message`、`deliver`、
  `probe`、`close`。

这个 gateway 是 harness 对 provider 的适配层，不是新的 source of truth。它
读写的最终状态仍然是 harness store。

V1 可以先把 gateway 实现在 CLI/runtime adapter 内；等 provider contract 稳定
后再拆成独立 crate 或 daemon。

## Codex 源码分层和接入点

基于当前 Codex 源码，app-server 的主链路可以理解成下面几层：

```text
CLI / config / auth
  -> transport acceptor
  -> TransportEvent queue
  -> MessageProcessor
  -> request processors
  -> thread / turn / tool execution
  -> OutgoingEnvelope
  -> outbound router
  -> client connection writer
```

对应到 harness 的接入判断：

| Codex 层 | Codex 职责 | Harness 接入方式 | 判断 |
| --- | --- | --- | --- |
| CLI / config / auth | 启动 app-server、读取配置、认证、权限 profile。 | 由 `AgentRuntime` supervisor 生成启动命令、cwd、环境变量、profile。 | 可以接入，但只做启动配置，不把状态放在 Codex config 里。 |
| transport acceptor | 提供 `stdio`、Unix socket、WebSocket、remote control 等连接入口。 | V1 使用 Unix socket transport，并按 WebSocket 协议连接。 | 主接入点之一。不能把 Unix socket 当裸 JSONL。 |
| `TransportEvent` | 把连接打开、关闭、入站 JSON-RPC 统一送入 processor。 | 不直接接入内部 channel，只在外部实现 provider client。 | 不 patch 内部实现。 |
| `MessageProcessor` | 校验初始化状态，分发 `thread/start`、`turn/start`、`thread/read` 等请求。 | 通过 JSON-RPC 方法驱动它。 | 主接入点之一。Harness 要实现自己的 JSON-RPC state machine。 |
| request processors | 具体处理 thread、turn、config、fs、skills、plugins、hooks、remote control。 | 使用公开 request 方法；不要直接调用 Rust 内部 processor。 | 可以使用能力，但不依赖内部 Rust API。 |
| thread / turn | Codex 的会话、轮次、工具调用和 assistant 输出生命周期。 | `AgentMember.provider_thread_id` 绑定 Codex thread；`Message` 投递为 turn。 | 核心映射层。 |
| outbound router | 把 notification/response 路由到已初始化连接。 | harness 读取 notification，转成 `AgentEvent`、`ProviderSession`、report candidate。 | 核心观测层。 |
| hooks | Codex 生命周期外部脚本。 | 作为回写、治理、evidence candidate、reconciliation 辅助。 | 辅助层，不是消息总线。 |
| plugins / skills | 分发 skills、hooks、apps、MCP 工具和操作指南。 | 稳定后打包 harness skill/hook/MCP；turn 输入中显式引用 skill。 | 产品化层，不是 runtime。 |
| rollout / state db | Codex 本地 transcript、thread/turn 可读状态。 | 用 `thread/read`、`thread/turns/list`、hook 回写做最终报告补齐。 | 兜底和审计层。不要把 rollout 当唯一 API。 |
| remote control | 远程控制连接状态和实验性 remote control 请求。 | 暂不作为 V1 主通道。 | 未来可选，不是 gateway。 |

因此我们的最佳接入边界是：

```text
Harness Provider Gateway
  -> process supervisor
  -> Codex app-server WebSocket-over-UDS client
  -> JSON-RPC protocol client
  -> thread/turn/message mapper
  -> notification ingestor
  -> hook and thread-read reconciler
  -> harness store
```

这层 gateway 属于 harness，不属于 Codex。它把不同 provider 统一成
`create member`、`send message`、`deliver`、`health`、`close` 和
`read events`，并且保证 Dashboard 只读 harness store，而不是直接依赖
Codex 的 transcript 或进程内状态。

不建议的接入层：

- patch Codex 内部 `MessageProcessor` 或 processor Rust API：升级成本高，
  且会把 harness 绑死在 Codex 内部实现；
- 只用 hooks：hooks 不能创建常驻 member，也不能可靠投递 queued message；
- 只用 plugin：plugin 解决分发，不解决 runtime 生命周期和 durable state；
- 只用 `codex exec resume`：适合一次性任务，不适合常驻 AgentMember；
- 通过 TUI/PTY 自动化：状态不可结构化，Dashboard 和 task graph 无法可靠验收。

## Codex 全局接入面审计

上面的 app-server 链路只是 runtime control plane。横向检查 Codex 源码后，
还需要把下面这些模块纳入设计，否则 harness 会漏掉 Codex 已经提供的能力或
状态边界。

| Codex 模块 | 关键能力 | 对 Harness 的影响 |
| --- | --- | --- |
| `app-server-client` | 官方内部 remote client facade，封装 initialize、request/response、server request、notification stream，并区分 lossless/best-effort events。 | 我们的 `CodexProtocolClient` 应该对齐这个语义，尤其不能丢 `TurnCompleted`、`ItemCompleted`、assistant deltas、plan/reasoning deltas。 |
| `app-server-daemon` | Codex 自己的 daemon lifecycle、pid file、socket path、operation lock、managed binary、remote-control bootstrap。 | V1 仍用 one process per member；但 supervisor 的 pid/socket/lock/restart 设计应参考 daemon，不要低估 lifecycle 细节。 |
| `core/agent` 和 `multi_agents` tools | Codex 内建 subagent：`spawn_agent`、`send_input`、`wait_agent`、`resume_agent`、`close_agent`，支持 agent path、nickname、role、depth、status。 | 这是 provider 内部 multi-agent 能力，不等于 harness `AgentMember`。需要区分“harness member”和“Codex native subagent”。 |
| `agent-graph-store` / `state.thread_spawn_edges` | 持久化 parent/child thread-spawn edge，支持 open/closed 状态和 descendants 查询。 | 如果启用 Codex native subagents，harness 要 ingest 这些边为 provider child graph；不要丢子 agent 的状态和完成消息。 |
| `thread-store` / `rollout` / `state` | 存储中立的 thread persistence、live writer、history/read/list/search、turn/item pagination、metadata sync。 | `thread/read`、`thread/turns/list` 应优先于 raw rollout parsing；rollout 是 fallback 和 forensic，不是唯一 source。 |
| `protocol` SQ/EQ | Codex core 是 Submission Queue / Event Queue 模型，`Op::UserInput`、`Op::InterAgentCommunication`、`EventMsg::*` 是核心语义。 | Harness `Message` 到 Codex 时通常映射为 `turn/start` user input；provider 内部 agent 通信可映射/观察 `InterAgentCommunication`。 |
| hooks | `SessionStart`、`UserPromptSubmit`、`PreToolUse`、`PostToolUse`、`PermissionRequest`、`SubagentStart`、`SubagentStop`、`Stop` 等。 | 我们之前只覆盖普通 lifecycle，漏了 native subagent hooks；需要把 SubagentStart/Stop 纳入观察和 dashboard timeline。 |
| plugins / core-plugins / skills | plugin bundle、marketplace、本地/远程 sync、skill roots、hook sources、MCP server capabilities。 | plugin 是分发层；但 plugin metadata 可以成为 provider capability source，后续让项目 adapter skill/hook/MCP 一键安装。 |
| MCP / `mcp-server` | Codex 可作为 MCP server，也可连接外部 MCP server；tools 有 provenance、auth、elicitation、sandbox state。 | Harness CLI/API 最终应该提供 MCP server 或 plugin-bundled MCP，让 Codex 用结构化工具而不是只读文档。 |
| `exec-server` / environments | 本地和远程执行环境、fs/process RPC、environment id、remote cwd、WebSocket/stdio transports。 | Worktree、远程机器、per-agent cwd 和权限不应只靠 shell 字符串；未来可把 AgentMember 绑定到 environment。 |
| config / agent roles | user-defined 和 built-in agent roles，role layer 可改 model、instructions、service tier、nickname candidates。 | Harness 创建 member 时需要保存 role prompt、provider config、permission profile；不能只存一个 agent name。 |
| approvals / guardian / sandbox / network proxy | 命令审批、guardian review、execpolicy、filesystem/network sandbox、protected metadata paths。 | Task graph 和 review flow 必须把 permission profile、owned paths、approval reviewer 作为 AgentMember 的一等字段。 |
| agent identity | agent runtime id、task scoped assertion、ChatGPT backend identity。 | 本地 MVP 暂不依赖；远程/云 provider 或跨组织 agent 时需要补 identity/auth 设计。 |
| cloud-tasks / state agent jobs | CSV/job item runner、job/item status、assigned thread、result json。 | 可作为“批量任务”参考，但不等价于我们的 goal/task graph；后续可借鉴 item 状态和 structured result。 |
| external-agent migration | 检测/导入其他 agent 的 config、skills、hooks、sessions、subagents、commands。 | 说明 Codex 已经把跨 agent 迁移当成产品能力；harness 也应提供 project adapter import/export，而不是只支持新项目。 |
| TUI remote | TUI 可连 Unix/WebSocket app-server，区分 remote workspace。 | Dashboard 不应复刻 TUI，但可复用其 remote/local workspace 边界概念。 |
| OTEL / analytics / rollout-trace | turn timing、trace、reduced state、feedback/log db。 | Agent Dashboard 应显示 delivery latency、TTFT/TTFM、tool latency、event lag，而不是只显示最终文本。 |

这次审计后，V1 设计需要补充四个关键判断：

1. `Codex AgentMember` 和 `Codex native subagent` 必须分层。
   - `AgentMember` 是 harness durable actor，有自己的 member id、task
     assignment、runtime、message queue、evidence 和 dashboard 状态。
   - `Codex native subagent` 是某个 Codex thread 内部 spawn 出来的 provider
     child thread。它可以被观察、记录、甚至提升为 evidence，但默认不等同于
     harness member。

2. Provider Gateway 要有两个 ingest 通道。
   - 外部通道：app-server notifications、responses、thread/read。
   - 内部通道：Codex collab/subagent events、thread_spawn_edges、SubagentStart /
     SubagentStop hooks。

3. 我们的 Codex client 不能只是“能发 turn/start”。
   它至少需要实现 request routing、response correlation、server request
   handling、notification lossless tier、disconnect/reconnect、thread-read
   reconciliation。Codex `app-server-client` 已经证明这些都是一等问题。

4. Worktree 和权限要进入 AgentMember schema。
   Codex 已经把 cwd、runtime workspace roots、environment、sandbox policy、
   approval reviewer、permissions profile 做成 turn/thread 配置。Harness 也必须
   把这些纳入 create member 和 task assignment，而不是只在 prompt 里约定。

## Provider Gateway 需要补的实现面

基于全局源码审计，gateway 不能停留在“启动进程 + 发 JSON-RPC”。
合理的 V1.1 边界如下：

```text
ProviderGateway
  RuntimeSupervisor
    - start/stop/restart/probe app-server process
    - pid/socket/lock/stderr/stdout/session files
  CodexTransportClient
    - WebSocket over UDS
    - future TCP WebSocket
    - reconnect and disconnect classification
  CodexProtocolClient
    - initialize / initialized
    - request id correlation
    - response/error mapping
    - server request resolve/reject
    - notification lossless tier
  ThreadTurnMapper
    - AgentMember <-> provider thread
    - Message <-> turn/start
    - interrupt/close/archive/read
  NativeSubagentIngestor
    - collab events
    - thread_spawn_edges
    - agent_path/nickname/role/status
    - SubagentStart/SubagentStop hooks
  Reconciler
    - turn/completed
    - thread/status idle
    - thread/read and thread/turns/list
    - Stop hook final report
  HarnessStoreWriter
    - AgentRuntime
    - AgentEvent
    - ProviderSession
    - Message status
    - optional ProviderChildThread
```

V1 已经把 `ProviderChildThread` 做成 Rust 类型和 JSON schema。否则一旦 Codex
member 内部真的 spawn 了 native subagent，Dashboard 会只看到父 member，丢掉
真正执行工作的子 thread。

当前 contract 要保留以下字段，否则 Codex 的常驻 agent 只能“跑起来”，不能被
验收为可治理的 harness member：

- `AgentMember.provider_config`：model、service tier、collaboration mode、
  approval policy、sandbox/permissions profile、runtime workspace roots；
- `AgentRuntime.health`：process/socket/protocol/delivery 四层健康检查；
- `AgentEvent.provider_thread_id`、`provider_turn_id`、`provider_child_thread_id`：
  让 dashboard 能按 provider thread/turn 聚合；
- `ProviderChildThread`：记录 Codex native subagent 的 path、
  nickname、role、status、parent thread、final message；
- `Message.delivery`：request id、turn id、terminal source
  (`turn_completed`、`thread_idle`、`thread_read`、`hook_stop`)。

## 当前不改变的主判断

全局审计没有推翻 app-server 主方案，但改变了我们对“完整集成”的定义：

- 主 runtime 仍是 `codex app-server`；
- 不能只实现 hot path delivery，还要实现 event/reconciliation/permission
  体系；
- Codex native subagent 是 provider 内部能力，短期作为观测对象，长期可成为
  harness 的优化执行后端；
- plugin/MCP 是 Codex 使用 harness/project CLI 的产品化入口，不是 durable
  store；
- `exec-server` / environment 是 worktree 和远程执行的未来边界，不能在
  `AgentMember` schema 里缺席；
- cloud-tasks/agent-jobs 可参考，但我们的 goal/task graph 仍应由 harness
  自己定义。

## Transport 和协议

Codex app-server 的 Unix socket transport 是 WebSocket over Unix socket。
因此 harness 不能把普通 JSONL 或 LSP `Content-Length` frame 直接写到 socket。

正确流程是：

```text
connect unix socket
perform WebSocket HTTP Upgrade
send JSON-RPC message per WebSocket text frame
initialize
initialized notification
thread/start or thread/resume
turn/start
read notifications and responses until terminal event
```

需要注意两个边界：

- Codex JSON-RPC wire message 通常不需要 `"jsonrpc": "2.0"` 字段；
- `codex app-server proxy` 是 raw byte pipe，不会替 harness 生成 WebSocket
  frame，因此不能作为 JSON-RPC client 使用。

## Message Delivery

Harness 的 `Message` 是源头，Codex thread 只是 provider execution context。

```text
Message(delivery_status=queued)
  -> runtime adapter connects to app-server
  -> initialize / initialized
  -> thread/start if provider_thread_id is empty
  -> turn/start(thread_id, input=message envelope)
  -> collect provider output
  -> append AgentEvent / ProviderSession
  -> update Message(delivered or failed)
  -> append report Message when completion can be reconciled
```

`turn/completed` 是首选 terminal event。实测中可能先看到
`thread/status/changed(status.type=idle)`，而最终 assistant message 已经写入
Codex rollout。V1 可以把 `thread idle` 作为 terminal provider signal，但最终
报告仍应通过 rollout reconciliation 或 Stop hook 补齐。

## Hooks 集成

Hooks 不是 runtime，也不是 message bus。它们是 Codex lifecycle 里的确定性
脚本，适合做观测、校验、上下文注入和回写。

建议的 hook 用法：

| Hook | Harness 用法 |
| --- | --- |
| `SessionStart` | 注入 AgentMember/role/task 协议上下文，记录 session/thread 启动。 |
| `UserPromptSubmit` | 校验 prompt 是否带 harness message envelope，必要时补充上下文。 |
| `PostToolUse` | 把命令输出、检查结果、写文件行为记录成 evidence candidate。 |
| `Stop` | 把最后 assistant message、turn id、transcript path 回写为 report candidate。 |
| `PreToolUse` / `PermissionRequest` | 做安全和 owned-path guardrail。 |

Hook 输出必须进入 harness store 后才算 durable evidence。Hook 观察到的事实可以
帮助 reconciliation，但不能越过 `Message`、`Task`、`Decision` 的状态机。

Dashboard 的实时状态不能只靠轮询最终 snapshot。正确分层是：

```text
app-server notification stream
  -> lowest-latency live state: turn status, deltas, tool/process output
harness-managed hooks
  -> lifecycle checkpoints: SessionStart, PostToolUse, SubagentStart/Stop, Stop
rollout/thread-read reconciliation
  -> terminal fallback when a notification or hook is missed
```

因此 hooks/plugin 是产品化实时 dashboard 的必需能力。这里的限制只针对
unmanaged global plugin hooks：一个全局安装的第三方 hook 可以把 runtime 文件
写进任务 worktree，触发 owned-path gate，并污染 worker diff。Harness-managed
Codex runtimes 默认禁用这类全局 `plugin_hooks`。`hook record` bridge 已有，
但 v0.130.0 smoke 显示普通 `--config hooks.*` session override 没有触发 hook
command；生产路径应切到 trusted harness plugin 或 managed requirements policy。
`HARNESS_CODEX_ENABLE_PLUGIN_HOOKS=1` 只适合显式接受全局 plugin hook 副作用的
诊断任务。

## Skills 集成

Skills 是 “Codex 如何工作” 的操作指南，不是运行时对象。我们需要两类 skill：

- generic harness skill：如何使用 `Goal -> Task -> Message -> Evidence ->
  Decision -> GoalEvaluation` 工作流；
- project adapter skill：如何使用某个项目的 CLI、Dashboard、回测、实盘、
  CI/CD 和证据体系。

在 app-server `turn/start` 中可以显式带 skill input item，避免模型自行查找
skill 带来的延迟和不确定性。

## Plugins 集成

Codex plugin 的职责是把 skills、hooks、apps、MCP servers 打包并通过
marketplace 或本地 bundle 分发。它不是 durable source of truth，但它是
跨项目稳定安装实时 hook、project skill 和 harness MCP 工具的产品化方式。

```text
harness plugin
  -> bundled skills
  -> bundled hooks
  -> optional MCP server for harness commands
  -> optional app integration
  -> metadata and permissions
```

Plugin 不应该成为核心状态机 correctness 的依赖。原因：

- plugin 安装、启用、信任和外部 app/MCP auth 都是额外变量，不能决定
  `Message`、`Task`、`Decision` 是否有效；
- plugin 解决分发、实时 hook 安装和用户体验，不解决 AgentMember 的进程生命周期；
- plugin 不应该拥有 harness store，也不应该替代 Leader decision。

但对“好用的 Agent Dashboard”来说，plugin/hook 是必须项。没有它们，
Dashboard 只能看 delivery 结束后的状态或短轮询 snapshot，无法稳定展示工具
调用、permission request、subagent start/stop、Stop hook report 和最终
assistant message。合理顺序是 `docs -> skill -> schema -> CLI/API ->
managed hooks -> plugin`；plugin 帮项目获得 workflow、hook 和 MCP 工具，
canonical state 仍在 harness backend/store。

## 替代方案判断

| 方案 | 适用 | 不作为主方案的原因 |
| --- | --- | --- |
| `codex app-server` | 持久 AgentMember runtime | 当前最佳主方案。 |
| `codex exec resume` | CI、一次性自动化、fallback | 不是常驻进程，实时状态和队列化消息弱。 |
| Codex TUI `--remote` | 人连接远端 app-server | 是交互入口，不是 harness backend 控制面。 |
| Codex SDK / Responses / Agents SDK | 自研 provider、非 Codex agent | 会重建 Codex repo/tool/approval/skill 能力。 |
| Codex native subagents | Codex 内部并行辅助 | 可以作为 provider child graph 观察；默认不能替代 harness AgentMember。 |
| Hooks only | 观测、治理、回写 | 不能投递消息、管理 runtime、维护 task graph。 |
| Plugin only | 分发 skills/hooks/MCP | 不能替代 app-server runtime 或 harness store。 |

## 验收标准

一个 Codex AgentMember 集成通过 MVP 验收，需要同时满足：

- `agent create --provider codex --start` 创建 member、prompt 和 runtime；
- `agent health` 显示 pid alive、socket exists、protocol probe pass；
- `agent send` 产生 queued `Message(kind=task)`；
- `agent deliver` 通过 app-server 投递到同一 provider thread；
- provider session 记录 request/stdout/stderr 或等价 event fixture，包含
  provider thread id、terminal source，并能解释缺失 turn id 的 reconciliation；
- harness store 里有 `AgentEvent`、delivered/failed message 状态变化，message
  的 `delivery` 字段能解释它是由哪个 provider session 和 turn 产生的；
- 如果 provider 产生 Codex native subagent，必须转成 `ProviderChildThread` 或
  带 `provider_child_thread_id` 的 `AgentEvent`，Dashboard 不能只展示父 member；
- turn completion 能通过 notification、thread idle + rollout、或 Stop hook
  reconcile 成 report candidate；
- Dashboard 能显示 member runtime health、message、event timeline、provider
  session 和 provider child thread；
- reviewer/critic 或 Leader decision 不能只依赖 chat summary。

以下情况不能算通过：

- 只有 `codex exec` / `codex review` 一次性 session，没有常驻 runtime；
- 只有 dry-run delivery，没有真实 app-server request/response 或失败 fixture；
- 只有 pid/socket alive，没有 protocol probe；
- 只有 provider stdout 文本，没有映射到 `Message`、`AgentEvent`、
  `ProviderSession`、`Evidence` 或 `Decision`；
- 只在聊天里说明完成，没有 critic/evaluator evidence 和 Leader decision。

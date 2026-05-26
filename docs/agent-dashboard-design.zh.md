# Agent Dashboard 设计

本文规划 Agent Dashboard。它是 multi-agent harness 的协作、审计和验收界面，
不是产品 Dashboard，也不替代市场图、策略图、same-market replay 或交易复盘图。
产品边界和拆分计划见
[Multi-Agent 产品边界与拆分计划](product-boundary.zh.md)。

## 设计动机

我们过去的问题不是缺少日志，而是缺少“谁发现了什么、证据在哪里、结论是否被
挑战、修复是否真正命中根因、下一步为什么被允许”的可审计链路。

Agent Dashboard 的目标是让 Lead 和人类 reviewer 快速回答：

```text
当前策略研究处在哪个场景？
哪些 Agent Member 正在工作？
任务和报告是否完整？
哪些 claim 有证据，哪些只是猜测？
哪些 blocker 阻止 live、promotion、scale 或 release？
下一步最小行动是什么？
```

## 与产品 Dashboard 的边界

| 界面 | 负责什么 | 不负责什么 |
| --- | --- | --- |
| 产品 Dashboard | 市场 replay、策略买卖点、订单生命周期、same-market compare、截图证据 | agent 成员生命周期和协作审计 |
| Agent Dashboard | agent member、message thread、task/report、claim、blocker、decision、permission、provider session | 重新画市场主图或复制策略 replay |

Agent Dashboard 必须 deep-link 到产品 Dashboard 的市场证据。市场价格线、Binance
delta、PM orderbook、entry/exit marker、same-market compare 仍由产品 Dashboard
承担。

## 主要对象

Agent Dashboard 第一版读取这些对象：

- `AgentRole`：职责模板；
- `AgentMember`：实际注册/常驻 agent instance；
- `SubagentSession` / `ProviderSession`：provider 内部临时执行会话；
- `AgentMessage`：统一通信记录，`type=message | task | report`；
- `AgentTask`：从 task message materialize；
- `AgentReport`：从 report message materialize；
- `Claim`：可被支持、挑战、拒绝或标记 stale 的结论；
- `Blocker`：阻止 live、promotion、scale、merge、release 的问题；
- `Decision`：Lead 的决策；
- `PermissionGrant`：涉及 live/order/wallet/资金扩大的授权；
- `EvidenceRef`：指向 artifact、log、schema、Dashboard URL 或截图。

## 信息架构

### 1. Harness Overview

首页显示当前 active scenario 和全局状态。

核心模块：

- 当前 scenario、round、strategy family、experiment card；
- Lead decision 和下一步最小行动；
- P0 blocker 摘要；
- stale member、stale report、unmaterialized gate-relevant message；
- live/order/wallet/permission 风险提示；
- 最近 artifact 和产品 Dashboard deep link。

这个页面回答：“现在能不能继续推进？如果不能，卡在哪里？”

### 2. Agent Member Roster

展示所有 Agent Member，而不是抽象角色列表。

字段：

- `member_id`；
- role；
- provider；
- status；
- heartbeat age；
- current task；
- capabilities；
- permissions；
- artifact root；
- last report；
- open blockers。

必须支持：

- create；
- pause/resume；
- retire；
- delete；
- view history。

删除不能删除历史 message、task、report、claim 或 decision。

### 3. Message Threads

这是 Agent Dashboard 的中心页面。

列表维度：

- scenario；
- channel；
- assigned member；
- status；
- blocker severity；
- materialization state；
- stale age；
- evidence completeness。

Thread detail 显示：

- 完整 message 流；
- task/report message；
- reply chain；
- evidence refs；
- claim refs；
- materialized task/report/claim/blocker/decision；
- 未回答追问；
- Lead handoff。

这个页面回答：“一个任务从提出到报告再到追问和决策，中间发生了什么？”

### 4. Task Board

Task Board 只展示需要状态跟踪的任务，不展示所有聊天。

列：

- proposed；
- assigned；
- running；
- waiting；
- reporting；
- blocked；
- done；
- archived。

每个 task card 显示：

- objective；
- assigned member；
- required outputs；
- due/stale；
- linked thread；
- linked report；
- blockers；
- next action。

### 5. Report Index

Report Index 是 agent 产出物目录。

过滤：

- role；
- member；
- provider；
- scenario；
- evidence completeness；
- claim status；
- changed surface；
- stale/current。

每个 report 必须能看到：

- objective；
- inputs；
- evidence refs；
- findings；
- claims；
- blockers；
- recommendations；
- changed surfaces；
- confidence；
- next actions。

### 6. Claim Ledger

Claim Ledger 是避免“凭感觉推进”的关键页面。

状态：

- proposed；
- supported；
- challenged；
- rejected；
- stale。

每个 claim 显示：

- claim text；
- source message/report；
- supporting evidence；
- challenge thread；
- decision impact；
- owner；
- stale reason。

任何影响 live、promotion、scale、merge 或 release 的 claim 必须 materialize。

### 7. Blocker Board

Blocker Board 显示所有阻止推进的问题。

分类：

- live safety；
- order lifecycle；
- wallet/reconciliation；
- data/fabric；
- backtest parity；
- strategy contract；
- Dashboard evidence；
- permission/security；
- docs/schema/CLI。

每个 blocker 必须有：

- severity；
- code；
- source；
- required action；
- owner；
- evidence；
- allowed next action。

### 8. Permission Queue

涉及真实资金、钱包、订单、live launch、扩大 size、merge/redeem、secret 的动作
进入 Permission Queue。

显示：

- requested action；
- requester；
- target member；
- risk level；
- scope；
- expires_at；
- approving Lead；
- linked task/thread；
- audit log。

未授权的 live/order/wallet action 不能被 UI 误显示为可执行。

### 9. Provider And Subagent Sessions

这个页面把 provider 内部临时执行和 harness 成员关联起来。

显示：

- provider；
- provider session id；
- parent member；
- task/thread；
- status；
- start/end；
- artifacts written；
- orphan session warning。

Subagent 不是 Agent Member。只有被 member 记录进 message/report/artifact 的输出
才进入 harness 审计链。

### 10. Decision Timeline

Decision Timeline 展示 Lead 的每次关键决策。

字段：

- decision；
- source threads；
- source reports；
- claims accepted/rejected；
- blockers resolved/kept；
- allowed next action；
- rationale；
- created_at。

它回答：“为什么当时允许继续 1h/8h live，或者为什么不能 promote？”

### 11. Evidence Links

Evidence Links 是跨系统索引，不是新证据源。

支持链接到：

- product Dashboard strategy page；
- product Dashboard market detail；
- same-market compare；
- screenshot artifact；
- live/backtest artifact；
- execution log；
- order lifecycle packet；
- wallet/reconciliation report；
- Trial DAG node；
- schema/CLI output。

## 布局原则

Agent Dashboard 应该是安静、密集、可扫描的操作台。

- 左侧固定导航：Overview、Members、Threads、Tasks、Reports、Claims、
  Blockers、Permissions、Sessions、Decisions、Evidence。
- 顶部显示当前 scenario、active round、Lead decision、P0 blocker 数量。
- 中间使用表格、timeline、split pane 和 details drawer。
- 只用卡片展示重复实体，不把页面 section 做成大卡片。
- 默认按 blocker severity、stale age 和 decision impact 排序。
- 所有状态都要有明确空态：`no evidence`、`not materialized`、`stale`、
  `blocked`、`not applicable`。
- 不在页面内解释功能说明或快捷键，交互通过清晰标签和 tooltip 完成。

## MVP 实现切片

第一版不要先做复杂 UI。推荐顺序：

```text
schema contracts
  -> file-backed CLI writes
  -> read API
  -> Overview + Threads + Members
  -> Task/Report materialization
  -> Claim/Blocker/Decision pages
  -> Permission/Session pages
  -> product Dashboard deep links
```

最低可用 API：

```text
GET  /api/agent-harness/overview
GET  /api/agent-harness/members
POST /api/agent-harness/members
POST /api/agent-harness/messages
GET  /api/agent-harness/threads
GET  /api/agent-harness/threads/:threadId
POST /api/agent-harness/materialize/task
POST /api/agent-harness/materialize/report
GET  /api/agent-harness/claims
GET  /api/agent-harness/blockers
GET  /api/agent-harness/decisions
```

第一版可以读取 `artifacts/agent-harness/<run-id>/` 的 file-backed artifacts。
等对象稳定后再决定是否迁移到数据库。

## 验收标准

Agent Dashboard 通过验收时必须满足：

- 能区分 Agent Role、Agent Member 和 Subagent；
- 能创建、暂停、恢复、退休和删除 Agent Member；
- 能把 `type=task` message materialize 成 AgentTask；
- 能把 `type=report` message materialize 成 AgentReport；
- 任何影响 gate 的 claim 都能回到 evidence；
- blocker 能明确说明阻止什么以及下一步要做什么；
- Lead decision 能回到 source thread/report/claim；
- stale report 不会被当成当前事实；
- provider/subagent 工作不会变成无归属结论；
- 市场证据通过产品 Dashboard deep link 查看。

## 与独立仓库的关系

Agent Dashboard 和 agent harness 可以成为独立产品。需要区分两层：

- docs/skills：主要是 agent 使用指南和设计依据，耦合很低，可以较早抽成通用
  harness 包；
- runtime implementation：CLI、API、artifact reader、Dashboard deep link、
  provider adapter 和 Earning Engine adapter，当前仍和本 repo 的证据系统有关。

当前阶段 runtime implementation 依赖：

- Earning Engine artifacts；
- strategy trial DAG；
- live/backtest round；
- 产品 Dashboard URL；
- Polymarket order/wallet/reconciliation 语义；
- S1 和 5m MM 验收场景。

正确做法是先在当前 repo 形成清晰边界，同时保持 docs/skills 可抽取：

```text
generic agent-harness kernel
  objects / schemas / file store / CLI / Agent Dashboard

earning-engine adapter
  Polymarket strategy scenarios / Trial DAG / live-backtest artifacts /
  product Dashboard links / wallet-order safety gates
```

docs/skills 可以先迁入独立 harness repo 或 package。runtime implementation 等
generic kernel 不再直接 import 当前 repo 内部实现、只通过 adapter 消费 artifact/API
时，再拆成独立仓库或插件。

# Multi-Agent Harness 设计

本文定义 Multi-Agent Harness 的多 agent 体系。这里的 agent 不是泛指某个
LLM 会话，而是一个可审计、可复用、可被 Dashboard 展示的工作角色。Codex 是
当前优先落地对象，但接口设计必须允许 Claude Code、Hermes-agent 或其他执行体
接入。

Agent Member 的创建/删除、消息线程、任务提交、报告返回、追问和 materialization
细节见 [Multi-Agent 生命周期与消息协议](multi-agent-lifecycle.zh.md)。

产品边界见 [Multi-Agent 产品边界与拆分计划](product-boundary.zh.md)：Multi-Agent
产品是工具使用者，业务项目通过 adapter 接入。通用内核不应该内嵌策略、回测、
实盘、钱包、市场或其他项目业务逻辑。

## 背后动机

单个 agent 很难同时做好研究、运行监控、工具调用、证据复盘、基础设施修复和
最终决策。更严重的是，单 agent 容易把不同场景混在一起：

- 看到工具失败就调业务逻辑；
- 看到 PnL 差就改参数；
- 看到 Dashboard 空就以为任务没跑；
- 看到实际结果和模拟结果不同就马上改模型；
- 看到 log 异常但没有形成根因修复。

Multi-agent harness 的目标不是制造更多聊天，而是把复杂工作流程拆成稳定职责
模板、常驻 agent 成员、临时执行单元和结构化证据。Lead 最后基于证据做决策。

## Agent Role、Agent Member 与 Subagent

这三个概念必须区分。本文按用户语义使用 `Agent Member`：它等价于一个实际
注册/常驻的 agent 实例，而不是抽象角色。

| 概念 | 定义 | 生命周期 | 是否稳定出现在 harness 对象里 | 示例 |
| --- | --- | --- | --- | --- |
| Agent Role | 稳定职责模板，定义输入、输出、权限和验收标准 | 长期存在，通常由 docs/schema 定义 | 是，作为 role id | `lead`、`execution`、`market_review` |
| Agent Member | 一个实际注册/常驻 agent 实例，绑定 role、provider、capability 和状态 | 可创建、暂停、恢复、退休、删除 | 是，作为 member id | `execution-codex-01`、`critic-hermes-01` |
| Subagent | 某次任务中由 Codex 或其他 provider 临时派生的执行单元 | 单次任务或短期会话 | 否，除非它写入 task/report | Codex explorer、worker |

Agent Role 是职责模板。Agent Member 是常驻成员实例。Subagent 是执行手段。

例如：

```text
Agent Role: execution
Agent Member: execution-codex-01
  provider: codex
  responsibility: 订单生命周期和执行 primitive
  current task: inspect FAK/FOK late-fill incident
  temporary subagent: Codex explorer A 检查 log
  output: execution-agent-report.json
```

同一个 Agent Role 可以有多个 Agent Member：

```text
execution-codex-01
execution-hermes-01
execution-human-reviewer-01
```

一个 Agent Member 绑定一个 provider。Provider 可以是：

- `codex`；
- `claude_code`；
- `hermes_agent`；
- `script`；
- `ci_job`；
- `human`。

反过来，一个 Codex subagent 不应该自动等于 Agent Member。只有当它的工作被
某个 Agent Member 记录到 task/report 里，才算进入 harness 审计链。

## 优先实现原则

当前优先考虑 Codex，因为 Codex 已经能读 repo、改代码、跑 CLI、检查 artifact、
派生 subagent，并能把修复落到文件和测试里。

但设计不能被 Codex 专属能力锁死。正确抽象是：

```text
Agent Role contract
  -> Agent Member runtime
  -> provider adapter
    -> codex
    -> claude_code
    -> hermes_agent
    -> script
    -> human_review
```

Provider 只负责执行。Harness 只相信 artifact。

## Agent Role 列表

### Lead Agent

职责：

- 场景分类；
- 风险和资金控制；
- 决定是否 live、pause、promote、fork、fix、kill；
- 整合其他 agent report；
- 决定下一条最小可验证行动。

输入：

- ExperimentCard；
- StrategyNode；
- Round；
- MarketPacket；
- AgentReports；
- Dashboard evidence；
- live/backtest comparison；
- wallet/reconciliation。

输出：

- LeadDecision；
- next action；
- promotion / diagnostic / stop verdict；
- unresolved risk list。

Lead 不应该把所有工作都自己做。Lead 的价值是判断和整合。

### Scenario Router Agent

职责：

- 判断当前请求属于哪个策略开发场景；
- 选择需要哪些 Agent Role 和 Agent Member；
- 防止错误路由，例如把执行层 bug 当成策略参数问题。

输入：

- user request；
- recent incident；
- current live status；
- artifact refs；
- product gap inbox。

输出：

- scenario classification；
- required evidence；
- recommended roles；
- recommended members；
- allowed actions；
- blocked actions。

### Strategy / EdgeHunt Agent

职责：

- 提出或评估 edge thesis；
- 维护 StrategyFamily 和 StrategyNode；
- 从复盘中产生新策略想法；
- 设计 fixed-market backtest 和参数 sweep；
- 区分新策略家族和旧策略变体。

输出：

- thesis proposal；
- StrategyNode draft；
- changed_axis；
- expected evidence；
- required comparison；
- kill/fork/refine suggestion。

对于 5m MM 策略，它应该先写出独立 thesis，而不是在 S1 上加 flag。

### Backtest Parity Agent

职责：

- 运行 same-market backtest；
- 判断 live/backtest divergence；
- 设计 calibration plan；
- 保护回测引擎不被单个市场过拟合。

输出：

- replay result；
- divergence classification；
- calibration jobs；
- model gap report；
- replay completeness verdict。

### Market Review Agent

职责：

- 使用产品 Dashboard 查看市场图；
- 标注 edge window；
- 判断 entry/exit/no-fill 是否和 edge 对齐；
- 产出 screenshot packet 和 visual verdict。

输出：

- visual edge labels；
- fill-to-edge alignment；
- screenshot refs；
- market review verdict。

Market Review Agent 使用产品 Dashboard。它不使用 Agent Dashboard 作为市场证据。

### Execution Agent

职责：

- 验证订单 primitive；
- 分析 submit、ack、fill、cancel、late fill、MINED、sellable、merge、reconcile；
- 发现执行状态机 bug；
- 要求策略消费执行能力前先做真实小额 probe。

输出：

- OrderLifecyclePacket；
- execution primitive contract；
- real-order probe result；
- safety blocker；
- code fix recommendation。

FAK/FOK terminal grace 修复就是 Execution Agent 应该自动识别的模式。

### Live Ops Agent

职责：

- 检查进程、PM2、driver、supervisor、child live loop；
- 检查 wallet store、env、safe、余额；
- 检查 stop criteria；
- 确认 live 是否仍在正常采样。

输出：

- live status；
- process tree；
- launch readiness；
- stop/pause recommendation。

### Data / Fabric Agent

职责：

- 检查 Polymarket 和 Binance 数据新鲜度；
- 检查 fabric fanout、consumer lag、upstream WS 数量；
- 判断 replay input 是否完整；
- 区分 Chainlink auxiliary 问题和 S1 决策数据问题。

输出：

- fabric status；
- input completeness；
- data freshness verdict；
- replayability blocker。

### Dashboard Product Agent

职责：

- 判断产品 Dashboard 是否能解释策略表现；
- 设计页面、API、图层和布局；
- 识别市场证据面缺口；
- 保持 strategy/run/market/trial/compare 之间的证据模型一致。

输出：

- dashboard gap；
- route/page responsibility；
- API/schema need；
- screenshot evidence readiness。

### Agent Dashboard Agent

职责：

- 设计和维护 Agent Dashboard；
- 展示 Agent Member 状态、任务、report、claim、blocker、handoff；
- 追踪多 provider agent 的输出和责任边界；
- 链接到产品 Dashboard 证据，但不复制市场图。

输出：

- agent cockpit design；
- member status model；
- task graph；
- report index；
- claim/evidence audit view。

### Critic / Gate Agent

职责：

- 挑战 unsupported claim；
- 检查是否把诊断样本当 promotion；
- 检查是否缺 screenshot、reconcile、fabric、same-market evidence；
- 输出 gate verdict。

输出：

- blocker list；
- promotionAllowed；
- diagnosticSamplingAllowed；
- required fixes。

### Knowledge Agent

职责：

- 把重复痛点转成 docs、schemas、CLI、Dashboard、skills；
- 管理 skill/archive；
- 确保 durable truth 进入 docs，不停留在聊天。

输出：

- docs diff；
- schema proposal；
- skill update；
- product-gap entry；
- archive recommendation。

## Agent Member Runtime Summary

Agent Member 是可创建和删除的常驻实例。设计层只保留原则：

- Member 绑定一个 role、provider、capabilities 和 permissions。
- Member 的历史 task、message、report、claim、decision 必须在 retire/delete 后保留。
- Member 之间通过 message-first 协议递交任务、返回报告、追问和补证据。
- Task、AgentReport、Claim、Blocker 和 Decision 从 message thread materialize 出来。
- 涉及 live、wallet、order execution 的 member 必须有明确权限标记。

完整生命周期、AgentMessage 字段、thread 示例、materialization 规则、权限要求和
CLI 草案见 [Multi-Agent 生命周期与消息协议](multi-agent-lifecycle.zh.md)。

## Agent Member Runtime Interface

所有 Agent Member 的输出都应可归一成同一种报告形状。

```text
AgentReport
  report_id
  agent_role
  agent_member_id
  provider
  provider_session_id?
  scenario
  objective
  inputs
  evidence_refs
  findings
  claims
  blockers
  recommendations
  changed_surfaces
  confidence
  next_actions
  generated_at
```

关键规则：

- 没有 evidence ref 的 claim 不能进入 promotion。
- provider 不是信任边界，artifact 才是信任边界。
- report 必须说明它服务哪个 scenario。
- report 可以由 Codex 主 agent、Codex subagent、Claude Code、Hermes-agent、
  脚本或人工 reviewer 产生。

## Subagent 使用规则

Codex subagent 适合处理并行、边界清楚、不会阻塞主路径的任务：

- 一个 explorer 检查 Dashboard 页面职责；
- 一个 explorer 检查 DAG/manifest；
- 一个 worker 修某个独立 CLI；
- 一个 worker 补测试；
- 一个 critic 检查文档自洽性。

不适合交给 subagent 的任务：

- 当前最阻塞的 live 安全判断；
- 需要 Lead 立即决策的资金/风险问题；
- 跨多个模块的最终整合；
- 不清楚写入边界的代码修改。

Subagent 输出必须被 Lead 整合。不能把 subagent 的自然语言总结当最终事实。

## Agent Dashboard 与产品 Dashboard 的区别

这两个 Dashboard 必须分开。

### 产品 Dashboard

产品 Dashboard 是市场和策略证据面。它回答：

```text
市场发生了什么？
Binance / PM / depth / taker flow 怎么变化？
策略在哪里 signal、submit、fill、exit？
live 和 backtest 在同一市场上有什么差异？
```

典型页面：

- `/strategy/:strategyName`
- `/market/:conditionId`
- `/market/:conditionId/compare`
- `/compare/:bundleId`
- `/runs/:runId/gallery`
- `/trial/:trialId`

它服务 Market Review、Backtest Parity、Strategy 和 human researcher。

### Agent Dashboard

Agent Dashboard 是多 agent 协作和审计控制台。它回答：

```text
现在哪个场景在运行？
哪些 Agent Role 需要参与？
哪些 Agent Member 被创建、分配或暂停？
每个 member 看了哪些 evidence？
哪些 claim 已被证据支持？
哪些 blocker 还没解决？
哪个 subagent/provider 产生了哪个 report？
Lead 为什么做这个 decision？
```

它不应该重复画完整市场图。它应该 deep-link 到产品 Dashboard 的图、截图和
comparison bundle。

建议路由独立命名，例如：

- `/agent-harness`
- `/agent-harness/runs/:harnessRunId`
- `/agent-harness/reports/:reportId`
- `/agent-harness/tasks/:taskId`
- `/agent-harness/claims/:claimId`

Agent Dashboard 的核心组件：

- Scenario Router 面板；
- Agent Role catalog；
- Agent Member roster；
- Task graph；
- Evidence refs；
- Claim ledger；
- Blocker board；
- Decision timeline；
- Provider/subagent sessions；
- Handoff / heartbeat；
- Links to product Dashboard。

## Multi-Provider 集成边界

Agent provider 接入不应该直接写最终结论。它只能提交 report。

```text
ProviderAdapter
  start_task(agent_member_id, objective, inputs)
  collect_output()
  normalize_to_agent_report()
  attach_evidence_refs()
  mark_status()
```

Provider 状态：

- `queued`
- `running`
- `completed`
- `failed`
- `stale`
- `superseded`

Provider 类型：

- `codex_main`
- `codex_subagent`
- `claude_code`
- `hermes_agent`
- `script`
- `human`

Codex 优先落地，因为它已经是当前最强 repo-operating agent；其他 provider 在
接口稳定后接入。

## CLI 优先原则

每个场景最终都应该有好用 CLI。Agent 不是靠记忆操作系统，而是调用场景 CLI。

示例目标：

```bash
node scripts/agent-harness.mjs role list
node scripts/agent-harness.mjs member create --role execution --provider codex
node scripts/agent-harness.mjs member list
node scripts/agent-harness.mjs member retire <member-id>
node scripts/agent-harness.mjs member delete <member-id>
node scripts/agent-harness.mjs task create --scenario canary_live --role execution
node scripts/agent-harness.mjs task assign <task-id> --member <member-id>
node scripts/agent-harness.mjs message send --from <member-id> --to <member-id-or-channel>
node scripts/agent-harness.mjs report add --member <member-id> --file <report.json>
node scripts/strategy-harness.mjs scenario classify --input <request-or-artifact>
node scripts/strategy-harness.mjs strategy-node show <strategy>
node scripts/strategy-harness.mjs gate-census --round-dir <round>
node scripts/strategy-harness.mjs order-lifecycle --round-dir <round> --condition-id <id>
node scripts/strategy-harness.mjs decision build --round-dir <round>
```

CLI 产生 artifact，Dashboard 展示 artifact，agent 基于 artifact 决策。

## 成熟度路径

```text
docs
  -> schema
  -> CLI/API
  -> product Dashboard links
  -> Agent Dashboard
  -> skill
  -> plugin
```

现在应先写 docs 和 schema。skill 适合沉淀高频操作。plugin 只有在接口稳定后
才有意义。

# Multi-Agent Harness 验收模型

本文定义 Multi-Agent Harness 如何验收。验收目标不是证明某一个业务项目成功，
而是证明 harness 能稳定帮助 agent 发现、验证、修复、比较和迭代任务。

## 背后动机

如果 harness 的价值只是启动 live、汇总 PnL 或展示几个 log，它不能解决我们
真正的问题。我们需要验收的是：

```text
当策略表现异常时，harness 能否定位根因？
当策略家族变复杂时，harness 能否保持对比有效？
当实盘和回测不一致时，harness 能否分类并推进校准？
当旧 edge 失效时，harness 能否帮助产生新策略想法？
当多个 agent/provider 并行工作时，harness 能否保留责任、证据和决策链？
```

因此验收必须覆盖策略矩阵、执行层、Dashboard、回测、实盘、agent 协作和新策略
生成。

## 验收层级

| 层级 | 验收对象 | 通过标准 |
| --- | --- | --- |
| L0 文档和对象 | 场景、Agent Role、Agent Member、核心对象 | 新 agent 能按 docs 判断当前场景和所需证据 |
| L1 CLI/API | harness 命令和 artifact | 场景操作可通过确定性 CLI 产生 artifact |
| L2 产品 Dashboard | 市场和策略证据 | 能解释交易、no-fill、missed edge、live/backtest 差异 |
| L3 Agent Dashboard | 多 agent 协作审计 | 能追踪 role、member、subagent、claim、evidence、decision |
| L4 策略矩阵 | S1 family live/backtest | 同市场对比有效，slot 按 contract 运行 |
| L5 新策略生成 | 5m MM 等新 family | 能从 evidence 生成 thesis 并走完整流程 |

## L0：文档和对象验收

必须存在并互相链接：

- 策略开发场景流程；
- Multi-Agent 产品边界与拆分计划；
- Project Tool Adapter；
- Multi-Agent Harness 设计；
- Multi-Agent 生命周期与消息协议；
- Harness 验收模型；
- Agent Dashboard 设计；
- Strategy Harness Architecture；
- Project Adapter Example；
- Strategy Trial Lineage；
- Dashboard docs。

核心对象必须清楚：

- Thesis；
- StrategyFamily；
- StrategyNode；
- ExperimentCard；
- Matrix；
- Round；
- MarketPacket；
- OrderLifecyclePacket；
- AgentReport；
- LeadDecision；
- Lesson。

验收问题：

- 一个新 Codex session 是否能判断当前任务属于哪个场景？
- 是否能区分 strategy issue、execution issue、data issue、dashboard issue？
- 是否能区分 Agent Role、Agent Member 和 subagent？
- 是否能创建、暂停、恢复、退休和删除 Agent Member？
- 是否能解释产品 Dashboard 与 Agent Dashboard 的区别？
- 是否能解释 Multi-Agent 产品与项目工具环境的区别？
- 是否能判断一个文档或 skill 属于 generic core 还是 project adapter？

## L1：CLI/API 验收

每个高频场景都应该有确定性 CLI 或 API。最低验收集合：

```bash
node scripts/strategy-harness.mjs status --round-dir <round-dir>
node scripts/strategy-harness.mjs runs --root <rolling-root>
node scripts/strategy-harness.mjs build --round-dir <round-dir>
node scripts/strategy-harness.mjs fabric-status --round-dir <round-dir>
node scripts/strategy-harness.mjs calibration-plan --round-dir <round-dir>
node scripts/strategy-harness.mjs calibration-summary --round-dir <round-dir>
node scripts/strategy-harness.mjs link-trial-artifacts --round-dir <round-dir> --dry-run --check-trials
```

下一阶段需要新增或强化：

```bash
node scripts/strategy-harness.mjs scenario classify
node scripts/strategy-harness.mjs strategy-family show
node scripts/strategy-harness.mjs strategy-node validate
node scripts/strategy-harness.mjs gate-census
node scripts/strategy-harness.mjs order-lifecycle
node scripts/strategy-harness.mjs agent-report add
node scripts/strategy-harness.mjs claims audit
```

验收标准：

- CLI 输出 schema-shaped artifact；
- artifact 可被 Dashboard 或 Agent Dashboard 读取；
- 失败时返回 typed blocker，而不是只给自然语言；
- CLI 不依赖聊天上下文；
- destructive tests 不碰 live `strategy_trials` Postgres。

## L2：产品 Dashboard 验收

产品 Dashboard 验收的是市场和策略证据，不是 agent 协作。

必须能回答：

- 这个 market 上 Binance 是否先动？
- Polymarket executable YES/NO 或 UP/DOWN 是否 lag？
- 策略是否在 edge window 内 signal/submit/fill？
- no-fill 是流动性、延迟、队列竞争、min-size、执行层，还是 data gap？
- exit 是否过早、过晚、没 sellable，还是执行状态机问题？
- live 和 same-market backtest 的差异是什么？

必备页面能力：

- strategy family / strategy workspace；
- market detail with run overlay；
- same-market compare；
- comparison bundle；
- run gallery；
- trial lineage；
- order lifecycle drilldown。

注意：Agent Dashboard 不能替代这些市场图。它只能链接到产品 Dashboard 的证据。

## L3：Agent Dashboard 验收

Agent Dashboard 是多 agent 协作和审计面，必须与产品 Dashboard 分开。
Agent Member 生命周期、message-first task/report 和 materialization 细节见
[Multi-Agent 生命周期与消息协议](multi-agent-lifecycle.zh.md)，页面布局和 MVP
切片见 [Agent Dashboard 设计](agent-dashboard-design.zh.md)。

它必须能回答：

- 当前 harness run 属于哪个 scenario？
- 哪些 Agent Role 需要参与？
- 哪些 Agent Member 被创建、分配、暂停、退休或删除？
- 哪些 provider/subagent 执行了任务？
- 每个 report 引用了哪些 evidence？
- 哪些 claim 已支持、待验证、被反驳？
- 哪些 blocker 阻止 promotion？
- Lead decision 是基于哪些 report 和 evidence 做出的？

最低视图：

- Scenario timeline；
- Agent Role catalog；
- Agent Member roster；
- Message thread list；
- Message thread detail；
- Task graph；
- AgentReport index；
- Claim ledger；
- Blocker board；
- Decision timeline；
- Evidence links；
- Provider/subagent sessions；
- Handoff/heartbeat。

验收标准：

- 任意结论都能从 claim 回到 evidence；
- 任务递交、报告返回、追问和补证据能在同一个 `thread_id` 下追踪；
- `type=task` message 可以 materialize 为 AgentTask；
- `type=report` message 可以 materialize 为 AgentReport；
- 任意 subagent 工作都能归属到一个 Agent Member 或 task；
- Agent Member 删除不会删除历史 task、message、report、claim；
- stale report 不会被当作当前状态；
- agent 失败不会吞掉 blocker；
- Lead 可以看到下一步最小行动。

## L4：S1 策略矩阵验收

这是当前最直接的验收场景。

目标：证明 harness 能支撑 BTC 5m S1 strategy family 的同市场对比和 live/backtest
复盘。

候选矩阵：

- `sell-current`：baseline；
- `depth-early`：entry fork；
- `sell-current-presign-wide`：execution-latency fork；
- `depth-early-presign-wide`：entry + execution diagnostic；
- `depth-early-microprice005`：depth threshold diagnostic；
- GTD ladder 变体：exit-policy fork。

验收流程：

```text
StrategyNode validation
  -> fixed roster
  -> 1h canary
  -> completed-market packets
  -> same-market backtests
  -> order lifecycle packets
  -> visual review
  -> agent reports
  -> decision
  -> 8h diagnostic only if gates pass
```

必须证明：

- 每个 slot 使用预期参数；
- 每个 slot 使用预期 data source，例如 fabric；
- presign slot 有真实 always-warm prepared-submit 证据，否则不能证明 presign；
- gate census 能解释 signal 为什么被阻断；
- no-fill 被分类，而不是简单当失败；
- wallet/reconciliation 没有安全 blocker；
- live/backtest divergence 被分类；
- Dashboard 能显示每个策略的对应市场图和买卖点。

Promotion gate：

- 不能只看总 PnL；
- 不能用没有按 contract 运行的 slot 做 A/B 结论；
- 没有 screenshot / visual review 不能 promote；
- 有 unresolved order lifecycle bug 不能扩大资金。

## L5：新 5m Market Making 策略验收

这是验证 harness 是否能生成新策略想法的场景。

动机：如果 PM-lag taker capture 被竞争者压缩，系统应该能从 evidence 推导新的
策略 family，例如 5m market making。

MM thesis 示例：

```text
Polymarket 5m 市场在某些阶段 spread / depth / taker flow 提供 passive quoting
机会。策略不再只抢 Binance lead 的短窗口，而是通过报价、库存、取消和 settlement
管理获取 edge。
```

新增特殊证据：

- maker queue / order resting time；
- adverse selection；
- quote refresh/cancel latency；
- inventory exposure；
- complete-set merge/redeem；
- capital recycling；
- spread capture vs directional loss；
- maker order fill probability。

验收流程：

```text
EdgeHunt
  -> MM StrategyThesis
  -> Execution primitive validation for maker/GTD/cancel/merge
  -> backtest fill model design
  -> Dashboard maker evidence panel
  -> small live canary
  -> same-market replay calibration
  -> decision
```

Gate：

- 不能把 MM 作为 S1 参数变体；它是新 StrategyFamily。
- 没有 maker/cancel/queue 执行验证，不能 live。
- 没有 inventory 和 adverse-selection 指标，不能评价 MM。
- 回测 fill model 必须明确是模拟假设，不是实盘 oracle。

## Agent 体系验收

同一个事件应该能被多个 Agent Role 分工处理，并由具体 Agent Member 执行。

以 FAK/FOK late-fill 事件为例：

```text
Live Ops: 发现 round 出现 wallet mismatch
Execution: 还原订单生命周期，发现 cancelled 后 late fill
Backtest Parity: 判断这不是回测模型问题
Critic/Gate: 阻止继续 promotion
Lead: 决定修状态机并跑最小 rerun
Knowledge: 把模式写入 docs/schema/CLI backlog
```

验收标准：

- 每个参与 role 至少有一个 member 或明确 skip reason；
- 每个参与 member 有 report 或 handoff；
- report 引用具体 log/artifact；
- Lead decision 引用 report；
- 修复后有测试或 rerun 证据；
- 相同模式下次能被 CLI 或 Dashboard 更早发现。

## Claim 验收

Harness 中的 claim 必须分级。

| Claim 类型 | 示例 | 需要证据 |
| --- | --- | --- |
| operational | live 正在运行 | process/status artifact |
| data | 使用 fabric，不是 direct WS | logs + fabric-status |
| strategy | depth-early 更早进入 | same-market visual comparison |
| execution | presign 降低延迟 | presignHit + latency telemetry |
| safety | wallet flat | reconciliation |
| promotion | 可以扩大规模 | decision + no P0 blocker |

没有证据的 claim 只能进入 notes，不能进入 decision。

## 最小可接受版本

第一版不需要完整自动化，但必须做到：

- 七篇中文边界/adapter/流程/agent/生命周期/验收/dashboard 文档存在；
- S1 StrategyNode 的关系能被 docs 清楚解释；
- harness build 能产出 market packet、agent report、decision；
- live status 能判断 running/post-live/stuck；
- fabric status 能区分 fabric consumer 和 direct WS；
- dashboard 能打开 strategy、market、compare、postmortem；
- agent report 能记录 role、provider、evidence、claim、blocker；
- Lead 能基于这些 artifact 做下一步。

## 后续迭代

建议优先级：

1. `StrategyNode` schema。
2. `OrderLifecyclePacket` schema 和 CLI。
3. `gate-census` CLI。
4. AgentReport provider/session 字段。
5. Agent Dashboard 第一版。
6. S1 matrix 1h -> 8h diagnostic 验收。
7. 5m MM StrategyFamily 验收。
8. skill 精简高频操作。
9. plugin 封装稳定命令和视图。

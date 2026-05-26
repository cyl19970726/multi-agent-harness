# Multi-Agent 产品边界与拆分计划

本文定义 Multi-Agent 产品与业务项目工具系统之间的关系。核心结论：两者是
“使用者”和“工具环境”的关系，不是强耦合的内部模块关系。

## 两个层面

### 1. Multi-Agent 产品

Multi-Agent 产品本身是一个通用协作系统。它负责：

- 创建、暂停、恢复、退休和删除 Agent Member；
- 维护 Agent Role、Agent Member、Subagent / Provider Session；
- 通过 Message 提交任务、追问、补证据和返回报告；
- 从 Message materialize Task、Report、Claim、Blocker、Decision；
- 管理权限、审计、上下文和证据引用；
- 在 Agent Dashboard 展示协作状态和决策链。

它不应该内嵌 Polymarket、策略、回测、实盘、钱包或某个项目 Dashboard 的业务
逻辑。

### 2. 工具环境

业务项目通过 adapter 提供工具环境。一个项目可以提供：

- 策略、回测、实盘和执行层；
- 产品 Dashboard；
- CLI；
- artifact；
- Trial DAG；
- Worker/fabric；
- 钱包、订单和 reconciliation 工具。

Multi-Agent 产品通过 Skill、Prompt、Message 和 Tool Contract 学会使用这些工具。
Agent 可以被分配任务，然后调用项目 CLI、打开 Dashboard、读取 artifact，最后
返回结构化 report。

关系应该是：

```text
Multi-Agent Product
  -> Skills / Prompts / Messages / Tool Contracts
  -> Project CLI / Dashboard / Artifacts / Runtime Tools
  -> AgentReport / Claim / Blocker / Decision
```

而不是：

```text
业务项目内部硬编码一个 agent 系统
```

## 通用内核

以下内容属于可独立拆出的 generic agent-harness kernel：

- `AgentRole`
- `AgentMember`
- `ProviderSession`
- `SubagentSession`
- `AgentMessage`
- `AgentTask`
- `AgentReport`
- `Claim`
- `Blocker`
- `Decision`
- `PermissionGrant`
- `EvidenceRef`
- Skill registry
- Provider adapter 接口：Codex、Claude Code、Hermes-agent、script、human
- file-backed / DB-backed store
- Agent Dashboard
- agent-harness CLI/API

这些对象不能依赖任何业务项目的目录结构。

## Project Adapter

以下内容属于项目 adapter：

- 如何调用 `node scripts/strategy-harness.mjs status/runs/build/...`；
- 如何调用 backtest/live/trial CLI；
- 如何读取 live round、completed-review、screenshot、execution log；
- 如何拼产品 Dashboard URL；
- 如何解释 Trial DAG；
- 如何解释项目业务对象、权限和结果；
- 哪些操作需要真实资金或下单权限；
- 当前 S1、5m MM 等策略研究场景的验收规则。

Adapter 可以很厚，但它应该是工具说明和证据解释层，不应该污染 generic core。

## Skill 的位置

Skill 是 agent 使用工具的说明书。它可以分两类：

| 类型 | 内容 | 是否可迁出 |
| --- | --- | --- |
| Generic harness skill | 如何创建 member、发 message、materialize report、处理 claim/blocker | 应迁出 |
| Project adapter skill | 如何使用项目 CLI、Dashboard、artifact、权限工具 | 留在项目 repo，或作为 adapter 包 |

因此 docs/skills 本身耦合很低。我们可以先把通用部分整理成独立项目，再把具体
业务项目作为 adapter 接入。

## 目录归属规则

当前 repo 内的归属规则：

| 内容 | 当前路径 | 未来归属 |
| --- | --- | --- |
| Multi-Agent 产品对象和生命周期 | `docs/agents/multi-agent-*.zh.md` | generic repo |
| Agent Dashboard 通用协作界面 | `docs/agents/agent-dashboard-design.zh.md` | generic repo |
| 项目 agent system 目标 | project repo docs | project adapter |
| 项目工具使用说明 | `examples/adapters/<project>/` 或 project repo docs | project adapter |
| 项目业务模块 docs | project repo docs | project repo |
| 项目 task skill | project repo skills | project repo，稳定后可提取通用部分 |

## 拆分阶段

### Phase 0：当前 repo 内形成边界

目标：

- 通用 docs 不再把任何业务项目当成内部实现；
- 项目特定能力只通过 adapter doc / skill 暴露；
- `docs/agents` 只保留 agent workflow、multi-agent 产品、adapter 说明；
- 非 agent 架构迁到 `docs/modules/**` 或 `docs/overview/**`。

通过标准：

- 新 agent 能说清楚 Multi-Agent 产品与项目工具环境的区别；
- 能判断一个新文档属于 generic core 还是 project adapter；
- 旧路径没有断链。

### Phase 1：抽出独立 docs/skills 包

目标仓库可以是：

```text
multi-agent-harness/
  docs/
  skills/
  schemas/
  examples/
```

先迁：

- Multi-Agent 产品边界；
- Role/Member/Subagent 设计；
- Message-first lifecycle；
- Agent Dashboard 设计；
- 通用验收；
- generic skill。

不迁：

- 特定业务订单/交易/运行细节；
- 特定项目 DAG；
- 特定项目策略流程；
- 特定项目 CLI 命令细节。

### Phase 2：实现 generic runtime

实现：

- append-only message store；
- member registry；
- materializer；
- permission gate；
- provider session registry；
- Agent Dashboard read API；
- provider adapter 接口。

这时业务项目只提供 adapter。

### Phase 3：Project Adapter

Adapter 提供：

- tool catalog；
- artifact reader；
- Dashboard deep-link builder；
- project permission policy；
- strategy research acceptance pack。

### Phase 4：plugin/package

当 CLI/API 和 schema 稳定后，再把 generic harness 做成 plugin 或 package。

## 拆分判断

一个文件如果不需要知道具体业务项目、业务 DAG、运行模式、钱包/权限、产品
Dashboard 页面，就应该进入 generic harness。

一个文件如果需要解释具体策略、订单、钱包、回测、实盘、Dashboard 图或项目 CLI，
就应该留在 Earning Engine adapter。

## 当前结论

有必要拆项目，但正确顺序是：

```text
先边界化
  -> 抽 docs/skills
  -> 固化 schemas
  -> 实现 file-backed CLI
  -> 做 Agent Dashboard
  -> 再拆 runtime repo / plugin
```

不要先为了“拆仓库”搬代码。先让核心对象和 adapter contract 稳定。

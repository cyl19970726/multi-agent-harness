---
name: bootstrap-project-workflow
description: "Use when Codex needs to bootstrap, audit, or redesign a project's docs, CI/CD, task system, agent workflow, skills, tool adapters, or evidence-backed acceptance process for a new project, new requirement, or project migration. Applies to generic projects and to Multi-Agent Harness itself; keep domain tools behind adapters."
---

# Bootstrap Project Workflow

## Overview

用这个 skill 把一个项目从“只有代码或想法”推进到“Agent 能可靠使用和改进它”。输出必须同时覆盖人类可读的文档、机器可验证的契约、CI/CD 校验、Agent 工作流和验收标准。

核心原则：

- 文档回答“为什么、是什么、怎么做、怎么判断好坏”。
- CI/CD 回答“这些承诺现在是否仍然成立”。
- Skill 教 Agent 怎么用项目工具；schema/CLI/API 让这个使用过程稳定。
- Multi-Agent Harness 是协调层；项目业务能力通过 adapter 暴露，不写进 generic core。

## What Good Docs Express

高明的文档不是资料库，也不是命令合集。它表达的是作者对项目整体体验和系统结构的把握，让新的人或 Agent 能迅速形成正确心智模型。

它首先要表达这些东西：

| 核心表达 | 含义 |
| --- | --- |
| 动机 | 为什么这个项目值得存在，解决什么真实问题 |
| 体验 | 使用者应该怎样完成关键任务，哪里应该快、稳、可观察 |
| 边界 | 系统负责什么，不负责什么，外部依赖和 adapter 在哪里 |
| 模块文化 | 每个重要模块的责任、价值观、不可破坏的约束 |
| 模块关系 | 哪些模块调用谁，数据流、控制流、证据流如何经过系统 |
| 递进层次 | 从整体到模块，再到子模块和内部细节，逐层深入 |
| 判断标准 | 什么叫做做得好，什么情况必须阻断、回滚或重新设计 |
| 演进方向 | 哪些部分只是临时约定，哪些应升级为 schema、CLI、API、Dashboard 或 plugin |

“模块文化”是文档里很重要但容易漏掉的部分。它不是口号，而是模块的设计性格：

```text
Module Culture
  purpose: 这个模块存在的理由
  owns: 它拥有的对象和责任
  refuses: 它明确不做什么
  invariants: 不能破坏的约束
  inputs_outputs: 它和其他模块怎么交换数据
  failure_modes: 它失败时系统如何发现和处理
  evidence: 怎么证明它正常工作
```

好的文档应该让读者先看到整栋建筑，再进入楼层，最后进入房间。不要一开始就堆内部字段、历史讨论或零散命令。

## Core Content

一个项目的核心文档内容应该围绕五条主线组织：

1. Product Story：用户、Agent 或系统为什么需要它，核心体验是什么。
2. System Model：核心对象、模块、边界、依赖和不变量是什么。
3. Operational Path：如何启动、验证、排障、发布、回滚。
4. Evidence Path：如何知道一个结果是真的、一次任务完成得好不好。
5. Evolution Path：重复流程如何从 docs 进入 skill，再进入 schema/CLI/API/Dashboard/plugin。

如果一段文档不能服务这五条主线之一，它通常应该被删除、迁移到代码注释、迁移到 schema/CLI help，或放进临时 task notes。

## First Step

先判断当前任务属于哪一种：

| 场景 | 目标 |
| --- | --- |
| 新项目启动 | 建立最小 docs、CI/CD、task/message/evidence 工作流 |
| 新需求进入老项目 | 补齐这个需求需要的 PRD、架构、验收、检查命令 |
| 项目迁移或产品化 | 分离 generic harness 和项目 adapter，定义发布边界 |
| 文档/CI 审计 | 找出过期文档、缺失检查、代码和协议漂移 |
| Agent 工作流接入 | 为项目设计 skills、tool descriptors、tasks、evidence policy |

读取最小上下文：

- `README*`, `AGENTS.md`, `docs/`, `skills/`
- `.github/workflows/`, `package.json`, `Cargo.toml`, `pyproject.toml`, `Makefile`, `scripts/`
- schema/API/CLI/dashboard 入口
- 用户最新目标、风险边界、验收要求

如果上下文不足，优先做合理假设；只有目标受众、风险等级、发布目标或权限边界不可推断时才提问。

## Required Questions

每次设计文档和 CI/CD 都要回答这些通用问题：

| 问题 | 为什么要回答 |
| --- | --- |
| 这个项目/需求的动机是什么？ | 防止文档和工具变成无目标的模板堆叠 |
| 主要场景有哪些？ | 不同场景需要不同 workflow、证据和验收 |
| 谁在使用它？ | 人类、Agent、CI、Dashboard 需要不同粒度的信息 |
| Agent 最短可靠路径是什么？ | 决定需要哪些 CLI、adapter、skill、dashboard 链接 |
| 怎么知道任务做得好或坏？ | 决定 evidence、acceptance criteria 和 CI gates |
| 哪些东西是稳定协议？ | 稳定内容进入 schema/CLI/API，不稳定内容留在 docs |
| 哪些操作有风险？ | 决定权限、审批、dry-run、回滚和审计要求 |
| 哪些文档会被机器读取？ | 这些文档必须更结构化，并纳入一致性检查 |
| 什么情况下拆分文档？ | 控制文档膨胀，降低 Agent 上下文成本 |
| 什么情况下发布？ | CD 必须有版本、契约和回滚边界 |

## Workflow

### 1. Map Scenarios

先写场景，而不是先写目录。

每个场景最少包含：

```text
Scenario
  trigger
  actor / agent_member
  goal
  tools_or_adapters
  evidence
  dashboard_view?
  acceptance_criteria
  failure_modes
```

区分两类场景：

- 技术开发：实现、测试、评审、发布。
- 业务/研究使用：使用项目能力产生结果，例如策略回测、数据分析、报告生成。

如果是 Multi-Agent 项目，还要补充：

- Leader 如何拆 task。
- Agent member 如何通过 message 接任务和回报告。
- Evidence 如何被引用。
- Decision 由谁做。

### 2. Choose Minimal Docs

默认从这组最小文档开始：

| 文档 | 内容 |
| --- | --- |
| `README.md` | 项目一句话、边界、入口命令、文档索引 |
| `docs/prd.md` | 动机、场景、非目标、成功标准 |
| `docs/architecture.md` | 核心模块、数据流、对象、边界 |
| `docs/operations.md` | 本地运行、CI/CD、排障、权限、安全操作 |
| `docs/schemas.md` | schema/API/CLI/tool descriptor 索引 |
| `docs/decisions.md` | 关键架构决策和取舍 |

复杂系统必须配图。优先使用 Mermaid，因为它可 diff、可审查、可被 CI 检查；只有在 UI 视觉、云拓扑、部署截图、白板推演更合适时才使用 bitmap 或外部图。

最小图集：

| 图 | 放在哪里 | 回答什么问题 |
| --- | --- | --- |
| System Context | `README.md` 或 `docs/prd.md` | 系统和用户、外部系统、项目 adapter 的边界 |
| Architecture Diagram | `docs/architecture.md` | 模块如何组成，谁依赖谁 |
| Data Flow Diagram | `docs/architecture.md` | 数据、artifact、evidence 从哪里来，到哪里去 |
| Workflow Diagram | `docs/prd.md` 或 `docs/operations.md` | 一个真实场景从触发到验收怎么走 |
| Sequence Diagram | `docs/architecture.md` | 多 Agent、CLI、API、Dashboard 的时序交互 |
| State/Lifecycle Diagram | `docs/architecture.md` | Task、Message、AgentMember、Decision 等对象状态如何变化 |
| Deployment/Runtime Diagram | `docs/operations.md` | 本地、CI、生产、worker、dashboard 如何运行 |

每张图必须有一句图注，说明它回答的问题。图不要替代正文；图负责结构理解，正文负责约束、例外和验收。

不要一开始创建很深的目录。只有满足以下条件之一才拆分：

- 单文档稳定超过约 500 行。
- 读者明显不同，比如用户文档和维护者文档。
- 生命周期明显不同，比如 PRD 稳定但 runbook 高频变化。
- 文件需要被机器独立读取或校验。
- 内容已经稳定到可以变成 schema、CLI help、API spec 或 dashboard contract。

拆分前先问：

| 问题 | 如果答案是 yes |
| --- | --- |
| 这个内容是否有独立读者？ | 可以拆分 |
| 这个内容是否有独立更新频率？ | 可以拆分 |
| 这个内容是否会被 CI、CLI、Dashboard 或 Agent 单独读取？ | 应该拆分 |
| 这个内容是否已经稳定成协议？ | 应该迁移到 schema/API/CLI spec，而不只是拆文档 |
| 这个内容是否只是当前文档太乱？ | 先重排和删重复，不要急着拆 |
| 拆出去以后是否需要新的索引和 owner？ | 如果没有 owner，暂时不要拆 |

新增目录的标准比新增文档更高。只有出现同类文件至少 3 个、或有明确机器消费边界时才新增目录。

推荐目录演进：

```text
docs/
  prd.md
  architecture.md
  operations.md
  schemas.md
  decisions.md
```

需要时再演进：

```text
docs/
  scenarios/        # 多个稳定场景，每个都有不同 workflow 和验收
  runbooks/         # 高频运维/排障，生命周期和 architecture 不同
  adapters/         # 多个项目 adapter 的接入说明
  adr/              # decisions.md 太长，或需要单独 ADR 编号
  diagrams/         # 图源文件不是内嵌 Mermaid，或需要被工具渲染
```

不要新增这些内容，除非有真实消费方：

- 空的 `guides/`、`reference/`、`concepts/`。
- 只有一个文件的深层目录。
- 为了“看起来完整”创建的模板文档。
- 和 README 重复的 quickstart。
- 没有 CI、schema 或 owner 的长期规范目录。

拆分后必须补三件事：

1. 上级索引链接到新文档。
2. 新文档开头写明读者、场景、更新条件。
3. CI 至少能检查链接和基础格式；机器消费文档要有更强校验。

### 3. Define Machine Contracts

把稳定协议变成机器可读对象。最小集合通常是：

```text
Task
Message
Evidence
Decision
ToolDescriptor
PermissionPolicy
```

不要太早设计复杂 DSL。优先支持：

- JSON schema
- CLI 输出 JSON
- append-only artifact
- dashboard URL / artifact ref

经验顺序：

```text
docs -> skill -> schema -> CLI/API -> dashboard -> plugin
```

只有当 schema 和命令稳定后，才考虑 plugin。

### 4. Design CI/CD

先按成熟度选择检查集合。

| 阶段 | 最小 CI |
| --- | --- |
| 文档种子期 | markdown 链接、JSON 合法性、skill frontmatter |
| 协议期 | schema 校验、示例 artifact 校验、CLI JSON smoke test |
| 代码期 | fmt、lint、typecheck、unit test、build |
| dashboard 期 | build、关键页面 smoke test、截图或 layout 检查 |
| 发布期 | version check、changelog/release notes、artifact publish dry-run |

CI 要验证承诺，不要只跑工具。常见承诺包括：

- 文档链接存在。
- docs 提到的 schema/CLI 文件存在。
- 示例 JSON 符合 schema。
- 代码里的类型和 schema 没有明显漂移。
- CLI 示例能返回结构化输出。
- dashboard 能构建并打开关键页面。
- 高风险命令默认 dry-run 或需要显式权限。

CD 只在边界清楚后启用：

- crate/package/container/docs site 分别版本化。
- schema 破坏性变化必须有迁移说明。
- 发布前跑同一套 CI，加 release smoke test。

### 5. Create Agent Workflow

把项目能力暴露给 Agent，而不是让 Agent 猜。

最小结构：

```text
User Request
  -> Leader creates/updates Task
  -> Leader sends Message kind=task
  -> Agent member uses Skill + Tool Adapter
  -> Agent returns Message kind=report with Evidence refs
  -> Reviewer/Critic checks evidence
  -> Leader records Decision
```

对每个 agent member 定义：

- role
- capabilities
- allowed tools
- forbidden actions
- expected evidence
- acceptance responsibility

对每个 tool adapter 定义：

- command/API/dashboard entry
- inputs
- outputs
- evidence shape
- permission level
- failure modes

### 6. Implement Smallest Durable Change

落地顺序：

1. 更新或创建最小 docs。
2. 新增必要 schema 或 tool descriptor。
3. 新增/更新 skill。
4. 新增 CI check，优先检查刚创建的契约。
5. 运行本地验证。
6. 记录剩余 gap，不把未验证假设写成事实。

## Evaluation

评价文档和 CI/CD 时，不要只说“清楚/不清楚”。要把它们当成项目治理系统来审计：它们是否让人和 Agent 更快、更准、更安全地完成任务。

### Documentation Rubric

每项按 `0-3` 打分：`0` 缺失，`1` 有但不可执行，`2` 基本可用，`3` 可验证且低摩擦。

| 维度 | 好的表现 | 坏的表现 |
| --- | --- | --- |
| 动机和边界 | 说明为什么做、服务谁、不做什么 | 只有目录和口号，看不出产品判断 |
| 场景覆盖 | 主要场景能从 trigger 走到 acceptance | 只有模块介绍，没有真实使用流程 |
| 可执行性 | 新 agent 能按文档找到命令、入口、证据 | 需要翻聊天记录或猜脚本 |
| 准确性 | 命令、路径、schema、状态和代码一致 | stale 命令、断链、旧架构图、重复矛盾 |
| 证据和验收 | 每个重要 claim 有 evidence 或验收方法 | 说“已完成/合理”，但没有验证来源 |
| 信息分层 | README/PRD/Architecture/Operations 各司其职 | 所有内容混在一个长文档里 |
| 上下文成本 | Agent 读取少量文档就能开始工作 | 必须读大量历史文档才能判断下一步 |
| 演进路径 | 知道哪些内容应升级为 schema/CLI/test | 文档长期承载重复手工流程 |
| 图示质量 | 架构图、数据流图、工作流图能快速解释系统 | 只有长文字，或图和代码/文档明显不一致 |

判断文档好坏的实际方法：

1. 做 scenario trace：从一个真实用户请求追踪到 task、tool、evidence、decision，卡住的位置就是文档缺口。
2. 做 claim inventory：列出文档里的关键事实，标注 source of truth；没有来源的 claim 是风险。
3. 做 command audit：抽取文档里的命令、路径、schema、URL，确认真实存在且能运行或被解释。
4. 做 fresh-agent test：让未参与上下文的 agent 只凭文档完成一个小任务；它问的基础问题就是文档坏点。
5. 做 drift scan：对比最近代码/CLI/schema/dashboard 变化，找没有同步更新的文档。
6. 做 reader split：如果同一文档同时给用户、维护者、CI、Agent 使用且互相干扰，考虑拆分。
7. 做 diagram review：用图解释系统边界、模块、数据流和任务流；解释不出来的部分要补图或改正文。

### Diagram Rubric

每张关键图按 `0-3` 打分：

| 维度 | 好的表现 | 坏的表现 |
| --- | --- | --- |
| 目的 | 图注明确说明回答的问题 | 图只是装饰，读者不知道该看什么 |
| 边界 | 清楚标出系统、adapter、外部依赖、权限边界 | 内外部系统混在一起 |
| 流向 | 数据、控制、任务或消息方向明确 | 箭头随意，无法判断因果和顺序 |
| 粒度 | 一张图只解释一个层级 | 把所有模块、字段、时序塞进一张图 |
| 一致性 | 节点名和 docs/schema/code/CLI 一致 | 图里的名称和实现不一致 |
| 可维护性 | Mermaid 或可版本化源文件可审查 | 只有不可编辑截图，无法 diff |

图的选择规则：

- 用 `flowchart` 表达架构、数据流、工作流。
- 用 `sequenceDiagram` 表达 agent、CLI、API、dashboard 的交互顺序。
- 用 `stateDiagram-v2` 表达任务、订单、agent 生命周期。
- 用 `C4` 风格分层表达复杂系统边界，但不要引入重 DSL，除非项目已经使用。
- 超过约 12 个节点就考虑拆成多张图。
- 图和正文冲突时，以 schema/code/CLI 为准，并修正文档。

### CI/CD and Governance Rubric

每项按 `0-3` 打分：

| 维度 | 好的表现 | 坏的表现 |
| --- | --- | --- |
| 承诺覆盖 | CI 对应 PRD/架构/schema/CLI 的真实承诺 | 只跑格式化，不能证明系统可用 |
| 漂移防护 | docs/schema/code/CLI/dashboard 有一致性检查 | 文档和实现各自演化 |
| 风险分级 | 高风险操作有 permission、dry-run、审计 | live、删除、发布、密钥操作无闸门 |
| 反馈质量 | 失败信息能指向 owner、文件、修复路径 | 红叉但不知道该改什么 |
| 速度和稳定性 | PR checks 快且稳定，重检查放 nightly/release | 慢、flaky，导致大家绕过 CI |
| 契约测试 | schema fixtures、CLI JSON、API/dashboard smoke 可验证 | 只有单元测试，没有跨层契约 |
| 图示验证 | Mermaid 可渲染，关键图和文件名/节点约定可检查 | 图长期坏掉但 CI 不知道 |
| 发布纪律 | version、migration、release smoke 明确 | schema 破坏性变化无说明就发布 |
| 治理闭环 | 重复事故会转成新的 gate 或 runbook | 同类问题反复靠人工记忆处理 |

判断 CI/CD 好坏的实际方法：

1. 建立 commitment matrix：每条项目承诺对应一个检查、owner、失败后果。
2. 区分 `blocker` 和 `warning`：未验证假设先 warning，稳定且高风险的承诺才 blocker。
3. 从真实事故倒推：过去三次故障是否会被现有 CI/CD 提前抓住；抓不住就是治理缺口。
4. 检查 release path：从 merge 到 publish 的每一步是否有版本、artifact、回滚和 smoke test。
5. 检查 fixture path：schema、CLI、API、Dashboard 是否共享同一批 valid/invalid fixture。
6. 检查权限 path：生产、金钱、删除、secret、外部发布是否默认不可静默执行。
7. 检查 diagram path：Mermaid 是否能渲染；关键图是否覆盖系统边界、数据流、工作流和生命周期。

### Evaluation Output

审计或新建体系时，用这个输出格式：

```text
Evaluation
  status: pass | warn | block
  docs_score: <0-24>
  cicd_score: <0-24>
  best_parts:
    - <what is working and why>
  weak_parts:
    - <gap, evidence, consequence>
  diagram_gaps:
    - <missing or stale diagram, consequence>
  missing_contracts:
    - <doc claim that should become schema/CLI/test>
  missing_checks:
    - <commitment that CI/CD does not verify>
  next_changes:
    - <smallest durable change>
```

不要把高分当目标本身。目标是让下一次真实任务更少猜测、更少上下文、更少人工复盘，并且失败时能更早暴露。

## Subagent Use

当用户要求多 Agent，或任务涉及全局 workflow/CI/docs 设计时，使用独立 subagent 做并行评审。

推荐分工：

| Agent | 任务 |
| --- | --- |
| Architect | 判断文档结构、模块边界、场景流 |
| CI/CD Reviewer | 判断检查是否覆盖承诺、防漂移是否足够 |
| Critic | 找过度设计、不可执行、耦合业务项目的问题 |
| Domain Adapter Reviewer | 判断项目工具是否通过 adapter 暴露清楚 |

给 subagent 原始 artifact 和问题，不要泄露预期答案。主 agent 负责最终整合和取舍。

## Guardrails

- 不要把项目业务逻辑写进 generic harness。
- 不要让 docs 代替 schema/CLI/test；重复执行的东西要逐步产品化。
- 不要把 dashboard 文案当证据；dashboard 应链接到 artifact、log、test、schema 或人工 review。
- 不要让 agent dashboard 和项目 dashboard 混在一起。
- 不要为猜测创建 CI gate；先用 warning 或 docs gap 记录。
- 不要创建空目录、占位文档、无读者文档。
- 不要让 message 无限泛化；task/report 可以是 message，但完成后要能 materialize 成 Task/Evidence/Decision。

## Completion Checklist

完成时汇报：

- 新增或修改了哪些 docs、skills、schemas、CI checks。
- 每个主要场景如何从 request 走到 evidence-backed decision。
- 哪些承诺已经进入 CI/CD。
- 哪些内容仍只是 docs 假设，还不能作为 gate。
- 本地跑过哪些验证命令。

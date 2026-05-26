# Multi-Agent 生命周期与消息协议

本文细化 Multi-Agent Harness 的 Agent Role、Agent Member、Subagent、Message、
Task、Report、Claim 和 Dashboard 关系。它补充
[Multi-Agent Harness 设计](multi-agent-harness-design.zh.md)，用于指导后续
schema、CLI/API 和 Agent Dashboard 实现。

## 设计动机

我们希望多 agent 系统不是一组并行聊天窗口，而是一个可审计的研究协作系统。
核心目标是：

```text
Lead 可以把任务递交给某个 Agent Member。
Agent Member 可以回报结果。
其他 Agent Member 可以追问、质疑、补证据。
整个任务上下文保留在同一个 message thread。
最终 Task、Report、Claim、Blocker、Decision 都能从消息流 materialize 出来。
```

这比先设计复杂 task/report API 更灵活。第一版可以让所有协作都走
`AgentMessage`，再由 CLI/Dashboard 在需要时把消息线程提炼成结构化对象。

## 核心对象

### AgentRole

`AgentRole` 是职责模板，不是运行实例。

```text
AgentRole
  role_id
  label
  responsibilities
  allowed_scenarios
  required_inputs
  expected_outputs
  capabilities_required
  permissions_required
  acceptance_checks
```

示例 role：

- `lead`
- `scenario_router`
- `strategy_edgehunt`
- `execution`
- `live_ops`
- `data_fabric`
- `backtest_parity`
- `market_review`
- `critic_gate`
- `dashboard_product`
- `agent_dashboard`
- `knowledge`

### AgentMember

`AgentMember` 是实际注册/常驻的 agent 实例，等价于用户语义里的 agent member。

```text
AgentMember
  member_id
  role_id
  provider
  provider_member_ref?
  status
  capabilities
  permissions
  current_task_ids
  created_at
  heartbeat_at
  retired_at?
  deleted_at?
```

示例：

```text
execution-codex-01
  role_id: execution
  provider: codex
  permissions: read_repo, run_cli, edit_code, small_order_probe_requires_lead

critic-hermes-01
  role_id: critic_gate
  provider: hermes_agent
  permissions: read_artifacts, send_messages, write_reports
```

### Subagent

`Subagent` 是 provider 内部的临时执行单元，不是常驻成员。

```text
Subagent
  provider
  provider_session_id
  parent_member_id
  task_id?
  message_thread_id?
  status
```

Codex explorer/worker 属于 subagent。它的输出只有在被 Agent Member 写入
message、report 或 artifact 后，才进入 harness 审计链。

### AgentMessage

`AgentMessage` 是统一通信总线。任务递交、补充要求、追问、报告返回都可以通过
message 完成。

```text
AgentMessage
  message_id
  type: message | task | report
  from_member_id
  to_member_id?
  channel?
  task_id?
  scenario?
  thread_id
  reply_to_message_id?
  body
  payload?
  evidence_refs
  claim_refs
  created_at
```

`type` 只表达最小通信意图：

- `message`：自由沟通、追问、补充上下文。
- `task`：提交任务或补充任务要求。
- `report`：返回任务报告、摘要或 report artifact 引用。

不要把复杂 workflow 状态塞进 message type。blocker、claim、decision 应从消息
内容和报告中 materialize 成独立对象。

### AgentTask

`AgentTask` 可以从 `type=task` 的 message materialize 出来。

```text
AgentTask
  task_id
  scenario
  created_from_message_id
  thread_id
  assigned_role_id
  assigned_member_id?
  objective
  required_inputs
  required_outputs
  status
  due_at?
  created_at
  completed_at?
```

第一版不要求所有 task 都预先存在。一个 `type=task` message 可以先启动协作，
Agent Dashboard 再把它显示为 task。

### AgentReport

`AgentReport` 可以从 `type=report` 的 message materialize 出来。

```text
AgentReport
  report_id
  created_from_message_id
  thread_id
  task_id?
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

一个报告可以先作为 message 返回。如果后续需要参与 gate、decision 或长期复盘，
再保存为 report artifact。

### Claim / Blocker / Decision

`Claim`、`Blocker`、`Decision` 不应该只是聊天内容。

```text
Claim
  claim_id
  source_message_id?
  source_report_id?
  text
  evidence_refs
  status: proposed | supported | challenged | rejected

Blocker
  blocker_id
  source_message_id?
  source_report_id?
  severity
  code
  message
  required_action

Decision
  decision_id
  lead_member_id
  source_thread_ids
  source_report_ids
  promotion_allowed
  diagnostic_sampling_allowed
  next_action
  rationale
```

规则：message 可以先表达 blocker 或 claim，但只有 materialize 后才能参与 gate。

## Agent Member 生命周期

```text
created
  -> idle
  -> assigned
  -> running
  -> waiting
  -> reporting
  -> completed
  -> idle
  -> retired
  -> deleted
```

异常状态：

- `blocked`
- `failed`
- `stale`
- `superseded`

状态语义：

- `created`：已注册，但未接任务。
- `idle`：可接任务。
- `assigned`：已有任务，但未开始或 provider 未确认。
- `running`：正在执行。
- `waiting`：等待其他 member、外部系统、用户或数据。
- `reporting`：正在整理 report。
- `completed`：当前 task 完成。
- `blocked`：无法继续，需要 Lead 或其他 member 处理。
- `failed`：provider 或执行过程失败。
- `stale`：heartbeat 过期，不能作为当前状态。
- `retired`：不再接新任务，但历史保留。
- `deleted`：注册记录删除或隐藏，但历史 task/message/report 不删除。

## 创建、删除与权限

### 创建

创建 Agent Member 时必须绑定：

- role；
- provider；
- capabilities；
- permissions；
- artifact root；
- optional budget；
- optional default channels。

示例：

```bash
node scripts/agent-harness.mjs member create \
  --role execution \
  --provider codex \
  --capability read_repo \
  --capability run_cli \
  --capability edit_code \
  --permission no_live_order_without_lead
```

### 暂停和恢复

暂停用于阻止接新任务，但保留当前上下文。

```bash
node scripts/agent-harness.mjs member pause execution-codex-01
node scripts/agent-harness.mjs member resume execution-codex-01
```

### 退休和删除

`retire` 是推荐的常规下线方式。`delete` 只删除注册状态，不删除历史。

```bash
node scripts/agent-harness.mjs member retire execution-codex-01
node scripts/agent-harness.mjs member delete execution-codex-01
```

删除规则：

- open task 存在时不能 hard delete；
- 必须保留 message、report、claim、decision；
- 涉及 live/wallet/order 权限的 member 删除必须记录 reason；
- provider session 可以关闭，但 harness artifact 保留。

## Message-First 任务流程

### 递交任务

Lead 或 Scenario Router 发送 `type=task`：

```text
from_member_id: lead-codex-01
to_member_id: execution-codex-01
type: task
scenario: execution_primitive_validation
thread_id: th-fak-late-fill-001
body: |
  请还原 run X 中 FAK/FOK SELL 的订单生命周期。
  重点检查 terminal cancel 后是否出现 late fill，以及是否触发重复退出。
evidence_refs:
  - artifacts/live/.../live-depth-early/*.log
```

Dashboard 可从这条 message materialize：

```text
AgentTask(task_id=task-fak-late-fill-001)
```

### 返回报告

Execution member 完成后发送 `type=report`：

```text
from_member_id: execution-codex-01
to_member_id: lead-codex-01
type: report
thread_id: th-fak-late-fill-001
body: |
  发现第一笔 FAK SELL 在 cancelled 后 11s 收到 Order filled，
  策略已释放 exitInFlight 并发出第二笔 SELL。
  建议增加 FAK/FOK terminal grace。
evidence_refs:
  - artifacts/live/.../live-depth-early.log:123
  - artifacts/live/.../live-depth-early.log:134
payload:
  report_path: artifacts/agent-reports/execution-fak-late-fill.json
```

如果这份报告需要长期进入 gate，Dashboard/CLI materialize：

```text
AgentReport(report_id=execution-fak-late-fill)
Claim(claim_id=late-fill-after-terminal-cancel)
Blocker(code=duplicate_exit_risk)
```

### 追问和补证据

Critic/Gate 可以在同一 thread 下追问：

```text
from_member_id: critic-codex-01
to_member_id: execution-codex-01
type: message
thread_id: th-fak-late-fill-001
reply_to_message_id: msg-report-001
body: |
  你需要说明这是执行状态机问题，而不是回测模型问题。
  请补充 live/backtest parity 的排除理由。
```

Execution 可以返回补充报告：

```text
type: report
thread_id: th-fak-late-fill-001
body: |
  该问题发生在 live order lifecycle：terminal status 后 late fill。
  回测模型没有真实 CLOB terminal/late user activity，因此不是回测填充模型能解释。
```

这保持了完整上下文，也允许多个 agent 灵活追问。

## 何时 Materialize

不是每条 message 都要变成 task/report/claim。materialization 规则：

| 对象 | 何时 materialize |
| --- | --- |
| AgentTask | `type=task` 需要状态跟踪、分配、超时或 Dashboard 展示时 |
| AgentReport | `type=report` 影响 gate、decision、docs、代码修复或复盘时 |
| Claim | 报告中出现需要被支持/挑战/引用的结论时 |
| Blocker | 结论会阻止 live、promotion、size increase、merge 或 release 时 |
| Decision | Lead 根据一个或多个 thread/report 做出下一步时 |

第一版可以手动 materialize。后续 CLI/API 可以自动从 message 生成对象。

## Channel 语义

Channel 是订阅和广播边界，不是权限边界。

| Channel | 用途 |
| --- | --- |
| `lead` | 决策请求、最终整合 |
| `scenario_router` | 场景分类和路由 |
| `execution` | 订单生命周期、wallet、CLOB primitive |
| `live_ops` | 进程、PM2、driver、supervisor、preflight |
| `data_fabric` | 数据新鲜度、fabric、consumer lag、replayability |
| `backtest_parity` | same-market replay、divergence、calibration |
| `market_review` | Dashboard 视觉复盘、截图、edge labels |
| `critic_gate` | unsupported claim、promotion blocker |
| `dashboard_product` | 产品 Dashboard 页面/API/证据面 |
| `agent_dashboard` | Agent Dashboard 和协作审计面 |
| `knowledge` | docs、schema、skill、archive |
| `broadcast` | 全员通知 |

如果发给 channel，Agent Dashboard 应显示哪些 member 收到了、谁响应了、谁没有
响应。

## Agent Dashboard 视图要求

Agent Dashboard 应围绕 message thread 展示协作。
完整布局、页面职责和 MVP 切片见 [Agent Dashboard 设计](agent-dashboard-design.zh.md)。

最低页面：

- thread list：按 scenario、status、member、blocker 过滤；
- thread detail：完整 message 流、task/report materialization、evidence links；
- member roster：role、provider、status、heartbeat、current tasks；
- task board：从 `type=task` materialize 的任务；
- report index：从 `type=report` materialize 的报告；
- claim ledger：从 report/message 提炼出的 claim；
- blocker board：阻止 live/promotion/scale 的 blocker；
- decision timeline：Lead 决策和引用的 thread/report。

Agent Dashboard 不画市场主图。市场图、replay、same-market compare、screenshot
都应该 deep-link 到产品 Dashboard。

## CLI 草案

第一版可以 file-backed，写入 `artifacts/agent-harness/<run-id>/`。

```bash
node scripts/agent-harness.mjs role list
node scripts/agent-harness.mjs member create --role execution --provider codex
node scripts/agent-harness.mjs member list
node scripts/agent-harness.mjs member heartbeat <member-id>
node scripts/agent-harness.mjs member pause <member-id>
node scripts/agent-harness.mjs member resume <member-id>
node scripts/agent-harness.mjs member retire <member-id>
node scripts/agent-harness.mjs member delete <member-id>

node scripts/agent-harness.mjs message send \
  --from lead-codex-01 \
  --to execution-codex-01 \
  --type task \
  --thread th-fak-late-fill-001 \
  --body-file task.md \
  --evidence-ref artifacts/live/.../live.log

node scripts/agent-harness.mjs message list --thread th-fak-late-fill-001
node scripts/agent-harness.mjs task materialize --message msg-001
node scripts/agent-harness.mjs report materialize --message msg-002
node scripts/agent-harness.mjs claim materialize --report execution-fak-late-fill
```

后续可以把这些合入 API：

- `POST /api/agent-harness/members`
- `POST /api/agent-harness/messages`
- `GET /api/agent-harness/threads/:threadId`
- `POST /api/agent-harness/materialize/task`
- `POST /api/agent-harness/materialize/report`

## 安全和权限

Agent message 和 report 不能包含：

- 助记词；
- 私钥；
- wallet encryption key；
- CLOB secret；
- 未脱敏的 credential；
- 会直接扩大资金或下单的隐式授权。

涉及真实下单、钱包、live launch、资金扩大、merge/redeem 的任务必须带权限说明，
并且由 Lead 决策或显式授权。

## 第一版实现边界

第一版不需要完整数据库。

推荐边界：

- roles 静态定义在 repo 文档或 JSON；
- members 存 file-backed registry；
- messages append-only JSONL；
- materialized tasks/reports/claims/blockers/decisions 写 JSON；
- Agent Dashboard 读取这些文件或 API；
- provider session 只作为引用，不作为事实来源。

信任边界：

```text
provider chat < message < materialized report < claim/blocker < lead decision
```

只有 materialized artifact 能进入 promotion gate。

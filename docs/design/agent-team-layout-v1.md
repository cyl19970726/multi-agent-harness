# Agent Team 页面 Layout 设计方案 v1（实施蓝图）

## 0. 依据、关键判断与遗留确认

**三个待确认判断（已在方案中定案）**
1. 取消现有 Overview/Activity/Messages/Wave 四个 Tab，改为单页分区（模块嵌套 vs Tab 平级隐藏面板矛盾；观看优先要求首屏一屏看完当前波）。
2. 拍板项"已处理"判定复用 ACK（approval 消息的 host delivery 被 ACK 即离开 needs-you），不新增 resolved 字段——依赖后端补充①。
3. 删除现有 member 横排卡片条（与 cockpit 表信息重复），成员区只保留 cockpit 表。

## 1. 信息架构总图

| 层级 | 地址 | 区域 | 用户看什么 | 用户做什么 |
|---|---|---|---|---|
| L0 列表 | `?surface=team` | Team Runs 收件箱 | 所有 wave（按 lineage 缩进）、每个 run 的状态/成员/needs-you/最后活动；needs-you 大的排前 | 打开一个波；创建 run |
| L1 详情 | `?team=<runId>` | 单波页面（本方案核心） | 当前波进行到哪、谁需要我拍板、每个成员在干什么 | 处理拍板项、发消息、过集成门、开下一波 |
| L1.5 消息深链 | `?team=<id>&msg=<messageId>` | 详情页 + 定位高亮某条消息 | 从 needs-you 直达具体消息 | 就地 Decide / ACK |
| L2 成员抽屉 | 无 URL（overlay） | Member drawer | 单成员的动作时间线/委派/消息/Raw | 只读钻取（v0 无成员级操作） |

导航惯例与 Workflows 页完全一致（`?workflowRun=` ↔ `?team=`；列表行点击进详情；back row 返回）。

## 2. 详情页 layout（1440px 桌面，`DocumentSurface max-w-[1180px]` 居中）

### 结构总述

页面自上而下：**header → needs-you（钉住）→ Wave 模块**。Wave 模块 = 链式 stepper + 选中波卡（展开）+ 其他波折叠卡；**Agent Team 模块作为选中波卡的下半部分内嵌**。两模块边界即组件边界：`<WaveModule>` 渲染 `<AgentTeamModule runId={…}>`。

### 取舍：单页链视图 + 每波一个 URL 的混合

- **取**：每波保留自己的 URL（`?team=<runId>`）。理由：后端实体就是一波=一个 TeamRun；深链、浏览器前进/后退、从列表/MCP `dashboardUrl` 直达都免费；与 Workflows 惯例一致。
- **取**：页面内保留链式上下文（stepper + 历史折叠卡）。理由：wave 的意义来自"上一波偏差驱动本波 re-plan"，孤立单页丢失这个叙事。
- **舍**：全部波展开的单页长链。理由：信息密度爆炸 + 历史波挂实时流（审查明确禁止）。
- 落地：URL 选中的波**唯一展开**；历史波只渲染一行摘要折叠卡（**不挂流、不渲染成员区**，数据来自 snapshot 已有的该 run 记录）；从列表进入时选中波=该 wave 本身，首屏即当前波。若 URL 选中的是历史波，stepper 上方显示一行提示 "You're viewing wave 2 — current wave is 3 · Jump ↗"。

### ASCII 线框图

```
┌────────────────────────── max-w-[1180px] centered ──────────────────────────┐
│ ← AGENT TEAMS                                                               │
│ ┌─ HEADER ────────────────────────────────────────────────────────────────┐ │
│ │ TEAM RUN                                          [✉ New message] [▶ Start]│
│ │ Stage 6 — real-device wave          ← 短标题(派生: objective 首行, ≤60字) │ │
│ │ [running] [wave 3] [host: codex-app] run_019f… · created Jul 18 14:07   │ │
│ │ objective 全文一行截断, title 悬停看全文                                   │ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
│ ┌─ NEEDS YOU ── 仅非空渲染, 钉在页级, 永不进时间线 ────────────────────────┐ │
│ │ ⛔ 2 decisions waiting（红区, 只有拍板项能进）                           │ │
│ │  • [blocker]  backend-worker · "deploy updates the remote container…"   │ │
│ │    14:26 · policy interrupt                                  [Decide ↗] │ │
│ │  • [waiting]  host-lead is waiting for input                 [Decide ↗] │ │
│ │  ⚠ 3 deliveries unacked — hygiene, not decisions        [view] (一行折叠)│ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
│ ╔═ WAVE MODULE ═══════════════════════════════════════════════════════════╗ │
│ ║ chain: [w1 ✓]›[w2 ✓]›[▶ w3 running]›[w4 planned]   (+ Start next wave) ║ │
│ ║        ↑ 每节点 pill: wave N + status; 当前高亮; 兄弟分支显示 "×2" 角标  ║ │
│ ║ ┌─ WAVE 3 · real-device capabilities ─────────── [running] ──────────┐ ║ │
│ ║ │ Entry  ← wave 2 gate passed · operator · Jul 17 21:04 [view w2]    │ ║ │
│ ║ │          "all lanes merged, gate rerun green"   ← gate_note 引用    │ ║ │
│ ║ │          (derived)                                                  │ ║ │
│ ║ │ Exit →  integration gate: ✓ members terminal · ✗ 1 unacked handoff │ ║ │
│ ║ │          · ✓ no open blockers                    (derived checklist)│ ║ │
│ ║ │                                                                     │ ║ │
│ ║ │ ── Member contract ─────────────────────────────────────────────── │ ║ │
│ ║ │ Member      │ Delegate (quoted)  │ Done when        │ Boundaries   │ ║ │
│ ║ │ ●F ·device  │ "NFC, scan, AR…" ✉#│ in assignment ↗  │ write: src/  │ ║ │
│ ║ │  reviewer   │                    │ handoff ACKed ✓✉#│ + policy ⛔   │ ║ │
│ ║ │                                                                     │ ║ │
│ ║ │ ── Integration gate ────────────────────────────────────────────── │ ║ │
│ ║ │ ✓ all members terminal   ✗ 1 unacked handoff (msg-42 ↗)            │ ║ │
│ ║ │ ✓ no open blockers                                                │ ║ │
│ ║ │ [Complete gate…]  ← 仅 reviewing 且 checklist 全绿可点; 开 note 对话框│ ║ │
│ ║ │                                                                     │ ║ │
│ ║ │ ── Deviations (re-plan input for wave 4) ── 空则单行, 非空展开 ──── │ ║ │
│ ║ │ • [unacked handoff] media→host "T1 delivered: HANDOFF.md…" ✉#msg-42│ ║ │
│ ║ │ • [failed action] backend · test_completed · "contract 3/124 fail" │ ║ │
│ ║ ├─ AGENT TEAM MODULE (embedded) ─────────────────────────────────────┤ ║ │
│ ║ │ Members — cockpit (两路数据流的投影都在此区)                         │ ║ │
│ ║ │ ┌─────────┬──────────┬───────────────────┬────────┬────────┬─────┐ │ ║ │
│ ║ │ │ Member  │Cur. task │ Current action ⟳内 │Runtime │ Status │Last │ │ ║ │
│ ║ │ │ ●F dev  │ T-…      │ test_started 87/124│●ready  │running │2s   │ │ ║ │
│ ║ │ │ ●rev kimi│ T2/T3   │ review_started …   │●ready  │reviewing|1s  │ │ ║ │
│ ║ │ └─────────┴──────────┴───────────────────┴────────┴────────┴─────┘ │ ║ │
│ ║ │  行点击 → member drawer (右侧 overlay, 现有组件原样保留)             │ ║ │
│ ║ │ ┌─ Internal flow ⟳ (member 实时事件流) ─┬─ External flow ✉ (消息) ─┐ │ ║ │
│ ║ │ │ 最新在前 · filter [all members ▾]     │ 最旧在前 ledger          │ │ ║ │
│ ║ │ │ #1287 F       test_progress 87/124    │ ┌assignment host→F · ACK┐│ │ ║ │
│ ║ │ │ #1286 reviewer review_started …       │ │"NFC, scan…"          ││ │ ║ │
│ ║ │ │ #1285 host    message_sent control…   │ └──────────────────────┘│ │ ║ │
│ ║ │ │ …展开行: summary + evidence_refs       │ ┌blocker F→host · id=msg-│ │ ║ │
│ ║ │ │                        [Load more]    │ │41 · unacked · ⛔DECIDE ││ │ ║ │
│ ║ │ │                                       │ │ [Approve] [Reject]    ││ │ ║ │
│ ║ │ │                                       │ └──────────────────────┘│ │ ║ │
│ ║ │ │                                       │ ┌ composer (常驻) ─────┐│ │ ║ │
│ ║ │ │                                       │ │ From host·Kind·To chips││ ║ │
│ ║ │ │                                       │ │ [textarea…]   [Send] ││ │ ║ │
│ ║ │ │                                       │ └──────────────────────┘│ │ ║ │
│ ║ │ └───────────────────────────────────────┴──────────────────────────┘ │ ║ │
│ ║ └─────────────────────────────────────────────────────────────────────┘ ║ │
│ ║ Other waves (collapsed · 一行摘要 · 不挂流):                              ║ │
│ ║ ▸ wave 2 · "data & E2E" · completed · gate: "all lanes merged…" · 1 dev  ║ │
│ ║ ▸ wave 1 · "unblock" · completed · 0 deviations                          ║ │
│ ╚══════════════════════════════════════════════════════════════════════════╝ │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 默认状态与空态/离线态

- **展开/折叠**：needs-you 空则不渲染；decisions 永远展开，unacked 恒为一行折叠。契约表展开。gate 区在 running/reviewing 展开、completed 收成一行（"gate passed · operator · completed_at · note 引用"）。deviations 空则单行 "No deviations recorded"。internal flow 默认最新 30 条 + Load more；external flow 全量（单波消息量级小）+ composer 常驻。其他波恒折叠。
- **空态**：无 members / 无消息 / 无动作 → 复用现有 `EmptyState`；成员无 assignment → 契约表该格 "No assignment recorded"（诚实，不编造）；无 handoff → Done-when 列只显示 "stated in assignment ↗"。
- **离线态**：非 live source 时所有写操作走现有 `ActionButton` 禁用 + tooltip；顶栏 freshness chip 已有 sse/polling/connecting 三态，不新增。选中波非最新时的 "viewing historical wave" 提示条只在 selection≠最新波时出现。

### 现有组件 → 新区域映射（重组不重写）

| 现有（TeamRuns.tsx） | 去向 |
|---|---|
| `WaveStepper` | → chain stepper（加兄弟分支角标、sibling 提示） |
| `WaveTab` 的 contract/gate/deviations 三段 | → Wave 卡三段（契约表改四列，gate 加 note 对话框，deviations 补 failed actions） |
| `OverviewTab` cockpit 表 | → Agent Team 模块成员区（删 Goal summary 卡，预算/状态并入 wave 卡 header meta） |
| member strip 横排卡 | **删除**（与 cockpit 重复） |
| `ActivityTab` | → Internal flow lane（加 member filter） |
| `MessagesTab` + `MessageComposer` | → External flow lane（消息卡加 dom id、ACK/Decide 按钮） |
| `NeedsYouBanner` | → needs-you（拆红/琥珀两区，加 Decide/深链） |
| `MemberDrawer` | 原样保留（L2 钻取） |
| `NewTeamRunDialog` | 原样复用（创建 + Start next wave 预填） |

文件建议：`TeamRuns.tsx` 已 2374 行，按模块边界拆为 `surfaces/team/TeamRunsList.tsx`、`surfaces/team/WaveModule.tsx`、`surfaces/team/AgentTeamModule.tsx`，tones/helpers 收进 `surfaces/team/shared.ts`。

## 3. Wave 卡设计

### 进入/退出条件（诚实显示规则：引用原文 vs 标派生）

- **进入条件**：
  - wave 1：引用字段——"Created by operator · `created_at` · `host_surface`"。
  - wave N>1：派生——"wave N−1 gate passed · operator · `completed_at`"，附上一波 `gate_note` 原文引用（有则引号引用，无则不显示该行，绝不编造）+ `[view wN−1]` 链到上一波 URL。行尾挂 muted `derived` 小标。
- **退出条件**：固定三条派生 checklist（全部可由现有数据计算）：① all members terminal（`member_runs.status` ∈ completed/failed/stopped）② no unacked handoffs（`team_messages` kind=handoff 的 deliveries 无 queued/delivered）③ no open blockers（blocker 消息的 host delivery 未 ACK）。每条 ✓/✗ + 失败项内联深链（如 `msg-42 ↗`）。过门后该区收成一行并引用 `gate_note` 原文。

### Member 契约表四列（每列的字段映射与缺数据处理）

| 列 | 数据来源 | 呈现规则 |
|---|---|---|
| **Member** | `member_runs`（name/role/provider/model/status） | provider 色点 + 名 + role pill + status pill（现有 tones） |
| **Delegate 什么** | 最新一条发往该成员的 `team_messages`（kind=assignment）`body` | **引号原文引用**，2 行截断 + `✉#msg-id` 跳 External flow 对应卡；无 → "No assignment recorded"（muted） |
| **完成标准** | v0 **无字段**（缺陷④） | 不假装有：显示 "stated in assignment ↗"（链同一条 assignment）；其下一行派生完成信号：该成员最新 handoff 消息 + ACK 状态（"handoff ACKed Jul 18 ✉#"），挂 `derived` 小标。结构化 `done_criteria` 列入可推迟后端补充 |
| **边界 / 不做** | `member_runs.owned_paths` + 全局政策 | `owned_paths` 渲染为 write-scope badges（空 → "read-only"）；下方固定一行 muted 政策文案 "deploy / merge / remote-delete 需用户拍板"（harness 级不变量，标 `policy` 不标数据） |

### 集成门：避免"一键无证据翻转"

- "Complete gate" 按钮只在 `status==="reviewing"` 且三条 checklist 全绿时可点（沿用现有 `gateReady` 逻辑）。
- 点击开 **Gate 对话框**（不是直接提交）：上半 = 三条 checklist 快照（只读）；下半 = **必填** textarea "Gate note — what did you verify?"。note 为空禁止 Confirm。
- Confirm → `POST /v1/team-runs/{id}/transition {to:"completed", note}`（**后端补充③**：接受并持久化 `gate_note` 到 `team_runs`）。
- 过门后：wave 卡 gate 区显示 "gate passed · operator · `completed_at`"，note 原文引用；该 note 同时成为**下一波进入条件**的引用文本——过门证据在链上自然流动，re-plan 回路（偏差→决策→下一波）在 UI 闭环。

## 4. 拍板项设计

- **定义（只认需要人拍板的）**：`decisions =`（kind ∈ {blocker, review_request} 且发向 host 的 delivery 未 ACK 的消息）+（status=waiting 的成员）。红区。`unacked`（所有 queued/delivered deliveries）是卫生项，琥珀色、一行折叠、**不进红区计数、在列表排序权重中降权**。
- **页面级位置**：needs-you 永远钉在 header 与 wave 模块之间，是全页唯一红色区块；timeline 里不再重复告警。
- **处理闭环**：needs-you 项的 `[Decide ↗]` → 设置 `?team=<id>&msg=<messageId>` → 页面滚动到 External flow lane 中该消息卡（`scrollIntoView` + 高亮 ring）→ 消息卡上 `[Approve] [Reject]` → decision 对话框（预设 kind=control、预填 body "Approved: …"/"Rejected: …"、可改、可指定收件人）→ Confirm 一次提交两个调用：`sendTeamMessage` + `POST …/messages/{id}/ack`。ACK 落库后消息离开 needs-you（下轮 snapshot/帧到达即消失）。
- **深链路径（列表 → 具体消息）**：列表 needs-you 列的红色 "N decisions" chip 本身就是链接，指向 `?team=<id>&msg=<第一个未决 approval 的 id>`（琥珀 unacked 数只做展示，链到详情页不定位）。点击后 3 击完成处理：chip(1) → Approve(2) → Confirm(3)。
- **前端改动点**：`SelectionState` 加 `messageId`（param `msg`，`selectionParamKeys` += "msg"）；消息卡加 `id={teamMessageDomId(m.id)}`；详情页加 effect：`selection.messageId` 存在时滚动+高亮。

## 5. 两路数据流的实时性设计（一个事实源）

**架构事实（已存在，直接复用）**：`openEventStream(/v1/events)` → 命名帧 → `applyFrame(snapshot, frame)`（`upsertById` 合并进同一份 snapshot）→ `WorkbenchModel` → 各视图。**唯一的 store 是 snapshot；所有视图都是它的渲染期投影。**

- **内部流（member SSE 实时事件流）**：`member_actions` + `team_run_events` + `member_runs`。
  - `team_run_event` 帧已有；**后端补充②**：新增 `member_run` / `member_action` / `team_message` 三种 SSE 帧（payload 与 snapshot 数组元素同形，写入时广播）。
  - 前端改动机械对称：`SseFrame` +3 变体、`openEventStream` +3 listener、`applyFrame` +3 case（全部走 `upsertById`）。**不新增任何第二数据通道**。
  - 消费点：cockpit "Current action" 列（`latestMemberAction` 重算）、internal flow lane（追加）、drawer actions tab（过滤投影）、心跳（`last_event_at`）。
- **外部流（host↔member 消息）**：`team_messages`（含 deliveries）。同一帧通路；消费点：external flow ledger、needs-you、wave 卡 checklist/deviations、列表 needs-you 列。
- **成员流 vs 全局流**：**订阅只有一个**（现有 app 级 `/v1/events` 连接，P6 project 作用域）。member drawer / member filter 全部是渲染期 `filter(member_run_id)` 投影，**禁止**为单成员开新流或新 fetch。
- **降级与自愈**：SSE 断开 → 现有 `useEventStream` 自动切 polling + 退避重连；重连后 snapshot 帧触发 resync。轮询与 SSE 吃同一端点同一份数据，不构成第二事实源。写操作后沿用现有 "POST → 刷新 snapshot" 兜底。
- **离线/历史波**：历史波折叠卡不消费流（一行摘要来自 snapshot 静态数据）；展开历史波时其 team 模块渲染同一 store 的静态投影——该波成员已终态、无新帧，自然静止。

## 6. 操作清单与点击路径

| 操作 | 路径 | 点击数 |
|---|---|---|
| 创建 run（含 member 配置） | 列表 `New Team Run`(1) → 填 objective + members → `Create team run`(2) → 自动跳详情 | **2** + 表单 |
| 发消息 | 详情页 composer 常驻（To chips + kind + body） → `Send`(1) | **1** + 表单 |
| 处理拍板项 | needs-you `Decide ↗`(1) → 定位消息卡 → `Approve`(2) → 对话框 `Confirm`(3)（= 发 control 消息 + ACK） | **3** |
| 过集成门 | wave 卡 `Complete gate…`(1) → 对话框看 checklist + 必填 gate note → `Confirm`(2) | **2** |
| 开下一波 | 链尾 `+ Start next wave`(1，仅最新波 completed 可用) → 预填对话框（继承 + deviations hints） → `Create wave N`(2) | **2** |
| 看历史波证据 | 点历史折叠卡(1) → 该波只读页 → 可选 drawer 展开 action 看 evidence_refs（再 1–2） | **1–3** |

全部写操作沿用现有 POST → snapshot 刷新通路；`Start orchestration` 保留 501 + CLI 提示的诚实处理。

## 7. 后端最小补充清单

**必须有（缺了设计不成立）**
1. `POST /v1/team-runs/{id}/messages/{messageId}/ack` — 没有它 unacked/needs-you 永远无法清除，拍板项闭环不存在（缺陷①）。
2. SSE 新增 `member_run` / `member_action` / `team_message` 三种 upsert 帧 — "观看成员在干什么"的实时性承诺；前端 merge 通路已就绪（缺陷②）。
3. transition 端点接受并持久化 `gate_note`（`POST /v1/team-runs/{id}/transition {to, note}`，落 `team_runs.gate_note` 并回 snapshot）— 过门必须留证据，杜绝一键无证据翻转（缺陷③）。

**可推迟（前端有诚实的降级显示）**
4. `team_runs.title` 短标题 — 前端先派生 objective 首行 ≤60 字（缺陷⑨）。
5. per-member `done_criteria` — v0 引用 assignment 原文 + handoff ACK 派生信号（缺陷④）。
6. budget `used` — 只显示 limit，标 "usage not tracked in v0"（缺陷⑤）。
7. snapshot 分页/cursor — Stage 6 规模全量可承受（缺陷⑥）。
8. DelegationRun 写入者 — drawer 已有 honest empty state（缺陷⑦）。
9. waiting/failed 等 member 状态写入路径 — 前端已渲染全部状态（缺陷⑧）。
10. 消息 `correlation_id`（review 配对）— v0 用时间序 + ACK 规则近似。

## 8. 明确不做什么（v0 边界）

- 不做 Task Graph / DAG 可视化（team 域 v0 无 task 实体；wave 卡契约表 + gate checklist 承担其信息职责）。
- 不做成员级控制按钮（暂停/恢复/注入/中断）：v0 无后端动作；操作面只有 create / message / ack / gate / next-wave。
- 不做原始 provider 流面板：保留 drawer 的 Raw JSON tab 作诊断下限；不展示隐藏推理。
- 不做 per-member 独立订阅/第二通道：一个事实源，成员视图只是投影。
- 不做自动 re-plan：next-wave 对话框只预填 deviations 作 hints，改 objective/roster 的是人。
- 不做常驻组织/通讯录视图、跨 run 对比、预算仪表盘。
- 列表不做搜索/筛选/分页：needs-you-first + lineage 缩进已覆盖 v0 量级。
- 不做 needs-you 的桌面通知/声音：页面内钉住即 v0 全部告警面。

## 附：设计决定可追溯性

- 两模块嵌套/两路数据流（要求 2、3）→ §2 结构总述、§5
- 逐波委派卡（要求 4）→ §3 契约表四列 + 进入/退出条件
- 观看优先 + 简单必须操作（要求 1）→ §2 去 Tab 单页、§6 点击数 ≤3
- 三个失败模式 → §2 单波首屏与历史折叠、§3 契约表责任到人、§3 边界列 + §4 拍板项闭环 + §3 gate note
- Stage 6 场景（4 波/兄弟分支/真机互斥/11 验收门）→ chain stepper + 兄弟角标、契约表 owned_paths、gate checklist
- Wave 模型 → §2 每波一 URL、§3 gate、deviations 区 + next-wave prefill hints
- 九项缺陷 → §7 分档（①②③必须有；④⑤⑥⑦⑧⑨可推迟且前端有诚实降级）
- 四项审查 UX 结论 → needs-you 页级钉住+深链（§4）、首屏当前波（§2 取舍）、单卡密度（默认折叠）、单一事实源（§5）、历史波折叠卡一行摘要不挂流（§2）

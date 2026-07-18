# Agent Team 页面 Layout 设计方案 v2（实施蓝图）

## 第一轮质疑消化总表

| # | 状态 | 怎么改的（落点） |
|---|---|---|
| P0-1 ACK 语义混用 | 已消化 | 新建用户拍板维度 `team_messages.resolved_at/resolved_by/resolution` + `POST …/messages/{id}/resolve`；transport ACK 不动；needs-you 与 gate checklist③ 全部键在 resolved 维度（§4、§3、§7-①） |
| P0-2 waiting 无闭环 | 已消化 | waiting↔running 写入路径升必须有（adapter 在 waiting_for_approval 时写 waiting；operator control 送达翻转）+ 新增成员级深链 `?team=<id>&teamMember=<id>`，Decide = 预填给该成员的 decision 对话框（§4、§5、§7-⑥）。blocked/failed 写入保持可推迟但 UI 条件渲染 |
| P0-3 漏 team_run 帧 | 已消化 | 必须有清单加第④项 `team_run` upsert 帧；gate 按钮靠它点亮（§5、§7-④） |
| P1-4 成员在干什么埋两屏下 | 已消化 | 波卡重排：Entry/Exit 各收成一行（点击展开）→ **Agent Team 模块提升到契约表之前** → 契约表 → gate（running 收成一行计数）→ deviations（非空才展开）（§2 线框图全改） |
| P1-5 契约两列空壳 | 已消化 | 创建/下一波对话框加 per-member 三段契约输入（Delegate/Done when/Boundaries），前端拼成带固定小节标记（`## Task` / `## Done when` / `## Boundaries`）的 assignment 发出（不等后端字段）；契约表按小节解析入列；Delegate 列=该成员全部 assignment 合并视图；行可展开全文；无小节的消息诚实显示 "not stated"（§3、§6） |
| P1-6 操作者身份冒充 | 已消化 | composer 去掉 From 选择、固定 from=operator；Decide/Answer 也从 operator 发；后端接受并持久化 `sender_kind`（必须有⑤）；operator 独立身份色（decision 紫 + "operator (you)"）（§4、§5、§7-⑤） |
| P1-7 needs-you 漏报 | 已消化 | decisions 扩为四类：发向 host 且未 resolve 的 {blocker, review_request, question} + waiting/blocked/failed 成员 + decision 类消息的 failed/expired delivery；每类配处理路径；琥珀区只留普通 unacked（§4） |
| P2-8 双栏高度失衡 | 已消化 | 两栏 `max-h-[520px]` 各自内滚、栏底对齐；composer 固定在 external 栏滚动区外；`<lg(1100px)` 退化上下堆叠（external 在上）；internal 行可展开全文（§2） |
| P2-9 消息卡无 evidence | 已消化 | 消息卡渲染 `evidence_refs` badges（复用 drawer 渲染件）；handoff/review_result 卡默认展开 delivery+evidence 行（§2、§3） |
| P2-10 gate checklist 是卫生条件 | 已消化 | checklist 挂 muted "derived minimum conditions" 小标；gate 对话框上半=证据包（本波 handoff 清单+evidence 深链+objective/各成员 Done when 原文引用）；note 预填自动草稿（checklist 状态+deviations+handoff 清单拼装），用户改写（§3） |
| P2-11 多波缺口 | 已消化 | `waveLineage` 重写为 `waveTree`（children 全量）；主链=含最新活跃波的分支；兄弟角标可切换；Start next wave 仅在"选中=链尾且 completed"显示，历史波上明示 "Branch new wave from wave N"；stepper 区分 selected(outline)/active(fill+pulse)（§2、§3） |
| P2-12 标题派生失效 + 裸 task id | 已消化 | 对话框加必填 "Wave title"，提交时组合 `objective = title + "\n\n" + details`（前端约定，缺陷⑨ 保持可推迟）；next-wave 时 title 强制重填；cockpit 隐藏 Current task 列，current_task_id 挪进 drawer 属性（§2、§3、§6） |
| P3-13 列表排序与 lineage 冲突 | 已消化 | 根节点排序键=其 lineage 子树内最大 signals.total（§1） |
| P3-14 Decide 双调用非原子 | 已消化 | 固定 send→resolve 顺序；resolve 失败 toast 明示+仅重试 resolve（§4、§6） |
| P3-15 历史波丢失当前波 needs-you | 已消化 | Jump 提示条带红色计数："current wave 3 · 2 decisions waiting · Jump ↗"（§2） |
| P3-16 次要三项 | 已消化 | 琥珀 [view] → 锚到 external lane 顶部（`#external-flow`）；Approve/Reject 仅渲染于 pending decision 消息（decision 类∧发向 host∧resolved_at IS NULL）；budget limit 补进 wave header meta（§2、§4） |

无不采纳项。

## 0. 关键判断与遗留确认

1. **取消现有 Overview/Activity/Messages/Wave 四 Tab**，改单页分区（负责人要求模块嵌套；Tab 是平级隐藏面板，矛盾）。
2. **"已处理"是独立用户维度，不碰 transport ACK**（v1 判断作废，P0-1）：拍板闭环键在 `resolved_at/by/resolution`，delivery 的 ACK 仍是 host agent 的传输签收。
3. **删除 member 横排卡片条**（与 cockpit 表重复）。
4. **waiting 状态选"提升写入路径"而非降级隐藏**（P0-2 两选一）：它是 needs-you 一等信号的触发点便宜且明确（adapter 已观测 waiting_for_approval 动作）。
5. **波标题用前端组合约定**（P2-12 两选一）：必填 title 字段拼进 objective 首行，`team_runs.title` 保持可推迟。

## 1. 信息架构总图

| 层级 | 地址 | 区域 | 用户看什么 | 用户做什么 |
|---|---|---|---|---|
| L0 列表 | `?surface=team` | Team Runs 收件箱 | 所有 wave（lineage 树缩进）、状态/成员/needs-you/最后活动 | 打开一个波；创建 run |
| L1 详情 | `?team=<runId>` | 单波页面 | 当前波成员在干什么、谁需要我拍板、契约/门/偏差 | 处理拍板项、发消息、过门、开下一波/分支波 |
| L1.5 消息深链 | `?team=<id>&msg=<messageId>` | 详情页+定位高亮消息卡 | needs-you 直达具体消息 | 就地 Decide/Answer |
| L1.6 成员深链（v2 新增） | `?team=<id>&teamMember=<memberRunId>` | 详情页+高亮 cockpit 行并自动开 drawer | waiting/blocked/failed 成员直达 | 就地发 control |
| L2 成员抽屉 | 无 URL（overlay） | Member drawer | 单成员动作/委派/消息/Raw | 只读钻取 |

- **selection 扩展**（纯前端）：`SelectionState` 加 `teamMessageId`（param `msg`）与 `teamMemberId`（param `teamMember`——注意现有 `member` param 已被 Agents 面占用，不复用）；`selectionParamKeys` += 两键。
- **列表排序（P3-13）**：先算每 run 的 `signals.total`，stitchLineage 缩进不变，**根节点排序键=其 lineage 子树内最大 signals.total**（热子波把整族顶上来，层级不错乱），平级按最后活动倒序。

## 2. 详情页 layout（1440px，`DocumentSurface max-w-[1180px]`）

### 结构与取舍（同 v1，结论不变）

单页链视图 + 每波一个 URL：URL 选中的波唯一展开；历史波一行折叠卡（不挂流）；stepper 保留链式上下文。放弃全展开长链（密度爆炸+历史挂流）。

### 波卡区域顺序（P1-4，v2 重排）

运行中的波：**header（Entry/Exit 各一行，点击展开）→ AGENT TEAM 模块（cockpit+双栏）→ 契约表 → 集成门 → deviations**。成员实时状态进首屏；契约/门/偏差作为参考区下沉。gate 区按状态变默认：running=一行计数（点击展开）、reviewing=展开、completed=一行结果。deviations 非空才展开，空则单行。历史波（终态）保持同顺序，双栏静态渲染（无 pulse）。

### ASCII 线框图（v2 更新版）

```
┌────────────────────────── max-w-[1180px] centered ──────────────────────────┐
│ ← AGENT TEAMS                                                               │
│ ┌─ HEADER ────────────────────────────────────────────────────────────────┐ │
│ │ TEAM RUN · WAVE 3                              [✉ New message] [▶ Start]│ │
│ │ real-device capabilities   ← wave title（= objective 首行, 创建时必填）   │ │
│ │ [running] [host: codex-app] run_019f… · created Jul 18 · budget $2.00   │ │
│ │ limit · usage n/a (v0)                                                  │ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
│ ┌─ NEEDS YOU（钉住, 红区只有拍板项; 键在 resolved 维度, P0-1/P1-7）────────┐ │
│ │ ⛔ 4 decisions waiting                                                  │ │
│ │  • [blocker]  backend-worker · "deploy updates remote container…"       │ │
│ │    from member · 14:26                    [Decide ↗](→msg 深链)         │ │
│ │  • [question] reviewer · "which account for NFC test?"  [Answer ↗]      │ │
│ │  • [waiting member] host-lead is waiting for input  [Decide ↗](→成员深链)│ │
│ │  • [delivery failed] blocker msg-41 → host · retry via composer [View ↗]│ │
│ │  • [blocked] media-worker is blocked                    [View ↗](→成员) │ │
│ │  ⚠ 3 deliveries unacked — hygiene        [view](→ #external-flow 顶部)  │ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
│ ╔═ WAVE MODULE ═══════════════════════════════════════════════════════════╗ │
│ ║ chain: [w1✓]›[w2✓ ×2▾]›[▶w3]›[w4 planned]        (+ Start next wave)   ║ │
│ ║   selected=outline · active=fill+pulse · ×2▾=兄弟分支切换(P2-11)        ║ │
│ ║   ⚠ 若选中历史波: "Viewing wave 2 · current wave 3 · 2 decisions        ║ │
│ ║      waiting · Jump ↗"（红色计数, P3-15）                               ║ │
│ ║ ┌─ WAVE 3 · real-device capabilities ─────────────────── [running] ──┐ ║ │
│ ║ │ ▸ Entry · ← w2 gate passed · operator · Jul 17 21:04 · note 引用   │ ║ │
│ ║ │ ▸ Exit  · gate min-conditions 2/3 · 1 open deviation  (点击展开, P1-4)│ ║ │
│ ║ ├─ AGENT TEAM MODULE (embedded · 提升到此, P1-4) ────────────────────┤ ║ │
│ ║ │ Cockpit — every member at a glance（行点击→drawer）                 │ ║ │
│ ║ │ ┌────────────┬──────────────────────┬─────────┬──────────┬───────┐ │ ║ │
│ ║ │ │ Member     │ Current action ⟳内部流│ Runtime │ Status   │ Last  │ │ ║ │
│ ║ │ │ ●F device  │ test_started 87/124  │ ●ready  │ running  │ 2s    │ │ ║ │
│ ║ │ │ ●rev kimi  │ review_started …     │ ●ready  │ reviewing│ 1s    │ │ ║ │
│ ║ │ └────────────┴──────────────────────┴─────────┴──────────┴───────┘ │ ║ │
│ ║ │ (Current task 列 v0 隐藏, P2-12; current_task_id 挪 drawer 属性)    │ ║ │
│ ║ │ ┌─ Internal flow ⟳ ────────────────┬─ External flow ✉ ────────────┐│ ║ │
│ ║ │ │ id=internal-flow                 │ id="external-flow"(琥珀view锚)││ ║ │
│ ║ │ │ max-h 520 内滚 · 栏底对齐(P2-8)  │ max-h 520 内滚 · 栏底对齐     ││ ║ │
│ ║ │ │ filter [all members ▾]           │ ┌assignment operator→F · ACK ┐││ ║ │
│ ║ │ │ #1287 F    test_progress 87/124  │ │"## Task… ## Done when…"    │││ ║ │
│ ║ │ │ #1286 rev  review_started ▸展开  │ │evidence: [ev-…] (P2-9)     │││ ║ │
│ ║ │ │  展开行: summary+evidence badges │ └────────────────────────────┘││ ║ │
│ ║ │ │              [Load more]         │ ┌handoff F→host · 默认展开────┐││ ║ │
│ ║ │ │                                  │ │delivery: ACK · evidence ×3 │││ ║ │
│ ║ │ │                                  │ └────────────────────────────┘││ ║ │
│ ║ │ │                                  │ ┌blocker F→host · ⛔PENDING──┐││ ║ │
│ ║ │ │                                  │ │ [Approve] [Reject](条件渲染)│││ ║ │
│ ║ │ │                                  │ └────────────────────────────┘││ ║ │
│ ║ │ │                                  │ ├ composer (固定栏底, 滚动区外)┤│ ║ │
│ ║ │ │                                  │ │ from: operator(固定,P1-6)   │││ ║ │
│ ║ │ │                                  │ │ Kind▾ To[chips] [textarea]  │││ ║ │
│ ║ │ │                                  │ │                      [Send] │││ ║ │
│ ║ │ │                                  │ └────────────────────────────┘││ ║ │
│ ║ │ └──────────────────────────────────┴──────────────────────────────┘│ ║ │
│ ║ │  <1100px: 双栏堆叠, external 在上 (P2-8)                             │ ║ │
│ ║ ├─ Member contract（结构化契约, P1-5; 行可展开读全文）─────────────────┤ ║ │
│ ║ │ Member   │ Delegate(全部 assignment 合并) │ Done when  │ Boundaries │ ║ │
│ ║ │ ●F device│ "## Task NFC, scan, AR…" ✉#×2  │ "owner     │ "other     │ ║ │
│ ║ │          │  ▸ expand full text            │  signoff…" │  lanes…"   │ ║ │
│ ║ │          │                                │ (parsed)   │ +owned_paths│ ║ │
│ ║ │          │                                │            │ badges+⛔政策│ ║ │
│ ║ │ 无小节的消息: Delegate 引全文, 其余列 "not stated in assignment"(诚实)│ ║ │
│ ║ ├─ Integration gate ── running: 一行计数(点击展开) / reviewing: 展开 ──┤ ║ │
│ ║ │ min-conditions (derived, muted 小标, P2-10):                        │ ║ │
│ ║ │ ✓ all members terminal · ✗ 1 unacked handoff(msg-42↗) · ✓ no open  │ ║ │
│ ║ │ blockers(键 resolved 维度)                                          │ ║ │
│ ║ │ [Complete gate…] → Gate 对话框:                                     │ ║ │
│ ║ │  上半=证据包: 本波 handoff 清单(evidence badges+✉深链) + objective │ ║ │
│ ║ │  原文 + 各成员 "## Done when" 引用                                  │ ║ │
│ ║ │  下半=note 必填, 预填自动草稿(checklist 状态+deviations+handoff     │ ║ │
│ ║ │  清单拼装), 用户改写 (P2-10)                                        │ ║ │
│ ║ ├─ Deviations（非空才展开, re-plan input）────────────────────────────┤ ║ │
│ ║ │ • [unacked handoff] media→host "…" ✉# · [failed action] test_…     │ ║ │
│ ║ └────────────────────────────────────────────────────────────────────┘ ║ │
│ ║ Other waves (collapsed · 一行摘要 · 不挂流):                              ║ │
│ ║ ▸ wave 2 · "data & E2E" · completed · gate:"all lanes merged…" · 1 dev   ║ │
│ ╚══════════════════════════════════════════════════════════════════════════╝ │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 默认状态/空态/离线态（增量）

- 历史波打开时：banner 带当前波红色 decisions 计数（P3-15）；双栏静态。
- 无 structured assignment 的成员：Done when/Boundaries 列显示 "not stated in assignment"（muted，诚实）。
- 其余同 v1（EmptyState 复用、ActionButton 离线禁用、freshness chip 三态）。

### 组件映射（v2 增量）

- `WaveStepper` → `WaveChain`（`waveTree` 树、selected/active 双态、兄弟角标菜单、Branch 按钮逻辑）
- `WaveTab` 三段 → 波卡参考区三段（契约表四列+小节解析+行展开；gate 一行/展开自适应+Gate 对话框；deviations 补 failed actions、非空才展开）
- `OverviewTab` cockpit → 成员区（隐藏 Current task 列）
- `ActivityTab`/`MessagesTab` → 双栏（max-h 内滚、composer 钉栏底、evidence badges、条件渲染 Decide 按钮）
- `NeedsYouBanner` → needs-you（四类 decisions + 琥珀一行）
- 新增：`GateDialog`、`DecisionDialog`（approve/reject/answer/nudge 四模式）、`ContractFields`（三段输入子表单）、`waveTree`/`mainChain` helpers、`teamMessageDomId`/`teamMemberDomId`
- `NewTeamRunDialog`：加必填 "Wave title" + per-member `ContractFields`；提交后对有契约的成员逐个发 assignment（from operator）

## 3. Wave 卡设计

### 进入/退出条件（P1-4：各收成一行，点击展开）

- Entry 一行：`← w{N−1} gate passed · operator · {completed_at} · "{gate_note}"`，点击展开看完整引用与 `[view w{N−1}]` 链接；wave 1 = "Created by operator · created_at · host_surface"。派生行挂 `derived` 小标的规则不变。
- Exit 一行：`gate min-conditions {x}/3 · {n} open deviations`，点击展开三条 checklist。checklist 整体挂 muted **"derived minimum conditions"** 小标（P2-10）——它是内部卫生下限，不冒充 Stage 6 那种外部事实门（子 PR 合入+门重跑）；外部事实由 gate note 证据包人工确认（v0 不自动拉取，见 §8）。

### Member 契约表（P1-5：结构化契约）

- **输入侧**：创建/下一波对话框每个 member 增加可折叠 "Contract (optional)" 三段输入：Delegate（任务包）/ Done when（完成标准）/ Boundaries（不做）。提交时前端拼成带固定小节标记的 assignment body 并逐成员 POST（kind=assignment，from operator）：

  ```
  ## Task
  <delegate 文本>
  ## Done when
  <done when 文本>
  ## Boundaries
  <boundaries 文本>
  ```

- **渲染侧**：契约表按小节标记解析到对应列；**Delegate 列=该成员全部 assignment 的合并视图**（operator 契约 + host 后续 assignment，逐条 ✉# 深链）；行可展开读全文。无小节标记的 assignment：Delegate 列引全文，Done when/Boundaries 列 "not stated in assignment"。
- **Boundaries 列**：解析文本 + `owned_paths` badges（空=read-only）+ 固定政策行（deploy/merge/remote-delete 需用户拍板，标 `policy`）。
- host 自发 assignment 不带小节时优雅降级，不阻塞表格。

### 集成门（P2-10：证据包 + 草稿 note）

- 可点条件不变（reviewing + 三条 min-conditions 全绿）。
- **Gate 对话框**：上半 "Evidence pack" = 本波全部 handoff 消息清单（evidence badges + ✉深链）+ objective 原文 + 各成员 `## Done when` 小节引用；下半 note 必填但**预填自动草稿**（由 checklist 状态 + deviations 摘要 + handoff 清单拼装），用户改写而非从零写。
- 提交：`POST /v1/team-runs/{id}/transition {to:"completed", note}`（后端③）。过门后一行结果（gate passed · operator · completed_at · note 引用），note 同时被下一波 Entry 引用。

### 多波分支（P2-11）

- `waveTree(runs)`：childrenByParent 全量建树；**主链**=从根沿"含最新活跃波（非终态；无活跃则最新 created）"的分支走。
- stepper 节点：`selected`=outline，`active(running)`=fill+pulse；多 child 节点带 `×N▾` 角标，菜单列出各分支末端（wave N + status + title），点击 `?team=<id>` 跳分支。
- "Start next wave" 显示规则：选中波=主链链尾且 completed → `Start wave N+1`（prefill previousRunId=选中）；选中波 completed 但非链尾 → **`Branch new wave from wave N`**（明示创建兄弟分支）；选中波未 completed → 隐藏并一行提示原因。

## 4. 拍板项设计（P0-1/P1-7 重写）

- **用户拍板维度（新）**：`team_messages.resolved_at / resolved_by / resolution`（resolution ∈ approved/rejected/answered/dismissed）。写入只经 `POST /v1/team-runs/{id}/messages/{messageId}/resolve {resolution, note?}`。transport ACK（deliveries[].status）语义不动。
- **decisions（红区）= 四类，各有处理路径**：

  | 类别 | 判定 | 处理路径 |
  |---|---|---|
  | decision 消息 | kind∈{blocker, review_request, question} ∧ 发向 host ∧ `resolved_at IS NULL` | Decide/Answer ↗ → `?team=&msg=` 定位消息卡 → Approve/Reject/Answer → 对话框 → Confirm |
  | waiting 成员 | `member_runs.status==="waiting"` | Decide ↗ → `?team=&teamMember=` 高亮 cockpit 行+开 drawer → 预填 control 对话框（to=该成员） → Confirm；后端送达后翻转 running（后端⑥） |
  | blocked/failed 成员 | status∈{blocked, failed} | View ↗ → 成员深链开 drawer 看现场；可选 nudge（同 control 对话框）；状态被 adapter 改写后自动出区 |
  | failed delivery | decision 类消息 ∧ delivery∈{failed, expired} | View ↗ → 消息卡；经 composer 预填引用重发（自动重投端点可推迟，§8） |

- **琥珀区**只剩普通 unacked（非 decision 类的 queued/delivered），一行折叠，`[view]` 锚到 `#external-flow` 顶部（P3-16）。旧数据无 resolved_at 时按未处理计（诚实默认）。
- **Decide 提交（P3-14）**：固定顺序 ① `sendTeamMessage`（from operator，kind=control/answer，body 含决定+note）→ ② `resolve`；②失败时 toast 明示 "decision sent, resolve failed" + 仅重试 ②。
- **消息卡按钮渲染条件（P3-16）**：仅当消息 ∈ decision 类 ∧ 发向 host ∧ `resolved_at IS NULL` 时显示 [Approve] [Reject]（question 显示 [Answer]）；已 resolve 的卡显示 `resolved · approved · operator · time` 一行。
- **身份（P1-6）**：composer 固定 from=operator（删 From 下拉）；所有 operator 消息渲染为 "operator (you)" + 独立 decision 紫 pill；`from_member_id` 为空 + `sender_kind="operator"`（后端⑤），台账不再出现"host 自己批准自己"。
- **列表红 chip**：`N decisions` = 四类合计，链接到 `?team=<id>&msg=<首个未决 decision 消息>`（无 decision 消息则链 `teamMember`）；琥珀仅展示。

## 5. 两路数据流的实时性设计（一个事实源）

架构不变：`/v1/events` → 命名帧 → `applyFrame`（upsertById 进同一份 snapshot）→ 唯一 store → 渲染期投影。

- **帧清单（v2 补齐，P0-3）**：现有 `team_run_event`；新增四种 upsert 帧——`member_run`、`member_action`、`team_message`（缺陷②）、**`team_run`**（P0-3：status/completed_at/gate_note 的实时通路，gate 按钮靠它在观看中点亮，needs-you 计数靠 member_run 帧的 waiting 翻转即时增减）。前端：`SseFrame` +4 变体、`openEventStream` +4 listener、`applyFrame` +4 case（全走 upsertById）。
- **内部流**：member_actions/team_run_events/member_runs → cockpit Current action 列、internal lane（filter=渲染期投影）、drawer、心跳。
- **外部流**：team_messages（含 deliveries、evidence_refs、resolved_*）→ external ledger、needs-you、契约表、gate checklist/deviations、列表 needs-you。
- **单订阅原则**：仅 app 级一条 `/v1/events`；成员视图全部 `filter(member_run_id)` 投影，禁止 per-member 流/fetch。
- **降级自愈**：`useEventStream` polling fallback + 退避重连 + snapshot resync（现有）；写操作后 POST→刷新 snapshot 兜底（现有）。

## 6. 操作清单与点击路径（v2 更新）

| 操作 | 路径 | 点击数 |
|---|---|---|
| 创建 run（含契约） | `New Team Run`(1) → 必填 Wave title + objective details + members（每成员可展开 Contract 三段） → `Create`(2)；前端自动：create → 逐成员发契约 assignment（失败 toast 列出+仅重发失败项） | **2** + 表单 |
| 发消息 | composer 常驻栏底（from=operator 固定） → `Send`(1) | **1** + 表单 |
| 处理 decision 消息 | needs-you `Decide ↗`(1) → 定位消息卡 → `Approve`(2) → 对话框（可改 note） `Confirm`(3)（=send→resolve，失败仅重试 resolve） | **3** |
| 处理 waiting 成员 | `Decide ↗`(1) → cockpit 行高亮+drawer → `Send control`(2) → 预填对话框 `Confirm`(3) | **3** |
| 过集成门 | `Complete gate…`(1) → 对话框（证据包 + 预填草稿 note，改写） → `Confirm`(2) | **2** |
| 开下一波 | 链尾 `+ Start next wave`(1) → title 强制重填 + roster/契约继承 + deviations hints → `Create wave N`(2) | **2** |
| 开分支波（P2-11） | 历史波上 `Branch new wave from wave N`(1) → 同上对话框（previousRunId=该波） → `Create`(2) | **2** |
| 看历史波证据 | 折叠卡(1) → 只读波页（banner 带当前波红色计数） → 可选 cockpit 行→drawer→展开 action（+1–2） | **1–3** |

## 7. 后端最小补充清单（v2 重排）

**必须有（缺了设计不成立）**

1. `POST /v1/team-runs/{id}/messages/{messageId}/resolve` + `team_messages.resolved_at/resolved_by/resolution` — 用户拍板维度，needs-you 与 gate 闭环都键在它上面；只写该维度，不动 transport ACK（P0-1）。
2. SSE 新增 `member_run` / `member_action` / `team_message` 三种 upsert 帧 — 两路数据流实时性（缺陷②）。
3. transition 接受并持久化 `gate_note`（落 `team_runs.gate_note` 回 snapshot）— 过门留证据（缺陷③ + P2-10）。
4. SSE 新增 `team_run` upsert 帧 — run 行状态/completed_at/gate_note 的实时通路，gate 按钮在观看中点亮（P0-3）。
5. team messages 接受并持久化 `sender_kind`（`from_member_id` 可空=operator）— 操作者身份诚实，不对 Lead/host 冒名；旧 `/v1/messages` 通路已有该字段与 "never impersonating the Lead" 约定可参照（P1-6）。
6. `member_runs` waiting↔running 写入路径：adapter 在观测到 waiting_for_approval 时写 waiting；operator 的 control 送达该 member session 时翻转 — 否则 waiting 是死代码或常驻误报（P0-2）。

**可推迟（前端有诚实降级）**

7. `team_runs.title` — 前端必填 title 拼 objective 首行（P2-12，缺陷⑨）。
8. per-member `done_criteria` 结构化字段 — 前端 `## Done when` 小节约定解析（P1-5，缺陷④）。
9. budget `used` — 只显示 limit + "usage n/a (v0)"（缺陷⑤）。
10. snapshot 分页/cursor — Stage 6 量级全量可承受（缺陷⑥）。
11. DelegationRun 写入者 — drawer honest empty state（缺陷⑦）。
12. blocked/failed 成员状态写入 — UI 条件渲染兜底：无数据则该类红区项不出现（非死代码）；写入随 adapter 失败观测到来（缺陷⑧余量）。
13. delivery 自动重投端点 — v0 经 composer 预填手动重发（P1-7 第四类处理路径）。
14. correlation 配对视图（review_request↔result）— 字段 `correlation_id` 已在，配对 UI 推迟。

## 8. 明确不做什么（v0 边界）

- 不做 Task Graph / DAG 可视化；不做成员级 pause/resume/inject/interrupt；不做原始 provider 流面板（drawer Raw 兜底）；不做 per-member 第二通道；不做自动 re-plan；不做常驻组织/通讯录/跨 run 对比/预算仪表盘；列表不做搜索/筛选/分页；不做桌面通知（以上同 v1）。
- **不做 gate 外部事实自动校验**（自动拉 PR 合入状态/门重跑结果）：v0 的门 = min-conditions（derived）+ 证据包 + operator 署名 note（attested）（P2-10）。
- **不做 cockpit Current task 列**：无 task 实体可解析，纯噪声；列随 task 实体落地再恢复（P2-12）。
- **不做 delivery 自动重投**：手动重发已闭环（§7-13）。
- **不做 resolve 批量操作**：逐条拍板是责任设计，不是效率缺失。

## 9. 可追溯性

- 负责人要求 1（观看+简单操作）→ §2 重排（成员实时状态进首屏）、§6 点击 ≤3；要求 2（两模块嵌套）→ §2 结构；要求 3（两路数据流）→ §5；要求 4（逐波委派卡）→ §3 结构化契约表；要求 5（美术风格/Workflows 惯例）→ DocumentSurface/DocSection/tones/anchor 深链全沿用。
- 三个失败模式 → 上下文崩塌（首屏当前波+历史折叠）、责任悬空（契约表合并视图+handoff ACK 派生信号）、边界失守（决策四维分类+operator 身份+gate note 证据包）。
- 九缺陷 → §7 分档全覆盖；16 条质疑 → 消化总表逐条对应 §2–§7。
- types.ts 核实：`TeamMessage.evidence_refs/correlation_id/causation_id` 已存在（P2-9 纯前端）；`TeamMessageDelivery` 已含 failed/expired；`TeamRun` 无 title/gate_note（§7-3/7）；`MemberRun.current_task_id` 无 task 实体（§8）。

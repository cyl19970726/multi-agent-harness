# Agent Team 页面 Layout 终稿（v4 · 经三轮质疑-修订循环）

> 本文档 = v3 方案 + 终审 5 项必修 + P2 打包修订的合并终稿。
> 循环记录：v1（/tmp/layout-v1.md）→ 质疑 16 条 → v2（/tmp/layout-v2.md）→ 质疑 10 条 → v3（/tmp/layout-v3.md）→ 终审 5 项必修 → 本稿。
> 终审结论：**可开工**。后端按 §7 先行修订，前端按本稿实施，两线并行。

## 0. 关键判断

1. 取消四 Tab 改单页分区（模块嵌套 vs Tab 平级隐藏面板矛盾；观看优先要求首屏一屏看完当前波）。
2. **三维分离**：transport ACK（host 传输签收）/ 用户签收 ack（operator 确认收到交接）/ 用户拍板 resolve（decision 维度）——三条独立轨道，两个端点各管一条用户轨道。
3. 删除 member 横排卡片条（与 cockpit 表重复）。
4. **waiting 闭环 = 状态写入（⑥a 结果标记）+ 显式 re-drive（⑥b 全新会话）**，不做 parked session。
5. 波标题前端组合约定（title 拼 objective 首行，自动建议预填），`team_runs.title` 推迟。

## 1. 信息架构总图

| 层级 | 地址 | 区域 | 用户看什么 | 用户做什么 |
|---|---|---|---|---|
| L0 列表 | `?surface=team` | Team Runs 收件箱 | 所有 wave（lineage 树缩进）、状态/成员/needs-you/最后活动 | 打开波；创建 run |
| L1 详情 | `?team=<runId>` | 单波页面 | 成员在干什么、拍板项、契约/门/偏差 | 处理拍板、签收交接、发消息、过门、开波/分支/重试 |
| L1.5 消息深链 | `?team=<id>&msg=<messageId>` | 定位高亮消息卡 | needs-you 直达消息 | 就地 Decide/Answer/Ack |
| L1.6 成员深链 | `?team=<id>&teamMember=<memberRunId>` | 高亮 cockpit 行 + 自动开 drawer | waiting/blocked/failed 成员直达 | 就地发 control |
| L2 成员抽屉 | 无 URL（overlay，z-40） | Member drawer | 动作/委派/消息/Raw + current_task_id | 只读钻取 |

- selection 扩展（纯前端）：`teamMessageId`(param `msg`)、`teamMemberId`(param `teamMember`，避开 Agents 面已占用的 `member`)。
- **列表排序**：signals 只在**非终态 run** 上计数；根节点排序键 = 其 lineage 子树内最大 signals.total，平级按最后活动倒序；stitchLineage 缩进不变。

## 2. 详情页 layout（1440px，`DocumentSurface max-w-[1180px]`）

### 结构与取舍

单页链视图 + 每波一个 URL：URL 选中的波唯一展开；历史波一行折叠卡（不挂流）；stepper 保留链式上下文。放弃全展开长链（密度爆炸+历史挂流）。

### 波卡区域顺序（观看优先）

header（Entry/Exit 各一行点击展开）→ **AGENT TEAM 模块** → 契约表 → 集成门 → deviations。成员实时状态进首屏；契约/门/偏差作为参考区下沉。gate 区按状态变默认：running=一行计数（点击展开）、reviewing=展开、completed=一行结果。deviations 非空才展开。历史波（终态）同顺序、双栏静态渲染。

### 横切规则

- **needs-you 密度控制**：红区最多渲染 3 行 + "and N more decisions" 内联展开；头部恒显示总红数；整个 needs-you 可一键折叠为一行汇总（"⛔ 4 decisions · ⚠ 3 unacked"），折叠态红数保留。
- **overlay 层级**：member drawer z-40；DecisionDialog/GateDialog z-50（压 drawer）；Esc 只关最顶层（先 dialog 后 drawer）。
- **composer 收件人**：To chips 只列成员，**移除 host**（host 无消费循环，发它不驱动任何事）。
- **跨波注意力条**：当树内存在选中波以外的活跃波（或选中波为历史波）时，wave 模块上方显示："active waves: w3a · **w3b (2 decisions)** · Jump ↗"——红色计数，点击跳 `?team=<id>`；选中波自身为历史波时该行同时承担 historical banner 职责。

### ASCII 线框图

```
┌────────────────────────── max-w-[1180px] centered ──────────────────────────┐
│ ← AGENT TEAMS                                                               │
│ ┌─ HEADER ────────────────────────────────────────────────────────────────┐ │
│ │ TEAM RUN · WAVE 3                              [✉ New message] [▶ Start]│ │
│ │ real-device capabilities        ← wave title(=objective 首行, 自动建议) │ │
│ │ [running] [host: codex-app] run_019f… · created Jul 18 · budget $2.00   │ │
│ │ limit · usage n/a (v0)                                                  │ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
│ ┌─ NEEDS YOU（钉住; 仅非终态 run 计数; cap 3 行+N more; 可折叠成一行）─────┐ │
│ │ ⛔ 5 decisions waiting                                            [▾]   │ │
│ │  • [blocker] backend-worker · "deploy updates remote…"  [Decide ↗]      │ │
│ │  • [question] reviewer · "which account for NFC?"       [Answer ↗]      │ │
│ │  • [waiting] host-lead waiting for input                [Decide ↗]      │ │
│ │  and 2 more decisions…（内联展开: blocked 成员 / delivery failed 项）    │ │
│ │  ⚠ 3 deliveries unacked — hygiene        [view](→#external-flow 顶部)   │ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
│ ╔═ WAVE MODULE ═══════════════════════════════════════════════════════════╗ │
│ ║ active waves: w3 · w3b (2 decisions) · Jump ↗   ← 跨波注意力条(条件显示) ║ │
│ ║ chain: [w1✓]›[w2✓ ×2▾•1]›[▶w3]›[w4 planned]   [Retry wave]/[+ Start…]  ║ │
│ ║   selected=outline · active=fill+pulse · ×2▾=兄弟切换 · 红点=分支决策数  ║ │
│ ║ ┌─ WAVE 3 · real-device capabilities ─────────────────── [running] ──┐ ║ │
│ ║ │ ▸ Entry · ← w2 gate passed · operator · Jul 17 21:04 · note 引用   │ ║ │
│ ║ │ ▸ Exit  · gate min-conditions 2/3 · 1 open deviation   (点击展开)  │ ║ │
│ ║ ├─ AGENT TEAM MODULE (embedded) ─────────────────────────────────────┤ ║ │
│ ║ │ Cockpit — every member at a glance（行点击→drawer, z-40）           │ ║ │
│ ║ │ ┌────────────┬───────────────────────────┬─────────┬────────┬────┐ │ ║ │
│ ║ │ │ Member     │ Current action ⟳内部流     │ Runtime │ Status │Last│ │ ║ │
│ ║ │ │ ●F device  │ test_started 87/124       │ ●ready  │running │2s  │ │ ║ │
│ ║ │ │ ●host-lead │ control queued · resumes  │ ●idle   │waiting │1m  │ │ ║ │
│ ║ │ │            │ when host re-drives ↻     │         │        │    │ │ ║ │
│ ║ │ └────────────┴───────────────────────────┴─────────┴────────┴────┘ │ ║ │
│ ║ │  waiting 空窗诚实表达(⑥b); Current task 列 v0 隐藏                  │ ║ │
│ ║ │ ┌─ Internal flow ⟳ ────────────────┬─ External flow ✉ ────────────┐│ ║ │
│ ║ │ │ id=internal-flow                 │ id="external-flow"           ││ ║ │
│ ║ │ │ max-h 520 内滚 · 栏底对齐        │ max-h 520 内滚 · 栏底对齐    ││ ║ │
│ ║ │ │ filter [all members ▾]           │ ┌handoff F→host · 默认展开──┐││ ║ │
│ ║ │ │ #1287 F    test_progress ▸展开   │ │delivery: delivered · ev ×3│││ ║ │
│ ║ │ │  展开: summary+evidence badges   │ │[Ack receipt] ←条件渲染     │││ ║ │
│ ║ │ │              [Load more]         │ └──────────────────────────┘││ ║ │
│ ║ │ │                                  │ ┌blocker F→host · RESOLVED─┐││ ║ │
│ ║ │ │                                  │ │resolved·approved·operator│││ ║ │
│ ║ │ │                                  │ │· 14:31 · view reply ↗   │││ ║ │
│ ║ │ │                                  │ └──────────────────────────┘││ ║ │
│ ║ │ │                                  │ ┌control operator→backend──┐││ ║ │
│ ║ │ │                                  │ │"Approved: …"              │││ ║ │
│ ║ │ │                                  │ │↩ in reply to msg-41(锚链)│││ ║ │
│ ║ │ │                                  │ └──────────────────────────┘││ ║ │
│ ║ │ │                                  │ ├ composer(固定栏底,滚动区外)┤│ ║ │
│ ║ │ │                                  │ │ from: operator(固定)      │││ ║ │
│ ║ │ │                                  │ │ Kind▾ To[成员chips,无host]│││ ║ │
│ ║ │ │                                  │ │ [textarea]         [Send] │││ ║ │
│ ║ │ │                                  │ └──────────────────────────┘││ ║ │
│ ║ │ └──────────────────────────────────┴──────────────────────────────┘│ ║ │
│ ║ │  <1100px: 双栏堆叠, external 在上                                   │ ║ │
│ ║ ├─ Member contract（## 小节解析; 行可展开; Delegate=全部 assignment合并)┤ ║ │
│ ║ │ Member   │ Delegate        │ Done when      │ Boundaries           │ ║ │
│ ║ │ ●F device│ "## Task NFC…"✉#│ "## Done when   │ "## Boundaries…" +   │ ║ │
│ ║ │          │  ▸ expand full │  owner signoff"  │ owned_paths+⛔政策行 │ ║ │
│ ║ │ 无小节消息: Delegate 引全文, 其余列 "not stated in assignment"       │ ║ │
│ ║ ├─ Integration gate ── running:一行计数(点击展开)/reviewing:展开 ──────┤ ║ │
│ ║ │ min-conditions (derived, muted 小标):                               │ ║ │
│ ║ │ ✓ all members terminal · ✗ 1 unacked handoff(msg-42↗, 键 operator  │ ║ │
│ ║ │   ack 维度) · ✓ no open blockers(键 resolved 维度)                  │ ║ │
│ ║ │ 提示: waiting/blocked members must be re-driven to a terminal      │ ║ │
│ ║ │  state before the gate can pass                                     │ ║ │
│ ║ │ [Complete gate…] → Gate 对话框(证据包+预填草稿 note; 提交 {status,  │ ║ │
│ ║ │  note})                                                             │ ║ │
│ ║ ├─ Deviations（非空才展开）───────────────────────────────────────────┤ ║ │
│ ║ │ • [unacked handoff] media→host "…" ✉# · [failed action] test_…     │ ║ │
│ ║ └────────────────────────────────────────────────────────────────────┘ ║ │
│ ║ Other waves (collapsed · 一行摘要 · 不挂流):                              ║ │
│ ║ ▸ wave 2 · "data & E2E" · completed · gate:"all lanes merged…" · 1 dev   ║ │
│ ╚══════════════════════════════════════════════════════════════════════════╝ │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 默认状态/空态/离线态

- 终态 run 的 needs-you：不渲染红区，改 muted 一行 "N decisions were never resolved (historical)"。
- waiting 成员 cockpit 行：operator 已发 control 后显示 "control queued · resumes when host re-drives"（诚实空窗）。
- 无 structured assignment 的成员：Done when/Boundaries 列显示 "not stated in assignment"（muted，诚实）。
- 空态复用现有 EmptyState；离线态写操作禁用 + tooltip；freshness chip 三态（现有）。

## 3. Wave 卡设计

### 进入/退出条件

- Entry 一行：`← w{N−1} gate passed · operator · {completed_at} · "{gate_note}"`，点击展开完整引用 + `[view w{N−1}]`；wave 1 = "Created by operator · created_at · host_surface"。派生行挂 `derived` 小标。
- Exit 一行：`gate min-conditions {x}/3 · {n} open deviations`，点击展开三条 checklist（整体挂 muted **"derived minimum conditions"** 小标——内部卫生下限，不冒充外部事实门）：① all members terminal（member_runs.status）；② no unacked handoffs（**键 operator ack 维度**）；③ no open blockers（**键 resolved 维度**）。失败项内联深链。

### Member 契约表

- **小节约定是协议层契约**：`## Task` / `## Done when` / `## Boundaries` 写进 MCP `send_message` 工具描述与 team-orchestrator Skill——主路径（host 经 MCP/CLI 发 assignment）也产出同样小节；**前端 parser 是唯一渲染器**，双轨合一。
- **parse-back 幂等规则（写死）**：body 已含小节 → 按小节拆回三个输入框，**禁止重复包裹**；同名小节出现多次**取第一**；小节之外的文本归入 Task 框。
- 创建/下一波对话框 per-member 三段输入（默认折叠/下一波默认展开并 parse-back）；提交时组合小节发 assignment（from operator）。
- 渲染：Delegate 列=该成员全部 assignment 合并视图（逐条 ✉# 深链、行可展开全文）；无小节 → 其余列 "not stated in assignment"；Boundaries 列=解析文本 + `owned_paths` badges（空=read-only）+ ⛔政策行（deploy/merge/remote-delete 需用户拍板，标 `policy`）。
- **契约协议补充（Reject 收口）**：协议层（MCP 工具描述 + Skill + contract_prompt）补一条——授权请求被用户 Reject 后，相关成员不得原地等待：下一轮必须报 `## RESULT: done` 并在 SUMMARY 记录 deviation（被拒事项+原因），让偏差进入 re-plan 回路。

### 集成门

- 可点条件：reviewing + min-conditions 全绿。固定提示文案（写死）："waiting/blocked members must be re-driven to a terminal state before the gate can pass"。
- Gate 对话框：上半证据包（本波 handoff 清单 evidence badges+✉深链 + objective 原文 + 各成员 `## Done when` 引用）；下半 note 必填、预填自动草稿（checklist 状态+deviations+handoff 清单拼装，用户改写）。
- 提交：`POST /v1/team-runs/{id}/transition`，body `{status:"completed", note}`，按 §7 通则 D 合并写落库。过门后一行结果（gate passed · operator · completed_at · note 引用），note 同时被下一波 Entry 引用。

### 多波分支

- `waveTree(runs)`：childrenByParent 全量建树；**主链**=从根沿"含最新活跃波"的分支，活跃=非终态，并列时**写死 `run.updated_at`** 决胜。
- stepper：selected=outline、active=fill+pulse；多 child 节点 `×N▾` 角标 + **红色 decisions 计数**（该分支各波合计）；菜单列分支末端可跳。
- 开波按钮三态：非终态 → 无按钮（一行原因提示）；=主链链尾且 completed → `Start wave N+1`；completed 非链尾 → `Branch new wave from wave N`；**终态非 completed（failed/cancelled）→ `Retry as new wave from wave N`**（兄弟重试，预填该波 deviations）。
- 三种对话框 `wave_index` 一律**预填 = 选中波 wave_index + 1**（可改），stepper 不出现同号双 wave。

## 4. 拍板项设计

### 三条轨道的语义（终稿）

| 轨道 | 字段 | 写入端点 | 语义 |
|---|---|---|---|
| transport ACK | `deliveries[].status` | host runtime（非本前端） | host agent 传输签收，秒级，不含人意 |
| 用户签收 ack | host delivery → acknowledged | `POST …/messages/{id}/ack`（窄：仅非 decision 类 kind，其余 409） | operator 确认收到交接；checklist② 与琥珀区键它 |
| 用户拍板 resolve | `resolved_at/by/resolution` | `POST …/messages/{id}/resolve`（仅 kind ∈ {blocker, review_request, question}，其余 409） | 人对 decision 类消息的裁决；needs-you 与 checklist③ 键它 |

同卡规则：(a) **resolve 级联**：写 `resolved_*` 的同时把该消息 host delivery 置 acknowledged——处理蕴含签收，数据自洽，琥珀区免特判、计数随 Decide 自然下降。(b) **kind 域共享常量**：kind→域映射前后端同源；新增 kind 默认两端点都 409，显式归类后才开放。(c) handoff 卡 ack 后 delivery 行渲染 "acknowledged · operator · {time}"（good 绿色态）。

### decisions（红区）= 四类 + run 状态作用域

仅当 `run.status ∈ {planning, running, waiting, reviewing}` 时计数；终态 → muted 历史行，不占排序权重。

| 类别 | 判定 | 处理路径 |
|---|---|---|
| decision 消息 | kind∈{blocker,review_request,question} ∧ 发向 host ∧ `resolved_at IS NULL` | Decide/Answer ↗ → `?msg=` 定位 → 对话框 → Confirm（send→resolve） |
| waiting 成员 | status==="waiting" ∧ 无更新的 operator control | Decide ↗ → `?teamMember=` 高亮+drawer → 预填 control 对话框 → Confirm |
| blocked/failed 成员 | status∈{blocked,failed} | View ↗ → 成员深链开 drawer；可选 nudge control |
| delivery 失败 | decision 流消息（含 operator 的 decision 回复）delivery∈{failed,expired} | View ↗ → 消息卡 → composer 预填手动重发 |

去重规则：未 resolve 且 delivery failed 的 decision 消息只在第一类出现一次、带 "delivery failed" 角标；第四类只兜"已 resolve 但决定未送达"。waiting 的"已处理"代理规则（标 derived）：存在 operator 发往该成员的 control 且 `created_at > member.last_event_at` → 出红区、cockpit 显示 queued 空窗文案；成员下一事件到达或 re-drive 后规则自然失效。

### waiting 闭环（终稿）

- 出现依据（⑥a）：契约 `## RESULT: waiting` 结果标记 + turn 终了写 `status=waiting`。
- 处理路径：Decide → control 对话框 → Confirm 发送 → 出红区，cockpit 显示 "control queued · resumes when host re-drives" → **Re-drive now 为 inline 二次确认**（"Confirm re-drive? This starts a new member session and spends budget. [Confirm] [Cancel]"）。
- **Re-drive 按钮渲染条件（守卫表 UI 面）**：仅 `member.status ∈ {waiting, blocked}` 时出现；idle 守卫放行但 UI 不主动提供；run 终态时整波只读、按钮不出现。completed run 不可能有 waiting 成员（checklist① 结构排除）。
- **⑥a 落地前的降级叙事**：⑥a 上线前，等审批的成员以 **blocked** 呈现（现有 `MemberRoundResult::Blocked` 写入路径；旧协议报 `## RESULT: blocked`），处理路径相同——blocked 非终态、⑥b 守卫允许 re-drive——**waiting 不出现不是 bug**。release note 写明出现时间线：⑥a 上线前红区只有 blocked 项；上线后 waiting 项开始进入红区，并存期属预期。
- 上下文丢失缓解（re-brief）：re-drive 首轮 prompt = control 正文 + 该成员 assignment 契约引用（小节在盘上可复读）。

### Decide 双向链接

- Decide/Answer 的 send 带 `causation_id = 原消息.id`；回复卡渲染 "↩ in reply to msg-41"（锚链 `?msg=`）；原卡 resolved 行渲染 "resolved · approved · operator · 14:31 · **view reply ↗**"。
- 提交顺序：send → resolve；resolve 失败 toast 明示 + 仅重试 resolve。
- 按钮渲染条件：仅 decision 类 ∧ 发向 host ∧ 未 resolve 显示 [Approve][Reject]/[Answer]；handoff 卡仅当 host delivery 未 ack 显示 [Ack receipt]。

### 身份

composer 固定 from=operator（`sender_kind` 持久化，from_member_id 空）；operator = "operator (you)" + decision 紫 pill；To chips 无 host。

### 密度

红区 cap 3 行 + "and N more decisions" 内联展开；头部恒显示总数；全区可折叠成一行汇总（红数保留）。

## 5. 两路数据流的实时性设计（一个事实源）

架构：`/v1/events` → 命名帧 → `applyFrame`（upsertById 进同一份 snapshot）→ 唯一 store → 渲染期投影。

- **帧清单**：现有 `team_run_event`；新增 `member_run` / `member_action` / `team_message` / `team_run` 四种 upsert 帧。消费映射：`member_run` → cockpit/needs-you（waiting 翻转、re-drive 后 running 回来）；`team_message` → ledger/needs-you/ack/resolve 结果；`team_run` → gate 按钮点亮、completed_at/gate_note；`member_action` → internal lane/Current action。
- **单订阅原则**：仅 app 级一条流；成员视图全部渲染期 `filter` 投影，禁止 per-member 流/fetch。
- **降级自愈**：polling fallback + 退避重连 + snapshot resync（现有）；写操作 POST→刷新 snapshot 兜底。
- **合并写与 UI**：operator 过门与 orchestrator 终态写并发时，后端合并写纪律（§7-D）保证门不回归；UI 信任 `team_run` 帧的最新合并结果，不做本地乐观锁。

## 6. 操作清单与点击路径

| 操作 | 路径 | 点击数 |
|---|---|---|
| 创建 run | `New Team Run`(1) → title 自动建议（objective 首行截 60 字预填可改）+ members（行级折叠）→ `Create`(2)；有契约的成员自动发 assignment | **2** + 表单 |
| 发消息 | composer 常驻栏底（from=operator、To 无 host）→ `Send`(1) | **1** + 表单 |
| 处理 decision 消息 | `Decide ↗`(1) → 消息卡 `Approve`(2) → 对话框 `Confirm`(3)（send 带 causation_id → resolve，失败仅重试 resolve） | **3** |
| 签收交接 | handoff 卡 `Ack receipt`(1) | **1** |
| 处理 waiting 成员 | `Decide ↗`(1) → 对话框 `Confirm`(2)；可选 `Re-drive now`(3) → inline `Confirm`(4) | **3–4** |
| 过集成门 | `Complete gate…`(1) → 证据包+草稿 note 改写 → `Confirm`(2)（body `{status,note}`） | **2** |
| 开下一波 / 分支 / 重试 | 链尾或节点按钮(1)（Start/Branch/Retry 三态；wave_index 预填 = 原+1 可改） → 对话框（title 自动建议、契约 parse-back 默认展开、deviations hints） → `Create`(2) | **2** |
| 看历史波证据 | 折叠卡(1) → 只读波页（注意力条带活跃波红数） → 可选 drawer 钻取（+1–2） | **1–3** |

## 7. 后端最小补充清单（后端工作包先行修订，与前端并行）

**必须有**

1. `POST …/messages/{id}/resolve` + `team_messages.resolved_at/resolved_by/resolution` — 用户拍板维度；写 `resolved_*` 时**级联**把 host delivery 置 acknowledged；仅接受 kind ∈ {blocker, review_request, question}，其余 409。
2. `POST …/messages/{id}/ack`（窄：仅非 decision 类 kind，decision 类 409）— handoff 签收的写入方；checklist② 与琥珀区键它。两端点共用同一 kind 域常量；新 kind 默认双端关闭、显式归类后开放。
3. SSE upsert 帧 ×3：`member_run` / `member_action` / `team_message`。
4. transition 接受 `{status, note}` 并持久化 `gate_note`。
5. SSE upsert 帧：`team_run`。
6. team messages 接受并持久化 `sender_kind`（from_member_id 可空=operator）。
7. ⑥a waiting 状态写入：契约 `## RESULT: waiting` 结果标记 + turn 终了写 `member_runs.status=waiting`。
8. ⑥b re-drive 端点：`POST /v1/team-runs/{id}/members/{memberId}/start`（或 CLI/MCP 触发），以该成员最新 operator control 为首轮 prompt 起全新会话。守卫表：

   | 条件 | 结果 |
   |---|---|
   | `run.status ∈ {completed, cancelled}` | 409（终态吸收） |
   | `run.status === planning` | 放行，视同该成员首次 start |
   | `member.status ∈ {waiting, blocked, idle}` | 放行 |
   | `member.status ∈ {running, starting}` | 409 "member is live" |
   | `member.status ∈ {completed, failed, stopped}` | 409（终态吸收） |

9. messages 端点接受可选 `causation_id`（字段已在，放开硬编码 None）。

**后端通则 D · 合并写纪律（三实体版）**：

- `team_runs` / `member_runs`：终态吸收——Completed/Cancelled 不可回归、`completed_at`/`gate_note` 不可擦除；orchestrator 终态写前 read-merge-write。
- `team_messages`：三个并发写入方（resolve 写 `resolved_*`、ack 写 deliveries、orchestrator `mark_message_delivered` 整行 clone append）必须 read-merge-write：append 前重读该行最新版本；deliveries 按 `member_id` 逐条合并（不整组覆盖）；`resolved_*` 只能由 resolve 端点写入，其他写入方原样携带、不得置空。

**联调顺序（写死）**：先上 ② ack 端点 + ③ SSE 三帧（⑤ `team_run` 帧建议同行），再开 gate 交互（④ transition `{status,note}` + 前端 Complete gate 按钮）——避免"门可点但状态不实时 / 签收无写入方"的中间态误判。① resolve、⑦ ⑥a、⑧ ⑥b、⑨ causation_id 与前端并行开发，按 §4 降级叙事灰度上线。

**可推迟（前端有诚实降级）**

- `team_runs.title`（前端自动建议+组合进 objective 首行）；per-member `done_criteria` 字段（`## Done when` 小节约定）；budget used；snapshot 分页；DelegationRun 写入者；delivery 自动重投（composer 预填手动重发）；correlation 配对视图（字段已在）；**parked session / ACP session-load resume**（serve 常驻+会话挂起是大项目；v0 用 re-drive 全新会话模型）。
- ~~blocked/failed 状态写入~~ **已存在**（`MemberRoundResult::Blocked` → status=Blocked；`journal_member_failure` → Failed），无后端工作，UI 红区直接消费。

## 8. 明确不做什么（v0 边界）

- 不做 Task Graph / DAG 可视化；不做成员级 pause/resume/inject/interrupt；不做原始 provider 流面板（drawer Raw 兜底）；不做 per-member 第二通道；不做自动 re-plan；不做常驻组织/通讯录/跨 run 对比/预算仪表盘；列表不做搜索/筛选/分页；不做桌面通知；gate 不做外部事实自动校验（v0 的门 = min-conditions + 证据包 + operator 署名 note）；cockpit 无 Current task 列；无批量 resolve。
- **不做 parked session**：成员等待态=re-drive 全新会话（契约在盘上+re-brief），不假装"原会话挂起唤醒"。
- **不给 host 发消息**：composer 无 host 收件人；operator 对 host 编排器的指示通道不在 v0。
- **不做跨波红区合并**：跨分支可见性=注意力条+角标计数+跳链，不把多波 decisions 合并进单一红区（波是拍板的天然作用域）。
- **不做 waiting 自动 re-drive**：re-drive 是显式人工动作（真实起会话烧预算），UI 侧同样要求 inline 二次确认。

## 9. 可追溯性

- 三轮质疑 26+10+6 条 → 各轮消化总表；九缺陷 → §7 分档；负责人五要求 → §2/§3/§5/§6；三失败模式 → §3 契约表、§4 三轨道与四类 decisions、§3 门证据包；Stage 6 场景 → waveTree 分支/重试、契约 owned_paths、门 checklist。
- types.ts 核实：`TeamMessage.evidence_refs/correlation_id/causation_id` 已存在（§7-⑨ 仅放开写入）；`TeamMessageDelivery` 含 failed/expired；`TeamRun` 无 title/gate_note。
- 代码实证采纳：`MemberUpdateMapper` 仅映射 progress/tool、`KimiAcpClient` 无 session/load、`send_team_message` 落库即 Queued、`team_run_start` 一次性进程——⑥ 拆分与 re-drive 语义据此定稿。

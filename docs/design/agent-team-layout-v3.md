# Agent Team 页面 Layout 设计方案 v3（实施蓝图）

## 第二轮质疑消化总表

| # | 状态 | 怎么改的（落点） |
|---|---|---|
| 新 P0-1 后端⑥无落地基座 | 已消化 | ⑥拆 ⑥a/⑥b：⑥a = 契约协议加 `## RESULT: waiting` 结果标记 + 终态写 status=waiting（红区出现有依据）；⑥b = 显式 re-drive 端点（`POST /v1/team-runs/{id}/members/{memberId}/start`，或 CLI/MCP 触发），control 消息作首轮 prompt；UI 诚实表达异步空窗（"control queued · resumes when host re-drives"）；parked session 明确推迟进 §8（§4、§5、§7-⑦⑧、§8） |
| 新 P0-2 handoff ACK 写入方消失 | 已消化 | 恢复窄 ack 端点 `POST …/messages/{id}/ack`（仅非 decision 类消息的 host delivery，语义=operator 确认收到交接）；checklist② 与琥珀区键它；resolve（拍板）与 ack（签收）并存、职责唯一；handoff 卡加条件渲染的 [Ack receipt] 按钮（§3、§4、§7-②） |
| 新 P0-3 旧数据僵尸 decisions | 已消化 | decisions 计算加 run 状态作用域：仅 run 非终态（planning/running/waiting/reviewing）计数；终态 run 渲染 muted 一行 "N decisions were never resolved (historical)"，不进红区、列表排序权重为 0（§1、§4） |
| 新 P1-4 waveTree 两洞 | 已消化 | ① 终态非 completed 的波显示 "Retry as new wave from wave N"（兄弟重试，文案与 Branch 区分，预填 deviations）；② ×N 角标带红色 decisions 计数；提示条改为全树活跃波集合（"active waves: w3a · w3b (2 decisions) · Jump ↗"）；主链 tie-break 写死 `run.updated_at`（§2、§3） |
| 新 P1-5 run 行 latest-wins 竞态 | 已消化 | §7 加"后端通则·合并写纪律"：终态吸收（Completed/Cancelled 不可回归、completed_at/gate_note 不可擦除）；orchestrator 终态写前 read-merge-write（§7-D） |
| 新 P1-6 Decide 回复无双向链接 | 已消化 | messages 端点接受可选 `causation_id`（字段已在，仅放开硬编码 None）；Decide 的 send 带 `causation_id=原消息.id`；回复卡渲染 "↩ in reply to msg-41" 锚链，原卡 resolved 行加 "view reply ↗"（§4、§7-⑨） |
| 新 P1-7 契约三段双轨质量 | 已消化 | 小节约定上移协议层：`## Task/## Done when/## Boundaries` 写进 MCP send_message 工具描述与 team-orchestrator Skill（前端 parser 保持唯一渲染器）；parse-back 幂等规则写死（已含小节按小节拆回、禁止重复包裹、同名多小节取第一、节外文本归 Task 框）；下一波对话框契约默认展开并 parse-back（§3、§6） |
| 新 P2-8 红区无 cap 压出首屏 | 已消化 | 红区 cap 3 行 + "and N more decisions" 内联展开（头部恒显示总数）；needs-you 全区可一键折叠为一行汇总（红数保留）（§2 线框图、§4） |
| 新 P2-9 title 必填摩擦+表单膨胀 | 已消化 | title 自动建议（objective 首行截 60 字预填、可改、非空校验）；契约三段创建时默认折叠（下一波默认展开）；成员卡行级折叠（默认 name/role 一行摘要）（§6） |
| P2-10 四小项 | 已消化 | ① transition 参数统一 `{status, note}`（§7-④）；② 主链"最新活跃"写死 `run.updated_at`（§3）；③ overlay z 层级：drawer z-40、对话框 z-50，Esc 先关顶层（§2）；④ composer 收件人移除 host（无消费循环），§8 注明"不给 host 发消息"（§2、§8） |

无不采纳项。实施判断同质疑官：前端可开工，后端工作包（resolve 作用域、补回 handoff ACK、⑥ 拆分、合并写纪律）先出修订，两线并行。

## 0. 关键判断与遗留确认

1. 取消四 Tab 改单页分区（同 v2）。
2. **三维分离**：transport ACK（host 传输签收）/ 用户签收 ack（operator 确认收到交接）/ 用户拍板 resolve（decision 维度）——三条独立轨道，两个端点各管一条用户轨道（新 P0-2）。
3. 删除 member 横排卡片条（同 v2）。
4. **waiting 闭环 = 状态写入（⑥a 结果标记）+ 显式 re-drive（⑥b 全新会话）**，不做 parked session（新 P0-1）。
5. 波标题前端组合约定（title 拼 objective 首行），`team_runs.title` 推迟（同 v2，v3 改自动建议降低摩擦）。

## 1. 信息架构总图

| 层级 | 地址 | 区域 | 用户看什么 | 用户做什么 |
|---|---|---|---|---|
| L0 列表 | `?surface=team` | Team Runs 收件箱 | 所有 wave（lineage 树缩进）、状态/成员/needs-you/最后活动 | 打开波；创建 run |
| L1 详情 | `?team=<runId>` | 单波页面 | 成员在干什么、拍板项、契约/门/偏差 | 处理拍板、签收交接、发消息、过门、开波/分支/重试 |
| L1.5 消息深链 | `?team=<id>&msg=<messageId>` | 定位高亮消息卡 | needs-you 直达消息 | 就地 Decide/Answer/Ack |
| L1.6 成员深链 | `?team=<id>&teamMember=<memberRunId>` | 高亮 cockpit 行 + 自动开 drawer | waiting/blocked/failed 成员直达 | 就地发 control |
| L2 成员抽屉 | 无 URL（overlay，z-40） | Member drawer | 动作/委派/消息/Raw + current_task_id | 只读钻取 |

- selection 扩展（纯前端）：`teamMessageId`(param `msg`)、`teamMemberId`(param `teamMember`，避开 Agents 面已占用的 `member`)。
- **列表排序（v3 修订）**：signals 只在**非终态 run** 上计数（新 P0-3）；根节点排序键 = 其 lineage 子树内最大 signals.total，平级按最后活动倒序；stitchLineage 缩进不变。

## 2. 详情页 layout（1440px，`DocumentSurface max-w-[1180px]`）

### 结构与取舍（同 v2，结论不变）

单页链视图 + 每波一个 URL；URL 选中的波唯一展开；历史波一行折叠卡（不挂流）。

### 波卡区域顺序（同 v2：观看优先）

header（Entry/Exit 各一行点击展开）→ **AGENT TEAM 模块** → 契约表 → 集成门 → deviations。

### 新增横切规则（v3）

- **needs-you 密度控制（新 P2-8）**：红区最多渲染 3 行 + "and N more decisions" 内联展开；头部恒显示总红数；整个 needs-you 可一键折叠为一行汇总（"⛔ 4 decisions · ⚠ 3 unacked"），折叠态红数保留。
- **overlay 层级（P2-10③）**：member drawer z-40；DecisionDialog/GateDialog z-50（压 drawer）；Esc 只关最顶层（先 dialog 后 drawer）。
- **composer 收件人（P2-10④）**：To chips 只列成员，**移除 host**（host 无消费循环，发它不驱动任何事）。
- **跨波注意力条（新 P1-4②）**：当树内存在选中波以外的活跃波（或选中波为历史波）时，wave 模块上方显示："active waves: w3a · **w3b (2 decisions)** · Jump ↗"——红色计数，点击跳 `?team=<id>`；选中波自身为历史波时该行同时承担 v2 的 historical banner 职责。

### ASCII 线框图（v3 更新版）

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
│ ║ │  waiting 空窗诚实表达(新 P0-1⑥b); Current task 列 v0 隐藏           │ ║ │
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
│ ║ │   ack 维度, 新 P0-2) · ✓ no open blockers(键 resolved 维度)         │ ║ │
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

### 默认状态/空态/离线态（增量）

- 终态 run 的 needs-you：不渲染红区，改 muted 一行 "N decisions were never resolved (historical)"（新 P0-3）。
- waiting 成员 cockpit 行：operator 已发 control 后显示 "control queued · resumes when host re-drives"（新 P0-1⑥b 的诚实空窗）。
- 其余同 v2。

## 3. Wave 卡设计

### 进入/退出条件（v3 仅措辞修订）

- Entry/Exit 各一行点击展开（同 v2）。
- Exit checklist 三条，整体挂 muted **"derived minimum conditions"** 小标；键维度明确（新 P0-1/P0-2 后）：① all members terminal（member_runs.status）；② no unacked handoffs（**键 operator ack 维度**：handoff 的 host delivery 被 operator 经 ack 端点确认）；③ no open blockers（**键 resolved 维度**：decision 类消息 resolved_at IS NULL）。

### Member 契约表（v3：协议层归属上移，新 P1-7）

- **小节约定是协议层契约**（不再是前端私有格式）：`## Task` / `## Done when` / `## Boundaries` 写进 MCP `send_message` 工具描述与 team-orchestrator Skill——主路径（host 经 MCP/CLI 发 assignment）也产出同样小节；**前端 parser 是唯一渲染器**，双轨合一。
- **parse-back 幂等规则（写死）**：body 已含小节 → 按小节拆回三个输入框，**禁止重复包裹**；同名小节出现多次**取第一**；小节之外的文本归入 Task 框。
- 创建/下一波对话框的 per-member 三段输入（默认折叠/下一波默认展开并 parse-back 上一波）不变；提交时组合小节发 assignment（from operator）。
- 渲染：Delegate 列=该成员全部 assignment 合并视图（逐条 ✉# 深链、行可展开全文）；无小节 → 其余列 "not stated in assignment"；Boundaries 列=解析文本 + `owned_paths` badges（空=read-only）+ ⛔政策行（deploy/merge/remote-delete 需用户拍板，标 `policy`）。

### 集成门（v3：端点参数与合并写）

- 可点条件不变（reviewing + min-conditions 全绿——②现在有写入方，门真的可以点亮，新 P0-2）。
- Gate 对话框：上半证据包（本波 handoff 清单 evidence badges+✉深链 + objective 原文 + 各成员 `## Done when` 引用）；下半 note 必填、预填自动草稿（checklist 状态+deviations+handoff 清单拼装，用户改写）。
- 提交：`POST /v1/team-runs/{id}/transition`，**body 统一 `{status:"completed", note}`**（P2-10①，避免实施漂移）；后端按**合并写纪律**落库（§7-D）。

### 多波分支（v3 补洞，新 P1-4）

- `waveTree(runs)`：childrenByParent 全量建树；**主链**=从根沿"含最新活跃波"的分支，活跃=非终态，并列时**写死 `run.updated_at`** 决胜（P2-10②）。
- stepper：selected=outline、active=fill+pulse；多 child 节点 `×N▾` 角标 + **红色 decisions 计数**（该分支各波 decisions 合计）；菜单列分支末端可跳。
- 开波按钮规则（按选中波状态）：非终态 → 无按钮（一行原因提示）；=主链链尾且 completed → `Start wave N+1`；completed 非链尾 → `Branch new wave from wave N`；**终态非 completed（failed/cancelled）→ `Retry as new wave from wave N`**（兄弟重试，预填该波 deviations——最常用 re-plan 入口不再消失）。

## 4. 拍板项设计（v3 重写核心规则）

### 三条轨道的语义（新 P0-1/P0-2 定稿）

| 轨道 | 字段 | 写入端点 | 语义 |
|---|---|---|---|
| transport ACK | `deliveries[].status` | （host runtime，非本前端） | host agent 传输签收，秒级，不含人意 |
| 用户签收 ack | host delivery → acknowledged | `POST …/messages/{id}/ack`（窄：仅非 decision 类） | operator 确认收到交接；checklist② 与琥珀区键它 |
| 用户拍板 resolve | `resolved_at/by/resolution` | `POST …/messages/{id}/resolve` | 人对 decision 类消息的裁决；needs-you 与 checklist③ 键它 |

### decisions（红区）= 四类 + run 状态作用域（新 P0-3）

仅当 `run.status ∈ {planning, running, waiting, reviewing}` 时计数；终态 → muted 历史行，不占排序权重。

| 类别 | 判定 | 处理路径 |
|---|---|---|
| decision 消息 | kind∈{blocker,review_request,question} ∧ 发向 host ∧ `resolved_at IS NULL` | Decide/Answer ↗ → `?msg=` 定位 → 对话框 → Confirm（send→resolve） |
| waiting 成员 | status==="waiting" ∧ 无更新的 operator control（见下） | Decide ↗ → `?teamMember=` 高亮+drawer → 预填 control 对话框 → Confirm |
| blocked/failed 成员 | status∈{blocked,failed} | View ↗ → 成员深链开 drawer；可选 nudge control |
| delivery 失败 | decision 流消息（含 operator 的 decision 回复）delivery∈{failed,expired} | View ↗ → 消息卡 → composer 预填手动重发 |

去重规则：未 resolve 且 delivery failed 的 decision 消息只在第一类出现一次、带 "delivery failed" 角标；第四类只兜"已 resolve 但决定未送达"。waiting 的"已处理"代理规则（标 derived）：存在 operator 发往该成员的 control 且 `created_at > member.last_event_at` → 出红区、cockpit 显示 queued 空窗文案；成员下一事件到达或 re-drive 后规则自然失效。

### waiting 闭环（新 P0-1 定稿）

- 出现依据（⑥a）：member 契约含 `## RESULT: waiting` 结果标记（或 mapper 观测 waiting 类 update），turn 终了 harness 写 `status=waiting`——红区 waiting 项不再无源。
- 处理路径（⑥b）：Decide → control 对话框（to=该成员，from=operator）→ Confirm 发送（落库 queued）→ 成员出红区，cockpit 行显示 "control queued · resumes when host re-drives"；operator 可点 **Re-drive now**（`POST /v1/team-runs/{id}/members/{memberId}/start`，或 CLI `team-run start --member <id>`）显式重驱，control 作首轮 prompt；新会话写 running 经 `member_run` 帧回到 UI。**诚实口径**：re-drive 是全新会话（worktree 在盘上、上下文靠 re-brief），不是原会话唤醒。
- 上下文丢失缓解（re-brief）：re-drive 的首轮 prompt = control 正文 + 该成员 assignment 契约引用（`## Task/## Done when/## Boundaries` 小节在盘上可复读）。

### Decide 双向链接（新 P1-6）

- Decide/Answer 的 send 带 `causation_id = 原消息.id`；回复卡渲染 "↩ in reply to msg-41"（锚链 `?msg=`）；原卡 resolved 行渲染 "resolved · approved · operator · 14:31 · **view reply ↗**"（经 causation_id 反查）。
- 提交顺序（P3-14 保留）：send → resolve；resolve 失败 toast 明示 + 仅重试 resolve。
- 按钮渲染条件（同 v2）：仅 decision 类 ∧ 发向 host ∧ 未 resolve 显示 [Approve][Reject]/[Answer]；handoff 卡仅当 host delivery 未 ack 显示 [Ack receipt]（新 P0-2）。

### 身份（同 v2）

composer 固定 from=operator（`sender_kind` 持久化，from_member_id 空）；operator = "operator (you)" + decision 紫 pill；**To chips 无 host**（P2-10④）。

### 密度（新 P2-8）

红区 cap 3 行 + "and N more decisions" 内联展开；头部恒显示总数；全区可折叠成一行汇总（红数保留）。

## 5. 两路数据流的实时性设计（一个事实源）

架构不变：`/v1/events` → 命名帧 → `applyFrame`（upsertById 进同一份 snapshot）→ 唯一 store → 渲染期投影。

- **帧清单**：现有 `team_run_event`；新增 `member_run` / `member_action` / `team_message` / `team_run` 四种 upsert 帧。消费映射：`member_run` 帧 → cockpit/needs-you（waiting 翻转、re-drive 后 running 回来）；`team_message` 帧 → ledger/needs-you/ack/resolve 结果回来；`team_run` 帧 → gate 按钮点亮、completed_at/gate_note；`member_action` 帧 → internal lane/Current action。
- **单订阅原则**：仅 app 级一条流；成员视图全部渲染期 `filter` 投影。
- **降级自愈**：polling fallback + 退避重连 + snapshot resync（现有）；写操作 POST→刷新 snapshot 兜底。
- **合并写与 UI**：operator 过门与 orchestrator 终态写并发时，后端合并写纪律（§7-D）保证门不回归；UI 信任 `team_run` 帧的最新合并结果，不做本地乐观锁。

## 6. 操作清单与点击路径（v3 更新）

| 操作 | 路径 | 点击数 |
|---|---|---|
| 创建 run | `New Team Run`(1) → title 自动建议（objective 首行截 60 字预填可改）+ members（行级折叠，默认 name/role 一行）→ `Create`(2)；有契约的成员自动发 assignment | **2** + 表单 |
| 发消息 | composer 常驻栏底（from=operator、To 无 host）→ `Send`(1) | **1** + 表单 |
| 处理 decision 消息 | `Decide ↗`(1) → 消息卡 `Approve`(2) → 对话框 `Confirm`(3)（send 带 causation_id → resolve，失败仅重试 resolve） | **3** |
| 签收交接（新 P0-2） | handoff 卡 `Ack receipt`(1) | **1** |
| 处理 waiting 成员 | `Decide ↗`(1) → drawer/对话框 `Confirm`(2)；可选 `Re-drive now`(3) | **2–3** |
| 过集成门 | `Complete gate…`(1) → 证据包+草稿 note 改写 → `Confirm`(2)（body `{status,note}`） | **2** |
| 开下一波 / 分支 / 重试 | 链尾或节点按钮(1)（Start/Branch/Retry 三态） → 对话框（title 自动建议、契约 parse-back 默认展开、deviations hints） → `Create`(2) | **2** |
| 看历史波证据 | 折叠卡(1) → 只读波页（注意力条带活跃波红数） → 可选 drawer 钻取（+1–2） | **1–3** |

## 7. 后端最小补充清单（v3 重排；后端工作包先行修订，与前端并行）

**必须有**

1. `POST …/messages/{id}/resolve` + `team_messages.resolved_at/resolved_by/resolution` — 用户拍板维度（P0-1，第一轮）。
2. `POST …/messages/{id}/ack`（窄：仅非 decision 类消息的 host delivery → acknowledged）— handoff 签收的写入方；没有它 checklist② 永不绿、琥珀区被历史 handoff 永久污染（新 P0-2）。
3. SSE upsert 帧 ×3：`member_run` / `member_action` / `team_message`（缺陷②）。
4. transition 接受 `{status, note}` 并持久化 `gate_note`（P2-10① + 缺陷③）。
5. SSE upsert 帧：`team_run`（第一轮 P0-3）。
6. team messages 接受并持久化 `sender_kind`（from_member_id 可空=operator）（第一轮 P1-6）。
7. ⑥a waiting 状态写入：契约 `## RESULT: waiting` 结果标记（或 mapper 观测 waiting 类 update）+ turn 终了写 `member_runs.status=waiting`（新 P0-1）。
8. ⑥b re-drive 端点：`POST /v1/team-runs/{id}/members/{memberId}/start`（或 CLI/MCP 触发），以该成员最新 operator control 为首轮 prompt 起全新会话（新 P0-1）。
9. messages 端点接受可选 `causation_id`（字段已在，放开硬编码 None）（新 P1-6）。

**后端通则 D · 合并写纪律（新 P1-5）**：终态吸收——run 一旦 Completed/Cancelled，后续写入不得回归 status、不得擦除 completed_at/gate_note；orchestrator 终态写前必须 read-merge-write（读最新行，已是终态则跳过回归、保留既有 gate_note/completed_at）。同一纪律适用于 member_runs 终态。

**可推迟（前端有诚实降级）**

10. `team_runs.title`（前端自动建议+组合进 objective 首行）。11. per-member `done_criteria` 字段（`## Done when` 小节约定）。12. blocked/failed 状态写入（可沿 `## RESULT:` 标记模式扩展；UI 条件渲染兜底）。13. budget used。14. snapshot 分页。15. DelegationRun 写入者。16. delivery 自动重投（composer 预填手动重发）。17. correlation 配对视图（字段已在）。18. **parked session / ACP session-load resume**（serve 常驻+会话挂起是大项目；v0 用 re-drive 全新会话模型，新 P0-1 明确放弃）。

## 8. 明确不做什么（v0 边界）

- 同 v2 全部（无 DAG、无成员级 pause/inject/interrupt、无原始 provider 流、无第二通道、无自动 re-plan、无常驻组织、列表无搜索分页、无桌面通知、gate 不做外部事实自动校验、cockpit 无 Current task 列、无自动重投、无批量 resolve）。
- **不做 parked session**（v3 新增，新 P0-1）：成员等待态=re-drive 全新会话（契约在盘上+re-brief），不假装"原会话挂起唤醒"。
- **不给 host 发消息**（v3 新增，P2-10④）：composer 无 host 收件人；operator 对 host 编排器的指示通道不在 v0。
- **不做跨波红区合并**（v3 新增）：跨分支可见性=注意力条+角标计数+跳链，不把多波 decisions 合并进单一红区（波是拍板的天然作用域）。
- **不做 waiting 自动 re-drive**：re-drive 是显式人工动作（会真实起会话烧预算）。

## 9. 可追溯性

- 两轮质疑 26 条 → 两张消化总表逐条落 §1–§8；九缺陷 → §7 分档；负责人五要求 → §2/§3/§5/§6；三失败模式 → §3 契约表、§4 三轨道与四类 decisions、§3 门证据包；Stage 6 场景 → waveTree 分支/重试、契约 owned_paths、门 checklist。
- types.ts 核实（v2 已做，v3 仍有效）：`TeamMessage.evidence_refs/correlation_id/causation_id` 已存在（§7-⑨ 仅放开写入）；`TeamMessageDelivery` 含 failed/expired；`TeamRun` 无 title/gate_note。
- 质疑官代码实证采纳：`MemberUpdateMapper` 仅映射 progress/tool、`KimiAcpClient` 无 session/load、`send_team_message` 落库即 Queued、`team_run_start` 一次性进程——⑥ 拆分与 re-drive 语义据此定稿（新 P0-1）。

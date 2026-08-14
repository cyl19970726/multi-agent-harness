---
name: harness-frontend-product-design
description: "通用前端产品设计-实现工作流：文字合同 → agent 出稿图 → 美术风格测量成文字规格 → 对照规格实现 + 机械验证 → Owner 视觉验收。当任务涉及：做/改任何前端页面或组件的视觉与交互、设计图/出稿/mockup、前端美术风格、UI 还原度、设计规格、前端验收，或要判断'这个界面做得对不对/好不好看'时使用。核心纪律：agent 是翻译者与验证者，永远不是视觉裁判。"
---

# Frontend Product Design · 前端产品设计与实现

## 心智模型（先内化，再动手）

### 1. 前端是三条流之间的翻译，禁止跳译

```
合同流（文字·意图）  →  感知流（图·方向）  →  规格流（测量值·实现）  →  Owner 眼（验收）
   Phase 0               Phase 1               Phase 2 → 3                Phase 4
```

每两条相邻流之间必须有**显式翻译产物**：合同是意图的文字化、设计图是合同的可视化、
规格是图的测量化。跳级即失败：

- 图 → 直接写代码 = 每个 agent 各译一遍，65 个 reviewer 65 种"差不多"（实测）
- 实现 → 反补设计图 = 给犯罪现场补合同，验收失去独立参照系
- 感觉 → 直接调 CSS = 改没渲染都不知道（@layer 致盲：preflight 静默 nullify 全表
  margin/padding/border，多轮"调整"从未生效）

### 2. 判断权归属表（本 skill 的第一约束）

| 判断 | 唯一有资格的来源 |
|---|---|
| 方向/风格好不好看 | **Owner 的眼睛**（对实际截图/候选图） |
| 落地没落地（元素在不在、值对不对） | **机械测量**（测试 / 像素采样 / computed style） |
| agent 自己 | **翻译者 + 验证者。永远不是裁判** |

agent 输出里出现"looks good / 符合设计 / 视觉效果不错"而无 Owner 批注 = 违规信号。
实测代价：65 个 agent reviewer 的自判 PASS 被 Owner 证伪 ≥17 次。

### 3. 设计是一个会过期的引用

设计图是 reference 不是 constant。规则：**单一真源**（一处存活，如 Notion）、
**选定即冻结**、**落选即显式标记 DEAD**、**变更留 changelog**、**代码仓只放指针
（链接），不放设计图源文件**。失效不传播 = 全队朝作废目标施工（实测：v3 作废图
留在 git 历史被当作合同用了一年）。

### 4. 诚实边界：每个 UI 元素只有三种出身

```
数据有出处（字段存在）  /  静态文案（可写死）  /  needs-backend（立后端任务）
```

设计期就在「元素→数据映射表」里标注出身；实现期**禁止伪造中间态**——没有字段的
元素缺省并具名，绝不发明占位数据、占位因果、占位状态。needs-backend 项必须成为
可追踪的后端工作项，不许在"残余清单"里躺平。

### 5. 验证的单位是渲染结果，不是代码改动

"改了 CSS" ≠ "渲染了"。验收只看：截图、像素采样、computed style、精确 revision。
材质类结论必须有像素级证据（例：白卡 #ffffff vs 画布 #fffefb 差 1/255 = 不可见，
无论规则写没写）。

### 6. 规格必须可重生

spec 由**脚本化测量**产出，不是手工感觉笔记。可重生才可校验、才不怕丢失、
才能在新设计图进来时快速再版。存放分工：**脚本实体入仓**（可运行、可复测），
spec 文档存设计真源页，真源页放脚本的仓内链接（脚本无法"同存"于 Notion）。

## 五阶段流水线

```text
Phase 0 文字合同 ──GATE: Owner 确认合同──▶
Phase 1 出稿探索 ──GATE: Owner 选定方向+冻结──▶
Phase 2 测量成规格 ──GATE: 数据可行性标注完──▶
Phase 3 对照规格实现 ──GATE: 机械验证全绿+渲染证据──▶
Phase 4 Owner 视觉验收 ──批注→回写 spec/清单→再迭代──▶ 交付
```

### Phase 0 · 文字合同（不生成任何图）

产出物（文字，存设计真源处）：

- 范围与非目标（明确不做什么）
- 数据可见性/权限规则（谁能看到什么，结构性 absent 而非隐藏）
- 布局合同（栏结构、滚动归属、首视口必须可见的内容、sticky 元素）
- 组件清单（页面由哪些区域/组件构成）
- UX 行为规则（见下文「交互行为基线」）
- 文案语调（如 quiet operations：陈述事实、无感叹、无营销词）

闸门：Owner 确认合同文字。没确认不出稿。

### Phase 1 · 出稿探索（生成图片的唯一合法时机）

图是**决策输入**，不是交付物。

- 只从 Phase 0 合同生成，把合同喂进生成 prompt；禁止自由发挥
- 一次出多候选，Owner 选的是**方向**（风格/密度/层级），不是像素
- 选定那一刻：**冻结进设计真源（唯一 authority）+ 落选者显式标记 DEAD**
- 两个禁止时机：① 实现已存在后"照实现补图"（合同变追认）；② 合同没变却
  反复重新生成（每次重生成都是隐性合同变更）
- **出图方式不限**：方向选定的本质是冻结唯一真源——agent 生成、Owner 供图、
  外部工具（Figma 等）均可；无出图能力时以 Owner 供图直接进冻结，不许卡死

### Phase 2 · 测量成规格（不许照图直接写代码）

把冻结图**测量**成文字规格，全部标注 `[实测]`（采样值）或 `[推导]`（图未展示、
按信条推）：

- 调色板（像素采样：canvas/卡片/发丝线/墨色/强调色/状态色）
- 字阶（量字高换算：标题/正文/标签/meta 的 size·weight·color）
- 几何（栏宽、padding、行节奏、圆角——按基准宽度等比换算）
- 组件形态逐条（含 default/hover/focus/selected/disabled/empty/loading 全状态）
- **元素→数据映射表**：每个元素标 有出处（字段名）/ 静态文案 / needs-backend；
  needs-backend 立后端工作项

产出：`visual-spec-<版本>.md` + 测量脚本（同存）。信条级原则（如"平面、发丝线、
留白分隔、强调色仅主动作"）写在 spec 头部，后续所有推导项不得违反。

### Phase 3 · 对照规格实现 + 机械验证

- 逐元素清单驱动（每个元素一行：实现状态），一次改完一个维度集合再送验，
  不倒退回"改一点送一次"
- 每批必跑项目机械验证（typecheck / lint / 项目 browser-check），断言强度
  **只许升不许降**；引用被有意改动元素的断言同步更新并说明
- **渲染验证**：改完用像素采样/computed style 证明生效，不看 diff 下结论
- 截图证据**绑定精确 revision**（exact SHA 进文件名/manifest）
- 铁律：不伪造数据、不自判视觉、最小改动、不动冻结接口

### Phase 4 · Owner 视觉验收

- 把实际截图和冻结参照图**同尺度并排**交给 Owner（可附 montage）
- 批注 → 回写 spec/清单 → 下一轮；残余项全部具名+附原因（无数据/超范围/
  设计示例文案）
- **PASS 只有一个来源：Owner 对截图的明确批准**。验收报告只交"清单落地证据
  + 具名残余"，不写视觉结论

## 规模分档（防止"流程太重→绕行→旧病复发"）

完整管线适用于标准档与新模块档；**微调档**（不新增 surface/数据元素/组件、
不动布局合同）可免合同与 spec 全量编制——但三条铁律无豁免：渲染证据、绑定
revision、Owner 批注。档级判据与留痕要求见 `references/acceptance-and-falsification.md` §7；
拿不准 = 标准档。

## 交互行为基线（UX 合同通用条款）

- **权威写**：写操作等权威回执，禁止乐观假成功；回执失败保留输入并明示
- **stale/error/loading**：数据过期或刷新失败时写操作禁用并说明原因；error
  保留 last-good 只读视图
- **危险动作**：close/retire/delete 类给确认或明确 danger 视觉，不一键即发
- **键盘与焦点**：可交互行可 Tab 到达、Enter/Space 激活、focus-visible 全表统一
  形态；弹层 Esc 关闭+焦点圈闭+关闭后焦点还原
- **空态**：给"为什么空+能做什么"，不给裸空白

## 组件登记表（防实现分叉）

每个项目维护一页 registry：组件 / 用途 / 何时复用 / 何时允许新增。
**新建 primitive 前必须查表**；同一需求的第二个实现出现 = 分叉信号，先合并
再继续。改共享 primitive 要查全部调用点。

## Spec 变更管理

- 本 skill 语境下 spec 只有一件产物：Phase 2 的 `visual-spec-<版本>.md`（含元素→
  数据映射表）；它只在设计真源一处存活（如 Notion），仓内只放链接
- 任何修改在真源页留一行 changelog（日期/改了什么/为什么）
- 设计图换代时：新图冻结、旧图标 DEAD、spec 重新测量再版（版本号递增）

## 失效模式表（签名 → 检测 → 处置）

| 失效 | 检测 | 处置 |
|---|---|---|
| 作废设计继续生效 | 设计源有多个版本并存/无唯一 authority | 冻结唯一真源，旧版标 DEAD |
| 样式"改了没渲染" | computed style/像素采样 ≠ 预期 | 查层叠（@layer/preflight/优先级），修根因 |
| agent 自判视觉 PASS | 报告出现 `PASS`/`符合设计`/`视觉达标` 字样且无 Owner 批注引用 | 机械打回（无需进一步阅读），改交落地证据清单；合法结论词仅 `no-blocking-findings` / `findings(listed)` |
| 无数据伪造 | 元素映射表有缺口却已"实现" | 删伪造，转 needs-backend 工作项 |
| 实现分叉 | 同一组件 ≥2 个私有实现 | 合并进 registry 的 canonical 件 |
| 验收粒度错误 | 只看 diff/测试，没看渲染 | 补像素/截图证据再下结论 |

## 工具箱与参考文件（references/）

通用流程的支撑文件，按需加载，不要全量预读：

- `references/measurement-toolbox.md` — **测量与渲染验证工具箱**（Phase 2/3 必用）：
  像素采样、几何测量、并排 montage、层叠/产物根因排查五步法、证据绑定约定、
  round 间差异定位。全部脚本化、可复测
- `references/contract-and-spec-templates.md` — **Phase 0/2 产出模板**：产品合同
  （能力/journey/surface 正当性/coverage 标签/元素→数据映射/needs-backend 立项区）、
  visual-spec 模板（信条/色板/字阶/几何/组件全状态）、组件登记表模板、
  小工件存放位置统一约定
- `references/acceptance-and-falsification.md` — **Phase 3/4 验收纪律**：判断权词表
  （机械打回规则）、硬不变量 vs 视觉诊断双轨、分层闸、失效触发器、评审失效
  黑名单、证据包清单、规模分档

与本文件冲突处以本文件为准。

## 本仓适配（multi-agent-harness）

- 机械验证：`pnpm exec tsc -p apps/agent-dashboard/tsconfig.json --noEmit`
- 证据捕获：`AGENT_WORKSPACE_EVIDENCE_DIR=<dir> [FIRM_BUILD_GIT_REV=<40位sha>] node apps/agent-dashboard/tests/agent-workspace-browser-check.mjs`（fixture 七帧 + 移动视口断言）
- 证据目录 `.visual-evidence/`（gitignored）；冻结接口 `apps/agent-dashboard/src/model/roleViews.ts`（类型只读）；设计真源在 Notion

# 审计报告: Work 描述质量和成员回复质量 — Governance W5 Team

**审计员**: quality-auditor (member-run-1786063385018-p68940-0)
**审计范围**: team-run-1786024940867-p15167-0 (Execute #366, #368, #369 in parallel)
**审计日期**: 2026-08-07
**总评**: ⭐⭐⭐ (3/5) — 工作描述 struct 化程度不均，成员回复格式缺乏一致性

---

## 1. 被审计 Works 总览

| # | Work ID | 标题 | 成员 | 状态 | Context 评分 | Criteria 评分 | Summary 评分 |
|---|---------|------|------|------|-------------|--------------|-------------|
| 1 | p16373-1 | Lane A: Multi-team daemon (#366) | daemon-arch | done | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ |
| 2 | p16377-1 | Lane B: Rename Phase 1 (#368) | rename-agent | done | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| 3 | p16380-1 | Lane C: GitHub linkage Phase 1 (#369) | github-link | done | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| 4 | p97655-1 | Fix: Dashboard team view confusion | daemon-arch | done | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ |
| 5 | p83295-1 | Fix: Dashboard 系统性加载问题 | daemon-arch | done | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ |
| 6 | p14045-1 | Fix: 系统性问题 #372 | daemon-arch | done | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ |
| 7 | p89998-1 | Org + Company Work 设计 (#373) | org-designer | done | ⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| 8 | p94310-1 | 重命名 Phase 2 | rename-agent | done | ⭐⭐ | ⭐⭐ | ⭐⭐⭐ |
| 9 | p90001-1 | 探索: 技术方案 (#376) | cross-machine-explorer | cancelled | ⭐⭐ | ⭐⭐⭐ | N/A |
| 10 | p90004-1 | 实现: Dashboard All Works (#377) | company-view-impl | cancelled | ⭐⭐ | ⭐⭐ | N/A |
| 11 | p94313-1 | GitHub linkage Phase 2 | github-link | in_progress | ⭐⭐ | ⭐⭐ | N/A |
| 12 | p56587-1 | 实现: Dashboard All Works retry | dashboard-builder | in_progress | ⭐ | ⭐⭐ | N/A |
| 13 | p67514-1 | 探索: 技术方案 retry | x-explorer2 | in_progress | ⭐ | ⭐⭐ | N/A |
| 14 | p69301-1 | Audit (本 work) | quality-auditor | in_progress | N/A | N/A | N/A |

---

## 2. Work Context 质量分析

### 2.1 高质量 Context（⭐⭐⭐⭐+）

**Lane A/B/C 初始 Works** (p16373, p16377, p16380) 是模板质量的范例：

```
┌─ What ─┐
├─ Mental Model ─┤ (before/after, loop logic, data flow)
├─ Boundary ─┤ (modify/don't modify, worktree path, constraints)
├─ Evidence ─┤ (artifacts, tests, CI checks)
```

特点：
- ✅ 清晰的 What/Mental Model/Boundary/Evidence 分层结构
- ✅ 包含工作流、数据模型、代码路径等具体细节
- ✅ 明确标注 OWNED paths 和 DON'T MODIFY 边界
- ✅ worktree 路径明确（`../multi-agent-harness-daemon`）
- ✅ 关键约束明确（CRITICAL: backward compatible）

**Dashboard 修复 Works** (p97655, p83295, p14045) — 甚至更好：

- ✅ **包含完整的调用栈/数据流追踪图**（问题定位清晰）
- ✅ 列出所有可能的根因假设
- ✅ 文件路径+行号精确
- ✅ 验证步骤明确

### 2.2 中等质量 Context

**org-designer 的 design work** (p89998) — 信息完整但格式压缩：

```
┌─ What ─┐ 输出 Organization + Company Work 设计文档
│ Mental Model ─┐ Company > Organization > Company Work; Views: ...
│ Boundary ─┐ 参考 docs/..., Output: specs/...
```

- ⚠️ 所有信息在一行 `│ ... ─┐ ...` 中，可读性下降
- ✅ 关键信息齐全（参考文档、输出路径、交付方式）
- ⚠️ 缺少具体的 worktree 路径

### 2.3 低质量 Context（⭐⭐及以下）

**Phase 2 Works** (p94310, p94313) — 过度精简：

```
┌─ What ─┐ 更新 skills/plugins/docs/ 中所有 harness→firm
│ Mental Model ─┐ grep 替换所有旧名称, 更新 plugin manifests
│ Boundary ─┐ skills/ plugins/ docs/; Worktree: ../multi-agent-harness-rename2
```

- ❌ 缺少具体文件列表、替换规则、边界条件
- ❌ 没有说明哪些 patterns 需要替换、哪些要保留
- ⚠️ Mental Model 过于简单（"grep 替换所有"）

**Retry Works** (p56587, p67514) — 丢失了原始 context：

```
Previous member failed. Retry the Dashboard All Works view implementation.
Reuse existing TeamWorksBoard component, add team filter + status filter.
Only modify apps/agent-dashboard/src/. Worktree: ../multi-agent-harness-org-views2.
```

- ❌ **完全丢失了原始 work 的详细分析**（调用栈、组件结构、API 端点）
- ❌ 没有包含原始 work 中已诊断的问题和失败原因
- ⚠️ 新成员无法了解前一个成员的尝试和遇到的坑

### 2.4 Context 缺少的关键信息（模式总结）

| 缺失信息 | 影响的 Works | 后果 |
|---------|-------------|------|
| worktree 路径 | p89998 (org-designer) | 成员需自行推断路径 |
| 具体文件路径 | p94310, p94313, p56587, p67514 | 增加探索成本 |
| 失败原因/前车之鉴 | p56587, p67514 (retry works) | 重复踩坑 |
| CRITICAL 约束 | p94310 (rename Phase 2) | 可能误改不该改的文件 |
| 依赖的参考文档 | p90001, p90004 (cancelled works) | 成员需自行发现 |

---

## 3. Completion Criteria 质量分析

### 3.1 良好标准（⭐⭐⭐⭐+）

**Lane A/B/C** 的 completion criteria 是标杆：

```
RULE ZERO: done = merged PR with green CI.
Layer 1: work transitions emit events
Layer 2: implicit delivery routes to owners/host/dependents
Tests: lifecycle integration + event emission + delivery routing
```

- ✅ **RULE ZERO** 明确最终交付状态
- ✅ 分层描述，每层独立可验证
- ✅ 测试和 CI 要求具体

### 3.2 常见问题

1. **模糊的验收条件**: "commit+push+PR+merge" 过于泛化，缺少具体验证点
2. **缺少 RULE ZERO**: Phase 2 works 没有明确"done = merged PR with green CI"
3. **不可验证**: "Dashboard All Works 视图可用" — 什么叫"可用"？需要具体验收步骤
4. **丢失原始 context**: retry works 的 criteria 比原始 work 更模糊

---

## 4. Result Summary 格式合规分析

### 4.1 标准格式定义（org-designer 基准）

org-designer 的 Work result_summary 格式：

```
设计文档完成并已合入 master: specs/organization-company-work/design.md (613 行).
覆盖 Organization 模型, Company Work 模型, View 设计, API 草案, Store 布局, 验收标准.
```

特点：
- ✅ **结果声明**（已完成/已合入）
- ✅ **产物清单**（文件+行数）
- ✅ **覆盖范围**（枚举关键主题）
- ✅ **artifact_refs + check_refs** 填充（PR URL）

org-designer 的 TeamMessage 格式：

```
## RESULT
### Summary
### Coverage
### Key Design Decisions
```

特点：
- ✅ **结构化标题**（## RESULT, ### Coverage, ### Key Design Decisions）
- ✅ **Summary ≤10 lines**
- ✅ **Coverage bullet list** 逐项枚举
- ✅ **明确的 PR URL 和 commit 引用**

### 4.2 其他成员对比

| 成员 | result_summary 格式 | TeamMessage 格式 | 评分 |
|------|-------------------|-----------------|------|
| org-designer | 结构化，产物明确 | `## RESULT` + sections | ⭐⭐⭐⭐⭐ |
| github-link | 详细但段落式 | `REPORT:` 前缀，无标题层级 | ⭐⭐⭐⭐ |
| daemon-arch | 不一致，混合 prose | `PROGRESS:` / `REPORT:` 无标题 | ⭐⭐⭐ |
| rename-agent | 详细段落式 | 无独立消息（仅 work submit） | ⭐⭐⭐ |

### 4.3 格式差异典型案例

**github-link Phase 1 result_summary**（⭐⭐⭐⭐⭐）：
```
Phase 1 complete (issue #369): work create --github-issue owner/repo#N snapshots {...}
into Work.github_links and auto-populates artifact_refs with the issue URL;
work submit --github-pr owner/repo#N attaches {...} with a live CI summary from gh pr checks...
```
→ 细节丰富、可验证，但缺少分层标题，需要全文阅读才能定位关键信息。

**daemon-arch work #4 (p14045) result_summary**（⭐⭐⭐⭐）：
```
Systemic diagnosis of 3 issues:
1. Status display: CORRECT — daemon-arch status 'running' reflects the active provider turn...
2. Hook injection: FIXED — root cause was missing host binding...
3. Auto notifications: SSE already pushes work status changes...
```
→ 结构清晰（编号列表），内容充实。但作为段落而非 markdown headers。

### 4.4 TeamMessage 格式差距最大的方面

| 字段 | org-designer | 其他成员 |
|------|-------------|---------|
| **WORKTREE** (path, branch, commit) | ❌ 未在 message 中提供 | ❌ 均未提供 |
| **ARTIFACTS** (PR URL, CI URL) | ✅ PR #379 URL | ⚠️ github-link 提及 PR #382，其他人缺失 |
| **KEY DECISIONS** | ✅ 6 条设计决策 | ❌ 均未提供 |
| **COVERAGE** (bullet list) | ✅ 6 项覆盖 | ❌ 均未提供 |

---

## 5. 成员失败模式分析

### 5.1 Cancelled Works

**cross-machine-explorer** (探索 #376) — "Member stuck, reassigned"
- 在收到 Host "Continue your work" 消息后回复 "Received. Continuing..." 
- 但未产出结果即 stuck
- **根因推测**: Context 过于精简（单行指令），成员可能缺少足够方向

**company-view-impl** (实现 #377) — "Member failed, reassigned"
- 在收到 "Continue your work" 后无回复即失败
- **根因推测**: 可能遇到技术障碍但未主动汇报

### 5.2 模式总结

- ⚠️ **成员不主动上报阻塞**: 两个 cancelled work 都没有 `BLOCKED` 消息
- ⚠️ **Retry 未传递失败知识**: retry works 的 context 没有包含前一个成员的诊断信息

---

## 6. Work Context 改进建议

### 6.1 推荐模板（基于 Lane A/B/C 最佳实践）

```markdown
┌─ What ──────────────────────────────┐
│ [一句话描述任务目标]                                                    │
├─ Mental Model ──────────────────────┤
│ [当前状态 → 目标状态的变化]                                            │
│ [关键数据流/调用链]                                                     │
│ [关键设计决策引用]                                                     │
├─ Boundary ──────────────────────────┤
│ Modify: [具体文件路径列表]                                             │
│ Don't modify: [明确禁改范围]                                            │
│ Worktree: [OUTSIDE repo 的具体路径]                                    │
│ Depends on: [参考文档、ADR、issue 引用]                                │
│ CRITICAL: [不可违反的硬约束]                                            │
├─ Previous Attempt (retry only) ─────┤
│ [前一个成员做了什么]                                                   │
│ [失败原因/卡在哪]                                                       │
│ [已有的诊断和发现]                                                     │
├─ Delivery ──────────────────────────┤
│ commit + push + PR + merge                                            │
├─ Evidence ──────────────────────────┤
│ [产物/测试/验证步骤列表]                                             │
└─────────────────────────────────────┘
```

### 6.2 关键改进点

1. **Retry works 必须包含 "Previous Attempt" 字段**
   - 前成员的 worktree 路径
   - 已有代码/tests 状态
   - 已知的失败原因
   - 前成员的诊断信息

2. **Phase 2 不能假设成员记得 Phase 1 context**
   - 必须重新声明关键约束
   - 引用 Phase 1 的 artifacts 作为起点
   - 如果 Phase 1 改变了规则（如 crate 重命名），必须明确

3. **Worktree 路径不可省略**
   - 每个 work 必须明确 `Worktree: ../multi-agent-harness-<purpose> (OUTSIDE repo)`

4. **Completion criteria 必须包含 RULE ZERO**
   - 格式: `RULE ZERO: done = merged PR with green CI`

---

## 7. 成员回复格式改进建议

### 7.1 推荐的 TeamMessage 提交格式

```markdown
## RESULT (done/blocked/failed)

## SUMMARY (≤10 lines)
[关键结果摘要]

## COVERAGE
- [覆盖的模块/功能 1]
- [覆盖的模块/功能 2]
- ...

## KEY DECISIONS
- [决策 1 + 理由]
- [决策 2 + 理由]

## WORKTREE
- Path: [绝对或相对路径]
- Branch: [分支名]
- Commit: [commit hash]

## ARTIFACTS
- PR: [URL]
- CI: [URL]
- Files: [关键文件路径]
```

### 7.2 推荐的 Work submit result_summary 格式

```
[结果动词]: [产物描述]. [关键覆盖范围, 用逗号分隔].
[文件路径/PR URL].
[测试状态] + [CI 状态].
Worktree: [path], branch: [branch], commit: [hash].
```

---

## 8. 典型问题示例

### 示例 1: Phase 2 Context 缺失

**问题**: rename Phase 2 work (p94310) 只有一行 "更新 skills/plugins/docs/ 中所有 harness→firm"
**后果**: 成员需要自己发现哪些文件属于 scope、哪些 patterns 需要替换、哪些要保留
**改进**: 应列出 "Modify: skills/*/SKILL.md, plugins/star-harness/, docs/*.md" 和 "Keep: legacy references in historical docs"

### 示例 2: Retry 丢失前车之鉴

**问题**: dashboard-builder 收到的 retry work (p56587) 只有 "Previous member failed. Retry..."
**后果**: 新成员不知道前成员做了什么、遇到了什么问题
**改进**: 应包含 "Previous: work-xxx, worktree: ../multi-agent-harness-org-views, found: component X conflicts with Y, attempted approach Z failed because..."

### 示例 3: 缺少结构化回复

**问题**: daemon-arch 在所有 5 个 works 中只用 `PROGRESS:` / `REPORT:` 前缀
**后果**: Host 和其他成员需要完整阅读才能定位关键信息
**改进**: 采用 `## RESULT / ## SUMMARY / ## COVERAGE` 分层标题

### 示例 4: 未汇报阻塞

**问题**: cross-machine-explorer 在 stuck 前只发了一条 "Received. Continuing..." 消息
**后果**: Host 无法及时重分配，浪费了一个轮次
**改进**: 成员应在遇到 3 轮无进展时主动发送 blocked 消息，说明具体障碍

---

## 8. 额外发现：Host Session 上下文分析

基于 Host session `session_9c6cfb21-27a4-4d68-b3e6-37a094ddfcce` 的分析。

### 8.1 Host 的核心期望

1. **自我验证的修复** — "声称已修复"必须有验证结果，不做口头修复
2. **深度设计推理** — 需要结合历史决策与当前约束的综合分析，非表面摘要
3. **自动化被动通知** — team member 消息应自动进入 inbox，非手动轮询
4. **显式 artifact 引用** — 每个任务应有 PR URL、commit SHA、文件路径
5. **失败升级与分类** — 失败需分类（业务/仪器/环境），3 次重复失败应升级重分配
6. **积极记录系统性问题** — 不可见的重复问题应创建 Issue + 根因分析

### 8.2 Host 明确表达的痛点

- **"你他妈自己验证下"** — agent 声称修复后未验证，导致信任侵蚀
- **"这个很明显是产品设计缺陷"** — 3 次修复同一前端问题未解决
- **"为什么这里你还有 wave 这个 我们之前不是已经移除了吗？"** — 概念滞后
- **"这个不是会自己 push 到我们的上下文吗"** — 通知架构不满足预期
- **"任务清晰度问题...Member 的回复有时不太清晰"** — 直接指向本审计的核心问题

### 8.3 Member Runs 执行特征

- **模型分布**: 5 个 deepseek-v4-pro + 3 个 deepseek-v4-flash
  - flash: github-link, company-view-impl, dashboard-builder
  - pro: daemon-arch, rename-agent, org-designer, cross-machine-explorer, x-explorer2
- **分支不匹配**: Batch 1 (daemon/rename/github) 在 `codex/supervisor-recovery-liveness-340`，Batch 2 (org-design 系列) 在 `codex/org-company-work-design-373` — 不同代码基线
- **worktree_ref 全为 null**: 所有 member runs 的 worktree_ref 字段均未设置，虽然 work context 中指定了 worktree 路径
- **native_session 全为 null**: 成员被创建但执行记录未关联 provider session

---

## 9. 审计结论

### 9.1 做得好的

- **Lane A/B/C 初始 work 的 context/criteria 质量**达到模板级别
- **org-designer 的 result_summary + TeamMessage 格式**是 best practice
- **github-link 的 result_summary 细节**充足，可验证性强
- **Dashboard fix works 的根因分析**和调用栈追踪非常出色
- **所有 done works 都有实际的代码产出**（PR merged 或 code ready）

### 9.2 需要改进的

1. **Phase 2 / Retry works 的 context 必须重新声明约束**，不能假设成员记得 Phase 1
2. **所有成员应统一使用结构化回复格式**（## RESULT / ## SUMMARY / ## COVERAGE / ## WORKTREE / ## ARTIFACTS）
3. **Completion criteria 必须包含 RULE ZERO**（`done = merged PR with green CI`）
4. **成员必须在 3 轮无进展时主动上报阻塞**，不要静默 stuck
5. **Worktree 路径、PR URL、commit hash 是必须信息**，不可省略

### 9.3 建议优先级

| 优先级 | 改进项 | 影响范围 |
|--------|--------|---------|
| 🔴 P0 | Retry works 包含失败原因和前成员诊断 | 减少重复浪费 |
| 🔴 P0 | 所有 works 包含 RULE ZERO completion criteria | 明确交付标准 |
| 🟡 P1 | 统一 TeamMessage 回复格式 | 提高可扫描性和一致性 |
| 🟡 P1 | Worktree/PR/commit 信息强制填写 | 提高可追溯性 |
| 🟢 P2 | 成员上报阻塞的 3 轮规则 | 减少静默 stuck |
| 🟢 P2 | Phase 2 works 重新声明所有约束 | 减少 context 丢失 |

---

## 10. Session-Forensics 深度分析（Host Session）

基于 session-forensics 脚本对 Host session `session_9c6cfb21` 的度量分析。

### 10.1 关键指标

| 指标 | 值 | 解读 |
|------|-----|------|
| 总行数 | 45,192 | 28.1 MB |
| Turns | 246 | 含 246 个完整轮次 |
| Execs (工具调用) | 1,697 | 平均每 turn 6.9 次调用 |
| Failures | 325 | 19.15% 失败率 |
| Goal-loop continuations | **135** | **54.9% 的 turns 是自主续跑** |
| Background notifications | **175** | task completed/lost/failed 通知 |
| narrative_to_evidence | 17.42 | 远高于合理基线 |
| Peak context | 668,673 chars | 窗口接近饱和 |
| Bash flood_share | **74.0%** | Bash 占用了 3/4 的上下文预算 |

### 10.2 命令重复度分析（Harness 轮询低效）

| 命令 | 出现次数 | 问题 |
|------|---------|------|
| `team-run status` | **178** | Host 无法被动接收状态变更，必须主动轮询 |
| `team-run wait` | **126** | 阻塞等待，浪费 turn |
| `team-run work` | **321** | create(94) + submit(57) + 其他(170) |
| `git push` | **234** | 大量重复推送（多人多分支） |
| `gh pr` | **244** | PR 创建/查看/合并操作频繁 |
| `cargo test/build/...` | **159** | CI 替代成本高 |
| "Continue working toward the active goal" | **135** | 54.9% turns 是 goal-loop 自动续跑 |

### 10.3 诊断发现

**发现 1: Goal-loop 严重浪费上下文**

Host session 的 135 个 goal-loop 续跑 prompt 中，**92 个是相同的 "Continue working toward the active goal"** 消息。这些续跑发生在：
- 后台 bash 任务完成通知后（94 个 notification restatements）
- 每个 notification 消耗 ~300 bytes，累积 28KB — 但更严重的是每个通知触发一轮新 turn

**发现 2: Polling 代替 Push 的模式**

178 次 `team-run status` + 126 次 `team-run wait` = 304 次主动状态查询。这是 Host session 的核心低效模式：
- 每次查询消耗一次 bash 调用 + JSON 输出解析
- 大量查询结果为空（无变化时）

**发现 3: 跨 Worktree PR 管理开销**

234 次 `git push` 和 244 次 `gh pr` 反映 Host 在同时管理多个 worktree 的 PR：
- 9 个 worktree 被触碰
- 每个 worktree 独立 git push + gh pr create/merge

**发现 4: failure_rate 19.15% 需要分层解读**

325 次失败中，instruments 35 次、business 37 次。但 metrics 脚本指出：多数 business 失败是 cargo/test 迭代，属于 TDD 合法开销。需要语义判断，不能仅凭计数。

**发现 5: Bash 占 74% 上下文预算**

1,696 次 Bash 调用产生 1.27M chars（平均 750 bytes/次），但其中大量是重复的 harness 命令。

### 10.4 Work 创建/提交完整性

从 Host session 的命令行统计：
- `work create`: 94 次（含 W1-W5 全部 waves 的创建工作）
- `work submit`: 57 次

与 store 中实际 Works 的对比：
- W5 team-run 中有 14 个 Works
- 10 个已 done（含 accepted/review 状态）
- 2 个 cancelled（成员 stuck/failed）
- 2 个 in_progress

**提交结果格式审计**（基于 store 中的 result_summary）：

| 成员 | result_summary 结构化程度 | 包含 WORKTREE? | 包含 PR URL? | 包含 COMMIT? |
|------|------------------------|---------------|-------------|-------------|
| github-link (p15167-3) | ⭐⭐⭐⭐⭐ 极详细 | ✅ (路径+分支) | ✅ | ❌ |
| daemon-arch (p15167-1) | ⭐⭐⭐ 信息丰富但段落式 | ❌ | ❌ | ❌ |
| rename-agent (p15167-2) | ⭐⭐⭐ 详细但段落式 | ❌ | ❌ | ❌ |
| org-designer (p27732-0) | ⭐⭐⭐⭐ 结构清晰 | ❌ | ✅ (PR #379) | ❌ |

**100% 的 result_summary 缺少 commit hash。75% 缺少 worktree 路径。**

### 10.5 Host Session vs Member Sessions 对比

| 维度 | Host (28.1MB) | daemon-arch (2.4MB) | github-link (2.0MB) |
|------|--------------|--------------------|--------------------|
| Lines | 45,192 | ~6,000 | ~5,000 |
| Goal-loops | 135 | ~30 (estimated) | ~25 (estimated) |
| Harness commands | 625 | ~50 (estimated) | ~40 (estimated) |
| Context waste (notification) | 175 notif | ~20 notif | ~15 notif |

Host session 的体积是 member sessions 的 10-14 倍，但实际的生产力产出（work create/submit）集中在少数 turns。大量 turns 被 polling 和 goal-loop 消耗。

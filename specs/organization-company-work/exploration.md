# Organization + Company Work — 技术探索报告

```text
status: complete
produced_by: member-run-1786038758973-p66721-0 (x-explorer2)
references:
  - specs/organization-company-work/design.md (org-designer, merged PR #379)
  - docs/decisions/0052-nested-agent-teams-are-the-agent-organization.md
  - docs/current/company-os/nested-agent-team-organization.md
```

## 1. 范围

本文档分析当前代码库中 Organization 和 Company Work 的**已有实现**与**设计文档中规划的 API/组件/数据模型之间的差距**。从 apps/agent-dashboard/ 和 crates/ 两个维度进行，为后续实现提供具体路径。

## 2. 现有代码全景

### 2.1 数据模型（Rust — crates/harness-core/src/）

#### Work（lib.rs:3328）
当前 `Work` struct 已经是 ADR 0052 的 partial 实现：

- ✅ `team_id: Option<String>` — 已有，注释写 "Durable AgentTeam scope (ADR 0052)"
- ✅ `parent_work_id: Option<String>` — 已有
- ✅ `owner_member_id: Option<String>` — 已有
- ✅ `created_by_member_id: Option<String>` — 已有
- ✅ `github_links: Vec<GitHubLink>` — Phase 1 刚完成
- ❌ 缺失 Company OS 扩展字段：`business_module_ref`, `milestone_ref`, `document_refs`, `approval_refs`, `finance_refs`, `source_observation_ref`, `due_at`

关键约束：当前 Work 通过 `team_run_id` 绑定到一次执行尝试。`team_id` 提供 durable 作用域但尚未在创建/查询路径中正式启用 — 它是 additive field，现有 code 兼容 `None`。

#### DurableAgentMember（lib.rs:489）
- ✅ 核心身份字段完整：id, name, role, provider_profile, model, workspace_policy, status
- ✅ Dashboard 已有 `durableAgentMembers()` 层读取（orgSelectors.ts:104）
- ❌ 缺失：`hosted_team_id`（当前 Member Host 了哪个子 Team）
- ❌ 缺失：`human_sponsor_ref`

#### AgentTeam（lib.rs:455）
- ✅ `parent_team_id: Option<String>` — 已有，递归拓扑基础
- ✅ `host_member_id: Option<String>` — 已有
- ❌ 缺失：`company_id`（Company-level grouping）
- ❌ 缺失：`machine_id`（跨机器路由）
- ❌ 缺失：`labels`

#### Company OS WorkItem（company_os.rs:950）
这是完全独立的兼容性模型，与 Work kernel 无关：
- 更重的多角色模型：`accountable_owner`, `assignees`, `contributors`, `reviewer`, `approver`
- 自己的状态机：Draft → Submitted → Triaged → Accepted → InProgress → ...
- `WorkProjection`（company_os.rs:867）提供 board/business_lines/work_types/workload 投影

### 2.2 HTTP API（crates/harness-cli/src/main.rs）

当前 API 面：

| Endpoint | 方法 | 功能 | 状态 |
|----------|------|------|------|
| `GET /v1/snapshot` | GET | 全量 snapshot（含 company_os projection） | ✅ |
| `GET /v1/team-runs/:id/snapshot` | GET | 单 TeamRun 投影 | ✅ |
| `GET /v1/meta` | GET | 构建/数据溯源 | ✅ |
| `GET /v1/spaces` | GET | Execution Spaces | ✅ |
| `GET /v1/projects` | GET | Project Bindings | ✅ |
| `...team-runs/:id/works/...` | POST | Work CRUD 操作 | ✅ |

**缺失的 API**（design.md §6.2 中规划）：
- `GET /v1/organization` — 完整 Org tree + work counts
- `GET /v1/works` — 全局 Work list（带 filter）
- `GET /v1/organization/teams/:id` — 单 Team + members + children
- `GET /v1/organization/members/:id` — 单 Member + owned work
- `POST /v1/works` — 创建 Work（独立 endpoint）

关键洞察：当前 snapshot API 是打包的；所有 Work 已经在一个 `/v1/snapshot` 响应中返回。全局 Work 视图**不需要新的后端查询** — 客户端可以从现有 `snapshot.works` + `snapshot.teams` + `snapshot.member_runs` 中构建。

### 2.3 Dashboard 组件（apps/agent-dashboard/src/）

#### 核心复用组件

| 组件 | 路径 | 功能 | 复用难度 |
|------|------|------|----------|
| `TeamWorksBoard` | `components/workbench/team/TeamWorksBoard.tsx` | 5-lane kanban board + create/assign/review | ⭐ 低 — 直接复用 |
| `AgentTeamOrganization` | `surfaces/AgentTeamOrganization.tsx` | 递归 Team tree + filter + TeamDetail | ⭐ 低 — 直接复用 |
| `TeamWorks` | `surfaces/TeamWorks.tsx` | 跨 TeamRun 聚合 Works（demand 分组） | ⭐⭐ 中 — 需加 team filter |
| `WorkOperatingPage` | `company-os/work/WorkOperatingPage.tsx` | Company OS WorkItem 视图（overview/board/all/milestones/timeline/workload） | ⭐⭐⭐ 高 — 数据模型完全不同 |

#### 关键选择器

| 选择器 | 路径 | 功能 |
|--------|------|------|
| `teamWorksSelectors.ts` | `model/teamWorksSelectors.ts` | `buildTeamWorksModel()` — 从 snapshot 聚合所有 TeamRun 的 Work 行，生成 facets + counts |
| `orgSelectors.ts` | `model/orgSelectors.ts` | `buildAgentTeamOrgModel()` — 从 teams + runs + members + works 构建递归 Org tree，含 work counts |

#### 数据流

```
Store (JSONL)
  ↓ dashboard_snapshot_with_company()
Snapshot JSON (GET /v1/snapshot)
  ↓ buildTeamWorksModel() / buildAgentTeamOrgModel()
Dashboard React state
  ↓ filterTeamWorks() / orgTeamPath()
Rendered views
```

**关键设计**：`teamWorksSelectors.ts:130` 的 `scopedToSingleRun` 标记。当 snapshot 只包含一个 TeamRun 的数据时（通过 `GET /v1/team-runs/:id/snapshot` 获取），自动标为 "single-run"，不声称是全局聚合。这个语义保护已经内置。

### 2.4 类型定义（apps/agent-dashboard/src/types.ts）

Dashboard 的 TypeScript 类型与 Rust 结构基本一一对应：

- `Work`（ts:747）— 缺少 GitHub links（刚加的）、Company OS 扩展字段
- `AgentTeam`（ts:149）— 有 `parent_team_id?`、`host_member_id?`
- `DurableAgentMember`（ts:105）— 无 `hosted_team_id`、`human_sponsor_ref`
- `DashboardSnapshot`（ts:939）— 包含 `company_os?: CompanyOsSnapshotProjection` 用于读取 durable members

## 3. 设计文档与实现的差距分析

### 3.1 API 差距

| 设计要求的 API | 当前实现 | 工作量估计 | 风险 |
|---------------|---------|-----------|------|
| `GET /v1/organization` | 无，但 `buildAgentTeamOrgModel()` 已在前端实现了等价逻辑 | 后端：新增 endpoint + 已有 selector 逻辑搬到 Rust；前端：从 snapshot 迁移到专用 endpoint | 低 — 逻辑已验证 |
| `GET /v1/works?team_id=&status=...` | 无，但前端 `buildTeamWorksModel()` 已做跨 team 聚合 | 后端：新增 endpoint + 查询参数解析；前端：用 API 替换 selector | 低 — 只是查询投影 |
| `POST /v1/works` | 现有 work create 通过 team-run CLI/HTTP 子路径 | 新增独立 endpoint，复用 WorkOperation pipeline | 低 — pipeline 不变 |
| `POST /v1/organization/teams` | 现有 `harness team create` | 已有 | 无 |
| `GET /v1/organization/members/:id` | 无 | 新增 endpoint | 低 |

### 3.2 组件差距

| 设计要求的视图 | 现有组件 | 工作量 | 策略 |
|---------------|---------|--------|------|
| Global Works View | `TeamWorks` surface | ⭐ 低 | 已有跨 TeamRun 聚合。只需加 `team_id` filter（当前已有 facets.teams 列表但 filter 通过 workTeamId 做 selection state）。status/owner/priority 过滤同理。 |
| Per-Team Works View | `TeamWorksBoard` | ⭐ 无需改动 | 当前 Per-Team War Room 已经显示该 TeamRun 的 Works。设计文档说 "reuses existing without modification" — 确认。 |
| Organization Tree View | `AgentTeamOrganization` | ⭐⭐ 中 | 已有递归树 + work counts。需要加 cross-machine indicator、member roster。 |
| Member Focus | 无独立组件 | ⭐⭐ 中 | 需要新建 `MemberFocus` surface，读取 member + owned work + runtime |

### 3.3 数据模型差距

核心差距不是缺少字段，而是**缺少 Company 概念**和 **Work kernel 统一**：

```
现在:
  两个独立 Work 系统:
    Work (Team scope, team_run_id)       ← 活跃
    WorkItem (Company scope, richer)      ← 兼容，不活跃
  
  Organization:
    AgentTeam parent_team_id/host_member_id  ← 活跃
    OrgUnit/OrganizationMembership           ← 兼容，不活跃
  
  无 Company 容器 — Teams 不属于任何 Company

目标:
  一个 Work kernel（Work struct 扩展 Company OS 字段）
  一个 Organization（AgentTeam + DurableAgentMember 递归投影）
  一个 Company 容器（Teams, Members, Works）
```

### 3.4 迁移路径评估

#### WorkItem → Work 迁移

`harness-core/src/company_os.rs` 中的 `WorkItem` 有 40+ 字段，其中很多在统一 Work kernel 中被丢弃（`accountable_owner`, `assignees`, `contributors`, `reviewer`, `approver` 等）。迁移需要：

1. 将每个 `WorkItem` 行映射到 `Work` 行
2. 保留 `business_module_ref`, `milestone_ref`, `document_refs`（这些是设计文档中新扩展字段）
3. 映射状态：Draft→Open, Submitted→Open, InProgress→InProgress, Blocked→Blocked, InReview→Review, Completed→Done, Cancelled→Cancelled
4. 设置 `team_run_id` = placeholder（需要决定）

**风险**：WorkItem 没有 `team_run_id` 概念 — 它是 Company-level。迁移需要要么创建 placeholder TeamRun，要么在 Work body 上允许 null `team_run_id`（当前 schema 要求非空）。

#### StandingAgent → DurableAgentMember 迁移

设计文档 §3.5 规划：
1. `harness org member converge` — 创建 DurableAgentMember 行
2. `harness org cutover-audit` — 验证所有 Host 是 DurableAgentMember
3. 归档 StandingAgent 行

当前 CLI 已有 `harness org member converge|list|show` 和 `harness org bootstrap-lead`、`harness org host`、`harness org cutover-audit`。Dashboard 已有 `durableAgentMembers()` 读取。

## 4. 实现优先级建议

### Phase 1: 最小可行桥接（1-2 天）

目标：Dashboard All Works 视图可用。

1. **`GET /v1/works` endpoint**（后端，~100 行 Rust）
   - 从 store 读所有 Work + AgentTeam + DurableAgentMember
   - 支持 `?team_id=`, `?status=`, `?owner_member_id=` 参数
   - 返回 `{ works: [...], teams: [...], members: [...] }`

2. **AllWorksBoard 组件**（前端，~200 行 TSX）
   - 复用 `TeamWorksBoard` 的 5-lane layout
   - 加 `team_id` filter dropdown（从 teams list 构建）
   - 加 `status` filter（复用现有 status 选项）
   - 使用 `buildTeamWorksModel()` 做跨 team 聚合（已有逻辑）
   - 无新后端逻辑 — 前端 selector 已处理聚合

### Phase 2: Organization API（2-3 天）

1. `GET /v1/organization` — 返回完整 org tree
2. `GET /v1/organization/members/:id` — member detail
3. Dashboard `MemberFocus` surface

### Phase 3: Work 统一（3-5 天）

1. Work struct 加 Company OS 扩展字段（`business_module_ref`, `milestone_ref`, 等）
2. WorkItem → Work migration
3. 兼容性保护（旧 WorkItem 行仍可读）

### Phase 4: Company 概念（2-3 天）

1. `AgentTeam.company_id` 字段
2. `machines.jsonl` 跨机器注册
3. `harness company bind-team`

## 5. 风险与注意事项

1. **`team_run_id` 非空约束**：Work struct 的 `team_run_id` 是必填字段。Company-level Work（如 WorkItem 迁移而来的行）需要一个 placeholder TeamRun。选项：(a) 创建永久的 "company-work" TeamRun，(b) 放宽 `team_run_id` 为 optional（schema 变更，需要向后兼容）。

2. **单一 snapshot vs 专用 endpoints**：当前 `/v1/snapshot` 返回所有数据。专用 endpoints（`/v1/works`, `/v1/organization`）更高效但增加 API 面。建议 Phase 1 用现有 snapshot + 前端 selector，Phase 2 加专用 endpoints。

3. **Company OS WorkItem 已有完整的 board/milestone/timeline/workload 视图**（`WorkOperatingPage.tsx`, 536 行）。这些视图的 UX 质量很高但是基于 WorkItem 模型。如果要复用这些视图到统一 Work kernel，需要重新映射数据模型。不建议直接复用 — 更好基于 Team Works 组件构建新的 Company Work 视图。

4. **跨机器交付**：设计文档 §4.7 规划了跨机器 Work 交付协议。当前 Supervisor 是单机器进程。实现跨机器需要：(a) `POST /v1/team-runs/:id/deliveries` endpoint，(b) 远程 Supervisor 认领逻辑，(c) 机器注册表。这是最大的未实现部分，建议在 Organization/Work 视图完成后再处理。

5. **TeamWorksBoard.tsx 的 `teamRunId` 依赖**：组件接收 `teamRunId` 参数用于创建 Work (`createTeamWork(teamRunId, ...)`) 和 review (`reviewTeamWork(teamRunId, ...)`)。全局 Work 视图需要处理多 TeamRun 的创建/操作或限制为只读。

## 6. 结论

当前代码库已经走完了 Organization + Company Work 统一约 40% 的路程：

- ✅ Work struct 已有 `team_id` 和 `parent_work_id`
- ✅ DurableAgentMember 已有完整身份模型
- ✅ AgentTeam 已有递归拓扑（`parent_team_id`, `host_member_id`）
- ✅ Dashboard 已有跨 TeamRun Works 聚合选择器
- ✅ Dashboard 已有递归 Organization tree 视图
- ❌ 缺少 Company 容器概念
- ❌ 缺少统一的 Work API（独立于 team-run 子路径）
- ❌ 缺少跨机器交付
- ❌ WorkItem/StandingAgent 尚未迁移

**关键实现洞察**：Dashboard 的全局 Works 视图不需要新后端 — `buildTeamWorksModel()` 已从 snapshot 中聚合所有 TeamRun 的 Work 行。需要的只是新的表面组件和 filter UI。这是 Phase 1 的最小可行方案。

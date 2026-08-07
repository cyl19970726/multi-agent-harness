# Work Gate Contracts

## 概述

Multi-Agent Harness 的 Work 是一个完整的契约声明。一个 work 创建时必须回答五个问题：

```
WHERE   工作区    在哪里干活，能改什么
WHAT    做什么    目标、上下文、完成标准
OUTPUT  产出什么  声明式产出物清单
HOW     怎么交卷  可组合的 Gate 验证门
WHO     谁参与    owner / assignee / reviewer
```

核心洞察：**`gates` 替代分散的 `completion_criteria_markdown`（自由文本）中的验证逻辑。
Gate 是可组合的、可自动执行的验证器，与 Work 的 `github_links`（已有的结构化 GitHub 引用）协同工作。**

---

## 一、架构总览

```
                         Gate 插件层（可组合的验证门）
┌─────────────────────────────────────────────────────────────────────┐
│                                                                     │
│   github-pr gate    code-review gate    check-pass gate   ...       │
│   (PR 存在+合并?)   (Critic agent审查)   (CI 通过?)                 │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
                                  ↑ 消费
                                  │
┌─────────────────────────────────────────────────────────────────────┐
│                         Evidence 层（不可变事实）                     │
│                                                                     │
│   diff    check log    provider session    github_pr    review_note  │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
          ↑                  ↑                    ↑
          │                  │                    │
   ┌──────┴──────┐   ┌──────┴──────┐    ┌───────┴────────┐
   │ Worker agent │   │ Critic agent│    │ GitHub Connector│
   │ (做工作)     │   │ (做审查)    │    │ (同步外部事实)  │
   └─────────────┘   └─────────────┘    └────────────────┘
          ↑                  ↑
          │                  │
   ┌──────┴──────────────────┴──────────────────────────────┐
   │            Work Delivery + Review 管线                  │
   │                                                        │
   │   work assign → member run → work submit → review      │
   │   → gates evaluate → work accept / request-changes     │
   │                                                        │
   └────────────────────────────────────────────────────────┘
```

---

## 二、Work 契约

### 2.1 完整 Work 定义

| 维度 | 字段 | 说明 |
|---|---|---|
| WHAT | `title` + `context_markdown` | 目标与上下文 |
| WHAT | `completion_criteria_markdown` | 完成标准（自由文本，gate 可引用） |
| HOW | `gates: [GateSpec]` | **新增：验证门列表** |
| HOW | `github_links: [GitHubLink]` | **已有：GitHub Issue/PR 关联** |
| HOW | `check_refs: [String]` | 已有：CI check 引用 |
| HOW | `artifact_refs: [String]` | 已有：产出物引用 |
| WHO | `owner_member_id` | 责任 agent |
| WHO | `active_member_run_id` | 当前执行者 |

### 2.2 GateSpec（验证门声明）

```rust
/// A declared verification gate for a Work.
struct GateSpec {
    /// Gate plugin identifier: "github-pr" | "code-review" | "check-pass" |
    /// "artifact-exists" | "owned-path-check" | "goal-design"
    plugin: String,
    /// Plugin-specific configuration (free JSON).
    /// e.g. {"require_merged": true, "require_ci_pass": true} for github-pr
    config: serde_json::Value,
}
```

### 2.3 GateVerdict

```rust
/// Result of evaluating a single Gate.
enum GateVerdict {
    Pass,
    Fail { reason: String },
    Blocked { reason: String },  // 前提条件不满足（如 PR 未开）
}
```

### 2.4 三类典型 work

**代码类 work（需要 PR + review + CI）**：
```json
{
  "title": "实现用户登录",
  "completion_criteria_markdown": "登录功能可用，测试通过，PR 合并",
  "gates": [
    { "plugin": "github-pr", "config": { "require_merged": true, "require_ci_pass": true } },
    { "plugin": "code-review", "config": { "reviewer": "critic-1", "focus_paths": ["src/auth/**"] } }
  ],
  "github_links": [{ "kind": "pull_request", "owner": "x", "repo": "y", "number": 42, "url": "..." }]
}
```

**探索类 work（有产出，不需要 review）**：
```json
{
  "title": "调研 Rust async runtime 选型",
  "completion_criteria_markdown": "产出对比文档",
  "gates": [
    { "plugin": "artifact-exists", "config": { "paths": ["docs/research/async-runtime-comparison.md"] } }
  ]
}
```

**纯文档 work（无 review，无 PR）**：
```json
{
  "title": "更新架构文档",
  "completion_criteria_markdown": "架构文档更新完毕",
  "gates": []
}
```
> `gates: []` → 与当前行为完全一致，Work 的 `review → done` 完全由 `work accept` 手动控制。

---

## 三、Gate 模型

### 3.1 Gate trait

```rust
trait Gate {
    fn plugin_name() -> &'static str;

    fn evaluate(
        work: &Work,
        delivery: &WorkDelivery,
        evidence: &[Evidence],
        config: &serde_json::Value,
    ) -> GateVerdict;
}
```

### 3.2 内置 Gate 清单

| Gate | 消费数据 | 自动/需agent | 说明 |
|---|---|---|---|
| `github-pr` | `work.github_links` + `gh` CLI | 自动 | PR 存在? 合并? CI 通过? |
| `code-review` | `source_type="critic_findings"` | **需 agent** | 代码审查（策略可配置，见 3.4） |
| `check-pass` | `work.check_refs` | 自动 | CI/本地检查通过 |
| `artifact-exists` | `work.artifact_refs` | 自动 | 产出文件存在且非空 |
| `owned-path-check` | delivery.changed_paths | 自动 | 变更不越界 |

### 3.3 Gate 执行语义

- **Gate 独立执行**：每个 gate 的 `evaluate()` 不依赖其他 gate 的结果。
- **自动 gate 先跑**：`github-pr`、`artifact-exists`、`check-pass` 可并行执行。
- **需 agent gate 后跑**：`code-review` 在自动 gate 全部 pass 后触发。
- **全部 pass → done**：所有 gate 返回 `Pass` → `work accept` 可执行。
- **任一 fail → blocked/request-changes**：任何 gate 返回 `Fail` 或 `Blocked` → `work request-changes`，附 gate 名称和原因。

### 3.4 code-review gate 的审查策略

`code-review` gate 不硬编码「必须由 Critic agent 审查」。审查策略通过 `config.strategy` 声明：

| strategy | 谁审查 | 触发方式 | 适用场景 |
|---|---|---|---|
| `peer` | 指定的 reviewer member | `agent deliver` → reviewer member run | 正式代码审查，需要第二人 |
| `self` | Worker 自己的 subagent | Worker 完成后 spawn subagent 做自审 | 小型任务，Worker 自己 review |
| `host` | Host operator/agent | Host agent 直接执行 review | 简单变更，operator 自己把关 |
| `none`（缺省） | 不指定，等于没有 code-review gate | — | 探索任务、文档任务 |

```json
// 同队 peer review
{ "plugin": "code-review", "config": { "strategy": "peer", "reviewer": "critic-1" } }

// Worker 自审（spawn subagent）
{ "plugin": "code-review", "config": { "strategy": "self" } }

// Host operator 直接审查
{ "plugin": "code-review", "config": { "strategy": "host" } }
```

**关键设计决策**：审查策略是 `code-review` gate 的配置，不是新 gate 类型。因为无论谁审，gate 的语义相同——「需要有人审查代码并通过」。只是触发的 agent 管线不同。

---

## 四、Work 状态机

```
  Open ──assign──→ InProgress ──submit──→ Review
    ↑                 ↑                     │
    │           member run 执行         Gate 执行引擎
    │                 │                     │
    │           work submit                  │
    │           + delivery                   │
    │                 │              ┌───────┼───────┐
    │                 │           auto gates   agent gate
    │                 │           (并行)       (需 deliver)
    │                 │              │              │
    │                 │           pass/fail    pass/fail/
    │                 │                      needs_changes
    │                 │              │              │
    │                 │              └──────┬───────┘
    │                 │                     │
    │                 │              全部 pass?  任一 fail?
    │                 │                     │           │
    │                 │                     ↓           ↓
    │                 │               work accept  request-changes
    │                 │                     │           │
    │                 │                   Done       Blocked
    │                 │                     │           │
    │                 │                              可重分配→InProgress
    │                 │
    └─────────────────┴── Cancelled
```

---

## 五、Agent 参与管线

### 5.1 Worker agent 执行

```
1. Lead: work create（含完整 gates 声明）
2. Lead: work assign --member-run-id worker-run
3. Worker member run 执行工作
4. Worker: work submit --result <text> [--github-pr owner/repo#N]
   → WorkDelivery 创建
   → GitHubLink 附加到 Work
5. Work.status → Review
```

### 5.2 代码审查（code-review gate）

审查管线由 `config.strategy` 决定：

#### strategy = "peer"（队友审查）

```
6a. 系统检测 work 有 code-review gate (strategy=peer)
    → 自动生成 review 任务给 config.reviewer member
    → Reviewer member run 启动
    → Reviewer 读 delivery、github_links、artifact_refs
    → Reviewer 产出 Review { verdict, summary, blockers }
7a. code-review gate.evaluate() 消费 Review →
      Pass | Fail | NeedsChanges
```

#### strategy = "self"（Worker 自审）

```
6b. Worker work submit 后
    → Worker 的 prompt 指示它 spawn review subagent
    → Subagent 读 diff、evidence、completion_criteria
    → Subagent 产出 Review { verdict, summary, blockers }
7b. code-review gate.evaluate() 消费 subagent 产出的 Review →
      Pass | Fail | NeedsChanges
```

#### strategy = "host"（Host operator 审查）

```
6c. Host agent 收到 review 通知
    → Host agent 读 delivery、github_links
    → Host agent 产出 Review { verdict, summary, blockers }
7c. code-review gate.evaluate() 消费 Review →
      Pass | Fail | NeedsChanges
```

**原则**：不管谁审，产出都是同一个 `Review` 对象，gate 的 evaluate 逻辑不变。

---

## 六、GitHub Connector 定位

当前 Work 模型已有 `github_links: [GitHubLink]`（含 kind、owner、repo、number、url、status、ci_status、ci_url）和 `work poll-github-ci` 命令来刷新 CI 状态。

Gate 模型在此基础上：
- **`github-pr` gate**直接消费 `work.github_links` + `work.artifact_refs` + `work.check_refs`
- **`work poll-github-ci`** 更新 `GitHubLink` 的 `ci_status` → `github-pr` gate 重新评估
- **GitHub Connector** 作为外部事实的同步管道保持独立

**原则不变**：PR merge ≠ work acceptance。PR merge 只是 `github-pr` gate pass 的前提。
其他 gate 可能还没过。最终由全部 gate pass 后 `work accept` 确认。

---

## 七、向后兼容

- `gates: []`（空列表或字段缺失）→ 行为与当前完全一致。
- 现有 `completion_criteria_markdown` 保持自由文本，gate 可引用但不替代它。
- 现有 `work accept` / `work request-changes` 命令继续工作。
- 现有 `work poll-github-ci` 继续刷新 `github_links` 的 CI 快照。
- 未来 `work accept` 内部先跑 gate 评估，全部 pass 才允许 accept。

---

## 八、实现阶段

### Phase 1：补 Work 契约 ← 当前阶段

- [ ] 在 `firm-core` 中定义 `GateSpec`、`GateVerdict`
- [ ] 在 `Work` struct 上加 `gates: Vec<GateSpec>`
- [ ] 更新 `schemas/work.schema.json`
- [ ] 在 `work create` CLI 支持 `--gate plugin=value` 参数
- [ ] 更新 schema fixtures
- [ ] 向后兼容：gates 为空时行为不变

### Phase 2：Gate trait + 执行引擎

- [ ] 定义 `Gate` trait
- [ ] 实现 `github-pr` gate（消费 `work.github_links`）
- [ ] 实现 Gate 执行引擎（逐个 evaluate，汇总结果）
- [ ] `work accept` 前置 gate 检查

### Phase 3：Agent 管线

- [ ] Worker prompt 注入 gates 声明
- [ ] code-review gate 触发 Critic agent deliver
- [ ] Critic 产出自动解析为 Review

### Phase 4：更多 gate + 注册机制

- [ ] artifact-exists gate
- [ ] check-pass gate
- [ ] plugin 注册机制

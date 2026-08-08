# Work Gate Contracts

## 概述

Multi-Agent Harness 的 Work 是一个完整契约。创建 Work 时需要回答：

```text
WHERE   工作区    在哪里工作
WHAT    目标      做什么、何时算完成
OUTPUT  产出      当前候选需要携带哪些 durable refs
HOW     验证      哪些可组合 Gate 必须通过
WHO     责任      owner / assignee / reviewer
```

`gates` 把可机械判定的验证条件从
`completion_criteria_markdown` 中分离出来，但不取代自由文本完成标准。
声明的 Gate 是 Store-managed `accept_work` 操作的强制不变量：该类型
操作只在所有 Gate 通过时 accept，且不暴露 waiver 参数。

## 一、数据边界

```text
Worker / reviewer / connector
              |
              v
Work current candidate
  - result_summary
  - github_links        external-fact snapshots
  - artifact_refs       durable reference strings
  - check_refs          durable reference strings
  - exact bound Review  Work id + version + strategy + reviewer
              |
              v
GateEngine (pure evaluation; produces no evidence)
              |
              v
Store accept_work seam (all declared gates must Pass)
```

GateEngine 只读当前 Work 候选和精确绑定的 Review。它不启动 agent、
不读文件、不重跑检查、不刷新外部状态，也不证明 ref 的内容真实。
证据采集、reviewer 编排和外部事实刷新是独立管线。

## 二、Work 契约

| 维度 | 字段 | 说明 |
|---|---|---|
| WHAT | `title` + `context_markdown` | 目标与上下文 |
| WHAT | `completion_criteria_markdown` | 自由文本完成标准 |
| HOW | `gates: [GateSpec]` | 声明式验证门 |
| HOW | `github_links: [GitHubLink]` | GitHub Issue/PR 快照 |
| HOW | `check_refs: [String]` | 当前候选携带的持久检查引用 |
| HOW | `artifact_refs: [String]` | 当前候选携带的持久产出引用 |
| WHO | `owner_member_id` | 稳定责任人 |
| WHO | `active_member_run_id` | 当前执行者 |

### GateSpec

```rust
struct GateSpec {
    // "github-pr" | "code-review" | "check-pass" | "artifact-exists"
    plugin: String,
    // 内建 Gate 的字段受类型配置和 deny_unknown_fields 约束
    config: serde_json::Value,
}
```

Store-managed Work 只接受上述四种类型内建 Gate。`GateRegistry` 的显式注册 seam
可供 embedder 和测试使用，但不是 Store 的未类型逃生口。

### GateVerdict

```rust
enum GateVerdict {
    Pass,
    Fail { reason: String },
    Blocked { reason: String },
}
```

Verdict 不是 Work 状态。`Fail` / `Blocked` 会让 Store 拒绝 accept，
但不会自动改成 `blocked` 或自动 request changes。

## 三、四种内建 Gate

| Gate | 精确输入 | 判定语义 |
|---|---|---|
| `github-pr` | 当前候选的单一 PR `github_links` 快照 | 根据配置检查 merged/CI 快照；外部刷新由 `work poll-github-ci` 完成 |
| `code-review` | 精确绑定 Work id、当前 version、strategy 和 reviewer 的 code Review | 消费最新精确匹配 Review 的 verdict |
| `artifact-exists` | 当前候选的 `artifact_refs` | 只比对配置的精确 durable ref；不读文件、不验真 |
| `check-pass` | 当前候选的 `check_refs` | 只比对配置的精确 durable ref；不重跑检查、不验真 |

`artifact-exists` 或 `check-pass` 未配置名称列表时，只要求对应 ref
列表非空。配置名称列表时，每个字符串都必须在当前候选的列表中精确出现。

### code-review 策略

| strategy | 记录 Review 的权限身份 | 额外约束 |
|---|---|---|
| `peer` | 配置的 reviewer MemberRun | reviewer 必填，且必须与 owner 不同 |
| `self` | Work owner MemberRun | 不允许 reviewer 字段 |
| `host` | Host | 不允许 reviewer 字段 |

`strategy` 必填且仅允许 `peer | self | host`。不需要代码审查时应省略
整个 `code-review` gate；`none` 不是策略值。

旧的未绑定 `Review` 仍可读，但不能满足 Gate。Gate 匹配要求可信
Work review 写入边界为当前候选派生的精确绑定，不接受任意 ledger 字段声明。
重新 submit 会产生新 Work version，旧候选的 Review 不再匹配。

### Execution Space 迁移的 Review 信任边界

`space migrate-from-project` 不信任原项目 `reviews.jsonl` 中的 Work 绑定声明。
在创建或替换任何 target ledger 前，迁移会先对完整 source 做预检：

1. 每个 source row 先必须成功 `Deserialize<Review>` 并通过 `Review::validate()`。
2. 只要行中包含以下任一字段，就删除该行中实际存在的绑定字段。
3. 剥离后的行必须再次成功 `Deserialize<Review>` 并通过 `Review::validate()`。

- `reviewed_work_id`
- `reviewed_work_version`
- `review_strategy`

缺失或未知字段、非法 null、部分 Work binding 以及任何其他无效行，都会让
整次迁移 fail closed，不留下 partial target writes。只有完整预检成功后，
剥离后的行才保留为可读的历史 unbound Review，并且不能满足
`code-review` Gate。迁移 manifest 用 `downgraded_bound_reviews` 记录
受影响的 Review 行数；运营者应检查此计数，而不应将原始 ledger
视为 Gate 信任来源。

```json
{ "plugin": "code-review", "config": { "strategy": "peer", "reviewer": "critic-1" } }
{ "plugin": "code-review", "config": { "strategy": "self" } }
{ "plugin": "code-review", "config": { "strategy": "host" } }
```

## 四、状态与接受

```text
Open --assign/claim--> InProgress --submit--> Review
                                               |
                         check-gates (read-only) --> Pass / Fail / Blocked
                                               |
                         Host request-changes ----> InProgress
                                               |
                         Host accept ------------> Done
                               (all declared gates must Pass)

any allowed state --explicit cancel--> Cancelled
```

`work check-gates` 是只读诊断。Store-managed `accept_work` 在写入 Done 前
强制评估 Gate；调用该 Store 操作的 adapter 继承这个不变量。已退役的
CLI `--skip-gates` 会被拒绝，不是 waiver 机制。原始项目 ledger
不属于可信写入边界，必须经过上述迁移降级。

`gates: []` 或缺失字段保留向后兼容的手动 Host accept 行为。这不是
绕过：该 Work 从候选创建起就没有声明 Gate。

## 五、交付与 Review 管线

```text
1. Host 创建 Work，完整声明 gates
2. Work context 向 Worker 显示这些 gates
3. Worker 提交 result 及所需 artifact/check/PR refs
4. Work 进入 Review，当前 version 就是候选身份
5. 若有 code-review Gate，peer/self/host 完成审查后调用类型 Work review 入口
6. Host 诊断 gates，然后显式 accept 或 request changes
```

GateEngine 当前不会自动生成 reviewer Work、启动 reviewer，或将任意 agent
文本自动解析为 Review。这些属于尚未完成的编排管线，不影响
Store 已实现的精确绑定和接受不变量。

## 六、GitHub Connector 定位

`github-pr` Gate 只消费当前候选中的单一 PR `GitHubLink` 快照。
`work poll-github-ci` 负责刷新 `status` / `ci_status` / `ci_url`，GateEngine
本身不联网。PR merge 只能让 `github-pr` Gate 满足其中一项契约；
它不等于 Work acceptance。

## 七、实现状态

### Phase 1：Work 契约（已完成）

- [x] `GateSpec` / `GateVerdict` 与 `Work.gates`
- [x] schema、fixtures 和 `work create --gate`
- [x] 空 gates 的向后兼容

### Phase 2：类型 Gate + Store 接受不变量（已完成）

- [x] 四种类型 built-in Gate
- [x] GateEngine 和 embedder/test `GateRegistry` seam
- [x] 当前候选的精确 durable ref / Review 绑定
- [x] Store `accept_work` 最终入口强制 Gate 检查
- [x] CLI 拒绝已退役的 `--skip-gates`
- [x] Execution Space 迁移降级原项目 ledger 的 Review 绑定并记录计数

### Phase 3：Agent 管线（部分完成）

- [x] Worker context 注入 gates 声明
- [x] Store Work review 入口记录精确绑定 Review
- [ ] 按 strategy 自动编排 reviewer
- [ ] 将 reviewer 输出经类型验证后写入 Work review 入口

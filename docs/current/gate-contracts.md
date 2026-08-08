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
    // 非空 built-in 或 custom plugin name
    plugin: String,
    // 必须是 JSON object；旧 wire 省略时规范化为 {}
    config: serde_json::Value,
}
```

Work 可持久化 custom Gate，前提是 `plugin` 非空且 `config` 是 object。
四种 built-in 的 config 还受各自的类型结构和 `deny_unknown_fields` 约束；
`code-review.strategy` 仍然必填。旧 wire 省略 `config` 时，反序列化统一
规范化为 `{}`，因此它与显式空 object 是同一个声明。

完整 Gate 列表 fail closed：精确重复的 `GateSpec` 被拒绝，且一个 Work
最多声明一个 `code-review` Gate。同 plugin 的不同 custom config 不是
“精确重复”，但仍需有明确 evaluator 语义。

默认 `GateRegistry` 只信任四种 built-in。因此 custom Gate 可持久化，
但默认 GateEngine 和默认 Store `accept_work` 会对未注册 plugin 返回
`Fail`。只有显式提供 custom `GateRegistry` 的 embedder 评估入口才能
调用 custom evaluator；这不会暗中改变默认 Store 的信任集。

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
绑定 Review 的 `reviewed_work_version` 必须大于 `0`；合法 Work 候选从
version `1` 开始。Review ledger 是 append-only 审计历史，`Review.id`
在完整历史中全局唯一；通用和 Work-bound 写入口都拒绝复用既有 id。

### Review 执行者与权限归因

可信 Work review 写入会持久化两个不同概念：

- `performed_by_actor`：可信调用边界提供的 actor 归因；这是 caller input，
  不是 Store 自行完成的身份认证。
- `authority_actor`：实际使用的权限 actor；与执行者相同时可为空。

`peer` / `self` 由绑定 MemberRun 身份执行。`host` Review 的可信权限
固定为 `TeamActorKind::Host` / `host`，`reviewer_agent_id` 也固定为 `host`。
CLI `--actor` 或 HTTP `actor_id` 只改变 `performed_by_actor` 的归因，不能
修改 `authority_actor` 或冒充 reviewer 身份。

### Execution Space 迁移的 Review 信任边界

`space migrate-from-project` 不信任原项目 `reviews.jsonl` 中的 Work 绑定声明。
在创建 target 前，迁移会先对完整 source 做预检：

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

迁移是 **new-target only**：目标 space id 对应的任何路径类型只要已存在，
命令就拒绝并要求新的 `--id`，不会覆盖。`--force` 已退役；传入它会
立即报错，不修改 source、target 或 registry。

迁移使用 source `HarnessStore` 的 exclusive migration guard，复用普通 Store
writer 的 `.store.lock`。该 guard 从首次 source preflight read 持有到 staging、
校验和发布完成，因此遵守 Store 写入协议的普通 writer 会被阻塞，
快照与发布保持一致。guard 持有期间不得调用 source Store writer，
避免对同一把锁重入。

迁移在 target 同 parent 下构建 staging，验证 source 快照与 staged
ledger/directory，再次确认 target 不存在后，用一次 rename 将 staging
发布为 target。这是目录发布边界，不声称 target 与 registry/
`ACTIVE_SPACE` 之间具有 crash-atomic transaction。

manifest 先以 `registration.status: "pending"` 发布，并记录
`registration.recovery_command: "harness space switch <id>"`。register/activate 成功后
会 best-effort 将状态更新为 `complete`。如果 register/activate 失败，已完整验证的 target
保留且 manifest 保持 `pending`；运营者应按错误提示执行公开的
`harness space switch <id>` 恢复注册和激活，而不是期待迁移回滚。
成功的初次注册或恢复 switch 都会 best-effort 调和与该 id 匹配的
`pending` manifest；manifest 读取、解析或写回失败只输出 warning，
不否定已成功的 registry/`ACTIVE_SPACE` 切换。

这些保证以受信本地文件系统和合作方遵守 Store/registry 锁协议为
边界。实现会尽力检查路径、类型和 symlink，但不声称能抵御绕过
协议的 out-of-band root/path replacement 或恶意本地文件系统攻击。

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
- [x] custom Gate 持久化、旧 wire `{}` 规范化与默认 Store fail-closed
- [x] 精确重复 Gate 与多个 `code-review` Gate 拒绝
- [x] 当前候选的精确 durable ref / Review 绑定
- [x] Review performer/authority 持久化与固定 Host review authority
- [x] Store `accept_work` 最终入口强制 Gate 检查
- [x] CLI 拒绝已退役的 `--skip-gates`
- [x] Execution Space 迁移降级原项目 ledger 的 Review 绑定并记录计数
- [x] new-target-only、source Store exclusive guard、same-parent staged single-rename publish
- [x] registration pending/complete manifest 与保留 target 的 switch recovery

### Phase 3：Agent 管线（部分完成）

- [x] Worker context 注入 gates 声明
- [x] Store Work review 入口记录精确绑定 Review
- [ ] 按 strategy 自动编排 reviewer
- [ ] 将 reviewer 输出经类型验证后写入 Work review 入口

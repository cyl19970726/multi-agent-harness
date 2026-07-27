# Claude Integration

本文档定义 Star Harness 如何集成 Claude Code。目标是把 Claude 变成 harness 里
**持续存在**的 `AgentMember` provider：可创建、可投递、可 steer/interrupt、可
resume，并且把 Claude 原生 session 作为 transcript、tool activity 与 resume 的
唯一真相；Harness 只保存 binding、协调、显式 outcome 和 artifact/check refs。

Provider-neutral runtime contracts live in [../agent-runtime.md](../agent-runtime.md).
This file should explain only how Claude implements those contracts. Shared
object semantics such as `Task`, `Message`, `Evidence`, `Proposal`, and
`Decision` must not be redefined here.

## 当前状态：两个执行模式

| mode | 状态 | 形态 |
| --- | --- | --- |
| `claude_cli` | 保留，需显式指定 | `claude -p` 每次投递起一个进程 |
| `claude_agent_sdk` | **默认**，`review_required` | Agent SDK streaming input，进程常驻 |

两个模式都在 `(provider, execution_mode)` 白名单里。`claude_agent_sdk` 的运行时
在 `apps/claude-member-runner/`，Rust 侧由 `run_claude_agent_sdk_team_member`
经 NDJSON 驱动；`claude_cli` 的代码一行未改，但**不再是默认**。

```bash
--member "Name:Role:claude"              # 默认 → claude_agent_sdk，持续 member
--member "Name:Role:claude/agent-sdk"    # 同上，显式
--member "Name:Role:claude/cli"          # 旧的一次性模式，需点名
```

默认换过来的理由不是「新的更好」，而是：**默认到 `claude_cli` 等于默认到一个已
证明满足不了 ADR 0037 验收第 6 条的模式。** 一个 member 在队列瞬时为空时就消失，
不是可以当缺省的行为。

代价要说清楚：`claude_agent_sdk` 需要 `node` 和 runner 的依赖，`claude_cli` 只要
`claude` 二进制。找不到 runner 时不会静默退回——那正是这个模式要消除的失败——而是
显式报错并给出三条出路（指 `HARNESS_CLAUDE_MEMBER_RUNNER`、装依赖、或
`claude/cli`）。

`claude_agent_sdk` 的 profile 刻意把 `reviewed_provider_versions` 留空，
`interaction_mode` / `plan_mode` 也没有超过 `claude_cli` 的声明——interrupt、
steer 和 PreToolUse 拦截在 runner 里有实现且过了确定性测试，但没跑过真实
provider，所以不写进能力声明。这正是 `member providers --fail-on-review` 会把该
模式报成 review_required 的原因。

### `claude_cli` 的实测限制（不是设计选择，是缺陷）

2026-07-27 在同一台机器、同一个 provider、同一天实测：

```text
harness team-run start（claude_cli 路径）   → member 跑 1 轮后终止
apps/claude-member-runner（Agent SDK）      → 投递 → 空档 3s → 再投递，3 轮，同一 session
```

根因在 `run_claude_team_member` 的循环：

```rust
let queued = ledger.queued_messages_for(&member.id)?;
if queued.is_empty() { break; }        // 队列瞬时为空 == member 终止
```

Member 在队列**恰好为空**的那一刻停止存在。晚一毫秒到达的 TeamMessage 没有收
件人，它会永远停在 `queued`。这与 ADR 0037 的核心条款直接冲突：

> Member … owns its plan, Workspace, session … **until the Team Lead accepts its
> handoff through an ordinary Host acceptance `message`**.

也是 ADR 0037 §Acceptance 第 5、6 条至今没有任何测试覆盖的原因。

## `claude_cli`（已发布路径）

adapter 以内存方式消费 `claude -p --output-format stream-json --verbose`，从
`system(init)` 绑定真实 session id；显式重试通过 `resume_native_session_id`
调用 `--resume`。工具、命令、文件活动与对话不写入 `MemberAction`，Member 详情页
通过 `GET /v1/member-runs/{id}/native-activity` 读取 Claude 自己的 project
JSONL。thinking 在 reader 层直接丢弃。

启动一个 Claude member（实测可用）：

```bash
harness team-run create \
  --objective "…" \
  --member "SmokeMember:Smoke tester:claude/cli:claude-haiku-4-5"
harness team-run start --id <team_run_id>
```

member spec 是 `name:role:provider[/mode][:model][@owned,paths]`。

调用形状：

```bash
claude -p "{harness_message_envelope}" --output-format stream-json --verbose \
  [--resume {prior_session_id}] [--append-system-prompt {developer_instructions}] \
  [--model {model}] [--json-schema {schema}] \
  --permission-mode {mode} [--allowedTools {t1,t2}] [--mcp-config {path}] \
  [--add-dir {root}]
```

opt-in 的 resident 模式（`HARNESS_CLAUDE_RESIDENT=1`，ADR-0021）把
`claude --input-format stream-json` 保持常驻、逐 turn 喂 stdin frame。它解决了
进程反复启动，但**没有**双向控制通道。

### 成败判定依赖自由文本，这是个 false-negative 源

member 的成功与否由输出里有没有 `## RESULT` / `## SUMMARY` 段决定。实测：一个
objective 写成「只回 X 别的都不要」的 member，provider 正确执行并返回了 X，
harness 仍标记 `failed`。模型只要没按格式说话就算失败。`claude_cli` 下没有更好
的办法；`claude_agent_sdk` 下有（structured output / hooks），见下。

## `claude_agent_sdk`（目标路径，review_required）

`@anthropic-ai/claude-agent-sdk` **就是 Claude Code 打包成库**——同一个 native
二进制、同一个 `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl` 会话存储。选它不
是换 provider，是换调用面。

| 能力 | `claude -p` | resident stream-json | Agent SDK |
| --- | :---: | :---: | :---: |
| 持续 member | ❌ | ✅ | ✅ |
| interrupt（真实 ACK） | ❌ | ❌ | ✅ |
| steer（permission/model） | ❌ | ❌ | ✅ |
| hooks | ❌ | ❌ | ✅ |
| 会话注册表 | ❌ | ❌ | ✅ |

映射到 Harness 对象：

| Harness 契约 | SDK 原语 |
| --- | --- |
| 持续 member | `query({ prompt: AsyncIterable })` |
| Mailbox 投递 | 往那个 iterable push |
| Interrupt | `query.interrupt()` → `still_queued` |
| Steer | `setPermissionMode()` / `setModel()` |
| `native_session_id` | `system/init` 的 `session_id` |
| Provider session tag | `tagSession(id, "<team_run_id>:<member_run_id>")` |
| Provider session discovery | `listSessions({dir})` 按 tag 过滤 |
| 详情页原生活动 | `getSessionMessages(id)` |
| retry 不污染原会话 | `forkSession: true` |
| owned paths（ADR 0033） | `PreToolUse` 观察；不是 containment |
| 普通计划讨论（ADR 0039） | correlated `message`；没有工具闸 |
| `evidence_refs`（#232） | `PostToolUse` 观察 |

`tagSession` 用于 provider 侧发现；canonical 成员名册仍然是 Harness 的
`AgentTeam/MemberRun`。Harness 只保存 native-session binding，不复制 transcript。

### AGENTS.md 陷阱：streaming input 的消息形状

官方 TypeScript 文档的 "Streaming Input Mode" 示例是错的。它写：

```ts
yield { type: "user", content: [...] }        // ← 运行时拒绝
```

实际报 `Expected message role 'user', got 'undefined'`，SDK 子进程 exit 1。
正确形状在 SDK 自己的 `sdk.d.ts`：

```ts
type SDKUserMessage = {
  type: 'user';
  message: MessageParam;          // { role: 'user', content: [...] }
  parent_tool_use_id: string | null;
}
```

以 `.d.ts` 为准，不要以文档示例为准。

## Desktop 可见性

Claude Desktop 的会话列表**只列它自己创建的会话**。外部进程即使在
`~/.claude/sessions/<pid>.json` 注册得与真 desktop session 完全一致
（`kind:"interactive"`, `peerProtocol:1`, `entrypoint:"claude-desktop"`），也不
会出现在列表里——已实测。

但 desktop 注册了 `claude://` URL scheme，其中一个动作就是导入 CLI 会话：

```bash
open "claude://resume?session=<native_session_id>"
```

参数名是 `session`（不是 `sessionId`），值需通过 uuid 正则校验。desktop 侧执行
`importCliSession(id)`，日志为
`Imported CLI session <id> as Desktop session local_<id>`。

三个已验证的性质：

- **id 映射确定**：import 进来的会话是 `local_` + 原生 session id。（desktop
  自己新建的会话则是无关 uuid + 一条 `Mapping internal session X to CLI
  session Y` 日志，不确定。Harness 走 import，所以不需要对照表。）
- **import 会剥离 thinking**：`Stripped thinking blocks from …jsonl`。AGENTS.md
  的 thinking 政策由 provider 侧代为执行。
- **有 300 秒重复投递抑制**，连续试同一个链接会被静默吞掉。

`claude_cli` 和 `claude_agent_sdk` 两条路产出的会话都可以这样导入——都实测过。

### 权限模型：hook 才是边界，不是 permission mode

两个模式都默认 `permissionMode = bypassPermissions`。理由不是图省事，而是
**无人值守的 member 没有人能回答交互式权限提示**，留着那一层只会死锁。

关键是它**不会**关掉闸。2026-07-27 对真实 provider 实测（`gate-live.mjs`，
Haiku 4.5，`ownedPaths: ["owned"]`）：

| 情况 | 结果 |
| --- | --- |
| 写 lane 外 | `PreToolUse` 拒绝，**文件从未生成** |
| 写 lane 内 | 成功 |

即：`bypassPermissions` 跳过的是**提示层**，hook 照常执行且 `deny` 仍然优先。
正对照和负对照一样重要——否则「什么都写不了」会被误读成「闸生效」。

这一点对 **plan 闸**有意义——它是这里唯一还会拦的 hook。

### 没有 containment 边界，这是设计选择

三个 provider 的 member 都按设计跑在最大权限、全量工具：Claude
`bypassPermissions`、Codex `danger-full-access`、Kimi 的 headless `-p` 干脆拒绝
一切权限 flag。member 要跑构建、测试、git，交互提示也没人能答。

在这个前提下，**`owned_paths` 不做强制,只做观察**：

```text
写在 lane 外  → 发 cross_lane_write 事件，写照常进行
写在 lane 内  → 无事发生
```

不拦的理由不是做不到，而是拦了更糟：能拦住 `Write` 却拦不住 `echo >` 的东西不是
边界，是**长得像边界的东西**——它制造信任，而 shell 从旁边走过去。而且拦下来只会
把同一个改动推进 Bash 里，从 Host 眼前消失。观察反而让它在验收时可见:「这个
member 写到 lane 外面了，是有意的吗?」——那才是真正要问的问题。

`owned_paths` 因此就是 ADR 0033 里它本来的样子:**协作与验收用的声明 lane**。

真需要 containment 只能来自 OS——member 出不去的 worktree，或者容器——不可能来自
一个 PreToolUse matcher。

ADR 0039 同样删除了 **Plan 闸**。Host 若希望先审计划，就发送普通关联消息：
“先返回 Markdown 计划，不要执行”；Member 回复计划后，Host 再发送“修改”或
“执行”。Claude 的 native planning 可以作为 Member 内部能力，但不会改变 Harness
权限或生命周期。原因仍然相同：能拦 `Edit` 却拦不住 `Bash` 的 hook 不是可信边界。

### 并发边界（未验证，按保守规则操作）

import 之后 desktop 会 **接管** 会话（`startShellPty`、`replaceEnabledMcpTools`、
`Warming session`）。已验证的只有**顺序访问**：desktop warm 之后 Harness 用
`resume` 继续驱动，transcript 连贯追加、同一 session id、无分叉、无冲突日志。

**同时写入没有验证。** 在有人验证之前，操作规则是：**Harness 驱动期间，desktop
只做只读旁观。**

## Provider Runtime 模型

```text
AgentRuntime
  id / agent_member_id / provider = claude
  status = Running | Suspended | Closed
  pid = None（claude_cli 按需启动，无持久 pid）
  control_endpoint = "claude-runtime://{dir}"
  command = "claude"
  started_at / ended_at / last_event_at
```

健康检查三层（Codex 是四层，Claude 无 process 层）：

```text
endpoint: runtime directory exists + last_session within acceptable time
session:  NativeSessionRef resolves and native terminal state is readable
delivery: latest message delivery has proof of receipt from Claude
```

## Event Sources

1. **native session / stdout stream-json** — adapter 内存归一化，不复制成
   Harness ledger
   - `system(init)`：携带 `session_id`
   - `assistant` / `user`：content blocks（text / tool_use / tool_result）
   - `stream_event`：细粒度增量
   - `result`：终态帧，携带最终文本、usage/cost/model、可选 `structured_output`
   - native subagents（Task tool）出现在转录帧里，不是独立事件类型
2. **NativeSessionRef** — mode-aware 引用：provider、`native_session_id`、
   availability、provider version、adapter contract、resume capability
3. **Explicit promotion** — 只有 assignment、handoff、outcome、artifact/check
   refs、PendingInteraction 与控制确认进入 Harness

## Runtime Mapping

```text
system(init)   → NativeSessionRef.native_session_id
stream_event   → transient NativeActivityProjection
result         → explicit delivery outcome；Succeeded / Failed
无 result 帧    → Stale（有事件）/ Failed（空输出或进程失败）
assistant text → DeliveryOutcome.summary + Evidence
```

Queue discipline（来自 harness，不由 provider 定义）：

- 投递前 `queued`；投递中 `acknowledged`（原子 claim/lease）；成功 `delivered`
- claim/lease 原子性：`claim_queued_message_delivery` 必须在事件入库前原子提交

### 终止条件

`claude_agent_sdk` 把终止条件从「队列瞬时为空」换成「member 报告终态、且宽限窗口
内队列仍为空」。窗口默认 3 秒，可用
`HARNESS_CLAUDE_AGENT_SDK_IDLE_GRACE_MS` 调整。窗口内到达的 TeamMessage 由**同一
个 MemberRun、同一个原生 session** 消费。

这是缓解不是目标契约。ADR 0037 要求 member 活到 Host 的普通验收 `message`；那需要
`team-run start` 不再是前台编排，单独跟踪。

覆盖它的确定性测试是
`crates/harness-cli/tests/claude_agent_sdk_member.rs`：fake runner 在发出
`turn_complete` **之后**才回头 `team-run send`，所以「队列已空才到达」是被构造出
来的、不是赛跑出来的。

> ⚠️ 该原子 claim/lease **尚未实现**。当前 `mark_message_delivered` 是
> clone 整条消息、改自己那条 delivery、整条重新 append，折叠按 message id
> latest-wins。崩溃窗口丢消息与多收件人并发覆写都是真实风险。见 Issue #230。

## Permission Model

```json
{
  "provider": "claude",
  "provider_config": {
    "approval_policy": "none" | "prompt_required",
    "workspace_policy": "workspaceWrite" | "readOnly",
    "service_tier": "free" | "pro" | "team"
  }
}
```

落到 CLI 层是 `--permission-mode` + `--allowedTools`：read-only =
`Read,Grep,Glob`（无 Edit/Write/Bash），即 claude 的
`enforces_read_only = true`（对比 kimi 无法物理只读）。

在 `claude_agent_sdk` 下，owned paths 可以升级成 `PreToolUse` 的真实拦截，而不
只是协作/验收边界。

## Workspace Model

```text
{harness_root}/runtimes/{member_id}/                # runtime 目录标记
{harness_root}/runtimes/deliveries/{delivery_id}/   # ephemeral transport
```

worker cwd 取 member.worktree_ref → project root → process cwd。Claude 从 cwd
发现 `CLAUDE.md` / `.claude/`；SDK 下由 `settingSources` 控制加载哪些来源。

## Native Multi-Agent Features

Claude native subagents 自动成为 `ProviderChildThread`，**不升级为
`AgentMember`**：

> Child threads stay **under** the parent member, not promoted to members.

Subagent 是 stateless generation，不是持久线程——Harness 记录
`ProviderChildThread` 但不能 resume 同一个 subagent。若子代理需要与其他 members
通信，必须经 parent 中转。与 Codex 的 `subagent/collab_agent_spawn` 同原则。

## 鉴权与版本

- **凭据在 Keychain 的 `Claude Code-credentials`**，机器上所有 Claude Code 版本
  共用一份，`claude auth login` 一次全部生效。
- ⚠️ **`claude auth status` 报 `loggedIn: true` 不代表 token 有效**——它只查凭
  据存在。实测出现过 `status` 说已登录、实际调用返回
  `401 OAuth access token has expired`。以真实调用为准。
- ⚠️ **Agent SDK 的官方鉴权是 API key。** 文档明确：未经事先批准，不允许第三方
  产品向其用户提供 claude.ai 登录或额度，包括基于 Agent SDK 构建的 agent。
  自用 dogfood 走 operator 自己的订阅是另一回事；分发前必须处理。
- **品牌**：产品中不得使用 "Claude Code" / "Claude Code Agent" 命名。
- 安装 `@anthropic-ai/claude-agent-sdk` 会引入一个 **bundled native Claude Code
  二进制**——这是一次 provider 版本引入，按 AGENTS.md 需要人点名批准。当前
  reviewed 版本：**2.1.220**（SDK 0.3.220，commit 4073f595）。adapter 仍为
  `review_required`，直到 mode-specific 确定性检查 + live canary 通过。

## Validation Gates

`claude_cli`（已验证）：

- [x] `team-run create --member "…:claude/cli:<model>"` 声明落库
- [x] `team-run start` 真实启动 provider 并拿到回复
- [x] `system(init)` 绑定 `native_session_id`，availability=available
- [x] transcript 落在 `~/.claude/projects/`
- [x] `claude://resume?session=<id>` 导入 desktop

`claude_agent_sdk`（确定性接线已验证，live 兼容性仍为 `review_required`）：

- [x] 持续 member 跨空档存活，同一 native session 多轮
- [x] `tagSession` / `listSessions` 作为成员注册与发现
- [x] `claude://resume` 导入 desktop
- [x] resume-after-import 顺序访问连贯
- [x] Rust 侧 spawn + NDJSON 接线（`run_claude_agent_sdk_team_member`）
- [x] 空队列后到达的消息由同一 member/session 消费（确定性测试 + live 各一次）
- [ ] `PreToolUse` 拦截的 live 验证（单测已过，live 未跑）
- [ ] interrupt / steer 的 live 终态确认
- [ ] 同时写入的并发行为
- [ ] live canary → 退出 `review_required`

实现细节、协议与实测记录见 [`apps/claude-member-runner/`](../../apps/claude-member-runner/)
的 `README.md` 与 `FINDINGS.md`。已经完成或被架构决策取代的阶段性实现计划不作为
长期产品契约保留。

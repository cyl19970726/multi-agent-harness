# Agent Team — Kimi Code Plugin

Kimi Code 插件包：给 host 会话接入 Multi-Agent Harness 的 **Agent Team** 能力
(ADR 0025 / Issue #206) — 创建 AgentTeamRun、配置并拉起跨 provider 的
Codex/Claude/Kimi MemberRun、收发 ACK 消息、实时观察成员状态。

English summary: this plugin turns a Kimi Code session into the host (Lead) of a
cross-provider agent team. A sub-agent is one function call; a team member is a
living collaborator with its own state, mailbox, and responsibility domain. The
plugin ships the method (skills), the call surface (MCP), the plumbing (CLI
commands), and the nerves (hooks) — it contains no runtime logic of its own.

Product context is **Mission → ordered Wave → executor**. An `AgentTeamRun` is
one attempt for an `agent_team` Wave. The target ownership chain starts from
`TeamMessage(kind=assignment)` and its `correlation_id`. Automatic handoff
reuses the assignment correlation; manual CLI/API/MCP sends may pass
`correlation_id` and `causation_id` explicitly.

## 前置要求 / Prerequisites

- `harness` CLI 在 PATH 中（本仓库构建：`cargo install --path crates/harness-cli`）。
- 想用 Browser Team Console 时，先起常驻服务：
  `harness serve --addr 127.0.0.1:8787`，然后访问
  <http://127.0.0.1:8787/team-console>。
  CLI 文本视图是同一 read model 的紧凑投影，两者同一事实源。

## 安装 / Install

Kimi Code 内：

```text
/plugins install <github-url-or-local-path-to-this-plugin-dir>
```

例如本地安装：`/plugins install /path/to/multi-agent-harness/plugins/kimi-agent-team`。

manifest 为 `kimi.plugin.json`（优先于 `.kimi-plugin/plugin.json`）。

## 包含什么 / Contents

| 部件 | 内容 |
| --- | --- |
| Skills | `agent-team-orchestrator`（编排方法，会话开始自动加载）、`agent-team-member`（被拉起 member 的交付契约与 handoff 格式） |
| MCP server | `harness`（stdio，`harness mcp`）：Mission create/list、Wave create/list/gate，以及 TeamRun create/list/status/send/events |
| Commands | `/agent-team:new-run` 创建 run、`/agent-team:status` 紧凑状态表、`/agent-team:dashboard` 打开 Team Console |
| Hooks | `hooks/team-events.sh`：SessionStart 与 Stop 时注入一行 active run 摘要（run id / status / 未 ACK 数 / console URL），10s 超时，失败静默放行 (fail-open) |

## 使用 / Usage

1. `/agent-team:new-run` — 描述目标，确认 member 配置
   （`name:role:provider[:model][@ownedPaths]`，ownedPaths 两两不相交），
   插件组装并执行 `harness team-run create`，返回 run id 与 console URL，
   确认后 `harness team-run start`。
2. `/agent-team:status [run-id]` — 成员 / 状态 / 当前动作 / 心跳 / 未 ACK
   的紧凑状态表，附 Team Console URL。
3. `/agent-team:dashboard` — 打印并尝试打开
   <http://127.0.0.1:8787/team-console>（macOS `open`，Linux `xdg-open`）。

CLI 兜底（不经过插件也可用）：

```bash
harness mission create --title "..." --objective "..." --desired-outcome "..."
harness wave create --mission-id <mission-id> --title "..." --objective "..." \
  --executor-kind agent_team
harness team-run create --mission-id <mission-id> --wave-id <wave-id> \
  --objective "..." [--budget-usd X] \
  [--member name:role:provider[:model][@path1,path2]]...
harness team-run start --id <run-id>
harness team-run status --id <run-id> [--json]
harness team-run send --id <run-id> --from <id|host> --to <ids> \
  --kind <kind> --body "..." [--correlation-id <assignment-correlation>] \
  [--causation-id <message-id>]
harness team-run events --id <run-id> [--after-seq N] [--json]
harness wave gate --id <wave-id> --status accepted --run-id <run-id> \
  --accepted-by <actor> --note "..." --outcome "..." [--artifact <ref>]...
```

## 纪律 / Ground Rules

- **授权闸**：部署、删除远端资源、支付选型等外部变更必须上报用户拍板，
  member 与 host 都不得自行决定。
- **ACK 纪律**：handoff 与关键任务消息必须 ACK；超阈未 ACK 会重发并升级告警。
- **归属**：`TeamMessage(kind=assignment)` 的 message id 与 `correlation_id`
  是 lane 的目标主身份。自动 handoff 会复用它；手工 blocker / progress /
  review 消息应传入同一 assignment correlation，或通过同一 run 的
  `causation_id` 继承。
- **判断标准**：结果需要回到我的上下文 → sub-agent；结果留在执行者那里、
  我只留指针 → Agent Team member。member 自主调用自己的原生 sub-agent，
  harness 只捕获归属、不调度。
- 每次状态输出都必须带 Team Console URL。

## 卸载 / Uninstall

Kimi Code 内 `/plugins uninstall agent-team`，或直接移除本目录。插件不向
仓库写入任何运行时文件；harness store 中的 run 历史不受影响。

## 参考 / References

- [ADR 0025: Agent Team Run Control Plane](../../docs/decisions/0025-agent-team-run-control-plane.md)
- [Team Run Console page spec](../../docs/dashboard/pages/team-run-console.md)
- [Concept model](../../docs/concept-model.md)

# Multi-Team Supervisor Daemon

```text
status: stable
owner: lead-operations
last reviewed: 2026-08-08
authority class: implementation reference
canonical_for: multi-team supervisor daemon architecture, control socket protocol, and current implementation state
```

> 本文件替代早期 `specs/supervisor-daemonization/multi-team.md`（设计意图，未落地）。本文件的
> 一切描述以 `crates/firm-cli/src/supervisor_daemon.rs` 当前代码为准；代码是 executable truth。

## 心智模型

Agent Team run 的驱动力是 supervisor：它负责投递轮次、心跳租约、控制监听。
Per-run supervisor daemon 每个 team-run 一个进程；multi-team daemon 用一个进程管理 N 个 run。

```
BEFORE (per-run supervisor daemon, 已接线):
  $ firm team-run start --id run-A    # spawn/adopt run-A 的独立 daemon 进程

AFTER (multi-team daemon, 核心逻辑已合并 #399, CLI 未接线):
  $ firm daemon supervisor serve      # 一个常驻进程扫描 store 内所有 active run
  $ firm team-run start --id run-A    # (计划) 经 control socket 委托给 daemon
```

multi-team daemon 是 per-run daemon 的**超集演进**：per-run daemon 作为回退路径保留。

## 实现现状（#399 合并后的真实状态）

| 部分 | 状态 | 说明 |
| --- | --- | --- |
| `MultiTeamDaemon` 核心（结构、run、serve_loop、scan/adopt/reap） | **已合并** | `supervisor_daemon.rs` 585 行起 |
| `recover_orphaned_runs` 崩溃收养 | **已合并** | 启动时枚举非终止 run，收养租约过期者 |
| Control socket（start/status/stop） | **已合并** | `multi_team_socket_path` + 行分隔 JSON |
| 优雅停机（drain + join deadline） | **已合并** | SIGTERM/SIGINT + socket `stop` 命令 |
| CLI 命令入口（`MultiTeamDaemon::run` 的调用点） | **未接线** | main.rs 无任何 `MultiTeamDaemon` 引用 |
| `team-run start` 委托 daemon | **未接线** | `try_delegate_to_daemon` 定义于 daemon 模块，main.rs 未调用 |
| `daemon_status_via_socket` / `daemon_stop_via_socket` | **未接线** | 同上 |
| 集成测试覆盖 multi-team 路径 | **未覆盖** | `tests/team_run_daemon.rs` 目前覆盖 per-run `serve` 路径 |

**结论**：#399 交付了 multi-team daemon 的完整引擎，但把它接进 CLI 的工作
（一个 `daemon multi-team serve` 入口 + `team-run start` 的 socket 优先委托）是下一波。
当前生产路径仍完全走 per-run supervisor daemon（`team-run start` → spawn/adopt）。
写文档时不要按"已上线"叙述——它现在是**可独立运行的库级组件**。

## 组件结构

### 核心结构

```rust
struct MultiTeamContext {
    run_id: String,
    heartbeat_valid: Arc<AtomicBool>,        // false 时 drive_prepared_team_run 快速退出
    thread: Option<JoinHandle<CliResult<()>>>,
    started_at: Instant,
}

struct MultiTeamDaemon {
    store: HarnessStore,
    contexts: Mutex<Vec<MultiTeamContext>>,  // 上下文注册表，仅短临界区
    max_concurrency: usize,                  // 并发 run 上限（默认 4）
    idle_timeout_secs: u64,                  // 默认 300
    scan_interval: Duration,                 // 扫描周期
    shutdown: Arc<AtomicBool>,               // 信号 + socket stop 共用
}
```

### 主循环（serve_loop）

```
while !shutdown:
    scan_and_adopt()      # 扫描 store，收养未管理且租约无效的 Running run
    reap_finished()       # 收割已结束的 supervisor 线程
    poll_control_socket() # 非阻塞 accept 一条命令
    sleep(scan_interval)  # 100ms 粒度、shutdown-aware
```

### 收养条件（scan_and_adopt）

对每个 `TeamRunStatus::Running` 且未被管理的 run，检查
`latest_team_supervisor_lease`：租约不存在、非 `Active`、或已过期 → 收养。
有活跃租约（别处有活 supervisor）→ 跳过。这保证多实例共存时不会双驱动。

### 优雅停机（graceful_shutdown）

1. drain 注册表，取出全部 context；
2. 每个 context 的 `heartbeat_valid` 置 false —— 驱动循环检测到租约失效自行退出；
3. 以 30s 为 deadline 轮询 `thread.is_finished()` 并 join（`std::thread` 无 join_timeout，
   用带 deadline 的轮询替代，250ms 粒度）；
4. 超时放弃，记录 run 名后继续收尾。

## Control socket 协议

Unix-domain socket，路径 `<store-root>/supervisor.sock`（路径超 macOS AF_UNIX 104 字节
限制时，用 store-root 的 hash 落到 `/tmp/firm-supervisor-<hash>.sock`）。

行分隔 JSON，非阻塞单命令/轮询：

```
→ {"cmd":"start","run_id":"team-run-..."}
← {"ok":true,"run_id":"team-run-..."}            # 失败时 {"ok":false,"error":"..."}

→ {"cmd":"status"}
← {"ok":true,"runs":[{"run_id":"...","status":"running|finished","elapsed_secs":N},...]}

→ {"cmd":"stop"}                                  # 真正置 shutdown，触发主循环退出
← {"ok":true}
```

socket 与 resident daemon socket（`resident.sock`，claude 温池）分属不同子系统，互不相关。

## 并发与锁纪律（含 #378 review 的 P0 修复）

- **P0-1 停机真信号**：`stop` 命令与信号处理都落 `shutdown`/`heartbeat_valid`，不假应答。
- **P0-2 并发上限 + 收养前验租约**：`start_supervising` 检查 `contexts.len() >= max_concurrency`
  拒绝超载；adopt 前查租约避免双驱动。
- **P0-3 join 带 deadline**：停机等线程 30s 上限，不无限挂死。
- **P0-4 错误传播**：socket `start` 响应返回真实 `prepare_team_run_start_body` 错误，
  不只是 "delegated to daemon"。
- **P0-7 锁纪律**：扫描与收养不在持锁状态下做 store I/O；锁只保护注册表短临界区。
- **P0-8 信号处理**：`AtomicBool` + channel 风格，无 static raw pointer（修 UB）。

## 接线计划（下波工作，尚未开始）

1. CLI 入口：`firm daemon supervisor serve`（multi-team 变体，不需要 `--team-run-id`），
   或新命令 `firm daemon multi-team serve`；参数 `--max-concurrency` `--idle-timeout-secs` `--scan-interval-secs`。
2. `team-run start` 委托：socket 可达 → `try_delegate_to_daemon` 后立即返回；
   socket 不可达 → 现有 per-run spawn/adopt 回退（已满足无回归）。
3. `team-run status` 支持 `daemon_status_via_socket` 聚合报告。
4. 集成测试：multi-run 并发、崩溃重启收养、停机 drain、socket 协议负例。
5. 接线后把 `docs/current/architecture/cli-map.md` 的 daemon 行更新为 multi-team。

## 相关文档

- `docs/current/architecture/cli-map.md` — CLI 命令现状（daemon 行待接线后更新）
- `docs/current/architecture/architecture-map.md` — 运行时架构总览
- `docs/current/operations/operations.md` — operator 运行手册

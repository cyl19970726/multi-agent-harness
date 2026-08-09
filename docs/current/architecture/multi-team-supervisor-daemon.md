# Multi-Team Supervisor Daemon

```text
status: stable
owner: lead-operations
last reviewed: 2026-08-09
authority class: implementation reference
canonical_for: multi-team supervisor daemon architecture, control socket protocol, and current implementation state
```

> 本文件取代 PR #396 中未合入的设计文档（design intent）。本文件的一切描述以
> `crates/firm-cli/src/supervisor_daemon.rs` 当前代码为准；代码是 executable truth。

## 心智模型

Agent Team run 的驱动力是 supervisor：它负责投递轮次、心跳租约、控制监听。
Per-run supervisor daemon 每个 team-run 一个进程；multi-team daemon 用一个进程管理 N 个 run。

```
BEFORE (per-run supervisor daemon, 已接线):
  $ firm team-run start --id run-A    # spawn/adopt run-A 的独立 daemon 进程

AFTER (multi-team daemon, #415 已恢复生产接线):
  $ firm daemon serve               # 一个进程管理多个 team-run
  $ firm team-run start --id run-A  # socket 可达时委托；不可达时回退到 per-run daemon
```

multi-team daemon 是 per-run daemon 的**超集演进**：per-run daemon 作为回退路径保留。

## 实现现状（#415）

| 部分 | 状态 | 说明 |
| --- | --- | --- |
| `MultiTeamDaemon` 核心（结构、run、serve_loop、scan/adopt/reap） | **已合并** | `supervisor_daemon.rs` 585 行起 |
| `recover_orphaned_runs` 崩溃收养 | **已合并** | 启动时枚举非终止 run，收养租约过期者 |
| Control socket（start/status/stop） | **已合并** | `multi_team_socket_path` + 行分隔 JSON |
| 优雅停机（drain + join deadline） | **已合并** | SIGTERM/SIGINT + socket `stop` 命令 |
| CLI 命令入口（`MultiTeamDaemon::run` 的调用点） | **已接线** | `daemon serve` 支持并发、空闲超时与扫描周期参数 |
| `team-run start` 委托 daemon | **已接线** | socket 可达时委托；仅在 socket 不存在或拒绝连接时回退 per-run daemon；不确定通信错误 fail closed |
| `team-run status` 聚合 | **已接线** | 文本和 JSON 输出都包含 multi-team daemon 状态 |
| 集成测试 | **已恢复并扩展** | 覆盖 multi-run、重复 start、崩溃重启收养、优雅 drain、双 daemon 排他与协议负例 |

**结论**：#415 恢复了 #399 的 CLI 入口，并补齐 start 委托、status 聚合、单实例
保护和 control socket 的有界读取。multi-team daemon 是首选路径；per-run supervisor
daemon 继续作为 daemon socket 明确不可达时的兼容回退。

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
    if scan_due:
        scan_and_adopt()      # 按 scan_interval 扫描 store
        reap_finished()       # 收割已结束的 supervisor 线程
    poll_control_socket()     # 20ms 轮询；增量读取所有挂起连接
    sleep(20ms)               # 控制面响应不再绑定 store 扫描周期
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

行分隔 JSON。daemon 会排空已经排队的连接；每条命令最多 64 KiB，必须在 1 秒内
收到换行。空探针被忽略，超时、非法 UTF-8、畸形 JSON 和未知命令返回结构化错误，
不会终止服务进程。

```
→ {"cmd":"start","run_id":"team-run-..."}
← {"ok":true,"run_id":"team-run-...","reused":false}
                                                   # 重复 start 为 reused:true
                                                   # 失败时 {"ok":false,"error":"..."}

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

## 相关文档

- `docs/current/architecture/cli-map.md` — CLI 命令现状
- `docs/current/architecture/architecture-map.md` — 运行时架构总览
- `docs/current/operations/operations.md` — operator 运行手册

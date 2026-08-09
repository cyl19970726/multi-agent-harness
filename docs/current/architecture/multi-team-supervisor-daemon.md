# NodeDaemon Runtime

```text
status: canonical runtime contract
owner: lead-operations
last reviewed: 2026-08-09
canonical_for: one-machine NodeDaemon, Team placement, child supervision, and recovery
```

## Model

One logical Firm may span machines. Each machine has exactly one stable
`ExecutionNode` identity and one `NodeDaemon`. An `AgentTeam` is placed on one
Node and its Members never execute across machines.

```text
Firm
├── Node A → one NodeDaemon → Team A runs, Team B runs
└── Node B → one NodeDaemon → Team C runs
```

There is no public per-TeamRun daemon and no fallback that silently starts one.
`firm team-run start`, HTTP, and MCP all validate the exact Team, Node,
Execution Space, and project binding, then delegate to the NodeDaemon. An
unreachable daemon is an explicit `NODE_DAEMON_UNAVAILABLE` failure.

## Durable authority

- `ExecutionNode` is the stable machine identity.
- `NodeProjectRegistration` admits an Execution Space/project binding to a Node.
- `NodeDaemonLease` gives one daemon generation authority over one registered
  Execution Space.
- `TeamSupervisorLease` is a child lease fenced by Node id, daemon id, daemon
  generation, Execution Space, and project binding.
- A stale child cannot heartbeat or write after its parent generation changes.

The daemon scans every registered Execution Space independently. A corrupt or
busy store is reported and isolated; it does not stop supervision in another
space. On restart the new daemon generation recovers eligible non-terminal
TeamRuns without duplicating provider delivery.

## Control protocol

The machine socket is `<FIRM_HOME>/nodes/<node_id>/daemon.sock`, with a hashed
`/tmp` fallback for Unix path-length limits. Requests are bounded newline JSON:

```json
{"cmd":"start","execution_space_id":"space-a","run_id":"team-run-1"}
{"cmd":"status"}
{"cmd":"stop"}
```

Repeated start is idempotent and returns `reused: true`. A request naming a
Team placed on another Node, an unregistered project, or a mismatched Execution
Space is rejected before child supervision starts.

## Operator surface

```bash
firm node init
firm node project register --project-binding-id <id>
firm daemon start
firm daemon status
firm team-run start --id <run-id>
firm daemon stop
```

`daemon serve` exposes concurrency, idle-timeout, and scan-interval tuning for
foreground operation. Tests that claim runtime behavior must start a real
NodeDaemon and cover at least two isolated Execution Spaces.

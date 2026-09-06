# firm-node-daemon

Machine-scoped NodeDaemon lifecycle library, composed by the existing CLI
binary. It owns authority renewal, the managed Team registry, control serving,
adoption/reaping, recovery and stop/drain ordering. It does not create provider
adapters, compose Work, parse native transcripts or spawn the CLI binary.

## Module ownership

| Module | Responsibility |
|---|---|
| private `supervisor_daemon` | Foreground machine loop and private context/registry ownership |
| private `machine_authority` | Machine lease acquisition/renewal and authority-loss fencing |
| private `team_supervision` | Adoption, Supervisor thread lifecycle and reaping |
| private `control_protocol` | Existing socket command serving, auth and dispatch |
| private `recovery` | Existing managed-run recovery coordination |
| private `shutdown` | Existing stop/drain and authority release |
| private `self_stop_events` | Existing machine authority-loss phase evidence |
| `daemon_application_port` | Finite injected CLI operations and owned prepared/session handles |
| `daemon_error` | Shared typed error and unchanged Display |
| `daemon_protocol` | Shared existing native-read/wake DTOs and serde |
| `daemon_support` | Existing neutral time/path/run-state helpers |
| `lease_renewal_diagnostics`, `scan_diagnostics` | Existing process-local status observations |
| `start_failure_classification` | Existing typed transient-start classification |

`firm-cli` retains `daemon_application`, `daemon_client`, provider-specific
creation/control, prepared TeamRun drive, native readers, execution-space
configuration, Message composition and predecessor recovery application calls.
There is one daemon implementation, no second binary or reverse CLI dependency.

## Public boundary

The entry point is foreground `run`; root also exports the socket path and
existing stop/transient-read constants. `DaemonError` / `DaemonResult` keep typed
Store/CAS/Unknown cases. Protocol read/wake types keep their original wire shape.
The application port consists of the named methods in `DaemonApplicationPort`,
`PreparedRun` and `NodeSessionHandle`, together with their concrete argument and
result types. Prepared handles remain owned, Send and one-shot; their CLI Drop
and registration cleanup sequence is unchanged.

The one added named operation, `message_body_digest`, forwards the exact existing
CLI fabric SHA256 helper over unmodified body bytes. The daemon has no fabric
or provider-specific dependency. Five CLI provider error converters replace
orphan-rule-invalid From implementations with the same match branches; errors
are not flattened across this boundary.

Direct dependencies are firm-core, firm-store, firm-runtime-host, neutral
firm-provider-events DTOs, serde, serde_json and thiserror. Store never depends
on this crate. The package gate checks the actual dependency graph, source
ownership, imports and default feature guards.

## Tests

`test-support` is default-disabled and enabled only by the CLI dev-dependency.
Its finite adapter wraps the same private daemon and preserves the existing
real CLI fixtures. It does not export a registry, lock guard or Deref. Shared
stop flags and owned synthetic thread/context inputs preserve fixture ownership.
Normal production builds cannot access this adapter.

See [the complete migration and adapter map](TEST-MIGRATION.md): 56 original
coupled scenarios remain in CLI, while six pure recovery tests follow the
library. Shared protocol and start-error unit tests also follow their code.

# Claude Residency

```text
status: limited provider optimization
canonical_for: in-process Claude session reuse only
```

Claude provider code may reuse a child/session inside the runtime process and
may resume a provider-native session after transport loss. This optimization
does not own TeamRun lifecycle or machine authority.

The former per-workspace `resident.sock` daemon and its `firm daemon` commands
are retired. `firm daemon start|serve|status|stop` now exclusively controls the
machine-scoped NodeDaemon described in
[NodeDaemon Runtime](../architecture/multi-team-supervisor-daemon.md).

Provider-native storage remains the source of transcript, turn, tool, and
resume truth. Firm stores only the normalized session locator and coordination
evidence needed to resume safely.

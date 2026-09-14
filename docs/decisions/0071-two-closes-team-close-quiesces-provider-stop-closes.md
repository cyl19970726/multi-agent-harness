# ADR 0071: Two Closes — Team Close Quiesces the Session, Provider StopSession Closes It

```text
status: accepted
owner_role: architecture
amends: ADR 0049 (Close/Reopen semantics), ADR 0065 (epoch ownership)
canonical_for: the two distinct Close operations, which one writes
  AgentSession `closed`, and why Team Host authority cannot reach a Session
```

## Context

One word, "Close", names two different operations on this checkout, and the
documentation collapsed them. Readers of
[ADR 0049](0049-member-coordination-and-runtime-lifecycle.md) and
`docs/current/architecture/member-continuation-model.md` reasonably concluded
that a Host Close ends the provider session and that Reopen resumes that same
session row. Neither is what the code does.

The confusion has a cost. It makes a Team Host look like it can terminate a
machine-owned AgentSession, which the Trust Kernel explicitly refuses; and it
hides the fact that a session row observed after a provider Close is a
different row from the one observed before it, even though both carry the same
provider-native session id.

## Decision

### 1. Team Close ends one MemberRun generation and quiesces the Session

The Host verb `close-member`
(`crates/firm-cli/src/main_modules/team_run_cli.rs:800`, HTTP
`POST /v1/team-runs/{run}/members/{member}/close`) latches the close, cancels
the current provider turn where one is running, releases the Harness-owned
adapter process, and writes `MemberRun.coordination_status = closed` with
`status = stopped`
(`crates/firm-cli/src/main_modules/member_lifecycle.rs:600-612`).

Against the AgentSession it does exactly one thing: if the session is not
already `idle`, it transitions it to `idle`
(`crates/firm-cli/src/main_modules/member_lifecycle.rs:549-559`). It never
writes `closed`. The comment above that branch states the rule the Store also
enforces: "the Team lifecycle may only quiesce the machine-owned Session; it
cannot stop it or rewrite another Team's bindings."

### 2. Provider Close is a settled `StopSession` RuntimeCommand

`AgentSessionStatus::Closed` is written on two paths, both gated on the same
exact settled `StopSession`:

- the NodeDaemon control protocol, when a `StopSession` RuntimeCommand settles
  (`crates/firm-node-daemon/src/supervisor_daemon/control_protocol.rs:709-717`;
  the sibling `ResumeSession` writes `Cold`);
- the runtime-effect projection, whose desired-status ladder admits every
  non-`Closed` source to `Closed`
  (`crates/firm-cli/src/main_modules/runtime_effects.rs:286-292`; an `Active`
  lane goes through `Interrupted` first) but refuses the step unless exactly
  one `StopSession` command matching the
  session id, session generation, daemon id, and daemon generation is
  `Settled`/`Applied`, and then carries that command's idempotency key into the
  Store transition (`:327-350`).

There is no third path and no ungated one.

The Store admits the write only on an ordinary edge — `Idle -> Closed`,
`Waiting -> Closed`, `Interrupted -> Closed` — or, from `Cold`/`Active`, under
an `authorized_stop` proof that binds the write to that exact prepared
`StopSession` command, its target session generation, and its target
NodeDaemon generation
(`crates/firm-store/src/trust_kernel/fabric_identity_sessions.rs:634-653`,
`:702-729`).

### 3. `closed` is terminal

`Closed` never appears as a source in the AgentSession transition table
(`fabric_identity_sessions.rs:702-729`), and neither narrow re-entry lane
(`resumes_terminated_interrupted_lane`, `recovers_reconciled_lane`,
`:688-701`) starts from it. A closed session is not reopened; a closed session
row is not rewritten.

### 4. Team Host authority is Team-scoped and cannot reach a Session

An AgentSession-targeted RuntimeCommand requires exact self — the AgentMember
that owns the session — or the exact machine NodeDaemon/Operator Service actor.
The Trust Kernel rejects everyone else with
"AgentSession RuntimeCommand requires exact self or exact machine
NodeDaemon/Operator authority; Team Host authority is Team-scoped only"
(`crates/firm-store/src/trust_kernel/fabric_runtime_commands.rs:382-396`). A
Team Host is neither actor, so it cannot issue the provider Close at all. This
is why Team Close quiesces rather than stops.

### 5. Continuity across a provider Close is by native session id, not row id

At most one non-`Closed` AgentSession exists per AgentMember per Execution
Space (`fabric_identity_sessions.rs:134-150`), and closed rows are retained
rather than deleted. After a provider Close the member therefore has no current
session, so the next adoption pass takes the mint branch and creates a **new**
AgentSession row
(`crates/firm-cli/src/main_modules/member_orchestration.rs:228-285`). Its id is
`agent-session:<member>:<node>:<daemon generation>:<MemberRun runtime
generation>`, and its `native_session_ref` is read back from
`MemberRun.native_session`, so the new row carries the same provider-native
session id as the closed one.

The provider-native session is the continuity; the AgentSession row is not.

### 6. Reopen advances the MemberRun epoch only

Reopen increments `MemberRun.runtime_generation`, clears `finished_at`, returns
coordination to `active`, and resets `status` to `queued` (or `idle` for
`external_interactive`)
(`crates/firm-cli/src/main_modules/http_member_control.rs:546-559`). It writes
no AgentSession field. `AgentSession.runtime_generation` is the provider-session
epoch and is immutable per row ([ADR 0065](0065-two-runtime-epochs.md)); the
only production writers set it at mint
(`member_orchestration.rs:269`, `http_trust_routes.rs:538`).

## Consequences

- ADR 0049's `### Close`, `### Reopen`, and their Consequences are about the
  MemberRun. That ADR carries an amendment banner pointing here, and its MCP
  Reopen surface is marked retired by
  [ADR 0061](0061-retire-harness-coordination-mcp.md).
- `docs/current/architecture/member-continuation-model.md`,
  `docs/current/architecture/agent-runtime.md`, and
  `docs/current/integration/native-session-storage.md` name which Close they
  mean wherever the distinction changes the reader's conclusion.
- `AGENTS.md` Hard Invariant 7 says "Team Close" rather than "Close".
- No schema, wire format, store layout, or behavior changes. This ADR records
  what the checked-out code already does.

## Validation

- `cargo test -p firm-cli --test team_run_api close_cancels_kimi_provider_request_without_resuming_member`
  exercises a Team Close of a member blocked on provider input and asserts the
  MemberRun reaches `closed`/`stopped`.
- `crates/firm-store/src/trust_kernel/fabric_identity_sessions.rs` transition
  tests cover the admitted and refused `Closed` edges.
- `pnpm check:native-session-boundary` keeps the canonical native-session
  documents consistent with the boundary rules.

# ADR 0049: Split Member Coordination And Runtime Lifecycle

> Successor note (DOC-16 Keep row, DEV-40 flip 2026-08-18): this ADR is kept; the governing successor context is [DOC-105](https://app.notion.com/p/3be49a4fa379817aa594fd8e7331c30d).

```text
status: accepted
owner_role: architecture
canonical_for: MemberRun Close, Reopen, Retire, mailbox freezing, runtime
  generations, and provider-native history continuity
```

## Context

A persistent Agent Team member has two different lifecycles:

1. its durable participation identity and mailbox inside a TeamRun; and
2. the disposable adapter process currently attached to its provider-native
   conversation.

The previous model collapsed both into `MemberRun.status=stopped`. Close could
release a managed adapter, but there was no explicit same-MemberRun reopen.
Operators therefore had to create a replacement MemberRun or start a new run,
which obscured whether the same Work, mailbox, and provider history were
continuing. External interactive members made the ambiguity sharper because
Harness never owns their process or native conversation.

## Decision

`MemberRun` carries a durable `coordination_status` independent of provider work
status:

```text
active -> closed -> active   # explicit Reopen
active|closed -> retired     # permanent
```

It also carries a monotonic `runtime_generation`, starting at 1. Reopen keeps
the same MemberRun id and increments this generation so an active Supervisor
can start a new adapter process even though it has already observed that
MemberRun id.

### Close

Close is reversible runtime shutdown, not deletion or retirement.

- Harness durably latches Close and changes coordination to `closed` before
  releasing a process-local control handle.
- A managed Codex, Claude, or Kimi adapter terminates its Harness-owned process.
- The MemberRun, Work ownership, mailbox rows, NativeSessionRef, and
  provider-native transcript remain.
- Mail queued before Close is frozen. Closed members cannot send, receive,
  claim, or acknowledge mail; ordinary delivery never reopens them.
- TeamRun completion never implies Close (nor did the retired Wave/Mission
  completions).

### Reopen

Reopen is an explicit control operation on the same MemberRun.

- It requires `coordination_status=closed` and a non-active runtime state.
- It increments `runtime_generation`, clears `finished_at`, and returns
  coordination to `active`.
- For a managed member, both the captured provider profile and NativeSessionRef
  must support resume. Missing or incompatible native sessions fail visibly.
- A member closed before any native session was ever created may reopen into
  its first session; this is labelled `no_native_session_yet`, not history
  continuity. A missing session after failed execution is never replaced.
- Harness starts a new adapter process and invokes the provider's verified
  native resume operation with the recorded native session id. It never builds
  a transcript from TeamMessages and never silently substitutes a fresh
  session.
- Frozen mail becomes actionable again after coordination is active.

An active Supervisor notices the higher runtime generation and starts it. If no
Supervisor exists, Dashboard/HTTP and MCP Reopen start one; CLI Reopen reports
that `team-run start` is required.

### Retire / Deactivate

Deactivate sets `coordination_status=retired`. Retired is the permanent
coordination end: messages, ACK, runtime start, and Reopen are rejected. A Host
that needs the actor again must create a new MemberRun and Assignment.

### External interactive members

For `external_interactive`, Close and Reopen apply only to the Harness
coordination binding. Harness does not stop, restart, or prove continuity of
the user's external process or conversation. Reopen still preserves the same
MemberRun, correlation history, and frozen Harness mailbox; provider history
continuity remains user-owned and cannot be claimed as Harness evidence.

## Consequences

- Dashboard shows provider work status and coordination status separately and
  exposes Reopen only for closed members.
- Supervisor deduplication is keyed by `(member_run_id, runtime_generation)`,
  not MemberRun id alone.
- Hooks and all message/ACK entry points require active coordination.
- Historical MemberRun rows deserialize as `coordination_status=active` and
  `runtime_generation=1` for compatibility.
- `stopped` alone no longer means permanent retirement.

## Validation

- schema fixtures reject generation zero;
- external-member acceptance proves queued mail freezes on Close and thaws on
  same-MemberRun Reopen;
- managed Codex acceptance proves generation 2 calls `thread/resume` with the
  exact prior native session id;
- CLI, HTTP, MCP, Dashboard, hook, and Skill checks share the same semantics.

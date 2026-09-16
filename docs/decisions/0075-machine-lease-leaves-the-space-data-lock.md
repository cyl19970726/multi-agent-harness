# ADR 0075: The machine-authority lease leaves the Execution Space data lock

```text
status: Accepted — Owner 2026-09-15 (CORE5-E2); implementation tracked by Task CORE5-E2-20260915;
        amended 2026-09-15 (predecessor history; corrected fence list) before any code was written
date: 2026-09-15
amends: ADR 0042 (the machine lease stops being Execution Space data); ADR 0044 (the NodeDaemon
        parent fence reads a file, not a Space ledger row); AGENTS.md "machine-scoped authority";
        docs/current/architecture/agent-runtime.md:16-29
canonical_for: where machine authority is stored, which lock protects it, how it is renewed and
        read, and how its generation is minted
baseline: master 35bcda73 for the evidence; line citations re-verified at master fccbf4cc,
        which is where E2a executes this checklist from
```

## Context

A NodeDaemon's machine authority is a **lease row inside every Execution Space's data
store**. `acquire`/`renew`/`drain`/`release` all append to that Space's
`node_daemon_leases.jsonl` while holding that Space's `.store.lock`
(`store_node_runtime.rs:273-343`, `:368-424`, `:431-470`, `:472-506`) — the same lock that
serializes every ordinary data mutation there. That coupling has a measured price:

- **Data writes are O(whole file) under that lock.** A Work transition rewrites
  `agentfirm_trust_operations.jsonl` end to end (`trust_foundation.rs:803-823`: truncate
  `.next` → serialize every envelope → flush → `sync_all` → rename → `sync_all` on the
  directory; two fsyncs). Measured in the K-tier review of #951 on a copy of the live
  dogfood store: **8.75 MB / 1,786 envelopes → ~1.45 s per Work write** (#952). I
  re-measured the inputs first-hand on the S1 copy: 8,752,864 bytes, 1,786 rows.
- **The heartbeat waits in that same queue, with a budget equal to its own remaining
  life.** `renew_node_daemon_lease_cancellable` pre-reads the confirmed expiry and sets
  `budget = remaining TTL` (`store_node_runtime.rs:382-391`), then queues on `.store.lock`.
  A renewal therefore cannot fail *before* expiry — and cannot succeed after a queue
  longer than the TTL either. TTL is `max(scan_interval*4, 15_000)` ms
  (`machine_authority.rs:551-563`); renewal runs every `remaining/4`, capped at 5 s
  (`:40-44`, `:586-616`, `:676-711`).
- **Every renewal also rewrites the lease ledger.** `compact_node_daemon_leases_unlocked` runs
  on each renewal (`store_node_runtime.rs:415`, `store_jsonl.rs:195-251`): full read + temp +
  rename + directory fsync, added by #811 because append-only heartbeats grew the ledger without
  bound (one uncompacted copy in evidence: 87,354 rows / 28 MB).

**The only journaled machine-authority loss in the live dogfood store is exactly this
failure.** From the S1 copy (`recovery-dogfood-20260910`, byte-identical to source), daemon
generation 4, four `node_daemon_self_stop` TeamRunEvents:

```text
reason NODE_DAEMON_MACHINE_AUTHORITY_LOST
error  timed out waiting for store write lock …/recovery-dogfood-20260910/.store.lock;
       lock_wait_ms=14996; lock_budget_ms=14994; confirmed_expires_unix_ms=1789043767671
phases shutdown_initiated → lease_lost → process_groups_terminated [20147] → shutdown_complete
```

No authority check refused that renewal. It spent **14,996 ms queued on a data lock against
a 14,994 ms budget** and lost the machine by two milliseconds of queueing. From the same
ledger: expiry 1789043767671, `released` only at 1789050648990 (**6,881 s later**, by
operator `recover-predecessor`), successor generation 5 acquired 50 s after that —
**6,931 s with no machine authority**. #796 and #836 are earlier instances; #937 is the
Supervisor-lease analogue (`lock_wait_ms=2001; lock_budget_ms=2000`).

Nothing is wrong with the fence: across the audited stores, 0 double drives in 884 session
events, 0 cross-generation settlements in 405 prepare/settle pairs, 0 reused generations.
**The fence design is defending against a problem the storage design creates.**

Two further facts make today's shape hard to defend on its own terms. First, **the machine
scope is reconstructed, not stored**: generations are Space-local counters
(`store_node_runtime.rs:323`, one per Space store), and "machine-wide" exists only because
`ensure_node_authority_bundle` takes the whole set at once and treats any member's failure
as total loss (`machine_authority.rs:190-315`) — a rule written down only in
`agent-runtime.md:24-27`. In evidence one node (`2437c3dd…`) is registered in 25 Spaces
carrying **23 distinct** "current" generations for the same authority, from 1 to 148;
`AGENTS.md`'s "never scoped to one Execution Space" is not true of the rows. Second, **the
readers that matter most already read it lock-free**: the admission fence
(`fabric_foundation.rs:35-93`), the settlement fence (`:98-158`) and
`current_node_daemon_lease_after_admission_at` (`runtime_command_admission.rs:9-31`) resolve
through `latest_node_daemon_lease`, an unsynchronized `read_jsonl`
(`store_read_models.rs:325-331`; the torn-tail retry that makes it survivable is documented at
`store_jsonl.rs:328-350`). The write lock is not buying reader consistency for them; it is
buying a queue — while every lease row in evidence is **351–365 bytes** (median 353).

The other machine-authority readers are **not** in that shape, and the difference decides how
E2a has to edit them, so it is stated here rather than discovered:

- The three TeamSupervisorLease parent fences (`store_node_runtime.rs:123-127`, `:562-566`,
  `:696-700`) do not call `latest_node_daemon_lease` at all — each **inlines**
  `latest_by_id(self.read_jsonl::<NodeDaemonLease>("node_daemon_leases.jsonl")?, …)` — and each
  runs **while the Space write lock is held** (`require_exact_supervisor_authority_unlocked`
  says so at `:106-108`; the other two sit inside `acquire_write_lock()` at `:527` and
  `acquire_write_lock_cancellable` at `:678`). They are the riskiest edit in the slice, for the
  reason the Risks section gives.
- `from_admitted_command` (`firm-runtime-contract/src/provider_capabilities.rs:453-462`; the
  `firm-core` file of the same name is 135 lines and is not this one)
  reads no store at all: it receives the lease as a parameter and is already source-agnostic.

## Decision

**Machine authority moves out of Execution Space data into one file per machine, under its
own lock.**

### File layout

```text
<FIRM_HOME>/nodes/<node_id>/node-daemon-lease.json      the one machine lease
<FIRM_HOME>/nodes/<node_id>/.node-daemon-lease.lock     its own flock file
<FIRM_HOME>/nodes/<node_id>/node-daemon-lease.json.tmp  write staging (same directory)

(`~/.harness` is only the default FIRM_HOME; the dogfood fixtures run their own Firm homes.)
```

**Scope.** "Machine" here means one Firm home on one host: `(FIRM_HOME, node_id)`. Two Firm homes on the same
physical machine (today: the `~/.harness` node and the Codex dogfood fixture home) each keep their own node directory,
node_id, daemon and lease, exactly as they keep separate sockets and logs today; this ADR does not introduce a
host-wide authority.

The node directory is already the machine rendezvous — `daemon.sock` (`firm-node-daemon/src/supervisor_daemon.rs:66`)
and `node-daemon.log` (`daemon_cli.rs:8-12`) — and the lease has the same scope. The document
keeps today's `NodeDaemonLease` fields plus `schema_version` and the owning pid, so E1a's
liveness proof needs no second source; a `node_id` disagreeing with the directory is a named
refusal, never a repair.

### Lock model and the ordering rule

The lease lock is a **leaf, non-nesting lock**:

1. A thread holding the lease lock acquires **no other lock** and performs **no Space I/O**; it
   touches only the three paths above.
2. A thread holding any Space `.store.lock` **may not acquire** the lease lock; it reads the
   lease file lock-free.
3. Acquire → write → release the lease lock *before* any Space work begins (predecessor session
   settlement, drain bookkeeping).

The lock graph therefore has no edges: the two locks are never held simultaneously in either
order, so deadlock is impossible by construction rather than by discipline. Rule 1 is enforced
by a debug-time lock registry that panics on a Space lock requested under the lease lock.

### Renewal protocol

Acquire the lease lock (timeout `min(TTL/4, 1 s)` — an honest ceiling once the only
contenders are this daemon's own renewal and a rare operator verb) → read → verify exact
`(daemon_id, instance_id, generation)` ownership and `expires > now` → sample `now` → write
`tmp` → `fsync(tmp)` → `rename` → `fsync(dir)` → release. The directory fsync is what makes the
replacement survive a crash, and it is deliberately **best-effort**: by the time it runs the
rename has already succeeded, so the new document is already current for every reader on the
machine. Reporting a failed directory fsync as a write failure would tell the caller its write
did not land when it did, and the caller's only honest response — retry — would republish a
document that is already in place. A failure there narrows the durability claim without
invalidating the write. ~350 bytes, two fsyncs, no unrelated
data in the critical section, and no compaction step because a replace has nothing to compact.
The renewal is **one write per machine**, not one per Space:
`run_held_node_authorities` and its per-Space workers (`machine_authority.rs:621-674`, `:676-711`)
collapse into a single heartbeat, and `ensure_node_authority_bundle` (`:190-315`) becomes one
acquire plus the unchanged per-Space *registration* check — `node_project_registrations.jsonl`
stays Space-local because it says which Spaces this node serves, not who owns the machine.

### Read protocol

Readers `read` and parse the file with no lock. Atomic replace guarantees a reader sees either
the whole previous document or the whole next one — strictly stronger than today's torn-tail
retry — and expiry is still judged against the reader's own clock, as `fabric_foundation.rs:80`
does now. One resolver, `current_machine_lease(node_id) -> (NodeDaemonLease, MachineLeaseSource)`,
replaces `latest_node_daemon_lease` at every fence site listed above. `MachineLeaseSource` is
`NodeFile` or `LegacySpaceRow`, and **only `NodeFile` authorizes a provider effect**. A
`HarnessStore` built without a node-home (imports, fixtures) fails the fence closed with
`MACHINE_LEASE_FILE_UNRESOLVED`; it never skips it.

**Production binds the home at exactly two entry points**, and neither infers it from a path:

- the CLI, once per process (`firm-cli/src/main.rs:516`);
- the NodeDaemon, at both places it builds a Store — `registered_spaces`
  (`machine_authority.rs:148-183`), which hands out the Store every machine-lease writer uses,
  and `ensure_stale_socket_reclaimable` (`:104-146`).

Everything else is test or tool surface. The store-root shape derivation
(`firm_home_of_execution_space_root`) exists only so a fixture in production's layout, or a
store opened by bare path, resolves at all; it is **scheduled for deletion in E2b**, after which
an unbound Store fails every machine-authority read closed. That deletion is safe precisely
because the two production entry points above bind explicitly — a derivation that production
still depended on could not be removed without breaking the daemon.

**The full checklist.** Verified site by site at 35bcda73. Grepping for
`latest_node_daemon_lease` and swapping it finds **three** of these and silently misses the
rest, with no compile error, so each site below names how it reads today and under which lock:

| Site | Reads today | Lock |
|---|---|---|
| admission fence `fabric_foundation.rs:35-93` | `latest_node_daemon_lease` | lock-free |
| settlement fence `fabric_foundation.rs:98-158` | `latest_node_daemon_lease` | lock-free |
| `runtime_command_admission.rs:9-31` | `latest_node_daemon_lease` | lock-free |
| Supervisor parent fence `store_node_runtime.rs:123-127` | inline `read_jsonl` | Space write lock held |
| Supervisor parent fence `store_node_runtime.rs:562-566` | inline `read_jsonl` | Space write lock held |
| Supervisor parent fence `store_node_runtime.rs:696-700` | inline `read_jsonl` | Space write lock held |
| Supervisor parent fence `trust_foundation.rs:296-306` (delivery mutations) | `latest_node_daemon_lease` | Space write lock held |
| RuntimeCommand admission `fabric_runtime_commands.rs:119-131` | `latest_node_daemon_lease` | Space write lock held |
| shutdown settlement `trust_kernel/fabric_work_execution_recovery/node_daemon_shutdown.rs:41-47` | inline `read_jsonl` | Space write lock held |
| recover-predecessor `trust_kernel/fabric_work_execution_recovery/node_daemon_predecessor.rs:262-267` | inline `read_jsonl` | Space write lock held |
| `ensure_stale_socket_reclaimable` `machine_authority.rs:104-146` | `latest_node_daemon_lease` per Space | lock-free |
| reattach released-predecessor proof `fabric_identity_sessions.rs:944-952` | inline `read_jsonl`, **historical** | Space write lock held |
| provider-session transition `runtime_effects.rs:264` (`transition_provider_session_for_member_as`) | `latest_node_daemon_lease` | lock-free |
| provider-session authority `runtime_effects.rs:617` (`require_provider_session_authority_inner`) | `latest_node_daemon_lease` | lock-free |
| message claim `runtime_effects.rs:767` (`claim_canonical_messages_with_before_claim`) | `latest_node_daemon_lease` | lock-free |
| member start `runtime_effects.rs:949` (`start`, immediately before `acquire_team_supervisor_under_node_lease`) | `latest_node_daemon_lease` | lock-free |

There are **four** TeamSupervisorLease parent fences, not three. `ensure_stale_socket_reclaimable`
belongs here rather than in the Issue Pool: it decides a machine-authority *refusal*
(`NODE_DAEMON_LEASE_HELD`) from lease state by enumerating every Execution Space, so if it is
forgotten it keeps refusing on legacy rows after cutover and nothing else fails. The reattach
proof is the one reader that asks about a **named past generation**; it moves to the history
file above, not to the document.

### The inclusion rule, and the rest of the deciders

"Full" has to be checkable, not asserted, because this is the list E2a works from. The rule is
mechanical:

> A production site must switch when it **reads the lease directly** —
> `.latest_node_daemon_lease(`, `.latest_node_daemon_leases(`, or an inline
> `read_jsonl::<NodeDaemonLease>` — **and its enclosing function produces a machine-authority
> refusal**, in any of the three forms this repository spells one:
>
> - a Harness refusal token: `NODE_DAEMON_GENERATION_FENCED`, `NODE_DAEMON_UNAVAILABLE`,
>   `TEAM_SUPERVISOR_PARENT_FENCED`, `NODE_DAEMON_LEASE_HELD`, `NODE_DAEMON_PREDECESSOR_*`,
>   `NODE_DAEMON_SOCKET_RECLAIM_UNSAFE`, `NODE_DAEMON_SHUTDOWN_SETTLEMENT_*`;
> - a typed error: a `SupervisorGenerationFenced` trust error, or a
>   `FabricErrorCode::NodeStaleGeneration`;
> - an HTTP response **code string**: `"SUPERVISOR_GENERATION_FENCED"` in a `409` body.
>
> The last two forms are easy to miss precisely because they do not look like the first: an
> HTTP route refuses by writing a JSON code, and the Fabric path refuses by an enum variant.
> Both still decide whether a provider effect proceeds.

Reading and refusing is the whole test: a site that reads but never refuses cannot admit an
effect on a stale source, and a site that refuses without reading is already downstream of one
that does. Callers of `require_current_node_daemon_unlocked` /
`require_node_daemon_settlement_authority_unlocked` are **not** on the list — they delegate to a
fence that is, and move with it for free.

**Production only.** The rule reads production code: `#[cfg(test)]`, `#[cfg(feature =
"test-support")]` and `tests/` sites are excluded from both buckets, because a fence that a test
helper bypasses is not a fence anyone ships. One helper sits exactly on that line —
`supersede_node_authority_for_test` (`machine_authority.rs:902-903`) reads the lease under
`#[cfg(any(test, feature = "test-support"))]` — so it is counted in neither bucket and named
here instead.

At `fccbf4cc` the rule selects **46 sites in 22 files**, against **31** production reads that
never refuse: 46 + 31 = **77 production reads**, plus that one test helper = the 78 direct reads
a bare grep finds. Table A above carries
the kernel ones, where the lock context decides how the edit is written. The rest are listed
here so the count closes:

| Site | Function |
|---|---|
| `firm-cli/src/runtime_composition/runtime_command_admission.rs:258`, `:452` | `prepare_provider_effect_kind`, `prepare_provider_process_effect` |
| `firm-cli/src/daemon_application.rs:63` | `prepare_team_run` |
| `firm-cli/src/main_modules/daemon_predecessor_recovery.rs:59` | `validate_daemon_predecessor_recovery` |
| `firm-cli/src/main_modules/member_lifecycle.rs:449` | `release_closed_generation_work_bindings` |
| `firm-cli/src/main_modules/member_start_fabric.rs:348` | `supervisor_fabric_authority` |
| `firm-cli/src/main_modules/member_work_coordination.rs:157`, `:1115`, `:1195` | `claim_canonical_work_for_member`, `fail_unreceived_work_claims_for`, `fail_team_messages_for` |
| `firm-cli/src/main_modules/node_team_commands.rs:702`, `:1033` | `team_message_send`, `team_message_claim` |
| `firm-cli/src/main_modules/supervisor_control.rs:537` | `require_bound_live_member_authority` |
| `firm-cli/src/main_modules/team_messaging.rs:499` | `publish_team_message_with_draft` |
| `firm-cli/src/provider_event_persisted.rs:64`, `:395` | `read_persisted_session_for_daemon`, `local_operator_session_read_request` |
| `firm-cli/src/main_modules/http_trust_routes.rs:641` | `handle_trust_routes` — refuses `409 SUPERVISOR_GENERATION_FENCED` |
| `firm-cli/src/remote_fabric.rs:214` | `validate_wave4c_node_authority` — refuses `FabricErrorCode::NodeStaleGeneration` |
| `firm-cli/src/role_actions_api/operator_actions.rs:418`, `:451`, `:489`, `:507` | `execute_operator_action` |
| `firm-cli/src/role_actions_api/canonical_actions.rs:565` | `execute_canonical_role_action` |
| `machine_authority.rs:125`, `:222`, `:287`, `:379`, `:746`, `:981` | the bundle, E1a's automatic predecessor recovery, the heartbeat, acquire, release and shutdown settlement |
| `store_node_runtime.rs:295`, `:383`, `:395`, `:443`, `:483` | the acquire/renew/drain/release quartet — these do not "switch", they **become** the document writer |

**Excluded, and why** — the other 31 production reads never refuse: the Dashboard and HTTP lease
projections (`dashboard_projection.rs:197`/`:200`/`:459`, `http_get_routes.rs:214`),
`daemon status` display (`daemon_cli.rs:144`; **corrected during E2a-2a — `daemon_cli.rs:404` and
`control_protocol.rs:1211` are not display, they are the CLI and daemon halves of `stop` and both
refuse; see "What executing this checklist found"**), RoleView
surfaces (`member_surface.rs:326`, `team_surface.rs:669`, `workspace_surface.rs:714`,
`firm-cli/src/role_views_api.rs:1376`), the host-binding projection
input (`store_host_runtime_binding.rs:86`), the two accessors themselves
(`store_read_models.rs:327`/`:335`), the compactor (`store_jsonl.rs:200`), and the reattach
proof (`fabric_identity_sessions.rs:945`), which is historical and moves to the history file
rather than the document. These keep reading legacy rows after cutover by design.

Two sites need no swap: `from_admitted_command`
(`firm-runtime-contract/src/provider_capabilities.rs:453-462`) receives
`node_daemon: &NodeDaemonLease` as a parameter and reads no store — the swap belongs at its
callers — and `team_supervision.rs` reads no lease at all.

### Time sampling

Today the authority clock is sampled *inside* the write lock precisely so queueing cannot
carry a pre-lock timestamp past expiry (`store_node_runtime.rs:106-108`, the Supervisor helper that states the rule, with the node
lease's own `renewal_now()` re-sampled at `:400` and again after compaction at `:416`). The equivalent guarantee here:
sample `now` **after** the lease lock is held and **immediately before** the rename, and write
`expires = that sample + TTL`. Additionally refuse any write that would move `expires` or
`generation` backwards relative to the document on disk — monotonic in both fields, per node.

**Scoped within a status.** `generation` is monotonic unconditionally. `expires` is monotonic
only while the *status* is unchanged: a renewal must never shrink the lease, which is the
backwards-clock case the rule exists for, but `Draining` deliberately sets a shorter settlement
window and `Released` deliberately expires the lease now. Clamping those would leave a released
generation looking live on disk, which is the opposite of what the rule protects.

### Generation minting

The generation becomes **machine-wide and monotonic**: `generation + 1` of the document under the
lease lock, once per machine, for all Spaces. The Space-local counters disappear — the 1..148
spread above becomes one number.

### Predecessor history

One document is the current authority, and it is deliberately **not** a history. Exactly one
reader needs history: `predecessor_was_released` (`fabric_identity_sessions.rs:944-952`) does an
`rfind` for a **named past generation** to prove that generation reached `Released`, because
expiry is not a provider-drain receipt. Today that proof survives only because
`compact_node_daemon_leases_unlocked` keeps first-of-group plus last-of-each-status *per
generation* for exactly this reason (its own comment at `store_jsonl.rs:185-194` says so), and
two kernel tests pin it: `agent_session_reattach_rejects_expiry_without_provider_drain_receipt`
and `drained_session_resumes_under_the_next_daemon_generation`. A latest-state document alone
would silently delete that proof on the successor's acquire and break session adoption on every
daemon restart.

So the document gains a companion:

```text
<FIRM_HOME>/nodes/<node_id>/node-daemon-lease-history.jsonl
```

**One row per generation transition** — `acquired`, `drained`, `released` — carrying the same
fields as the document row, appended under the **same lease lock**, inside the same
acquire/drain/release write. The leaf-lock rule is unchanged: still no Space I/O and no second
lock under the lease lock, and the file lives in the same directory as the document.

No compaction: this is one row per *generation*, never per renewal. In evidence the busiest node
took 14 generations in 3 days and 79 in 12 days — roughly 28 KB of history, against the 87,354
rows / 28 MB that heartbeat appends produced before #811.

`predecessor_was_released(generation)` reads the history file **lock-free**, exactly like every
other reader here; a single-line atomic append means a concurrent reader can see at most a torn
final line, which the existing torn-tail retry (`store_jsonl.rs:328-350`) already handles. A
generation that appears in **neither** the history file nor the legacy Space rows fails closed
with today's reattach refusal — expiry still never becomes a drain receipt.

### Drain, release, predecessor recovery

`drain` and `release` become single-document status transitions under the lease lock. The
**settlement gate keeps its current strength**: `require_node_daemon_settlement_unlocked`
(`trust_kernel/fabric_work_execution_recovery/node_daemon_predecessor.rs:489-553`) refuses Release while any Supervisor lease of that
generation is unreleased or any Session of it is non-`Detached` or has an open cycle. With one
document that proof is gathered from **every registered Space first**, and only then is
`released` published — so today's continue-past-failure partial release
(`machine_authority.rs:931-975`, `:1128-1162`) becomes one all-or-nothing publish over an explicit
proof set, and `authority_released: false` stops meaning "partly". `recover-predecessor`
(`daemon_cli.rs:200-252`, `daemon_predecessor_recovery.rs:43-163`) keeps its exact-literal
confirm, dead-socket and dead-pid checks and its per-Space session settlement, but the lease half
stops being a cross-Space sweep and its "different unreleased instances across Spaces" refusal
becomes structurally impossible. E1a's automatic takeover (ADR 0073) reads one document instead
of enumerating Spaces; the self-stop journal (`self_stop_events.rs:116-156`) keeps its form and
gains a `lease_source` field.

### `node_daemon_leases.jsonl` and the compactor

All writers retire. Rows stay decodable and readable by projections
(`dashboard_projection.rs:195-236`, `http_get_routes.rs:197`) as pre-cutover legacy.
`compact_node_daemon_leases_unlocked` bounds heartbeat appends *and* preserves the
per-generation history the reattach proof depends on (`store_jsonl.rs:185-251`). The history file
above replaces the second guarantee, so the compactor retires with the per-Space writers in E2b —
not before — and its test is retargeted at the legacy path. `daemon status` gains `lease_path` and `lease_source`; its
`authority_lost` derivation (`control_protocol.rs:1077`) and `lease_renewals` diagnostics
(`lease_renewal_diagnostics.rs:11-31`) are unchanged.

## Migration

1. **Fresh Execution Space first.** No in-place migration of the live dogfood store.
2. Existing stores keep decoding legacy rows; they are `LegacySpaceRow` and never authorize a
   provider effect.
3. **First generation on a node = `max(latest generation across every Space registered to that
   node) + 1`**, computed once at cutover and recorded in the document. On the evidence:
   `2437c3dd…` → 149; `498b0704…` → 15.
4. No dual-write: two live authority sources is the failure this ADR removes.
5. **Rollback**: stop the daemon and run the previous binary against a fresh Space; it ignores
   the node file and mints its own Space-local rows. Safe only because one machine runs one
   NodeDaemon; a mixed-version machine is not a supported state.

## Risks

- **Two locks.** Mitigated by the non-nesting rule and the debug lock registry; the graph has
  no cycle to order.
- **`rename` + `fsync` semantics.** Atomic only within one filesystem — the temp file sits in
  the same directory, and the directory fsync is required for the reason already documented at
  `store_jsonl.rs:242-249`; on macOS/APFS this is the primitive the trust journal already relies
  on. **NFS is out of scope**: `flock` and rename visibility are not dependable there, so the
  daemon fails closed at start if it cannot take the lease lock on that path.
- **Clock skew.** Single machine, one clock — unchanged in kind; the monotonic
  `expires`/`generation` rule bounds a backwards system-clock step.
- **A reader that still trusts a legacy row.** `MachineLeaseSource` alone would not have made
  this a compile-time obligation — a caller can destructure a `(lease, source)` pair and drop the
  source with no diagnostic. `AuthorizedMachineLease` does: its field is private and its only
  constructor is the `NodeFile` arm of `authoritative_machine_lease`, so a fence that wants a
  lease it may act on must name that type, and a `LegacySpaceRow` can never become one. E2a-2's
  deciders take it rather than a bare `NodeDaemonLease`.
- **The TeamSupervisorLease parent fence needs the file.** Three fences inside `firm-store`
  read a Space ledger while holding the Space lock today; they now need a node-home path
  injected into `HarnessStore`, and a store without one must fail the fence closed rather than
  skip it. This is the riskiest edit in the slice.
- **Filesystem aliasing.** One Firm home reached through two spellings (macOS exposes `/var`
  through `/private/var`) would be two documents under two flocks — two machine authorities for
  one machine, the failure this ADR removes, reintroduced at the path layer. The home is
  therefore canonicalized before the node directory is named, exactly as
  `node_daemon_socket_path` already does for `daemon.sock` in that same directory, and a home
  that is not absolute is refused rather than joined to the caller's cwd.
- **Store copies.** A Space copy no longer carries its machine authority; forensics must copy the
  node directory too, or lose the lease timeline that produced this ADR's evidence.

## Test plan

- **Unit** — atomic replace leaves exactly the old or the new complete document under an injected
  failure at each step; `generation`/`expires` monotonicity; foreign `node_id` refused; the lock
  registry panics on a Space lock requested under the lease lock;
  `MACHINE_LEASE_FILE_UNRESOLVED` on a node-home-less store.
- **Two clocks, deliberately.** The lease writer's time sample is injected
  (`node_lease_document.rs`, `LeaseClock`) because *when* `now` is taken under the lock is the
  property this ADR turns on, and `ttl_ms.max(1)` (`store_node_runtime.rs:338`) means no TTL
  value can express it — the arithmetic that made a `Some(0)` TTL a no-op for #990. That
  injection covers **lease-document writes in `firm-store` only**. The daemon's bundle
  revalidation samples its own clock and writes no document, so #992's
  `bundle_revalidation_delay_ms` seam remains the right instrument there and is **not** made
  redundant by the injected clock. Neither replaces the other; do not remove one as a cleanup.
- **Integration (the gen-4 regression)** — hold a Space `.store.lock` for 3× TTL while a writer
  thread keeps rewriting the trust journal; assert **zero** renewal failures and a still-Active
  lease. The same test on today's code fails at TTL; that is the point of the slice.
- **Fault injection** — SIGKILL between `tmp` write and `rename` (old document intact);
  SIGKILL after `rename` before `fsync(dir)` (either version readable, neither torn); lease
  lock held by a dead process (assert the kernel releases the flock, do not assume it).
- **Upgrade** — pre-cutover store + new daemon: legacy rows decode, projections render, admission
  refuses with the named error, `daemon status` reports `lease_source: legacy_space_row`.
- **Stale-socket reclaim** — `ensure_stale_socket_reclaimable` is bound to the Firm home at
  `machine_authority.rs:124` but reads only Space rows today, so nothing observable proves the
  binding until cutover. The cutover test must assert that this path's **first read of the node
  file** goes through that binding: a daemon whose Space roots name no home must still reclaim
  (or refuse to reclaim) from `<FIRM_HOME>/nodes/<node_id>/`, not from a derived directory.
- **Predecessor history** — `agent_session_reattach_rejects_expiry_without_provider_drain_receipt`
  and `drained_session_resumes_under_the_next_daemon_generation` stay green against the history
  file; one row per generation transition and none per renewal; a generation in neither the
  history file nor the legacy rows keeps today's reattach refusal; a torn final line is tolerated.
- **Evidence** — replay the S1 gen-4 window against the new writer in a fixture; assert no
  self-stop.

## What executing this checklist found (E2a-2a)

The inventory above was verified by hand at `fccbf4cc`, and executing it found the limits of that.
Four things are recorded here because a reader who trusts the tables above would otherwise be
misled, and because each was found by running code rather than by reading it.

### The checklist missed thirteen sites, and the miss has a shape

Eleven production sites read the lease and produce a machine-authority refusal — deciders by this
ADR's own inclusion rule — and appear nowhere in the tables above. Two more moved deliberately for
reasons given below. Every one of them is in a file the tables never name at all, which is the
pattern: the hand inventory was organised around the kernel and the `main_modules` surface, and the
CLI's own verbs, the control protocol and the Fabric routing layer were not swept.

| Site | Function and refusal | How it was filed before |
|---|---|---|
| `firm-node-daemon/src/supervisor_daemon/control_protocol.rs:1211` | the `stop` control verb; refuses `SUPERVISOR_GENERATION_FENCED` | "`daemon status` display" |
| `firm-cli/src/main_modules/daemon_cli.rs:404` | the CLI `stop` verb's generation selector; chooses the generation the fence above then checks | "`daemon status` display" |
| `firm-cli/src/daemon_client.rs:500` | `start_daemon_process_fenced`; refuses `SUPERVISOR_GENERATION_FENCED` | absent |
| `firm-cli/src/daemon_client.rs:555` | `daemon start`'s acquisition wait; decides whether the start succeeded | absent |
| `firm-cli/src/daemon_client.rs:677` | `reconcile_team_run_start_postcondition`; the exact-generation postcondition | absent |
| `firm-cli/src/fabric_runtime/cli_routing.rs:44` | NodeGateway session parent fence | absent |
| `firm-cli/src/fabric_runtime/cli_routing.rs:520` | NodeGateway serve requires the current active lease | absent |
| `firm-cli/src/main_modules/http_protocol.rs:216` | peer-Team author parent fence | absent |
| `firm-cli/src/main_modules/team_recovery_work.rs:1334`, `:1396` | refuses `NODE_DAEMON_LEASE_REQUIRED` | absent |
| `firm-cli/src/role_actions_api/runtime_recovery_adapter.rs:28` | supplies the lease the runtime-recovery fence decides on | absent |

Two more moved without the rule requiring it, and are named so the edge is deliberate rather than
accidental:

- `firm-node-daemon/src/supervisor_daemon/recovery.rs:212`
  (`block_start_failure_with_unresolved_runtime_command`) reads the lease and returns a bool, so the
  rule excludes it — the ADR's own Issue Pool said so. It moved anyway, because the question it asks
  is "is the live machine authority *mine*", and after cutover a record this daemon no longer writes
  can only make that comparison wrong; a wrong answer there silently drops a blocking recovery
  marker. An unresolvable or legacy-sourced lease stays the fail-safe `None`.
- `firm-cli/src/main_modules/daemon_cli.rs:144` (`daemon_absent_status`) is a display and stays one.
  It moved because the retarget below requires it to name *the* predecessor lease once, with its
  document path, instead of one line per Execution Space.

**Why leaving them would not have been a partial cutover but a broken one.** `firm daemon start`
would have waited for a generation on a ledger nothing writes and then timed out; `firm daemon stop`
would have selected a legacy generation that the daemon's own stop fence then refused. Neither
failure is subtle, and neither was reachable by the unit suites — the first test to catch any of it
was a lifecycle test whose stop no longer converged.

**So the site-by-site checklist is now discharged mechanically, not by hand.**
`machine_lease_cutover::no_production_decider_reads_the_legacy_lease_ledger_outside_the_named_exclusions`
walks every production source file and fails on any read of `latest_node_daemon_lease`,
`latest_node_daemon_leases` or an inline `read_jsonl::<NodeDaemonLease>` that is not in a named
allowlist carrying its reason — counts included, so a second read added to an already-listed file
also turns it red. A hand list is only as good as the sweep that produced it; this one re-runs on
every build.

### The Rule-2 hazard, and the incident that proved it is real

Seven production deciders resolve the machine lease **while holding an Execution Space
`.store.lock`**: the four TeamSupervisorLease parent fences
(`store_node_runtime.rs:125`, `:567`, `:701` and `trust_foundation.rs:298`), RuntimeCommand
admission (`fabric_runtime_commands.rs:119`), shutdown settlement
(`node_daemon_shutdown.rs:41`) and predecessor recovery (`node_daemon_predecessor.rs:360`). Each is
a place where taking the lease lock would nest the two locks and reintroduce, in the opposite
direction, the queueing this ADR exists to remove.

That is not hypothetical. The first recover-predecessor cut published the document from inside the
Space-locked body, and the debug lock registry panicked by name in 0.2 s rather than deadlocking
under load. Recovery is therefore two-phase: gather the settlement proof and settle the Space's
Sessions under the Space write lock, **`drop(_lock)`**, then take the lease lock and publish
`Released` plus its history row. The `drop` is load-bearing and the comment beside it says so.

The registry is `cfg(debug_assertions)`, which is the right trade and worth stating plainly: it
compiles out of release builds, so "protected structurally" means "caught by debug test runs on the
paths a test exercises". CI runs debug, and every newly wired decider path now has a test that runs
it — including `machine_lease_cutover::the_registry_is_armed_in_this_test_binary`, whose only job is
to make a deliberate violation panic so the other lock tests cannot pass by the registry being off.

### Collapsing a per-Space walk drops the questions it was asking

Collapsing `ensure_node_authority_bundle`'s per-Space loop lost `blocked_by_predecessor`, so
automatic predecessor recovery silently stopped running and acquisition refused instead. E1a's own
test caught it. The lesson is not "be careful": it is that a loop over Spaces was carrying *two*
questions — which Spaces this node serves, and who owns the machine — and only the second one
collapses. The first stays per-Space and always was Space data. `blocked_by_predecessor` is now one
question asked once, of the document, which is the form that cannot disagree with itself.

The same collapse has one consequence worth naming: `ensure_node_authority_bundle`'s cutover mint
reads `latest_node_daemon_lease`, deliberately, and not the resolver. The resolver's whole job is to
refuse a legacy row, and a pre-cutover store is nothing but legacy rows — asking it there would turn
the one input Migration step 3 needs into `MACHINE_LEASE_NOT_AUTHORITATIVE` and leave the first
document unable to start above the generations the Spaces already issued.

### Two properties the mechanism had that nobody had stated

- **An unresolved machine lease is not transient.** The heartbeat classified every non-fence error
  as retryable and kept renewing until the confirmed expiry. `MACHINE_LEASE_FILE_UNRESOLVED` and
  `MACHINE_LEASE_NOT_AUTHORITATIVE` are not transient — waiting cannot turn either into authority —
  so a daemon meeting one would have gone on driving provider effects for a full TTL on authority it
  had already been told it did not have. That is the fence being skipped, just slowly. Both now
  latch on the first occurrence, exactly like a generation fence, keyed off the typed
  `StoreError::is_machine_lease_unresolved()` rather than a message prefix.
- **`expires` monotonicity makes a mid-generation TTL reduction fatal.** A generation acquired under
  a 20 s TTL can never renew down to 1.2 s: every such renewal is refused as a backwards expiry
  until the lease simply runs out. This is correct — a lease that silently shrinks is one whose
  holder believes it owns more time than the file grants — and production never meets it, because a
  generation's TTL is fixed for its life and a new TTL arrives with a fresh acquire. It is written
  down because a fixture that shortens a live TTL looks reasonable and fails in a way that reads as
  a flake.

### Retargeted acceptance tests

Eleven per-Space acceptance tests pinned behaviour this ADR abolishes. None was deleted. Each was
retargeted to a successor property **at least as strong**, and renamed wherever the old name states
the abolished behaviour. The Brain approved nine; two more — rows 10 and 11 — met the same clause of
this ADR ("its 'different unreleased instances across Spaces' refusal becomes structurally
impossible") and are submitted for the same bounded review rather than quietly folded in.

Originating change verified with `git log -S<test name>` rather than inferred. Four of the nine were
attributed to ADR 0073's acceptance in the E2a-2a handoff; that was wrong, and only row 10 is
actually E1a's.

| # | Old name | Old property | New name | New property | Originated in |
|---|---|---|---|---|---|
| 1 | `recovery_uses_space_local_generations_and_http_never_expands_its_tuple` | generations are Space-local; recovery honours each Space's own counter | `the_generation_is_machine_wide_and_monotonic_and_http_never_expands_its_tuple` | **inverted**: one machine-wide monotonic generation, whichever Store asks; the HTTP tuple half unchanged | `e5f1adc3` (captured space-local predecessor generations, DEV-234 lineage) |
| 2 | `contended_space_does_not_delay_healthy_space_to_expiry` | one Space's contention does not starve another Space's lease to expiry | `a_contended_space_does_not_delay_the_machine_heartbeat_at_all` | the renewal lands *while* the Space lock is held, not merely before the TTL | `c96f7f0d` (bound node renewal rounds across Spaces, ADR 0044 lineage) |
| 3 | `default_leases_survive_ten_second_writer_contention_without_extending_ttl` | renewal survived 10 s of data-lock contention | `a_ten_second_space_lock_does_not_slow_the_machine_heartbeat` | same 10 s contention, now measured: the first renewal lands inside the hold, under 2 s | `2b6c20ad` (renew leases within confirmed authority budgets, DEV-149-REVIEW-02 lineage) |
| 4 | `absent_status_names_each_predecessor_lease_expiry` | status names one predecessor lease per Execution Space | `absent_status_names_the_one_predecessor_lease_and_the_document_it_read` | names it once, with `lease_source` and the `lease_path` an operator can open | `83970be3` (#796/#802, DEV-193) |
| 5 | `authority_bundle_rolls_back_partial_acquisition_until_every_predecessor_is_released` | a partial per-Space acquisition is rolled back | `authority_acquisition_is_atomic_on_the_document_until_every_predecessor_is_released` | atomic on the document under injected staging failure: previous generation and history byte-for-byte intact | `651dfee8` (fence NodeDaemon authority handoff, ADR 0044 lineage) |
| 6 | `recover_predecessor_partial_failure_preserves_releases_and_retry_markers` | a partial release preserved per-Space markers; `authority_released: false` meant "partly" | `recover_predecessor_failure_publishes_no_release_and_keeps_its_retry_markers` | all-or-nothing: a failure publishes no release, keeps the previous generation, and names every Space and refusal for the retry | `5f57c6bf` (DEV-234; DEV-149-REVIEW-03 semantics) |
| 7 | `recovery_does_not_add_spaces_after_validation` | a Space cannot join the recovery set after validation | `a_space_appearing_after_validation_joins_neither_the_recovery_set_nor_the_authority` | Spaces no longer enter the authority decision at all; a late Space has no lease of its own to release | `e5f1adc3` |
| 8 | `unreadable_held_space_latches_only_after_confirmed_deadline` | an unreadable Space latches authority loss after a deadline | `an_unreadable_space_is_a_projection_problem_not_an_authority_problem` | **inverted**: the heartbeat renews straight through two unreadable Spaces, which still resolve the same document | `2b6c20ad` |
| 9 | `expired_owned_space_still_latches_global_authority_loss` | an expired per-Space lease escalates to a global loss | `an_expired_machine_lease_latches_authority_loss` | an expired *document* is a machine-wide loss by construction, not by a rule written in prose | `c96f7f0d` |
| 10 | `automatic_recovery_refuses_two_unreleased_predecessor_instances` | two different unreleased instances fence the successor | `the_machine_admits_one_unreleased_predecessor_instance_so_recovery_never_guesses` | the second instance is refused at acquisition, so the ambiguity cannot be written down; recovery of the one real predecessor then proceeds | `cf828dff` (#985, **ADR 0073 / E1a**) |
| 11 | `recovery_refuses_same_generation_foreign_instance_and_fences_successor` (first clause only) | `validate` refused when two Spaces held different unreleased instances | unchanged name — both clauses are still true | the foreign instance is refused earlier, at acquisition, so `validate` is never handed the ambiguity; the successor-fence clause is untouched | `e5f1adc3` |

"At least as strong" is meant literally in every row: rows 1, 8 and 9 invert a claim that was only
ever true because of the defect; rows 2, 3 and 5 keep the original scenario and raise the assertion;
rows 4, 6, 7, 10 and 11 replace a per-Space enumeration with the single fact that made the
enumeration unnecessary.

## Rollout

**Three slices, one cutover.** E2a-0 bound the node home into `HarnessStore` with nothing
reading it. **E2a-1 ships this mechanism inert**: the document, its history, the leaf lock and the
typed resolver exist and are tested, but no fence reads them, so the slice cannot change who owns
a machine. **E2a-2 flips the readers** — all 46 deciders, the daemon's heartbeat and bundle,
recover-predecessor, `daemon status` — which is the one revision where authority actually moves.
**E2b retires the writers**: the per-Space `node_daemon_leases.jsonl` writers, the compactor, and
the store-root shape derivation. Splitting the mechanism from the cutover is a review property,
not a rollout one: a reviewer can hold an inert 1.3k-line mechanism in one head, and then judge
the cutover against a diff that is only call sites.

**No feature flag.** A flag would mean two authority sources alive at once — the exact hazard
being removed — and would double the fence surface at every call site above. Cut over on a fresh
Execution Space, keep legacy rows readable, keep the rollback path above. One kernel-tier slice
with a K-tier review, sequenced after E1 (ADR 0073/0074) and before E3 (#983).

## Consequences

- A slow or large data write can no longer cost the machine its authority. The coupling that
  produced the only journaled loss in the live store is gone, not merely budgeted around.
- `AGENTS.md`'s "machine-scoped, never scoped to one Execution Space" becomes true of the
  storage, not only of the bundle rule: one document, one lock, one generation.
- #952 (segmented append-only journals) stays worth doing and becomes **orthogonal**: it
  shortens data-lock hold time for data's own sake, while E2 removes the heartbeat's dependence
  on it. Neither blocks the other. E3 (#983) is made easier — a machine lease that is already
  one document with one monotonic generation is the object E3 wants to fence against.
- #937's Supervisor lease is a separate lease on the same data lock. E2 does not fix it and the
  fix on `codex/night-recovery-937` is unaffected; E3 retires that lease outright.

## What this ADR does not decide

- The TeamSupervisorLease: whether it is retired, and its expiry-takeover asymmetry; one session
  runtime epoch absorbing `control_state.driver_generation` and `MemberRun.runtime_generation`;
  the three-field `RuntimeBindingFence`; `HostBindingLease`'s demotion to a task-ownership
  marker without TTL — all E3 / #983.
- Whether `NodeDaemonLeaseStatus::Expired` (no writer) is deleted (E1c).
- Trust-journal segmentation or per-aggregate files (#952).
- Automatic takeover for a provably dead predecessor (E1a / ADR 0073) and the cooperative
  interrupt on authority loss (E1b / ADR 0074): E2 changes where their input lives, not what
  they decide.
- Cross-machine fabric — paused by the Owner; this ADR is explicitly single-machine.

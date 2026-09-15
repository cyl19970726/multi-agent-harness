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
baseline: master 35bcda73 for the evidence; line citations re-verified at master 43185050
        (E1c merged), which is where E2a executes this checklist from
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
  (`machine_authority.rs:273-285`); renewal runs every `remaining/4`, capped at 5 s
  (`:290-305`, `:398-430`).
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
(`daemon_predecessor_recovery.rs:75-77`), and "machine-wide" exists only because
`ensure_node_authority_bundle` takes the whole set at once and treats any member's failure
as total loss (`machine_authority.rs:157-251`) — a rule written down only in
`agent-runtime.md:24-27`. In evidence one node (`2437c3dd…`) is registered in 25 Spaces
carrying **23 distinct** "current" generations for the same authority, from 1 to 148;
`AGENTS.md`'s "never scoped to one Execution Space" is not true of the rows. Second, **the
readers that matter most already read it lock-free**: the admission fence
(`fabric_foundation.rs:35-93`), the settlement fence (`:98-158`) and
`current_node_daemon_lease_after_admission_at` (`runtime_command_admission.rs:9-31`) resolve
through `latest_node_daemon_lease`, an unsynchronized `read_jsonl`
(`store_read_models.rs:325-331`; the torn-tail retry that makes it survivable is documented at
`store_jsonl.rs:324-346`). The write lock is not buying reader consistency for them; it is
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

The node directory is already the machine rendezvous — `daemon.sock` (`supervisor_daemon.rs:66`)
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
`tmp` → `fsync(tmp)` → `rename` → `fsync(dir)` → release. ~350 bytes, two fsyncs, no unrelated
data in the critical section, and no compaction step because a replace has nothing to compact.
The renewal is **one write per machine**, not one per Space:
`run_held_node_authorities` and its per-Space workers (`machine_authority.rs:343-440`)
collapse into a single heartbeat, and `ensure_node_authority_bundle` (`:157-251`) becomes one
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
| shutdown settlement `node_daemon_shutdown.rs:41-47` | inline `read_jsonl` | Space write lock held |
| recover-predecessor `node_daemon_predecessor.rs:79-84` | inline `read_jsonl` | Space write lock held |
| `ensure_stale_socket_reclaimable` `machine_authority.rs:84-120` | `latest_node_daemon_lease` per Space | lock-free |
| reattach released-predecessor proof `fabric_identity_sessions.rs:944-952` | inline `read_jsonl`, **historical** | Space write lock held |

There are **four** TeamSupervisorLease parent fences, not three. `ensure_stale_socket_reclaimable`
belongs here rather than in the Issue Pool: it decides a machine-authority *refusal*
(`NODE_DAEMON_LEASE_HELD`) from lease state by enumerating every Execution Space, so if it is
forgotten it keeps refusing on legacy rows after cutover and nothing else fails. The reattach
proof is the one reader that asks about a **named past generation**; it moves to the history
file above, not to the document.

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
final line, which the existing torn-tail retry (`store_jsonl.rs:324-346`) already handles. A
generation that appears in **neither** the history file nor the legacy Space rows fails closed
with today's reattach refusal — expiry still never becomes a drain receipt.

### Drain, release, predecessor recovery

`drain` and `release` become single-document status transitions under the lease lock. The
**settlement gate keeps its current strength**: `require_node_daemon_settlement_unlocked`
(`node_daemon_predecessor.rs:290-330`) refuses Release while any Supervisor lease of that
generation is unreleased or any Session of it is non-`Detached` or has an open cycle. With one
document that proof is gathered from **every registered Space first**, and only then is
`released` published — so today's continue-past-failure partial release
(`machine_authority.rs:646-712`, `:751-765`) becomes one all-or-nothing publish over an explicit
proof set, and `authority_released: false` stops meaning "partly". `recover-predecessor`
(`daemon_cli.rs:197-253`, `daemon_predecessor_recovery.rs:51-127`) keeps its exact-literal
confirm, dead-socket and dead-pid checks and its per-Space session settlement, but the lease half
stops being a cross-Space sweep and its "different unreleased instances across Spaces" refusal
becomes structurally impossible. E1a's automatic takeover (ADR 0073) reads one document instead
of enumerating Spaces; the self-stop journal (`self_stop_events.rs:71-111`) keeps its form and
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
- **A reader that still trusts a legacy row.** The typed `MachineLeaseSource` makes this a
  compile-time obligation, not a review habit.
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
- **Integration (the gen-4 regression)** — hold a Space `.store.lock` for 3× TTL while a writer
  thread keeps rewriting the trust journal; assert **zero** renewal failures and a still-Active
  lease. The same test on today's code fails at TTL; that is the point of the slice.
- **Fault injection** — SIGKILL between `tmp` write and `rename` (old document intact);
  SIGKILL after `rename` before `fsync(dir)` (either version readable, neither torn); lease
  lock held by a dead process (assert the kernel releases the flock, do not assume it).
- **Upgrade** — pre-cutover store + new daemon: legacy rows decode, projections render, admission
  refuses with the named error, `daemon status` reports `lease_source: legacy_space_row`.
- **Predecessor history** — `agent_session_reattach_rejects_expiry_without_provider_drain_receipt`
  and `drained_session_resumes_under_the_next_daemon_generation` stay green against the history
  file; one row per generation transition and none per renewal; a generation in neither the
  history file nor the legacy rows keeps today's reattach refusal; a torn final line is tolerated.
- **Evidence** — replay the S1 gen-4 window against the new writer in a fixture; assert no
  self-stop.

## Rollout

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

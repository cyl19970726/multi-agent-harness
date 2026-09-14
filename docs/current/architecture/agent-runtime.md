# Node Runtime and Message Fabric

```text
status: accepted implementation contract
authority: AF-ADR-011
canonical_for: Agent identity/session separation, NodeDaemon runtime ownership,
  messaging, provider dispatch, runtime control, recovery, and provider parity
```

Provider runtime is machine-local infrastructure. It does not own AgentMember
identity, Team membership, Work responsibility, acceptance, or provider-native
transcripts. One machine-scoped NodeDaemon owns every local AgentSession,
provider process/thread, provider delivery, and runtime-control effect across
the machine's registered Execution Spaces.

Lease renewal is independent of Execution Space discovery. Each held Space
has one lifecycle-owned heartbeat worker, so waiting for one Space's Store
lock does not delay another Space's next renewal. A logical renewal retains
one FIFO position until its confirmed lease deadline or explicit shutdown;
short cancellation checks do not discard and re-enqueue that position. The
Store still checks exact ownership and expiry after acquiring the lock, and
all workers are joined during daemon shutdown. Authority remains
process-local and machine-wide. Before any Team may admit a provider effect,
the NodeDaemon acquires and revalidates the complete set of per-Space leases
for every registered Space owned by that Node. A failure in any member of this
bundle permanently closes admission for that daemon instance and initiates
machine-wide drain. A partial first acquisition rolls back only leases that
this instance acquired before provider admission opened. Lease expiry alone is
never a provider-drain receipt and never permits a successor to steal authority.

## Daemon package and application boundary

`firm-node-daemon` owns the machine daemon as a library, invoked by the existing
`firm-cli` foreground command. Its `DaemonApplicationPort` is a finite boundary: the CLI prepares and drives TeamRuns, constructs
provider handles, resolves Execution Space configuration, reads native Sessions
with the existing authorization checks, and composes recovery records and wake
callbacks. The daemon keeps its contexts, admission gate, control dispatch,
authority renewal and shutdown ordering. This is not another runtime driver or
an additional persisted authority.

`PreparedRun` is a non-Clone, Send owned handle consumed by `drive`; its CLI
implementation owns the original `PreparedTeamRunStart` and registration.
Preparation and thread joins remain outside the contexts mutex. Registration
Drop still stops and joins control, stops and joins heartbeat, releases the
Supervisor lease, then invalidates local authority under its gate. Opaque
NodeSession handles likewise retain their original provider adapter's Drop.

`daemon_error` preserves the finite typed errors and their Display text;
provider-effect classification stays a CLI application helper. Registry errors
cross the port as their existing displayed configuration error, at the same
Usage-mapping call sites; Store/CAS errors remain typed. `daemon_protocol` owns
one definition of the native-read and wake DTOs. `daemon_client` owns socket
requests, single-send start delegation and bounded start observation. No wire
shape, timeout, replay policy, or native-transcript authority changes here.
The neutral coordination helpers, diagnostics and start-failure classifier also
live in `firm-node-daemon`; CLI callers use the same definitions. Its public
surface is foreground `run`, socket path/timeout constants, the finite
application port and its owned handles/results, typed errors, shared protocol
DTOs and these existing neutral helpers. Contexts, registries and locks remain
private. The Message body digest is still computed by the CLI's existing fabric
helper through the named application operation, preserving exact body bytes.
Provider errors are converted by CLI functions retaining the original typed
admission/lease-loss branches; the daemon does not import provider packages.

| Package | Direct implementation dependencies and responsibility |
| --- | --- |
| `firm-node-daemon` | `firm-core`, `firm-store`, `firm-runtime-host`, neutral `firm-provider-events` DTOs; serde/JSON/error derivation |
| `firm-cli` | Composes the daemon library, provider-specific adapters, prepared drive, native readers/auth, message/recovery composition and socket clients/binary spawning |
| `firm-store` | No dependency on the daemon; canonical authority and CAS remain Store-owned |

`check-node-daemon-package-boundaries.mjs` checks actual package dependencies,
source imports and duplicate old owners. The structural gates independently
require machine lease writes and registry lifecycle in daemon modules,
registration/drive composition in CLI, and socket transport in the CLI client.
The default-disabled `test-support` feature retains the existing coupled CLI
fixtures against the same private daemon implementation. Only the CLI dev
dependency enables it; normal builds expose no fixture adapter. The bounded
owned inputs, operations and observations are mapped in the crate
[testing inventory](../../../crates/firm-node-daemon/TEST-MIGRATION.md).
This is a machine-lifecycle boundary, not a second consumer or another binary.

## Canonical separation

```text
AgentMember
  ├─ TeamMembership (participation in one flat Team)
  └─ AgentSession (machine-local provider runtime)

Work revision
  └─ WorkExecutionBinding
       └─ exact TeamMembership + AgentMember + AgentSession generation

Message
  └─ MessageSubscription
       └─ CanonicalMessageDelivery per recipient

NodeDaemon
  └─ RuntimeCommand
       └─ ProviderInvocation / provider effect
```

The objects are intentionally independent:

| Object | Owns | Never owns |
| --- | --- | --- |
| `AgentMember` | the sole durable addressable agent identity and organization status | provider process, Team membership, Work, transcript |
| `AgentSession` | one provider session on one Node and exact NodeDaemon generation, hanging off its AgentMember | Team identity, Work acceptance |
| `TeamMembership` | one AgentMember's active participation in one flat Team | identity, provider lifecycle |
| `WorkExecutionBinding` | one exact Work revision bound to one membership and AgentSession generation | authored conversation |
| `Message` | immutable identity-authored, source-NodeDaemon-attested conversation | Work ownership or runtime control |
| `MessageSubscription` | authorized recipient policy and delivery mode | a second Message or browser-chosen recipient truth |
| `CanonicalMessageDelivery` | per-recipient queue, claim, provider receipt and ACK/cursor state | provider transcript |
| `RuntimeCommand` | crash-recoverable prepare/settle journal for provider/process effects | conversation or Work state |
| `ProviderInvocation` | target-NodeDaemon-built provider input derived from a claimed delivery | public mutation authority |

Current DEV-31 schema work adds bounded runtime-control facts to
`AgentSession.control_state`: runtime residency, runtime activity,
execution-driver class, driver generation/ref, handoff state, native
continuation projection/activation, composition fingerprint, capability
fingerprint, and last reconciliation time. These fields are control fences and
projections only; they do not mirror native turns, tool calls, commands,
files, transcript, or provider reasoning.

The `AgentIdentity` name is retired (ADR 0069). No retired `AgentIdentity`
type, store projection, schema or RoleView field is left anywhere in the tree:
`AgentMember` is the only identity root, and the StartSession
permission-ceiling fence reads the `AgentMember` ceiling directly. Rows
persisted before the cutover may still spell the legacy field
`agent_identity_id`; retained serde aliases decode them and no writer re-emits
that spelling.

## Team Host runtime

[ADR 0057](../../decisions/0057-host-is-an-agent-member.md) closes the former
Host/Member execution split. The Host is the exact `AgentMember` named by
`AgentTeam.host_agent_id` and its one active `TeamMembership(role=host)`.
Every current TeamRun resolves one Host MemberRun.

The application-owned `HostRuntimeBinding` resolver is the only current join
from Host role authority to runtime identity. It requires the exact active Host
membership, TeamRun Host actor, active canonical MemberRun and synchronized
runtime projection. Managed mode additionally requires one attached
AgentSession under the exact live TeamSupervisor and NodeDaemon generations;
external-interactive mode requires zero AgentSessions and exposes only the
exact Host MemberRun inbox. CLI, HTTP, Store delivery, RoleViews, and Dashboard
consume this result instead of independently inferring Host mode. The literal
`host` is never a runtime id, and a control-plane-visible row is never a
provider receipt.

In the default `managed` mode, that MemberRun uses the same AgentSession,
NodeDaemon, RuntimeCommand, provider admission, TeamRuntimeAdapter, Message
claim/receipt/ACK, and Close/Reopen lifecycle as every other managed
participant. Host authority is role policy, not a provider feature and not a
second runtime species. In trusted development, every managed coding Host and
Member starts with an honestly declared `FullAccess` ceiling. The exact
canonical cwd is resolved and frozen on the MemberRun/AgentSession before the
provider starts. A Host and multiple Members may share that cwd, including
concurrently; worktrees are optional task-level isolation, not a runtime
identity or lease authority. Existing lower-permission Sessions are never
widened in place.
Managed Host status delivery additionally carries the
exact recipient MemberRun, AgentSession/runtime generation, and NodeDaemon
generation fence before a provider receipt can settle it.

`external_interactive` is an explicit user-driven exception. It keeps the same
Host AgentMember and business authority but has only a detached MemberRun and
durable pull-based inbox. Harness performs no provider admission or turn and
creates no AgentSession, RuntimeCommand, or native-session record for it; it
cannot claim timely wake, provider receipt, or ACK. Historical `external` values decode to
this mode without fabricating managed evidence, and there is no silent fallback
between modes.

Work events and Messages remain independent canonical planes; runtime and
recovery attentions (`HostAttention`) are the Host-notification delivery
ledger derived from them, the peer of `CanonicalWorkDelivery` and
`CanonicalMessageDelivery`, not a fourth plane (ADR 0064). The daemon may
batch them into the next Host cycle. Delivery never authorizes Work mutation.
The exact Host's versioned `ExecutionRetargeted` is the Work intake decision;
notification delivery or ACK is not a prerequisite. The local CLI acts as a
trusted same-user operator proxy for the configured Host; this is not proof of
an authenticated Host executor. Terminal Work provenance comes from the exact
Work submission operation or atomic Result and execution admission, never a
notification row. No Work ACK field or second intake operation exists (S9).
Provider completion does not mean Host acceptance. Ordinary
progress is batched; decisions, blocked Work,
submission, direct Messages, and recovery facts can wake an idle managed Host.
Host-authored status updates do not recursively wake that same Host.

`TeamRun` remains an internal coordination and history projection.
`MemberRun` carries the member's coordination status and its
**adapter-process epoch** (`runtime_generation`): Reopen, recovery, and
non-clean runtime replacement advance it, and `RuntimeBindingFence` requires
the exact value before any provider effect (ADR 0065).
`AgentSession.runtime_generation` is the **provider-session epoch**: immutable
per session row, embedded in the session id, equal to the MemberRun epoch at
mint for Team-path sessions, and deliberately independent afterwards —
Close/Reopen advances the adapter-process epoch while retaining the same
AgentSession, native transcript, and WorkExecutionBindings. The relation
between the two epochs holds only for the MemberRun that minted the session
row; a session reused by a later MemberRun, a session minted by the
standalone session-start route, and an `external_interactive` Host (no
AgentSession) are outside it. Neither object scopes durable identity or Work
responsibility, and no CLI, HTTP, Dashboard, adapter, or mutable Store seam
may dispatch, resume, interrupt, or stop a provider through them; every
provider effect goes through a `RuntimeCommand`.

## Authority flow

### Same-node messaging

1. The authenticated source AgentSession sends an authoring RuntimeCommand to
   its current NodeDaemon.
2. The source NodeDaemon freezes sender identity/session, immutable content,
   sequence, Team/Work relation, recipients, and content fingerprint.
3. Canonical subscriptions produce one delivery per authorized recipient.
4. The target NodeDaemon claims the delivery for the exact current recipient
   AgentSession generation.
5. Only after the durable claim does it build a `ProviderInvocation` and touch
   the provider.
6. Provider receipt and recipient ACK are separate durable facts on the same
   `CanonicalMessageDelivery` row.

The source and target may be the same NodeDaemon. That does not allow a second
Message, sequence, or delivery authority.

### Managed message boundaries

Wake intent and cycle context are separate. A queued `response_required`
Message can wake an eligible idle managed member; informational mail alone
cannot. Once another ordinary cycle is selected (Work, continuation,
acceptance, Host attention, or Messages), its input also includes the queued
Messages successfully claimed for that input boundary, including informational
mail. The same canonical claim and exact Session/NodeDaemon fences apply.
Claims are individually fenced, not an atomic freeze of the whole mailbox.
Later arrivals stay queued; a failed or uncertain claim follows the existing
reconciliation path and cannot be represented as accepted input.

The cycle retains one primary purpose. Its conversation section does not grant
Work ownership, resume blocked Work, or authorize implementation after
submission. A recipient checks the current linked Work before acting on delayed
instructions. A message-only cycle does not create responsibility.

Only Messages actually rendered in the accepted input receive that cycle's
provider receipt. The managed delivery bridge also advances the recipient's
transport ACK at this handoff; neither fact proves the model read, understood,
or acted on the text. Native input and an explicit correlated response are
needed to establish those separate observations. Failed or uncertain handoffs
retain the existing explicit reconciliation boundary; no blind replay.

Ordinary Messages never interrupt a running turn. Explicit Steer remains a
separate capability-checked RuntimeCommand; adapter support alone does not
establish an ordinary-Message injection path. Kimi ACP without reviewed steer
support waits for a real turn boundary. Interrupt ends a turn, not its Work,
and does not by itself pause automatic continuation.

### Cross-node messaging

The source NodeDaemon remains the only Message author. The Control Plane owns
only a route journal for the immutable Message id and source fingerprint. The
target NodeDaemon owns recipient delivery and provider state. Routing may
retry, but it may not rewrite content, allocate a second source sequence, or
fold target delivery into Control Plane truth.

### Work delivery

Work and Message are separate planes. `CanonicalWorkDelivery` carries an exact
Work id/revision to an active `WorkExecutionBinding`. Claiming Work verifies the
current membership, AgentMember, AgentSession generation, Node placement,
NodeDaemon lease, and Work owner under canonical Store authority. Work result,
progress, finding, failure, revise, submit, gate, and acceptance remain Work
operations.

Work responsibility remains bound to the stable TeamMembership/AgentMember.
Automatic scheduling resolves exactly one current active MemberRun for that
responsible AgentMember and, under the same Store writer lock, revalidates the
Work revision, membership, AgentSession/runtime generation, NodeDaemon and
Supervisor before creating `WorkExecutionBinding` and
`CanonicalWorkDelivery`. Missing, ambiguous, stale, or concurrently replaced
runtime authority fails before delivery. `Work.active_member_run_id` is legacy
compatibility evidence only; current assignment and execution never require or
populate it.

One scheduling pass dispatches at most one Work to a member, so it creates at
most one `WorkExecutionBinding`: the exact one it claims in that same pass. A
`CanonicalWorkDelivery` is therefore only ever `queued` for the dispatch the
Supervisor is performing now. Binding a member's other ready Works ahead of
time froze them against runtime facts that the member's next provider round
invalidates; those deliveries could never be claimed, answered
`DELIVERY_NOT_DISPATCHED` to the member, and were released
(`WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM`) and re-minted only at a later
round boundary. A member's remaining ready Works stay unbound until the pass
that dispatches them, and that pass needs no Host message.

Member-authored WorkReport, Finding, and FailureAnalysis operations resolve the
same unique active `WorkExecutionBinding` under the Store writer lock. The
binding must still match the current stable responsibility, active membership,
exact AgentSession generation, and the one current active MemberRun. A
responsibility change after the binding revision invalidates it even if
ownership later returns to the same member; this closes responsibility ABA
without a second epoch.

`Result` has one narrow settlement exception for an honest Close → Reopen, a
direct consequence of the two-epoch model (ADR 0065). If the provider effect
is already `ProviderReceived`, the same stable AgentMember, same MemberRun id,
and same exact provider-session epoch (`AgentSession.runtime_generation`) may
submit the Result from a higher current adapter-process epoch
(`MemberRun.runtime_generation`). The
operation revalidates the unchanged responsibility and provider-received
delivery, then atomically releases the old binding. It does not admit or replay
a provider effect, create a successor delivery, or authorize Progress,
Finding, FailureAnalysis, or another Work mutation from the stale binding.

A Work-linked Message is different: the Work id is immutable conversation
context, not Work mutation authority. Any exact current active Member of the
same TeamRun may send or reply with that Work link, including after Result
submission releases the WorkExecutionBinding and including a peer Member who
does not own the Work. Store settlement still fences the authenticated sender,
current AgentSession, active TeamMembership and active MemberRun, and rejects a
missing Work or a Work outside the exact Team/TeamRun. It never requires or
revives a WorkExecutionBinding and never changes Work status, version,
responsibility, delivery, or acceptance.

A WorkReport remains candidate evidence until the exact Host accepts it. The
legacy unfenced binding writer rejects every call; only exact runtime admission
may create a binding.
`CurrentWorkDeliveryView` uses this same responsibility-history check. Released
bindings and their exact ProviderReceived or Failed deliveries remain readable
as immutable historical evidence; only the unique Active binding may project
current runtime authority. Ordinary lifecycle revisions preserve frozen
evidence, while malformed/conflicting joins or stale current authority fail
closed. Host Request Changes moves Review → Open so the scheduler can create a
fresh monotonic binding/delivery generation for the next attempt.

Every current CLI, HTTP, RoleView, recovery diagnostic, and Dashboard
reader consumes the non-persisted `CurrentWorkDeliveryView`. The application
projection joins canonical Work, `WorkExecutionBinding`,
`CanonicalWorkDelivery`, AgentSession, MemberRun, and TeamRun facts inside one
explicit Execution Space. Broken canonical joins fail closed. The retired
run-addressed dispatch record and its update ledger have been removed; there is
no compatibility reader, export adapter, fallback, or migration path.

| Reader | Current source |
| --- | --- |
| CLI Work show | TeamRun-scoped `CurrentWorkDeliveryView` |
| HTTP Work detail | TeamRun-scoped `CurrentWorkDeliveryView` |
| Dashboard snapshot | one projection per explicit Execution Space |
| RoleView and member status | same Execution-Space projection |
| TeamRun recovery | canonical claim/receipt diagnostics only |

The repository Work boundary gate rejects direct legacy delivery readers and
types from every maintained current product module in this table.

Canonical delivery revisions are folded by an explicit source/lifecycle
contract, not by last-row-wins. The WorkDelivery id, Work/binding revision,
recipient AgentMember and AgentSession generation, target Node, and creation
time are immutable. The legal revision edges are
`Queued -> Claimed`, `Claimed -> ProviderReceived|Failed`,
`Queued -> Failed` when the execution binding is released before any claim
(`WORK_EXECUTION_BINDING_RELEASED_BEFORE_CLAIM`), and
`ProviderReceived -> Failed` when the exact runtime generation that received
the delivery is provably gone — the immutable provider receipt stays on the
row and only the named lost-generation failure codes may claim that edge, as
the recovery section below describes. Exact replay is idempotent, while
identity drift, same-version drift, version regression/gaps, and illegal
transitions fail the entire read closed. There is no legacy WorkDelivery row
to merge into this fold.

Host cancellation remains a Work responsibility-plane decision after
`ProviderReceived`: it closes only the exact current Work revision and
preserves the delivery receipt, active execution binding, and RuntimeCommand
history unchanged. An unsettled `Claimed` delivery remains uncertain and
fences cancellation until it is reconciled. Cancellation never fabricates a
provider effect, delivery failure, binding release, or semantic Work start.

Only an unsettled `Claimed` delivery is uncertain. A member Start against Work
assigned to that member but not yet bound by the Supervisor, or a member Start
or semantic Work report against a `Queued` delivery that was never claimed and
never produced a provider receipt, returns the retryable
`DELIVERY_NOT_DISPATCHED` rather than `DELIVERY_RECOVERY_UNCERTAIN`: the member
simply waits for the next Supervisor pass and repeats the same command. A
member that already holds one active Work is answered `MEMBER_BUSY` before
either delivery check runs.

`HostAttention` uses the same boundary with different storage ownership. The
canonical trust side record owns the immutable causal source fact; the
HostAttention lifecycle ledger may change only claim, transport, receipt,
failure, attempt, status, and update-time fields through reviewed transitions.
Canonical source rows fold before lifecycle rows. A matching legacy-only
HostAttention lifecycle row remains readable, but it is not canonical Work or
delivery authority and cannot synthesize a WorkDelivery. See ADR 0060.

### One journal for Work

**Every Work transition commits one `work` aggregate envelope to
`agentfirm_trust_operations.jsonl`, and nothing else writes a Work revision.**
`work_operations.jsonl` is legacy read-only input: it holds the rows a
pre-cutover binary wrote, it keeps reading forever, and a store created after
the cutover never gains the file at all. Its crash-atomic
`work_delegation_operations.jsonl` composite retired with the Work-ledger
delegation stack: a store that still holds that file keeps it, and no reader
folds it.

The envelope's transition is the `WorkEventKind` in snake_case — `created`,
`assigned`, `claimed`, `started`, `released`, `blocked`, `resumed`,
`submitted`, `changes_requested`, `accepted`, `cancelled`, `updated`,
`dependencies_changed`, `rebound`, `execution_retargeted`,
`execution_recovered` — from one map that the writer and the reader share, so a
revision can never be written under a name the reader cannot label. Its
`expected_version`/`resulting_version` are the Work versions, its
`resulting_projection` is the Work, and its immutable side records carry the
complete `WorkOperation`: the `WorkEvent`, the same projection, and the
condition records, reports and evidence the ledger row carried. That
is what makes the commit crash-atomic in exactly the way a single JSONL append
used to be.

Idempotency is the trust kernel's replay check and nothing else. A command
checks its caller-context authority, resolves its Work, builds its
`MutationContext`, and asks for a replay — in that order, all *before* its own
guards and version fence. A retry of a command that already committed must
return that committed result, not a `VERSION_CONFLICT` against the revision its
own first attempt produced; and the caller-shape refusal a command applies to
its write applies to its replay too, because `Host/<id>` and `AgentMember/<id>`
canonicalise to the same actor and the replay alone could not tell them apart.
The replay is exact: the same key with different request content is
`IDEMPOTENCY_KEY_REUSED`, never a substitution.

**Where the retry's repair went.** The retired ledger idempotency lookup did
one thing besides answering: it re-derived the operation's HostAttention rows,
so a crash between the Work write and its derived wake was repaired by whatever
retried. The HostAttention reconciler now derives from the whole Work journal
instead, so the same gap is closed for every entrance whether or not anything
ever retries, and a replay is a pure read again. A
Result submission writes its `work`/`submitted` envelope in the SAME atomic
rewrite as its `work_report/created` envelope, so the report and the Review
revision it produced can never exist without one another; the paired envelope
derives its idempotency key from the report's by appending `#<transition>`, and
an exact replay re-appends neither. `#` is therefore reserved in a canonical
idempotency key, and EVERY Work entrance refuses a caller key containing it as
a request-shape error — never as a replay. The responsibility migration plans
every write and then commits them all in one atomic multi-envelope write: a
partially applied sweep has no single revision to reconcile from.

**The performer is an identity, not a runtime generation.** A member command
still arrives signed by the exact MemberRun the caller-context gates fence
against (`ProviderRuntimeProjection/<member-run-id>`), and those gates are
unchanged. What is persisted is the durable AgentMember — `TeamActorKind::AgentMember`
with the `agent_member_id`, the caller's `authn_source` preserved — and the
MemberRun is recorded beside it in `WorkEvent.executed_by_member_run_id`. The
canonical actor on the envelope follows the same one mapping:

| caller `TeamActorRef` | canonical `ActorRef` |
| --- | --- |
| `Host/<host-agent-id>` | `AgentMember/<host-agent-id>` |
| `ProviderRuntimeProjection/<member-run-id>` | `AgentMember/<agent-member-id>` |
| `AgentMember/<id>` | `AgentMember/<id>` |
| `Operator/<id>` | `Human/<id>` |
| `Service/<id>` | `Service/<id>` |

The Host row is an identity, not a widening: the Host gate has already proven
that id is the AgentTeam's `host_agent_id` with the one active Host membership.
A persisted `WorkEvent` for a Host write keeps `TeamActorKind::Host`, which is
what the Host-suppression and re-authorization predicates read. Legacy rows keep
their shape forever, and every predicate that reads member provenance accepts
both through `WorkEvent::executing_member_run_id`. Peer review of Host-owned
Work re-authorizes the next execution admission on an explicit marker that only
the peer-reviewer gate writes (plus the legacy shape whose performer *was* the
reviewing runtime) — never on "some AgentMember requested changes".

One Store reader owns the fold. It answers four shapes: the latest Work per id;
one Work's history, strictly in version order; every Work record in the store in
one deterministic total order; and a monotonic **Work journal position**. It
folds immutable additive provenance forward across both journals, so a sparse
row a stale binary appends still inherits the `accountable_team_id` and
`created_by_member_id` its creation established. A Work version persisted in
both journals with different content is refused
(`WORK_JOURNAL_REVISION_CONFLICT`), not tie-broken: with one writer, two
disagreeing accounts of one revision are corruption, and picking either would
make the answer depend on which file a reader looked at first.

Every shape has an Execution-Space-scoped form, and a caller holding a space
uses it. The scope is a type — `ExecutionSpaceId` — because a TeamRun id and an
Execution Space id are both bare strings and have already been confused for one
another. **Writer and reader resolve in the same scope**: the writer's CAS reads
through the space-scoped fold using its command's own Execution Space, so a Host
that read scoped and wrote with that version never meets a `VERSION_CONFLICT`
against a revision written in another space it cannot see. `work_operations.jsonl`
is the store's own file and carries no space of its own, so a scoped read folds
this store's ledger rows plus only that space's trust transitions. No
current-phase, event, record, count or cursor reader may read one journal alone:
the RoleView, `work show`, `work list`, the dashboard projection, the TeamRun
canonical-state fingerprint and the delta cursor all consume this one surface,
so they cannot disagree about a Work's version or phase.

**`RemoteWorkRef.work_event_id` binds to the `work` transition that produced the
revision** — one revision, one transition, one id, the same id `work_history`
reports and the same id a HostAttention and a delivery name it by. Later
revisions never rebind it. The canonical scan behind that rule remains only for
a revision persisted solely as an atomic side projection of another aggregate's
envelope, which has no `work` transition of its own to name.

The journal position still carries one component per journal, because the two
files share no comparable clock. The ledger component is frozen at the
pre-cutover row count and only the trust component advances for new writes. A
position advances past a cursor when EITHER component is strictly greater. The
single-integer transport used by `firm team-run work list --since` packs it as
`trust * 2^32 + ledger`, so an integer cursor issued before this contract — a
bare ledger row count — decodes unchanged to that same ledger position, and any
position carrying a trust row orders after every such legacy value. Comparison
always decodes first: comparing packed integers directly would be trust-major
and would skip a Work whose only new row is a ledger row. A position neither
component of that packing can name is refused
(`WORK_JOURNAL_CURSOR_OVERFLOW`), never clamped: a clamped ledger component
would freeze a Host's `--since` loop with nothing to act on.

The legacy file's raw rows stay readable on their own, through an explicitly
named legacy reader, for migration, export and historical inspection of that
file itself.

### Runtime control

Start, resume, turn, queued input, interrupt, and stop all use the same durable
RuntimeCommand protocol:

Every managed provider adapter (Claude, Codex, DeepSeek, Kimi, Pi) imposes no
hidden wall-clock limit after a cycle is accepted: a long reasoning turn or
silent provider tool remains live while the owned runner process and transport
remain intact, and Interrupt/Close keep polling. The only timeouts an adapter
applies are the three physical quantities of
`CycleTimeouts` (`crates/firm-runtime-contract/src/timeouts.rs`):
`input_acceptance` bounds only the delivery boundary from input written to the
provider's exact acceptance receipt; `transport_liveness` bounds the proof
that the owned process and transport are still alive; `control_settle` bounds
an issued control's settlement. Silence after acceptance is never a failure
and never an adapter-initiated interrupt; an interrupted cycle carries an
attributed `InterruptCause` (Host control, adapter policy, or provider
initiated), and a provider terminal failure never settles `Satisfied` — it
either settles the cycle receipt `Unsatisfied` (Claude, Codex, DeepSeek, Pi)
or stops the cycle at `RuntimeRecoveryRequired` before any receipt exists
(Kimi; cross-adapter unification is tracked in [GitHub issue
#857](https://github.com/cyl19970726/multi-agent-harness/issues/857)). Child
exit, stdout disconnect, runner error, and unsettled durable effects remain
explicit fail-closed recovery conditions.

The pure wake priority, zero-output degradation/backoff, and bounded
pre-effect admission contention retry are owned by
`firm-runtime-supervisor`. The application supplies store observations and
classifies errors; adapters retain transport observation and protocol control.
`ControlRequest.timeouts` carries the caller's budget through all five semantic
control adapters. That control request currently has no production constructor;
ordinary Team cycles keep their configured `CycleTimeouts` and the unchanged
300/30/15-second contract defaults.

A managed Host-driven member may receive one reconsideration cycle when its
own other Work is accepted after its current block, with no other owned Normal
Active/Review Work remaining. Selection uses current membership and durable
Team responsibility, reading native canonical acceptance events as well as
historical Work events. Recorded `unix-ms` and RFC3339 times establish strict
ordering; unprovable order produces no wake. The input never resumes Work or
creates a Message. A stable acceptance event reference in RuntimeCommand is
consumed at preparation, including subsequent NotApplied outcomes. Shared
preparation rechecks eligibility and rejects a second automatic preparation
across rounds or Session generations; explicit Host intervention remains
available. Ordinary Work/Message delivery, Close, driver fencing and degradation
retain priority.

```text
authenticate and resolve authority
  -> bind exact Node + NodeDaemon generation
  -> bind exact AgentSession generation and permission ceiling
  -> validate full command fingerprint and idempotency key
  -> persist Prepared / effect=Unknown
  -> touch process/provider
  -> persist Settled, Rejected/NotApplied, or RecoveryRequired/Unknown
```

DEV-236 / S4 keeps one current phase alongside independent effect certainty
and postcondition satisfaction. New records and settlement requests carry
`phase`, never the retired `status`. Historical journal envelopes stay intact:
consistent legacy status/phase pairs retain their phase; a missing phase folds
Requested/Accepted to Unknown, Quiesced to Observed, Applied to Settled, Failed
to Rejected, and RecoveryRequired to RecoveryRequired. An explicit Unknown or
conflicting pair stays Unknown, without inventing certainty or postconditions.
Unknown effects still block new drive and runtime replacement; Unknown phase
authorizes neither settlement nor public recovery. Dispatched and
ProviderAcknowledged remain readable vocabulary without new producers.
Historical settlement request spelling is recognized only after the normal
exact authority checks and returns the original event/fingerprint on replay.

`ControlIntent` owns the two control mappings shared by all five adapters.
ADR 0067 retired the two continuation intents and ADR 0068 retired the two
injection intents, so what remains is start one cycle and stop the current one:

| Intent | Durable command kind | Semantic capability |
| --- | --- | --- |
| StartCycle | StartCycle | start_cycle |
| Interrupt | InterruptCurrentCycle | interrupt_current_cycle |

The other RuntimeCommandKind values remain separate lifecycle, inspection,
reconciliation or existing legacy-named handlers. There is no catch-all
conversion of all 32 kinds into these two intents. Unsupported capabilities
keep their existing fail-closed admission, and interrupt does not imply
session close. Ordinary mail reaches a member only through the durable queue
at its next cycle: there is no mid-cycle injection path and no provider-native
boundary queue. The real cycle, interrupt and close paths retain their native
adapter operations.

New Store admission rejects `ReopenMember`, `RetireMember`,
`DeleteNativeSession`, `InjectCurrentCycle`, `QueueAtNativeBoundary`,
`CancelPendingInput`, `InspectContinuation`,
`ActivateContinuation`, `InhibitContinuation`, `ResumeContinuation`,
`ReplaceContinuationCondition`, `ClearContinuation`, `StopBackgroundTask`,
`TransferExecutionDriver`, `InspectCommandEffect`, `ReconcileUnknownEffect`
and `AbortIfNotApplied` with `RUNTIME_COMMAND_KIND_FROZEN`. These command names
have no production effect handler; dynamic envelope decoding alone is not
support. Their persisted values and exact historical replay remain readable,
but changing the envelope or using a new key cannot create a fresh admission.
The three continuation-control kinds joined that list under ADR 0067 and the
two injection kinds under ADR 0068, each of which retired the matching intents
and capabilities outright. Existing member lifecycle and Store recovery
operations are unchanged. This restriction does not freeze the two remaining
control intents, adapter release, Drain/Quiesce/Reattach, or the read-only
continuation observation that feeds the activation projection.

DEV-31 and DEV-68 tighten this into an exact binding fence for every
provider/process effect: the prepared command records the target MemberRun id
and adapter generation, AgentSession id and machine-runtime generation,
execution-driver generation/ref, NodeDaemon generation, optional
TeamSupervisor generation, NativeSessionRef, permission envelope, composition
fingerprint, capability fingerprint, preconditions, and postconditions. These
generation domains are distinct types and may advance independently; in
particular, Close→Reopen advances MemberRun without fabricating a new
AgentSession generation. Provider adapters receive only a private-field
`RuntimeBindingFence` constructed from the already Prepared canonical
RuntimeCommand and the exact current leases. They cannot rebuild authority from
a Session snapshot or public struct literal. A command whose binding cannot be
proven is rejected before the provider boundary. Exactly one difference is
tolerated, and only where an already admitted binding is re-validated (for
example the stale-release and the claim of a stored `WorkExecutionBinding`,
or a `RuntimeCommand` at settlement, recovery, or cycle correlation): a
binding frozen before the first provider Open, with an empty
`NativeSessionRef`, still matches the same exact session, runtime and driver
generation once the provider's native id has attached — the attachment is
durable progress, never stale authority (#745, #583). Replacing an existing
native id is always fenced, and a freshly prepared command must carry the
current id. A provider ACK proves only
transport acceptance; terminal, quiesce, release, and semantic postconditions
require adapter observation evidence and are tracked separately from provider
effect certainty.

Exact replay returns the original durable result and never repeats the effect.
The same key with a changed provider, mode, payload, permission, Node, Space,
Session, or generation fails with an idempotency conflict. Authorization and
generation rejection have zero canonical ledger, provider, process, Message,
and delivery side effects.

The public runtime-command route accepts only closed semantic intents. It does
not accept caller-selected capabilities, permission envelopes, provider
profiles, or complete AgentSession payloads. For session control, the server
resolves exact self or the exact machine Operator/NodeDaemon. Team Host
authority is Team-scoped coordination authority and never controls the global
machine Session. StartSession derives the local Node and active project
registration independently of TeamMembership and enforces the frozen
AgentMember permission ceiling under the same Store lock before any session,
command, process, or provider side effect. Team join/leave does not create,
resume, or close a Session. Likewise, Team `close-member` closes only that
MemberRun generation and cancels its current provider turn; it leaves the
machine-owned AgentSession available and never releases or rewrites Work
bindings from this or another Team.

## Effect certainty and recovery

| Observation | Durable result | Retry rule |
| --- | --- | --- |
| failure proven before provider/process boundary | `Failed / NotApplied` | a new command may be issued intentionally |
| provider/process effect observed complete | `Applied / Applied` | exact replay returns completion |
| socket loss, timeout, callback race, or torn state after the boundary | `RecoveryRequired / Unknown` | no automatic repeat; reconcile first |
| stale NodeDaemon or AgentSession generation | typed fenced error | zero side effects |

The application layer keeps four closed results instead of recovering policy
from display text: `DeliveryEligibility`, `AdmissionOutcome`,
`ProviderEffectOutcome`, and `CycleOutcome`. A not-ready delivery consumes no
transport attempt. `RejectedNoEffect` stops and requires corrected authority,
binding, or new Host intent. Only a proven `NotApplied` provider effect may use
the bounded automatic transport-attempt budget. `Accepted` is never resent,
and `Unknown` always becomes explicit recovery. MemberRun, AgentSession,
NodeDaemon, driver, and Supervisor generations remain fences; the transport
attempt is a separate counter and cannot stand in for any generation.

These decisions use typed Trust/Application results. Production retry code may
not inspect `WORK_NOT_READY`, `RUNTIME_COMMAND_RECOVERY_REQUIRED`, provider
stderr, or any other human-readable string. Terminal cycle observation is also
separate from semantic Work submission and Host acceptance.

`effect_certainty` and semantic `postcondition_status` are distinct. For
example, a native interrupt frame may be sent (`Applied`) while terminal
settlement is still `Unknown` until the adapter observes the provider's settled
boundary. RuntimeCommand is the only durable provider-effect journal; DEV-31
does not introduce a second event ledger, PermissionRequest object, or
PermissionDecision object.

Canonical operation rows are recovered atomically. A torn prepared or settled
tail must yield the last complete operation, never an unreadable ledger or a
fabricated completion. A successor NodeDaemon or AgentSession cannot settle an
older generation's command. Every active WorkExecutionBinding that references
a Session must be explicitly released, rebound, or quiesced before StopSession;
rejection changes neither the Session, binding, command journal, nor provider
process.

An expired predecessor instance retains settlement-only authority: exact
RuntimeCommand replay/settlement, provider drain, reconciliation, and final
release. It cannot prepare a new provider effect or reacquire authority. A
graceful handoff requires explicit drain, proof that exact Sessions and
commands are settled, and explicit Released rows in every registered Space.
If the process crashed, the exact Node Operator may use the critical
`daemon-recover-predecessor` action only after the process is absent, provider
process groups are proved terminated, every RuntimeCommand effect is known,
and the exact expired daemon/instance/generation matches. Recovery detaches the
dead generation's Sessions and releases its Supervisor and NodeDaemon leases;
only then may a successor generation be acquired.

For a dead predecessor's `AuthorMessage` only, this same locked recovery path
can establish the local creation outcome from canonical history before release.
It validates the original accepted envelope and immutable authored Message,
including historical sender binding and initial delivery evidence. A matching
authored operation proves `Applied` (creation, not delivery); complete absence
without conflicting or orphaned records proves `NotApplied`. Unterminated tails,
malformed records, sequence gaps, or inconsistent evidence remain blocked.
Canonical event sequences are checked independently of object versions: Work
versions also span WorkOperation history and need not start at one in this
ledger. Work rows still advance their expected version once; other aggregates
retain their complete canonical version chain. All
pending outcomes are checked before any settlement is appended, and any other
Unknown command still blocks recovery. This does not replay a Message, permit
a successor to execute an old effect, or unfreeze generic reconciliation verbs.

`RecoveryRequired / Unknown` is visible in the exact Node Operator RoleView.
Resolution is a critical confirmed action bound to command version, Node,
NodeDaemon and AgentSession generation, authority, and idempotency fingerprint.
The Operator may record evidence that the effect was applied, was not applied,
or remains unknown; resolution never blindly repeats the native effect.

## Provider conformance

Codex, Claude, Kimi, Pi, and DeepSeek Harness expose separate, closed capability tuples:

- requested permission must fit both the AgentMember ceiling and provider
  adapter capability, and the ceiling must be verifiably enforced — the
  adapter names its `security_enforcement_locus` in the provider profile
  (provider-native policy, adapter tool allowlist, adapter auto-approval, OS
  sandbox, network/credential boundary, or honestly `none_verified`);
- safe current-turn injection requires both adapter support and an observed
  safe point;
- unsupported or unprovable permission, queue, interrupt, resume, or stop
  behavior fails closed;
- every interrupt/close path crosses one closed executable provider-adapter
  seam: it freezes the native plan, durably prepares RuntimeCommand, performs
  the actual native control, waits for terminal acknowledgement, then settles;
  exact replay cannot repeat the native effect, and ambiguous dispatch becomes
  `RecoveryRequired`;
- one table-driven faithful-shim harness applies that lifecycle to Codex,
  Claude, Kimi, and Pi, including permission mapping, safe-injection downgrade,
  terminal acknowledgement, replay, and recovery. It is contract evidence,
  not a claim that an unavailable provider passed a live run;
- collaboration Role Actions use one process-local
  `CollaborationCapabilityEnvelope`. It freezes the exact TeamRun, MemberRun,
  AgentSession, NodeDaemon and Supervisor generations, exact-self scope,
  non-secret fingerprint, provider delivery mechanism, and expiry with the
  live Supervisor registration. The bearer secret is deliberately neither
  cloneable nor serializable and remains inside a non-cloneable, Debug-redacted
  launch environment until spawn. The live registry retains only its
  non-secret fingerprint. The bearer never enters Store, provider profile,
  RuntimeCommand, logs, transcript, artifacts, API, or Dashboard state;
- every provider owns one closed agent-tool delivery seam: Codex app-server
  direct tool environment, Claude SDK tool environment, Kimi ACP tool
  environment, Pi RPC tool environment, or DeepSeek Cordis `shellEnv`.
  Provider composition must call that owned seam before spawn. DSH exposes its
  renamed capability only when `execution.agent` is true; arbitrary ambient
  TOKEN/KEY/SECRET/PASSWORD variables are not accepted by the envelope;
- the CLI submits the capability to the exact live Supervisor. Token,
  Supervisor, AgentSession and NodeDaemon generation mismatches fail before a
  Work or Message mutation. Dropping the live registration expires the
  capability; Close/Reopen creates a new registration and secret. Harness
  coordination has no MCP server or fallback;
- the persistent Team member loop is provider-neutral: the monotonic round
  progression lives in `firm-runtime-supervisor` over an application port,
  while `firm-runtime-contract` owns the provider-facing lifecycle language.
  Wake → claim → ExecutionCycle → settle is shared, and each provider package
  compiles the semantic intents
  (open/resume, start cycle, interrupt, narrow Team Close, strong quiesce, and
  release) into provider primitives with an executable per-intent
  capability report. Pi, Codex app-server, Claude Agent SDK, Kimi ACP, and
  DeepSeek Harness all enter through this shared loop;
  `firm-provider-{codex,claude,kimi,pi,deepseek}` own
  native transport, observation, permission mapping, and exact-version
  receipts, while application composition owns Work/Message/RuntimeCommand
  preparation and settlement;
- Team `CloseRuntime` and strong runtime replacement are deliberately separate
  intents. `CloseRuntime` terminates and reaps the Harness-owned provider
  handle, freezes the Member mailbox, and retains the native session id for an
  explicit higher-generation Reopen. Strong `quiesce`/`release` additionally
  require every adapter to prove the continuation cannot start another cycle
  (four adapters from the disarmed activation projection; Codex by pausing an
  observed active native Goal first), current-cycle terminal state, native
  queue settlement, writable-child drain, idle observation, and durable native
  flush. A provider that cannot observe one of
  those postconditions remains degraded and fails closed; a process exit or
  session-close ACK never fills in missing evidence;
- DeepSeek Harness `deepseek_sdk` is a managed host-driven Team mode at exact
  upstream `0.1.1-rc.2` / `b150a551`. Its native Cordis composition uses
  `ctx.agents.create/resume`, `Agent.followup`, `Agent.cancel`, `whenIdle`, and
  provider-owned JSONL sessions. Goal plugins are intentionally absent;
  `quiesce`, `release`, effect inspection/reconciliation, and standalone Node
  sessions remain degraded or unsupported and fail closed;
- Codex has a proven NodeDaemon-owned app-server start/resume/stop path;
  standalone cancel remains disabled until a native turn is bound;
- Claude, Kimi, and Pi remain disabled for standalone AgentSession lifecycle
  until their NodeDaemon-owned handles are executable; their existing Team
  transports do not imply global Session conformance;
- provider binary/version availability is probed explicitly; installation alone
  does not grant a capability, and an unavailable or unprovable provider is
  disabled rather than reported as conformance PASS;
- Team RoleViews project exact-version compatibility and executable capability
  admission as separate facts. An idle Member counts as Ready only when its
  provider tuple is current and the core `open_or_resume`, `start_cycle`, and
  `observe` bindings are all `verified/active`; protocol support or a passing
  deterministic shim without exact live evidence cannot make that row Ready;
- the browser cannot declare provider compatibility, permission, current turn,
  or effect success;
- provider transcripts, tool calls, commands, files, reasoning, and child
  threads remain in native provider storage unless explicitly promoted as a
  result/evidence reference.

Provider adapters consume only canonical claimed `CanonicalMessageDelivery` or
`CanonicalWorkDelivery` plus a NodeDaemon-built `ProviderInvocation`. The
retired dispatch envelope, run-addressed Work dispatch record, and associated
update ledgers have no runtime type, writer, reader, fallback, SSE, RoleView,
Dashboard, CLI, HTTP, recovery, adapter, audit/export, or migration path.

## Product views and clients

## Remote Node Fabric

One logical Fabric Control Plane coordinates machines, while each machine has
exactly one `ExecutionNode` identity and one current NodeDaemonLease. A
NodeGatewayLease is only a child of the exact current NodeDaemonLease
generation and never a second machine authority. The former `CompanyNode`
name is retired (DOC-108); it was always the same row, under the rule
`CompanyNode.id == ExecutionNode.id`.
Nodes initiate outbound TLS 1.3 mutual-authenticated WSS connections to the
Control Plane. They do not expose an inbound collaboration listener and do not
connect directly to sibling Nodes.

`FabricStore` operations, attempts and receipts are the sole cross-Node route
truth. There is no second route record: ADR 0069 deleted the writer-less,
reader-less `MessageRouteJournal` type, so nothing can be written in parallel
or drive replay, delivery or application claims. A `RouteAttempt` proves
transport only. Application effect
is `none | not_applied | applied | unknown` and only a generation-fenced target
result/receipt may assert it.

A routed message carries either its complete canonical immutable Message
envelope or an authenticated content-addressed reference. The target verifies
and persists that exact Message before creating the existing per-recipient
`CanonicalMessageDelivery`.
A routed RuntimeCommand carries the complete canonical command envelope; the
target resolves it through the existing NodeDaemon service and derives the
terminal effect from the canonical RuntimeCommand record. Fabric never becomes
a second Message, Delivery, RuntimeCommand, Work or provider-session Store.

A routed native-session read carries only the exact viewer, TeamRun,
AgentSession, NodeDaemon-generation, source-generation, watermark, and page
request. The target NodeGateway verifies the closed
`collaboration.native_session_read` application capability and asks the same
NodeDaemon persisted-read service used by local AF_UNIX clients. The route
result contains projected provider-owned rows and disposable cursors only; it
never carries a filesystem locator and never persists a transcript in Fabric.

Source authority is closed to `node | control_plane`; canonical bytes are
versioned. Exact replay is fingerprint-bound. Unknown effect, stale gateway or
NodeDaemon generation, wrong Company/Node/Execution Space, and incompatible
schema/protocol/capability all fail closed without business or provider side
effects. See `docs/current/architecture/remote-node-fabric.md` for the complete
contract and `docs/current/operations/remote-fabric-operations.md` for the
operator procedure.

Server-built RoleViews project current canonical state. Browsers refetch after
SSE invalidation; they do not fold raw ledgers or invent lifecycle truth.
Current inboxes use `CanonicalMessageDelivery`; its per-recipient status is
the only recipient-progress record (ADR 0069 deleted `SubscriptionCursor`,
which no reader ever read). Current
runtime state uses AgentSession and RuntimeCommand. Historical TeamRun,
MemberRun, native-session locator, and legacy export rows are labeled history
and cannot enable actions.

CLI, HTTP, Dashboard, and skills must expose only actions
the server can bind to authenticated identity, authority, target, exact version,
idempotency, confirmation, and current Node/Session generations. Retired
message and runtime mutation routes fail closed with typed errors.

## Acceptance boundary

Release requires:

- deterministic start/resume/turn/input/interrupt/stop replay and recovery
  tests, including terminal, socket-loss, callback-race, torn-row, and
  successor-generation cases;
- real Host→Team/Member and Member→Host/Team Message journeys with
  subscriptions, per-recipient delivery, provider receipt, ACK/cursor, and
  sibling Team/Node/Space negatives;
- Codex/Claude/Kimi/Pi/DeepSeek Harness permission and queue/current-turn conformance;
- executable native control conformance for all five adapters, explicit
  unavailable-provider negatives, and real provider-backed dogfood for each
  provider available in the release environment;
- executable zero-match governance for retired runtime/message authorities;
- populated RoleView and live provider/message acceptance;
- full Rust, formatting, clippy, repository governance, docs, and fresh
  clean-archive gates.

The development batch historically named “Wave 5” must consume these
server-built projections and disabled reasons; it may not reconstruct runtime
state in the client. The Wave 6 development-batch dogfood must prove the
multi-provider Message/Work/RuntimeCommand journeys and recovery contracts on
real Company work before widening permissions or topology.

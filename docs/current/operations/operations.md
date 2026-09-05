# Operations

Dynamic Workflow has no current operational commands, services, Dashboard
routes, plugins, or recovery loops. Historical records use only the lossless
legacy archive export/verify/restore-read path. Operational automation must not
restore its writers or treat Host/provider-local orchestration as another
Harness ledger.

## Repository Delivery Gate

Repository development uses the canonical
[Notion Task -> Codex -> Review -> PR flow](workflow-git-pr.md). A GitHub Issue
is optional problem provenance and enters execution only after Brain triage. One Primary
Session owns a development Wave (repository delivery batch, not the retired
runtime `Wave` structure) end to end in a clean isolated worktree. Ordinary work uses
final-SHA self-review rather than a mandatory second reviewer; a narrow Host
Gate is required only when the Development Record says so. A native Agent Team
run is required only when the runtime itself is the claim under test or an
accepted Spec selects that scenario. Spec-level integrated dogfood runs after
that Spec's Tasks merge; it is an exam, not a development mode. Findings enter
the Issue Pool and do not suspend ordinary repository repair. Hot-fix only a
finding that prevents the run, invalidates its evidence, or breaks safety,
integrity, or an authority boundary.

The reproducible final gate starts from a clean committed SHA:

```bash
pnpm gate:clean-archive
```

It requires pnpm 9.15.4 and enforces this order inside a fresh archive:

```text
frozen pnpm install -> cargo fmt -> cargo clippy -> serial cargo test
  -> governance check -> pnpm check
```

CI installs the frozen JavaScript dependency graph before Rust runtime
integration tests because the Claude Agent SDK executable is discovered from
the repository-owned `node_modules` tree.

## Current Gates

```bash
npx pnpm@9.15.4 check
```

Current checks:

- JSON parsing for schemas, docs, and examples;
- provider-runtime package boundaries, the closed five-provider catalog, and
  forbidden provider-to-CLI/Store/application dependency edges;
- runtime-composition boundaries, including the single typed Member operating
  contract consumed by provider prompts, CLI help, and focused tests;
- Work kernel/package direction, the 1,500-line maintained-file limit, and zero
  active Work-containment vocabulary outside an exact historical allowlist;
- schema fixture validation;
- Markdown local link validation;
- document size warning and a blocking 1500-line maintained-source ceiling;
- skill frontmatter and UI metadata validation;
- docs governance registry validation;
- Agent Dashboard TypeScript typecheck and Vite production build.

The focused Work architecture gate is available independently:

```bash
npx pnpm@9.15.4 check:work-kernel-boundaries
```

It checks dependency inversion: the Work application service declares a
core-facing persistence port without importing Store, CLI, or Provider
packages; Store depends on application + core and implements that port. The
existing `firm-cli` composition layer then joins lifecycle `WorkAction`
dispatch with canonical Trust report/acceptance semantics and returns one typed
outcome containing the canonical projection, current Work, event identity,
version, Store sequence, and replay status. CLI and HTTP consume this same seam;
they do not select Store writers or reconstruct mutation metadata. The
application crate's reviewed
`firm-runtime-contract` policy dependency remains allowed. The gate also scans
the tracked repository rather than trusting a hand-maintained active-file
list. Historical evidence
is admitted only by exact path plus reason. The old wire key is admitted only
at the exact non-serializing core decode declaration and its compatibility
test; its neutral in-memory field cannot become a current public contract.

The focused runtime-composition boundary gate is available independently:

```bash
npx pnpm@9.15.4 check:runtime-composition-boundaries
```

It proves that Member message commands and their actor, recipient, Work
binding, response intent, correlation/causation, and wake semantics have one
typed source. Provider turn prompts, CLI help, and command usage consume that
source; rendered command copies elsewhere in `firm-cli` fail the gate.

Rust checks are also active in CI:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The executable Agent Team acceptance gate is:

```bash
npx pnpm@9.15.4 acceptance:legacy-retirement
```

(Formerly `acceptance:mission-wave`; renamed with the DOC-108 legacy cutover.)
It covers the Agent Team create/start,
shared Works/WorkDelivery, Work-linked conversation, Host-facing CLI transport, the
Dashboard read model and operator controls, plus deterministic persistent
Codex app-server, Claude Agent SDK, Kimi ACP, Pi RPC, and DeepSeek Harness Team Member adapters, and the
retired Mission/Wave legacy reads and retired-write errors. It also
gates durable Supervisor generations, authenticated identity-first Message
authoring, atomic per-recipient delivery
claim/provider receipt/per-recipient acknowledgement, cross-process control
routing, reconnect, and
explicit Close. Historical Codex/Claude/Kimi exec modes are never Agent Team
fallbacks. Current direct-delivery compatibility routes and explicit external
Host entry points use distinct typed transports implemented by provider
packages; managed Hosts use the ordinary Team binding, and historical mode
decoding cannot authorize either surface. Dynamic
Workflow remains retired and has no runtime fallback.

Real self-hosting follows the canonical
[Agent Team Dogfood Loop](../product/agent-team-dogfood-loop.md). A failed live
scenario becomes a Host-triaged repair batch or tracked issue, then the original
scenario is rerun before the matrix expands. Finding a bug is evidence, not
closeout.

Do not report a focused `coordination_canary` as coding self-hosting. A no-edit
SHA check can prove a Work/Message/acceptance seam, but it cannot prove coding
execution. A `coding_dogfood` completion additionally runs:

```bash
pnpm verify:agent-team-dogfood -- <evidence.json> \
  --trust-ledger <execution-space>/agentfirm_trust_operations.jsonl \
  --expected-execution-space-id <trusted-execution-space-id>
```

Both options are mandatory for `coding_dogfood`. The expected Execution Space
id must come from the trusted Execution Space selection that resolved the
ledger, not from caller-controlled path components or ledger records.
Verification fails closed unless exactly one canonical WorkReport, independent
Pass review Message, exact Host acceptance, and native-session binding per
evidenced AgentSession agree with the bundle's Work, version, candidate,
AgentTeam, TeamRun, AgentMember, provider, AgentSession, and native-session ids.
Extra unrelated append-only rows are tolerated, but ambiguity, a malformed
complete JSONL frame, a record from the wrong Execution Space, or any mismatch
is rejected. An unterminated final append-crash fragment is ignored;
whitespace-only frames and an uncommitted `.next` sibling are not evidence.

The changed candidate, changed files, checks, and implementer provider-native
tool start/terminal counts are still required. The evidence bundle and trust
ledger record ids, counts, digests, and native-session pointers only. Provider
transcripts remain native-only; never copy conversation content into Harness
coordination state. Fixture coverage lives in
`schemas/agent-team-dogfood/fixtures/canonical-ledger/manifest.json`.

When a live Member appears stuck, inspect MemberRun/Supervisor health, Inbox
delivery, unresolved `provider_interaction_request` Messages, WorkDelivery,
RuntimeCommand status, AgentSession control state, and provider
capability/profile admission first. There is no generic PendingInteraction
object; for a protected project action, inspect the Work review/acceptance
record that carries the Human or policy approval (the retired generic Approval
ledger no longer exists). Then use bounded provider-native session forensics
through its `NativeSessionRef`. Compare tool/process evidence with the Member
narrative; never read an entire large JSONL into the Host context or copy the
transcript into Harness. The output is a diagnosis and next control action,
not a replacement execution history.

Use focused Rust tests while iterating on one slice:

```bash
cargo test -p firm-cli --test team_run_api --test team_run_daemon -- --test-threads=1
cargo test -p firm-cli --test team_run_api \
  persistent_codex_supervisor_survives_handoffs_transport_loss_and_team_completion \
  -- --test-threads=1
```

There is currently no packaged live-provider command. When a claim depends on
a real provider, record the exact durable Team and
Team/Node/Project-fenced TeamRun, MemberRuns, provider-native session ids, Work
ids/versions, WorkDelivery, linked conversation, submissions/Host
acceptance, artifacts, and
Host judgment from the live run. Do not present deterministic provider-shim
tests as live proof.

For DEV-31 runtime-control work, also record the exact RuntimeCommand binding
and settlement evidence: target AgentSession/runtime generation, execution
driver generation/ref, NativeSessionRef, permission envelope, composition and
capability fingerprints, provider effect certainty, semantic postcondition
status, and adapter observations for terminal/quiesce/release claims. A
provider ACK alone is not sufficient evidence for semantic completion.

Provider-cycle conformance additionally requires the durable
`ProviderCycleCorrelation` on the exact `StartCycle` command. For Codex,
Claude, Kimi, DeepSeek and Pi, verify two consecutive inputs, interrupt then
immediate follow-up, receipt-before-terminal ordering, mismatched-terminal
rejection, and no replay after transport loss following receipt. The terminal
observation may advance runtime lifecycle only after its RuntimeCommand,
NativeSessionRef, AgentSession generation and provider attempt all match.

## Harness And Provider Update Windows

Compare the repository source with the local Harness installation:

```bash
pnpm star-harness:install:check
```

After the source commit is accepted, install it with:

```bash
pnpm star-harness:install
```

This builds the Harness binary from the checkout, publishes it under a
per-revision directory (`~/.local/lib/star-harness/<crate-version>+g<sha>[.dirty]/`,
together with the Claude and DeepSeek member runners), atomically updates the
stable binary links — `harness` (primary) and `firm` (alias) both point at the
same versioned binary, and the command examples in these docs use `firm` —
and records the installation under `~/.local/state/star-harness/installations/`.
Existing sessions keep the binary they loaded; start new member sessions after
applying it.

ADR 0063 retired the Star Harness plugin package, so the installer no longer
touches Codex, Claude, or Kimi plugin marketplaces. Machines that installed the
plugin before that cutover remove it once:

```bash
codex plugin remove star-harness@multi-agent-harness && codex plugin marketplace remove multi-agent-harness
```

```bash
claude plugin uninstall star-harness@multi-agent-harness --scope user && claude plugin marketplace remove multi-agent-harness --scope user
```

```bash
rm -rf ~/.kimi-code/plugins/managed/star-harness
```

### Collaboration skill distribution

The canonical Host/Member contract is `skills/collaborate-as-agent-team-member`
(plus `skills/shared-references`). It reaches agents through exactly these
paths:

- **Inside this repository**: `.agents/skills/collaborate-as-agent-team-member`
  and `.agents/skills/shared-references` are symlinks to the canonical
  `skills/` sources, so Codex (`.agents/skills`), Claude Code
  (`.claude/skills` → `.agents/skills`), and Kimi (`--skills-dir
  .agents/skills`) members dogfooding in this checkout read the current file.
- **Other projects / user scope**: `scripts/install-skill.sh` copies a
  snapshot (`--agent both --scope user`); there is no plugin marketplace copy
  any more (ADR 0063). A copy without a `references/` directory beside
  `SKILL.md` predates the two-role contract and must be refreshed or removed,
  otherwise it shadows the current skill.

The Host waiting protocol (`firm team-run wait`) is part of that contract; a
Host that scripts a sleep-and-status loop instead is a defect to file, not a
workaround to keep (#766).

Provider binary maintenance is separate and follows ADR 0031. The operating
window is:

1. discover releases at most once that day;
2. select one Provider and record current version, candidate, install channel
   and exact rollback;
3. leave active MemberRuns/native sessions on the current runtime;
4. install the candidate for new sessions and run
   `firm member providers --fail-on-review`;
5. run the mode-specific deterministic acceptance and one proportional live
   canary;
6. promote the reviewed version only after green evidence, otherwise roll back
   and retain the failed attempt.

Agent-managed maintenance removes the per-version confirmation prompt. It does
not bypass authentication, payment, license, credential or permission policy,
and it never upgrades several Providers in one review window.

### Rolling Reconciliation After Master Merges

A necessary master merge that changes the Harness binary, adapter, protocol,
permission, model-control, Plugin, or Skill contract triggers rolling
Supervisor reconciliation of live dogfood runtimes before new Team work
starts:

1. projection-only merges need no Agent restart;
2. drain or interrupt active turns before replacing an incompatible runtime;
3. install/sync canonical artifacts from the new master first;
4. rebase member worktrees onto the new master or recreate clean
   same-repository worktrees; two runtime generations never write one
   Workspace;
5. resume the same MemberRun/native session under a higher Supervisor
   generation when compatible, otherwise record the reason and start a new
   native session, retaining the old one as history;
6. reconcile queued/claimed mail, permissions, model controls, cwd/Skill
   roots, and each Session's single execution driver; and
7. probe lane by lane: fresh correlated delivery, same-session answer, and
   exact-recipient acknowledgement where the consumer exposes it.

The dogfood execution roster is deliberate: Kimi `kimi_acp` with the reviewed
K3 model alias at `max` thinking effort is primary (verify the MemberRun
requested-vs-effective `provider_controls` receipt); Claude `claude_agent_sdk`
joins only while its installed SDK passes
`firm member providers --fail-on-review`; Codex providers are not dogfood
execution members. Each member also runs under a strict research budget: one
evidence pass over the Work, owned paths, and directly linked records,
then produce deliverables or report a blocked verdict with the exact missing
fact. The Host steers or interrupts a member that explores past that
checkpoint.

For Kimi ACP members, `--member name:role:kimi:<model-alias>` is applied with
ACP `session/set_config_option` before the first prompt. The alias must exist in
the active Kimi Code configuration; a recorded name alone is never proof of the
model actually used. Keep scarce-provider review lanes narrow and inspect the
MemberRun plus provider output before advancing or accepting the Host plan.

The retired `acceptance:mvp*` and `acceptance:autonomous-team` commands belonged
to the superseded Goal/GoalPhase planning stack and are intentionally not part
of the active command surface.

Start the operator surface with an explicit Workspace selection:

```bash
firm serve --addr 127.0.0.1:8787
```

The current Team authoring path is available through Cargo:

```bash
cargo run -p firm-cli -- --help
cargo run -p firm-cli -- init
cargo run -p firm-cli -- node init
cargo run -p firm-cli -- team create --name <team-name> \
  --description <purpose> --host-agent-id <agent-id> --node-id <node-uuid>
cargo run -p firm-cli -- team-run create --agent-team-id <team-id> --objective <objective> \
  --member-owned-path <member-name>:crates
cargo run -p firm-cli -- team-run start --id <team-run-id>
cargo run -p firm-cli -- dashboard snapshot
cargo run -p firm-cli -- serve --addr 127.0.0.1:8787
```

Detached `firm daemon start` appends both stdout and stderr to
`<FIRM_HOME>/nodes/<node_id>/node-daemon.log`. Start prints that stable path
whether the daemon becomes ready or fails; a readiness failure also includes
the last 20 lines from a seek-from-end read bounded to 64 KiB and calls out the
last daemon error line, so the underlying `daemon serve` error is immediately
visible without loading an unbounded log.
At daemon start, a log larger than 8 MiB rotates to `node-daemon.log.1`,
replacing the previous `.1`, before a new current log is opened. `firm daemon
status` includes `log_path` in a live daemon's JSON and includes the same path
in absent status output. If no live daemon exists but any registered Execution
Space retains a latest `NodeDaemonLease` that is not `Released`, status names
the lease state and the recovery command `firm daemon recover-predecessor
--confirm daemon-recover-predecessor` instead of reporting a bare absence. A lease store
that cannot be read is reported by Execution Space without hiding readable
Spaces or changing the absent status exit code.

If the NodeDaemon loses its machine authority and self-stops, it records the
loss on every TeamRun it was serving through the ordinary TeamRun event log.
The service-authored `node_daemon` / `self_stopped` events carry the first
renewal error, shutdown phase, daemon instance and generation, and terminated
provider process groups. Read them with `firm team-run events --id
<team-run-id>` (or the TeamRun dashboard event stream); if Store contention
prevents the bounded event-write retries, the same structured loss record is
written to `node-daemon.log` as `NODE_DAEMON_SELF_STOP_EVENT_WRITE_FAILED`.

The named recovery action is an ordinary CLI command, not a hand-crafted HTTP
call. After the daemon is stopped and the dead predecessor instance's pid is
proven absent, `firm daemon recover-predecessor --confirm
daemon-recover-predecessor [--evidence-ref <text>]` releases the exact
unreleased predecessor `NodeDaemonLease` in every Execution Space belonging to
this Node and prints the recovery projection (`daemon_id`, `instance_id`,
`generation`, `recovered_spaces`, `status=released`). It refuses when there is
no predecessor, when the confirmation literal is missing or wrong, or when the
predecessor process still exists, and a second run reports the already
released lease without changing anything. Recovery inside the lease TTL is
refused up front with the exact expiry (`predecessor lease generation N has
not expired (expires unix-ms:<n>, in <s>s)`), and the absent `daemon status`
output lists each unreleased predecessor lease with its expiry the same way.

```bash
firm daemon recover-predecessor --confirm daemon-recover-predecessor \
  --evidence-ref "operator: pid 11823 and its provider process groups are gone"
```

`team-run start` is an accepted-command boundary, not a fire-and-forget hint.
If the request was sent but the response socket times out or closes, the CLI
uses the reserved daemon status lane once and accepts success only when the
exact NodeDaemon instance/generation and exact active TeamSupervisor
id/generation, Execution Space, Project Binding, and owner process all match.
Otherwise it returns `TEAM_RUN_START_RESULT_UNKNOWN`; do not retry blindly.
Inspect daemon status and the canonical RuntimeCommand inventory, then use an
explicit recovery or new start intent. A Supervisor that exits with an
unresolved current-generation RuntimeCommand records
`team_supervisor_recovery_required` and automatic recovery scanning stops for
that TeamRun until such explicit intent. Losing only the client response after
a command completed is diagnostic response loss and never poisons the daemon
generation.

There is a second, deliberately weaker hold. A Supervisor that returns with its
TeamRun still `running` and no canonical `TeamRun`, `MemberRun`, `Work`,
`Message` or `RuntimeCommand` change, and a start that fails structurally
before any RuntimeCommand exists (missing cwd, missing Team, stale permission
ceiling, unreleased AgentSession), record `team_supervisor_no_progress` bound
by an evidence ref to a fingerprint of the canonical state they observed.
Automatic adoption skips that TeamRun only while the fingerprint still matches.
This is what stops a NodeDaemon re-adopting an unchanged run and burning a new
Supervisor generation on every scan.

- It clears by itself as soon as any of those canonical rows changes — no
  operator action is required, and clock stamps deliberately do not count.
- It also clears on an explicit operator recovery or Host start intent, which
  records `team_supervisor_recovered`.
- `team_supervisor_recovery_required` always outranks it: a hard diagnosis is
  never shadowed by a no-progress observation, and a no-progress marker that
  cannot prove which canonical state it observed fails closed as one.
- Transient rejections — capacity, an already-managed run, a lost CAS race, a
  fenced daemon generation — never record a hold.
- `firm daemon status` lists any process-local holds under
  `recovery_blocked_runs`, each with whether a canonical change or an explicit
  start lifts it.

`firm daemon stop` answers with its drain result, not with its acceptance.
It replies only after accepted control commands, the recovery scanner, managed
Supervisor threads and authority settlement have converged, within a documented
75-second bound (20s control workers + 20s scanner + 30s Supervisor drain + 5s
forced process-group drain). A drain that does not complete returns `ok:false`
with `NODE_DAEMON_DRAIN_INCOMPLETE` and the failing phase; the CLI exits
non-zero and the Operator action returns the `NODE_DAEMON_DRAIN_INCOMPLETE`
code rather than `SUPERVISOR_GENERATION_FENCED`. Never treat a failed stop as a
stopped daemon.

`authority_released` on that receipt is an observation, not a prediction: it is
true only when `release_node_authorities` actually ran and every registered
Execution Space lease came back Released. Every phase — including the recovery
scanner — gates the release, so a scanner failure leaves the lease `Draining`
and the receipt reports `authority_released:false`.

Read `authority_released:false` as **not wholly released**, never as "nothing
was released". Release walks every registered Execution Space and continues
past a per-Space failure, so some Space leases may already be Released while
others are not. The receipt names them in `released_execution_space_ids` and
`release_failed_execution_space_ids`, and the CLI prints both. Read each
`NodeDaemonLease` when you need certainty about a specific Space.

Two known limits:

- `firm daemon status` is unanswered for the remainder of the drain, because
  control ingress closes when the stop is accepted; the bound above is the
  window. `daemon status` also lists only process-local adoption holds under
  `recovery_blocked_runs` — a durable `team_supervisor_no_progress` hold is
  read from the TeamRun's MemberAction journal, not from status, because
  listing it would put whole-Store scans on the reserved control lane.
- The Operator `daemon-stop` HTTP action can occupy its connection for the full
  drain bound. The serving loop sets no socket timeouts and handles each
  connection on its own thread, so this starves nothing, but a dashboard or
  proxy client with a shorter timeout may give up before the receipt arrives. A
  client-side timeout is not a failed stop: re-read the `NodeDaemonLease` to
  learn whether authority was released.

NodeDaemon lease expiry is not takeover authority. If the exact predecessor
process is still alive, let that instance settle commands, drain providers and
release every registered Execution Space. If it crashed, use the Operator
RoleView's critical `recover daemon predecessor` action only with an exact
process/provider-group termination evidence reference. The action is fenced to
the expired daemon id, instance id and generation, fails closed on any unknown
RuntimeCommand effect, and must release the complete per-Space authority bundle
before a new daemon may start. Never delete lease rows or retry Start to bypass
this settlement boundary.

A drain leaves every member that was mid-turn with an `Interrupted`
AgentSession. That is recoverable, not wedged: after the predecessor lease is
Released, run `firm daemon start`, then inspect `firm daemon status`. The new
daemon automatically adopts eligible runs during boot; only run `firm team-run
start --id <team-run-id>` when that status does not list the run. A start for a
run already listed returns `already_managed: true`, prints `already managed by
NodeDaemon <id> (gen <generation>)`, and exits successfully without creating a
second Supervisor generation or restarting its members. Either path reattaches
each Session to the new generation and resumes it
(`Interrupted -> Idle`, then a fresh cycle). Members that were idle at drain
time keep `Idle` and resume unchanged. The killed cycle's `RuntimeCommand`
remains settled against the dead daemon generation and is never replayed.

The `Interrupted -> Idle` hop happens at the adoption seam, immediately after
each Session is reattached to the live NodeDaemon generation and before any
provider effect is prepared. That is the only moment the lane still proves the
killed runtime gone: once a member runner opens its provider handle the lane is
Attached, and the fence below correctly refuses the hop the runner would then
need. A lane that is not resumable at adoption is left `Interrupted` and the
next Supervisor pass observes it again — the refusal is scoped to that attempt,
so the member stays startable rather than being journalled `blocked`.

Resuming the Session does not resume the killed turn, and the member does not
pick up its in-flight Work where it left off. The drain settlement ends that
Work's execution authority in the same write: a `WorkExecutionBinding` whose
delivery was `Claimed` or `ProviderReceived` moves to `Released` under the
canonical transition `invalidated_by_lost_runtime_generation` (cause
`node_daemon_drain`, or `node_daemon_predecessor_recovery` for the Operator
recovery path), and its `CanonicalWorkDelivery` moves to `Failed` with
`WORK_DELIVERY_SUPERSEDED_BY_NODE_DAEMON_DRAIN` (respectively
`..._BY_NODE_DAEMON_PREDECESSOR_RECOVERY`) while keeping its
`provider_receipt_id` as evidence. Neither record claims a turn outcome.

The path that re-drives it is the ordinary one: the Work keeps its assignee,
revision and phase, so the next Supervisor pass after `daemon start` +
`team-run start` binds a new binding generation and queues a new delivery with
a new claim id. No Host verb is needed. Read it back with
`firm team-run work show --work-id <id>`; the superseded delivery and the
fresh one are both visible. A delivery that was still `Queued` at drain time is
untouched and is claimed unchanged by the reattached lane. `team-run work
redeliver` remains the Host's way to supersede a delivery it still owns and is
not the recovery path for a killed one — if it answers `WORK_DELIVERY_LIVE`,
the binding is still Active and the member must be closed first. That is still
the case after a member Close and Reopen, which deliberately never replays a
provider-received Work; the Host re-drives it with `redeliver`.

If a resume is refused with `AgentSession interrupted by a NodeDaemon drain may
resume only from a detached, disarmed lane…`, the lane still claims a live
provider handle or carries an ambiguous `RuntimeCommand`: reconcile that command
through `runtime-commands/{id}/resolve` first. When the member should not come
back at all, `team-run close-member` is the escape hatch and works on an
`Interrupted` Session whose runtime is detached at a terminal turn boundary.

### Recovering a member left `blocked` over a dead lane

`firm team-run recover --id <run>` is the verb for a member blocked by a
runtime fence — the drain refusal above is the case it exists for. It is not a
general unblock: each of the other gates that writes `blocked` clears its own
block (a successful capacity probe clears a capacity block, the provider review
gate clears a compatibility block), and `recover` deliberately leaves those
alone. Use it when `team-run status` shows a member as `blocked` and prints:

```text
blocked with a detached, idle AgentSession lane — return it to startable with:
harness team-run recover --id <run>
```

It applies on two proofs, both required. The member's own AgentSession must
prove no runtime can be driving it — detached residency, idle activity,
disarmed continuation, no open turn, no queued native input and no ambiguous
`RuntimeCommand` — and the block must carry no typed provenance: no
`provider_compatibility_block_cause`, no known-unavailable
`provider_capacity` snapshot, and no zero-output streak that has actually
reached the wake loop's degradation threshold (a shorter streak is ordinary
probation, not a verdict, and never withholds this repair). It then changes
`MemberRun.status` to `idle` and nothing else: coordination status, runtime
generation, native-session binding and the AgentSession itself are untouched, so
the next Supervisor pass resumes the same provider-native session and never
replays the killed cycle. It needs no live Supervisor, which is the point — a
held adoption has none, so `team-run close-member` refuses with
`RUNTIME_COMMAND_RECOVERY_REQUIRED`, `team-run reopen-member` answers
`{"idempotent": true}`, and `team-run start` re-delegates into the same hold.
The run report counts the repairs as `restarted_blocked_members`.

When the lane is *not* provably dead, `team-run status` says so instead and
`recover` deliberately leaves the block standing: reconcile the RuntimeCommand
or close the member.

A block that carries typed provenance is a live diagnosis owned by the gate that
wrote it, and `recover` never clears it. It reports each one instead — counted
as `blocked_by_typed_provenance`, listed in `blocked_members_not_restarted`, and
printed as `blocked, not restarted — <reason>`:

| provenance | who clears it |
| --- | --- |
| `provider_compatibility` | the provider review gate (`firm member providers --fail-on-review`), which clears the typed cause with the status |
| `provider_capacity` | a successful capacity probe, through the preflight's own recovery |
| `zero_output_degradation` | explicit Host reconciliation through `team-run close-member` + `team-run reopen-member`. Messaging a `Blocked` member cannot restart it: `claim_member_provider_start` admits only `Queued`/`Idle`/`Disconnected` and `decide_wake` keeps a streak-at-threshold member asleep, so a message never reaches the provider; the detached-blocked close (`close_detached_blocked_member_for_recovery`) is what resets `zero_output_streak`, and the reopened member returns to `Queued` |

Clearing one of those by hand would strand its evidence — and for a
compatibility block the typed cause is bound to `blocked` by validation, so a
bare status flip produces a row the Store refuses outright.

Teams are created without any Mission (DOC-108); `--mission-id` survives only
as optional legacy provenance. Omit ad-hoc `--member` overrides when starting
from a durable AgentTeam definition. That path preserves each registered
AgentMember's stable identifier as `MemberRun.agent_member_id`.

Select the Execution Space and Project Binding explicitly:

```bash
firm space switch <execution-space-id>
firm project switch <project-binding-id>
```

`--space` / `HARNESS_SPACE` selects current Agent Team coordination and the
historical archive store. `--project` / `HARNESS_PROJECT` independently selects
provider cwd, project instructions, Skills, Git/worktree, and permission
boundaries. `--store` / `HARNESS_ROOT` remains a deprecation-warned
compatibility override. Provider transcripts, tool streams, command output,
and turns remain in the provider's native store and are joined through
`NativeSessionRef`.

The local API serves the current file-store read model:

```text
GET /health
GET /v1/health
GET /v1/snapshot
GET /v1/dashboard/snapshot
GET /v1/events
GET /v1/team-runs/host-inbox
GET /v1/views/agent-workspace/{team-run-or-member-run}?agent_id={exact-team-member}
```

Managed provider members read only their exact-self coordination Inbox through
the Supervisor-bound `firm member inbox [--all] [--json]` command. The
former unauthenticated per-Member HTTP Inbox route is retired. The cooperative
`external_interactive` hook may still use `team-run inbox` only for the
TeamRun and MemberRun already bound in its environment; it cannot select
another recipient.

The local API also exposes safe control-plane actions used by the Agent
Dashboard:

```text
POST /v1/agentfirm/team-runs/{id}/messages/send
POST /v1/agentfirm/team-runs/{id}/messages/reply
POST /v1/agentfirm/team-runs/{id}/messages/request-decision
POST /v1/team-runs
POST /v1/team-runs/{id}/start
POST /v1/team-runs/{id}/members
POST /v1/agentfirm/nodes/{node-id}/message-deliveries/{delivery-id}/reconcile
POST /v1/team-runs/{id}/members/{member-run-id}/steer
POST /v1/team-runs/{id}/members/{member-run-id}/interrupt
POST /v1/team-runs/{id}/members/{member-run-id}/close
POST /v1/team-runs/{id}/members/{member-run-id}/reopen
POST /v1/team-runs/{id}/members/{member-run-id}/deactivate
POST /v1/gateway/tick
POST /v1/agents/{id}/deliver
POST /v1/agents/{id}/retry-delivery
POST /v1/agents/{id}/reconcile-delivery
POST /v1/agents/{id}/close
POST /v1/tasks/{id}/request-review
```

The API is a read surface and an operator control plane for the Agent
Dashboard. It does not replace review gates, provider-native execution truth,
or decisions. Agent Team controls route through the current durable Supervisor
generation; a service that does not own the live provider handle forwards over
the lease's loopback locator and the owner fences the operation again. Safe
actions must call the same application logic and append store records instead
of mutating dashboard-only state.

Bind the API to `127.0.0.1` for normal local use. It sends permissive CORS
headers so a static Dashboard file can read it; do not bind it to a public
interface unless that harness store is intentionally shareable.

`review gate --decision accept` is evidence-hardened by default. It rejects:

- evidence ids that do not exist;
- evidence attached to another task;
- missing source refs for file-backed evidence;
- failed check evidence;
- missing proposal evidence;
- missing `check_passed`, `critic_findings`, or provider/worker output
  evidence;
- Codex provider-session evidence whose referenced provider session did not
  succeed;
- changed paths outside `owned_paths`, unless explicitly waived.

The `--allow-no-check`, `--allow-no-critic`, `--allow-no-provider-output`,
`--allow-no-proposal-evidence`, and `--allow-global-evidence` flags are escape
hatches. They should appear only with a rationale in the recorded decision.

## Planned Gates

These are design commitments, not current blockers until scripts and CI jobs
exist.

The Agent Dashboard gate is already current (not planned): `pnpm check:dashboard`
is defined in `package.json` and chained into the default `pnpm check`, which CI
runs (`.github/workflows/ci.yml`). It is also listed under Current Gates above.

```bash
pnpm check:dashboard
```

This runs:

```text
tsc -p apps/agent-dashboard/tsconfig.json --noEmit
vite build --config apps/agent-dashboard/vite.config.ts
```

Dashboard build output is committed under `apps/agent-dashboard/web/` so the
static snapshot viewer can still be opened directly.

The following remain genuinely planned (no executable script or CI job yet):

```text
CLI --help snapshot
Rust type <-> schema coverage
adapter descriptor validation
Mermaid render/lint
SSE/WebSocket event stream
non-dry-run Codex app-server delivery smoke
Docker image build
GitHub release
```

## Code And Docs Consistency

- CLI commands shown in docs must appear in CLI help snapshots.
- JSON schemas referenced in docs must parse.
- Examples referenced in docs must be checked by CI.
- Any doc above roughly 500 lines should produce a warning and include a reason
  if it stays unsplit.
Managed coding Agent Teams use the CLI-only collaboration surface. Inside an
exact Host or Member Session, use `firm member message send`, `firm member work
start|submit|accept`, `firm member runtime interrupt`, and the Host-authorized
operator surfaces `firm team-run interrupt-member` / `firm team-run close-member` /
`reopen-member`. These commands submit the exact Supervisor-issued
capability; they do not write the Store directly. Managed provider launch
profiles remove Harness mutation MCP servers.

The capability is not a durable credential. Each provider adapter compiles it
through its reviewed agent-tool boundary, and the Supervisor rechecks the
exact TeamRun, MemberRun, AgentSession, live NodeDaemon lease and Supervisor generations
on every Role Action. A successor Supervisor or replacement Session cannot
reuse an older token. Do not export the token, add it to provider profiles, or
forward arbitrary parent secrets alongside it.

Trusted-development Sessions must be newly created with FullAccess and a
canonical cwd. The cwd may be shared by Host and Members; choose a separate
worktree only when the task needs filesystem or Git-history isolation.

# Operations

Dynamic Workflow has no current operational commands, services, Dashboard
routes, plugins, or recovery loops. Historical records use only the lossless
legacy archive export/verify/restore-read path. Operational automation must not
restore its writers or treat Host/provider-local orchestration as another
Harness ledger.

## Repository Delivery Gate

Repository development uses the canonical
[Notion Spec -> Issue -> Codex -> PR flow](workflow-git-pr.md). One Primary
Session owns a development Wave (repository delivery batch, not the retired
runtime `Wave` structure) end to end in a clean isolated worktree. Ordinary work uses
final-SHA self-review rather than a mandatory second reviewer; a narrow Host
Gate is required only when the Development Record says so. Harness Member
dogfood remains suspended for repository repair, while product TeamWork Gate
and acceptance semantics remain unchanged.

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
execution. A `coding_dogfood` completion additionally runs
`pnpm verify:agent-team-dogfood -- <evidence.json>` and binds the changed
candidate, changed files, checks, WorkReport, independent reviewer, exact Host
acceptance, and implementer provider-native tool start/terminal counts. The
evidence bundle records ids and counts only; transcripts remain provider-native.

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

Validate the repository's unified Harness/Plugin source and compare it with the
local installation:

```bash
pnpm star-harness:install:check
```

After the source commit is accepted and published in the repository
marketplace, install it with:

```bash
pnpm star-harness:install
```

This builds a versioned Harness binary, updates the stable binary link,
converges Codex and Claude on the Git marketplace copy, removes the duplicate
Codex personal copy, and records the installation under
`~/.local/state/star-harness/installations/`. Start new Codex and Claude
sessions after applying it. Existing sessions keep the Plugin and Provider
runtime they already loaded.

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
the Supervisor-bound `harness member inbox [--all] [--json]` command. The
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

# Operations

## Current Gates

```bash
npx pnpm@9.15.4 check
```

Current checks:

- JSON parsing for schemas, docs, and examples;
- schema fixture validation;
- Markdown local link validation;
- document size warning;
- skill frontmatter and UI metadata validation;
- docs governance registry validation;
- Agent Dashboard TypeScript typecheck and Vite production build.

Rust checks are also active in CI:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

The executable Mission/Wave + Agent Team acceptance gate is:

```bash
npx pnpm@9.15.4 acceptance:mission-wave
```

It covers native Mission/Wave HTTP and CLI contracts, Agent Team create/start,
Mission closeout, Host-facing MCP transport, assignment correlations, the
Dashboard read model and operator controls, plus deterministic persistent
Codex app-server, Claude Agent SDK, and Kimi ACP Team Member adapters. It also
gates durable Supervisor generations, typed actor mail, atomic delivery
claim/provider receipt/ACK, cross-process control routing, reconnect, and
explicit Close. Bounded Codex/Claude/Kimi exec paths belong to Dynamic
Workflow and are never Agent Team fallbacks.

Real self-hosting follows the canonical
[Agent Team Dogfood Loop](product/agent-team-dogfood-loop.md). A failed live
scenario becomes a Host-triaged Repair Wave or tracked issue, then the original
scenario is rerun before the matrix expands. Finding a bug is evidence, not
Mission closeout.

When a live Member appears stuck, inspect MemberRun/Supervisor health, Inbox
delivery and PendingInteraction first, then use bounded provider-native session
forensics through its `NativeSessionRef`. Compare tool/process evidence with the
Member narrative; never read an entire large JSONL into the Host context or
copy the transcript into Harness. The output is a diagnosis and next control
action, not a replacement execution history.

Use focused Rust tests while iterating on one slice:

```bash
cargo test -p harness-cli --test mcp_stdio --test team_run_start -- --test-threads=1
cargo test -p harness-cli --test team_run_api \
  persistent_codex_supervisor_survives_handoffs_transport_loss_and_team_completion \
  -- --test-threads=1
```

There is currently no packaged live-provider command. When a claim depends on
a real provider, record the exact Mission, selected Host-plan Wave revision,
Mission-scoped TeamRun, MemberRuns, provider-native session ids, assignment
correlations (including `origin_wave_id` when useful), handoffs, artifacts, and
Host judgment from the live run. Do not present deterministic provider-shim
tests as live proof.

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
   `harness member providers --fail-on-review`;
5. run the mode-specific deterministic acceptance and one proportional live
   canary;
6. promote the reviewed version only after green evidence, otherwise roll back
   and retain the failed attempt.

Agent-managed maintenance removes the per-version confirmation prompt. It does
not bypass authentication, payment, license, credential or permission policy,
and it never upgrades several Providers in one review window.

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
harness serve --addr 127.0.0.1:8787
```

The current Mission/Team authoring path is available through Cargo:

```bash
cargo run -p harness-cli -- --help
cargo run -p harness-cli -- init
cargo run -p harness-cli -- mission create --title <title> --objective <objective> --context "<mission-markdown>"
cargo run -p harness-cli -- mission create-team --id <mission-id> --name <team-name> --description <purpose> --lead host
cargo run -p harness-cli -- wave create --mission-id <mission-id> --title <title> --objective <objective> --context "<wave-markdown>"
cargo run -p harness-cli -- team-run create --mission-id <mission-id> \
  --agent-team-id <team-id> --objective <objective> \
  --member-owned-path <member-name>:crates
cargo run -p harness-cli -- team-run start --id <team-run-id>
cargo run -p harness-cli -- wave advance --id <wave-id> --outcome "<host-decision>" --advanced-by host
cargo run -p harness-cli -- wave create --mission-id <mission-id> --title <next-title> --objective <next-objective> --context "<next-wave-markdown>"
cargo run -p harness-cli -- dashboard snapshot
cargo run -p harness-cli -- serve --addr 127.0.0.1:8787
```

Omit ad-hoc `--member` overrides when starting from a reusable AgentTeam
definition. That path preserves each registered AgentMember's stable identifier
as `MemberRun.agent_member_id`; an intentionally matching Company OS
StandingAgent can then project the participation without inferring identity or
authority.

Select the Execution Space and Project Binding explicitly:

```bash
harness space switch <execution-space-id>
harness project switch <project-binding-id>
```

`--space` / `HARNESS_SPACE` selects Mission/Wave, Agent Team, Workflow, and
coordination storage. `--project` / `HARNESS_PROJECT` independently selects
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
GET /v1/team-runs/{id}/members/{member-run-id}/inbox
GET /v1/member-runs/{id}/native-activity
```

The local API also exposes safe control-plane actions used by the Agent
Dashboard:

```text
POST /v1/messages
POST /v1/team-runs
POST /v1/team-runs/{id}/start
POST /v1/team-runs/{id}/members
POST /v1/team-runs/{id}/messages
POST /v1/team-runs/{id}/messages/{message-id}/ack
POST /v1/team-runs/{id}/messages/{message-id}/reconcile-delivery
POST /v1/team-runs/{id}/members/{member-run-id}/steer
POST /v1/team-runs/{id}/members/{member-run-id}/interrupt
POST /v1/team-runs/{id}/members/{member-run-id}/close
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

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

The executable MVP acceptance gate is:

```bash
npx pnpm@9.15.4 acceptance:mvp
```

Use the quick gate while iterating on a single implementation slice:

```bash
npx pnpm@9.15.4 acceptance:mvp:quick
```

Use the live gate only when the claim depends on real Codex provider delivery:

```bash
npx pnpm@9.15.4 acceptance:mvp:live
```

The quick gate creates an isolated `HARNESS_ROOT`, then exercises the staged
self-hosting flow: team creation, goal design, task assignment, worker report,
provider-event fixture ingestion, negative review-gate rejection, accepted
proposal, goal evaluation, hook bridge, Dashboard API, and the Earning Engine
adapter surface. The live gate adds real persistent Codex delivery, including a
single-member smoke and a Worker/Critic multi-member dogfood task, and can spend
provider tokens.

The first CLI is available through Cargo:

```bash
cargo run -p harness-cli -- --help
cargo run -p harness-cli -- init
cargo run -p harness-cli -- agent health --id <agent>
cargo run -p harness-cli -- git status --task <task>
cargo run -p harness-cli -- proposal from-diff --task <task> --agent <agent> --worktree <path> --title <title> --summary <text> --check-cmd "cargo test"
cargo run -p harness-cli -- review gate --task <task> --reviewer <agent> --decision accept --rationale <text> --evidence <id>
cargo run -p harness-cli -- dashboard snapshot
cargo run -p harness-cli -- board
cargo run -p harness-cli -- serve --addr 127.0.0.1:8787
```

Set `HARNESS_ROOT` to point the file store somewhere other than `.harness`.
The local store writes append-only JSONL collections for goals, members, tasks,
messages, events, proposals, evidence, provider sessions, and decisions.

The default `.harness` directory is local runtime state. Keep durable product
contracts in docs, schemas, skills, and code; use evidence refs when a runtime
store item needs to support a decision.

The local API serves the current file-store read model:

```text
GET /health
GET /v1/health
GET /v1/snapshot
GET /v1/dashboard/snapshot
GET /v1/events
```

The API is a read surface for the Agent Dashboard. It does not replace review
gates, provider-session evidence, or decisions.

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

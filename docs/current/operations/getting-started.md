# Getting started

Choose the execution surface first:

- install **Star Harness** for durable Agent Teams, Work, and Messages;
- install **star-workflow** for bounded, one-shot Dynamic Workflow; and
- start one Harness service plus the Workbench for shared state and controls.

Agent Team and Dynamic Workflow deliberately use different provider modes.

## Prerequisites

- **Rust** (stable) — builds the `firm` binary.
- **Node + pnpm** — for the dashboard and the doc checks.
- At least one provider on `PATH`, authenticated:
  - Agent Team: Codex app-server, Claude Agent SDK streaming, or Kimi ACP.
  - Dynamic Workflow: one-shot `codex exec`, `claude -p`, or
    `kimi -p --output-format stream-json`.

## 1. Install an execution capability

The skill ships in [`skills/star-workflow/`](../../../skills/star-workflow). It is
a plain [Agent Skill](https://code.claude.com/docs/en/skills) (`SKILL.md` +
examples), so it installs into either agent's skill directory:

- Claude Code reads `<project>/.claude/skills/<name>/` (or `~/.claude/skills/`).
- Codex reads `<project>/.agents/skills/<name>/` (or `~/.agents/skills/`).

Pick one:

**A. Install script (simplest, no extra tooling).** Into the current project:

```bash
# from a clone of this repo:
scripts/install-skill.sh --agent both          # both Claude Code + Codex, project-level
scripts/install-skill.sh --agent claude --scope user   # user-level library

# or standalone (no clone needed):
curl -fsSL https://raw.githubusercontent.com/cyl19970726/multi-agent-harness/master/scripts/install-skill.sh \
  | bash -s -- --agent both
```

**B. `npx skills` (cross-agent installer).**

```bash
npx skills add cyl19970726/multi-agent-harness --skill star-workflow --agent codex
npx skills add cyl19970726/multi-agent-harness --skill star-workflow --agent claude
```

**C. Unified Mission + Agent Team plugin for Codex and Claude Code.**

```bash
codex plugin marketplace add cyl19970726/multi-agent-harness
codex plugin add star-harness@multi-agent-harness

claude plugin marketplace add cyl19970726/multi-agent-harness --scope user
claude plugin install star-harness@multi-agent-harness --scope user
```

Start a new task/session, then verify that `star-harness` is enabled and that
the `collaborate-as-agent-team-member` skill is visible (the historical
`orchestrate-mission-waves` Host skill was archived by DOC-108 and is no
longer shipped). The bundled MCP entry runs `firm mcp`, so the `firm` binary
must be on `PATH`.

The separate `star-workflow` plugin remains available for Dynamic Workflow:

```text
/plugin marketplace add cyl19970726/multi-agent-harness
/plugin install star-workflow
```

## 2. Initialize an Execution Space and Project Binding

```bash
cargo build -p firm-cli
./target/debug/firm init
./target/debug/firm space list
./target/debug/firm project list
```

An **Execution Space** owns Agent Team, Work, Message, Workflow, and
coordination state (plus the retired Mission/Mission Log rows as read-only
legacy history). A **Project Binding** independently selects provider cwd, project
instructions, Skills, Git/worktree, and permission boundaries. Select them
explicitly with `--space` / `HARNESS_SPACE` and
`--project` / `HARNESS_PROJECT`, or `firm space switch` and
`firm project switch`. Raw `--store` / `HARNESS_ROOT` and repo-local
`.harness` discovery are compatibility paths, not the preferred model.

## 3. Start the Harness service

```bash
# build output: ./target/debug/firm

# start the API + store (the dashboard and the run-script journal read this)
./target/debug/firm serve --addr 127.0.0.1:8787

# in another terminal, start the dashboard UI (Vite) to watch runs live
pnpm install
pnpm dashboard:dev          # then open the printed URL and click "Load live"
```

`serve` hosts the snapshot and control API on `127.0.0.1:8787`; the Workbench
reads it and the live SSE stream. Start CLI/MCP/service commands with the same
Execution Space selection. Project selection may differ per TeamRun when the
Host deliberately targets another Project Binding.

For a live TeamRun, the service that starts it acquires the durable Supervisor
generation and keeps Member provider connections alive across idle periods.
Other Dashboard/MCP/CLI processes route Steer, Interrupt, Close, and queued
mail to that owner. A TeamRun completion does not close a Member.

## 4. Create a durable AgentTeam

Teams are created without any Mission (DOC-108); `agent create` is retired,
so the Host AgentMember is created through the canonical member-trust
mutation:

```bash
./target/debug/firm member-trust mutate --actor-kind human --actor-id operator \
  --idempotency-key create-builder --expected-version 0 --json '<create_agent_member payload>'
./target/debug/firm node init
./target/debug/firm team create \
  --name builders --description "Persistent builders" \
  --host-agent-id builder-codex --node-id <node-uuid> --member builder-codex
Use authenticated AgentFirm Message role actions together with `inbox`,
`host-inbox`, `status`, and `events` for durable coordination. The retired
`team-run send` writer cannot select an authenticated sender and must not be
used. Ordinary Message queues for the next safe provider cycle; Steer
is a distinct real same-turn control where supported. `close-member` releases
the native runtime while retaining the MemberRun/session; `reopen-member`
starts a new adapter generation on the same native session; `deactivate-member`
retires the coordination identity permanently.

## 5. Author + run a Dynamic Workflow

With the skill installed, ask your agent (Codex or Claude Code) to author a
workflow — it will write a Starlark `.star` program and run it. A minimal one by
hand looks like:

```python
# hello.star
workflow("hello", "one serial scan then a parallel two-way audit")

phase("scan")
scope = agent("List the modules to audit for the login flow.", provider = "codex")

phase("audit")
findings = parallel([
    {"prompt": "Audit auth for: " + scope, "provider": "codex"},
    {"prompt": "Audit session handling for: " + scope, "provider": "claude"},
])
```

Run it through the harness:

```bash
./target/debug/firm workflow run-script hello.star
# bounded + safe options:
#   --space <id>            select the Execution Space used by `serve`
#   --store <path>          deprecated raw compatibility override
#   --timeout-ms 300000     per-worker wall-clock ceiling
#   --max-budget-usd 2.00   per-run spend ceiling (short-circuits when reached)
#   --resume <prior_run_id> reuse a crashed run's succeeded leaves (no re-spend)
```

The run journals one `WorkflowRun` + one `WorkflowStep` per leaf. Read it back:

- in the **dashboard** (Workflows surface — shape, per-step status, tokens, cost,
  drill-in), or
- from the selected **Execution Space** store's `workflow_runs.jsonl` and
  `workflow_steps.jsonl`, or the snapshot API
  `curl -s http://127.0.0.1:8787/v1/snapshot`.

To get a text-producing workflow's **full deliverable** back (each leaf's complete
reply, not the capped per-step summary):

```bash
./target/debug/firm workflow get-output <run_id>            # JSON, all leaves
./target/debug/firm workflow get-output <run_id> --step synthesis --text > plan.md
```

`get-output` reads the durable explicit `WorkflowStep` outcome and joins optional
provider-native activity through `NativeSessionRef`. `--text` prints just the
text; `--step <label>` selects one leaf. Each step reports
`source: "workflow_step"`; native detail is an on-demand, non-authoritative join.

## What the skill teaches

`star-workflow` teaches the agent the runtime's host functions
(`workflow()` / `agent()` / `parallel()` / `pipeline()` / `phase()` / `log()` /
`verdict()`), structured output (`schema=` → native `--json-schema` /
`--output-schema`), the safety knobs (per-node `writable=`/`isolation=`, the
budget ceiling), and the quality meta-patterns (verify→repair, adversarial
verify, judge panel, loop-until-dry) — so it writes real closed-loop programs,
not a naive linear fan-out. See [`skills/star-workflow/SKILL.md`](../../../skills/star-workflow/SKILL.md)
and its [`examples/`](../../../skills/star-workflow/examples).

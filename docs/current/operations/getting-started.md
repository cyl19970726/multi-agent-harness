# Getting started

Choose the execution surface first:

- install **Star Harness** for Mission/Wave and persistent Agent Teams;
- install **star-workflow** for bounded, one-shot Dynamic Workflow; and
- start one Harness service plus the Workbench for shared state and controls.

Agent Team and Dynamic Workflow deliberately use different provider modes.

## Prerequisites

- **Rust** (stable) — builds the `harness` binary.
- **Node + pnpm** — for the dashboard and the doc checks.
- At least one provider on `PATH`, authenticated:
  - Agent Team: Codex app-server, Claude Agent SDK streaming, or Kimi ACP.
  - Dynamic Workflow: one-shot `codex exec`, `claude -p`, or
    `kimi -p --output-format stream-json`.

## 1. Install an execution capability

The skill ships in [`skills/star-workflow/`](../skills/star-workflow/). It is
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

**C. Unified Mission/Wave + Agent Team plugin for Codex and Claude Code.**

```bash
codex plugin marketplace add cyl19970726/multi-agent-harness
codex plugin add star-harness@multi-agent-harness

claude plugin marketplace add cyl19970726/multi-agent-harness --scope user
claude plugin install star-harness@multi-agent-harness --scope user
```

Start a new task/session, then verify that `star-harness` is enabled and that
the `orchestrate-mission-waves` and `collaborate-as-agent-team-member` skills
are visible. The bundled MCP entry runs `harness mcp`, so the `harness` binary
must be on `PATH`.

The separate `star-workflow` plugin remains available for Dynamic Workflow:

```text
/plugin marketplace add cyl19970726/multi-agent-harness
/plugin install star-workflow
```

## 2. Initialize an Execution Space and Project Binding

```bash
cargo build -p harness-cli
firm init
firm space list
firm project list
```

An **Execution Space** owns Mission/Wave, Agent Team, Workflow, and coordination
state. A **Project Binding** independently selects provider cwd, project
instructions, Skills, Git/worktree, and permission boundaries. Select them
explicitly with `--space` / `HARNESS_SPACE` and
`--project` / `HARNESS_PROJECT`, or `harness space switch` and
`harness project switch`. Raw `--store` / `HARNESS_ROOT` and repo-local
`.harness` discovery are compatibility paths, not the preferred model.

## 3. Start the Harness service

```bash
# build output: firm

# start the API + store (the dashboard and the run-script journal read this)
firm serve --addr 127.0.0.1:8787

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
mail to that owner. A Wave or TeamRun completing does not close a Member.

## 4. Create a Mission and persistent Agent Team

```bash
firm mission create \
  --title "Dogfood Agent Team" \
  --objective "Prove persistent multi-member collaboration" \
  --context "## Context\nUse native provider sessions, shared Works, and explicit Host acceptance."
firm member register \
  --id builder-codex --name Builder --role builder --provider codex
firm mission create-team \
  --id <mission-id> --name builders --description "Persistent builders" \
  --lead host --member builder-codex

Use `team-run send`, `inbox`, `host-inbox`, `status`, and `events` for durable
coordination. Ordinary Message queues for the next safe provider cycle; Steer
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
firm workflow run-script hello.star
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
firm workflow get-output <run_id>            # JSON, all leaves
firm workflow get-output <run_id> --step synthesis --text > plan.md
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
not a naive linear fan-out. See [`skills/star-workflow/SKILL.md`](../skills/star-workflow/SKILL.md)
and its [`examples/`](../skills/star-workflow/examples/).

# Getting started

Two things to get going: **install the `author-workflow` skill** (so your agent
knows how to write workflows) and **start the harness service** (so the workflows
have somewhere to run). Then ask your agent to author and run one.

## Prerequisites

- **Rust** (stable) — builds the `harness` binary.
- **Node + pnpm** — for the dashboard and the doc checks.
- At least one provider CLI on `PATH`, authenticated:
  - **Codex** (`codex`) and/or **Claude Code** (`claude`). Each workflow leaf
    runs as a one-shot `codex exec` / `claude -p` worker.

## 1. Install the skill

The skill ships in [`skills/author-workflow/`](../skills/author-workflow/). It is
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
npx skills add cyl19970726/multi-agent-harness --skill author-workflow --agent codex
npx skills add cyl19970726/multi-agent-harness --skill author-workflow --agent claude
```

**C. Claude Code plugin (auto-updates).**

```text
/plugin marketplace add cyl19970726/multi-agent-harness
/plugin install author-workflow
```

Verify it landed: the agent should now see `author-workflow` in its skill list.

## 2. Build + start the harness service

```bash
# build the CLI -> ./target/debug/harness
cargo build -p harness-cli

# start the API + store (the dashboard and the run-script journal read this)
./target/debug/harness serve --addr 127.0.0.1:8787

# in another terminal, start the dashboard UI (Vite) to watch runs live
pnpm install
pnpm dashboard:dev          # then open the printed URL and click "Load live"
```

`serve` hosts the snapshot API on `127.0.0.1:8787`; the dashboard reads it (and
the live SSE stream) to show each workflow run's per-step progress, tokens, cost,
and drill-in.

## 3. Author + run a workflow

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
./target/debug/harness workflow run-script hello.star
# bounded + safe options:
#   --timeout-ms 300000     per-worker wall-clock ceiling
#   --max-budget-usd 2.00   per-run spend ceiling (short-circuits when reached)
#   --resume <prior_run_id> reuse a crashed run's succeeded leaves (no re-spend)
```

The run journals one `WorkflowRun` + one `WorkflowStep` per leaf. Read it back:

- in the **dashboard** (Workflows surface — shape, per-step status, tokens, cost,
  drill-in), or
- from the **store**: `.harness/workflow_runs.jsonl` and
  `.harness/workflow_steps.jsonl`, or the snapshot API
  `curl -s http://127.0.0.1:8787/v1/snapshot`.

## What the skill teaches

`author-workflow` teaches the agent the runtime's host functions
(`workflow()` / `agent()` / `parallel()` / `pipeline()` / `phase()` / `log()` /
`verdict()`), structured output (`schema=` → native `--json-schema` /
`--output-schema`), the safety knobs (per-node `writable=`/`isolation=`, the
budget ceiling), and the quality meta-patterns (verify→repair, adversarial
verify, judge panel, loop-until-dry) — so it writes real closed-loop programs,
not a naive linear fan-out. See [`skills/author-workflow/SKILL.md`](../skills/author-workflow/SKILL.md)
and its [`examples/`](../skills/author-workflow/examples/).

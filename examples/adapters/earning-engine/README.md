# Earning Engine Adapter Example

This example shows how a project can expose its tools to the generic
Star Harness without coupling the generic core to project code.

The adapter supplies:

- tool descriptors for project CLI commands;
- evidence policy for project artifacts;
- Dashboard deep-link templates;
- permission policy for live/order/wallet actions;
- skills or prompts that teach Agent Members how to use the tools.

The descriptor set covers the minimum strategy-matrix operating loop:

| Descriptor | Purpose |
| --- | --- |
| `strategy_harness_runs` | Inspect rolling roots before launch or monitoring decisions. |
| `strategy_harness_status` | Build active or completed round status evidence. |
| `strategy_live_status` | Inspect one live round's child process, preflight, and post-live state. |
| `strategy_harness_build` | Build completed-market packets and role reports. |
| `strategy_fabric_status` | Gate shared market-data fabric, freshness, and direct-WS risk. |
| `strategy_calibration_summary` | Summarize same-market replay and live/backtest divergence. |
| `strategy_trial_inspect` | Inspect Trial DAG node params, metrics, status, verdict, and links. |
| `strategy_trial_artifact_inspect` | List artifacts attached to a Trial DAG node. |
| `strategy_artifact_inspect` | Reference local harness artifacts not yet linked to Trial DAG. |
| `strategy_dashboard_link` | Link strategy-specific dashboard pages or screenshots. |
| `strategy_live_launch_gate` | Record explicit permission checks for fund-affecting live work. |

See [adapter.json](adapter.json) for boundaries and
[pilot-workflow.md](pilot-workflow.md) for the first matrix-review scenario.

## Product Goal

The Earning Engine adapter is not accepted because it can run one bounded
evaluation. It is accepted only when it lets the generic harness operate the
original strategy-matrix workflow as a long-lived research and operations
program.

The project-specific goal is:

- understand the BTC five-minute Polymarket strategy family and why each
  strategy exists;
- keep the strategy matrix tied to DAG or manifest nodes, parameters, live
  runs, backtests, dashboard pages, and artifact history;
- compare strategy variants such as base, depth-early, presign, sell-current,
  and exit variants with evidence from the same markets where possible;
- diagnose why strategies do not trade, fill, exit, or reconcile as expected;
- turn repeated strategy friction into infrastructure tasks for market data,
  execution, backtest/live parity, dashboard, CLI, wallet safety, and
  reconciliation;
- preserve the boundary that strategy logic remains in LetMeTry / Earning
  Engine while the generic harness owns task coordination, messages, evidence,
  permissions, and decisions.

## Required Agent Roles

The adapter must expose enough project context and tools for a Leader Agent to
coordinate these roles:

| Role | Responsibility |
| --- | --- |
| Strategy Lead | Owns the long-term matrix goal, final decisions, capital/risk sequencing, and promotion rules. |
| Strategy Research | Generates and refines hypotheses from market behavior, dashboard evidence, and prior runs. |
| Matrix Curator | Keeps strategy DAG or manifest nodes, parameters, family lineage, and run history aligned. |
| Backtest Parity | Compares backtests with same-market live evidence and classifies expected optimism vs real gaps. |
| Live Ops | Starts, monitors, stops, and summarizes bounded live rounds with process and service evidence. |
| Execution | Reviews order submission, presign, FAK/FOK/GTD, fills, no-fills, cancels, merge, and settlement lifecycle. |
| Market-Data Fabric | Owns direct-WS vs worker/fabric migration, subscription health, freshness, fanout, recorder, and replay evidence. |
| Dashboard Review | Ensures strategy pages show entries, exits, order lifecycle, PnL, market context, and comparison views. |
| Infrastructure | Converts repeated manual work or failure modes into CLI, schema, adapter, dashboard, or CI improvements. |
| Critic / Risk | Challenges unsupported causal claims, promotion decisions, missing evidence, and unsafe live actions. |
| Knowledge | Maintains docs, skills, task archives, and outdated-design cleanup. |

These are harness roles. They do not require a different provider for every
role, but the work must be separable into tasks and messages so the same role
can be performed by Codex, Claude Code, Hermes Agent, or another Agent Member.

## MVP Pilot Flow

For the MVP, this adapter is the first real project pilot. It must drive the
strategy matrix, not just a single strategy command:

```text
long-term strategy goal
  -> matrix audit task
  -> role-specific agent messages
  -> DAG/manifest strategy-node evidence
  -> historical backtest/live/dashboard evidence
  -> bounded diagnostic or evaluation command
  -> execution/data/dashboard/parity review
  -> decision: refine strategy / kill strategy / promote bounded live /
               create infrastructure task / update docs or skill
  -> next matrix task
```

Strategy logic, market-specific judgment, wallet handling, and live execution
remain in LetMeTry / Earning Engine. The generic harness only owns task
coordination, evidence references, permission boundaries, and decisions.

The first pilot task should not start with live orders. It should start with a
matrix audit and evidence-pack review. If the evidence shows a live diagnostic
is needed, the Leader creates a separate permission-gated live task.

This adapter slice accepts the tool surface and initial matrix-audit scenario.
It does not accept a strategy result. A later strategy-matrix review task must
use the descriptors to produce a concrete refine / kill / bounded diagnostic /
infrastructure decision.

## Evidence Policy

Adapter evidence must show both strategy quality and operating quality.

| Evidence class | Examples | Supports |
| --- | --- | --- |
| Strategy matrix | strategy DAG or manifest node, parameters, lineage, active roster, trial id | explaining which variant is being evaluated and why |
| Backtest | same-market backtest, replay result, calibration note, performance summary | diagnostic comparison and hypothesis refinement |
| Live | bounded live round, live artifact, process state, completed-market review, reconciliation | execution reality and promotion consideration |
| Execution | submit, ack, fill, no-fill, cancel, presign, FAK/FOK/GTD, merge, settlement lifecycle | diagnosing missed trades and exit behavior |
| Data quality | Polymarket/Binance freshness, depth/trade age, missing inputs, consumer lag, direct-WS vs fabric status | deciding whether evidence is trustworthy |
| Dashboard | strategy page, market chart, entry/exit labels, order lifecycle chart, comparison view, screenshot | visual review and human-auditable context |
| Review | role report, critic challenge, gate blocker, Leader decision | final decision and follow-up task creation |

Diagnostic evidence and promotion evidence are different. No-fills, gate
blocks, aborted orders, stale data, and backtest/live divergence may justify
more diagnosis or an infrastructure task. They do not justify promotion or
larger live size unless the relevant execution, data, and parity questions are
answered with enough samples.

## Permission Boundaries

The adapter may describe live, order, wallet, and secret-touching tools, but it
must not make those actions implicit. A task that can affect funds or live
orders must record:

- who requested the action;
- which strategy, market, wallet, and budget boundary it applies to;
- whether the action is diagnostic or promotion;
- which evidence made the action acceptable;
- which stop, cancel, reconciliation, or rollback path exists.

## Agent Dashboard Expectations

The generic Agent Dashboard should show this pilot as a Kanban-like operating
view, not as a strategy chart replacement. It should show matrix tasks grouped
by status, role ownership, latest message, blockers, evidence count, decision
state, and links into the Earning Engine dashboard for strategy-specific
charts.

Recommended columns:

```text
Backlog -> Assigned -> In Progress -> Blocked -> Review -> Decision -> Archived
```

Cards should make it obvious whether a task is a strategy iteration task or an
infrastructure-upgrade task.

## MVP Acceptance

The adapter pilot is accepted when the harness can perform all of the following
against the Earning Engine project:

1. Read or reference the active strategy matrix from project sources such as
   DAG nodes, manifests, trial history, dashboard pages, and strategy artifacts.
2. Explain the relationship between strategy family members: what problem each
   variant solves, which parameters differ, and which infrastructure assumption
   it depends on.
3. Create tasks for matrix-level work, not just one-off runs: audit the matrix,
   compare variants, diagnose quiet strategies, review no-fill behavior, review
   exits, or propose a new strategy.
4. Assign role-specific work through messages and collect role-specific
   evidence from logs, commands, dashboard links, screenshots, artifacts, and
   review notes.
5. Detect whether a strategy failure is likely strategy logic, execution
   lifecycle, market-data freshness, dashboard visibility, backtest/live
   mismatch, wallet/order safety, or missing tooling.
6. Produce infrastructure-upgrade tasks when repeated evidence shows the tools
   are blocking strategy progress, for example direct upstream WebSocket usage,
   missing order-lifecycle charts, weak no-fill diagnostics, or incomplete
   same-market replay.
7. Preserve diagnostic vs promotion boundaries: diagnostic no-fills, gate
   blocks, and backtest/live divergence are useful evidence, but they do not
   justify size increases or strategy promotion by themselves.
8. Keep live/order/wallet/secret-touching actions behind explicit permission
   gates and record why an action was allowed.
9. Record decisions with evidence references and create follow-up matrix tasks
   when evidence is incomplete, strategy performance is weak, or tooling is
   insufficient.

## Rejection Criteria

The adapter pilot is not accepted if:

- it only proves that one bounded evaluation can run;
- it cannot describe the current strategy family and the purpose of each
  strategy variant;
- it cannot connect strategy nodes to backtest, live, dashboard, and artifact
  history;
- it treats no-trade or no-fill situations as generic failures without
  classifying the likely layer;
- it cannot create infrastructure tasks from repeated strategy friction;
- it hides the difference between backtest optimism and live execution;
- it requires strategy logic to move into the generic harness core.

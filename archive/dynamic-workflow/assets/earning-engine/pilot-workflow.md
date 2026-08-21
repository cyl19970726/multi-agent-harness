# Earning Engine Pilot Workflow

This pilot tests whether Star Harness can drive strategy-matrix work
without importing Earning Engine strategy code.

## First Pilot Scenario

Use the harness to review the BTC five-minute strategy family after a bounded
live or recent historical round.

```text
Leader
  -> create matrix audit task
  -> assign Matrix Curator, Strategy Research, Execution, Data Quality,
     Backtest Parity, Dashboard Review, and Critic/Risk messages
  -> collect tool evidence
  -> decide refine / kill / promote diagnostic live / create infrastructure task
```

## Required Evidence Pack

| Evidence | Tool descriptor | Required for |
| --- | --- | --- |
| rolling run schedulability | `strategy_harness_runs` | launch or monitor decisions |
| active/completed round status | `strategy_harness_status` | operations state |
| live child / post-live state | `strategy_live_status` | live operations review |
| completed-market packets | `strategy_harness_build` | strategy review |
| shared-input and freshness gate | `strategy_fabric_status` | data-quality review |
| live/backtest divergence summary | `strategy_calibration_summary` | parity review |
| Trial DAG node detail | `strategy_trial_inspect` | matrix identity and parameters |
| Trial DAG artifact index | `strategy_trial_artifact_inspect` | artifact discovery |
| local artifact ref | `strategy_artifact_inspect` | unlinked filesystem evidence |
| strategy dashboard link or screenshot ref | `strategy_dashboard_link` | visual review |
| explicit live permission decision | `strategy_live_launch_gate` | any fund-affecting action |

## Role Messages

The Leader should send role-specific task messages rather than one giant
prompt.

```text
Matrix Curator
  identify active strategy nodes, parameters, lineage, and prior run refs

Strategy Research
  explain the edge hypothesis and which strategy variants test which idea

Execution
  classify submit/ack/fill/no-fill/cancel/presign/GTD/merge lifecycle issues

Data Quality
  classify Binance/Polymarket freshness, direct-WS vs fabric, missing inputs,
  and consumer lag

Backtest Parity
  compare same-market live/backtest evidence and classify expected optimism
  versus model gaps

Dashboard Review
  verify pages show market context, entry/exit labels, order lifecycle, PnL,
  and comparison views

Critic / Risk
  reject unsupported causal claims, promotion without enough samples, unsafe
  wallet/order assumptions, or stale evidence
```

## Decision Template

```text
Decision:
  refine strategy | kill strategy | run bounded diagnostic live |
  create infrastructure task | update docs/skill

Evidence refs:
  run status:
  matrix / DAG / manifest:
  backtest:
  live:
  execution:
  data quality:
  dashboard:
  critic:

Required follow-up:
  next strategy task:
  next infrastructure task:
  promotion blocked by:
```

## Acceptance For This Adapter Slice

This slice is accepted when the adapter exposes the minimum tool surface and
the pilot workflow can create a matrix review task with evidence references.
It is not accepted as a strategy result and does not authorize live orders.

The first follow-up strategy task should review the 2026-05-24 S1 family
diagnostic and classify why most runs ended at
`blocked_by_completed_market_gate_after_review`.

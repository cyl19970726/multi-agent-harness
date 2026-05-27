# Goal Cases

Goal cases are reusable examples for future Lead Agents.

They are not raw transcripts. A case distills one completed, blocked, or
materially replanned goal into a compact lesson:

- what goal and scenario were being operated;
- how the Lead designed infra, agent team, and task graph;
- what evidence and decisions were produced;
- what the evaluator judged as good or bad;
- what future Leads should reuse or avoid.

## Storage Boundary

| Source | Purpose |
| --- | --- |
| `.harness/*.jsonl` | raw runtime trace and append-only operational truth |
| `.harness/evidence/**` | raw evidence, command outputs, reviews, and reports |
| `examples/goal-cases/**` | sanitized reusable examples and templates |

Do not copy secrets, wallet material, long provider transcripts, or noisy logs
into cases. Link to stable evidence refs when useful.

## Case Structure

Recommended files:

```text
examples/goal-cases/<case-id>/
  README.md
  goal-design.md
  evaluation.md
```

Use `_template/` when creating a new case.

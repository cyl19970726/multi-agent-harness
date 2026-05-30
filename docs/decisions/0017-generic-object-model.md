# 0017: Generic Object Model — Additive-Optional Schema Evolution

## Status

Accepted.

Implements the schema/data-contract checkpoint described in the generic
object-model migration plan (the let-me-try generalization design). This ADR
records the durable versioning decision and the six new generic objects so
future agents do not re-litigate them.

## Context

The harness needed to absorb the reusable *coordination contract* from a prior
trading project (let-me-try): an evidence to verdict pipeline, a
self-evaluation stop loop, typed review/evaluator output, gap/bug ledgers, and a
goal-design / goal-evaluation / goal-case learning layer. The value is the
generic mechanism, not the domain vocabulary. The migration therefore had to add
generic *shapes* while keeping harness core free of any domain words (no
trading, market, exchange, wallet, or feed terms).

The hard constraint: every existing schema is `additionalProperties:false` and
lists all of its properties in `required`. Adding a new required field to an
existing object would break every persisted JSONL row and every fixture. The
[0003 minimal-first-types](0003-minimal-first-types.md) and
[0015 evidence/message/decision-first](0015-autonomous-proposals-use-evidence-message-decision.md)
decisions also caution against freezing large schemas before a workflow is
proven.

## Decision

### 1. Additive-optional evolution, single schema file per object, no version field

New fields on existing objects (`Goal`, `Task`, `Evidence`, `Decision`) are
added as **property-but-NOT-required**, using nullable type unions
(`["string","null"]`) for scalars, arrays for lists, and booleans for flags.
Because the schemas remain `additionalProperties:false`, old rows that *omit* a
new optional property still validate (only *unknown* keys are rejected); new
writers add the key. This mirrors the existing `Evidence.task_id` precedent.

The Rust layer (a later work package) models these as `Option<T>` / `Vec<T>` /
`bool` with `#[serde(default)]`, so old JSONL deserializes unchanged.

There is **no `schema_version` field** and **no `*.v2` schema file**. If a
future field must be required, that — and only that — triggers a real versioned
schema plus a migration. For this migration, additive-optional is sufficient, so
version churn is explicitly avoided.

Added optional fields:

- `Goal`: `vision_id`, `goal_design_id`, `closed_by_decision_id`.
- `Task`: `phase`, `scope_refs`, `requires_human_approval`, `verdict_decision_id`.
- `Evidence`: `evidence_kind`, `goal_id`.
- `Decision`: `decision_kind`, `goal_id`, `is_waiver`, `follow_up_task_id`.

### 2. Six new generic objects

Each new object gets its own `schemas/<obj>.schema.json` (still
`additionalProperties:false`, with full `required` for its own mandatory fields)
plus valid and invalid fixtures:

- **Review** — first-class evaluator/critic output (verdict, blockers,
  residual_risk, missing_validation, backing evidence). A Review is *evidence
  for* a Decision; it is not itself the global decision.
- **Gap** — first-class gap ledger. A **Bug is a Gap with `category=bug`** plus
  optional `repro_ref` / `closing_test_ref`; there is no separate Bug object.
- **GoalDesign** — the executable goal thesis (scenario, non-goals, risk and
  permission boundaries, required infra, task graph, evidence plan, acceptance
  gates).
- **GoalEvaluation** — the retrospective (outcome, what worked / failed,
  reusable patterns, anti-patterns, follow-ups).
- **GoalCase** — an optional manifest over a reusable, sanitized teaching case.
- **Vision** — a minimal durable north-star (`summary`, `source_refs`) that
  goals reference via `vision_id`.

A **Phase is a `Task.phase` label** (with `parent_task_id`), not a Phase object.

### 3. Open-enum pattern, domain-neutral core

Verdict / decision / review_kind / evidence_kind / decision_kind are free
`string` (`minLength: 1`) in JSON Schema — *open enums*. The canonical generic
set is documented and (in Rust) modeled with an enum carrying
`#[serde(other)] Other(String)`, so adapters can extend the vocabulary without a
schema bump. Only truly closed, harness-owned sets use a hard JSON `enum`:
`Gap.severity` (`p0`/`p1`/`p2`) and `Gap.status`
(`open`/`in_progress`/`fixed`/`blocked`/`deferred`/`wontfix`).

Domain vocabulary (trading verdicts, market reviews, named artifact schemas)
never enters harness core; it lives in adapters/skills, in free `*_detail` /
`source_type` fields, or in `ToolDescriptor` artifact registrations.

## Consequences

- Existing fixtures and persisted JSONL rows stay valid with zero edits;
  omission of a new optional key is accepted.
- The schema spine can merge before any Rust change, gated by
  `pnpm check:schema-fixtures` and `validate:json`.
- Harness core gains generic shapes (Review, Gap, GoalDesign, GoalEvaluation,
  GoalCase, Vision) and a few open-enum vocabularies, but zero domain words.
- A future *required* field is the only trigger for a versioned schema +
  migration; that is out of scope here and flagged for the owner.
- Later work packages add the Rust structs, store readers, CLI commands, and
  dashboard surfaces; this ADR fixes only the schema contract and versioning
  policy.

## Validation

```bash
npx pnpm@9.15.4 check
```

The check runs `validate:json` (every JSON under schemas/docs/examples parses),
`check:schema-fixtures` (Ajv 2020 valid/invalid fixtures for every schema,
failing on empty fixture dirs), `check:doc-governance`, and `check:links`.

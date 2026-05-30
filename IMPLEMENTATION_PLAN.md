# Goal: Generic Harness Object Model (align with let-me-try, generalize)

Owner-defined goal (2026-05-30): adopt the good design from ALL `let-me-try/v1/.agents/skills`
(task/goal/evidence/review/decision/roles/workflow/project-state/etc.), but GENERALIZE it —
strip the code-trial/domain-specific parts — into one coherent object-model migration across
**schema + Rust backend + frontend**, because we are building a *generic* multi-agent harness.

Autonomy (owner choice): once the design plan is approved, execute autonomously — each WP on its
own branch, gate green (tsc+vite build, `pnpm check`, `cargo test`), screenshot self-review where
UI changes, then **auto-open + auto-merge PR**, loop until the goal's success criteria are met.
Schema/data-contract design is the one checkpoint (owner asked to be shown the detailed plan first).

## Step 0 — Research & plan (COMPLETE)

Deliverable shipped as `.harness-genplan.md` (sections 1–7): side-by-side object/field comparison,
genericization principles, the unified additive-optional schema, Rust/frontend change plans,
sequenced WPs, and risk/back-compat analysis. Owner approved the schema checkpoint
(additive-optional, single schema file per object, no `schema_version` field, the six generic
objects, Bug = `Gap(category=bug)`, Phase = `Task.phase`, open-enum pattern).

## Object-model migration — WP-A..G (ALL COMPLETE)

| WP | Scope | Status | PR |
| --- | --- | --- | --- |
| WP-A | Additive-optional schema spine + 6 new object schemas + ADR 0017 | done | #10 |
| WP-B | Rust core carries new optional fields on Goal/Task/Evidence/Decision | done | #11 |
| WP-C | Review object (schema + Rust + CLI + frontend) — structured evaluator output | done | #12 |
| WP-D | Gap object (incl. bug ledger) + Warnings ledger surface | done | #13 |
| WP-E | Learning layer (GoalDesign/GoalEvaluation/GoalCase/Vision) + Goal/Vision rendering | done | #14 |
| WP-F | Goal closeout + stop-gate + waiver enforcement | done | #15 |
| WP-G | Docs + registry governance for the generic object model | done | this PR |

Gates green throughout: `cargo test` (67 tests pass), `npx pnpm@9.15.4 check` (EXIT 0 — validate:json,
check:schema-fixtures, check:tool-descriptors, check:links, check:doc-size, check:skills,
check:doc-governance, tsc + vite build).

## Already shipped (this goal's runway)

- #7 Tailwind+shadcn rebuild + enriched Task/Goal detail (merged).
- #8 Docs cleanup: deleted deprecated specs, fixed stack-truth, ADR 0016; kept all 10 pages/*.md (merged).
- #9 WP1+WP2: honest disabled actions + rail 10→5 (Team/Vision/Tasks/Member/Warnings),
  Decisions→Warnings, Graph+kanban→Tasks, Debug→drawer, Docs off rail, tablet member-picker fix,
  timeline sort, GoalExtra cleanup (merged). master build green.

## Next phase — frontend roadmap

The object-model migration (WP-A..G) is complete. The remaining work is the
frontend roadmap, which is independent of the schema migration:

- **WP4 Member-to-spec** — render `runtime_health` layers, provider sessions, and
  child-threads on MemberWorkbench; fix the member picker across all widths; show
  reviews authored by a member (`reviewer_agent_id` join).
- **WP5 Graph canvas** — a real graph canvas using `depends_on_task_ids` edges
  with selection-sync between the graph and the Kanban lanes.
- **WP6 Docs context wiring** — drive DocsContext from the snapshot/registry so
  object-linked docs (including GoalCases as teaching docs) render live.

WP3 timeline correctness landed in #9. ProviderSession/ProviderChildThread depth
is WP4/WP5 scope, not the object-model migration.

## Constraints to respect

- schemas/*.json use `additionalProperties:false` + all-required → new fields are breaking; need a
  versioning/optionality strategy + fixture migration so `pnpm check` (validate-json,
  check-schema-fixtures) and `cargo test` stay green.
- Harness core stays domain-neutral; domain specifics live in adapters/skills.
- doc-governance: registry.json + check:links must stay green on any doc change.

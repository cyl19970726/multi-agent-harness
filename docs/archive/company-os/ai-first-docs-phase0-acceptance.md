# AI-first Docs Phase 0 acceptance contract

```text
status: canonical acceptance contract for the ADR 0054 Phase 0 slice
owner_role: Lead Agent + Docs Governance
authority_class: canonical_contract
canonical_for: acceptance criteria, commands, and process for the AI-first Docs Phase 0 slice
spec: docs/company-os/ai-first-docs-spec.md
decision: docs/decisions/0054-ai-first-docs-page-model-and-storage.md
```

This contract defines what counts as accepted for the AI-first Docs Phase 0
slice. Every item has an executable command and an unambiguous pass condition.
Acceptance is performed by an independent acceptance agent that runs each item
against the real worktree state and produces a structured report; any FAIL
must be fixed and re-verified before the slice is accepted.

## Preconditions

- Work happens in the worktree `.worktrees/ai-first-docs-v1` on branch
  `feat/ai-first-docs-v1`. The main worktree must contain no docs-v2 residue.
- `target/debug/harness` is built from this worktree (`cargo build -p
  harness-cli`).
- Frontend dependencies are installed in the worktree (`pnpm install`).

## Acceptance matrix

| # | Area | Criterion | Command | Pass condition |
| --- | --- | --- | --- | --- |
| A1 | Backend build | Whole workspace compiles | `cargo build` | exit 0, no errors |
| A2 | Backend tests | Core + store suites incl. docs_v2 pass | `cargo test -p harness-core -p harness-store` | all tests pass; docs_v2 suite >= 8 tests |
| A3 | Schema | docs-v2 schema is valid and discriminates kinds | `node /tmp/validate-docs-v2.mjs` or ajv compile of `schemas/company-os/docs-v2.schema.json` | compiles; closed kind set rejects legacy kinds |
| A4 | CLI full loop | create -> scoped reads -> write -> conflict -> replay -> append -> embed -> pinned reads -> friction-fix checks (F1 warnings, F2 anchor addressing, F3 search, F5 text format) | `node scripts/check-company-os-docs-v2-smoke.mjs` | exit 0, every PASS line present (30 checks) |
| A5 | Serve API live | pages index/create/read/write/append/revisions + 409 conflict + replay + token denial | `node scripts/check-company-os-docs-v2-api.mjs` | exit 0, 14 checks pass |
| A6 | Dashboard types | TypeScript compiles | `npx tsc -p apps/agent-dashboard/tsconfig.json --noEmit` | exit 0 |
| A7 | Dashboard build | Production bundle builds | `npx vite build --config apps/agent-dashboard/vite.config.ts` | exit 0 |
| A8 | Dashboard wiring | docs-v2 surface wired, store-live only, no fixture path | `node apps/agent-dashboard/tests/company-os-docs-v2-check.mjs` | exit 0 |
| A9 | Browser store-live | index/page/blocks/embed cards/inline transclusion/navigation/error honesty in chromium against a real serve | `node apps/agent-dashboard/tests/company-os-docs-v2-store-live-check.mjs` | exit 0, 18 checks pass |
| A10 | Governance | Registry + links + doc gates | `cargo run -q -p harness-cli -- governance check` | no failures for the new entries |
| A11 | Documentation | spec, ADR 0054, registry entries, decisions index, company-os README link present in the worktree | grep/ls per ADR 0054 acceptance notes | all files present; registry has both entries |
| A12 | Isolation | No docs-v2 residue in the main worktree | `git -C /Users/hhh0x/multi-agent-harness status --porcelain` filtered for docs-v2 names | no matching entries |
| A13 | No commit | Changes remain uncommitted | `git -C .worktrees/ai-first-docs-v1 status --porcelain` | modified/untracked entries present; `git log` head unchanged from origin/master |

## Revision semantics that A4/A5 must prove (behavioral contract)

1. Page creation commits revision 1 with a 64-hex content digest.
2. Scoped reads (`outline`, `keyword`, `section`, `range`) return honest
   `fragment` markers and `excerpt` ids; `simple` detail hides block ids,
   `with-ids`/`full` expose them.
3. Writes require `expected_revision`; a stale base returns
   `REVISION_CONFLICT` and appends nothing (CLI exit != 0 / HTTP 409).
4. The same `action_command_id` with the same payload replays the original
   revision (`replayed: true`, same revision id, history count unchanged); a
   divergent payload under the same id returns `IDEMPOTENCY_CONFLICT`.
5. Append supports anchors (`--after` / `after`); anchors from superseded
   revisions are invalid by design.
6. Full page writes replace the whole block set; `page_embed` blocks survive
   rewrites with their display mode.
7. Revision-pinned reads reconstruct historical snapshots.

## Acceptance process

1. The implementing agent finishes the slice and runs the full matrix itself.
2. An independent acceptance agent (separate subagent, no implementation
   context) receives only this contract plus the worktree path. It executes
   every item, records actual output, and returns a structured report:
   item id, PASS/FAIL, evidence line.
3. Any FAIL goes back to the implementing agent with the report; fixes land
   in the same worktree; the acceptance agent re-runs at least the failed
   items plus A1/A2 (regression guard).
4. The slice is accepted only when the report shows 0 FAIL across the whole
   matrix. Acceptance evidence (this document plus the final report summary)
   is retained with the Phase 0 records.

## Real-use acceptance (added 2026-08-06)

In addition to the automated matrix, the slice must survive a real operating
session: real project content (not test fixtures) created and maintained only
through the governed CLI, including a genuine `REVISION_CONFLICT` recovery and
an idempotent replay, plus a live serve + browser session captured as visual
evidence. The record, friction list, and screenshots live in
[real-use evidence](ai-first-docs-phase0-real-use-evidence.md); the capture
command is `node scripts/capture-docs-v2-real-use.mjs`. Friction items F1-F5
from that session were fixed the same day and are asserted by the extended
suites (smoke F1/F2/F3/F5 checks, API F4 resolution checks, browser F4
resolution checks).

## Out of scope for Phase 0 acceptance

SQLite FTS search, blob/attachment upload path, CommentThread UI, remote
authenticated multi-device proof, and BlockNote editor integration belong to
Phases 1-3 per the spec roadmap and are explicitly not accepted here.

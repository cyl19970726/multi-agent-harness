# AI-first Docs Phase 0 real-use evidence

```text
status: canonical acceptance evidence for the ADR 0054 Phase 0 real-use session
owner_role: Lead Agent
authority_class: actual_evidence
canonical_for: real-use acceptance record, observed friction, and visual evidence index of the docs-v2 slice
session_date: 2026-08-06
acceptance_contract: docs/company-os/ai-first-docs-phase0-acceptance.md
```

The automated acceptance matrix proves the docs-v2 mechanisms are correct. This
record proves they are usable: an Agent operated the system end to end on real
project content (the AI-first Docs module's own operating documents), and a
Human-reviewable dashboard session was captured as visual evidence.

## Store and pages

Company Store: `.real-use/harness-home`, company `ai-first-docs` (worktree
`.worktrees/ai-first-docs-v1`). Every page below was created and maintained
exclusively through governed CLI page operations; nothing was hand-written into
ledgers.

| Page | Final revision | Role |
| --- | --- | --- |
| `ai-first-docs-home` | r1 | Operating home; inline-transcludes the roadmap; embeds spec brief and ops log |
| `ai-first-docs-spec-brief` | r1 | Agent-readable condensed spec |
| `ai-first-docs-roadmap` | r3 | Phase truth; rewritten twice through `expected_revision` flow |
| `ai-first-docs-ops-log` | r3 | Append-only journal; three session entries appended by anchor |

## Session record (real operations, real outputs)

1. **R1 create** — four pages created from Markdown files; each returned
   revision 1 with a 64-hex digest (e.g. home digest
   `60f4d31137d224d6…`).
2. **R2 scoped reads** — `--scope outline --detail with-ids` on the home page;
   `--scope keyword "Phase|accepted" --context-after 1` on the roadmap
   returned an honest `fragment` with six excerpt block ids.
3. **R3 governed rewrite** — roadmap rewritten with `--expected-revision 1`
   -> r2.
4. **R4 anchored appends** — ops-log entries appended with `--after
   <block-id>`; anchor id obtained by a `--detail with-ids` read (two-step,
   see friction F2). Log advanced r1 -> r2 -> r3 across sessions.
5. **R5 real conflict and recovery** — a stale writer submitted from base r1
   and received `REVISION_CONFLICT: document ai-first-docs-roadmap is at
   revision 2, expected 1` (exit 1, nothing appended). Recovery: re-read the
   current revision, resubmit with base r2 -> committed r3.
6. **R6 idempotent replay** — the exact same command (same `--action-id`)
   returned `replayed: true` with the identical revision id and digest; the
   revision did not advance.
7. **R7 revision-pinned reads** — `--revision 1 --format markdown` returned
   the historical roadmap text while the latest read shows the completed
   checklist; history is reconstructable.
8. **R8 browser session** — `scripts/capture-docs-v2-real-use.mjs` booted
   `harness serve` over the same Store and browsed the docs-v2 surface in
   headless chromium: 6/6 assertions passed.

Raw session transcript: `.real-use/session.log` (worktree). Scope note: the
transcript covers R0-R7 commands; the final session-3 ops-log append was
executed outside the transcript capture, and its effect is proven by Store
truth (ops-log at r3 with three anchored entries) and the R8 capture
assertions.

## Visual evidence

`.visual-evidence/docs-v2-real-use-v1/` (this worktree):

| File | What it proves |
| --- | --- |
| `01-index.png` | Store-live index lists all four real pages with revision/block metadata |
| `02-home-inline-transclusion.png` | Home page renders; inline `page_embed` transcludes the live roadmap (table + checklist); card embed resolves the live spec-brief title |
| `03-roadmap-r3.png` | Roadmap at r3 with the completed real-use checklist item; revision banner shows store-live r3 |
| `04-ops-log.png` | Ops log at r3 with all three appended session entries |
| `capture-run.json` | Capture metadata: provenance, page list, final revision map, check counts |

All screenshots are 1536x1024 PNGs of the live dashboard against the real
serve process; no fixture participates in the docs-v2 data path.

## Friction observed (honest list)

All five items were subsequently implemented and are covered by check
assertions (smoke F1/F2/F3/F5, API F4, browser F4) as of 2026-08-06.

| # | Observation | Severity | Disposition |
| --- | --- | --- | --- |
| F1 | Embed targets are not validated at write time: a `page_embed` pointing at a missing page commits fine and renders a broken-ref card at read time. | low | **Resolved**: write results now carry `warnings[]` naming every missing `page_embed` target (writes remain non-blocking; broken-ref rendering stays honest). Smoke-asserted. |
| F2 | Anchored append needs the anchor block id, which requires a separate `--detail with-ids` read; ids are long. | medium | **Resolved**: `page append --after` accepts `-1`/`end` (document end) and `heading:<text>` (unique case-insensitive heading match; ambiguous/missing matches rejected with guidance). Smoke-asserted. |
| F3 | Search is per-document substring; the operator must know which page to read first. | medium | **Interim resolved**: `page search --keyword` scans all pages over latest projections and labels itself `projection-scan (not FTS)`; true SQLite FTS remains Phase 3 per spec. Smoke-asserted. |
| F4 | Entity embed cards show kind+id only; live title resolution for Views/records is not implemented. | low | **Resolved**: `GET /v1/company-os/docs-v2/pages/<id>` returns `resolved_embeds` (typed_record/view/work_item titles resolved live, missing targets `found:false`); dashboard cards consume it and expose `data-docs-v2-embed-resolved`. API- and browser-asserted. |
| F5 | CLI JSON output is verbose for quick human inspection. | low | **Resolved**: write commands accept `--format text` printing a one-line `ok <doc> r<n> sha256:<12>…` summary plus warnings. Smoke-asserted. |

No correctness bugs were found during real use. All conflicts, replays, and
pinned reads behaved exactly as the contract specifies.

## Conclusion

The Phase 0 slice is usable for real Agent operation: multi-page structures
with cross-page embedding, governed rewrites, anchored journal appends,
conflict recovery, and idempotent retries all work as a daily operating loop,
and the dashboard renders the resulting truth store-live. Friction items F1-F5
were fixed on 2026-08-06 in the same worktree and are covered by the extended
check suites; only true SQLite FTS search remains for Phase 3 per the spec.

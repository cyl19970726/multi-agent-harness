# AI-first Docs — Roadmap

Phase truth for the AI-first Docs module. Updated by governed page writes with
`expected_revision`; history is pinned by revisions.

## Phases

| Phase | Scope | State |
| --- | --- | --- |
| 0 | Schema + revision ledger + CLI + serve API + dashboard slice | accepted |
| 1 | Authenticated remote Company API, multi-device two-member proof | planned |
| 2 | Comments/mentions, propose-restore, BlockNote editor, Work revision pins | planned |
| 3 | SQLite FTS search, S3-compatible blobs, backup/restore | planned |
| 4 | Realtime Human co-editing PoC (Yjs), CRDT decision gate | deferred |

## Phase 0 checklist

- [x] docs-v2 schema with closed block kind set
- [x] harness-store revision ledger + optimistic concurrency + atomic writes
- [x] CLI page create/read/write/append with scoped reads
- [x] serve API docs-v2 endpoints (token-gated writes)
- [x] dashboard docs-v2 surface (store-live, zero fixture path)
- [x] independent acceptance audit (VERDICT: ACCEPTED, 13/13)
- [x] real-use acceptance session (in progress)

## Known gaps carried forward

1. `--detail full` not exercised by CLI checks (API uses it).
2. Superseded-anchor rejection asserted by design, not by a negative check.
3. Entity embed cards render refs only; live title resolution is Phase 1/2.

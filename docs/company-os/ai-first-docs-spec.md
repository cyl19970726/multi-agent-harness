# AI-first Docs module spec (v2 target)

```text
status: proposed target contract; non-canonical until ADR 0054 is accepted
owner_role: Docs Governance + Lead
authority_class: design_intent
canonical_for: nothing (pending ADR)
decision_target: ADR 0054 — AI-first Docs page model, revision contract, and storage
based_on: docs/research/ai-first-multi-device-docs-infrastructure.md,
          docs/company-os/document-system.md, lark-cli docs v2 surface study,
          product conversation 2026-08-05
```

This spec records the complete target definition of the Docs module agreed on
2026-08-05: product features, object model, storage, CLI/API contract,
frontend direction, technology selection, and the roadmap. It supersedes the
research proposal as the working blueprint once ADR 0054 accepts it; until
then it is design intent, not implementation proof.

## 1. Product stance

Docs is an **Agent-first document service**, not a local CLI tool and not a
Notion editor clone:

```text
AgentMembers on many machines
  -> stable CLI / HTTP API / MCP tools
  -> one governed Company Docs write service
  -> canonical append-only Company Store
  -> rebuildable query/search projections
```

- Agents are the primary writers and maintainers; Humans review content,
  structure, relations, diffs, risk, and outcomes in the UI.
- Pages stay **simple and beautiful**: a small closed block vocabulary,
  generous reading layout, no configuration-heavy editor chrome.
- Block capability is **retained as the storage and concurrency unit**;
  Markdown is the serialization and authoring unit. Neither is optional.
- Business facts never live inside page text. TypedRecord, View, WorkItem,
  Relation, and other Documents appear as typed embeds that resolve from
  their owning systems.

## 2. Design decisions (conversation record)

| # | Decision | Rationale |
| --- | --- | --- |
| D1 | Replace the access/write path, keep the object model. Document/Block/TypedRecord/Relation/View/BusinessModule survive. | The primitives are sound; the single-machine direct-write JSONL access model is what is being replaced. |
| D2 | Keep Block as canonical storage unit, but with a closed minimal kind set. | Typed embeds need stable anchors; no-CRDT optimistic concurrency needs a conflict granularity finer than whole-page text; block-level CLI commands were the complexity hotspot, so the set is deliberately small and CLI operates page-first. |
| D3 | Markdown-first serialization (CommonMark + GFM + a small reserved marker syntax). | CLI-first Agent authoring; human-readable diffs; lark-cli evidence that a serialized-document-plus-anchor model is the agent-friendly shape. |
| D4 | Whole-page immutable revisions with optimistic concurrency (`expected_revision_id`). | Agent writers submit discrete turn-based changes; revision pins support Work input/output evidence, diff, and restore. |
| D5 | Comments and mentions are annotations, not blocks. | Keeps revision snapshots, archive, and restore free of collaboration chatter; matches the research decision gate. |
| D6 | Pages can embed pages (link card or inline transclusion) and embed Views/records/work. Tree navigation plus in-page navigation are both first-class. | User requirement 2026-08-05; Notion-like navigability without a deep block tree. |
| D7 | Attachments are content-addressed blobs with digest verification; local-first, S3-compatible later. | Deterministic backup/restore; dedup; no ungoverned blob becomes document truth. |
| D8 | SQLite (FTS5) derived read/search layer, rebuildable from ledgers; never canonical. | Cheap to ship, satisfies search gap without violating ADR 0035. |

## 3. Object model

### 3.1 Closed block kind set

```text
content blocks:  paragraph | heading(1..6) | bullet_list | ordered_list |
                 checklist | quote | callout | code | table | divider
embed blocks:    page_embed(target document_ref, display=card|inline) |
                 entity_embed(entityRef: view|typed_record|work_item|relation,
                              display=card|inline) |
                 image(blob_ref) | attachment(blob_ref)
NOT blocks:      comment, mention -> CommentThread annotations (3.4)
```

Rules:

- The set is closed. Adding a kind is a schema change requiring ADR-level
  review, not an ad-hoc enum extension.
- Every kind has a deterministic content schema (no open `{}` payloads).
  `table` content is a typed header+rows structure that serializes to GFM
  tables; `callout` has `tone` + body; `code` has `language` + text.
- Embed blocks store only the ref plus display intent. Rendering resolves the
  target live; a missing target renders an explicit broken-ref state, never a
  copied snapshot.

### 3.2 Document / page model

- `Document(kind=page|template)` with `parent_document_id` hierarchy and
  `space_id`; nesting unlimited, rendering lazy.
- Directory-tree navigation and in-page `page_embed` are complementary:
  the tree is ownership structure, embeds are reading paths.
- Inline transclusion depth cap: 2 levels. Cycle detection on read; a cycle
  renders a warning card, not recursion.
- `page_embed(display=card)` shows title, snippet, lifecycle, updated_at,
  and maintainer — resolved live from projections.

### 3.3 Anchors and survival semantics

- Block ids are the primary anchors and are **stable by contract**:
  update/replace preserves the block id (append-only latest-row semantics),
  reorder preserves the set, archive/remove preserve the row. This fixes the
  lark-cli lesson where `block_replace` invalidates old ids.
- Heading slugs are derived, best-effort anchors for human links.
- Serialized Markdown carries ids only in `with-ids` fetch mode
  (attribute-style markers); plain Markdown output stays clean.
- Every write command documents its anchor effect (preserved / replaced /
  removed) in the result envelope.

### 3.4 Annotations

```text
CommentThread
  id / document_id / anchor(block_id | block_range | revision_id)
  status(open | resolved) / created_by / created_at
  comments[]: id, body(markdown), author(ActorRef), in_reply_to?, created_at
```

Mentions are references inside comment bodies or paragraph marks pointing at
Actors; they notify, they do not authorize.

## 4. Revision and concurrency contract

```text
DocumentRevision
  id / document_id / revision_number / parent_revision_id?
  content_snapshot      # normalized ordered blocks payload
  content_digest        # sha256 of normalized snapshot
  change_summary / authored_by / execution_ref? / action_command_id / created_at

DocumentChangeOperation
  action_command_id     # canonical idempotency key (existing ActionCommand.id)
  document_id / expected_revision_id
  mutations[]           # typed Docs-only operations
  resulting_document / resulting_blocks[] / document_revision
```

- One accepted write = one revision. Snapshots reconstruct the full document;
  mutable-id lists are insufficient (latest-row projections can erase history).
- Concurrency: submit with `expected_revision_id`. Match -> append, return new
  revision. Mismatch -> `REVISION_CONFLICT` + safe rebase context. The server
  never silently re-applies intent.
- Idempotency: same `ActionCommand.id` + same payload returns the original
  result; same id + different payload is an idempotency conflict.
- Safe automatic rebase is allowed only for pure append-after-stable-anchor
  operations. Replace/reorder/delete/relation changes require explicit
  conflict resolution.
- Crash consistency across ActionCommand, DocumentChangeOperation,
  DocumentRevision, projections, and terminal AuditEvent is defined in
  ADR 0054, not left as independently-valid disagreeing rows.

## 5. Storage model

| Layer | Choice | Role |
| --- | --- | --- |
| Canonical write store | Append-only JSONL ledgers + latest projections (existing pattern). Ledgers: document, block, typed_record, relation, view, business_module, **document_revision (new)**, **document_change_op (new)**, **comment_thread (new)**, **blob_meta (new)** | Execution truth; inspectable; single writer service per Company Store |
| Derived read/search | SQLite + FTS5, rebuildable from ledgers, freshness observable | Search, scoped reads, outline/section indexes. Never authorizes writes |
| Blobs | Content-addressed local store `<company>/blobs/sha256/<2>/…`; blob_meta ledger rows carry digest/size/mime/name/provenance | Attachments, images. Digest verified on read; dedup by digest |
| Hosted phase (later) | S3-compatible object store behind the same blob contract; SeaweedFS (Apache-2.0) as self-hosted candidate | MinIO is AGPL-3.0 and requires a license review before adoption |
| Live truth boundary | One authenticated Docs write service per Company Store; direct ledger/database writes are not an Agent path | Multi-device safety without multi-writer transactions |

Migration: existing Block rows map into the closed kind set; unmappable kinds
serialize to paragraph/table content with a migration note; comment/mention
blocks move to CommentThread annotations. Old block-level CLI commands are
deprecated with forwarding to the new page-level surface.

## 6. CLI / API contract (borrowed shape from lark-cli v2)

### 6.1 Reads

```text
harness company docs page read --doc <id|url>
  [--scope outline|section|range|keyword]   # partial-first reading
  [--detail simple|with-ids|full]           # detail by intent: browse/locate/edit
  [--revision <n|-1>]
  [--start-block-id <id>] [--end-block-id <id>] [--keyword "a|b"]
  [--context-before N] [--context-after N] [--max-depth N]
harness company docs search | traverse | refs | related | health | diff | snapshot
```

- Partial reads are the default discipline: outline -> section/range/keyword.
- Scoped output is wrapped in `<fragment>`; non-top-level slices are marked
  `<excerpt top-block-id=…>` so callers never mistake an excerpt for full
  content. Tables default to slim (header + matched rows).
- Keyword matching falls back in levels: substring -> normalized -> token
  forms -> regex. `|` separates OR branches.
- Every read returns `revision_id`.

### 6.2 Writes

```text
page create / page write (full replace, --expected-revision)
page append (end, or after anchor)
str_replace --pattern … --content …          # markdown mode: cross-line + "prefix...suffix"
block_insert_after / block_replace / block_delete / block_move_after
revision list | show | diff | propose-restore (gated)
comment add | reply | resolve
```

- `--expected-revision` baseline on every content write (default -1 = latest
  for low-risk writes; required for replace/move/delete).
- Result envelope: `success | partial_success | failed` + `warnings[]` +
  `new_revision_id` + `affected_blocks[]` (with anchor-effect notes).
- Risk levels label every command: `read | write | high-risk-write`;
  high-risk requires explicit `--confirm`.
- Content input: Markdown default; XML-like rich syntax only if a future kind
  exceeds GFM expressiveness (declared lossy boundaries documented).

## 7. Frontend direction

- Editor: **BlockNote (MIT, Notion-style block editor built on Tiptap)** for
  the Human review + low-risk authoring surface; every mutation still
  dispatches the governed page/block commands through the existing Action
  transport — the editor never writes the store directly.
- Read rendering resolves embeds live (View tables, record cards, transcluded
  pages) from projections; fixture fallbacks must be labeled as such.
- Tree navigation panel + breadcrumb + in-page embed cards; back-links
  derived from Relations and page_embed refs.
- Phase 4 (later): realtime co-editing via Yjs + Hocuspocus (both MIT) as a
  client of the same revision API; CRDT only if measured demand justifies it.

## 8. Technology selection

| Concern | Selection | License | Alternatives / notes |
| --- | --- | --- | --- |
| Markdown parse/serialize (Rust) | comrak (GFM-complete) | BSD-2 | pulldown-cmark (MIT) if extension model fits better |
| Derived read/search | rusqlite + FTS5 | MIT | sqlx if async becomes necessary |
| IDs | ulid crate | MIT | existing repo id conventions apply |
| Frontend editor | BlockNote on Tiptap | MIT | Plate (MIT), Lexical (MIT); plain CodeMirror 6 rejected: block/embed structure needs a block editor |
| Read-only render | react-markdown + remark-gfm | MIT | only for non-editor surfaces |
| Realtime (Phase 4) | Yjs + Hocuspocus | MIT | deferred; decision gate before adoption |
| Blob store (hosted, later) | S3-compatible; SeaweedFS candidate | Apache-2.0 | MinIO AGPL-3.0 requires license review first |
| Service/API | existing harness serve stack (axum) | repo | reuse company_os_api surface; no new framework |

## 9. Key models that must be nailed down (checklist)

Each item needs its section in ADR 0054 or a follow-up ADR before code claims
it:

1. Block kind set + per-kind content schema (closed).
2. Anchor model + survival semantics per write command.
3. `DocumentRevision` snapshot normalization + digest rules.
4. `DocumentChangeOperation` atomicity + crash-consistency envelope.
5. Concurrency envelope: `REVISION_CONFLICT`, safe-rebase whitelist,
   idempotency rules.
6. Embed reference model: entity kinds, display modes, broken-ref rendering.
7. Page-in-page transclusion: depth cap, cycle detection, permission
   inheritance.
8. CommentThread/mention annotation model + anchor preservation across
   revisions.
9. Markdown ⇄ block serialization contract (lossless within the closed set;
   declared lossy boundaries outside it).
10. Blob/attachment model: addressing, dedup, digest verification, lifecycle.
11. Derived read layer rebuild contract + freshness observability.
12. Remote identity/attribution: AgentMember -> credential/MemberRun mapping
    for writes; authority ceiling enforced server-side.

## 10. Roadmap

```text
Phase 0 (now):   this spec + ADR 0054 + schema draft + registry entry
                 + vertical slice: revision ledger, page read/write/append CLI,
                   page-level rendering in dashboard, tests + smoke script
Phase 1:         authenticated remote Company API; CLI endpoint/company
                 selection; two-machine two-member proof; direct-write lockdown
Phase 2:         full revision/conflict/replay/diff/history/propose-restore;
                 comments/mentions; BlockNote editor upgrade; Work revision pins
Phase 3:         SQLite FTS search; S3-compatible attachments; backup/restore
                 + writer failover fencing
Phase 4:         realtime Human co-editing PoC (Yjs); CRDT decision gate
```

## 11. Acceptance criteria (adapted PoC matrix)

A capability is implemented only when schema/store/API + UI path + evidence
prove it. Minimum gates for Phase 0/1:

- Concurrent write from the same base revision: exactly one accepted; the
  other receives `REVISION_CONFLICT`; no lost update.
- Idempotent retry with the same `ActionCommand.id` returns the original
  revision; no duplicate revision.
- Atomic multi-block change: readers see the whole resulting revision or none.
- Page embed renders live truth; target archive/remove shows broken-ref state,
  not a stale copy.
- Scoped read returns honest `<fragment>`/`<excerpt>` markers; slim tables by
  default.
- Blob digest mismatch on read is detected and reported.
- Rebuild of the derived SQLite index reproduces identical authorized results.
- No private reasoning or provider transcript appears in revisions, comments,
  logs, or indexes.

## 12. Relation to current implementation

- **Kept**: ledger + projection architecture; governed Action envelope;
  TypedRecord/Relation/View/BusinessModule layer; read-side commands
  (query/search/traverse/refs/related/health/diff/snapshot/change-report);
  module and page-definition governance.
- **Replaced**: direct block-command sprawl collapses into the page-level
  command surface (section 6); `comment`/`mention` leave the block enum;
  open `content: {}` payloads become per-kind schemas.
- **Added**: revision + change-op ledgers, CommentThread ledger, blob store,
  SQLite derived layer, remote authenticated API, BlockNote surface.
- Historical block-era acceptance evidence remains valid as evidence of the
  old surface; the new surface earns its own evidence per section 11.


## 13. Old surface retirement plan

The Block-era Docs surface is retired in stages, never by a single deletion:
each stage lands only when the v2 surface provably covers what it removes.
Deletion gates follow `documentation-governance.md` (consumers migrated,
checks updated, registry statuses changed, data readable or migrated).

| Stage | Content | Gate |
| --- | --- | --- |
| R0 (done) | `document-system.md` and `docs-operating-surface-matrix.md` carry supersession banners; v2 merged (PR #353) | banners + ADR 0054 |
| R1 (done) | v2 `page rename/move/archive` metadata commands landed through the revision mechanism (move with parent-cycle rejection, archive behind `--confirm`, dry-run by default); templates declared legacy-retained on the old surface until a v2 template need is real | asserted by the v2 smoke suite (rename/move/archive/cycle/stale-revision checks) |
| R2 (done) | Both `?surface=docs` document deep links and `?surface=docs-v2` render through DocsV2Surface over the v2 page endpoint; legacy ledger documents render as honest read-only projections (`legacy_projection` banner); `BasicDocumentPage`, `documentTree`, and the document/block builders of `documentAction.ts` deleted; fixtureAdapter no longer projects a document focus view-model | company-os-docs-check rewritten (92 checks), navigation/recursive-org checks updated, full check:company-os chain green |
| R3 | Old `document/block` CLI command tree and `document.append`/`block.append` API actions are deleted (not deprecated-forwarded); the record-layer commands (`module`, `typed-record`, `view`, `relation`, health, source sync) remain — they are the shared record layer, not old docs | check-company-os-docs-cli-smoke/live retired; v2 suites green; trademark scenario migrated or explicitly grandfathered |
| R4 | `company_os_blocks.jsonl` stays readable as historical data (never deleted in place); `knowledge.schema.json` Block definitions drop to archived status once no live code path reads them; the two superseded contract docs move to registry `archival` status or are deleted with Git history as the archive | governance check green after each change |

Explicit non-goals of the retirement: no loss of module/typed-record/view/
relation capability, no rewrite of existing ledger data, and no acceptance
gap — every stage keeps the full check chain green before it lands.

# ADR 0054: AI-first Docs page model, revision contract, and storage


> **Superseded by DOC-108 (legacy CompanyOS retirement, 2026-08-17).**
> This ADR is retained as historical evidence only; its object model is not
> current authority. See `docs/current/product/prd.md` and
> `docs/current/architecture/architecture-map.md`.

**Date:** 2026-08-05
**Status:** accepted
**Spec:** docs/current/company-os/ai-first-docs-spec.md

## Decision

Adopt the AI-first Docs target defined in the spec:

1. **Block stays the canonical storage and concurrency unit**, restricted to a
   closed minimal kind set (paragraph, heading, bullet_list, ordered_list,
   checklist, quote, callout, code, table, divider, page_embed, entity_embed,
   image, attachment). Comments and mentions are annotation records
   (`CommentThread`), not blocks.
2. **Markdown-first serialization.** CommonMark + GFM is the authoring and
   diff surface; block ids surface only in `with-ids` fetch mode. The
   serialization contract is lossless within the closed kind set and declares
   its lossy boundaries outside it.
3. **Whole-page immutable revisions.** Every accepted write produces a
   `DocumentRevision` (normalized snapshot + sha256 digest). Writes carry
   `expected_revision_id`; mismatches return `REVISION_CONFLICT` with safe
   rebase context. Idempotency keys are existing `ActionCommand.id`s. Multi-
   block changes persist as one `DocumentChangeOperation` row so readers see
   the resulting revision atomically.
4. **Storage keeps append-only JSONL ledgers + latest projections as the
   canonical write Store**, adding `document_revision`,
   `document_change_op`, `comment_thread`, and `blob_meta` ledgers. SQLite
   + FTS5 is a rebuildable derived read/search layer, never canonical
   (consistent with ADR 0035). One authenticated Docs write service per
   Company Store; direct ledger writes are not an Agent path.
5. **Attachments are content-addressed blobs** (`sha256`), local-first under
   the Company Store with digest verification; an S3-compatible hosted tier
   comes later behind the same blob contract.
6. **CLI operates page-first** (`page read` with scope/detail, `page write`,
   `page append`, anchor-scoped block commands) with honest result envelopes
   (`success | partial_success | failed` + warnings), risk-labelled commands,
   and `REVISION_CONFLICT` semantics.
7. **Pages embed pages** via `page_embed` (card or inline transclusion, depth
   cap 2, cycle detection) and embed Views/records/work via `entity_embed`;
   embeds resolve live and never copy truth.

## Context

The research proposal `docs/archive/research/ai-first-multi-device-docs-infrastructure.md`
established the direction (self-built semantic Docs core, remote Agent-first
service, revision boundary before multi-writer). The 2026-08-05 product
conversation fixed three additional constraints: keep block capability, keep
pages simple and beautiful, and make tree navigation and in-page embedding
(pages, tables) both first-class. A surface study of lark-cli docs v2
confirmed the serialized-document-plus-anchor CLI shape and contributed the
scope/detail read contract, honest fragment/excerpt markers, and the
anchor-stability lesson (their block ids do not survive replace; ours must).

The current single-machine block-command sprawl (append/update/archive/
remove/reorder plus `document.append` bookkeeping) is the layer being
replaced; the object model, governed Action envelope, and
TypedRecord/Relation/View/BusinessModule layer are retained.

## Consequences

- `document-system.md` remains canonical for the current surface until the
  Phase 0 slice lands; the spec is design intent until each capability earns
  its own acceptance evidence.
- Block-era CLI commands enter a deprecation path with forwarding to the
  page-level surface; their acceptance evidence remains historical evidence.
- Schema changes: closed block kind enum, per-kind content schemas replace
  open `{}` payloads, new `DocumentRevision` / `DocumentChangeOperation` /
  `CommentThread` / `blob_meta` definitions.
- Crash consistency across ActionCommand, DocumentChangeOperation,
  DocumentRevision, projections, and terminal AuditEvent is owned by the
  single-writer Store boundary and must be covered by tests.
- Remote authentication, AgentMember-to-MemberRun attribution, and
  single-writer fencing for the hosted phase each need their own follow-up
  ADR (Phase 1+); this ADR fixes only the local slice and the contract shape.
- Editor technology (BlockNote/Tiptap) is a client decision: it must dispatch
  governed commands and must never become a second write path.

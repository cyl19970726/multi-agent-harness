# AI-first Docs — Spec Brief

Condensed from `docs/company-os/ai-first-docs-spec.md` and ADR 0054. The full
spec remains the authority; this page is the agent-readable operating summary.

## What it is

An **Agent-first document service**: Agents write through CLI/API, Humans
review in the UI. Pages stay simple; business facts stay typed and live in
their owning systems.

## Core contract

1. Closed block set: paragraph, heading, lists, checklist, quote, callout,
   code, table, divider, page_embed, entity_embed, image, attachment.
2. Every accepted write produces an immutable `DocumentRevision` (normalized
   snapshot + sha256 digest).
3. Writes carry `expected_revision`; stale bases return `REVISION_CONFLICT`.
4. `action_command_id` is the idempotency key: same payload replays, divergent
   payload conflicts (`IDEMPOTENCY_CONFLICT`).
5. `page_embed` resolves live (card or inline, depth cap 2, cycle-safe).

> [!warning] Iron rules
> Embeds never copy truth. Fixtures never masquerade as store-live. UI-only
> editing never counts as implementation evidence.

## Interface priority

- Agent CLI/API first
- governed Action envelope
- Human review UI second
- rich collaborative editing last (Phase 4)

# Recursive Org + Docs + Works visual and interaction contract

```text
status: Wave 0 contract — design intent only; no production component edits
owner_role: product-design
canonical_for: recursive Organization, global Works, Member Focus, Team War
  Room, and Docs-to-Work handoff visual, data-provenance, interaction, and
  responsive acceptance
```

This slice freezes the implementation-ready UX contract for the ADR 0052
recursive Agent Team Organization era. It covers exactly five surfaces:

1. **Organization** — the recursive AgentTeam tree (root Team, child Teams,
   Host/direct-Member topology) with per-node Work counts and a strict
   separation between durable identity and runtime state.
2. **Global Works** — one aggregate projection over the recursive Team tree
   with the four demand classes (discovered-unassigned, self-owned, delegated,
   follow-up) and an unassigned first-class queue.
3. **Member Focus** — the accepted Agent Team Member Focus, reused and
   extended with created/child Work, child Team, and durable AgentMember
   identity through explicit links only.
4. **Team War Room** — the existing War Room (Works, Activity, Members,
   mailboxes, truthful capacity) plus Organization breadcrumbs and recursive
   drilldown; this slice does not create a second War Room implementation.
5. **Docs-to-Work handoff** — Document/Block to Work creation, linking, and
   result return using only the relations named in the canonical contracts.

Scope rules that bind every later implementation Wave:

- **No invented fields.** Every rendered value is classified in
  [data-provenance.md](data-provenance.md) as IMPLEMENTED (schema + store
  today), TARGET (PR #302 / ADR 0052 design contract, not yet shipped), or
  RESEARCH (directional candidate only). Production UI renders IMPLEMENTED
  fields plainly and must not show TARGET or RESEARCH fields until the
  corresponding store/API lands.
- **No second implementations.** Member Focus and Team War Room are reused,
  not rebuilt (`specs/nested-agent-team-organization/design.md` UI
  architecture; `docs/company-os/frontend-information-architecture.md`).
- **No unrelated redesign.** The warm light visual system, coral primary,
  status palette, FocusShell pattern, and navigation groups stay as they are.
  Only the five surfaces above change, and only as far as this contract
  states. See [expected-vs-actual.md](expected-vs-actual.md) for the exact
  production components likely to change and the explicit non-goals.
- **Truthful states.** Loading, empty, failed, and unavailable are visually
  distinct; ancestry, responsibility, availability, and runtime health are
  never inferred from names, sessions, authorship, or first-row fallback.

## Baseline viewports

```text
desktop: 1440 × 1000   (IA canon)
tablet:   900 × 1180   (IA canon)
mobile:   390 × 844    (IA canon)
small:    320 × 720    (overflow guard, inherited from execution-workbench-v4)
```

These match the viewport matrix already asserted by
`apps/agent-dashboard/tests/team-war-room-first-viewport-check.mjs` and
`member-run-focus-mobile-check.mjs`, so acceptance hooks into the existing
`*-check.mjs` geometry-probe idiom. Note the deliberate deviation from the
1536×1024 captures committed under `company-os-v2`/`company-os-v5`: new work
follows the IA canon; historical baselines remain valid for their slices.

## Evidence policy

Raw browser evidence is local and ignored under
`.visual-evidence/company-os-v6/recursive-org-docs-works-v1/`. Approved
Expected assets and selected comparisons will be stored beside this contract.
The machine contract must never name an implementation capture that is absent
from the evidence directory; an uncaptured case remains explicitly `null`.

Wave 0 Expected assets are the text wireframes in
[wireframes.md](wireframes.md), referenced from `visual-contract.json`.
Approved Expected PNGs replace them per the pipeline in
`docs/design/company-os-v2/visual-index.md` before broad styling changes.

Reproducing future evidence: add a fixture route to
`apps/agent-dashboard/fixtures/workbench-layout-v2-native-v1/fixture-manifest.json`,
run `pnpm visual:fixture` then `pnpm visual:capture:workbench` (Playwright,
deterministic context), and extend a `*-check.mjs` sibling with the same
geometry probes at the four baselines above.

## Placement decision

Opened as `company-os-v6` because the repository opens a new design area when
the canonical product model moves (v3 native trademark closure, v4 Standing
Agent workspace, v5 AgentOS self-hosting loop). The recursive AgentTeam
Organization model (ADR 0052; commits `b01df88`, `b23b7ac`) is the same class
of change, and the v5 slice stays scoped to the self-hosting loop. The Host
was notified on the Work thread; relocation into `company-os-v5/` remains
trivial while this slice is self-contained.

## Authoritative inputs

- `docs/company-os/frontend-information-architecture.md` — navigation,
  ownership, visual grammar, responsive policy, truthful-state rules.
- `docs/company-os/nested-agent-team-organization.md` and
  `specs/nested-agent-team-organization/` — recursive Org/Work target
  contracts (PR #302, open at branch `codex/nested-org-agent-teams-spec-v1`).
- `docs/company-os/organization-and-actors.md`,
  `docs/company-os/collaboration-and-agent-work.md`,
  `docs/company-os/agentos-self-hosting-loop.md` — product surfaces and UI
  acceptance.
- `docs/product/agent-team-works.md` — implemented Team Work board semantics.
- `schemas/` and `schemas/company-os/` — implemented wire truth.
- `docs/research/ai-first-multi-device-docs-infrastructure.md` — RESEARCH
  class only (DocumentRevision, DocumentInputRef/DocumentResultRef).
- `docs/company-os/implementation-truth-matrix.md` — target-vs-implemented
  arbitration.

## Files

- [Data provenance](data-provenance.md) — every rendered field, its source
  contract, and its IMPLEMENTED / TARGET / RESEARCH class.
- [Interaction contract](interaction-contract.md) — navigation, click,
  scroll, filter, loading, empty, and error rules per surface, plus motion.
- [Wireframes](wireframes.md) — Wave 0 Expected layouts per surface at
  desktop, tablet, 390, and 320.
- [Page matrix](page-matrix.md) — per-state, per-viewport acceptance rows.
- [Expected vs actual](expected-vs-actual.md) — gap map against the current
  dashboard, exact components likely to change, acceptance checks, non-goals.
- [Machine visual contract](visual-contract.json) — case list for visual
  acceptance tooling.

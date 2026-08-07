# Page matrix

Expected = the Wave 0 text wireframes (anchor in
[wireframes.md](wireframes.md)); they are replaced by approved Expected PNGs
per the evidence policy. Implemented stays `Pending` in Wave 0 — production
components are untouched by this slice. Baseline names the existing automated
coverage that pins today's behavior; "none" means the surface is new.

| Priority | Page | State | Viewport | Baseline | Expected | Implemented | Review |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P0 | Organization | recursive tree, root selected | 1440×1000 | none (new surface) | wireframes.md §1a | Pending | topology truth + durable/runtime badge separation |
| P0 | Organization | drill-down list | 390×844 | none | wireframes.md §1c | Pending | one-level-per-screen + breadcrumb |
| P0 | Organization | drill-down guard | 320×720 | none | wireframes.md §1d | Pending | no overflow, wrapped badges |
| P1 | Organization | tree + detail below | 900×1180 | none | wireframes.md §1b | Pending | Context & controls disclosure |
| P1 | Organization | empty (no root Team) | 1440×1000 | none | interaction-contract.md §Organization | Pending | honest empty, no fixture tree |
| P1 | Organization | integrity finding | 1440×1000 | none | interaction-contract.md §Organization | Pending | findings verbatim, no auto-repair |
| P0 | Global Works | aggregate, demand groups | 1440×1000 | none (WorkOperatingPage covers WorkItems only) | wireframes.md §2a | Pending | four demand classes + provenance per row |
| P0 | Global Works | queue + cards | 390×844 | none | wireframes.md §2c | Pending | first viewport keeps demand chips + queue |
| P0 | Global Works | guard | 320×720 | none | wireframes.md §2d | Pending | no overflow |
| P1 | Global Works | stacked groups | 900×1180 | none | wireframes.md §2b | Pending | pinned wrapped filter bar |
| P1 | Global Works | empty vs no-match | 390×844 | none | interaction-contract.md §Global Works | Pending | distinct copy + reset |
| P0 | Member Focus | extended sections | 1440×1000 | `apps/agent-dashboard/tests/member-run-focus-mobile-check.mjs` + `docs/dashboard/pages/member-run-focus.md` | wireframes.md §3a | Pending | created/child Work, child Team slot, durable identity via explicit link |
| P0 | Member Focus | compact | 390×844 | same member-focus check (asserts 390 + 320) | wireframes.md §3b | Pending | section order after Work queue |
| P1 | Member Focus | compact guard | 320×720 | same | wireframes.md §3b | Pending | existing composer-width assertion holds |
| P0 | Team War Room | breadcrumb + child-Team row | 1440×1000 | `apps/agent-dashboard/tests/team-war-room-first-viewport-check.mjs` + `docs/dashboard/pages/team-run-war-room.md` | wireframes.md §4a | Pending | breadcrumb renders only proven ancestors |
| P0 | Team War Room | stacked lanes | 390×844 | same war-room check (asserts 390 + 320) | wireframes.md §4b | Pending | child-Team card above tabs |
| P1 | Team War Room | stacked guard | 320×720 | same | wireframes.md §4b | Pending | no overflow |
| P0 | Docs-to-Work handoff | document with Related Works + selection actions | 1440×1000 | `docs/dashboard/pages/` Docs specs + existing `RelatedWorkBlock` (`data-docs-related-work`) | wireframes.md §5a/§5b | Pending | three legal placements; owner never inferred |
| P0 | Docs-to-Work handoff | bottom-sheet actions | 390×844 | none for handoff actions | wireframes.md §5c | Pending | full-screen route, 44px actions |
| P1 | Docs-to-Work handoff | mismatch chain status | 1440×1000 | none | interaction-contract.md §Docs-to-Work | Pending | `link_status` vocabulary verbatim |

P0 Expected assets must be approved before broad styling changes. Product-truth
and interaction fixes that remove invented relations or restore broken
navigation may proceed without changing the approved visual language.
Acceptance mechanism: extend the existing `*-check.mjs` geometry-probe idiom
(Playwright + fixture snapshot, `pnpm check:dashboard`) with rows from
`visual-contract.json`; captures land under
`.visual-evidence/company-os-v6/recursive-org-docs-works-v1/` and only
approved assets are committed beside this contract.

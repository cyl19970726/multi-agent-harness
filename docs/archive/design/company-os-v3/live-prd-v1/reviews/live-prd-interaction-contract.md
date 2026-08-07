# Live PRD interaction contract

## Context

- Parent object: Company OS product documentation.
- Primary journey: choose a business line, inspect its page journey, select a
  handoff, verify the route/object/authority contract, and open the referenced
  product or architecture evidence without losing report context.
- Covered viewports: desktop `1536×1024`, tablet `900×1180`, mobile
  `390×844`.

## Hotspots

| # | Object | Kind | Destination/action | Preserved context | Focus result |
| --- | --- | --- | --- | --- | --- |
| 1 | Report view | link | `?view=overview|journey|architecture` | selected business line and return URL | target view heading |
| 2 | Business line | control | replace journey data in place | current report view | selected line heading |
| 3 | Journey step | control | select handoff and update inspector | business line and journey | jump-contract heading |
| 4 | Product evidence | link/control | open source image dialog | selected step | dialog close control |
| 5 | Canonical document | link | repository Markdown/HTML source | browser history | linked document heading |

## Scroll owners

| Viewport | Region | Owner | Sticky/fixed elements | Reachability assertion |
| --- | --- | --- | --- | --- |
| desktop | report | document body | contents rail and optional truth rail | final truth matrix is reachable |
| desktop | journey steps | journey strip only when required | none | all six steps keyboard/pointer reachable |
| tablet | report | document body | compact view navigation | no nested vertical scroll traps |
| mobile | journey steps | horizontal snap strip | none | every step reachable without page overflow |
| mobile | source figure | labelled figure pan region | none | full-resolution image and transcript reachable |

## State and motion

| Trigger | Pending/success/failure | Motion | Reduced motion |
| --- | --- | --- | --- |
| change business line | immediate local replacement; unsupported detail stays Expected-labelled | 160ms opacity | no transition |
| select handoff | inspector and active border update together | 140ms color/opacity | instant |
| open evidence | native dialog; missing image exposes alt/source link | 160ms backdrop | instant |
| change report view | URL and visible panel update; invalid view falls back to overview | 180ms opacity | instant |

## Browser journeys

| Id | Fixture/route | Actions | Assertions |
| --- | --- | --- | --- |
| content-reachability | `live-prd.html?view=overview` | scroll top to sources | final source links visible; no console error |
| entity-deep-link | `?view=journey` | choose Brand & IP; select Work → Approval | exact inspector route/refs visible |
| return-context | journey evidence dialog | open and close evidence; use browser Back after view link | selected line/handoff retained where applicable |
| keyboard-path | `?view=journey` | Tab to line and step; Enter/Space activate | same inspector state as pointer |
| responsive-path | all three views | repeat primary path at 1536, 900, 390 | no page-level horizontal overflow; context remains reachable |
| motion-policy | all three views | emulate reduced motion | non-essential transitions disabled |

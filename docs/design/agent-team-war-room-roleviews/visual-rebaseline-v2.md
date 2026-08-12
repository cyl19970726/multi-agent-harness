# Agent Team visual rebaseline v2

Status: **frozen analysis; expected-v2 candidates pending explicit aesthetic
approval; implementation visual gate is failed**.

This document replaces every earlier claim that the Agent Team UI was visually
ready. Product-truth, authorization, accessibility and responsive checks remain
useful evidence, but none of them implies visual acceptance.

## 1. Revision and worktree state at rebaseline

- Branch: `codex/restore-agent-team-war-room-roleviews`
- HEAD when this rebaseline started:
  `eaf13a2798bc20b1bbe12aee629d0315d6bb1ac6`
- `origin/master` at the same checkpoint:
  `c0ab362ab887d7b40321089b85676beb58a394be`
- The branch contains local commits after the last pushed PR head. They are not
  a visually accepted candidate and must not be pushed as ready.
- Dirty experiment at rebaseline:
  `TeamWorkspace.tsx`, `TeamWorksBoard.tsx`, and the War Room browser check.
  These edits are a geometry experiment against Works expected-v2, not accepted
  runtime source.
- `expected-v2/` is now a governed candidate-design directory. It is not
  implementation evidence and must never be used to prove a source SHA.

## 2. Immutable visual sources and evidence roles

The seven files under `reference/` are the user's accepted visual-language
sources. They remain immutable. They establish composition, typography,
portraits, icon scale, warm paper, coral focus, hairlines and density. They do
not override the current product model.

| File | Evidence role |
| --- | --- |
| `reference/team-workspace-works-desktop.png` | source for Work canvas, lane/card rhythm and global shell |
| `reference/team-activity-desktop.png` | source for typed activity density and Lead attention hierarchy |
| `reference/team-members-desktop.png` | source for portrait roster and capacity hierarchy |
| `reference/agent-conversation-desktop.png` | source for conversation dominance and three-region relationship |
| `reference/host-console-desktop.png` | source for Host decision density and protected controls |
| `reference/member-home-desktop.png` | source for durable Agent identity/profile hierarchy |
| `reference/team-workspace-mobile-family.png` | source for mobile priority, sheets and touch density |

`expected-v2/` contains generated revisions that absorb the new mental model.
They are design candidates until an explicit aesthetic review marks them
approved. `implemented/` contains browser output and is always bound to a
runtime SHA through the visual contract; it is never an expected image.

## 3. Seven-page visual structure mapping

The table names what stays, what changes for the current model, what is removed,
what must be added, and where released space goes. Deleting a rail without
reallocating space is forbidden.

| Page | Preserve from reference | Model-driven change | Remove | Required addition and spatial allocation |
| --- | --- | --- | --- | --- |
| Works | compact Team header, left-aligned tabs, filter band, four-column rhythm, portrait Work cards | exactly `Open / Active / Review / Closed`; `Assigned` is an Open subgroup; blocked/on-hold are conditions; accepted/cancelled/failed are Closed resolutions | permanent Mission/TeamRun/member/authority rail; inferred readiness; old status stack | released rail width goes to denser Work cards and evidence/owner/decision footers, not four stretched empty boxes; filters move directly under tabs; Work detail remains on-demand sheet |
| Activity | Lead attention priority, source glyphs, actor portrait, direction, related Work, time/state, continuous row rhythm | authored Message, WorkEvent, WorkDelivery, GateEvaluation, RuntimeCommand and provider-native source stay visually distinct; no transcript mirroring | permanent selected-conversation/Work/member/provenance rail; generic rounded card per event | released rail width goes to the activity summary/action column and readable lineage; Host pressure appears only from projected Host facts; composer is one compact disclosure |
| Members | mature portraits, flat capacity band, dense roster table | durable AgentMember, exact MemberRun/generation, native session, Workspace, capacity and organization state remain separate; Host is not a MemberRun row | selected-member permanent rail; generic member card grid; fake Host provider/model | released rail width goes to stronger identity and exact runtime columns; Conversation remains the clear row action; administrative profile opens separately |
| Agent Conversation | left people navigator, dominant center chat, conditional right decision context, sticky composer | Host Agent is an addressable Team identity; member rows bind exact MemberRun; Message, Work, delivery, runtime and native observation remain separate | three equal dashboard panels; filler context; fake thinking/transcript stream | center receives the majority of width and visual contrast; left stays about 280px; right is about 290px and exists only for bound Work/runtime/actions; on mobile left/right become sheets |
| Host Console | main operating column, narrow factual rail, Lead Inbox, TeamRun, Supervisor, runtimes and exact controls | server `allowed_actions`, disabled reasons, Supervisor/RuntimeCommand/NodeDaemon generations remain exact and separate | nested action-target cards, duplicated panel headers, decorative summary boxes | main column receives activity and controls; right rail contains only current decision context, placement and Mission facts; metrics become hairline strips and runtimes become rows |
| Member Home | strong portrait and durable profile identity; My Work, ready pool, inbox/runtime facts | this is a P1 durable AgentMember profile, not the primary chat; multiple/current/historical MemberRuns stay distinct | Inbox as the dominant center experience; draft report/editor without write projection; one-run identity conflation | identity and organization occupy the header; Work and execution facts use continuous sections; a prominent Open conversation action leads to Agent Conversation |
| Mobile family | single primary task, compact pressure, portrait identity, sheets | stacked phase sections; exact Activity sources; AgentMember/MemberRun/native session separation; Agent and Context sheets around a full-width chat | horizontal Kanban, hidden pressure, squeezed desktop rails, drag-only interaction | desktop rail information moves into explicit sheets or inline priority bands; primary task receives full width; composer remains reachable above app navigation at 390 and 320px |

## 4. Current visual defects against reference

### P0

1. The earlier acceptance object was wrong: browser checks proved existence,
   focus, actions and overflow, not composition or quality.
2. Works used a full-width three-way tab bar and an extra title/filter block,
   delaying the task and weakening the reference's compact rhythm. The dirty
   experiment begins correcting this but is not an accepted source.
3. Sparse Work fixtures are rendered as four tall bordered lanes with too
   little internal density. Removing the rail amplified dead space without a
   deliberate reallocation contract.
4. Conversation regions read too similarly. The center chat does not dominate
   strongly enough over navigator and context.
5. Host Console contains repeated `panel -> inner bordered target -> control`
   nesting, producing a generic administration template rather than a mature
   decision console.
6. No high-fidelity visual comparison can currently fail CI or a review. The
   Playwright suite saves PNGs but never evaluates visual parity.
7. The existing `implemented/` set belongs to the pre-rebaseline source and
   cannot prove expected-v2.

### P1

1. Page-family primitives do not constrain radius, border frequency, density,
   row rhythm or selection. Callers assemble long Tailwind strings independently.
2. Agent Team tabs duplicate a roving-focus implementation despite installed
   Radix Tabs and a shared `tabs.tsx` primitive.
3. Icon-only context and shell actions do not consistently use the installed
   Tooltip primitive.
4. Members and Activity are functionally dense but their section headers and
   table frames remain visually generic.
5. Small metadata is sometimes too faint or compressed relative to portraits
   and primary facts.
6. Expected/reference sizes are not yet normalized to final evidence viewports;
   exact-size overlays therefore require a governed normalized revision.

## 5. Surface hierarchy and panel consolidation

Only three surface levels are allowed:

1. **Page/section plane**: canvas, lane, navigator, context region. Usually
   typography, whitespace and hairlines; not automatically rounded.
2. **Interactive record**: selected Work, member target, actionable row or
   message composer. Radius 8-10px only when the boundary is meaningful.
3. **Sheet/overlay**: Work detail, Agent list, Context or protected confirmation.
   This is the only level that receives material shadow.

Static source inventory at rebaseline found 16 direct `agent-team-panel`
occurrences across the core page family: Host Console 9, TeamWorkspace 2,
TeamConversation 2, TeamMembersCapacity 2 and TeamWorksBoard 1. Host Console is
the primary card-soup source.

Planned deletions/merges:

- merge Host Console `Current TeamRun` card and metric card wall into one flat
  TeamRun section plus `MetricStrip`;
- replace nested `ActionTargetGroups` cards with target subheads and one divided
  action row;
- replace Supervisor and Member runtime outer cards with continuous sections;
- keep Host right-rail cards only for one current decision or placement fact;
- flatten Activity filter and stream frame into section hairlines plus
  `RecordRow`;
- keep Work cards because they are genuine interactive records, but do not wrap
  their metadata in additional cards;
- remove rounded containers around pure labels/statistics;
- keep empty/error states bounded only when the boundary explains recovery.

## 6. Existing libraries and actual usage

The current stack is sufficient: React 18, Tailwind CSS 4, Radix UI, CVA,
`clsx`, `tailwind-merge`, Lucide and Playwright. **Good libraries and reusable
atoms already exist; the missing layer is an aesthetic-constraining page-family
design system, high-level visual primitives and a real visual acceptance loop.
Changing libraries will not fix the design automatically.**

### Core pages currently use

- shared `Button` (CVA plus Radix Slot), `Badge`, `Avatar`, `Markdown`;
- RoleView `ViewState`, `AttentionStrip`, `ViewProvenance`;
- `RoleActionPanel` and semantic Team composers;
- Lucide icons;
- Tailwind utility strings and page-local CSS tokens.

### Radix/shared primitive reality

- Radix Tooltip is globally provided and used by the application shell, not
  systematically by Agent Team surfaces.
- Radix ScrollArea is used by the Workbench shell. Agent Team subregions mostly
  use native overflow ownership.
- Radix Tabs and a shared `components/ui/tabs.tsx` exist, but TeamWorkspace
  currently hand-writes roles, roving tabindex and arrow-key behavior.
- Radix Separator and Slot are already represented by shared UI primitives.
- Mobile Work/Agent/Context sheets are hand-written responsive overlays with
  tested Escape, focus trap and focus restoration; no Radix Dialog dependency
  is installed.

### Keep or migrate

| Interaction | Decision | Reason |
| --- | --- | --- |
| Team tabs | migrate to a controlled page-family wrapper over Radix Tabs | removes duplicated roving focus and lets one visual primitive own focus/selected/hover states |
| Mobile sheets | keep behavior, extract `AgentTeamSheet` | current desktop-inline/mobile-overlay transformation and focus evidence are valid; adding a new dialog dependency is unnecessary |
| Scroll ownership | keep native page/canvas overflow; reuse shell ScrollArea only at the shell | nested custom scrollbars would obscure the single-owner contract |
| Tooltips | reuse installed Tooltip only for icon-only/truncated controls | labelled controls should remain self-explanatory; no tooltip decoration |
| Motion | keep CSS transform/opacity | intensity is 3; a motion library would not solve hierarchy |

No animation library, chart library or new universal Card system will be added
for this work. A dependency may be proposed only for a named consistency,
accessibility, constraint or efficiency gap.

## 7. High-level page-family primitives

These primitives have limited variants and forbid arbitrary caller overrides:

| Primitive | Variants | Constraint |
| --- | --- | --- |
| `AgentTeamSection` | `plain`, `recessed`, `decision` | page grouping only; no nested section of the same kind |
| `AgentTeamRecordRow` | `message`, `work`, `delivery`, `gate`, `runtime`, `native` | shared height, icon/portrait scale, source column and divided-row rhythm |
| `AgentTeamMetricStrip` | `team`, `member`, `runtime` | typography plus hairline; never KPI cards |
| `AgentTeamTabs` | `workspace` | Radix-controlled; compact left desktop, equal-width mobile, short underline focus |
| `ConversationCanvas` | `member`, `host` | center dominance, continuous flow, sticky composer boundary |
| `DecisionContext` | `work`, `member-run`, `host` | conditional; absent when no current decision fact exists |
| `AgentTeamSelection` | semantic tokens for selected, focus, hover and pressed | selected wash, focus ring, hover and pressed are independent states |
| `AgentTeamSheet` | `work`, `agents`, `context`, `protected-action` | one focus/motion/scroll contract; shadow only here |

The primitives are not a universal Card API. Callers supply semantic data and
actions, not arbitrary radius/border/padding class strings.

## 8. Visual acceptance v2

### A. Deterministic product gate

Retain RoleView schema/version/auth scope, exact allowed actions and disabled
reasons, store-live action checks, native-session boundary, SSE refetch,
keyboard/focus, reduced motion, 320px overflow and full repository checks.

### B. Visual quality gate

For each P0 page, bind the following to one exact source SHA:

- immutable `reference/` path;
- approved `expected-v2/` path and normalized exact-viewport revision;
- browser `implemented/` path;
- 1440x1000, relevant 900x1180, 390x844 and 320x844 comparison;
- exact-size overlay plus side-by-side comparison;
- geometry assertions for first-task vertical landmark and region ratios;
- review of dead space, maximum surface depth, row rhythm, avatar/icon/type
  scale, center-chat dominance and focus shape;
- separate `product_truth` and `visual_fidelity` verdicts.

`page.screenshot()` is evidence capture only. It cannot return a visual pass.
Expected images may be revised only through an explicit design revision; they
must never be overwritten to make a difference green.

### Independent visual reviewer

The visual reviewer is independent of implementation and PM/operator reviews.
It must inspect the named reference, expected and implemented files and only
judge:

1. recognizable parity with the reference language;
2. absence of generic admin-template character;
3. absence of card soup or fourth-level surfaces;
4. stable visual focus and correct primary-area dominance;
5. first-viewport density and absence of accidental dead white regions;
6. consistent typography, portrait, icon, hairline, radius and accent quality;
7. responsive preservation of task priority.

Any visual P0/P1 keeps `visual_fidelity=revise_implementation` and prevents a
ready-for-review or merge claim.

## 9. Invalidated conclusions

The following earlier conclusions are explicitly invalid:

- the claim after `42b1ae73` / `6b08b83f` that complete gates and two reviews
  made the War Room ready;
- the visual-evidence conclusion attached to `1eeaf4b7`;
- PM/operator P0/P1=0 as a substitute for art-direction review;
- any inference that successful screenshot capture, DOM assertions, no
  overflow, keyboard navigation, a green `pnpm check`, or CI success proves
  design quality;
- any evidence whose implemented image SHA differs from the source under
  review;
- any use of the old concept's right rail when it repeats facts rather than
  serving a current decision.

Still valid: the RoleView authority boundary, Work phase/condition/resolution
model, Mission-Team identity, AgentMember/MemberRun/native-session separation,
authenticated actions, native transcript non-mirroring, responsive focus
behavior and deterministic engineering checks. Those must survive the visual
rebuild.


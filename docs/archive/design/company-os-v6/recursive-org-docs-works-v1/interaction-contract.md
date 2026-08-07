# Interaction contract

Binding rules for navigation, click, scroll, filter, loading, empty, and
error behavior on the five surfaces. Field-level truth lives in
[data-provenance.md](data-provenance.md); layouts live in
[wireframes.md](wireframes.md). Existing War Room / Member Focus behaviors
already asserted by `apps/agent-dashboard/tests/*-check.mjs` remain in force;
this contract adds recursive-Organization, global-Works, and handoff rules.

## Global context

- Every internal link preserves `company`, `space`, `project`, and `api`
  query parameters when present, plus the surface selection params
  (`surface`, `team`, `memberRun`, `workItem`, `document`, `mission`,
  `wave`) that identify the current record.
- A link must not switch from Store-live data to fixture fallback; the
  existing data-mode banner (`data-company-os-data-mode`) stays visible on
  Company OS pages.
- Loading, empty, failed, and unavailable states are visually distinct.
  Failed loads keep the last honest empty state and the silent retry already
  implemented by the shell; write actions stay gated on `source === "live"`.
- One execution driver: no surface may both activate a provider-native goal
  and issue an ordinary Harness start for the same Work.
- Deep links are first-class: every surface state named in
  [page-matrix.md](page-matrix.md) is reachable by URL alone.
- Scroll ownership: the page owns vertical scroll; inner regions (boards,
  trees, conversations) scroll independently only when their header/filter
  row stays pinned and the body never scrolls the page horizontally.

## Organization

**Navigation.** Primary nav `Organization` lands on the recursive tree rooted
at the root AgentTeam. Selecting a Team node opens that Team's shared Team
War Room; selecting a Member node opens the shared Member Focus; selecting a
Member's child-Team edge drills into that Team node in place (tree state is
in the URL so reload restores it).

**Click.** Node rows have one primary action (open) and one disclosure
(expand/collapse children). Expand/collapse never navigates. A node with a
topology integrity finding (cycle, missing parent, Host not in parent)
shows the finding badge and still opens — integrity is reported, never
auto-repaired or hidden.

**Scroll.** The tree column scrolls independently at desktop/tablet; the
detail column follows the selected node. On mobile the tree is a drill-down
list (one level per screen) with a breadcrumb back path, per IA mobile
policy.

**Filter.** Filters: text match on Team/Member name, durable status, runtime
state, and "has unassigned Work". Filters never reorder the tree — they
dim non-matching nodes and keep ancestors visible for context.

**Loading/empty/error.**

- Loading: skeleton tree rows; counts never show placeholder numbers.
- Empty company (no root Team yet): an honest empty state explaining that the
  recursive Organization appears after the first AgentTeam exists — no
  fixture tree.
- Empty node (Team with no direct Members or no child Teams): the node
  renders with explicit "No direct Members" / "No child Teams" labels.
- Error: snapshot unavailable → the existing shell empty/error treatment;
  a topology integrity finding → in-page banner listing exact findings.

**Honesty.** Durable Team/Member status and runtime state are separate,
labelled facts (see data-provenance §1). Per-node Work counts (assigned,
in progress, blocked, review) derive from Work rows and are labelled "current
TeamRun scope" until `Work.team_id` lands. Org-change actions (create Team,
create Member, move Host) remain visibly unavailable until the governed
transport exists, mirroring the existing disabled `New work` precedent.

## Global Works

**Navigation.** Secondary nav `Work` keeps the Company WorkItem views. The
recursive Team Work aggregate is a distinct view within Work (working name:
`Team Works`) so the two kernels are never blended into one list
(data-provenance §6.1-6.4). A Work row opens its owning Team War Room Works
tab with the row selected; a delegated row additionally shows the child
TeamRun link from `WorkDelegation`.

**Click.** Row click opens the Work in its owning Team context. The
unassigned queue offers Take (self) and Delegate (direct child only when the
viewer Hosts one); both record through the existing Work operations — never
through chat.

**Scroll.** Filter bar pinned; the list owns scroll. Board-style grouping
(by status) may scroll horizontally on desktop only; at ≤900px groups stack
vertically.

**Filter.** Team path, Host, Member, status, source, and milestone
(`design.md:286-290`). The four demand classes (discovered-unassigned,
self-owned, delegated, follow-up) are a first-class grouping, not a hidden
predicate; each row shows the source observation that created it
(`source_work_item_ref` today; `source_refs[]` at target). Filters compose
(AND) and reflect in the URL.

**Loading/empty/error.**

- Loading: skeleton rows; the unassigned queue count badge stays empty until
  data arrives.
- Empty: distinct copy for "no Team Work anywhere" vs "no rows match these
  filters" (with a reset affordance), matching the existing conversation
  "Reset filters" pattern.
- Error: failed snapshot → shell treatment; a Work whose owning TeamRun is
  absent from the snapshot renders as `unavailable` with its id, never
  dropped silently.

**Honesty.** Submission state uses the store wording: `review` rows are
"Awaiting Host acceptance". A provider `completed` runtime status is never
rendered as Work acceptance (see never-render list).

## Member Focus

**Navigation.** Opened from Organization Member nodes, War Room Members tab,
and Global Works rows (assignee). The existing `?surface=team&team=<id>&
memberRun=<id>` route remains; Organization adds a durable-AgentMember entry
point once `agent_member_id` resolution is available in the snapshot —
through the explicit link only.

**Click/scroll/filter.** Unchanged from the accepted Member Focus contract:
hero header, goal panel, Work queue, context rail/sheet, composer with
explicit disabled reasons. Added sections — created Work, child Work, child
Team and direct Members — follow the same list semantics as the existing
Work queue (click row → owning context).

**Loading/empty/error.** Unchanged patterns: `Loading member history…`,
`MemberRunNotFound`, native-activity `idle/loading/ready/unavailable`
(`data-native-activity-state`). New sections reuse the same vocabulary:
"No created Work", "No child Work", "No child Team" — never fabricated rows.

**Actions (TARGET as Member-initiated).** Create unassigned Work, take own
Work, split Work, delegate to direct child (`design.md:300`). Each action is
hidden — not merely disabled — when its precondition is absent (no direct
child Team ⇒ no delegate action). Until Member-initiated transports land, the
Host-initiated flows keep working and these controls read as unavailable with
the reason.

## Team War Room

**Navigation.** Organization adds a breadcrumb above the War Room header
(Team path from the recursive chain) and, when child Teams exist, a
child-Team row that drills into each child's own War Room. Everything else
keeps the accepted composition: Works (default) | Activity | Members tabs,
mailboxes, composer, context rail.

**Click/scroll/filter.** Existing contracts stand: lane semantics, "Reset
filters", message/Work separation ("Send message" and "Create Work from
message" stay separate actions with `causation_ref` recorded), composer
collapse below `sm`, sheet-style context on small screens.

**Loading/empty/error.** Existing patterns stand: not-found EmptyState with
back button, lane placeholders, terminal-run integrity anomaly banner. Added:
when the recursive topology is unavailable (TARGET fields not yet in the
snapshot), the breadcrumb renders only the current Team name — it must not
fabricate ancestors.

## Docs-to-Work handoff

**Navigation.** A Document page opens its created WorkItems (implemented),
its Related Works block (implemented, derived backlink), and — at TARGET —
Create Work from selection. Work pages link back to the source Document
(`WorkItem.source_document_ref`; `workItemHref`/`documentHref` context
preservation is the existing pattern).

**Click.**

- `Create Work from selection` (TARGET): enabled only with a Document/Block
  selection and an available placement; opens a modal that prefills title
  and context from the selection, records the source ref, and offers only
  the three legal placements. It never infers an owner (mirrors the existing
  create-from-message modal rules).
- `Link existing Work` (TARGET): search-and-attach against existing Work;
  writes an explicit relation, never a chat mention.
- Result return: an explicit result/update action records
  `result_document_ref` (IMPLEMENTED) — at RESEARCH fidelity, the exact
  `DocumentRevision`. A Document revision never completes Work; Work `done`
  never approves the document.

**Loading/empty/error.**

- Related Works loading: skeleton within the block; empty: "No linked Work"
  with the handoff action beside it; error: the block shows
  `unavailable` — the document body itself never blocks on Work data.
- The existing disabled `New work` pattern (no governed transport) is the
  model for every handoff action whose transport has not landed.

**Honesty.** Handoff renders the `WorkExecutionChain.link_status` vocabulary
(`linked | mismatch | unavailable`) verbatim; a `mismatch` is surfaced as an
integrity finding, not hidden.

## Motion

- 120–180 ms opacity/color/translate transitions for hover, expansion, and
  delivery acknowledgement.
- Respect `prefers-reduced-motion`.
- Do not animate lifecycle state optimistically before Store acknowledgement.

## Responsive rules (all five surfaces)

- Baselines: desktop 1440×1000, tablet 900×1180, mobile 390×844, plus the
  320×720 overflow guard. No horizontal page overflow at any baseline.
- Tablet: primary sidebar collapses to the compact rail; Context Rail moves
  behind the labelled `Context & controls` disclosure/sheet; wide relation
  tables become compact rows with an explicit detail route.
- Mobile: one primary column; title, status, accountable actor, and the next
  required action stay in the first viewport; trees and database views become
  drill-down lists; bottom sheets/full-screen routes carry context and safe
  actions; touch targets ≥44px.
- 320×720: identical rules to 390 with the guard that nothing clips or
  overlaps; composer and filter bars may wrap but never overflow.

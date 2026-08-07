# Expected vs actual

Gap map between this contract (Expected) and the dashboard as committed
(Actual), with the acceptance that closes each gap. Wave 0 changes no
production component; this file is the implementation seam freeze for later
Waves. Component paths are relative to `apps/agent-dashboard/`.

## 1. Organization (recursive AgentTeam tree)

- **Expected**: wireframes §1 — recursive tree from explicit
  `parent_team_id` / `host_member_id` / `member_ids`, durable vs runtime
  badges, per-node Work counts, drilldown Member → child Team, integrity
  findings, unavailable org-change actions.
- **Actual**: `src/company-os/operations/pages.tsx` `OrganizationPage` +
  `OrganizationUnitBranch` render a recursive **OrgUnit** tree from the
  Company Store (`parent_unit_id`), with membership/actor cards and integrity
  findings. There is **no recursive AgentTeam UI**: `src/types.ts`
  `AgentTeam` carries `member_ids` only; grep finds no nested-team code in
  `src/`.
- **Gap**: the recursive AgentTeam tree does not exist in snapshot types,
  selectors, or components. The OrgUnit tree is a business grouping and must
  not be repurposed as the agent scheduling hierarchy
  (`docs/company-os/organization-and-actors.md`).
- **Acceptance**: new Organization view renders the tree from store rows with
  durable/runtime badge separation (probe: `data-org-team-depth`,
  `data-durable-status`, `data-runtime-state` attributes); integrity finding
  fixture renders the banner; empty store renders the honest empty state;
  geometry probes pass at 1440×1000 / 900×1180 / 390×844 / 320×720.

## 2. Global Works

- **Expected**: wireframes §2 — one aggregate over the recursive tree; four
  demand classes; filters Team path/Host/Member/status/source/milestone;
  unassigned first-class queue; row → owning War Room.
- **Actual**: `src/company-os/work/WorkOperatingPage.tsx` is a global board
  over the `company_os.work` aggregate (Company WorkItems). Team `Work` rows
  are read only in `src/model/teamSelectors.ts` (single TeamRun scope,
  `selectFilteredTeamWorks` at ~line 274) and in `src/surfaces/Missions.tsx`
  (~line 383, pressure roll-up only). **No cross-TeamRun Works view exists.**
- **Gap**: no aggregate selector, no view, no route; WorkItem and Team Work
  kernels must stay visibly distinct (data-provenance §6.1-6.4).
- **Acceptance**: a `Team Works` view within `?surface=work` lists Team Work
  rows across runs with the four demand classes and pinned filters; rows show
  source observation and owning Team path; unassigned queue is first-class;
  empty vs no-match copy differs; geometry probes pass at all four
  viewports; no WorkItem row and no Team Work row ever share one list.

## 3. Member Focus

- **Expected**: wireframes §3 — accepted Member Focus plus created Work,
  child Work, child Team + direct Members, Inbox/Outbox, and durable
  AgentMember identity via explicit `agent_member_id`; TARGET actions hidden
  without preconditions.
- **Actual**: `src/surfaces/MemberRuns.tsx` (`MemberRunFocus`, hero, goal
  panel, `MemberWorkQueue`, `MemberContextRail`, `MemberComposer`) +
  `components/workbench/member/MemberHistoryNarrative.tsx` +
  `components/workbench/layout/FocusShell.tsx`. Already renders runtime,
  workspace, provider, native-session facts and owned Work; org link resolves
  `agent_member_id` → Company OS actor.
- **Gap**: additive sections (created Work via `created_by_actor`, child Work
  via `parent_work_id`) are selector work; child Team section is TARGET-blocked
  (no `host_member_id` in snapshot); Member-initiated actions are
  transport-blocked.
- **Acceptance**: new sections render only when backed by store rows and use
  the existing empty vocabulary; child-Team slot renders nothing (not a
  placeholder claim) until TARGET fields land; existing member-focus mobile
  assertions keep passing unchanged.

## 4. Team War Room

- **Expected**: wireframes §4 — existing composition plus Organization
  breadcrumb and child-Team row; breadcrumb never fabricates ancestors.
- **Actual**: `src/surfaces/TeamWarRoom.tsx` (composition layer) +
  `components/workbench/team/{TeamWorksBoard,TeamConversation,TeamMailboxes,
  TeamMembersCapacity,TeamCapacityStrip,teamActivityItems,teamFormat}.tsx` +
  `components/workbench/layout/FocusShell.tsx` +
  `components/workbench/context/ContextRail`. Tabs Works (default) |
  Activity | Members; truthful capacity; integrity anomaly banner.
- **Gap**: breadcrumb and child-Team row are new but small; everything else
  is reuse. Source-assertion tests (`workbench-visual-fixture-check.mjs` via
  `read-war-room-source.mjs`) pin strings here and must be extended, not
  broken.
- **Acceptance**: breadcrumb shows only proven ancestors (pre-freeze: current
  Team name alone); child-Team row renders iff a child Team row exists in the
  snapshot; all existing war-room checks keep passing.

## 5. Docs-to-Work handoff

- **Expected**: wireframes §5 — Create Work from selection, Link existing
  Work, Related Works derived backlink, explicit result return with
  `result_document_ref`, `link_status` vocabulary, three legal placements.
- **Actual**: `src/company-os/docs/BasicDocumentPage.tsx` renders
  `RelatedWorkBlock` (`data-docs-related-work`, deduped from WorkItem
  source/context/result refs); `workItemHref`/`documentHref` preserve
  context. **No UI action creates Work from a Document**: `New work` is
  disabled ("No governed WorkItem creation transport is connected");
  `docs/documentAction.ts` builds Docs-object commands only.
- **Gap**: handoff actions are transport-blocked (Company side) and
  TARGET-blocked (Team side `source_refs[]`). The contract freezes the slots
  and the modal rules so the transport lands without redesign.
- **Acceptance**: selection actions render per wireframe §5a/§5b only when a
  selection and a legal placement exist; the modal never infers an owner and
  records the source ref; `link_status` renders verbatim incl. `mismatch` as
  an integrity finding; until transports land, actions follow the disabled
  `New work` precedent with the reason visible.

## Production components likely to change

Exact seam freeze (later Waves edit these, nothing else visual):

- `src/app/selection.ts` — new view/route params (Team Works view, org tree
  node, handoff deep links).
- `src/app/WorkbenchShell.tsx` — nav entries and `SurfaceSwitch` cases.
- `src/types.ts` — recursive Team fields, aggregate Works wire additions.
- `src/api.ts` / `src/api/actions.ts` — recursive-org reads and handoff
  action transports when they exist.
- `src/model/teamSelectors.ts` — cross-TeamRun Works selectors; recursive
  tree selectors (today strictly single-run).
- `src/model/readModel.ts` — surface the new aggregates.
- `src/surfaces/MemberRuns.tsx` — the additive sections above.
- `src/surfaces/TeamWarRoom.tsx` — breadcrumb + child-Team row only.
- `src/components/workbench/layout/FocusShell.tsx` — reuse as-is; touch only
  if the new sections need a layout affordance it lacks.
- `src/company-os/operations/pages.tsx` — Organization gains the recursive
  AgentTeam view beside (not instead of) the OrgUnit tree.
- `src/company-os/work/WorkOperatingPage.tsx` — gains the distinct
  `Team Works` view.
- `src/company-os/docs/BasicDocumentPage.tsx` + `docs/documentAction.ts` —
  handoff actions.
- Fixtures `fixtures/workbench-layout-v2-native-v1/*` +
  `fixture-manifest.json` — new routes/ledgers for checks and captures.
- Tests `tests/*-check.mjs` + root `package.json` `check:dashboard` chain —
  new sibling checks per page-matrix.

## Non-goals (explicit no-touch list)

- Workflows, Missions canvas, Approvals, Finance, Providers/Plugins/Settings,
  Debug surfaces.
- The OrgUnit tree semantics, Company WorkItem views, and their data mode
  banner ladder.
- Visual tokens (`src/index.css`), `member-focus-theme`, the
  `company-workbench` editorial layer, Geist fonts, portrait generation.
- Any Assignment-Message UI (no such contract), any `% utilization` widget,
  any fixture-fallback claim.

## What acceptance must never let through

- Ancestry, responsibility, availability, or runtime health inferred from
  names, provider sessions, document authorship, or first-row fallback.
- A provider `completed` status rendered as Work acceptance.
- A runtime badge presented as organizational authority; an AgentTeamRun
  presented as a department.
- A WorkItem row and a Team Work row blended into one list; a chat bubble
  presented as a Work row; a child Team presented as a native subagent group.

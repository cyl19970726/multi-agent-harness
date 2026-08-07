# Wireframes — Wave 0 Expected (text)

These ASCII frames are the Wave 0 Expected assets referenced by
`visual-contract.json`. They pin layout structure, labels, and state
placement — not pixels. Approved Expected PNGs replace them per
`docs/design/company-os-v2/visual-index.md` before broad styling changes.

Conventions: `[...]` action/button, `(•)` disclosure, `▸/▾` tree state,
`⟦ ⟧` badge/chip, `—` hairline. Badges are labelled facts; see
data-provenance for the class of every value. `T:` marks TARGET-class
elements that stay hidden until the store/API lands.

Shared shell (all surfaces): AppRail left (full `xl+`, icon rail `sm..xl`,
bottom tab bar `<sm`), TopBar with project/space/company pickers, freshness,
and error banner.

---

## 1. Organization

### 1a. Desktop 1440×1000

```text
┌──────┬────────────────────────────────────────────────────────────────────┐
│      │ TopBar: Company · Space · Project            fresh 12s · ⟦error?⟧  │
│ Rail ├────────────────────────────────────────────────────────────────────┤
│      │ Organization                                    [filter: name ▾]   │
│      │ ⟦integrity findings: 0⟧                       status ▾ runtime ▾  │
│      ├───────────────────────────────────┬────────────────────────────────┤
│ Home │ Team tree (independent scroll)    │ Selected node detail           │
│ Docs │ ▾ ◆ Root Team — "Company Lead"    │ ◆ Root Team                    │
│ Org* │   ⟦active⟧ ⟦runtime: 1 running⟧   │ durable status: active         │
│      │   Work: 3 assigned · 2 in prog ·  │ purpose (T)                    │
│ Work │     0 blocked · 1 review          │ Host: ▣ Lead Agent ──→ Member  │
│ Appr │   ├─ ▣ Lead Agent (Host)          │   Focus                        │
│ Fin  │   │   ⟦active⟧ ⟦rt: running⟧     │ Direct Members (3):            │
│      │   │   ▸ ◆ child: "Research" (T)   │  ▣ Lead Agent ⟦active⟧⟦rt:run⟧ │
│ Miss │   ├─ ▣ Docs Member                │  ▣ Docs Member ⟦active⟧⟦rt:…⟧  │
│ Wkfl │   │   ⟦active⟧ ⟦rt: idle⟧        │  ▣ UX Member  ⟦paused⟧⟦rt:—⟧  │
│ Teams│   ├─ ▣ UX Member                  │ Child Teams (T):               │
│      │   │   ⟦paused⟧ ⟦rt: —⟧           │  ◆ Research — Host: Lead Agent │
│      │   ▸ ◆ child: "Research" (T)       │ Work counts: 3 assigned ·      │
│      │     ⟦active⟧ Work: 1 · 0 · 0 · 0  │  2 in progress · 0 blocked ·   │
│      │                                   │  1 review ⟦current TeamRun     │
│      │                                   │  scope⟧                        │
│      │                                   │ [Open Team War Room]           │
│      │                                   │ [Create child Team] ⟦unavail-  │
│      │                                   │  able: governed transport⟧     │
└──────┴───────────────────────────────────┴────────────────────────────────┘
```

- Solid edges only: explicit `parent_team_id` / `host_member_id` /
  `member_ids`. No dashed inferred relations.
- Durable badge (`active`/`paused`/`archived`) and runtime badge
  (`rt: running|idle|—`) are visually distinct and labelled.
- Work counts derive from Work rows; until `Work.team_id` lands they carry
  the ⟦current TeamRun scope⟧ label.

### 1b. Tablet 900×1180

```text
┌────┬───────────────────────────────────────────────────────────────┐
│Ico │ TopBar (pickers hidden · ⟦error?⟧)                            │
│rail├───────────────────────────────────────────────────────────────┤
│    │ Organization              [filter] [Context & controls]        │
│    ├───────────────────────────────────────────────────────────────┤
│    │ ▾ ◆ Root Team — "Company Lead"   ⟦active⟧ ⟦rt: 1 running⟧     │
│    │   Work: 3 · 2 · 0 · 1                                          │
│    │   ├─ ▣ Lead Agent (Host) ⟦active⟧⟦rt:run⟧  ▸ ◆ Research (T)   │
│    │   ├─ ▣ Docs Member       ⟦active⟧⟦rt:idle⟧                    │
│    │   └─ ▣ UX Member         ⟦paused⟧⟦rt:—⟧                       │
│    │ ───────────────────────────────────────────────────────────── │
│    │ Selected: ◆ Root Team — detail renders BELOW the tree;         │
│    │ Context Rail content lives behind [Context & controls] sheet   │
└────┴───────────────────────────────────────────────────────────────┘
```

### 1c. Mobile 390×844 — drill-down list

```text
┌──────────────────────────────────┐
│ Organization          [filter]   │
│ ⟦integrity: 0⟧                   │
│ ──────────────────────────────── │
│ ◆ Root Team — "Company Lead"     │
│ ⟦active⟧ ⟦rt: 1 running⟧         │
│ Work 3·2·0·1              ▸      │
│ ──────────────────────────────── │
│ (tap → drill one level)          │
├──────────────────────────────────┤
│ › Root Team › … (breadcrumb)     │
│ ▣ Lead Agent (Host) ⟦rt:run⟧ ▸   │
│ ▣ Docs Member ⟦rt:idle⟧      ▸   │
│ ▣ UX Member ⟦paused⟧         ▸   │
│ ◆ Research (child, T)        ▸   │
│ ──────────────────────────────── │
│ [Open Team War Room] (44px min)  │
├──────────────────────────────────┤
│ Home Docs Org Work  More (tabbar)│
└──────────────────────────────────┘
```

### 1d. Small 320×720 — guard

Same drill-down as 1c with: name column truncates (`line-clamp-1`), badges
wrap to a second line instead of overflowing, breadcrumb collapses to
`› … › Root Team`, no horizontal overflow anywhere.

---

## 2. Global Works (view within Work)

### 2a. Desktop 1440×1000

```text
┌──────┬────────────────────────────────────────────────────────────────────┐
│ Rail │ Work  ⟦WorkItems⟧ ⟦Team Works⟧   ← view switch; kernels never mix  │
│      ├────────────────────────────────────────────────────────────────────┤
│      │ Team Works (pinned filter bar)                                     │
│      │ Team path ▾ · Host ▾ · Member ▾ · status ▾ · source ▾ · milestone▾ │
│      │ Demand: ⟦unassigned 4⟧ ⟦self-owned⟧ ⟦delegated⟧ ⟦follow-up⟧       │
│      ├────────────────────────────────────────────────────────────────────┤
│      │ UNASSIGNED (first-class queue)                                     │
│      │ ┌──────────────────────────────────────────────────────────────┐   │
│      │ │ ◇ Map PR302 contract → UI seams                              │   │
│      │ │ Root Team · src: operator intake · high · v1                 │   │
│      │ │ [Take (self)] [Delegate ▸] — direct child only               │   │
│      │ └──────────────────────────────────────────────────────────────┘   │
│      │ SELF-OWNED / DELEGATED / FOLLOW-UP (grouped, lineage visible)      │
│      │ ┌──────────────────────────────────────────────────────────────┐   │
│      │ │ ◆ UX contract v1            ⟦review: Awaiting Host accept.⟧  │   │
│      │ │ Root Team › CompanyOSUXBuilder · parent: — · src: Wave 0     │   │
│      │ │   └─ ◆ follow-up: register slice in registry  ⟦open⟧         │   │
│      │ │ ◆ Core topology slice       ⟦in_progress⟧ delegated ↓        │   │
│      │ │ Root Team › CoreKernelBuilder → child run p58829-1           │   │
│      │ └──────────────────────────────────────────────────────────────┘   │
│      │ (list owns scroll; grouping by status optional; row → owning War   │
│      │  Room Works tab with row selected)                                 │
└──────┴────────────────────────────────────────────────────────────────────┘
```

Every row shows: title, owning Team path, responsible Member (or the
unassigned glyph ◇), status in store wording, source observation, version.
Delegated rows show the `WorkDelegation` child link; follow-up rows indent
under their parent Work.

### 2b. Tablet 900×1180

Filter bar wraps to two rows, stays pinned; groups stack vertically (no
horizontal board scroll); row cards keep Team path + responsible Member +
status; secondary facts (source, version) move to the row's detail route.

### 2c. Mobile 390×844

```text
┌──────────────────────────────────┐
│ Work › Team Works      [filter]  │
│ ⟦unassigned 4⟧⟦self⟧⟦del⟧⟦fup⟧   │
│ ──────────────────────────────── │
│ UNASSIGNED                       │
│ ┌──────────────────────────────┐ │
│ │ ◇ Map PR302 → UI seams       │ │
│ │ Root Team · high             │ │
│ │ [Take] [Delegate ▸]          │ │
│ └──────────────────────────────┘ │
│ ┌──────────────────────────────┐ │
│ │ ◆ UX contract v1             │ │
│ │ ⟦Awaiting Host acceptance⟧   │ │
│ │ Root Team › UXBuilder        │ │
│ └──────────────────────────────┘ │
│ (filter → bottom sheet; groups   │
│  stack; first viewport keeps     │
│  demand chips + first queue row) │
├──────────────────────────────────┤
│ Home Docs Org Work  More         │
└──────────────────────────────────┘
```

### 2d. Small 320×720 — guard

Demand chips horizontally scrollable (snap-x, existing mailbox-strip idiom)
or wrap; queue cards keep title + status + one action per row
(`[Take]` first, `Delegate` inside the row detail route); no overflow.

---

## 3. Member Focus (reuse + extensions)

### 3a. Desktop 1440×1000 — delta on the accepted Member Focus

```text
┌──────┬────────────────────────────────────────────────────────────────────┐
│ Rail │ › Root Team › ▣ CompanyOSUXBuilder          (breadcrumb, T)        │
│      ├────────────────────────────────────────────────────────────────────┤
│      │ HERO (existing): portrait · name/role · durable ⟦active⟧ ·         │
│      │ runtime ⟦running⟧ · provider/capacity chips · controls             │
│      ├──────────────────────────────────────┬─────────────────────────────┤
│      │ Goal panel (existing)                │ Context Rail (existing):    │
│      │ Current owned Work + criteria        │ runtime · workspace ·       │
│      │ ───────────────────────────────────  │ provider · native session   │
│      │ Work queue (existing)                │ ─────────────────────────── │
│      │  ◆ UX contract v1 ⟦review⟧           │ NEW — Durable identity:     │
│      │ NEW — Created Work (2)               │ AgentMember via explicit    │
│      │  ◇ registry follow-up ⟦open⟧         │ agent_member_id link only   │
│      │ NEW — Child Work (1)                 │ NEW — Child Team (T):       │
│      │  ◆ … ⟦blocked⟧                       │ ◆ Research — [Open War Room]│
│      │ NEW — Inbox/Outbox (Work-linked)     │ NEW — Direct Members (T)    │
│      ├──────────────────────────────────────┴─────────────────────────────┤
│      │ Composer (existing): explicit disabled reasons; [Create unassigned]│
│      │ [Take own Work] [Split Work] [Delegate to child] — hidden when     │
│      │ precondition absent (T transports)                                 │
└──────┴────────────────────────────────────────────────────────────────────┘
```

Existing sections and their contracts are unchanged; NEW rows list exactly
the sections `design.md:291-300` adds. New sections reuse the existing
list/empty vocabulary.

### 3b. Mobile 390×844

Hero compact (existing `member-focus-theme` behavior), controls scroll-x,
context behind sheet; NEW sections stack after the Work queue in document
order: Created Work → Child Work → Child Team → Inbox/Outbox; first viewport
keeps name, durable + runtime status, current Work, next action. 320×720:
identical with composer textarea ≥80% width (existing assertion) and wrapped
action buttons.

---

## 4. Team War Room (reuse + Organization context)

### 4a. Desktop 1440×1000 — delta on the accepted War Room

```text
┌──────┬────────────────────────────────────────────────────────────────────┐
│ Rail │ NEW breadcrumb: Organization › ◆ Root Team        (T: full path)   │
│      │ Header (existing): run status · mission/wave · members · pressure  │
│      │ NEW child-Team row (T): ◆ Research — 1 running · [Open War Room]   │
│      ├────────────────────────────────────────────────────────────────────┤
│      │ Tabs (existing): ⟦Works (default)⟧ Activity Members                │
│      │ Works lanes (existing 5-lane grid):                                │
│      │  Unassigned │ Assigned │ In progress │ Blocked │ Review            │
│      │  ┌────────┐ ┌────────┐ ┌──────────┐ ┌───────┐ ┌────────────────┐  │
│      │  │◇ card  │ │◆ card  │ │◆ card    │ │◆ card │ │◆ UX contract   │  │
│      │  │        │ │        │ │          │ │       │ │ Awaiting Host… │  │
│      │  └────────┘ └────────┘ └──────────┘ └───────┘ └────────────────┘  │
│      │ Mailbox strip (existing) · Composer (existing)                     │
│      │ Context Rail (existing): supervisor lease · pending interactions · │
│      │ truthful capacity ⟦available|limited|exhausted|unauthorized|unkn⟧  │
└──────┴────────────────────────────────────────────────────────────────────┘
```

Breadcrumb renders only what the store proves: before `parent_team_id` lands,
it shows the current Team name alone — never fabricated ancestors. All other
War Room behavior is the existing contract (tabs, lanes, mailboxes, composer,
integrity banner).

### 4b. Mobile 390×844 / 320×720

Breadcrumb collapses to `‹ Org › Root Team`; child-Team row becomes a card
above the tab bar; lanes stack (existing behavior); composer collapses to
"Message team" (existing). No horizontal overflow; 320 guard identical.

---

## 5. Docs-to-Work handoff

### 5a. Desktop 1440×1000 — Document page with handoff

```text
┌──────┬────────────────────────────────────────────────────────────────────┐
│ Rail │ Docs › Space › ◇ "Company OS operating plan"   ⟦active⟧            │
│      ├──────────────────────────────────────────────┬─────────────────────┤
│      │ Document body (existing block composer)      │ Context Rail:       │
│      │  ▤ heading / rich text / …                   │ properties: creator,│
│      │  ▣ block selected                            │ maintainer, owner   │
│      │  ⟦selection actions⟧                         │ ──────────────────  │
│      │   [Create Work from selection] (T)           │ Related Works       │
│      │   [Link existing Work] (T)                   │ (existing block,    │
│      │ ──────────────────────────────────────────── │ derived backlink):  │
│      │ RELATED WORK (existing RelatedWorkBlock)     │ ◆ WorkItem "…"      │
│      │  ◆ WorkItem — originates_from this Document  │   ⟦in_progress⟧     │
│      │    ⟦in_progress⟧ · owner · team · src role   │   chain: ⟦linked⟧   │
│      │  chain: ⟦linked | mismatch | unavailable⟧    │ [Create Work…] (T)  │
│      │ Result return: [Record result document] —    │ empty: "No linked   │
│      │  explicit action; records result_document_   │ Work" + action      │
│      │  ref (exact revision at RESEARCH fidelity)   │                     │
└──────┴──────────────────────────────────────────────┴─────────────────────┘
```

### 5b. Create-Work-from-selection modal (TARGET)

```text
┌──────────────── Create Work ────────────────┐
│ Source: ◇ "…operating plan" › ▣ block 12    │
│ (recorded as source ref; exact revision T)  │
│ Title: [ prefilled from selection ]         │
│ Context: [ prefilled markdown ]             │
│ Placement (only legal options render):      │
│  (•) Unassigned in Root Team                │
│  ( ) Self — ▣ CompanyOSUXBuilder            │
│  ( ) Child Team ◆ Research (you are Host)   │
│ Owner: never inferred — unassigned default  │
│              [Cancel] [Create Work]         │
└─────────────────────────────────────────────┘
```

### 5c. Mobile 390×844 / 320×720

Selection actions enter a bottom sheet; Related Works block renders inline
after the document body (tree/context asides already hidden `<lg`); modal
becomes a full-screen route with 44px actions; placement list keeps only
legal options. No overflow at 320.

---

## State appendix (applies to every frame)

```text
loading:     skeleton rows/blocks; no placeholder numbers or fake counts
empty:       explicit copy per region ("No child Teams", "No linked Work",
             "No Works match these filters" + [Reset filters])
failed:      shell error banner + last honest empty state; writes gated on live
unavailable: element labelled "unavailable" with its id; never silently dropped
integrity:   findings banner/badge; reported verbatim; never auto-repaired
```

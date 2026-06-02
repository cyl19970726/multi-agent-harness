# Agents — Multica-style Layout (Synthesized Design)

## 0. Decision summary

**Chosen overall structure: Variant 2 (Pragmatic minimal-tabs) as the spine, with grafts from Variant 3 (chat-first full-height shell) and Variant 1 (Multica list vocabulary + the named config blocks).**

Why this spine wins against the owner intent ("list like Multica; detail = left config + right tabs; DEFAULT tab = conversation + live current-work, reusing #60's chat"):

- **It honors "no empty tabs."** Variant 1 ships 3 of its 7 tabs (环境变量 / 自定义参数 / MCP) as scaffolds with `EmptyState`, because `AgentMember.provider_config` is documented-but-not-modeled in `types.ts` (confirmed: `AgentMember` at `types.ts:83-109` has no `provider_config` field). Shipping three dead tabs an operator clicks into and finds blank is a worse first run than folding them into one honest **Config** tab where each unbacked concept costs one collapsed row reading "Not configured."
- **It keeps the tab set small and every pane backed.** Three tabs — **Conversation (default) · Tasks · Config** — each has real data on day one.
- **It satisfies "DEFAULT = conversation + live current-work" exactly**, reusing `ConversationStream` (`Surfaces.tsx:2766`), `ChatBubble` (`2837`), `TurnDrillIn` (`2928`), and `Composer` (`3067`) verbatim.

What we **graft from Variant 3 (chat-first)**:
- The **full-height two-pane shell** (`AgentDetailShell`) replacing the centered `DocumentSurface` for the *detail* page only. The chat needs viewport height with a pinned composer; an 800px centered column wastes the screen. `WorkbenchShell` already suppresses the global Inspector for `surface==="agents"` (`WorkbenchShell.tsx:119`), so the Agents area is already a layout island — no global-layout fight.
- The **persistent "Now" current-work banner placed in the pane chrome above the tab bar**, so "what is it doing right now" is visible from every tab, not just Conversation. This is the live-indicator the owner asked for.

What we **graft from Variant 1 (Multica fidelity)**:
- The **list row vocabulary**: status filter chips, Workload, 7-day sparkline, Runs.
- Inside the **Config tab**, we keep Multica's **named section headers** (指令 / Skills / Runtime / 环境变量 / 自定义参数 / MCP) as the labels of collapsible blocks, so the Multica vocabulary stays scannable even though it is one tab.

What we **reject**:
- Variant 1's full 7-tab 1:1 mirror (3 unbacked tabs).
- Variant 3's extra **Now / Runtime / Raw turns** tabs — Now is folded into the banner; Runtime is a Config block (it is "where/how it runs" config-adjacent plumbing, kept out of the chat per the explicit #60 intent that runtime/session/evidence must NOT clutter the chat); Raw turns stay reachable per-reply via the existing `TurnDrillIn` and from the banner.

**Justification of the tab set (3 tabs):**
| Tab | Backed by | Rationale |
|---|---|---|
| **Conversation** (default) | `selectedMemberTimeline` (kind=message), `provider_sessions` for drill-in | Owner-locked default; the whole reason to open an agent — message it + watch raw turns. |
| **Tasks** | `Task.assignee_agent_id`, `Task.reviewer_agent_id`, `current_task_id` | Fills the audited gap (today only a single `current_task_id` shows). Backed, zero schema change. |
| **Config** (folds 指令/Skills/Runtime/Env/Params/MCP) | `prompt_ref`, `skill_refs`, `runtime_health`, sessions, child threads; `provider_config` (future) | One reconfigure surface. Backed concepts open by default; unbacked ones are one collapsed "Not configured" row, never a dead tab. |

The **Now banner is pane chrome, not a tab** — it satisfies the "live current-work" requirement without spending a tab on data we have least of in pre-aggregated form.

---

## 1. Data mapping (what feeds each surface, all from the existing snapshot)

All fields confirmed present in `types.ts` / `readModel.ts`:

| UI element | Source | Status |
|---|---|---|
| List: Name/role | `AgentMember.name`, `.role` | EXISTS |
| List: Provider | `AgentMember.provider` → `ProviderBadge` | EXISTS |
| List: Status | `runtime_status ?? status` → `StatusDot(memberTone)` | EXISTS |
| List: Workload | `queued_count + inbox_count` | EXISTS (unused in list) |
| List: 7-day sparkline | `computeAgentStats(sessions).activity7d` | NEW helper, existing data |
| List: Runs | `computeAgentStats(sessions).runCount30d` | NEW helper, existing data |
| List: filter chips | `runtime_status`, `runtime_alive`, `runtime_health`, `current_task_id` | EXISTS |
| Left rail: identity props | `provider`, `model`, `runtime_status`, `runtime_health`, `current_task_id`, `prompt_ref`, `skill_refs` | EXISTS (reuse `DocProperties` block verbatim from `Surfaces.tsx:2548-2573`) |
| Left rail: Workload / Last active / Sessions rows | `queued_count`, `inbox_count`, `stats.lastActiveAt`, `stats.runCount30d` | NEW rows, existing data |
| Now banner: running session | `sessionsByMember` filter `status==="running"`, max `started_at` | EXISTS (`readModel.ts:413-414`) |
| Now banner: elapsed | `formatDuration(session.started_at, now)` | EXISTS (`readModel.ts:966`) |
| Now banner: deliver/wake | `deliverQueued(member.id,{startRuntime:true})` (the call Composer already chains) | EXISTS |
| Conversation tab | `ConversationStream` (`selectedMemberTimeline` kind=message + `provider_sessions`) | EXISTS, reuse verbatim |
| Tasks tab | `model.tasks` filtered by `assignee_agent_id`/`reviewer_agent_id`; `AgentCurrentTask`, `AssignTaskControl` | EXISTS |
| Config › 指令 | `prompt_ref` (`MonoId` + lazy markdown preview) | EXISTS (ref); body fetch best-effort |
| Config › Skills | `skill_refs` → Badge wrap | EXISTS |
| Config › Runtime | `AgentRuntimeSection` (RuntimeHealthPanel + SessionList + ChildThreadList) | EXISTS, reuse verbatim |
| Config › 环境变量 / 自定义参数 / MCP | `provider_config` (NOT in TS schema) | GAP → "Not configured" placeholder; shape defined below |

**Open the detail on the conversation by default:** row click → `onSelectionChange({surface:"agents", memberId, agentTab:"conversation"})`; a bare `?agent=<id>` (no `?agentTab`) also resolves to conversation.

---

## 2. Per-agent stats read-model shape

**Decision: compute CLIENT-SIDE from `snapshot.provider_sessions`. No backend change for v1.** The audit confirms the snapshot already serializes all sessions and `readModel.ts:413-414` already filters by member. Adding a per-agent aggregate to the snapshot would impose an O(n·m) backend scan for a list view; we avoid it.

Add a pure helper next to `formatDuration` (`readModel.ts:966`):

```ts
// types.ts — new interface
export interface AgentStats {
  runCount30d: number;          // sessions with started_at within 30d
  runsTotal: number;            // lifetime session count
  succeeded: number;            // status === "succeeded" (30d)
  failed: number;               // status === "failed" (30d)
  successRate: number | null;   // succeeded / (succeeded + failed), null if 0 terminal
  avgDurationMs: number | null; // mean(ended_at - started_at) over terminal sessions
  activity7d: number[];         // length-7 daily session counts, oldest→newest (sparkline)
  lastActiveMs: number | null;  // max(started_at) → "recent activity" sort + "last active"
  runningCount: number;         // status === "running" → list pulse + Now banner
  liveSessionId: string | null; // max started_at among running|queued (current work)
}

// readModel.ts — new pure helper (O(n) over a member's own sessions)
export function computeAgentStats(sessions: ProviderSession[], nowMs: number): AgentStats { … }
```

Implementation notes (all reuse-first):
- Timestamp parsing: **lift `parseTs` out of `Surfaces.tsx:2910` into a shared util** (or re-export from `readModel.ts`) so list + helper share one parser that handles `"unix-ms:<ms>"` and ISO. (`formatDuration` currently only `Date.parse`s — fine for display, but bucketing needs `parseTs`.)
- Status mapping uses the confirmed `ProviderSessionStatus = "queued"|"running"|"succeeded"|"failed"|"canceled"|"stale"`. Success = `succeeded`; failure = `failed`; `canceled`/`stale` excluded from rate.
- `activity7d`: bucket by `started_at` into 7 day-bins `[today-6 … today]`, count per bin.
- **List performance:** in `buildModel`, do ONE `groupBy(agent_member_id)` pass over `snapshot.provider_sessions` to build `Map<memberId, ProviderSession[]>`, then per-row stats are O(sessions-for-that-member). Total list cost = O(total sessions), not O(agents × sessions). Expose this map on `WorkbenchModel` (e.g. `sessionsByAllMembers`).
- **Detail:** reuse the already member-scoped `model.sessionsByMember` → `computeAgentStats(model.sessionsByMember, Date.now())`.

**v2 escalation path (documented, not done):** if session volume grows, move `computeAgentStats` server-side into `member_cards` (`crates/harness-cli/src/main.rs:6620-6665`, where `inbox_count`/`queued_count` are already derived) emitting the same `AgentStats` shape — zero UI change.

**Provider config shape (defines the contract so the backend can fill it later; tabs render "Not configured" until then):**
```ts
export interface AgentProviderConfig {
  model?: string | null;
  profile?: string | null;
  thinking_mode?: string | null;
  sandbox_policy?: string | null;
  runtime_workspace_roots?: string[];
  env?: Record<string, string>;            // 环境变量 (secret-looking keys masked in UI)
  mcp?: Array<{ name: string; transport?: string; command?: string; url?: string; allowed_tools?: string[] }>;
}
// AgentMember gains: provider_config?: AgentProviderConfig | null;
```

---

## 3. ASCII mockups

### 3a. Agents LIST (centered DocumentSurface, widened to max-w-[1180px])

```
┌─ 🤖 Agents ─────────────────────────────────────────────────────────────────────────────┐
│  Every agent in the workspace. Open one to message it and assign work.   [+ New agent]    │
│                                                                                            │
│  [All 6] [Online 4] [Working 2] [Idle 1] [Offline 0] [Unstable 1]      Sort: Recent ▾     │
│  ────────────────────────────────────────────────────────────────────────────────────────│
│  NAME              PROVIDER  STATUS        WORKLOAD    7-DAY        RUNS   CURRENT TASK     │
│  (◍) Nova          [codex]   ● running     2q · 1in    ▁▃▂▅█▂▁      142    ● Refactor auth  │
│   executor                                                                                 │
│  (◍) Atlas         [claude]  ○ idle        0           ▁▁▂▁▃▁▁       37    —                │
│   reviewer                                                                                 │
│  (◍) Echo          [codex]   ◌ offline     0           ▁▁▁▁▁▁▁        4    —                │
│   member                                                                                   │
│  (◍) Builder       [claude]  ◐ unstable    5in · 3q!   ▂▄█▇▅▃▂       58    Migrate db       │
│   member                                                                                   │
└────────────────────────────────────────────────────────────────────────────────────────┘
  Columns collapse on <1100px: 7-DAY + WORKLOAD hide (md:grid), degrading to the 4-col layout.
  RUNS tooltip → "succeeded/failed (P%)". Sparkline tooltip → per-day counts. Row click →
  agentTab:"conversation".  Empty-filter → EmptyState("No agents match this filter", "Clear").
```

### 3b. Agent DETAIL (full-height AgentDetailShell: left rail + right pane)

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ TopBar:  ● live · agent-dashboard            [freshness 2s]   [poll]  [debug]               │
├────────┬───────────────────────────────────────────────────────────────────────────────────┤
│ AppRail│ LEFT CONFIG RAIL (~300px, ScrollArea) │ RIGHT PANE (chat-first, fills w + h)        │
│  [Agts]│ ───────────────────────────────────── │ ─────────────────────────────────────────  │
│  [Work]│ ‹ Agents                              │ ● RUNNING · "Refactor auth"  ⏱ 4m12s        │  ← NOW banner
│  [Goal]│                                       │   codex · turn   ▸ raw      [Deliver / wake] │     (pane chrome,
│  [Docs]│ (◍) Nova                              │ ─────────────────────────────────────────── │      above tabs)
│  [Warn]│  ● running  [codex]  m_4f2a   [⋯]     │ [ Conversation* ] [ Tasks ] [ Config ]       │  ← TabsList
│        │ ───────────────────────────────────── │ ─────────────────────────────────────────── │
│        │ PROPERTIES                            │  Conversation · oldest first    [12 msgs]    │
│        │  Provider     [codex]                 │ ─────────────────────────────────────────── │
│        │  Model        gpt-5-codex             │                      ┌─ OPERATOR  09:14 ──┐  │
│        │  Status       running                 │                      │ pick up the auth   │  │
│        │  Runtime hlth ✓ alive · deliv pass    │                      │ refactor           │  │
│        │  Current task Refactor auth →         │                      └────────────────────┘  │
│        │  Workload     2 queued · 1 in         │  NOVA  09:15                                  │
│        │  Last active  just now                │  ┌────────────────────────────┐              │
│        │  Sessions     142 · 96% ok            │  │ On it. Reading the module. │              │
│        │ ───────────────────────────────────── │  └────────────────────────────┘              │
│        │ RUNTIME HEALTH ▾                      │    delivered · ▸ codex · 0:48 · 12 ev · turn  │
│        │  ▣ process_alive  ✓                   │                                              │
│        │  ▣ socket_exists  ✓                   │  … (ChatBubble timeline, reused #60) …        │
│        │  ▣ protocol_probe pass                │ ─────────────────────────────────────────── │
│        │  ▣ delivery_probe pass                │ ┌──────────────────────────────────────────┐ │
│        │  checked 4s ago                       │ │ Operator ▸ Message Nova…        [ Send ▸ ]│ │  ← Composer
│        │ ───────────────────────────────────── │ └──────────────────────────────────────────┘ │     (pinned)
│        │ SKILLS ▾                              │                                              │
│        │  [git] [rust] [code-review]           │                                              │
└────────┴───────────────────────────────────────┴───────────────────────────────────────────┘

── RIGHT PANE › Tab: Tasks ──────────────────────────────────────────────────────────────────
│ AgentCurrentTask (current task card + AssignTaskControl)                                     │
│ ── Executing (assignee_agent_id === me, not done/archived) ──                                │
│   ● Refactor auth        [in_progress]   ⎇ feat/auth         (current)                       │
│   ○ Migrate db schema    [todo]          ⎇ feat/db                                           │
│ ── Reviewing (reviewer_agent_id === me) ──                                                   │
│   ○ Fix snapshot race    [in_review]     ⎇ fix/race                                          │
│ ── Completed (30d) ──   (done/archived within window)                                        │

── RIGHT PANE › Tab: Config (Multica's 5 panes folded into collapsible blocks) ───────────────
│ ▾ 指令 (Prompt)        prompts/exec.md   <lazy markdown preview, read-only>                   │
│ ▾ Skills               [git] [rust] [code-review]  → .agents/skills/<id>/SKILL.md            │
│ ▾ Runtime              [RuntimeHealthPanel] + SessionList(8) + ChildThreads(1)  (reused)     │
│ ▸ 环境变量 (Env)        Not configured — no provider_config.env in snapshot                   │
│ ▸ 自定义参数 (Params)   Not configured                                                        │
│ ▸ MCP                  Not configured                                                         │
```

**Now banner states** (all from existing data):
- **Running:** running `ProviderSession` exists → pulsing `StatusDot(running)` + "RUNNING" + task title (→ task surface) + live elapsed (`formatDuration(started_at, now)`) + provider + `▸ raw` (opens that session's `TurnDrillIn`).
- **Idle + queued:** `StatusDot(warn)` + "Idle · {queued_count} queued" + **[Deliver / wake]** `ActionButton` (`deliverQueued(member.id,{startRuntime:true})`, gated on `actionsEnabled`).
- **Idle, nothing queued:** muted `StatusDot(idle)` + "Idle" + last-active relative time.

---

## 4. Component tree

```
WorkbenchShell (existing; Inspector already suppressed for "agents")
└─ SurfaceSwitch
   ├─ AgentsList  (EXISTING, EXTENDED — surface:"agents", no memberId)
   │   └─ DocumentSurface(max-w-[1180px])
   │      ├─ header (+ New agent)                          [reuse]
   │      ├─ AgentStatusFilterBar  ◀ NEW (Tabs primitive as segmented control + counts)
   │      ├─ Sort Select                                   [reuse Select]
   │      └─ DocSection("{n} agents")
   │         └─ grid rows (4→6 cols)                       [extend existing grid]
   │            ├─ Avatar + name/role                      [reuse]
   │            ├─ ProviderBadge                           [reuse]
   │            ├─ StatusDot + status (pulse if running)   [reuse]
   │            ├─ Workload badges                         ◀ NEW cell (existing data)
   │            ├─ AgentSparkline                          ◀ NEW atom (inline SVG)
   │            ├─ Runs badge                              ◀ NEW cell (computeAgentStats)
   │            └─ current task                            [reuse]
   │
   └─ AgentDetail  (EXISTING, REWORKED — surface:"agents", memberId set)
       └─ AgentDetailShell  ◀ NEW (flex; left rail + right pane; replaces DocumentSurface)
          ├─ LEFT: AgentConfigRail  ◀ NEW (ScrollArea ~300px, border-r)
          │   ├─ back button + header block                [lift verbatim 2516-2547]
          │   ├─ DocProperties (+ Workload / Last active / Sessions rows)  [reuse + add rows]
          │   ├─ CollapsibleSection("Runtime health")  ◀ NEW wrapper
          │   │   └─ RuntimeHealthPanel                    [reuse]
          │   └─ CollapsibleSection("Skills")
          │       └─ skill_refs Badge wrap                 [reuse Badge]
          └─ RIGHT: AgentDetailPane  ◀ NEW (flex-col, full height)
             ├─ CurrentWorkBanner  ◀ NEW (StatusDot + Badge + ActionButton + TurnDrillIn)
             ├─ Tabs / TabsList (Conversation* · Tasks · Config)   [reuse tabs.tsx]
             └─ TabsContent (flex-1 min-h-0)
                ├─ "conversation" → ConversationStream  [reuse VERBATIM; min-h-[34rem]→h-full]
                │     └─ ChatBubble → TurnDrillIn / Composer        [reuse]
                ├─ "tasks" → AgentTasksTab  ◀ NEW (thin)
                │     ├─ AgentCurrentTask + AssignTaskControl       [reuse]
                │     └─ task rows by role/status (TimelineRow)     [reuse]
                └─ "config" → AgentConfigTab  ◀ NEW
                      ├─ CollapsibleSection("指令") → MonoId + lazy Markdown
                      ├─ CollapsibleSection("Skills") → Badge wrap
                      ├─ CollapsibleSection("Runtime") → AgentRuntimeSection  [reuse VERBATIM]
                      ├─ CollapsibleSection("环境变量") → DocProperties | "Not configured"
                      ├─ CollapsibleSection("自定义参数") → DocProperties | "Not configured"
                      └─ CollapsibleSection("MCP") → server cards | "Not configured"
```

**New primitives (5, all small):** `AgentSparkline` (inline SVG, lives in `atoms.tsx`), `CollapsibleSection` (chevron wrapper over `DocSection`, `atoms.tsx`), `AgentDetailShell`, `CurrentWorkBanner`, plus the `computeAgentStats` helper + `AgentStats`/`AgentProviderConfig` types. Everything else is reused verbatim.

**The only edit to a reused component:** `ConversationStream`'s outer section `min-h-[34rem]` → `h-full` (so it fills `flex-1` `TabsContent`). One-line, low-risk.

---

## 5. Reuse notes (verbatim unless noted)

- `ConversationStream` / `ChatBubble` / `TurnDrillIn` / `Composer` (`Surfaces.tsx:2766/2837/2928/3067`) — verbatim except the one `min-h`→`h-full` line.
- `AgentCurrentTask` / `AssignTaskControl` (`2610/2672`) — verbatim in Tasks tab.
- `AgentRuntimeSection` (`2728`) — verbatim in Config › Runtime block (this is where the old stacked Runtime `DocSection` moves).
- `RuntimeHealthPanel` — verbatim in left rail.
- Header block + `DocProperties` (`2516-2573`) — lifted into the left rail.
- `Tabs/TabsList/TabsTrigger/TabsContent` (`components/ui/tabs.tsx`) — first real use; segmented filter bar + detail tabs.
- `parseTs` (`Surfaces.tsx:2910`) — lift to shared util for list + stats.
- `formatDuration` (`readModel.ts:966`), `sessionsByMember` (`413-414`), `taskTitle`, `memberTone`, `taskTone`, `deliveryHealthTone` — reused.
- `selection.ts` — extend with `agentTab` (`?agentTab=`), additive, defaults to conversation.

---

## 6. Sequenced WP plan (each compiles + is independently shippable)

**WP1 — Stats read-model + types (no UI).**
Add `AgentStats` + `AgentProviderConfig` to `types.ts`; add `provider_config?` to `AgentMember`. Add `computeAgentStats(sessions, nowMs)` to `readModel.ts`; lift `parseTs` to shared util; build `sessionsByAllMembers: Map<memberId, ProviderSession[]>` in `buildModel` and expose on `WorkbenchModel`. Unit-test bucketing + success rate.
Files: `src/types.ts`, `src/model/readModel.ts`, `src/model/readModel.test.ts` (if a test dir exists).

**WP2 — Routing: `agentTab`.**
Add `agentTab?: "conversation"|"tasks"|"config"` to `SelectionState`; read/write `?agentTab=`; default conversation. Verify bare `?agent=<id>` still lands on conversation.
Files: `src/app/selection.ts`.

**WP3 — Small primitives.**
`AgentSparkline({counts:number[]})` (pure SVG, ~80×18, 7 bars normalized) + `CollapsibleSection` (chevron over `DocSection`, `defaultOpen` prop) in `atoms.tsx`.
Files: `src/components/workbench/atoms.tsx`.

**WP4 — List extension (Multica vocabulary).**
Extend `AgentsList` grid 4→6 cols (Workload, 7-day sparkline, Runs); add `AgentStatusFilterBar` (Tabs segmented + counts: All/Online/Working/Idle/Offline/Unstable, derived from `runtime_status`/`runtime_alive`/`runtime_health` staleness >150s/`current_task_id`); add Sort Select (Recent default); responsive hide on <1100px; empty-filter `EmptyState`. Widen `DocumentSurface` to `max-w-[1180px]`. Row click sets `agentTab:"conversation"`.
Files: `src/surfaces/Surfaces.tsx`.

**WP5 — Detail shell + left rail + Now banner + Conversation default.**
`AgentDetailShell` (flex left/right) replacing the `DocumentSurface` body of `AgentDetail`; `AgentConfigRail` (lift header + `DocProperties` + add Workload/Last active/Sessions rows + `RuntimeHealthPanel` + Skills as `CollapsibleSection`s); `CurrentWorkBanner` (3 states); mount `Tabs` with Conversation default; `ConversationStream` `min-h`→`h-full`. Tasks/Config render placeholders pending WP6.
Files: `src/surfaces/Surfaces.tsx`.

**WP6 — Tasks tab + Config tab.**
`AgentTasksTab` (AgentCurrentTask + AssignTaskControl + role/status-grouped rows from `model.tasks`). `AgentConfigTab` (CollapsibleSections: 指令 lazy markdown, Skills, Runtime = `AgentRuntimeSection`, 环境变量/自定义参数/MCP from `provider_config` else "Not configured").
Files: `src/surfaces/Surfaces.tsx`.

**WP7 — Polish + verify.**
Tooltips (Runs success/fail, sparkline per-day, masked env keys), responsive left-rail collapse <1024px, keyboard/back-button check, screenshot pass.
Files: `src/surfaces/Surfaces.tsx`, `src/components/workbench/atoms.tsx`.

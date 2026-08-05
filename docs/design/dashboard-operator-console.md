# Dashboard Operator Console — Spec & Acceptance Design

Status: draft for implementation on `feat/dashboard-operator-console`.
Goal: the dashboard becomes the single operator console. A human can create
agents and teams, watch every member live, and chat — without opening
terminals. Every provider control surface the backend defines is reachable
from the UI or explicitly marked unavailable with a reason.

## 1. Product model

- **Host Agent**: the Host is the coordinating authority of the harness
  itself (serve process + operator), not a spawnable entity.
  `AgentTeamRun.host_surface` is `"http"` for console-created runs, i.e. the
  dashboard IS the host surface. The console therefore provides:
  - a **Host lane** per team run: the host inbox (member messages addressed
    to the host with their Work chain) and an answer composer that keeps the
    Assignment correlation;
  - optional durable host identity: an `AgentTeam.host_member_id` may point
    at a durable AgentMember (ADR 0052) — the team creation form exposes it.
- **Agent Members**: durable execution identities created from the Agents
  directory (`POST /v1/agents`), then used in teams/runs.
- **Agent Teams**: created independently (`POST /v1/teams`) or under a
  Mission (`POST /v1/missions/{id}/teams`); a team run is created from the
  team (Mission-scoped or standalone) and started from the same dialog.
- **Chat**: ordinary correlated TeamMessages (operator → member, member →
  host), PendingInteraction resolution, and live Steer where the provider
  mode supports it. No legacy `/v1/messages` path in new UI.
- **Providers**: only registered persistent bidirectional modes are
  selectable: `kimi/kimi_acp`, `codex/codex_app_server`,
  `claude/claude_agent_sdk`, `pi/pi_rpc` (mirrors
  `validate_team_member_execution_mode`). Future providers (e.g. a DeepSeek
  adapter) plug into the same registry; the UI lists what the registry
  declares, nothing hardcoded beyond the display map in `lib/provider.ts`.

## 2. Backend changes

### 2.1 `POST /v1/missions/{id}/log` (new)

Wave write routes are retired (ADR 0051); the replacement Mission Log append
is CLI-only today, leaving the console without any way to record Host plan
judgment. New route body:

```json
{ "kind": "judgment|replan|recovery|closeout_evidence", "body": "…",
  "actor": "optional, defaults to host" }
```

Reuses the same append path as `harness mission log` (append-only, newest
first, store-scoped). Returns the appended entry. Errors: unknown kind,
empty body, missing mission → 400/404.

### 2.2 No other new routes

Everything else already exists: `/v1/agents`, `/v1/teams`, `/v1/team-runs`
(create + `start`), `/v1/team-runs/{id}/members` (add member),
messages/ack/resolve, steer/interrupt/close/reopen, host-inbox GET, SSE.

## 3. Frontend changes

### 3.1 Agents directory (`Surfaces.tsx`)
- **New Agent Member** dialog → `createAgent` descriptor (exists, unused):
  name*, role*, provider (kimi|codex|claude|pi), model, execution mode
  (auto-default per provider, validated), optional permission profile /
  approval policy / sandbox policy. Honest note: creation does not start a
  runtime; members run when a team run starts or reopens them.
- Card deep-links unchanged; AgentDetail keeps Model & configuration +
  Chat blocks.

### 3.2 Agent Teams home (`AgentTeamsHome.tsx`)
- **New Agent Team** dialog → `createTeam`: name*, description, lead
  (host or durable member), member multi-select from durable AgentMembers,
  optional host member.
- Each team card row gains **New run** → extended AttemptDialog reachable
  outside a Wave: objective*, member specs (derived from the team or
  editable), optional Mission link, execution root, budget. After creation
  the dialog offers **Start now** (`startTeamRun`) — closing the
  create→start gap. Standalone retry entry stays in the War Room.

### 3.3 Missions surface
- Replace the retired Wave dialogs. **Add Wave** becomes **Append Host
  judgment** (kind/body form → `POST /v1/missions/{id}/log`); **Update
  plan** posts kind `replan`; **Gate/advance** copy becomes an explicit
  read-only note ("Host decision is recorded as a Mission Log entry") with
  a `judgment` form. `createWave/updateWaveContext/advanceWave/gateWave`
  descriptors are deleted.
- `updateMissionContext` + `linkMissionTeam`/unlink descriptors stay
  available; dedicated entry points are follow-up work (tracked, not in
  this iteration).

### 3.4 Team War Room
- Members tab: **Add member** button → add-member dialog
  (`POST /v1/team-runs/{id}/members`: name*, role*, provider, model,
  execution mode, optional resume native session id, initial work).
- War Room composer gains the **response intent** control (Needs reply /
  Informational), matching the Member composer.
- Host lane keeps LeadInbox Answer flow (correlation-preserving reply);
  host-authored messages render with a Host badge.

### 3.5 Live observation
- `fetchNativeMemberActivity` becomes a poll (5s) while the member status
  is running, stops otherwise — native provider activity is no longer
  one-shot. SSE already covers durable rows; `member_activity` keeps the
  transient preview.

### 3.6 Honesty rules
- Any control whose backend capability is absent for the selected
  provider/mode renders disabled with the reason (existing pattern).
- Creation forms never claim a runtime started.

## 4. Acceptance design

Deterministic, in the existing `check:dashboard` style.

| # | Check | Pattern | Asserts |
|---|---|---|---|
| A1 | `operator-controls-check.mjs` (extended) | transpiled `actions.ts` + source | `appendMissionLog`/`addTeamMember` shapes; retired wave descriptors gone; `TEAM_MEMBER_PROVIDER_MODES` single registry incl. `pi_rpc`; response-intent control; polling gated on running |
| A2 | folded into A1 | substring | see above |
| A3 | `operator-console-browser-check.mjs` (new) | Playwright + mocked `/v1` with POST capture + mutating snapshot | create member via form; create team → visible before first run; create run linked to team → Start now posts `/start`; Mission Log append renders; retired Add Wave gone; chat with explicit `response_intent` |
| A4 | `crates/harness-cli/tests/mission_wave_api.rs::http_mission_log_append_route_appends_and_rejects_unknown_kind` | cargo | log appends, monotonic revisions, default actor, unknown kind/missing mission rejected |

Gate: `pnpm check:dashboard` + the new checks green; acceptance loop = run
gate → fix → rerun until green. Live-provider runs remain separate evidence
per AGENTS.md; this spec claims only deterministic contracts.

## 5. Out of scope (tracked)

- Resume as a standalone endpoint (reopen + `resume_native_session_id`
  cover it today).
- HostAttention HTTP surface (no endpoint exists; needs its own design).
- New provider adapters (DeepSeek etc.) — registry-driven later.
- Work-delivery-claim reconciliation over HTTP (MCP-only today).
- Company OS governance actions (token-gated, separate surface).

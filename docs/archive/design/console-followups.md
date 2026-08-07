# Console Follow-ups — Spec & Acceptance

Status: implementing on `feat/console-followups` (base 2a61e93). Closes the four
tracked items from `docs/design/dashboard-operator-console.md` §5.

## 1. Mission context & team-link entry points (frontend)

Descriptors `updateMissionContext` and `linkMissionTeam` existed with no UI;
`unlinkMissionTeam` did not exist.

- New descriptor `unlinkMissionTeam` → `POST /v1/missions/{id}/unlink-team`
  (route already exists).
- Mission canvas brief section: **Edit context** button → dialog with the
  current Markdown → `updateMissionContext`.
- Mission canvas linked-teams list: each linked team row gets **Unlink**, and
  an **Link team** picker lists unlinked durable teams → `linkMissionTeam`.
- Honest gating: disabled without a live source, same title convention.

## 2. HostAttention HTTP surface

HostAttention is the durable wake-notification ledger the store derives from
WorkOperations (review requested, blocked, accepted, changes requested,
cancelled, delivery failed). It had no HTTP surface.

Backend (no new store ops; all lifecycle ops exist in harness-store):

- `GET /v1/host-attentions?team_run_id=<id>` → `{ "attentions": [...] }`,
  reconciled latest rows for the run. Unknown run → 404.
- `POST /v1/host-attentions/{id}/ack` → console acknowledgement. The console
  is the Host surface for http-bound runs, so the endpoint resolves the run's
  own `host_surface`/`host_thread_id` binding and walks the lifecycle as
  needed: Actionable → claim + complete(`console-ack` receipt) + acknowledge;
  Claimed (any claim) → fail stale claim, then re-claim/complete/ack;
  Delivered → acknowledge; Acknowledged → idempotent 200. Errors → 400/404.
  Transport intake only — never mutates Work.

No typed SSE frame this iteration: attentions materialize lazily on read
(reconciliation), so the console refetches after ack (runAction refresh) and
on War Room focus. Documented; `projection_invalidated` already covers
ledger appends.

Frontend:

- War Room gains a **Host attention** module (above the conversation when any
  row `needs_host_action`): kind, work title, member, attempt, Ack button.
- Ack dispatches the POST then relies on the standard snapshot refresh.

## 3. Standalone member resume endpoint

Research conclusion: there is no state where resume is meaningful but reopen
is not — reopen already respawns the adapter resuming the recorded native
session, and live-but-idle members are steered, not resumed. Therefore:

- `POST /v1/team-runs/{id}/members/{m}/resume`, body `{resumed_by?, reason?}`.
  - Active coordination → 400 with the honest message that an active member
    is continued by message/steer, not resume.
  - Otherwise the same capability gates as reopen (profile refresh,
    `supports_resume`, native-session availability) and the same reopen
    machinery incl. conditional Supervisor start (202).
- Member focus: when the member's native session supports resume, the closed
  member's reopen control becomes **Resume session** and posts `/resume`;
  without a resumable native session it stays **Reopen** on `/reopen`. One
  control, capability-labelled — no duplicate buttons.

## 4. Acceptance matrix

| # | Where | Asserts |
|---|---|---|
| F1 | `operator-controls-check.mjs` (extended) | edit-context + link/unlink wiring; Resume session capability label; attention module wiring |
| F2 | `operator-console-browser-check.mjs` (extended) | mission context edit POSTs `/context`; link/unlink POSTs; host attention module renders + Ack POSTs `/ack`; Resume button posts `/resume` |
| B1 | cargo (new `tests/host_attention_api.rs` or mission_wave_api) | host-attentions GET + ack lifecycle (Actionable→Acknowledged, idempotency) |
| B2 | same | resume route: active member rejected; closed resumable member resumes via reopen machinery |

Gate: `pnpm check:dashboard` + the cargo tests above green; iterate until green.

Results (2026-08-06): backend cargo tests 2/2 green on first full run after
two test-fixture fixes; `operator-controls-check` 42 pass; new browser flows
20/20 (create/start, mission log, chat intent, Host attention ack, context
edit, team link/unlink, capability-labelled resume).

## 5. Out of scope

- Typed HostAttention SSE frame (refresh-on-action suffices; documented).

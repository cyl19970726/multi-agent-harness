# Agent Team War Room RoleView parity matrix

```text
issue: #444
baseline_revision: c0ab362ab887d7b40321089b85676beb58a394be
historical_reference: 5c0258fa^
active_surface: TeamWorkspace
authority: authenticated server-built RoleViews + canonical Role Actions
```

This matrix is the implementation gate for restoring the mature Agent Team
War Room. It preserves the old page's product experience without restoring its
snapshot joins or retired writers.

## Classification

- **A — direct reuse:** the authenticated RoleView already carries the truth;
  only presentation is required.
- **B — adapter/refactor:** reuse the established visual/interaction language,
  but replace old snapshot-shaped props with bounded RoleView types.
- **C — server projection/action:** add a closed authenticated schema field or
  server-authorized action. The browser must not calculate or impersonate it.
- **D — deliberately retired:** do not restore; explain the current product
  replacement or authority boundary.

## Parity inventory

| Area | Mature contract | Current authenticated truth | Class | Required parity slice |
| --- | --- | --- | --- | --- |
| Works board/list | Works-default tabs; open/active/review/closed lanes; owner/readiness pressure; selected Work drawer or mobile sheet; explicit non-drag actions. | `TeamWorkspace.works` supplies closed `WorkSummary` rows and HostConsole supplies exact `allowed_actions`; active UI renders only `WorkTable`. | A/B/C | Reuse board/list and drawer interaction. Add bounded title, context, criteria, claim/readiness, blockers, parent/prerequisite progress, result/artifact/check, latest WorkEvent and exact delivery references to the server projection. Resolve actions only by `allowed_actions` target/version. |
| Filters and URL state | Owner, unassigned, blocked, review, event/message and search filters; selected Work and scroll anchor survive deep links. | No TeamWorkspace filter state; page cursor exists but is unused. | B/C | Local filtering is allowed only over fields already projected for this page. Add missing search/display fields, pagination contract and URL selection keys; never scan the full snapshot as a fallback. |
| Capacity and member pressure | Active turns, ready members, queued/review/blocked Works; member addressability, runtime/session state and separately labelled provider-account capacity. | `memberCapacity` has identity, role, organization status, current MemberRun, coarse runtime and capacity only. PendingInteraction is not loaded into RoleView `Facts`; MemberWorkbench currently emits an empty list. | B/C | Reuse `TeamCapacityStrip` and `TeamMembersCapacity` visual grammar. Server projects addressability, Work counts, current explicit action, native-session health, provider/model, observed provider capacity and typed pending pressure. Unknown remains `not observed`. |
| Activity | One source-aware timeline for WorkEvents, WorkDelivery, authored Message, PendingInteraction, control/outcome and transient native activity. | No activity collection exists in TeamWorkspace or HostConsole. Runtime fabric exposes generic record summaries but is not a page timeline. | B/C | Add bounded typed activity rows from canonical records. Native provider activity remains read-on-demand/transient and labelled; no transcript/tool/thinking persistence or browser join. |
| Lead Inbox and mailboxes | Host/member pressure counts, questions awaiting response, Work linkage, typed route, correlation/causation, delivery/ACK and direct answer. | `MessageSummary` omits body, kind, correlation, causation and exact delivery rows. No Host inbox projection exists. | B/C | Add Host-scoped inbox rows in HostConsole and bounded conversation rows in TeamWorkspace. Reuse mailbox disclosure/components after adapting props. |
| Supervisor/runtime/reconnect | Team Supervisor generation, heartbeat/currentness, owner locator, transport/reconnect and control availability; NodeDaemon shown as its parent. | `daemon_summary` reports only NodeDaemon lease status/generation. No Team Supervisor or reconnect projection. | C | Project exact latest Team Supervisor, parent daemon generation, expiry/heartbeat, owner availability, reconnect/recovery state and disabled reason. Never infer Team Supervisor health from NodeDaemon health. |
| Mission/plan/review context | Mission orientation, current intent, attempt facts and Work review readiness. | TeamWorkspace has only `mission_id`; HostConsole `convergence_plans` is empty. Historical page assumes current Wave. | A/B/C/D | Project compact Mission context plus latest Mission Log entries and run/attempt facts. Preserve Work review/gates via HostConsole. **D:** no new Wave judgment, Wave gate or Wave-owned TeamRun; historical Waves are read-only navigation only. |
| Composer/reply | Team or member target, optional Work link, response intent, correlated reply and visible pending/success/failure state. | HostConsole exposes authenticated `send_message` and `reply_message`, but only through a generic action form; no source-message selection. The TypeScript adapter currently omits supported `work_id`/`evidence_refs` and does not present `response_required`. | A/B/C | Repair the closed adapter fields, then reuse compact responsive composer presentation. Server projects replyable source rows and exact recipient options; submit only prepared Role Actions with server CAS/idempotency/auth. Browser never authors sender identity. |
| Work delivery and Message ACK | WorkDelivery version/claim/receipt/failure; MessageDelivery recipient/provider receipt/ACK; explicit recovery state. | WorkSummary has aggregate WorkDelivery counts. Runtime fabric has generic delivery summaries. Reconcile actions exist; a recipient ACK action is not exposed by the Role Action manifest. | A/B/C/D | Show aggregate signal directly and add exact bounded typed delivery rows. WorkDelivery never gains ACK. If manual MessageDelivery ACK remains required, add it only for the exact authenticated recipient/session with CAS on a valid state. **D:** do not restore legacy `team-run .../ack`, inferred Host ACK or caller-selected recipient identity. |
| Selected member/context rail | Mission, Mission Log, attempt, selected MemberRun, current Work/action, workspace, native session and artifacts. | Member rows deep-link, but a Host cannot read another actor's MemberWorkbench; no Host-visible selected-member summary exists. | B/C | Add Host-scoped selected-member summaries to HostConsole or a bounded Host member projection. Reuse context modules; do not weaken MemberWorkbench exact-self authorization. |
| Authorized controls | Work lifecycle, message/reply, member close/reopen/retire/resume and workspace/gate actions with visible disabled reasons. | HostConsole already emits closed `allowed_actions`; generic RoleActionPanel is functional. Legacy page used unauthenticated snapshot-era descriptors for some controls. | A/B/C/D | Compose contextual controls around selected objects while retaining generic fallback. **D:** do not restore retired free-form TeamRun writers or Host impersonation. Any missing runtime control requires a new server action and current authority proof. |
| Responsive behavior | Desktop three-region workbench; tablet stacked context; mobile grouped lists/sheets and collapsed composer; 44px targets; 320px no overflow. | Current page is padding plus a wide table and simple grid. Mature responsive components remain orphaned. | B | Restore the established responsive transformations using RoleView props. Horizontal Kanban and drag-only paths remain prohibited on mobile. |
| Loading/empty/error/stale | Useful zero-member/zero-Work states, filtered empty/reset, last-good stale state, partial-source failure, mutation conflict and unavailable native session. | `ViewState`, `AttentionStrip` and generic Work empty are reusable, but refresh replaces the whole page and empty state has no next action. | A/B/C | Preserve last-good view during refetch, separate initial loading from stale/partial failure, and render useful empty Team guidance from projected Team/member/action state. Surface exact server errors and disabled reasons. |

## Exact RoleView schema/API gaps

### TeamWorkspace

1. `team`: add bounded TeamRun/attempt summary, Host identity and display name,
   current run status, previous run, execution root/project binding and explicit
   Mission relation.
2. `works[]`: add the display and readiness fields listed above plus bounded
   WorkEvent and WorkDelivery detail. Do not return raw ledger rows.
3. `members[]`: add display name, provider/model, addressability, pressure
   counts, current action, heartbeat/native-session health and observed capacity.
4. `messages[]`: add safe rendered body source, message kind, Work relation,
   correlation/causation, typed delivery rows and reply eligibility.
5. Add typed `activity`, `pending_interactions`, `artifacts`, and
   `historical_wave_context` collections. The latter is read-only and optional.
6. Add the already schema-required `runtime_fabric` type to all four
   TypeScript RoleView data interfaces before any UI consumes those rows.

### HostConsole

1. Add `mission_context` with latest append-only Mission Log entries.
2. Add exact `team_supervisor` and `runtime_recovery` summaries; retain
   `daemon_summary` as the parent service fact.
3. Replace empty `convergence_plans` with explicit Work review/integration
   pressure or remove the placeholder from the page.
4. Add Host inbox/reply context and Host-visible selected-member summaries.
5. Add only actions that the authenticated Host may actually execute; preserve
   CAS version, idempotency and disabled reason for every action.

### Existing capability distinctions

- Owner/phase/condition/priority filters are already safe over the bounded
  TeamWorkspace Work list; richer search and cards depend on added fields.
- Canonical Message send/reply/request-decision actions already exist. Their
  visual composition is not permission to broaden the payload or actor.
- Operator currently exposes Team Supervisors only as lossy record summaries;
  Team/Host need a typed bounded summary, not access to the Operator view.
- Provider-private transcripts/tool/thinking remain **D**. Only native-session
  references and expiring transient activity may be shown.

### Transport and refresh

- `/v1/meta` negotiation remains mandatory.
- `X-AgentFirm-Token` remains memory-only and selects identity server-side.
- SSE remains invalidation-only. TeamWorkspace and HostConsole refetch their
  RoleViews; they never fold browser-authored events into authority.
- A stale/failed refresh retains the last-good view and disables mutations.

## Deliberate retirements

- Current-Wave scheduling, Wave gate/advance and Wave-owned TeamRun semantics.
- Client joins over `/v1/snapshot` to infer ownership, mailboxes, pressure,
  Supervisor state or activity.
- Legacy TeamRun message/ACK/control descriptors that select actor or recipient.
- Mirrored provider transcripts, tool calls, commands, file events or thinking.
- Drag-and-drop as the only Work mutation path.

## No-conflict boundary

This task owns the RoleView/dashboard paths listed in the first checkpoint. It
does not modify either Wave5 or Wave6 worktree. Git-ref comparison against
`codex/wave5-remote-node-fabric` and
`codex/wave6-cross-machine-collaboration` shows those branches own remote
fabric/collaboration contracts and shared runtime files, not the RoleView or
War Room files selected here. This task will avoid their shared `main.rs`,
store, collaboration schema and package-manifest files.

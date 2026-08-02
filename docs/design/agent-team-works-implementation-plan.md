# Agent Team Works Implementation And Acceptance Plan

```text
status: accepted implementation plan; code pending
owner_role: execution-foundation
decision: ADR 0050
```

## Goal

Replace Assignment-message ownership with one end-to-end Work model and then
use that model to finish itself. No release or dogfood space may expose two
ownership authorities.

## Ordered cutover

1. **Contract/bootstrap** — freeze durable member identity, trusted
   `ActorContext`, Work/Event/Delivery schemas, CAS/idempotency, cross-Team
   delegation, reset/export runbook, and contract version preflight.
2. **Core/store** — implement event-authoritative crash-recoverable commands,
   projection replay, delivery outbox, readiness, capacity, rebind, and
   concurrent claim tests.
3. **Application service** — expose the same Work service to CLI, HTTP, MCP,
   snapshots, and SSE. TeamRun creation materializes roster only.
4. **Supervisor/providers** — deliver Work envelopes and explicit receipt for
   `codex_app_server`, `claude_agent_sdk`, `kimi_acp`, and
   `external_interactive`. Provider turn completion never submits Work.
5. **Breaking cutover** — remove Assignment/Handoff ownership, auto-final
   handoff, stale environment names, schema readers, fixtures, Company OS joins,
   Skills, Plugin copies, and active-store compatibility in one boundary.
6. **Team Workbench** — ship Works, Activity, Members, Member owned/eligible
   Works, and Mission live/historical summaries using the same read model.
7. **Self-dogfood** — start a fresh Execution Space and use a Codex bootstrap
   Member to create/claim/submit the remaining UI, provider-parity, skill, and
   documentation Works; Host explicitly accepts them.

## Safe data boundary

Before the new binary writes:

1. quiesce the old service and release/expire Supervisor leases;
2. interrupt or close managed runtimes and reconcile claimed deliveries;
3. archive the old Execution Space with manifest, hashes, record counts,
   Team/Member ids, and native-session locators;
4. create a fresh Execution Space while preserving Company Store and Project
   Binding; and
5. make the new binary fail fast if it sees legacy Agent Team
   `TeamMessage(kind=assignment)` rows.

Rollback means reopening the archived space with the old binary. A future
offline converter may create another new space, but it never infers acceptance
from Handoff or provider final text.

## Frontend contract

| Surface | Desktop | Mobile | Primary proof |
| --- | --- | --- | --- |
| Works | Kanban/windowed list + non-modal detail drawer | grouped list + bottom sheet | assignment, ready pool, claim, block, Host acceptance |
| Activity | mailbox filters + Markdown timeline + composer | one timeline + filter sheet + composer | typed actor route, Message/WorkEvent/source distinction |
| Members | factual capacity table/grid | compact capacity list | addressability, active/queued/blocked-review/eligible-ready, provider capacity |
| Member Focus | owned/eligible Works + native activity + context rail | one stream + context bottom sheet | Work version, delivery, session, message/steer boundary |
| Mission Canvas | Wave history + live Works summary | one expanded Wave + live summary | immutable Wave snapshot separated from current execution |

New Works visual evidence must use frozen expected images, interaction/state
annotations, deterministic fixture captures, and labelled expected-vs-actual
comparisons. Pre-Works images are legacy baselines only.

## Deterministic acceptance

- host assignment without Assignment Message;
- one winner from concurrent eligible claims; loser receives latest owner;
- assigned busy Member is not interrupted and cannot start a second Work;
- prerequisite completion emits readiness event/delivery; prerequisite cancel
  requires explicit Host resolution;
- block, submit, request changes, resubmit, Host accept with required reasons
  and evidence;
- message-to-Work causation and idempotent retry;
- queued versus claimed/provider-received reassignment reconciliation;
- compatible Reopen preserves MemberRun/session; incompatible resume appends
  WorkRebound to a replacement binding while preserving ownership/evidence;
- child Team delegation uses typed cross-run refs and never auto-accepts parent;
- Company WorkItem relation aggregates result only through governed commands;
- standalone and Mission-linked Team navigation and URL restoration;
- 1,000 Works/100 Members with bounded DOM, stable sort, load-more/cursor;
- loading/partial/stale/empty/pending/conflict/failure/crash/close/retire/new
  Supervisor-generation states; and
- keyboard/non-drag paths, focus restoration, live announcements, reduced
  motion, non-color status, 44px mobile targets, zero serious/critical
  automated accessibility failures, and manual VoiceOver journeys.

## Real-provider self-dogfood

After the bootstrap slice is deterministic, run one standalone and one
Mission-scoped mixed-provider Team. At least one Member must create follow-up
unassigned Work, one must claim it, one must block/question and resume, and one
must delegate to a child Team. Preserve TeamRun, Work/Event/Delivery,
MemberRun/native-session, build SHA, timestamp, browser captures, artifacts,
checks, explicit submissions, Host acceptance, Wave judgment, and Mission
closeout.

The Host must reconstruct current responsibility without reading Message bodies.
Messages must still reconstruct why Agents coordinated. Provider-native records
must remain the only truth for transcript, tools, turns, files, and internal
subagents.


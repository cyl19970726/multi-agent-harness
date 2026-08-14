# ADR 0041: Provider-Neutral Member Continuation And Execution Ownership

```text
status: active; Assignment references amended by ADR 0050
date: 2026-07-28
extends: ADR 0031 interactive provider modes; ADR 0032 native execution truth; ADR 0037 Member autonomy; ADR 0039 durable mailbox
```

ADR 0050 replaces Assignment correlation as responsibility truth with Work
ownership and WorkDelivery. This ADR's one-top-level-driver, native
continuation, Workspace lease, and Host-acceptance boundaries remain active.
ADR 0056 supersedes this ADR's PendingInteraction and per-request permission
language; those terms below are historical only.

## Context

Persistent Agent Team members can now use providers that continue work in
different ways. Codex app-server exposes a thread-native Goal. Claude Code
2.1.139 and later exposes `/goal`, a session-scoped Stop-hook loop. Kimi ACP
currently has no reviewed equivalent. Treating all of these as
`goal_mode=native|emulated|unsupported` hides the most important fact: who is
allowed to start the next provider cycle.

A live Codex canary demonstrated the failure. Harness activated a native Goal
and also issued `turn/start`. The provider Goal later created another native
Turn while the first remained in progress, so two top-level executions wrote
the same worktree and inherited different permission postures.

## Decision

Star Harness models continuation through two independent concepts:

- **execution driver** — `host_driven`, `provider_driven`, or Workflow-only
  `bounded`;
- **completion policy** — one or more of `member_declared`,
  `provider_evaluator`, `deterministic_check`, and `host_reviewed`.

One MemberRun/native session/writable Workspace has one active top-level
execution driver. A provider-driven continuation may create many native turns
or cycles, but Harness must not start an independent cycle while that provider
mechanism owns the execution lease.

Harness does not create a new Goal object. Work ownership remains the durable
responsibility contract; Mission and Wave remain Host intent and judgment;
provider-native goals, plans, turns, evaluators and subagents remain
provider-owned execution state.

Native continuation is optional. A provider without it uses `host_driven`
mailbox delivery and can still be a complete persistent Agent Team member.
Adapters declare operation-level continuation capabilities and version review
state instead of relying only on a broad `goal_mode` label.

## Consequences

- Codex and Claude native goals can be used without forcing future providers
  to copy their APIs.
- Provider Goal achievement stops provider continuation but does not accept a
  Work.
- Normal TeamMessages stay in the Harness mailbox; busy delivery never
  silently interrupts native continuation.
- Dashboard primary IA centers current Work, driver, continuation state,
  Workspace lease, permissions and PendingInteraction. Native turns remain
  expandable diagnostics.
- Natural-language self-activation is valid only after an Adapter observes the
  native state transition.
- Permission inheritance must be tested across provider-created cycles, not
  only on the first Harness-created cycle.
- Existing `goal_mode` profile fields are insufficient capability summaries
  and must be refined additively before removal or migration.

## Rejected Alternatives

### One Harness Goal object

Rejected because it duplicates Work/Mission intent and falsely suggests
that every provider implements the same lifecycle.

### Always let Harness start turns

Rejected because it disables useful provider-native continuation and can race
with a native Goal.

### Always let providers run autonomously

Rejected because some providers lack observable continuation, safe message
injection, permission continuity or deterministic stop controls.

### Treat turns as the product lifecycle

Rejected because provider definitions of a turn differ. The stable product
boundary is MemberRun + Work + Mailbox + native session + Workspace.

## Validation

Implementation work following this ADR must add:

1. capability snapshots for continuation inspection and control;
2. an execution-driver/lease guard;
3. provider-specific permission-continuity tests;
4. Codex and Claude native-goal live canaries before positive compatibility
   claims;
5. host-driven fallback tests for Kimi and future providers; and
6. CLI/Dashboard projections that distinguish inactive, active, evaluating,
   blocked, satisfied, interrupted and unknown native continuation.

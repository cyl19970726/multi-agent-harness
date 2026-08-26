---
name: shared-references
description: Shared Hard Invariants referenced by team coordination skills. Do not install directly; load as a cross-reference from collaborate-as-agent-team-member.
---

# Shared Hard Invariants

## Local product responsibility views

When the local AgentFirm Dashboard is available, read responsibility state from
the five `agentfirm.role_views.v1` endpoints documented in
`docs/agentfirm-role-views.md`. Treat SSE only as an invalidation signal and
refetch the view. Never rebuild Global-Work/Team/Host/Member/Operator state by
joining raw ledgers, and never treat an `AllowedAction` as authority when it has
a `disabled_reason`.

These rules bind the Host Lead and every Agent Team Member. The Member-facing
skill (`collaborate-as-agent-team-member`) references this file; the Host-facing
`orchestrate-mission-waves` skill was archived by DOC-108. Where this file
and a canonical doc (ADR, schema, product doc) conflict, the canonical doc
wins.

## 1. No Assignment Message Compatibility Path

Agent Team responsibility is only through the shared Works board. There is no
Assignment Message compatibility path and no Harness Goal, Plan Gate, retired
Task Graph, or second task ledger. Current peer Works may form one hard
dependency DAG; only a versioned Work operation changes its edges and only the
kernel derives readiness. A Message is conversation only; never treat one as
responsibility, ownership, status, or a dependency mutation.

## 2. One Execution Driver Per MemberRun

Each active MemberRun/native session/writable Workspace has exactly one
top-level execution driver:

- `host_driven`: Harness starts the next eligible Provider cycle.
- `provider_driven`: a reviewed native continuation loop starts cycles.
- `user_driven`: only for declared `external_interactive` members; a human
  drives their own session.

Never activate a native Goal and also start ordinary Harness cycles for the
same Work. Provider Goal satisfaction, Provider turn completion, transport
receipt, Work submission, and Work acceptance are different facts.

## 3. Provider-Native Session Is Sole Execution Truth

The provider's native session store is the sole execution truth for a member's
transcript, tool calls, commands, file events, and provider turn lifecycle.
Never reconstruct a session from Harness messages.

## 4. Messages Never Change Work State

TeamMessage is authored conversation only. A message may explain scope, a
blocker, a result, or a review decision, but it never changes Work owner or
status. If conversation creates durable follow-up, create self-owned or
unassigned Work explicitly.

## 5. Independent Acceptance Separates Submission From Done

Submission moves Work to `review`; it does not imply acceptance. Ordinary
Member-owned Work requires explicit acceptance by the exact Team Host. A Host
cannot accept its own Work; one exact active non-owner Team peer in the same
TeamRun may accept Host-owned Work. That peer cannot accept another Member's
Work and is not a second durable Reviewer role. Treat `review` as non-terminal:
submission without the authorized explicit acceptance blocks TeamRun
completion.

## 6. Provider-Native Subagents Are Internal Only

A Member may use Provider-native subagents for bounded internal lanes. They
inherit the parent's Workspace and permission ceiling, return evidence to the
parent, and never become Harness Members, own Work, or serve as independent
reviewers.

## 7. Dependencies And Delegation Never Auto-Complete Responsibility

Works are flat peer nodes, never containment nodes. A Work may have many
hard prerequisites and many derived successors. Claim or start only when the
server says every prerequisite is accepted. A failed or cancelled prerequisite
requires Host replan; do not infer downstream failure, cancellation, rewiring,
or acceptance. Cross-Team responsibility uses explicit WorkDelegation. An
accepted prerequisite or delegated target never auto-submits or auto-accepts
another Work; its owner remains accountable for integration and submission.

## 8. No Plan Mode / No Plan Gate

Harness has no Plan Mode or Plan Gate. When the Host wants a plan first, it
asks through an ordinary correlated Markdown message; the Member replies, and
the Host argues or approves in the same chain. Provider-native plan/goal
features are optional internal aids; they are not Harness state or Host
acceptance.

## 9. Work Ownership Survives

Work identity persists across WorkEvent history, re-plans, crashes, and
runtime restarts. Active Work keeps the same Work id, MemberRun, Workspace, and
native session. Never clear ownership, duplicate side effects, or reconstruct a
session from Harness messages after a crash. Host assignment, resume,
request-changes, and rebind are external changes that still arrive as
WorkDelivery.

## 10. Cross-Node Routing Has One Truth

`CompanyNode.id` is the existing `ExecutionNode.id`. A NodeGateway is a child
of the exact current NodeDaemonLease generation, not a second Node authority.
For cross-Node operations, FabricStore `RoutedOperation`, `RouteAttempt`, and
`RouteReceipt` are the sole route truth. Never dual-write or replay from
MessageRouteJournal. A routed Message must carry the canonical immutable
Message envelope or an authenticated content-addressed reference; the target
persists and verifies it before creating MessageDelivery. RouteAttempt proves
transport only; only a generation-fenced target result proves application
effect. Unknown effect requires reconciliation and never permits blind replay.

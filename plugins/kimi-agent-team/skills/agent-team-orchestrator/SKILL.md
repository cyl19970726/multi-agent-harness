---
name: agent-team-orchestrator
description: "Use when deciding whether to form an Agent Team and when running one as the host: when a team beats a sub-agent, how to split waves, the member configuration contract (name:role:provider[:model][@ownedPaths]), handoff/ACK discipline, and the authorization gate (deploy / remote deletion / payment decisions must be escalated to the user, never decided by you or a member). Pair with [[agent-team-member]] for the prompt contract given to launched members."
---

# Agent Team Orchestrator

You are the **host** (Lead) of an Agent Team run. The harness gives you team
formation, assignment, messaging, and observation; this skill is the *method* —
when to form a team, how to split waves, and the discipline that keeps a run
auditable.

## The essential boundary

> A sub-agent is one function call. An Agent Team member is a living collaborator.

A function call takes a task, returns a result, ends, and is stateless to you.
A living collaborator has its own state, its own mailbox, and its own
responsibility domain: it keeps accepting new work, speaks up mid-execution,
and owns an outcome (branch / PR / evidence chain) until acceptance.

The judgment question, asked per unit of work:

> **Does the result need to come back into my context for me to keep using,
> or should it stay with the executor while I keep a pointer?**

- Comes back → use a **sub-agent** (Kimi `Agent` / `AgentSwarm`). Small,
  bounded, one-shot.
- Cannot come back (too large, too long, needs continuous follow-through,
  owns a live artifact like a branch or a deploy) → use an **Agent Team
  member**.

Corollaries, not a feature checklist: granularity, context, communication,
lifecycle, deliverable, and accountability all follow from that one boundary.

## When to form a team (and when not to)

Form a team when several of these hold:

- multiple parallel lanes whose intermediate state cannot fit back into one
  context (handoff docs, test logs, screenshots, deploy state, blockers);
- lanes need a **durable owner** who stays accountable until acceptance, not
  a summary that disappears;
- the work crosses responsibility or trust boundaries (one lane may touch
  infra, another only docs); the current executable member adapter is Kimi ACP,
  while Codex/Claude member adapters remain future work;
- there are external-change authorizations or physically exclusive resources
  (one device, one shared path) that need an enforcement carrier.

Do **not** form a team for: a single lane, a task that fits in one context,
research/lookup questions, or anything whose output you need inline right now.
That is sub-agent territory — cheaper and faster.

## Mission/Wave attempts, not marathons

The product hierarchy is **Mission → ordered Wave → executor**. An
`AgentTeamRun` is one execution attempt for a Wave, not the Wave itself; a
retry creates another run attempt and the Wave gate identifies the accepted
one. A wave boundary is an *integration gate*, not a time limit: it ends when
you complete the integration check and re-plan, not when members go idle.

At every wave boundary run the re-plan loop:

```text
plan vs actual -> deviation -> decision -> next wave plan
```

Deviation is normal input, not an exception. Create or select the native
Mission and Wave before creating the run, then pass their ids to the TeamRun.
The numeric `--wave N` remains a compatibility index for unlinked runs only.

## Creating a run: the member configuration contract

Prefer the MCP Mission/Wave and `team_run_*` tools. Use
`/agent-team:new-run` or the CLI only as a convenience/fallback.
The CLI shape:

```bash
harness mission create \
  --title "Payment reconciliation" \
  --objective "Ship reconciliation safely" \
  --desired-outcome "Verified production-ready slice"
harness wave create \
  --mission-id <mission-id> \
  --title "Implement and review" \
  --objective "Land the payment reconciliation slice behind PR #81" \
  --executor-kind agent_team
harness team-run create \
  --mission-id <mission-id> \
  --wave-id <wave-id> \
  --objective "Land the payment reconciliation slice behind PR #81" \
  --budget-usd 25 \
  --member lead:integrator:kimi \
  --member api:backend:kimi@crates/harness-store,crates/harness-core \
  --member ui:reviewer:kimi@apps/web
```

Member spec grammar: `name:role:provider[:model][@path1,path2]`.

Rules that keep a run sane:

- **ownedPaths are explicit and disjoint.** Two members owning the same path
  is a merge conflict you scheduled on purpose. Shared/integration paths
  belong to the Lead lane or to nobody.
- **Every member gets role + completion standard + evidence requirements +
  permission ceiling** in its prompt (see [[agent-team-member]]).
- **Budget is set at run level** (`--budget-usd`); the harness enforces it.
- A member may freely use its **own provider-native sub-agents**. The harness
  may capture honest attribution through optional hooks, but it never claims
  lifecycle control over those
  delegations — it never schedules them. Do not try to micromanage a member's
  fan-out; that is the member's own context discipline.

## Communication and ACK discipline

Create a `TeamMessage(kind=assignment)` before lane work begins. Its message id
and `correlation_id` are the lane's target work identity. Automatic member
handoff preserves that correlation. Manual CLI/API/MCP sends pass the existing
`correlation_id`, optionally with a same-run `causation_id`; a causation-only
reply inherits its cause's correlation. Assignment-message correlation, not a
parallel planning identifier, proves ownership.

`harness team-run send --id <run> --from host --to <ids> --kind <kind> --body "..." --correlation-id <assignment-correlation> [--causation-id <message-id>]`

- Kinds: `assignment | question | answer | progress | blocker | handoff |
  review_request | review_result | control | broadcast`.
- **Assignments, handoffs, and key messages must be ACKed.** Use
  `team_message_acknowledge` after a delivery reaches `delivered`; queued,
  failed, or expired deliveries cannot be acknowledged. Treat an un-ACKed
  handoff as a first-class alert, not a log line. Automated resend/escalation
  policy is not implemented yet.
- One message, one delivery record per recipient: semantics and delivery are
  separate facts. Check delivery state before assuming you were heard.
- Answer `blocker` messages promptly; a blocked member is burning budget and
  wall-clock while idle.

## The authorization gate — non-negotiable

Deploys, remote deletions, merges to protected branches, payment/plan
choices, and any other **external change must be escalated to the user**.
Neither you nor any member may decide these unilaterally:

- A member that hits one must stop and send a `blocker` message describing
  the exact change and its blast radius.
- You relay it to the user with a clear approve/reject question, and only
  then send the member a `control`/`answer` message with the user's decision.
- "Reasonable default" is not authorization. When in doubt, it is a gate.

## Clear-context working method

Your main thread holds **decisions, not bulk**:

- Hand big chunks of execution to MemberRuns; keep only pointers (Mission/Wave
  context, run id, member ids, assignment correlation ids, evidence refs) in
  your own context.
- Per member, know: status, current assignment, last heartbeat, un-ACKed count —
  not its full transcript. Drill in only on blockers, review requests, and
  handoffs.
- Sub-agent vs member is decided by the boundary question above. Do not
  spawn a sub-agent to "check on" a member — that is what
  `harness team-run status` / `events` are for.

## Observing the run

- `/agent-team:status` renders the compact cockpit table (member / provider /
  status / current action / heartbeat / un-ACKed) from
  `harness team-run status` + `harness team-run events`.
- The CLI text view is a **compact projection of the same truth** the Browser
  Team Console renders — one shared read model, not a second dataset.
- **Every status output includes the exact `dashboard_url` returned by MCP.**
  It carries `surface=team`, the TeamRun id, and the Workspace `project` id;
  this prevents the Console from opening another project's store.
- `harness team-run events --id <run> --after-seq <N>` follows the ordered
  event log; `seq` is monotonic, so resume by remembering the last `seq`.

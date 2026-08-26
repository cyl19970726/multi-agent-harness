---
name: collaborate-as-agent-team-member
description: The one Agent Team collaboration contract for BOTH roles — the Host who orchestrates a durable Team and the persistent Members who execute shared Work. Use whenever an agent creates or runs a Team, decomposes/assigns/reviews/accepts Work, or receives, claims, resumes, executes, blocks, submits Work; reads WorkDelivery or the message Inbox; coordinates Host↔Member or peer↔peer; uses provider-native subagents; or must survive review and runtime restart. The shared mental model in Part I is required reading for both roles — asymmetric mental models are the main way multi-agent collaboration fails.
---

# Agent Team Collaboration — Host & Member Contract

One skill, two roles, one mental model. The Host orchestrates; Members execute.
Every collaboration failure we have observed in real runs came from the two
sides holding different models of the same object — a Host assuming "assigned
means understood", a Member assuming "provider finished means Work done". So
Part I is identical required context for both roles; Parts II/III are the
role-specific operating loops; Part IV is one worked example traced from both
sides.

## Part I — Shared mental model (both roles hold this, verbatim)

### 1. Who you are: identity is three separate facts

```
AgentMember      durable identity   (Active | Paused | Retired)
  └─ TeamMembership   participation in ONE Team, with a role
       role:  Host | Member | Observer
       state: Invited | Active | Leaving | Inactive
  └─ AgentSession     runtime       (Cold | Idle | Active | Waiting |
                                     Interrupted | RecoveryRequired | Closed)
```

You are a durable `AgentMember` acting through exactly one active
`TeamMembership`. Host is a **role on a membership**, not a different kind of
being. Identity outlives every run; participation outlives every session;
sessions outlive every provider turn. Never infer identity from a display
name; the server resolves your identity — identity/runtime authority is
server-built, never caller-selected. (`AgentIdentity` is a deprecated same-ID
read-only compatibility projection of `AgentMember`.)

### 2. The Team: durable, flat, pinned to one machine

A Team is durable and Mission-less (legacy `legacy_mission_id` provenance is
read-only history, DOC-108). It lives on exactly one immutable `node_id`.
Teams are flat peers — no nesting; cross-Team execution is an explicit
Host-coordinated `WorkDelegation`, never hierarchy. Lifecycle:
`Active | Inactive | Trashed`; an Active Team has exactly one Active Host
membership. One machine-scoped NodeDaemon owns every local Team's sessions;
each Team's live run is fenced by a Supervisor generation — whoever starts the
run owns the provider transports, everyone else routes controls to the owner.

### 3. Work: the only responsibility authority

Three orthogonal axes, not one long chain:

```
phase:      Open ──assign/claim──▶ Active ──submit──▶ Review ──Host decides──▶ Closed
condition:  Normal ⇄ Blocked ⇄ OnHold          (overlay; does not change phase)
resolution: Accepted | Cancelled | Failed       (exists only at Closed)
```

- Every change is an ordered, append-only `WorkOperation`/`WorkEvent` — that
  history is the responsibility record, not chat.
- One accountable Team per Work; zero or one assignee TeamMembership.
- Claim/assign/start/submit are **atomic CAS operations with expected
  versions**. `VERSION_CONFLICT` means refresh and re-read; never retry with a
  guessed version. `CLAIM_LOST` means someone else owns it; do not perform its
  side effects.
- **Provider "completed" is not Work "done."** Submission moves Work to
  `review`. Ordinary Member Work needs exact Host acceptance; Host-owned Work
  needs one exact active non-owner Team peer in the same TeamRun because the
  Host cannot self-accept. A green fixture, delivery receipt, or provider
  completion status alone is never acceptance.
- **Assignment never travels by message.** Work assignment is a Work-module
  operation; a Message may explain, ask, or announce — it never changes Work
  owner or status.
- **Work is a flat dependency DAG.** A Work is a peer responsibility node, not
  a container. Claim/start only when server-derived readiness says every hard
  prerequisite is accepted. Messages may propose nodes or edges but never
  mutate them; failed/cancelled prerequisites require Host replan.

### 4. Messages: one authority, honest delivery states

```
Message (immutable, identity-authored)
  → MessageSubscription (admission: who may receive what)
    → CanonicalMessageDelivery (per-recipient state machine, owned by the
      target NodeDaemon)

Queued → Routed → Claimed → ProviderReceived → Acknowledged
                └─────────▶ Failed | Expired | Invalidated
```

- Delivery rows appear **automatically** for admitted recipients — inboxes
  (Team Inbox, member inbox, Host inbox) are projections of deliveries, never
  a second ledger.
- A Team-subject delivery is claimed by one member as an atomic transition on
  the same row; a stale or duplicate claim has zero side effects.
- Ordinary mail is injected at the recipient's **next safe provider cycle**;
  it does not interrupt the current turn. Steer is the separate same-turn
  control. Offline/Detached recipients keep the delivery honestly Queued — no
  invented sessions.
- `informational` intent does not start a provider round by itself; select
  `response-required` only when an answer or action is genuinely needed. This
  is what prevents two agents from bouncing acknowledgement mail forever.
- Transport `Acknowledged` is not a semantic answer. Provider-pausing
  questions and their answers are correlated Messages with exact ids.

### 5. Truth boundaries (what lives where)

| Truth | Owner |
| --- | --- |
| Responsibility, status, dependency DAG, readiness, evidence refs | Work kernel (ordered WorkOperations/WorkEvents) |
| Conversation, decisions-as-text | Message → Subscription → Delivery |
| Transcript, tool calls, thinking | provider-native session (never copied) |
| Session/runtime state, recovery | NodeDaemon + Supervisor generations |
| Inboxes, boards, views | projections — rebuildable, never authoritative |

Fail closed on anything you cannot prove: an unacknowledged interrupt stays
`RecoveryRequired`; an uncertain effect is reconciled, never blindly replayed;
Work ownership survives process exit.

## Part II — What each role must hold to collaborate well

**A Host cannot orchestrate without:** the roster (which AgentMembers, which
providers, which permission ceilings, which disjoint owned paths); bounded
Works with observable completion criteria (a Work a Member cannot verify
finished is a Work the Host cannot review); the board state (who is idle,
working, awaiting review — from `board-summary`, not from memory); the review
discipline (evidence refs, not vibes); and the recovery model (close/reopen
resumes the exact native session at a higher generation — a dead runtime is
not lost work).

**A Member cannot execute without:** its own Work context (What / Mental
Model / Workspace / Boundary / Gates / Evidence — read it fully before side
effects); its exact identity envelope (`FIRM_MEMBER_RUN_ID` etc. — never
substituted); the version of the Work it is mutating; its inbox at safe
boundaries; and the submission contract (result summary + artifact/check refs
matching the declared gates).

**Both fail without the shared model above** — that is why it comes first.

## Part III — Operating loops

Role-specific procedure lives in two references; read yours fully before the
first action of a run:

- Host loop: [references/host-loop.md](references/host-loop.md) — roster
  design, Work decomposition, create/start, watch without polling, answer
  correlated questions, review/accept, recovery, teardown.
- Member loop: [references/member-loop.md](references/member-loop.md) — first
  turn, claim/start, plan-first, converse, block honestly, submit with
  evidence, survive restart.

The gate-checked command shapes both roles share:

```bash
# Host creates an assigned Work (host_assign requires an explicit owner):
firm team-run work create \
  --team-run-id <team-run-id> \
  --title "<one bounded responsibility>" \
  --context "<why it exists; mental model; boundary paths>" \
  --completion-criteria "<observable criteria a reviewer can check>" \
  --claim-mode host_assign \
  --owner-member-run-id <member-run-id> \
  --idempotency-key <stable-command-key>

# Either role creates an open Work for eligible claim:
firm team-run work create \
  --team-run-id <team-run-id> \
  --title "<follow-up responsibility>" \
  --context "<why it exists and relevant evidence>" \
  --completion-criteria "<observable completion criteria>" \
  --claim-mode team_claim \
  --idempotency-key <stable-command-key>
```

Member essentials (full sequence in the member loop reference):

```bash
# start assigned Work (CAS on the freshly read version):
"$FIRM_BIN" member work start \
  --work-id "$FIRM_WORK_ID" \
  --expected-version <version-from-work-show> \
  --idempotency-key <stable-command-key>

# submit with evidence — moves Work to review, never to done:
"$FIRM_BIN" member work submit \
  --work-id "$FIRM_WORK_ID" \
  --expected-version <latest-version> \
  --result-summary "<RESULT/SUMMARY/COVERAGE/WORKTREE/ARTIFACTS template>" \
  --idempotency-key <stable-command-key>
```

To send message or reply, use the authenticated Member Role Action
(`send_message`, `reply_message`, `request_decision`) of the current
server-built view — legacy TeamRun send/ACK commands are retired because they
let a caller select another identity. Do not use provider Plan Mode
(EnterPlanMode/ExitPlanMode) in team context — Harness has no Plan Gate and it
blocks headless members indefinitely (ADR 0039); plan-first means an ordinary
Markdown plan message to the Host.

## Part IV — Worked example: one Work, both sides

The scenario: Host `hana` (Codex app-server) runs Team `builders` with Member
`kiwi` (Kimi ACP). The task: add a laundering-rejection check to the legacy
exporter's `verify`.

```
 HOST hana                                MEMBER kiwi
 ─────────────────────────────────────    ─────────────────────────────────────
 1. work create                           (idle; NodeDaemon holds session)
    --claim-mode host_assign
    --owner-member-run-id kiwi-run
    --completion-criteria "verify exits
    nonzero on a manifest that lists a
    contracted ledger as uncontracted;
    test proves it; PR opened, CI green"
        │
        └─▶ WorkDelivery reaches kiwi ──▶ 2. wakes with FIRM_WORK_ID set:
                                             work show → reads What/Boundary/
                                             Gates; work start --expected-
                                             version 3 → phase Active
                                          3. plan-first: sends Host ONE
                                             response-required Message:
                                             "plan: disjointness check in
                                             verify_archive + tamper test.
                                             OK?"  (work-linked, correlated)
 4. inbox at safe boundary: reads plan,
    replies on the SAME correlation id:
    "proceed; keep names-only"
        │                                 5. implements in own worktree
        │                                    (outside repo checkout); runs
        │                                    the test; commits; opens PR
        │                                 6. hits a doubt mid-way → does NOT
        │                                    go silent and does NOT block yet:
        │                                    informational Message "heads-up:
        │                                    also found excluded-name overlap
        │                                    — created team_claim follow-up
        │                                    Work W-42, not assigning it"
 7. board-summary shows kiwi=working,
    one new open Work W-42. Host does
    NOT poll in a loop — waits on
    events/notifications.
                                          8. work submit --expected-version 5
                                             --result-summary "## RESULT done…
                                             ## WORKTREE …"
                                             --artifact-ref <PR URL>
                                             --check-ref "cargo test -p
                                             firm-cli --test legacy… exit 0"
        │◀── Work → Review ──────────────────┘
 9. reviews EVIDENCE: opens the PR diff,
    reruns the named check, verifies the
    criteria line by line.
10a. accept → phase Closed,
     resolution Accepted.            OR
10b. request-changes with reasons →
     phase back to Active; kiwi
     continues in the SAME MemberRun,
     workspace, and native session —
     no new identity, no lost state.
11. Run teardown: TeamRun completion
    atomically REJECTS any non-terminal
    Work — W-42 must be closed,
    reassigned, or cancelled first.
```

What made this work — the six load-bearing habits: assignment traveled as a
Work operation (never prose); every mutation carried an expected version; the
plan cost one correlated round-trip instead of a wrong implementation; the
side-discovery became an unassigned Work instead of scope creep or a peer
order; the submission carried verifiable evidence so review was a check, not
an argument; and request-changes reused the same session instead of spawning
a fresh agent that would re-learn everything.

## Anti-patterns (each observed in a real run)

- **Polling loops.** `sleep 2` + status in a loop burns the Host's context and
  budget; use board cursors, waits, and event notifications.
- **Ack ping-pong.** Two agents bouncing acknowledgement-only mail; use
  informational intent.
- **Assignment by message.** "Please take W-7" in chat changes nothing and
  desynchronizes the board.
- **Trusting provider completion.** A member's provider saying "done" without
  submitted evidence; Work stays Active until submit, Review until accept.
- **Silent stall.** A blocked member spinning in a provider loop; block the
  Work with a reason and send one decision-shaped Message.
- **Shared-workspace clobber.** Two agents in one worktree; uncommitted state
  is not protected — disjoint owned paths, own worktrees, commit early.
- **Second inbox.** Any private "unread list" ledger drifts from delivery
  truth; inboxes are projections of CanonicalMessageDelivery, full stop.

## Envelope and provenance

The runtime injects the collaboration envelope (`FIRM_BIN`,
`FIRM_TEAM_RUN_ID`, `FIRM_MEMBER_RUN_ID`, `FIRM_SPACE`,
`FIRM_PROJECT`, `FIRM_PROJECT_ID`, and `FIRM_WORK_ID`/`_VERSION` when
Work is delivered). These bind identity and scope; bound commands reject
caller-selected identity. Use the exact `FIRM_BIN` — never another binary
from `PATH`.

Shared hard invariants live in
[`skills/shared-references/SKILL.md`](../shared-references/SKILL.md); when a
rule appears in both, the shared copy is authoritative. When developing Star
Harness itself, product doctrine is canonical in Notion (see
`docs/current/documentation-governance.md`, "Authority boundary"); the
repository files carry the implementation-bound remainder.

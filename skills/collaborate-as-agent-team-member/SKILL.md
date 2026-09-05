---
name: collaborate-as-agent-team-member
description: The one Agent Team collaboration contract for BOTH roles — the Host who orchestrates a durable Team and the persistent Members who execute shared Work. Use whenever an agent creates or runs a Team, decomposes/assigns/reviews/accepts Work, waits for member progress, or receives, claims, resumes, executes, blocks, submits Work; reads WorkDelivery or the message Inbox; coordinates Host↔Member or peer↔peer; uses provider-native subagents; or must survive review and runtime restart. The shared mental model in Part I is required reading for both roles — asymmetric mental models are the main way multi-agent collaboration fails.
---

# Agent Team Collaboration — Host & Member Contract

One skill, two roles, one mental model. The Host orchestrates; Members execute.
Every collaboration failure we have observed in real runs came from the two
sides holding different models of the same object — a Host assuming "assigned
means understood", a Member assuming "provider finished means Work done", a
Host rebuilding a polling loop because it did not know the CLI could wait. So
Part 0 pins the tools, Part I is identical required context for both roles,
Parts II/III are the role-specific operating loops, and Part IV is one worked
example traced from both sides.

## Part 0 — Which binary, which Host mode, which copy of this skill

- **The CLI is `firm`.** Some installs expose the same binary as
  `~/.local/bin/harness`; the commands are identical. A bound Member never
  picks a binary from `PATH` — it uses the exact `$FIRM_BIN` from its
  collaboration envelope. A Host running in its own interactive session uses
  whichever name is installed; examples below say `firm`.
- **A Host runs in one of two modes (ADR 0057).** `managed`: the Host is an
  ordinary MemberRun → AgentSession under the NodeDaemon and is woken by
  deliveries like any member. `external_interactive`: the Host is a user's own
  interactive provider session (a Claude Code, Codex, or Kimi window) bound
  with `--host-surface <surface> --host-thread-id <id>`; Harness creates no
  AgentSession for it and cannot wake it. An external Host learns about
  progress only by asking — `firm team-run wait` inside a turn, then
  `firm team-run host-inbox` for what arrived; nothing pushes into that window.
- **Load the current copy.** There is no plugin package (ADR 0063). Inside
  this repository `.agents/skills/collaborate-as-agent-team-member` links to
  the canonical `skills/` source so Codex, Claude Code (`.claude/skills` →
  `.agents/skills`), and Kimi (`--skills-dir .agents/skills`) members see the
  same file. Elsewhere, `scripts/install-skill.sh --agent both --scope user`
  copies a snapshot that goes stale; refresh or remove such copies when a
  `references/` directory is missing beside them.

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
being; a managed Host and a Member share one MemberRun → AgentSession →
NodeDaemon path. Identity outlives every run; participation outlives every
session; sessions outlive every provider turn. Never infer identity from a
display name; the server resolves your identity — identity/runtime authority
is server-built, never caller-selected. (`AgentIdentity` is a deprecated
same-ID read-only compatibility projection of `AgentMember`.)

### 2. The Team: durable, flat, pinned to one machine

A Team is durable and Mission-less (legacy `legacy_mission_id` provenance is
read-only history, DOC-108). It lives on exactly one immutable `node_id`.
Teams are flat peers — no nesting; cross-Team execution is an explicit
Host-coordinated `WorkDelegation`, never hierarchy. Lifecycle:
`Active | Inactive | Trashed`; an Active Team has exactly one Active Host
membership. One machine-scoped NodeDaemon owns every local Team's sessions;
each Team's live run is fenced by a Supervisor generation — whoever starts the
run owns the provider transports, everyone else routes controls to the owner.
Five persistent execution modes can be members: `codex_app_server`,
`claude_agent_sdk`, `kimi_acp`, `pi_rpc`, `deepseek_sdk`. Bounded one-shot
modes cannot.

### 3. Work: the only responsibility authority

Three orthogonal axes, not one long chain:

```
phase:      Open ──start──▶ Active ──submit──▶ Review ──Host decides──▶ Closed
condition:  Normal ⇄ Blocked ⇄ OnHold          (overlay; does not change phase)
resolution: Accepted | Cancelled | Failed       (exists only at Closed)
```

- Every change is an ordered, append-only `WorkOperation`/`WorkEvent` — that
  history is the responsibility record, not chat.
- One accountable Team per Work; zero or one assignee TeamMembership.
- Assign/claim freezes stable responsibility while Work remains Open; only an
  exact scheduler admission (`WorkExecutionBinding` + `WorkDelivery` bound to
  the member's current AgentSession generation) followed by Start moves it to
  Active.
- Claim/assign/start/submit are **atomic CAS operations with expected
  versions**. `VERSION_CONFLICT` means refresh and re-read; never retry with a
  guessed version. `CLAIM_LOST` means someone else owns it; do not perform its
  side effects.
- `DELIVERY_NOT_DISPATCHED` is transient: wait for the next Supervisor pass and
  retry instead of escalating immediately. `MEMBER_BUSY` means you already hold
  one active Work.
- An Open, never-started Work whose delivery is frozen on a member generation
  that no longer runs (typically after close-member + reopen-member) needs the
  Host's `work redeliver`; nothing else revives it.
- **Provider "completed" is not Work "done."** Submission moves Work to
  `Review`. Ordinary Member Work needs exact Host acceptance; Host-owned Work
  needs one exact active non-owner Team peer in the same TeamRun because the
  Host cannot self-accept. A green fixture, delivery receipt, or provider
  completion status alone is never acceptance. Request-changes returns
  Review → Open; the scheduler then admits the next exact
  binding/delivery generation before the member can Start again.
- **Acceptance does not wake a member.** A managed member's next provider
  cycle starts only from three wake sources: a delivery of an assigned or
  redelivered Work, a `response-required` Message, or the member's own
  active Work. Host acceptance moves a Work to `Closed`; it never starts
  the member's next cycle. So never park a Work `blocked` expecting
  acceptance — or another Work's completion — to resume it. Keep the
  standing Work active while running round Works when the one-Active-Work
  rule allows; when parking is unavoidable, the block note must name the
  exact Host action you wait for ("resume me with a response-required
  message after W-x is accepted") so the Host can send the wake
  deliberately.
- **Assignment never travels by message.** Work assignment is a Work-module
  operation; a Message may explain, ask, or announce — it never changes Work
  owner or status.
- **Work is a flat dependency DAG.** A Work is a peer responsibility node, not
  a container. Claim/start only when server-derived readiness says every hard
  prerequisite is accepted. Only the Host mutates edges
  (`work replace-dependencies`); Members may propose nodes or edges in a
  Message; failed/cancelled prerequisites require Host replan.

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
  A response-required Message wakes an exact idle **managed** recipient; it
  cannot wake an external_interactive Host, which must read its inbox itself.
- Transport `Acknowledged` is not a semantic answer. Provider-pausing
  questions and their answers are correlated Messages with exact ids
  (`ProviderInteractionRequest` / `ProviderInteractionResponse`); the Host
  answers with `team-run answer-message`.

### 5. Truth boundaries (what lives where)

| Truth | Owner |
| --- | --- |
| Responsibility, status, dependency DAG, readiness, evidence refs | Work kernel (ordered WorkOperations/WorkEvents) |
| Conversation, decisions-as-text | Message → Subscription → Delivery |
| Transcript, tool calls, thinking | provider-native session (never copied) |
| Session/runtime state, recovery | NodeDaemon + Supervisor generations |
| Inboxes, boards, views, `team-run events` | projections — rebuildable, never authoritative |

Fail closed on anything you cannot prove: an unacknowledged interrupt stays
`RecoveryRequired`; an uncertain effect is reconciled, never blindly replayed;
Work ownership survives process exit.

## Part II — What each role must hold to collaborate well

**A Host cannot orchestrate without:** the roster (which AgentMembers, which
providers, which permission ceilings, which disjoint owned paths); bounded
Works with observable completion criteria (a Work a Member cannot verify
finished is a Work the Host cannot review); the board state (who is idle,
working, awaiting review — from `board-summary`, not from memory); **a way to
wait that is not a sleep loop** (`team-run wait`, see Part III); the review
discipline (evidence refs, not vibes); and the recovery model (close/reopen
resumes the exact native session at a higher generation — a dead runtime is
not lost work).

**A Member cannot execute without:** its own Work context (What / Mental
Model / Workspace / Boundary / Gates / Evidence — read it fully before side
effects); its exact identity envelope (`FIRM_MEMBER_RUN_ID` etc. — never
substituted); the version of the Work it is mutating; its inbox at safe
boundaries; and the submission contract (result summary + artifact/check refs
matching the declared gates).

A member with no standing Work waits for an assigned Work instead of creating one.

**Both fail without the shared model above** — that is why it comes first.

## Part III — Operating loops

Role-specific procedure lives in two references; read yours fully before the
first action of a run:

- Host loop: [references/host-loop.md](references/host-loop.md) — Host mode,
  roster design, Work decomposition, create/start, wait without polling,
  answer correlated questions, review/accept/redeliver, recovery, teardown.
- Member loop: [references/member-loop.md](references/member-loop.md) — first
  turn, claim/start, plan-first, converse through the CLI, block honestly,
  submit with evidence, survive restart.

The gate-checked command shapes both roles share:

```bash
# Host creates a Work that only explicit membership assignment may claim:
firm team-run work create \
  --team-run-id <team-run-id> \
  --title "<one bounded responsibility>" \
  --context "<why it exists; mental model; boundary paths>" \
  --completion-criteria "<observable criteria a reviewer can check>" \
  --claim-mode host_assign \
  --idempotency-key <stable-command-key>
firm team-run work assign \
  --work-id <work-id> \
  --expected-version <created-version> \
  --membership-id <team-membership-id> \
  --idempotency-key <stable-command-key>

# Either role creates an open Work for eligible claim
# (a Member adds --as-member-run-id "$FIRM_MEMBER_RUN_ID" so provenance is its own):
firm team-run work create \
  --team-run-id <team-run-id> \
  --title "<follow-up responsibility>" \
  --context "<why it exists and relevant evidence>" \
  --completion-criteria "<observable completion criteria>" \
  --claim-mode team_claim \
  --idempotency-key <stable-command-key>
```

**Waiting protocol (Host).** Never `sleep` + `status` in a loop. Block on the
run's event stream and chain cursors:

```bash
# block until something happens on the run (or the timeout elapses):
firm team-run wait --id <team-run-id> --after-seq <last-seq> --timeout-secs 600 --json
# then read only what changed (next_since comes from the previous JSON `work list` response):
firm team-run work list --team-run-id <team-run-id> --since <next_since>
firm team-run board-summary --id <team-run-id>
# external_interactive Host: your mail is not pushed mid-turn — read it:
firm team-run host-inbox --surface <surface> --thread-id <id> --json
```

`wait` returns `timed_out`, `after_seq`, `next_after_seq`, and the new events;
pass `next_after_seq` back as `--after-seq`. Omitting `--after-seq` means
"wait for what happens next", not "replay history". If `wait` cannot express
what you need, file the gap as a repository Issue **before** scripting around
it; a bypass without an Issue hides a product defect.

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
  --result-summary "<Verbatim evidence first — see the submission report contract below>" \
  --candidate-revision <exact-commit-sha> \
  --artifact-ref <PR URL> --check-ref "<command and actual result>" \
  --idempotency-key <stable-command-key>

# read mail, then converse through the authenticated Member Role Action:
"$FIRM_BIN" member inbox --json
"$FIRM_BIN" member message send --recipient-agent-id <agent-member-id> \
  --body "<markdown>" [--work-id <discussed-work-id>] [--response-required]
"$FIRM_BIN" member message reply --recipient-agent-id <agent-member-id> \
  --correlation-id <incoming-correlation> --causation-id <incoming-message-id> \
  --body "<markdown>" [--work-id <incoming-work-id>] [--response-required]
"$FIRM_BIN" member message request-decision --body "<decision, options, recommendation>"
```

While blocked, remember acceptance does not wake you: name the exact Host
action you wait for in the block note; the resume arrives as a Work
delivery or a `response-required` Message, never from acceptance itself.

These Member commands are the authenticated Member Role Action of the
server-built member view: the server resolves your AgentMember, current
AgentSession generation, TeamMembership, Work scope, and NodeDaemon generation
from the envelope; you never supply them. An `external_interactive` Host
authors intra-Team mail with `firm team-run message send --team-run-id <run>
--to-membership <membership> --body <markdown> --surface <surface>
--thread-id <id> [--work-id <id>] [--response-required]`, and answers a
provider question with `firm team-run answer-message --id <run> --message-id
<id> (--option-id <id> | --response-text <text>)`. Legacy TeamRun send/ACK
commands are retired because they let a caller select another identity.
Do NOT use provider Plan Mode (EnterPlanMode/ExitPlanMode) in team context —
Harness has no Plan Gate and it blocks headless members indefinitely (ADR
0039); plan-first means an ordinary Markdown plan message to the Host.

### Submission report contract (READY_FOR_REVIEW)

- **A Work's explicit report requirement always wins** over any provider-side
  or personal template — including any template in this skill's references.
  Templates may ADD sections after the evidence section; they never replace
  it.
- Every READY_FOR_REVIEW `--result-summary` starts with one **Verbatim
  evidence** section, in this order:
  1. the exact commit SHA (full 40 hex);
  2. `git diff --stat <base>...<sha>` — three-dot, against the base the Work
     names;
  3. the literal `git status --porcelain` output — state `empty` explicitly
     when there is none;
  4. for every gate the Work names: the exact command line, its verbatim
     final result line(s), and the captured exit code. A summary sentence
     ("all gates passed") is never acceptable as gate evidence.

Short example (one gate shown; list every gate the Work names):

```text
SHA 76763afa4e807e470ce88b57d41e75dd2cc7bfe6 on r5c-kimi-followups-795 (base origin/master e7497697).
git diff --stat origin/master...HEAD:
 crates/firm-cli/src/supervisor_wake.rs | 27 +++++++++++++++++++++++
 1 file changed, 27 insertions(+)
git status --porcelain: empty
Gates: cargo fmt --all -- --check: FMT_EXIT=0; cargo test -p firm-cli --bin firm -- --test-threads=1 recover_classifier_and_wake_loop: "test result: ok. 1 passed; 0 failed" / TEST_EXIT=0
```

The Host side of this contract is in
[references/host-loop.md](references/host-loop.md) §5; the Member-side
submission section is in
[references/member-loop.md](references/member-loop.md).

## Part IV — Worked example: one Work, both sides

The scenario: Host `hana` (Codex app-server, managed) runs Team `builders`
with Member `kiwi` (Kimi ACP). The task: add a laundering-rejection check to
the legacy exporter's `verify`.

```
 HOST hana                                MEMBER kiwi
 ─────────────────────────────────────    ─────────────────────────────────────
 1. work create                           (idle; NodeDaemon holds session)
    --claim-mode host_assign
    work assign --membership-id kiwi-membership
    --completion-criteria "verify exits
    nonzero on a manifest that lists a
    contracted ledger as uncontracted;
    test proves it; PR opened, CI green"
        │
        └─▶ WorkDelivery reaches kiwi ──▶ 2. wakes with FIRM_WORK_ID set:
                                             work show → reads What/Boundary/
                                             Gates; work start --expected-
                                             version 3 → phase Active
                                          3. plan-first: member message send
                                             --response-required to hana:
                                             "plan: disjointness check in
                                             verify_archive + tamper test.
                                             OK?"  (work-linked, correlated)
 4. team-run wait returns the delivery
    event; reads plan, replies on the
    SAME correlation id: "proceed"
        │                                 5. implements in own worktree
        │                                    (outside repo checkout); runs
        │                                    the test; commits; opens PR
        │                                 6. hits a doubt mid-way → does NOT
        │                                    go silent and does NOT block yet:
        │                                    informational message "heads-up:
        │                                    also found excluded-name overlap
        │                                    — created team_claim follow-up
        │                                    Work W-42, not assigning it"
 7. board-summary shows kiwi=working,
    one new open Work W-42. Host does
    NOT poll in a loop — blocks on
    team-run wait --after-seq <cursor>.
                                          8. work submit --expected-version 5
                                             --result-summary "<Verbatim
                                             evidence first: SHA, three-dot
                                             diff stat, porcelain, gate
                                             commands + verbatim result
                                             lines + exit codes>"
                                             --candidate-revision <commit sha>
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
     Review → Open; stable kiwi
     responsibility remains. Scheduler
     admits the next exact
     binding/delivery generation before
     Start. Compatible workspace and
     native-session continuity may
     resume; it is not runtime ownership.
11. Run teardown: team-run complete
    atomically REJECTS any non-terminal
    Work — W-42 must be closed,
    reassigned, or cancelled first.
```

What made this work — the six load-bearing habits: assignment traveled as a
Work operation (never prose); every mutation carried an expected version; the
plan cost one correlated round-trip instead of a wrong implementation; the
side-discovery became an unassigned Work instead of scope creep or a peer
order; the submission carried verifiable evidence so review was a check, not
an argument; and request-changes preserved stable responsibility while a new
exact execution admission safely reused compatible native context.

## Anti-patterns (each observed in a real run)

- **Polling loops.** `sleep 15` + `events`/`work list` in a loop — in the
  foreground or in a background watcher — burns the Host's context and budget
  and hides whether `team-run wait` is sufficient. Block on `wait`, chain
  `--after-seq` / `--since` cursors, read the inbox at boundaries.
- **Silent bypass.** Scripting around a CLI capability without filing the gap
  as an Issue; the dogfood signal is lost and the workaround becomes the
  de facto product.
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
- **Stale skill copy.** Reading an old snapshot of this skill (no
  `references/` beside it) and following retired commands.

## Envelope and provenance

The runtime injects the collaboration envelope (`FIRM_BIN`,
`FIRM_TEAM_RUN_ID`, `FIRM_MEMBER_RUN_ID`, `FIRM_SPACE`,
`FIRM_PROJECT`, `FIRM_PROJECT_ID`, and `FIRM_WORK_ID`/`FIRM_WORK_VERSION` when
Work is delivered) plus the bearer capability `FIRM_MEMBER_ROLE_ACTION_TOKEN`
that authenticates every Member Role Action against the exact live
Supervisor. These bind identity and scope; bound commands reject
caller-selected identity. Use the exact `FIRM_BIN` — never another binary
from `PATH`. Never print, log, copy, or forward the token; it expires with the
Supervisor registration and is reissued on Close/Reopen.

Shared hard invariants live in
[`skills/shared-references/SKILL.md`](../shared-references/SKILL.md); when a
rule appears in both, the shared copy is authoritative. When developing Star
Harness itself, product doctrine is canonical in Notion (see
`docs/current/documentation-governance.md`, "Authority boundary"); the
repository files carry the implementation-bound remainder.

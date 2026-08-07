# Agent Team Mental Model
status: SUPERSEDED — canonical source is now docs/mental/agent-firm-mental-model.md

```text
status: canonical product reference
owner_role: product
canonical_for: Agent Team operational model — stores, events, states, wake/delivery, message push
```

This is the complete mental model for the Agent Team system. Read it once,
then keep it as a reference. The Lead skill (`orchestrate-mission-waves`)
and Member skill (`collaborate-as-agent-team-member`) both assume you
understand this model.

---

<!-- BEGIN-SKILL-SUMMARY -->
## Keep One Small Mental Model

```text
Store          = append-only JSONL, sole source of truth
Event          = every state change, monotonically sequenced (seq)
Daemon         = detached process, owns delivery loop + heartbeat
Work           = durable responsibility, owner, status, result, acceptance
TeamMessage    = authored conversation, optionally linked to one Work
Member         = autonomous worker, one turn = one round, idles after each
Host (Lead)    = decision-maker: review, accept, assign, close, re-plan
```

**Work states**: `open → assigned → in_progress → review → done` (Host accepts) or `→ cancelled`
**Message states**: `queued → claimed → delivered` (daemon handles)
**Member states**: `queued → running → idle` (normal breathing) or `→ stopped` (Host closes)
**Host receives**: hook auto-injects host-inbox at turn start — no polling needed
**Member receives**: daemon injects messages + work into CONTRACT prompt each round
**External member receives**: hook reads inbox → injects into session context
<!-- END-SKILL-SUMMARY -->

## 1. One Sentence

> **Store is truth. Event is history. Daemon executes. Member works. Lead decides.
> The system is asynchronous turn-based — member finishes a round and goes idle,
> Lead wakes up to review events and decide, daemon reliably delivers.**

---

## 2. Architecture Layers

```
┌─ Lead Session ────────────────────────────────────────────────────────┐
│                                                                       │
│  Each turn starts: hook reads host-inbox → member messages appear     │
│                                                                       │
│  1. Review new messages (injected automatically)                      │
│  2. board-summary (≤500 chars, instant overview)                      │
│  3. Process: accept submitted work / reply to questions / handle      │
│     blockers / close+reopen failed members                            │
│  4. Create new works if supply is low                                 │
│                                                                       │
│  team-run wait: optional, for scripting/automation only               │
│                                                                       │
└───────────────────────────────────┬───────────────────────────────────┘
                                    │
                              CLI / MCP
                                    │
┌───────────────────────────────────▼───────────────────────────────────┐
│  Durable Store (JSONL, append-only, sole source of truth)             │
│                                                                       │
│  team_run_events.jsonl    ← every event, monotonically sequenced      │
│  works.jsonl              ← work versions (open→in_progress→review→done)│
│  team_messages.jsonl      ← message state (queued→claimed→delivered)  │
│  member_runs.jsonl        ← member status (queued→running→idle→stopped)│
│  team_runs.jsonl          ← run status (planning→running→completed)   │
│  pending_interactions.jsonl ← ExitPlanMode / approvals                │
│  team_supervisor_leases.jsonl ← daemon heartbeat + generation         │
└───────────────────────────────────┬───────────────────────────────────┘
                                    │
                              Daemon reads
                                    │
┌───────────────────────────────────▼───────────────────────────────────┐
│  Daemon (detached process, one process manages N team-runs)           │
│                                                                       │
│  For each active member:                                              │
│    1. Has queued messages? → claim → inject into CONTRACT prompt      │
│    2. Has pending work? → build CONTRACT prompt → ACP turn            │
│    3. Neither? → sleep                                                │
│                                                                       │
│  CONTRACT prompt = WORK context + TEAM ROSTER + exact CLI commands    │
│  Heartbeat every TTL/3 seconds. Lease expiry = daemon is dead.        │
└───────────────────────────────────┬───────────────────────────────────┘
                                    │
                              ACP / App Server
                                    │
┌───────────────────────────────────▼───────────────────────────────────┐
│  Member (kimi/deepseek, 1 turn = 1 round, always idles after)         │
│                                                                       │
│  Turn start → read CONTRACT prompt → do work → tool calls →          │
│  submit/block/ask → idle                                              │
│                                                                       │
│  Next wake → daemon checks: messages? work? → new turn                │
└───────────────────────────────────────────────────────────────────────┘
```

---

## 3. Work State Machine

```
Host creates work
      │
      ▼
   ┌──────┐  member claims  ┌──────────┐  member starts  ┌─────────────┐
   │ open │────────────────▶│ assigned │────────────────▶│ in_progress │
   └──────┘                 └──────────┘                  └──────┬──────┘
                                                                │
                           ┌────────────────────────────────────┤
                           │ member blocks                      │ member submits
                           ▼                                    ▼
                      ┌─────────┐                          ┌────────┐
                      │ blocked │                          │ review │
                      └────┬────┘                          └───┬────┘
                           │                                   │
                           │ resume                       ┌────┴────┐
                           ▼                         ┌────┴────┐    │
                      in_progress                  Host       Host    │
                                                 accept  request-     │
                                                   │    changes       │
                                                   ▼       │         │
                                              ┌──────┐     │         │
                                              │ done │     ▼         │
                                              └──────┘ in_progress   │
                                                      (fix & resubmit)│
                                                                     │
                                                                     │
                              Host or member cancels (any state) ────┴──▶ cancelled
```

**Every state transition → one WorkEvent → stored in works.jsonl as a new version.**

- `review` is non-terminal — it blocks team-run completion until Host acts
- A member in `review` cannot be assigned new work until current is accepted
- Work is durable: survives daemon crash, session restart, replanning

---

## 4. Message State Machine

```
Host/Member sends message
      │
      ▼
  ┌────────┐  daemon claims   ┌─────────┐  provider confirms  ┌───────────┐
  │ queued │─────────────────▶│ claimed │───────────────────▶│ delivered │
  └────────┘                  └─────────┘                    └───────────┘
                                                                   │
                                                             Lead ACKs
                                                             (dequeue)
```

- `queued` → message written to store, daemon hasn't touched it yet
- `claimed` → daemon owns this delivery (generation-fenced)
- `delivered` → provider confirmed receipt (kimi: `kimi-acp-prompt:N`)
- Host manual ACK available but not required for delivery to complete
- **ACK means transport receipt, NOT semantic understanding**

**Per-provider message boundaries:**

| Provider | Boundary | Meaning |
|---|---|---|
| kimi | next_round_batched | Message delivered at start of NEXT round |
| claude | in_turn | Message delivered within current turn |
| codex | next_round | Message delivered at next round boundary |

---

## 5. Member Lifecycle

```
Daemon starts provider session
      │
      ▼
  ┌────────┐  provider ready  ┌─────────┐
  │ queued │─────────────────▶│ running │◀──────────┐
  └────────┘                  └────┬────┘           │
                                   │                │
                             turn completes     member resumes
                                   │                │
                                   ▼                │
                              ┌─────────┐     ┌─────┴─────┐
                              │  idle   │────▶│  running  │
                              └────┬────┘     └───────────┘
                                   │
                              Host closes
                                   │
                                   ▼
                              ┌─────────┐
                              │ stopped │
                              └─────────┘
```

**Critical**: `running → idle → running` is **normal breathing**, NOT an error.
- kimi members always idle after each turn
- idle members are woken by daemon when there's work or messages
- closed members can be reopened (session-resume), retaining work ownership
- retired members are permanent — work must be reassigned

**Failure recovery ladder:**

```
L0: Diagnose    → team-run status + daemon supervisor status
L1: Restart     → team-run start (spawns/adopts daemon)
L2: Stop+start  → daemon supervisor stop + team-run start
L3: Per-member  → close-member + reopen-member + start
L4: Nuclear     → team-run cancel + recover + start
```

---

## 6. Event Mechanism

All state changes produce events in `team_run_events.jsonl` with monotonic `seq`.

**Event producers:**

| Operation | Triggered by | Event |
|---|---|---|
| work create | Host CLI | work:created |
| work claim/start | Member CLI | work:started |
| work submit | Member CLI | work:submitted |
| work accept | Host CLI | work:accepted |
| work cancel | Host CLI | work:cancelled |
| work block/resume | Member CLI | work:blocked / work:resumed |
| message send | Host/Member CLI | message:created |
| message delivered | Daemon | message:updated (delivered) |
| member running | Daemon | member_run:updated (running) |
| member idle | Daemon | action:completed (round N) |
| member stopped | Daemon/Host | member_run:updated (stopped) |
| supervisor started | Daemon | team_run:updated |

**Reading events:**

```bash
# Incremental (since last seen seq)
harness team-run events --id <run> --after-seq <last-seq>

# Blocking wait (for scripting)
harness team-run wait --id <run> --after-seq <last-seq> --timeout-secs 60
```

---

## 7. Message Push Model

How messages reach each role's session context:

```
┌─────────────────────────────────────────────────────────────────┐
│                    Message Push Model                           │
│                                                                 │
│  Driven Member                                                  │
│    Host sends → store (queued) → daemon claims →                │
│    CONTRACT prompt injection → member sees message              │
│                                                                 │
│  External Member                                                │
│    Host/peer sends → store → hook reads inbox →                 │
│    injects into member session context                          │
│                                                                 │
│  Host (Lead) 🆕                                                  │
│    Member sends → store (delivered) → hook reads host-inbox →   │
│    injects into Host session context at turn start              │
│                                                                 │
│  Host work events                                               │
│    board-summary / events --after-seq (instant, no polling)     │
│    team-run wait (optional, for scripts)                        │
└─────────────────────────────────────────────────────────────────┘
```

**Key insight**: No role needs to poll `team-run wait` in normal operation.
- Host: hook auto-injects messages at turn start + board-summary for work status
- Member: daemon injects in CONTRACT prompt each round
- External member: hook injects inbox at session start

---

## 8. Host Operational Loop

What a Host agent does each turn:

```
Turn start (hook auto-injects host-inbox messages):

1. Review new messages
   - QUESTION → reply with decision
   - BLOCKER → unblock or reassign
   - PROGRESS → note, no action needed

2. board-summary (≤500 chars)
   - review count > 0 → accept or request-changes
   - blocked members → diagnose (L0-L4)
   - idle members > ready works → create more works
   - zero ready works → decompose next tranche

3. Process review queue FIRST (blocks downstream)
   harness team-run work show --work-id <id>
   → verify against criteria
   → accept OR request-changes with specific reason

4. Create works if supply low
   harness team-run work create --claim-mode team_claim

5. Record judgment if material decision was made
   harness mission log append --kind judgment --body "..."
```

**Never**:
- Treat TeamMessage as work status
- Accept work without verifying criteria
- Leave review queue unprocessed (blocks team-run completion)
- Assume member silence = failure (idle is normal)

---

## 9. Common Pitfalls

| Pitfall | Why it happens | How to avoid |
|---|---|---|
| Messages queue silently | Forgot `bind-host` | Always bind-host before start |
| Member stuck in plan mode | kimi native plan mode + no approval | CONTRACT prompt bans EnterPlanMode (fixed) |
| Host reply not seen by member | next_round_batched timing | Normal — wait 1-2 rounds. Message priority fix ensures delivery |
| Work stays in review forever | Host forgot to check | board-summary at turn start catches this |
| Member idle treated as failure | kimi always idles after turn | Check events — idle then running is normal breathing |
| Creating work in chat instead of board | Natural language habit | Works only via CLI; messages are conversation only |
| Daemon dead, members silent | Lease expired, daemon crashed | Run team-run start to spawn new daemon (L1 recovery) |
| Provider version blocked | New kimi version not in reviewed list | Run member providers --fail-on-review, add version, reinstall |

---

## 10. Commands Quick Reference

```bash
# Host
harness team-run bind-host --id <run> --surface kimi-code --thread-id <id>
harness team-run start --id <run>
harness team-run board-summary --id <run>
harness team-run events --id <run> --after-seq <last>
harness team-run host-inbox --surface <s> --thread-id <id> --json
harness team-run work create --team-run-id <run> --title "..." --claim-mode team_claim
harness team-run work accept --work-id <id> --expected-version <v>
harness team-run work request-changes --work-id <id> --reason "..."
harness team-run send --id <run> --from host --to <member> --kind message --body "..."
harness team-run close-member --id <run> --member-run-id <id> --reason "..."
harness team-run reopen-member --id <run> --member-run-id <id>
harness daemon supervisor status --team-run-id <run>
harness daemon supervisor stop --team-run-id <run>

# Member (injected via CONTRACT prompt — HARNESS_* vars are pre-set)
"$HARNESS_BIN" team-run work start --team-run-id "$HARNESS_TEAM_RUN_ID" --work-id "$HARNESS_WORK_ID" --member-run-id "$HARNESS_MEMBER_RUN_ID" --expected-version <v>
"$HARNESS_BIN" team-run work submit --team-run-id "$HARNESS_TEAM_RUN_ID" --work-id "$HARNESS_WORK_ID" --member-run-id "$HARNESS_MEMBER_RUN_ID" --expected-version <v> --result "<summary>"
"$HARNESS_BIN" team-run inbox --id "$HARNESS_TEAM_RUN_ID" --member-run-id "$HARNESS_MEMBER_RUN_ID" --all --json
"$HARNESS_BIN" team-run send --id "$HARNESS_TEAM_RUN_ID" --from "$HARNESS_MEMBER_RUN_ID" --to host --kind message --body "QUESTION: ..."
"$HARNESS_BIN" team-run board-summary --id "$HARNESS_TEAM_RUN_ID"

# Maintenance
bash scripts/manage-star-harness-install.sh --check
bash scripts/manage-star-harness-install.sh --apply
```

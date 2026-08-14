---
name: collaborate-as-agent-team-member
description: Use when a persistent Agent Team Member receives, claims, resumes, executes, blocks, or submits shared Work; reads its WorkDelivery and message Inbox; coordinates with the Host or peers; uses provider-native subagents; or survives review and runtime restart. Do not use for Host orchestration or a one-shot internal subagent.
---

# Collaborate As An Agent Team Member

## Member Workbench projection

For local product operation, read `GET
/v1/views/member-workbench/{member_run_id}` using the authenticated
AgentMember capability. A different actor must receive `NOT_AUTHORIZED`. The
view contains outcome Work, messages/deliveries, Workspace/NativeSession health
and Report/Finding/Failure/Gate history; private execution mechanics are not
product Work.

Own one shared-board Work end to end. This skill is a procedural capability, not
product authority. You are a durable AgentIdentity participating through one
exact active TeamMembership. The machine-local NodeDaemon owns your current
AgentSession and provider thread; Work responsibility is frozen separately in
WorkExecutionBinding. MemberRun and Workspace rows are coordination/history
projections, not provider runtime authority. Your Provider-native subagents are
implementation details.

Use the exact `HARNESS_BIN` and identifiers supplied by the collaboration
envelope. Do not substitute another binary from `PATH` or infer identity from a
display name.

These hard invariants apply to every Host and Member. The full shared text lives in [`skills/shared-references/SKILL.md`](../shared-references/SKILL.md); when a rule appears in both skills, the shared copy is authoritative. The rules below are the Member-specific application.

## Quick Start: First Turn

When you wake up as a new member with a Work assignment, the daemon has already
delivered your Work context and set these env vars. Run these exact commands
(paste them — do not retype):

```bash
# 1. See your Work
"$HARNESS_BIN" team-run work show --work-id "$HARNESS_WORK_ID" --json

# 2. Mark it in progress (version from step 1)
"$HARNESS_BIN" team-run work start \
  --team-run-id "$HARNESS_TEAM_RUN_ID" \
  --work-id "$HARNESS_WORK_ID" \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" \
  --expected-version <version-from-step-1> \
  --idempotency-key "start-$(date +%s)"

# 3. Check for messages from Host or peers
"$HARNESS_BIN" team-run inbox \
  --id "$HARNESS_TEAM_RUN_ID" \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" \
  --all --json

# 4. Read the board to see other members' status
"$HARNESS_BIN" team-run board-summary --id "$HARNESS_TEAM_RUN_ID"
```

## Start From Work, Not Chat

Your Work comes with a mini mental model. Parse it in one pass:

```
Work context structure (written by Lead):
┌─ What ──────────┐  ← The problem, in one sentence
├─ Mental Model ──┤  ← States, invariants, data flow. Study this.
├─ Workspace ─────┤  ← Your cwd is already set up. Work here.
├─ Boundary ──────┤  ← Paths to touch / NEVER touch. Respect this.
├─ Gates ─────────┤  ← Verification gates that must pass. Deliver what
│                    each gate needs (PR merge, artifacts, checks).
├─ Evidence ──────┤  ← What counts as done. Deliver this.
└─────────────────┘

Work criteria (written by Lead):
  Acceptance checklist. Match every item before submitting.
```

Before any side effect, read the full Work with:

```bash
"$HARNESS_BIN" team-run work show --work-id "$HARNESS_WORK_ID" --json
```

Then confirm these facts from the output:

Read the board and exact Work:

```bash
"$HARNESS_BIN" team-run work list \
  --team-run-id "$HARNESS_TEAM_RUN_ID"
"$HARNESS_BIN" team-run work show \
  --work-id "$HARNESS_WORK_ID"
```

The board is the sole responsibility/status authority. TeamMessage is conversation only — see shared hard invariants §1 (no Assignment Message compatibility path) and §4 (messages never change Work state).

For a compact board overview when context is limited:

```bash
"$HARNESS_BIN" team-run board-summary --id "$HARNESS_TEAM_RUN_ID"
"$HARNESS_BIN" team-run work list --team-run-id "$HARNESS_TEAM_RUN_ID" --brief
"$HARNESS_BIN" team-run work list --team-run-id "$HARNESS_TEAM_RUN_ID" --since <cursor>
```

`board-summary` prints a ≤500-character summary: open/in-progress/blocked/review/done/cancelled counts plus each Member's idle/working/awaiting-review state. `--brief` prints one plain-text line per Work. `--since` takes a monotonic cursor from a prior `list` response and returns only new or updated Works.

## Claim Or Start Exactly One Work

For a ready unassigned Work you are eligible to take, atomically claim it:

```bash
"$HARNESS_BIN" team-run work claim \
  --team-run-id "$HARNESS_TEAM_RUN_ID" \
  --work-id <work-id> \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" \
  --expected-version <latest-version> \
  --idempotency-key <stable-command-key>
```

For Work already assigned to you, explicitly start it:

```bash
"$HARNESS_BIN" team-run work start \
  --team-run-id "$HARNESS_TEAM_RUN_ID" \
  --work-id "$HARNESS_WORK_ID" \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" \
  --expected-version "$HARNESS_WORK_VERSION" \
  --idempotency-key <stable-command-key>
```

Refresh the Work after any `VERSION_CONFLICT`. Never retry with a guessed
version. `CLAIM_LOST` means another Member owns the latest Work; do not perform
its side effects.

A successful self-claim is already responsibility possession inside this
bound MemberRun/native turn. It records the `claimed` WorkEvent and returns the
new Work version; it does not send a WorkDelivery back to yourself. After a
runtime restart, continue the same `in_progress` Work only through the same
stable AgentIdentity, active TeamMembership, exact WorkExecutionBinding, and
current AgentSession generation. Inspect native history and the Workspace first,
and never invent a provider receipt. Host assignment,
resume, request-changes, and rebind are external changes and still arrive as
WorkDelivery.

V1 permits one active `in_progress` Work per Member unless a concrete capacity
profile says otherwise. You may own several open Works but must not start two
top-level cycles in one native session or writable Workspace.

## Own Your Internal Plan

Translate the current Work into your own design, implementation, and verification plan. Provider-native plan/goal features are optional internal aids; they are not Harness state or Host acceptance — see shared hard invariants §8 (no Plan Mode/Gate). When the Host asks for a plan first, reply with concise Markdown in a Work-linked conversation, address revisions, and execute only after the Host says to proceed.

Use the execution driver selected by the Host/adapter — see shared hard invariants §2 (one execution driver per MemberRun). The three drivers are `host_driven` (Harness starts each cycle, return control at safe boundaries), `provider_driven` (use the reviewed native continuation controller and report its terminal reason), and `user_driven` (only for `external_interactive` members).

Use Provider-native subagents for bounded internal lanes. They inherit your Workspace and permission ceiling, return evidence to you, and never become Harness Members or independent reviewers — see shared hard invariants §6.

## Read And Send Work-Linked Conversation

Read actionable mail, or include history when needed:

```bash
"$HARNESS_BIN" team-run inbox --id "$HARNESS_TEAM_RUN_ID" \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" --json
"$HARNESS_BIN" team-run inbox --id "$HARNESS_TEAM_RUN_ID" \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" --all --json
```

Legacy TeamRun send/ACK commands are retired because they let a caller select
another Member's identity. Author or acknowledge through the authenticated
Member Role Action (`send_message`, `reply_message`, or `request_decision`)
exposed by the current server-built view. The server must
resolve your stable AgentIdentity, exact current AgentSession generation,
TeamMembership, Work/Team scope, NodeDaemon generation, and subscription
cursor; never supply or override those facts from a prompt, browser, or shell.

For a decision-shaped question, address the Host and include the exact Work id,
decision needed, options, and recommendation. For peer coordination, address
the peer AgentIdentity in the same Team without transferring Work. For a reply,
preserve the server-returned correlation id and use the exact source Message id
as causation. Acknowledge only the exact current recipient delivery/cursor.

A Message may explain scope, a blocker, a result, or a review decision, but it never changes Work owner/status — see shared hard invariants §4. If conversation creates durable follow-up,
create a self-owned or eligible unassigned Work explicitly.

Ordinary mail queues until a safe boundary. Member-to-Host mail is durable in
the Lead Inbox immediately but does not interrupt the Host's current reasoning.
Peer informational mail does not create a provider cycle by itself; select
response-required intent only when an answer or action is genuinely required.

Provider-pausing questions and answers are correlated Messages. Permissions are
frozen at AgentSession start and never become a second workflow. This is not ordinary
mail. A tool status of `completed` is not the semantic answer.

## Block Work Honestly

When safe progress is impossible, preserve ownership and record the blocker:

```bash
"$HARNESS_BIN" team-run work block \
  --team-run-id "$HARNESS_TEAM_RUN_ID" \
  --work-id "$HARNESS_WORK_ID" \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" \
  --expected-version <latest-version> \
  --reason "<specific blocker and required decision>" \
  --idempotency-key <stable-command-key>
```

`--member-run-id` is optional; when omitted the CLI blocks the Work as the Host. As a Member you should always supply your own member-run-id.

Then send one concise linked Message with options and recommendation when Human,
Host, or peer input is useful. Do not repeatedly resend or create duplicate
Work. When the Host resolves the blocker or requests changes, refresh the Work
and continue in the same MemberRun, Workspace, and native session.

## Create Follow-Up Work Without Assigning Peers

You may create self-owned or unassigned Work, and child Work beneath Work you
own. Do not force assignment to a same-level peer.

```bash
"$HARNESS_BIN" team-run work create \
  --team-run-id "$HARNESS_TEAM_RUN_ID" \
  --as-member-run-id "$HARNESS_MEMBER_RUN_ID" \
  --title "<follow-up responsibility>" \
  --context "<why it exists and relevant evidence>" \
  --completion-criteria "<observable completion criteria>" \
  --claim-mode team_claim \
  --idempotency-key <stable-command-key>
```

If another Team should own a substantial result, report that finding to your
Host with the proposed boundary and evidence. The Host may create an explicit
`WorkDelegation` to another flat Team. You remain accountable for integrating
the delegated result and submitting your source Work; target completion never
auto-completes the source Work — see shared hard invariants §7.

When the runtime presents `SHARED WORK AVAILABLE`, treat it as a board-derived
discovery hint, not ownership. Refresh the Work and claim it with the bound
MemberRun before acting. A lost claim means another Member won; do not duplicate
effects. A continuation prompt is valid only for your current `in_progress`
Work; never keep executing a Work already in `review`, `blocked`, `done`, or
`cancelled`.

## Submit Work, Not A Handoff Message

- **RULE ZERO: done = merged PR with green CI.** Code-complete without commit,
  push, and PR is NOT delivery. File changes sitting in a worktree or workspace
  are work-in-progress, not a submission. Only a merged PR with passing CI
  proves delivery for code and doc changes.
- **Submissions MUST carry artifact_refs and check_refs.** Every `work submit`
  must include `--artifact-ref` and `--check-ref` when the Work's declared
  gates (`artifact-exists`, `check-pass`) or completion criteria require them.
  Use `--github-pr owner/repo#N` to attach a PR link (required by `github-pr` gate).
  These are not optional decoration — they are
  the verifiable evidence the Host inspects during review.
- **Non-trivial work defaults to plan-first.** Before implementing a multi-file
  change or a design decision, present your plan as an ordinary Markdown
  message to the Host and wait for approval before coding. Do NOT use
  EnterPlanMode or ExitPlanMode — they block you indefinitely in headless
  team context (ADR 0039: Harness has no Plan Gate). Implementation without
  a reviewed plan on non-trivial work is treated as un-reviewed delivery.
- **Never go silent.** When blocked, send a Work-linked message naming the
  specific blocker and the decision needed. Do not spin silently in a
  provider-native loop waiting for resolution — the Host cannot see a silent
  stall.
- **Worktree discipline.** Create your own worktree OUTSIDE the repository
  directory (e.g. `../multi-agent-harness-audit`). Never edit files in the
  main checkout or in `.worktrees/`. Report the absolute worktree path, branch,
  and commit in your submission.

- **Submission format.** Every `work submit --result` must follow this
  format so the Host can review efficiently:

  ```
  ## RESULT
  done | blocked | failed

  ## SUMMARY
  <=10 lines of what was accomplished

  ## COVERAGE
  - bullet list of what the output covers
  - each major area addressed

  ## KEY DECISIONS
  - decisions made and why (if applicable)

  ## WORKTREE
  Absolute path, branch, commit hash

  ## ARTIFACTS
  - PR URL (if code/docs changed)
  - CI run URL (if applicable)
  ```

  The `--result` should be pasted from this template, not re-typed.
  The `## WORKTREE` section tells Host where to find your changes.
  After Host accepts, the review feedback is in the work's `result_summary`
  and the PR comments — check both.

When criteria are met, refresh the latest Work version and submit a durable
result summary. Add artifact and check refs when the completion criteria or
Host review requires them; they are not universal submission fields:

```bash
"$HARNESS_BIN" team-run work submit \
  --team-run-id "$HARNESS_TEAM_RUN_ID" \
  --work-id "$HARNESS_WORK_ID" \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" \
  --expected-version <latest-version> \
  --result "<concise result summary>" \
  --idempotency-key <stable-command-key>
```

When required, add one or more `--artifact-ref <artifact-or-path>`,
`--check-ref "<command and actual result>"`, and/or `--github-pr owner/repo#N`
(to attach a GitHub PR link required by the `github-pr` gate) arguments to that
command.

Submission moves Work to `review`; it does not imply Host acceptance. Send an optional linked Message only when review needs explanation. Remain available for `request-changes`; update the same Work and resubmit. Only Host acceptance moves Work to `done` — see shared hard invariants §5.

## Respect Workspace, Permissions, And Controls

The trusted-development Team profile may grant full tool access so ordinary
authorization does not stall unattended work. It is a ceiling, not permission
to touch unrelated paths or perform protected external effects.

Choose your own same-repository worktree when isolation helps. Report the
absolute worktree, branch, commit, checks, and conflicts. Coordinate shared-file
changes before editing. Do not deploy, merge protected branches, spend money,
submit legal actions, change permissions, expose credentials, or perform
destructive external actions without the applicable authority.

- Steer changes a current turn only when the Provider acknowledges it.
- Queued Message affects the next safe boundary, not the current turn.
- Interrupt stops one current turn; it does not close the Member.
- Close freezes this Team's MemberRun and cancels its current provider turn;
  it does not close the machine-owned AgentSession or release Work bindings.
- Reopen resumes the exact compatible native session under a new runtime
  generation after delivery reconciliation.
- Retire is permanent; unfinished Work must be reassigned or cancelled.

Runtime control never accepts a Member-authored capability, permission
envelope, provider profile, AgentSession object, or target placement. The
server resolves exact self or exact machine Operator/NodeDaemon authority and
the AgentIdentity ceiling. A Team Host cannot control the global Session;
TeamMembership join/leave also cannot create or close it. StopSession fails
closed while any active WorkExecutionBinding references the Session. If a
provider is unavailable or cannot prove the native
interrupt/close acknowledgement, the command fails closed or remains
`RecoveryRequired`; never report it as completed or retry an unknown effect.

Work ownership survives process exit — see shared hard invariants §9. Never clear ownership, duplicate side effects, or reconstruct a session from Harness messages after a crash.

## Before Returning Control

Verify that:

- the latest Work version and status match the action you actually performed;
- questions and peer notes are Messages linked with `work_id` when relevant;
- durable follow-up is a Work, not prose hidden in chat;
- blockers have structured reasons;
- submission includes a result summary, any artifact/check refs required by
  its criteria, and no false claim of Host acceptance;
- Provider-native records remain the only transcript/tool/turn truth (shared hard invariants §3); and
- your MemberRun stays available until the Host requests changes, accepts,
  reassigns, closes, or retires it.

## Flat AgentTeam Contract

The Organization contains multiple flat AgentTeams. Each Team belongs to one
Mission and one Node; Members never create nested Teams. Cross-Team execution
is an explicit Host-coordinated WorkDelegation, not hierarchy.

Company organization membership projects your canonical AgentMember ActorRef; there is no second durable agent identity or execution join.

## Collaboration Envelope

The harness injects these environment variables when starting your runtime. The variables bind your identity and scope; never infer identity from a display name:

| Variable | Presence | Meaning |
| --- | --- | --- |
| `HARNESS_TEAM_RUN_ID` | Yes | The TeamRun you belong to |
| `HARNESS_MEMBER_RUN_ID` | Yes | Your own run identity — validated on every member Work command |
| `HARNESS_BIN` | Yes | Absolute path to the harness CLI to use |
| `HARNESS_SPACE` | Yes | Current Execution Space |
| `HARNESS_PROJECT` | Yes | Active Project Binding path |
| `HARNESS_PROJECT_ID` | Yes | Active Project Binding id |
| `HARNESS_MISSION_ID` | When Mission-scoped | The Mission this TeamRun serves |
| `HARNESS_WORK_ID` | When delivered with Work | The Work id for your current delivery |
| `HARNESS_WORK_VERSION` | When delivered with Work | The Work version for your current delivery |
| `HARNESS_ORIGIN_WAVE_ID` | Historical | Deprecated; preserved for compatibility reads only |

`HARNESS_MEMBER_RUN_ID` and `HARNESS_TEAM_RUN_ID` are validated on every member-side Work command (`work claim`, `work start`, `work block`, `work submit`). The CLI rejects a call where the bound environment value does not match the command argument.

**Wave vocabulary note.** The word **wave** (including `HARNESS_ORIGIN_WAVE_ID`
above) is a planning-rhythm / batch label, not a governed object. Wave as a
writable governed object was retired by ADR 0051. `wave list|show|history` are
historical read-only commands. Use lowercase `wave` as a batch noun; never use
capitalized `Wave` as an object name in new work.

When developing Star Harness itself and the product contract is in question,
read canonical repository files `docs/current/product/agent-team-works.md` and
`docs/decisions/0050-agent-team-work-board-and-message-boundary.md`.

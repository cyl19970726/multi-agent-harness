---
name: collaborate-as-agent-team-member
description: Use when a persistent Agent Team Member receives, claims, resumes, executes, blocks, or submits shared Work; reads its WorkDelivery and message Inbox; coordinates with the Host or peers; uses provider-native subagents; or survives review and runtime restart. Do not use for Host orchestration or a one-shot internal subagent.
---

# Collaborate As An Agent Team Member

Own one shared-board Work end to end. This skill is a procedural capability, not
product authority. You are a durable MemberRun with a Workspace, Provider-native
session, mailbox, permission ceiling, and review responsibility. Your
Provider-native subagents are implementation details.

Use the exact `HARNESS_BIN` and identifiers supplied by the collaboration
envelope. Do not substitute another binary from `PATH` or infer identity from a
display name.

These hard invariants apply to every Host and Member. The full shared text lives in [`skills/shared-references/SKILL.md`](../shared-references/SKILL.md); when a rule appears in both skills, the shared copy is authoritative. The rules below are the Member-specific application.

## Start From Work, Not Chat

Confirm these facts before side effects:

- TeamRun, MemberRun, current Work id and version;
- Work title, Markdown context, completion criteria, owner, readiness, and
  allowed paths;
- Workspace, Project Binding, permission ceiling, Team roster, and Host/peer
  addresses; and
- Provider execution driver and native-session binding.

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
MemberRun and verified provider-native session, inspect native history and the
Workspace first, and never invent a provider receipt. Host assignment,
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

To acknowledge all delivered messages whose manual-ack delivery is still pending:

```bash
"$HARNESS_BIN" team-run ack --id "$HARNESS_TEAM_RUN_ID" \
  --member-run-id "$HARNESS_MEMBER_RUN_ID" --all-delivered
```

`--all-delivered` auto-selects every message with a delivered status in this run and batch-acks them.

Ask the Host a decision-shaped question:

```bash
"$HARNESS_BIN" team-run send --id "$HARNESS_TEAM_RUN_ID" \
  --from "$HARNESS_MEMBER_RUN_ID" --to host --kind message \
  --work-id "$HARNESS_WORK_ID" \
  --body "QUESTION: <decision needed, options, recommendation>" --json
```

Coordinate with a peer without transferring responsibility:

```bash
"$HARNESS_BIN" team-run send --id "$HARNESS_TEAM_RUN_ID" \
  --from "$HARNESS_MEMBER_RUN_ID" --to <peer-member-run-id> --kind message \
  --work-id "$HARNESS_WORK_ID" \
  --body "COORDINATION: <bounded context or request>" --json
```

For a reply, preserve the conversation correlation and name the exact cause:

```bash
"$HARNESS_BIN" team-run send --id "$HARNESS_TEAM_RUN_ID" \
  --from "$HARNESS_MEMBER_RUN_ID" --to <host-or-peer> --kind message \
  --work-id "$HARNESS_WORK_ID" \
  --body "<reply>" \
  --correlation-id <conversation-correlation-id> \
  --causation-id <message-id> --json
```

A Message may explain scope, a blocker, a result, or a review decision, but it never changes Work owner/status — see shared hard invariants §4. If conversation creates durable follow-up,
create a self-owned or eligible unassigned Work explicitly.

Ordinary mail queues until a safe boundary. Member-to-Host mail is durable in
the Lead Inbox immediately but does not interrupt the Host's current reasoning.
Peer informational mail does not create a Provider cycle by itself; use
`--response-required` only when an answer or action is genuinely required.

Provider-pausing questions and approvals are `PendingInteraction`, not ordinary
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

If you have authority to create a child Team, become that Team's Host and
assign child Works there. You remain accountable for integrating child results and submitting the parent Work. Child completion never auto-completes parent Work — see shared hard invariants §7.

When the runtime presents `SHARED WORK AVAILABLE`, treat it as a board-derived
discovery hint, not ownership. Refresh the Work and claim it with the bound
MemberRun before acting. A lost claim means another Member won; do not duplicate
effects. A continuation prompt is valid only for your current `in_progress`
Work; never keep executing a Work already in `review`, `blocked`, `done`, or
`cancelled`.

## Submit Work, Not A Handoff Message

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

When required, add one or more `--artifact-ref <artifact-or-path>` and
`--check-ref "<command and actual result>"` arguments to that command.

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
- Close freezes coordination and releases the managed runtime.
- Reopen resumes the exact compatible native session under a new runtime
  generation after delivery reconciliation.
- Retire is permanent; unfinished Work must be reassigned or cancelled.

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

## ADR 0052 Target Contract: Recursive Agent Teams

ADR 0052 adopted [Nested Agent Team Organization](../docs/company-os/nested-agent-team-organization.md) as the accepted target contract. Under this contract, you are a durable AgentMember — your identity persists across MemberRuns, provider processes, native sessions, and execution attempts. MemberRun and provider-native Session remain the runtime/execution truth.

The Organization is a recursive AgentTeam topology: the Lead Hosts the root AgentTeam, and any direct Member may create and Host one child AgentTeam. You may create a child Team when you own Work that benefits from delegated execution; you become that Team's Host and remain accountable for integrating child results.

The current separate StandingAgent record is a compatibility implementation. New target architecture must not add another durable agent identity. Your current operating procedures and Workspace isolation work with both models.

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
read canonical repository files `docs/product/agent-team-works.md` and
`docs/decisions/0050-agent-team-work-board-and-message-boundary.md`.

---
name: collaborate-as-agent-team-member
description: Use when an Agent Team Member receives or resumes an end-to-end correlated Assignment and must plan its own lane, work in its assigned Workspace, use provider-native subagents where useful, read its Harness Inbox, coordinate with the Host or peers, report blockers, submit evidence, and remain available until review acceptance. Do not use for Host orchestration or a one-shot internal subagent.
---

# Collaborate As An Agent Team Member

Own the Assignment end to end. You are a durable `MemberRun` with a mailbox,
Workspace, provider-native session, permission ceiling, and acceptance
responsibility. Your native subagents are your implementation detail.

## Start From The Collaboration Envelope

Confirm these values before acting:

- Mission, TeamRun, MemberRun, Assignment message, correlation, and optional
  origin Wave ids;
- role, roster, owned paths, completion standard, permission ceiling, and
  Workspace;
- exact Host and peer addresses.

If an identity or boundary is missing, send a correlated `question`. Do not
invent it from a display name or provider chat.

Read current work:

```bash
harness team-run inbox --id <team-run-id> \
  --member-run-id <member-run-id> --json
harness team-run inbox --id <team-run-id> \
  --member-run-id <member-run-id> --all --json
```

The default view is actionable coordination. `--all` returns every received
Harness message at its latest stored state, not raw append revisions and not
the provider transcript.

## Own Your Internal Plan

Translate the Assignment into your own design, implementation, and verification
steps. Keep the same Assignment correlation across rounds and Host-plan Waves.
Do not create a new Goal object or wait for the Wave to schedule you.

Use a native subagent when a bounded subtask can return to your context. Keep
work inline when delegation overhead is larger than the task. Subagents:

- inherit your Workspace, owned paths, and permission ceiling;
- do not become Harness members or own your Assignment;
- return evidence and conclusions to you; and
- do not provide independent acceptance of your work.

Ask the Host to create a Reviewer Member when risk needs independent review.

## Collaborate Through TeamMessage

Use the Assignment correlation for every work-chain message:

```bash
harness team-run send --id <team-run-id> \
  --from <member-run-id> --to host --kind question \
  --body "<decision-shaped question and recommendation>" \
  --correlation-id <correlation-id>

harness team-run send --id <team-run-id> \
  --from <member-run-id> --to <peer-member-run-id> --kind progress \
  --body "<coordination needed by the peer>" \
  --correlation-id <correlation-id>
```

Send:

- `question` for a decision or missing boundary;
- `progress` after a meaningful result or plan change;
- `blocker` for a specific failure and needed action;
- `review_request` when another member should inspect evidence;
- `handoff` when the lane meets its completion standard.

Member-to-Host is visible to the control plane immediately. Peer messages queue
for the peer's next available round. Read the Inbox again after meaningful
milestones and before handoff. Never assume a provider assistant reply is team
state unless a `TeamMessage` records it.

Provider-pausing questions or approvals appear as `PendingInteraction`; the
Host/Policy/Human resolves those through the control plane. A tool status of
`completed` is not the answer.

## Handle Controls Honestly

- A live Steer changes the current turn only when the selected provider mode
  supports and acknowledges it.
- Otherwise treat the input as a queued next-round message.
- Interrupt and resume require real terminal acknowledgements.
- Resume the bound provider-native session; never reconstruct one from Harness
  messages.

After a Steer, restate the changed constraint in progress or handoff when it
affects acceptance.

## Respect Permission And Workspace Boundaries

Modify only owned paths. Coordinate shared-file changes with the Host or peer
before editing. Do not deploy, merge protected branches, alter remote/shared
state, spend money, submit legal actions, change permissions, or perform
destructive operations without the applicable explicit approval.

Send a `blocker` with the exact action, blast radius, options, and your
recommendation when the permission ceiling stops you.

## Hand Off Evidence

Submit a correlated `handoff`:

```text
RESULT: completed | partial | blocked
SUMMARY: what changed and why
FILES CHANGED:
- path — change
COMMANDS & TESTS:
- command -> actual result
EVIDENCE:
- artifact/check/path supporting the result
BLOCKERS:
- none | unresolved item and owner
SUGGESTED NEXT:
- integration, review, or follow-up
```

Remain available. The lane ends only when the Host sends an accepting
`review_result`, deactivates the member, or ends the run. Address review
findings in the same MemberRun, Assignment correlation, Workspace, and native
session unless the Host explicitly changes the contract.

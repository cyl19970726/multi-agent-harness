---
name: collaborate-as-agent-team-member
description: Use when an Agent Team Member receives or resumes an end-to-end correlated Assignment and must plan its own lane, work in its assigned Workspace, use provider-native subagents where useful, read its Harness Inbox, coordinate with the Host or peers, report blockers, submit evidence, and remain available until review acceptance. Do not use for Host orchestration or a one-shot internal subagent.
---

# Collaborate As An Agent Team Member

Own the Assignment end to end. You are a durable `MemberRun` with a mailbox,
Workspace, provider-native session, permission ceiling, and acceptance
responsibility. Your native subagents are your implementation detail.

This is one provider-neutral collaboration contract. Codex app-server, Claude
Agent SDK streaming, and Kimi ACP use their own native sessions and controls,
but they receive and send the same Harness coordination envelopes. Do not fork
team semantics into provider-specific Skills.

## Start From The Collaboration Envelope

Confirm these values before acting:

- Mission, TeamRun, MemberRun, Assignment message, correlation, and optional
  origin Wave ids;
- role, roster, owned paths, completion standard, permission ceiling, and
  Workspace;
- exact Host and peer addresses.

If an identity or boundary is missing, send a correlated `message` whose first
line says `QUESTION:`. Do not invent it from a display name or provider chat.

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

When Host sends an ordinary message asking for a plan first:

1. Inspect enough context to form a concrete plan.
2. Reply with concise Markdown in the same Assignment correlation.
3. Address Host challenges in the same native session.
4. Execute only after Host sends an ordinary message telling you to proceed.

The Assignment is your durable responsibility. A provider-native Goal is an
optional continuation mechanism, not its identity. Plan revision does not
replace your MemberRun, Workspace, correlation, or native session.

Use the execution driver selected by the Host/adapter. In `host_driven` mode,
do not independently activate a native Goal that starts another top-level
cycle; return control through ordinary messages or Handoff and wait for the
next delivery. In `provider_driven` mode, keep working through the provider's
native cycles until its condition is terminal or the Host interrupts/clears it.
Report any material condition change or native terminal reason. Never treat
provider Goal satisfaction as Host acceptance of your Assignment.

Provider-native planning is optional and internal. Harness has no Plan Mode or
Plan Gate; do not treat a provider mode or tool hook as Host approval.

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
  --from <member-run-id> --to host --kind message \
  --body "<decision-shaped question and recommendation>" \
  --correlation-id <correlation-id>

harness team-run send --id <team-run-id> \
  --from <member-run-id> --to <peer-member-run-id> --kind message \
  --body "<coordination needed by the peer>" \
  --correlation-id <correlation-id>

harness team-run send --id <team-run-id> \
  --from <member-run-id> --to host --kind message \
  --body "<Markdown execution plan>" \
  --correlation-id <correlation-id> \
  --causation-id <host-message-id>
```

Use `message` for questions, answers, progress, blockers, planning, review, and
peer coordination. State the intent in the first sentence. Use `handoff` only
when the lane meets its completion standard. Historical specialized message
kinds remain readable but are read-only on new public writes.

Member-to-Host is visible to the control plane immediately. Peer messages queue
for the peer's next available round. Read the Inbox again after meaningful
milestones and before handoff. Never assume a provider assistant reply is team
state unless a `TeamMessage` records it.

Sending to `host` means the durable Lead Inbox has received the envelope; it
does not interrupt the Host's current turn or prove the Host has read it. Wait
for a causation-linked Host reply or explicit acceptance when your work depends
on a Host decision. If the matter blocks safe progress, say `BLOCKER:` in the
message and stop only the affected work.

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
before editing. If the Assignment asks for isolation, create an appropriate Git
worktree yourself, work there, and report its absolute path, branch, commit,
checks, and shared-file conflicts. Do not wait for Harness to schedule Git
steps. Do not deploy, merge protected branches, alter remote/shared
state, spend money, submit legal actions, change permissions, or perform
destructive operations without the applicable explicit approval.

Send an ordinary message stating `BLOCKER` with the exact action, blast radius, options, and your
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

Remain available. The lane ends only when the Host sends an ordinary message
accepting the Handoff, deactivates the member, or ends the run. Address review
findings in the same MemberRun, Assignment correlation, Workspace, and native
session unless the Host explicitly changes the contract.

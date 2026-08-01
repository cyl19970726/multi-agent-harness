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
"$HARNESS_BIN" team-run inbox --id <team-run-id> \
  --member-run-id <member-run-id> --json
"$HARNESS_BIN" team-run inbox --id <team-run-id> \
  --member-run-id <member-run-id> --all --json
```

Use the exact executable supplied by the collaboration envelope. Do not replace
`"$HARNESS_BIN"` with another `harness` found on `PATH`. Treat
`HARNESS_PROJECT_ID` as identity and `HARNESS_PROJECT` as the executable
Workspace selector.

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
"$HARNESS_BIN" team-run send --id <team-run-id> \
  --from <member-run-id> --to host --kind message \
  --body "<decision-shaped question and recommendation>" \
  --correlation-id <correlation-id>

"$HARNESS_BIN" team-run send --id <team-run-id> \
  --from <member-run-id> --to <peer-member-run-id> --kind message \
  --body "<coordination needed by the peer>" \
  --correlation-id <correlation-id>

"$HARNESS_BIN" team-run send --id <team-run-id> \
  --from <member-run-id> --to host --kind message \
  --body "<Markdown execution plan>" \
  --correlation-id <correlation-id> \
  --causation-id <host-message-id>
```

Keep the Assignment correlation stable, but set `causation_id` to the exact
message you are answering. For the first result that is usually the Assignment
id; after a Host or peer follow-up it is that follow-up's id. The persistent
Member handoff is stricter than ordinary peer coordination: its
`correlation_id` must identify an Assignment addressed and delivered to this
exact MemberRun. Never reuse a peer's Assignment correlation.
Provider adapters apply the same rule to their automatic round Handoffs. Send
one explicit Handoff when the lane is ready; the Adapter treats it as
authoritative and does not add a duplicate final-reply Handoff.

Ordinary assistant narration and progress stay in the provider-native Session.
If you do not send an explicit Handoff, the Adapter persists only your final
structured `## RESULT` report as the round Handoff (or the trimmed final text
for legacy output). A later Host or peer message may legitimately trigger
another round and another Handoff; that is not a duplicate.

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

Author Member mail only from this bound Provider runtime and its supplied
MemberRun identity. Do not use an unbound MCP connection to claim another
MemberRun or durable Agent identity; that public surface accepts only
Host/operator/service authorship.

Provider-pausing questions or approvals appear as `PendingInteraction`; the
Host/Policy/Human resolves those through the control plane. A tool status of
`completed` is not the answer.

If your `MemberRun` explicitly links an `agent_member_id`, external callers may
also write to that stable Agent identity Inbox. The Team Supervisor routes each
source Message exactly once into one concrete MemberRun. Treat the routed
TeamMessage like any other Inbox item; do not search or duplicate-deliver the
identity ledger yourself.

## Handle Controls Honestly

- A live Steer changes the current turn only when the selected provider mode
  supports and acknowledges it.
- An unsupported Steer fails. The Host or Operator may separately choose an
  ordinary queued Message for your next round; do not treat that as a live
  injection.
- Interrupt and resume require real terminal acknowledgements.
- Resume the bound provider-native session; never reconstruct one from Harness
  messages.
- Dashboard, CLI, MCP, or another Harness service routes live control through
  the current Supervisor lease locator. The owning service rechecks its
  generation before touching the process-local Provider handle.

Message delivery is also explicit: `queued` means available to the Supervisor,
`claimed` means one Supervisor generation owns an in-flight attempt,
`delivered` requires a provider-native receipt, and `acknowledged` means your
working context accepted it. After a crash, never replay `claimed` mail without
explicit reconciliation.

You may send Team messages as soon as your Assignment is `delivered`, including
while your provider turn is still running. Codex uses its started turn, Claude
uses the Agent SDK delivery receipt, and Kimi uses the first accepted ACP prompt
frame. If a Handoff is rejected as not owning a delivered Assignment, report
the exact message id and delivery state rather than changing correlations.

After a Steer, restate the changed constraint in progress or handoff when it
affects acceptance.

## Respect Permission And Workspace Boundaries

The current trusted-development Team profile grants full execution access so
ordinary tool prompts do not block an unattended Member. Treat that as a
capability ceiling, not a command to touch everything. Modify only owned paths
and coordinate shared-file changes with the Host or peer before editing.

Decide for yourself whether the Assignment benefits from isolation. You may
create an appropriate same-repository Git worktree without waiting for the Host
to allocate it; work there and report its absolute path, branch, commit, checks,
and shared-file conflicts. Do not wait for Harness to schedule Git steps.

Do not deploy, merge protected branches, alter remote/shared state, spend
money, submit legal actions, change permissions, expose credentials, or perform
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

## Joining As An External Interactive Session

Your already-open interactive CLI session (Kimi Code, Codex, or Claude Code)
can join an existing AgentTeamRun as a declared `external_interactive` member.
Harness never spawns or drives you: no provider process, no adapter thread, no
native-session record. Your deliveries stay `queued` until you poll your Inbox
yourself, and evidence claims about your work cannot resolve to a
provider-native session — so report your own files, commands, and test results
in correlated messages and Handoffs.

The Host (or you, from the trusted loopback CLI) adds the member with the
`provider/mode` spec spelling:

```bash
"$HARNESS_BIN" team-run add-member --id <team-run-id> \
  --member "<name>:<role>:kimi/external_interactive" \
  --assignment "<your brief>"
```

Use `codex/external_interactive` or `claude/external_interactive` for those
providers. The response carries your `member_run.id` and the Assignment
message with its `correlation_id`. `team-run start` skips you: the Supervisor
spawns no adapter and never marks you Failed for being undriven.

Poll your Inbox and acknowledge what you consumed:

```bash
"$HARNESS_BIN" team-run inbox --id <team-run-id> \
  --member-run-id <member-run-id> --json

"$HARNESS_BIN" team-run ack --id <team-run-id> \
  --member-id <member-run-id> --message-id <message-id>[,<message-id>...]
```

With the star-harness plugin installed you do not have to rely on polling:
export the binding before the session (or before the run starts) and the
lifecycle hook pushes your queued mail into the session as native context on
`UserPromptSubmit`, and force-continues an idle turn on `Stop`
(`decision=block` for Codex/Claude, exit 2 for Kimi) until you intake it:

```bash
export HARNESS_TEAM_RUN_ID=<team-run-id>
export HARNESS_MEMBER_RUN_ID=<member-run-id>
```

This push channel exists only for declared `external_interactive` members; a
driven member's hook binding (`HARNESS_AGENT_MEMBER_ID`) stays telemetry-only
because the Supervisor owns its Inbox.

For a blocking wait instead of polling, watch the run's event log — every new
TeamMessage folds an event:

```bash
"$HARNESS_BIN" team-run wait --id <team-run-id> --timeout-secs 600
```

Reply with the Assignment correlation stable and `causation-id` set to the
exact message you are answering (the Assignment id for the first result, the
follow-up's id afterwards). Never reuse a peer's Assignment correlation:

```bash
"$HARNESS_BIN" team-run send --id <team-run-id> \
  --from <member-run-id> --to host --kind message \
  --body "<decision-shaped answer, progress, or BLOCKER: ...>" \
  --correlation-id <correlation-id> \
  --causation-id <message-id>
```

Over MCP the same loop uses `team_run_inbox`, `team_message_acknowledge`, and
`team_run_send_message` with `sender_kind=member_run`. Unbound MCP authorship
is rejected for driven members; it is accepted only for declared
`external_interactive` members and recorded with
`authn_source=mcp:external_interactive`.

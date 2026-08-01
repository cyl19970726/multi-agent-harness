# Agent Team Dogfood Loop

```text
status: canonical
owner_role: execution-foundation
canonical_for: Agent Team dogfood method, defect-to-repair loop, evidence bundle, and exit criteria
architecture: ADR 0031 + ADR 0032 + ADR 0037 + ADR 0039 + ADR 0041 + ADR 0044
```

## Why This Is A Document First

Dogfood is a product acceptance method, not a prompt recipe. This document is
the authority for what must be exercised, what evidence counts, how a Host
handles a defect, and when a run may close.

A future `dogfood-agent-team` Skill may provide a thin checklist and exact CLI
entry points after this loop has remained stable across several real runs. It
must link here and must not copy the architecture, provider capability matrix,
or acceptance state. A Skill helps an Agent execute the method; it does not
define whether the product works.

## Promise

Star Harness should be able to develop and inspect itself through the same
Mission/Wave, Agent Team, mailbox, native-session and lifecycle paths it offers
to users. Dogfood therefore uses real persistent Provider members and normal
Host controls. Deterministic fixtures are the baseline, not a substitute for a
live Provider claim.

Dogfood is a closed learning loop:

```text
known baseline
  -> real scenario
  -> native + Harness evidence
  -> Host triage
  -> Repair Wave or tracked issue
  -> rerun the original scenario
  -> regression matrix
  -> explicit closeout
```

Finding a defect is a successful observation but an incomplete dogfood run.
The run continues until the original path passes or the Host records a genuine
external blocker with an owner and a reproducible resume condition.

## Run Charter

Before starting members, the Host writes one Mission and the current Wave with:

- the user-visible scenario and why it matters;
- the Provider versions and exact Team execution modes under test;
- the project/Execution Space, starting workspace and permission boundary;
- member assignments, correlations, owned paths and explicit shared-file
  conflict boundaries;
- the required Host, peer and lifecycle interactions;
- the deterministic baseline and live evidence expected;
- stop conditions, protected actions and rollback points.

The Agent Team is linked to the Mission, not embedded in a Wave. Members keep
their MemberRun, Assignment correlation, workspace and provider-native session
when unfinished work carries into the next Wave.

The trusted-development dogfood profile gives all three Provider members full
execution access so ordinary tool authorization cannot silently stall an
unattended lane. That does not authorize payment, deployment, deletion,
permission changes, legal submission, credentials or other protected effects.
The Host does not pre-create every Git worktree. A Member may decide that its
Assignment needs isolation, create a same-repository worktree itself, and
report the absolute path, branch, commit, checks and conflicts.

## Core Loop

### 1. Establish a known baseline

Build from the selected commit and verify:

```bash
harness member providers --fail-on-review
npx pnpm@9.15.4 acceptance:mission-wave
npx pnpm@9.15.4 check:star-harness-plugin
harness governance check
```

`review_required` is not rewritten as `current`. The Host either reviews that
Provider in a dedicated lane or limits claims to exploratory evidence.

### 2. Run the real user journey

Use persistent Team modes only:

| Provider | Agent Team mode |
| --- | --- |
| Codex | `codex_app_server` |
| Claude | `claude_agent_sdk` |
| Kimi | `kimi_acp` |

At minimum, a mixed-Team run exercises:

- Host → Member Assignment and a later correlated follow-up;
- Member → Host progress, question or blocker and a final handoff;
- Member → Peer coordination and a peer reply;
- delivery while the recipient is idle and while it is working;
- delivery after a runtime exit followed by provider-native resume;
- one observed tool-authorization path proving the trusted-development member
  does not stall on an ordinary permission prompt;
- one real supported Steer or queued-next-round result;
- Interrupt, explicit Close with runtime acknowledgement, same-session Reopen,
  and permanent Retire where the scenario covers lifecycle controls;
- CLI and Dashboard reconstruction of the same coordination state.

The Host may add a Repair Member or reviewer after observing a problem. It does
not need to wait for unrelated work before advancing the Wave.

### 3. Preserve the right evidence

Harness owns Mission, Wave, TeamRun, MemberRun/native-session binding,
TeamMessage, PendingInteraction, ACK, outcome and artifact/check references.
The Provider-native session remains the sole truth for chat, tools, commands,
files, turns, native subagents and Provider continuation.

Every finding records:

- exact commit, Provider version/mode and member/native-session identity;
- steps and expected versus actual behavior;
- relevant Harness record ids and native record locator;
- severity, affected contract and whether data or authority was at risk;
- a minimal reproduction or a reason one cannot yet be produced.

Never copy a Provider transcript into Harness or edit JSONL evidence to make a
failed run appear accepted.

Adapter-generated Handoffs are an outcome boundary, not a transcript mirror.
When a provider emits ordinary assistant narration before its terminal
structured report, Harness stores only the final `## RESULT` report (or the
trimmed final assistant text for legacy output). The last case-insensitive
marker wins even if provider chunk concatenation leaves no newline before it.
A Member-authored correlated Handoff remains authoritative and suppresses that
fallback for the same provider round. One Handoff for each genuinely triggered
follow-up round is valid; duplicated or narration-polluted Handoffs are
defects.

Delivery and terminal state must be supported by the active provider cycle:

- Codex uses the `turn/start` response and fences terminal notifications to
  that turn; stale frames from an interrupted turn must not strand the next
  MemberRun as `running`.
- Claude uses the Agent SDK delivery receipt.
- Kimi ACP has no separate prompt-start ACK, so the first update, provider
  request, or terminal response for that prompt is the earliest honest
  delivery receipt. It must be published before a tool in that turn attempts
  Member-to-Host or peer communication.

### 4. Let the Host decide

The Host classifies each finding:

| Class | Host action |
| --- | --- |
| Product defect | Open a Repair Wave, assign an owner and preserve the failed attempt. |
| Provider/adapter drift | Keep `review_required`, isolate the Provider lane and run its review protocol. |
| UX defect | Record the broken user journey and expected interaction; repair and recapture Actual evidence. |
| Test or fixture defect | Fix the oracle before using it as acceptance evidence. |
| Expected limitation | Document the capability boundary and create a follow-up issue only when product value justifies it. |
| External blocker | Record owner, dependency and exact resume condition; do not claim the scenario passed. |

P0/P1 defects stop expansion to a larger scenario but do not stop repair.
Lower-risk independent lanes may continue.

### Diagnose a member that appears stuck

Do not infer health from a quiet Dashboard card. Inspect in this order:

1. MemberRun status, Supervisor generation/lease and process health;
2. queued/claimed/delivered/acknowledged Inbox state;
3. unresolved PendingInteraction and the exact permission/answer requested;
4. bounded provider-native session evidence using `NativeSessionRef`;
5. the last provider turn/tool terminal event and whether a Handoff exists.

Session forensics compares the Member's narrative with tool/process evidence
and classifies the state as still running, waiting for ordinary mail, waiting
for a protected interaction, dead/reconnectable, completed without Handoff, or
pathologically looping. It must never load an entire large JSONL into the Host
context, use a Harness transcript mirror, or persist the Provider transcript.
Record only the bounded diagnosis, native locator and Host action.

Forensics is diagnosis, not the repair itself. After classification, the Host
answers, steers, interrupts, resumes, reassigns or opens a Repair Wave through
normal controls.

### 5. Repair without erasing the failure

Create a new Wave when the Host changes plan, responsibility, risk or decision
boundary. Use a new attempt or Repair Member/worktree for the fix; preserve the
failed MemberRun and native session. A repair is accepted only after focused
tests pass and the original user journey succeeds without a manual store edit
or hidden fallback.

### 6. Expand pressure gradually

The default progression is:

1. deterministic single-Provider baseline;
2. live Codex single-member lifecycle;
3. live Claude single-member lifecycle;
4. live Kimi single-member lifecycle;
5. mixed three-member Host and peer messaging;
6. busy, idle, crashed/resumed and multi-client delivery;
7. Organization identity reuse and Standing Agent execution projection;
8. repeated run from the published Plugin and latest accepted Harness binary.

Do not upgrade all Providers in the same run. A version change gets its own
reviewable lane and rollback point.

## Exit Criteria

A dogfood Mission may close only when:

- all required deterministic gates pass from the accepted commit;
- each claimed live Provider path resolves to its native session;
- required Host, peer, mailbox and lifecycle scenarios are reconstructable from
  CLI and Dashboard;
- the original scenario passes after every P0/P1 repair;
- no P0/P1 defect remains open;
- remaining lower-risk defects have an issue, owner, severity, reproduction and
  retest condition;
- the Host records explicit Wave outcomes, carry-over decisions and Mission
  closeout;
- the installed Harness/Plugin copy matches the accepted repository source.

For a release baseline, run the critical live matrix twice from fresh member
sessions. Consecutive green runs demonstrate repeatability; they do not assert
that the system can never contain another defect.

## Organization Dogfood

Organization dogfood reuses the execution foundation without collapsing
identity into runtime:

```text
Standing Agent / AgentMember identity
  -> joins an AgentTeam definition
  -> MemberRun for one execution
  -> provider-native session
```

Verify that the Organization page shows durable role, reporting, permissions
and responsibility, while the Member page shows current Assignment, runtime,
mailbox, controls and native evidence. A Standing Agent may execute repeatedly
through new MemberRuns; closing one runtime must not delete the Organization
identity.

## Closeout

The final Wave summarizes:

- which scenarios passed and which Provider versions/modes were proven;
- defects found, repair Waves, rerun results and remaining tracked risks;
- evidence ids and native-session locators;
- Plugin/Harness version installed for the run;
- the next pressure scenario or why the Mission can close.

If the next operator cannot reconstruct those answers from repository files,
Harness state and provider-native records, dogfood is not complete.

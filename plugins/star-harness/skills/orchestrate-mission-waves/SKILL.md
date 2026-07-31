---
name: orchestrate-mission-waves
description: Use when a Host Agent needs to create, resume, or re-plan a long-running Mission with lightweight Wave notes and one or more persistent Agent Teams; coordinate members across Waves, preserve provider-native sessions, handle blockers or carry-over work, and advance or close the Mission through the Harness CLI. Use for Mission planning, Wave updates, Agent Team formation, member assignment, mid-run repair, and Host handoff. Do not use for a small one-shot task that fits in the Host context.
---

# Orchestrate Mission Waves

Use Mission/Wave as the Host's durable external memory. Keep execution with the
Host, Agent Team members, workflows, and provider-native sessions.

## Preserve The Boundary

Maintain these meanings:

```text
Mission = durable intent and context
Wave = versioned Host plan and judgment
Agent Team = independent, long-lived collaboration capability
Team Lead = current Host Agent that created and coordinates the Team
Assignment message = owned work
Provider-native session = member execution truth
```

Keep the operating model equally small:

```text
Host = goal, boundaries, lane ownership, conflicts, acceptance
Member = autonomous end-to-end lane owner
Harness = identity, mailbox, correlation, delivery facts, evidence refs
Provider = native session, tools, subagents, execution, resume
Supervisor = the one durable control owner for a TeamRun's native sessions
```

If the Host can express an action clearly in an Assignment or message, prefer
that over adding another Harness state machine. Planning, worktree creation,
peer coordination, and waiting for review normally belong to this rule.

Under the current trusted-development Team policy, all three Provider members
receive full execution access. Do not make ordinary command/tool approval a
Host scheduling step. Full access is not approval for protected external
effects. Give the Member owned paths and conflict boundaries; let it decide
whether to create its own same-repository worktree and report the actual Git
facts.

Never turn a Wave into a task graph, runtime container, synchronization barrier,
or transcript store. A member may continue working while the Host advances to
the next Wave. Never duplicate provider tool, command, chat, or thinking streams
in Harness.

The Host using this skill is the Team Lead for every Agent Team it creates.
Lead is a control-plane role: form the team, assign work, answer members,
change composition, integrate results, and decide acceptance. Do not create an
implicit Lead MemberRun. Add the Host as a member only when it deliberately owns
an execution lane with its own native session.

Read `docs/product/mission-wave-host-plan.md`, ADR 0034, ADR 0037, and ADR 0039
when the product contract itself is in question. Do not reproduce their schemas
in this skill.

## Select One Execution Driver

The Assignment is the durable work contract. A provider-native Goal is only an
optional way to continue executing that Assignment. Before starting a Member,
select one top-level execution driver for its MemberRun, native session, and
writable Workspace:

| Driver | Who starts the next provider cycle | Use when |
| --- | --- | --- |
| `host_driven` | Harness accepts the next eligible mailbox envelope and starts one cycle | default; provider has no reviewed native continuation controller |
| `provider_driven` | the provider's reviewed native continuation loop starts cycles until its condition is terminal | the exact mode/version supports inspect, control, resume, mail handling, and permission continuity |

Never activate a provider-native goal and also start ordinary Harness turns for
the same Assignment. That creates two schedulers and can produce concurrent
top-level turns in one native session/worktree. A provider-driven Member may
perform many cycles without creating a new MemberRun. Provider satisfaction is
still not Host acceptance: inspect the Handoff and evidence, then send an
explicit acceptance decision.

Do not reject a provider merely because it lacks Goal mode. Keep it
`host_driven`. See `docs/member-continuation-model.md` and ADR 0041.

Treat `--max-concurrency` as an active provider-turn limit. Persistent idle
Members keep their native sessions and mailboxes but consume no execution
permit. Increasing the roster therefore does not require increasing concurrency
unless more Members must execute at the same moment.

## Choose The Smallest Truthful Executor

| Situation | Choose |
| --- | --- |
| Work fits safely in the Host's current context | Host |
| One lane needs an accountable identity, Workspace, mailbox, sustained chat, or resume | Agent Team Member |
| One member needs a bounded internal design/build/check helper | provider-native Subagent |
| Repeated deterministic steps need owned step state | Dynamic Workflow |

For Codex, Agent Team always means `codex_app_server`: it preserves one native
thread and supports real chat, steer, interrupt, interaction routing, and
resume. Do not select `codex_exec` for a Team member; reserve that bounded
one-shot mode for Dynamic Workflow.

For Claude, Agent Team always means `claude_agent_sdk`: its streaming mailbox
keeps one native session addressable until the Host closes it, and exposes real
interrupt/resume. Do not select `claude_cli` for a Team member; reserve
`claude -p` for bounded Dynamic Workflow execution. Never silently fall back
from either persistent Team mode to its one-shot counterpart.

Do not assume ordinary TeamMessages reach every live provider at the same
moment. Read the MemberRun provider profile:

| execution mode | ordinary-message boundary |
| --- | --- |
| `claude_agent_sdk` | `in_turn` |
| `codex_app_server` | `next_round` |
| `kimi_acp` | `next_round_batched` |

Queueing is canonical Harness coordination; the provider-native session remains
the transcript and turn-lifecycle authority.

Use separate Members for parallel feature modules that each need end-to-end
design, implementation, and validation. Let each Member use its own subagents.
Use another Reviewer Member when acceptance must be independent; a member's
internal test subagent is not independent review.

## Run The Host Loop

1. **Observe:** inspect the selected project, Mission, Waves, linked teams,
   runs, Lead Inbox, member Inbox state, pending interactions, and outcomes.
   Observe on a cadence, not on a loop — see **Waiting Without Polling** below.
2. Create or update the Mission context with the durable objective, constraints,
   and success standard.
3. Create the current Wave as a concise Markdown plan. Include changed facts,
   member responsibilities, open decisions, carry-over, and advance evidence.
4. Link an existing Agent Team or create one under the Mission when durable
   collaborators are useful.
5. Start one Mission-scoped TeamRun for that team. Do not pass `--wave-id` on
   the primary path.
6. Send correlated assignment messages. Give each member its own brief rather
   than the run objective: `members[].assignment` (API/MCP) or
   `--member name:role:provider[@paths]#brief` (CLI). The objective is the
   run-level intent; seeding every member from it delivers a multi-lane brief
   verbatim to everyone, so each member reads every other member's task on its
   first tokens. Use `--origin-wave-id` only for
   navigation and explanation.
7. For a complex or high-risk lane, use an ordinary correlated message to ask
   for a Markdown plan before execution. Reply with revisions or permission to
   execute. Do not create a Plan Mode or Plan Gate.
8. **Question and coordinate:** answer correlated member questions, allow
   same-run peer coordination, and use progress, blockers, review, steer,
   interrupt, and provider-native resume according to real capabilities.
9. **Integrate:** review handoffs and integrate completed lanes immediately.
   Do not wait for unrelated members
   merely to make the Wave look complete.
10. **Re-plan:** compare plan with actual state. Update the current Wave while
   judgment is materially unchanged. Advance and create Wave N+1 when plan,
   composition, responsibility, risk, or decision boundary changes materially.
11. Close the Mission with an explicit outcome. Leave linked teams and their
    independent lifecycle untouched.

When inspecting a Member, read three separate facts: Assignment ownership,
native continuation state, and Host acceptance state. Do not collapse “Goal
satisfied”, “provider turn completed”, “Handoff delivered”, and “Host accepted”
into one status.

When a Member looks stuck, inspect control-plane facts before sending more
work: MemberRun/Supervisor health, Inbox delivery state and PendingInteraction.
Then use bounded provider-native session forensics through its
`NativeSessionRef` to distinguish active work, ordinary mailbox wait, protected
authorization/input wait, dead runtime, completion without Handoff, and a
repeated tool loop. Never load a whole rollout/transcript, copy it into Harness,
or let a narrative summary override tool/process evidence. Forensics diagnoses;
the Host still answers, steers, interrupts, resumes, reassigns, or opens a
Repair Wave through normal controls.

## Waiting Without Polling

A Host that repeats a full status snapshot while members work burns one model
round-trip per check and produces nothing. Measured on one run: 35 `team-run
status` calls at a 58 s median gap, including 27 calls in a 25-minute window in
which the Host changed no files.

Three rules:

1. **Block, do not poll.** `harness team-run wait --id <run> [--after-seq N]
   [--timeout-secs S]` returns as soon as a team-run event passes the cursor,
   with the new events and a `next_after_seq` to pass to the next wait. With no
   `--after-seq` it anchors on the current maximum, so a bare wait means "what
   happens next", not "replay this run". Use `team-run events --after-seq` when
   a non-blocking incremental read is enough; reserve the full `status`
   snapshot for re-anchoring after a compaction or a handoff.
2. **Work your own lane while waiting.** Between two observations the Host must
   do something that changes state — a patch, a build, a review of a delivered
   handoff. A turn whose only actions are observations is a defect, not
   diligence.
3. **A silent member is a question the control plane cannot answer.** `wait`
   tells you *whether* something happened; it cannot tell you what a member is
   doing while it is quiet. Before steering or interrupting, read that member's
   own provider-native session through its `NativeSessionRef` with a bounded
   forensics probe. Never load a whole rollout to check on progress.

Budget check for a Wave: more than ~30 harness calls in an hour against fewer
than ~10 state-changing actions means the Host is idling in orchestration, and
the Wave should be re-planned rather than polled harder.

At every safe Host turn boundary—session start, after the user sends a new
prompt, before re-planning, and before accepting a handoff—read the Lead Inbox.
Member mail is durable immediately, but it does not asynchronously interrupt
the Host's current reasoning:

```bash
harness team-run inbox --id <team-run-id> \
  --member-run-id host --json
```

For each actionable message:

1. inspect its sender, kind, Assignment correlation, causation, and body;
2. acknowledge receipt explicitly;
3. send a causation-linked reply when a semantic response is needed; and
4. keep acceptance separate from receipt.

Treat correlation and causation as two different facts. Correlation stays
anchored to the Member's Assignment across every round. Causation names the
exact message being answered; a later Member Handoff should therefore point to
the Host or peer follow-up that triggered that round, not always back to the
initial Assignment.

```bash
harness team-run ack --id <team-run-id> \
  --message-id <message-id> --member-id host
harness team-run send --id <team-run-id> --from host --to <member-run-id> \
  --kind message --body "<answer, revision, or acceptance decision>" \
  --correlation-id <assignment-correlation> --causation-id <message-id>
```

Bind every TeamRun to this exact native Host task using `host_surface` and
`host_thread_id`. Never read all active runs merely because they share a
project:

```bash
harness team-run host-inbox --surface <provider-surface> \
  --thread-id <native-host-task-id> --json
harness team-run bind-host --id <run> --surface <provider-surface> \
  --thread-id <native-host-task-id>
```

The Star Harness Plugin injects a bounded `Needs you` summary at supported
SessionStart and user-prompt boundaries. Codex and Claude Stop hooks use their
reviewed structured continuation response; Kimi uses its reviewed blocking
exit contract. Each may continue the same native task once when actionable mail
arrived while the Host was busy. No hook interrupts the middle of a turn, loops
after `stop_hook_active`, scans another native task's Inbox, or marks mail read.

Treat hook context as orientation, not mailbox truth: read the canonical Inbox
before acting. No hook may silently ACK, answer, or accept. If a Desktop/CLI
task is already idle and Harness does not own its live provider connection,
mail remains durable until the next prompt or resume; knowing a thread id is
not authority to claim background wake.

The Host owns Member lifecycle explicitly:

```bash
# create/add and assign
harness team-run add-member --id <run> \
  --member "Builder:Feature owner:codex/app-server" \
  --assignment "<end-to-end responsibility>"

# inspect, communicate, interrupt one turn, or end the runtime
harness team-run status --id <run>
harness team-run inbox --id <run> --member-run-id host --all
harness team-run send --id <run> --from host --to <member> \
  --kind message --body "<follow-up>" --correlation-id <assignment-correlation>
harness team-run close-member --id <run> --member-run-id <member> \
  --reason "<Host decision>"
```

Use `team_run_interrupt_member` only to interrupt the current provider turn;
use `team_run_close_member` to end the Member runtime. A resumed Member is
created or added with an explicit provider-owned native session id. Turn
completion and Handoff return an unclosed Member to `idle`; later Host or peer
mail wakes that same MemberRun/session. Wave, TeamRun, and Mission completion
never imply Close.

One durable Supervisor lease owns every live control and message-delivery claim
for the TeamRun. `team-run status` must show a current lease before the Host
claims a live control is available. Controls from another Dashboard, MCP, CLI,
or Harness process route through the current lease locator to the owning
Supervisor; that owner fences id, generation, status, and expiry again
immediately before the Provider operation. After a process crash, start the
TeamRun again to acquire the next generation and reattach unclosed Members to
their recorded native sessions. A delivery left `claimed` is uncertain:
inspect the provider-native record and explicitly reconcile it with a provider
receipt or return it to `queued`. Never auto-replay it.

## Write Useful Context

Prefer one readable Markdown body over many rigid fields:

```markdown
# Wave 2 — Integrate and continue review

The baseline passed. Integrate the completed build lane now. Keep Reviewer on
the same MemberRun and native session from Wave 1.

| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| Builder | Primary builder | Integrate the accepted baseline | Patch and checks |
| Reviewer | Interaction reviewer | Continue pending-input validation | Review report |
| Repair | Fixer | Join only if a real defect appears | Focused fix |

## Host judgment
Advance without waiting for Reviewer. Add Repair only after a reproducible bug.
```

Record the decision, not routine narration. Update the Wave when a blocker,
assignment, member composition, integration decision, or expected outcome
materially changes.

## Use The CLI As The Complete Path

Select the project explicitly before mutation:

```bash
harness project switch <project-id-or-path>
```

Create intent, team relation, and the first Host memo:

```bash
harness mission create --title "<title>" --objective "<objective>" \
  --context "<mission-markdown>" --json
harness mission create-team --id <mission-id> --name "<team>" \
  --description "<purpose>" --lead host --member <agent-member-id>
harness wave create --mission-id <mission-id> --title "<wave-title>" \
  --objective "<short objective>" --context "<wave-markdown>" \
  --updated-by host --json
```

Start a long-lived Mission-scoped run from the linked team definition:

```bash
harness team-run create --mission-id <mission-id> \
  --agent-team-id <team-id> --objective "<team objective>" \
  --member-owned-path <member-name>:crates --json
harness team-run start --id <team-run-id>
```

When a reusable team contains durable `AgentMember` identifiers, creating its
TeamRun preserves each identifier as `MemberRun.agent_member_id`. A Company OS
`StandingAgent` may join that execution only by setting
`execution_agent_member_ref` to the `AgentMember.id`; equal ids do not bind the
objects. Never infer a Standing Agent from a display name, role, provider,
model, or running process, and never treat runtime status as organization
authority. Use an ad-hoc member for temporary execution that must not appear as
standing organization work.

Assign and evolve work:

```bash
harness team-run send --id <team-run-id> --from host \
  --to <member-run-id> --kind assignment --body "<owned work>" \
  --correlation-id <stable-work-id> --origin-wave-id <wave-id>
harness team-run send --id <team-run-id> --from host \
  --to <member-run-id> --kind message \
  --body "Return a Markdown plan first; do not execute. Resolve: <questions>" \
  --correlation-id <stable-work-id> --causation-id <assignment-message-id>
harness team-run add-member --id <team-run-id> \
  --member repair:fixer:codex --assignment "<repair work>" \
  --origin-wave-id <wave-id>
harness team-run rename-member --id <team-run-id> \
  --member-run-id <member-run-id> --name "<new display name>"
harness team-run deactivate-member --id <team-run-id> \
  --member-run-id <member-run-id> --reason "<why this lane is no longer needed>"
harness wave update --id <wave-id> --context "<revised-markdown>" \
  --updated-by host
```

Read Lead and member mailboxes:

```bash
harness team-run inbox --id <team-run-id> \
  --member-run-id <member-run-id> --json
harness team-run inbox --id <team-run-id> \
  --member-run-id <member-run-id> --all --json
harness member-run show --id <member-run-id> --json
```

`member-run show` explains one Member's assignment, mailbox, latest action,
native-session binding, stable Agent Inbox routes, Supervisor, handoff, and
runtime facts. It does not mirror the provider transcript.

When a reusable Team member has `MemberRun.agent_member_id`, external messages
may first enter that stable Agent identity Inbox. Harness atomically routes the
source Message into one concrete active MemberRun:

```bash
harness agent route-inbox --id <agent-member-id> \
  --member-run-id <member-run-id> --json
```

If exactly one eligible runtime exists, `--member-run-id` may be omitted.
Several active runtimes require explicit selection. The source Message remains
identity-level truth; the resulting TeamMessage owns provider delivery.

## Use Messages Deliberately

| Need | Record |
| --- | --- |
| Start owned work | `assignment` with stable correlation |
| Any natural conversation | `message`, preserving correlation and direct cause |
| Plan first | ordinary `message`: request → Markdown reply → revise/execute |
| Return a lane | `handoff` with checks and evidence |
| Real current-turn injection | Steer only when mode supports it |
| Stop a real provider turn | Interrupt with terminal acknowledgement |

Question, answer, progress, blocker, review, and peer coordination are intents
inside ordinary messages, not additional lifecycle states. Historical
specialized kinds remain readable but are read-only on new public writes.

Provider-pausing questions and approvals are
`PendingInteraction`, not ordinary chat. If live Steer is unsupported, reject
the Steer action. The Host may then deliberately send an ordinary queued
Message for the next round; never silently convert one operation into the
other or fabricate a control ACK.

An unbound MCP connection may author only as the bound Host, an Operator, or a
Service. It may not select `member_run` or `agent_member` by supplying an id.
Member-originated mail comes from that member's bound Provider runtime.

## Debate A Member Plan

Use planning only where review adds value. Keep it ordinary:

```text
assignment
  -> Host message: "plan first; do not execute"
  -> Member message: Markdown plan r1
  -> Host message: challenge assumptions or boundaries
  -> Member message: Markdown plan r2
  -> Host message: "execute revision 2"
  -> execute in the same MemberRun and native session
```

Keep the same Assignment correlation throughout. Do not create a Wave revision
for every Member plan revision. Update or advance the Wave only when Host's
overall plan or judgment boundary changes.

Provider-native Goal or Plan features are optional Member-internal aids. Never
present them as a Harness approval or permission boundary.

Member-to-member messages are allowed inside the same TeamRun and remain
visible to the Lead. They queue for the recipient's next available round and
must preserve Assignment correlation.

Do not implement conditional delivery. If B depends on A, observe A's durable
handoff and explicitly send B's Assignment or review message.

Advance and re-plan without terminating active members:

```bash
harness wave advance --id <wave-id> --outcome "<Host decision>" \
  --advanced-by host --artifact <evidence-ref>
harness wave create --mission-id <mission-id> --title "<next wave>" \
  --objective "<next judgment boundary>" --context "<next-markdown>"
```

Inspect before acting:

```bash
harness mission show --id <mission-id>
harness wave list --mission-id <mission-id>
harness wave history --id <wave-id>
harness team-run status --id <team-run-id>
harness team-run events --id <team-run-id> --after-seq <last-seq>
```

Use MCP only when the Host environment benefits from typed tool discovery. It
must call the same behavior and store as the CLI; never invent an MCP-only
lifecycle or make MCP installation a correctness requirement.

## Handle Deviation

- On a member question, answer through the correlated message or resolve the
  `PendingInteraction`; a provider `completed` frame is not an answer.
- On a reproducible defect, update the Wave judgment, add a repair member, and
  assign the smallest owned surface.
- On incomplete but non-blocking work, explicitly carry the assignment into the
  next Wave without replacing its MemberRun or native session.
- On conflict, make the Host own integration and record the decision in Wave
  context.
- When isolation is useful, let the Member create and use an independent
  same-repository Git worktree without waiting for the Host to allocate it.
  The Host may require isolation when conflict or policy demands it, but the
  Member owns the Git mechanics and reports the actual path, branch,
  commit/checks, and shared-file conflicts; do not create a Worktree scheduler
  or Task Graph.
- On retry, preserve prior attempts and native session references. Resume only
  through a verified provider-native session operation.
- On sensitive external action, stop and obtain Human approval. A Wave advance
  is not approval for payment, deployment, deletion, permission, or legal work.

Revise the current Wave for small changes inside the same Host judgment.
Advance when the plan, roster, responsibility, risk, or next decision boundary
changes materially. An unfinished Member keeps the same MemberRun, Assignment
correlation, Workspace, and native session across that boundary.

## Finish With Evidence

Before claiming completion, verify that another Host can reconstruct:

- Mission context and ordered Wave judgments;
- linked Agent Teams and member composition changes;
- assignment correlation, blockers, handoffs, and Host answers;
- unchanged native session bindings for carried work;
- one current Supervisor generation and reconciled delivery claims;
- typed Host, MemberRun, AgentMember, Operator, and Service authorship;
- stable Agent Inbox routes where an external message entered by durable
  identity;
- explicit Wave advance outcomes and useful artifacts/checks; and
- explicit Mission closeout without team deletion.

Use the Dashboard for navigation and live operational judgment, but treat the
append-only Harness coordination records and provider-native sessions as truth.

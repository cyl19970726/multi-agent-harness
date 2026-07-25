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

Never turn a Wave into a task graph, runtime container, synchronization barrier,
or transcript store. A member may continue working while the Host advances to
the next Wave. Never duplicate provider tool, command, chat, or thinking streams
in Harness.

The Host using this skill is the Team Lead for every Agent Team it creates.
Lead is a control-plane role: form the team, assign work, answer members,
change composition, integrate results, and decide acceptance. Do not create an
implicit Lead MemberRun. Add the Host as a member only when it deliberately owns
an execution lane with its own native session.

Read `docs/product/mission-wave-host-plan.md`, ADR 0034, ADR 0037, and ADR 0038
when the product contract itself is in question. Do not reproduce their schemas
in this skill.

## Choose The Smallest Truthful Executor

| Situation | Choose |
| --- | --- |
| Work fits safely in the Host's current context | Host |
| One lane needs an accountable identity, Workspace, mailbox, sustained chat, or resume | Agent Team Member |
| One member needs a bounded internal design/build/check helper | provider-native Subagent |
| Repeated deterministic steps need owned step state | Dynamic Workflow |

Use separate Members for parallel feature modules that each need end-to-end
design, implementation, and validation. Let each Member use its own subagents.
Use another Reviewer Member when acceptance must be independent; a member's
internal test subagent is not independent review.

## Run The Host Loop

1. **Observe:** inspect the selected project, Mission, Waves, linked teams,
   runs, Lead Inbox, member Inbox state, pending interactions, and outcomes.
2. Create or update the Mission context with the durable objective, constraints,
   and success standard.
3. Create the current Wave as a concise Markdown plan. Include changed facts,
   member responsibilities, open decisions, carry-over, and advance evidence.
4. Link an existing Agent Team or create one under the Mission when durable
   collaborators are useful.
5. Start one Mission-scoped TeamRun for that team. Do not pass `--wave-id` on
   the primary path.
6. Send correlated assignment messages. Use `--origin-wave-id` only for
   navigation and explanation.
7. For a complex or high-risk lane, request a Member plan before starting
   execution. Debate the proposal through correlated feedback, then explicitly
   approve it. Simple work may skip this negotiation.
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
  --agent-team-id <team-id> --objective "<team objective>" --json
harness team-run start --id <team-run-id>
```

Assign and evolve work:

```bash
harness team-run send --id <team-run-id> --from host \
  --to <member-run-id> --kind assignment --body "<owned work>" \
  --correlation-id <stable-work-id> --origin-wave-id <wave-id>
harness team-run send --id <team-run-id> --from host \
  --to <member-run-id> --kind plan_request \
  --body "<what the plan must resolve>" \
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
```

## Use Messages Deliberately

| Need | Record |
| --- | --- |
| Start owned work | `assignment` with stable correlation |
| Ask a complex lane to plan first | `plan_request` caused by the Assignment |
| Member submits or revises its plan | `plan_proposal` caused by request/feedback |
| Host challenges a proposal | `plan_feedback` caused by latest proposal |
| Host allows execution | `plan_approval` caused by latest proposal |
| Member needs a Host/peer decision | `question`, then correlated `answer` |
| Useful checkpoint | `progress` |
| Work cannot continue | `blocker` with needed actor/action |
| Independent inspection | `review_request`, then `review_result` |
| Return a lane | `handoff`, then Lead `review_result` |
| Real current-turn injection | Steer only when mode supports it |
| Stop a real provider turn | Interrupt with terminal acknowledgement |

Provider-pausing questions, approvals, and plan reviews are
`PendingInteraction`, not ordinary chat. Unsupported live Steer becomes a
clearly labeled queued next-round message; never fabricate a control ACK.

The semantic Member plan review is a `TeamMessage` chain. Provider-native Plan
or ExitPlanMode pauses may additionally create a linked `PendingInteraction`,
but provider completion never substitutes for Host `plan_approval`.

## Debate A Member Plan

Use planning only where review adds value. The Host controls the decision; the
Member owns the proposal.

```text
assignment
  -> Host plan_request
  -> Member plan_proposal r1
  -> Host plan_feedback ("argue": challenge assumptions or boundaries)
  -> Member plan_proposal r2
  -> Host plan_approval
  -> execute in the same MemberRun and native session
```

Keep the same Assignment correlation throughout. Do not create a Wave revision
for every Member plan revision. Update or advance the Wave only when Host's
overall plan or judgment boundary changes.

For providers with a native Goal, keep the Assignment projection paused during
plan debate. Activate it only after correlated `plan_approval`; provider Goal
continuation must not cross the Host decision boundary.

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
- explicit Wave advance outcomes and useful artifacts/checks; and
- explicit Mission closeout without team deletion.

Use the Dashboard for navigation and live operational judgment, but treat the
append-only Harness coordination records and provider-native sessions as truth.

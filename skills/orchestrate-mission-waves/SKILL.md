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
Assignment message = owned work
Provider-native session = member execution truth
```

Never turn a Wave into a task graph, runtime container, synchronization barrier,
or transcript store. A member may continue working while the Host advances to
the next Wave. Never duplicate provider tool, command, chat, or thinking streams
in Harness.

Read `docs/product/mission-wave-host-plan.md` and ADR 0034 when the product
contract itself is in question. Do not reproduce their schemas in this skill.

## Run The Host Loop

1. Inspect the selected project and current Mission, Waves, linked teams, runs,
   pending interactions, and messages.
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
7. Continue interacting with members through questions, answers, progress,
   blockers, steer, interrupt, and provider-native resume as capabilities allow.
8. Integrate completed lanes immediately. Do not wait for unrelated members
   merely to make the Wave look complete.
9. Update the current Wave while the judgment is materially unchanged. Advance
   it and create Wave N+1 when the plan changes materially.
10. Close the Mission with an explicit outcome. Leave linked teams and their
    independent lifecycle untouched.

## Write Useful Context

Prefer one readable Markdown body over many rigid fields:

```markdown
# Wave 2 — Integrate and continue review

The baseline passed. Integrate the completed build lane now. Keep Reviewer on
the same MemberRun and native session from Wave 1.

| Member | Role | Responsibility | Deliverable |
| --- | --- | --- | --- |
| Builder | Lead builder | Integrate the accepted baseline | Patch and checks |
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
  --description "<purpose>" --member <agent-member-id>
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

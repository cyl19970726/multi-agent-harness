---
name: agent-team-orchestrator
description: Kimi plugin compatibility entry for Host orchestration. Use when a Kimi Host needs to create, resume, or re-plan a Mission with Host-plan Waves and a persistent Agent Team, including correlated assignments, member changes, carry-over, pending interactions, and Mission closeout. Do not use for one-shot work that fits in the Host context.
---

# Agent Team Orchestrator

This is the Kimi distribution wrapper for the repository's canonical
`orchestrate-mission-waves` skill. Preserve these meanings:

```text
Mission = durable intent and Markdown context
Wave = versioned Host plan and judgment
Agent Team = independent, long-lived collaboration capability
Assignment message = owned work
Provider-native session = member execution truth
```

Read ADR 0034 and `docs/product/mission-wave-host-plan.md` when changing the
contract. Never make this skill a second architecture source.

## Host Loop

1. Select the Workspace explicitly and inspect Mission, Waves, linked teams,
   runs, messages, and PendingInteractions.
2. Create/update Mission context and the current Wave Markdown.
3. Link or create an independent AgentTeam.
4. Start a Mission-scoped TeamRun with `mission_id + agent_team_id`; omit
   `wave_id` on the primary path.
5. Assign work through correlated TeamMessages. Use `origin_wave_id` only as
   navigation provenance.
6. Answer member questions, integrate completed lanes, and keep unrelated work
   active.
7. Update/advance the Wave when Host judgment changes; preserve the same
   MemberRun and native session for carry-over.
8. Add a repair member when a real defect appears.
9. Close the Mission explicitly without deleting or archiving the team.

Prefer the complete Harness CLI path. Use MCP only as a thin typed adapter over
the same store and application behavior.

```bash
harness mission create --title "<title>" --objective "<objective>" \
  --context "<mission-markdown>"
harness mission create-team --id <mission-id> --name "<team>" \
  --description "<purpose>" --member <agent-member-id>
harness wave create --mission-id <mission-id> --title "<wave>" \
  --objective "<objective>" --context "<wave-markdown>"
harness team-run create --mission-id <mission-id> \
  --agent-team-id <team-id> --objective "<objective>"
harness team-run send --id <run-id> --from host --to <member-run-id> \
  --kind assignment --body "<work>" --correlation-id <work-id> \
  --origin-wave-id <wave-id>
harness wave advance --id <wave-id> --outcome "<Host decision>" \
  --advanced-by host
```

Do not store provider transcript, tool, command, file, turn, or thinking
streams in Harness. Resume only through the bound provider-native session.
Treat provider questions/plan reviews as PendingInteractions; `tool completed`
is not a semantic answer.

Before claiming completion, verify Mission/Wave history, linked team,
assignments/correlation, member/native-session continuity, explicit outcomes
and evidence, Host advance decisions, and Mission closeout.

---
description: Create or reuse a Mission-linked Agent Team and start a long-lived TeamRun. Usage: /agent-team:new-run [objective sketch]
---

Use `$ARGUMENTS` as an objective sketch and follow
`[[agent-team-orchestrator]]`.

1. Select the Workspace explicitly and inspect existing Missions, linked teams,
   runs, and provider capabilities.
2. Reuse a suitable independent AgentTeam or propose a new stable team.
3. Ensure the Mission has durable Markdown context and links the chosen team.
4. Write the first Wave Markdown with changed facts, member responsibilities,
   deliverables, open decisions, and advance evidence.
5. Show the proposed roster and provider/mode/model. Do not claim capability
   until the reviewed integration profile proves it.
6. Create the TeamRun with `mission_id + agent_team_id` and no `wave_id`.
7. Send correlated assignments with optional `origin_wave_id`.
8. Start only after the user confirms any requested external or
   cost/permission-sensitive action.

Primary command shape:

```bash
harness mission create --title "..." --objective "..." \
  --context "<mission-markdown>"
harness mission create-team --id <mission-id> --name "..." \
  --description "..." --member <agent-member-id>
harness wave create --mission-id <mission-id> --title "..." \
  --objective "..." --context "<wave-markdown>"
harness team-run create --mission-id <mission-id> \
  --agent-team-id <team-id> --objective "..."
harness team-run start --id <run-id>
```

Report the Mission, Wave, Team, TeamRun, exact Workspace-scoped Dashboard URL,
and any capability degradation. Never invent an id when a command fails.

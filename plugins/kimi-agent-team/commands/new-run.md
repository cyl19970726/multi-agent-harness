---
description: Create a new AgentTeamRun — guides the user through objective and member configuration, then runs harness team-run create. Usage: /agent-team:new-run [initial objective sketch]
---

Create and (after explicit user confirmation) start a new Agent Team run.

Seed: `$ARGUMENTS` may contain an initial objective sketch; treat it as a
starting point, not a final spec.

Follow the [[agent-team-orchestrator]] method while doing this.

1. **Mission and Wave context.** Ask for the Mission (durable outcome) and a
   one-paragraph Wave objective (skip only when `$ARGUMENTS` states both
   clearly). An `AgentTeamRun` is one attempt for that Wave; if the objective
   crosses an integration boundary, suggest ordered Waves and create only the
   first attempt. Create a native Mission and an `agent_team` Wave when they do
   not already exist; otherwise select their existing ids.
2. **Member roster.** Propose a member list and confirm it with the user
   before creating anything. For each member show exactly one line:

```text
name:role:provider[:model][@path1,path2]   — what this lane owns
```

   Rules:
   - provider must currently be `kimi`; Codex/Claude Member adapters are not
     executable yet; model is optional;
   - `@paths` are that member's ownedPaths — explicit and pairwise disjoint;
   - every member gets a role and a completion standard (the member prompt
     contract is in [[agent-team-member]]; the CLI carries the roster, the
     prompt contract is attached per member by the harness);
   - include a Lead/integrator lane when lanes must be merged.
3. **Run options.** Ask about `--budget-usd X` (recommend always setting one).
   Use `--mission-id` and `--wave-id`; the native Wave owns its ordering.
4. **Assemble and show the command**, e.g.:

```bash
harness mission create \
  --title "..." \
  --objective "..." \
  --desired-outcome "..."
harness wave create \
  --mission-id <mission-id> \
  --title "..." \
  --objective "..." \
  --executor-kind agent_team
harness team-run create \
  --mission-id <mission-id> \
  --wave-id <wave-id> \
  --objective "..." \
  --budget-usd 25 \
  --member lead:integrator:kimi \
  --member api:backend:kimi@crates/harness-store,crates/harness-core \
  --member ui:reviewer:kimi@apps/web
```

   Get the user's explicit go-ahead, then execute it.
5. **Report the result:** parse the created run id from the output and print:

```text
Run created: <run-id>
Mission/Wave context: <mission> / <wave>
Team Console: <use the exact Workspace-scoped dashboard_url returned by MCP>
```

6. **Start only on confirmation.** Ask whether to launch now; if yes call MCP
   `team_run_start` and report its immediately returned `running` projection
   and deep link. Use `harness team-run start --id <run-id>` only as fallback.
   Otherwise remind
   the user the run stays in `planning` until started.

If the `harness` CLI is missing or any command fails, report the error
verbatim and stop — never invent a run id.

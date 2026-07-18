---
description: Create a new AgentTeamRun — guides the user through objective and member configuration, then runs harness team-run create. Usage: /agent-team:new-run [initial objective sketch]
---

Create and (after explicit user confirmation) start a new Agent Team run.

Seed: `$ARGUMENTS` may contain an initial objective sketch; treat it as a
starting point, not a final spec.

Follow the [[agent-team-orchestrator]] method while doing this.

1. **Objective.** Ask the user for a one-paragraph objective for this wave
   (skip if `$ARGUMENTS` already states it clearly). One wave = one run;
   if the objective obviously spans an integration boundary, suggest
   splitting it into waves and create only the first.
2. **Member roster.** Propose a member list and confirm it with the user
   before creating anything. For each member show exactly one line:

```text
name:role:provider[:model][@path1,path2]   — what this lane owns
```

   Rules:
   - provider is one of `codex|claude|kimi`; model is optional;
   - `@paths` are that member's ownedPaths — explicit and pairwise disjoint;
   - every member gets a role and a completion standard (the member prompt
     contract is in [[agent-team-member]]; the CLI carries the roster, the
     prompt contract is attached per member by the harness);
   - include a Lead/integrator lane when lanes must be merged.
3. **Run options.** Ask about `--wave N` (default: omit) and
   `--budget-usd X` (recommend always setting one).
4. **Assemble and show the command**, e.g.:

```bash
harness team-run create \
  --objective "..." \
  --wave 2 \
  --budget-usd 25 \
  --member lead:integrator:kimi \
  --member api:backend:codex:@crates/harness-store,crates/harness-core \
  --member ui:frontend:claude:claude-sonnet-4@apps/web
```

   Get the user's explicit go-ahead, then execute it.
5. **Report the result:** parse the created run id from the output and print:

```text
Run created: <run-id>
Team Console: http://127.0.0.1:8787/team-console  (requires `harness serve --addr 127.0.0.1:8787`; /agent-team:dashboard opens it)
```

6. **Start only on confirmation.** Ask whether to launch now; if yes run
   `harness team-run start --id <run-id>` and confirm the run moved to
   `running` via `harness team-run status --id <run-id>`. Otherwise remind
   the user the run stays in `planning` until started.

If the `harness` CLI is missing or any command fails, report the error
verbatim and stop — never invent a run id.

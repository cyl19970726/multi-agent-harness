---
description: Show a compact status table of an agent team run (members, state, heartbeat, un-ACKed) plus the Team Console URL. Usage: /agent-team:status [run-id]
---

Render the live status of an Agent Team run as a compact cockpit table.

1. Resolve the run id:
   - If `$ARGUMENTS` is non-empty, use it as the run id.
   - Otherwise run `harness team-run list --json` and pick the most recent
     run whose status is one of `planning|running|waiting|reviewing|blocked`.
     If none is active, say so, list the last 3 runs (id / status / Mission/Wave
     context / objective, one line each), and stop.
2. Run `harness team-run status --id <run-id> --json` and
   `harness team-run events --id <run-id> --json`. If the harness CLI is
   missing or errors, report that plainly and stop — do not fabricate state.
3. Print, in this order:
   - Header line: run id, status, Mission/Wave context, budget used/limit if
     present, elapsed time if present.
   - One markdown table, one row per MemberRun, columns:
     `member | provider | status | current assignment | current action | heartbeat | un-ACKed`.
     Use assignment-message id / `correlation_id` as the target lane identity.
     If that join is absent, show it as unavailable; do not infer it from another
     field or fabricate a correlation join.
     Keep cell text short (truncate with …); this table is the compact
     projection of the Browser Team Console, not a transcript.
   - Alerts section: any members in `blocked`, any `waiting_for_approval` /
     authorization-gate actions, any un-ACKed deliveries past threshold —
     each as one line with the member id and what is needed from the user.
   - The last 5 TeamRunEvents as `seq | source | summary` lines.
4. Always end with the console line, exactly:

```text
Team Console: http://127.0.0.1:8787/team-console  (requires `harness serve --addr 127.0.0.1:8787`; /agent-team:dashboard opens it)
```

The CLI text view and the web console render the same read model — never
imply they could disagree.

---
description: Print and open the Workspace-scoped Star Harness Dashboard deep link. Usage: /agent-team:dashboard [run-id]
---

1. Resolve the TeamRun id from `$ARGUMENTS` or the latest active run.
2. Run `harness team-run status --id <run-id> --json`.
3. Use the exact `dashboard_url` from that response. Do not reconstruct a URL
   or confuse the API service on port 8787 with the Vite UI on port 5173.
4. If the API or UI is unavailable, report the missing process and the
   documented start commands. Do not start a long-running service without
   telling the user.
5. Open the exact URL with the platform browser when supported, and always
   print it.

Preserve `project`, `surface`, `team`, and any Mission/Wave/member deep-link
parameters. One Dashboard service may manage several Workspaces.

---
description: Print the Browser Team Console URL and try to open it (macOS `open`, Linux `xdg-open`). Usage: /agent-team:dashboard
---

Open the Browser Team Console for live Agent Team observation.

1. Check whether the harness dashboard is serving:
   `curl -sf -o /dev/null --max-time 3 http://127.0.0.1:8787/team-console`
2. If it is not serving, start it in the background:
   `harness serve --addr 127.0.0.1:8787`
   then re-check once with the same curl. If the `harness` binary is missing,
   tell the user to install/build it (`cargo install --path crates/harness-cli`
   from the multi-agent-harness repo) and stop.
3. Try to open the page in the user's browser:
   - macOS: `open http://127.0.0.1:8787/team-console`
   - Linux: `xdg-open http://127.0.0.1:8787/team-console`
   Detect the OS with `uname -s`; if neither opener exists, skip silently.
4. Print the URL regardless of whether opening worked, exactly:

```text
Team Console: http://127.0.0.1:8787/team-console
```

If there is an active run (check `harness team-run list --json`), mention
its id and status on one line so the user knows what the console will show.

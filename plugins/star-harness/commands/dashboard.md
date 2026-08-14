---
description: Compatibility basename for the historical agent-team dashboard command.
---

Resolve the requested or latest active TeamRun with Harness CLI and open or
print its exact `dashboard_url`. Preserve project, Mission, TeamRun, and
MemberRun deep-link parameters. Preserve a `wave` parameter only when it is
already present in the server-returned URL: it is Legacy read-only compatibility
data, not a current object to infer or create. Do not reconstruct the URL.

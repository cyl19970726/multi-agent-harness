# Star Harness — Kimi Code Distribution

This provider-specific package installs the shared Star Harness experience into
Kimi Code. Runtime and product semantics remain in Harness; the package only
provides a thin Host skill, shortcuts, optional MCP registration, and lifecycle
hooks.

The current model is:

```text
Mission -> ordered Host-plan Wave
Mission <-> independent AgentTeam -> TeamRun -> MemberRun
```

A TeamRun may span Waves. Assignment messages own work; provider-native
sessions own transcripts, tools, commands, files, turns, and resume.
The Host that creates and coordinates a team is its Team Lead. Lead remains
outside the MemberRun roster unless explicitly added to execute a lane.

## Prerequisites

- `harness` is on `PATH`.
- A Workspace is explicitly selected.
- For the web UI, run the Harness API and Vite Dashboard; use the exact
  Workspace-scoped deep link returned by the CLI/MCP response.

## Contents

| Part | Responsibility |
| --- | --- |
| Host skill | Mission context, Host-plan Waves, team/member changes, carry-over and closeout |
| Member skill | assignment, evidence, blocker and handoff contract |
| CLI commands | create/status/dashboard shortcuts over canonical CLI behavior |
| Optional MCP | typed adapter over the same application services and store |
| Hooks | fail-open active-run/status injection |

Codex batch, Codex app-server, Kimi ACP, and Claude CLI are executable member
modes when their reviewed provider profiles are available. Capability is
mode/version-specific; the plugin never silently substitutes another provider.

## Primary CLI Path

```bash
harness mission create --title "..." --objective "..." --context "..."
harness mission create-team --id <mission-id> --name "..." \
  --description "..." --lead host --member <agent-member-id>
harness wave create --mission-id <mission-id> --title "..." \
  --objective "..." --context "..."
harness team-run create --mission-id <mission-id> \
  --agent-team-id <team-id> --objective "..."
harness team-run start --id <run-id>
harness team-run status --id <run-id> --json
harness wave advance --id <wave-id> --outcome "..." --advanced-by host
```

Use MCP when Kimi benefits from typed tool discovery. It is not required for
correctness and owns no storage or lifecycle.

## Ground Rules

- Wave is Host memory, never a TeamRun container or barrier.
- The current Host is Team Lead; do not invent a Lead MemberRun.
- Mission closeout never deletes or archives a team.
- Provider transcripts and thinking are never mirrored into Harness.
- Pending questions/approvals require semantic resolution; a provider
  `completed` frame is insufficient.
- Deploy, payment, legal submission, remote deletion, permission, and
  organization changes require the applicable Human approval.
- Provider upgrades always require explicit Human confirmation and adapter
  review.

## Canonical References

- [Host-plan product contract](../../docs/product/mission-wave-host-plan.md)
- [ADR 0034](../../docs/decisions/0034-host-plan-waves-and-mission-teams.md)
- [Provider integration model](../../docs/agent-integration-model.md)
- [Mission Canvas](../../docs/dashboard/pages/mission-wave-canvas.md)
- [Team War Room](../../docs/dashboard/pages/team-run-war-room.md)

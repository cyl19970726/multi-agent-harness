# Workbench Layout V2 visual contract

This directory is the durable visual contract for the Mission / Wave / Agent
Team Workbench redesign.

The evidence chain is:

```text
current browser baseline -> approved expected design -> implemented browser
-> labeled comparison -> review decision
```

`expected/`, `prompts/`, selected `comparisons/`, and reviews are versioned.
Raw repeatable browser captures live under the ignored
`.visual-evidence/workbench-layout-v2/` directory. Selected baseline and final
screenshots may be promoted here when they materially explain a product
decision.

The first approved design set covers:

- `member-run-focus`: run-scoped Member activity/chat with Wave and Team context;
- `team-war-room`: one AgentTeamRun attempt inside a Wave;
- `mission-wave-canvas`: ordered Waves with compact executor controls and a
  separate Wave gate.

Standing Agent uses the same workspace grammar but remains a different product
object and requires its own expected design before its page is replaced.

The deterministic pressure fixture is versioned at
`apps/agent-dashboard/fixtures/workbench-layout-v2-native-v1/`. It contains no
legacy dependency graph and no persisted thinking. The capture runner injects one sanitized
live-only preview after SSE connects, captures all three P0 pages at desktop,
tablet, and mobile, and records the context-open state at tablet and mobile
sizes before removing its temporary store.

```bash
npx pnpm@9.15.4 exec playwright install chromium
npx pnpm@9.15.4 visual:capture:workbench
```

Raw captures and `capture-run.json` remain under the ignored
`.visual-evidence/workbench-layout-v2/implemented/` directory. Selected
three-way comparisons are promoted here. P1 expected designs are candidates
only and remain unapproved.

Standing Agent uses a separate baseline fixture at
`apps/agent-dashboard/fixtures/workbench-layout-v2-standing-agent-v1/`. It
contains durable identity, runtime, sessions, messages, and events but
intentionally contains no Mission/Wave/Team/Workflow records, because the
current model cannot prove those assignments belong to the AgentMember. Run:

```bash
node scripts/capture-standing-agent-focus-baseline.mjs
```

Its candidate design is blocked on the explicit availability/capacity and
`StandingAssignment` contracts documented in
[`../../dashboard/pages/standing-agent-focus.md`](../../dashboard/pages/standing-agent-focus.md).

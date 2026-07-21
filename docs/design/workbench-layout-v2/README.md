# Remaining Workbench Layout V2 contracts

Mission/Wave Canvas and Agent Team War Room moved to the active
[`../execution-workbench-v3/`](../execution-workbench-v3/README.md) contract.
Their V2 expected images, prompts, comparisons, and manifest cases were deleted
after the V3 implementation was accepted. Do not use this directory as the
Mission or Agent Team visual baseline.

This directory temporarily retains only page contracts that do not yet have an
approved V3 replacement: MemberRun Focus, Standing Agent Focus, Missions
Collection, WorkflowRun Focus, and shared context-control candidates.

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

The remaining approved design covers `member-run-focus`: run-scoped Member
activity/chat with Wave and Team context.

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

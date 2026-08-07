# Asset inventory · team-war-room

| Asset | Role | Source strategy | Required sizes/states | Status |
| --- | --- | --- | --- | --- |
| Agent portrait set | Identifies Host and every Member in mailbox and messages | Reuse `agent-portrait-set-v1.png` through deterministic avatar mapping | 28, 32, 40 px; online/running/blocked/completed | ready |
| Message-kind icons | Assignment, plan, question, blocker, handoff, review and evidence | Lucide React; semantic icon mapping in code | 14–18 px; default/active/pressure | ready |
| Mailbox icons | Inbox, outbox, queued, delivered and acknowledged | Lucide React; token-colored, never rasterized | 14–16 px | ready |
| Delivery marks | Delivery and ACK visibility | Existing status tokens plus Lucide checks/clock | queued/delivered/acknowledged | ready |
| Expected reference | Approved full Team War Room composition | `expected/team-war-room/team-activity-mailboxes-group-chat-v3-1536x1024.png` | 1536×1024 | approved |

No new decorative raster asset is required. Portrait identity and semantic
icons are the only visual assets; all containers, rules and state indicators
must remain token-driven UI so they survive responsive layouts.

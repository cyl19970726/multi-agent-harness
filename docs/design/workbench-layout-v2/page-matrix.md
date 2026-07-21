# Page matrix

| Priority | Page | Representative state | Expected design | Browser coverage |
| --- | --- | --- | --- | --- |
| P0 | Shared Workbench shell | active Mission/Wave/Team context | represented in all three approved designs | desktop, tablet, mobile |
| P0 | MemberRun Focus | assigned and active, messages/actions/evidence, live-only preview | approved | desktop, tablet, mobile |
| P1 | Standing Agent Focus | available, non-exclusive cross-Mission/Workflow assignments | candidate generated; object-contract approval required | desktop, tablet, mobile |
| P1 | Mission collection | active/recent/empty | required before redesign | desktop, mobile |
| P1 | WorkflowRun Focus | Wave-scoped execution, failure/output | required before redesign | desktop, mobile |
| P1 | Gate review | accepted/revise/blocked decision | may be a Context Rail module | desktop, mobile |
| P1 | Entity control gallery | Wave/Team/Member micro, compact, panel variants | required as component sheet | desktop |
| P2 | System states | offline, loading, empty, error, debug | no generated mockup required | implemented regression |

Default viewports:

- `desktop-1440x1000`
- `tablet-900x1180`
- `mobile-390x844`

Generated expected images prioritize desktop product direction. Tablet and
mobile are accepted from real implementations unless the shell hierarchy
cannot be resolved without another design round.

Current responsive implementation evidence for MemberRun Focus uses the fixed
`workbench-layout-v2-native-v1` fixture, Chromium recorded in
`capture-run.json`, a fixed clock, and explicit `_store` project scope. The run
passes console-error and horizontal-overflow checks. Thinking is injected only
through the live SSE ingress after the Member page subscribes and is absent from
every fixture ledger.

Mission/Wave and Agent Team acceptance moved to
[`../execution-workbench-v3/`](../execution-workbench-v3/README.md).

Standing Agent is the next gated page. Its candidate expected image exists, but
implementation is intentionally held until explicit availability/capacity and
cross-executor `StandingAssignment` projection rules are approved. The durable
contract is recorded in
[`../../dashboard/pages/standing-agent-focus.md`](../../dashboard/pages/standing-agent-focus.md).

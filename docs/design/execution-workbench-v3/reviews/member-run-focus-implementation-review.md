# MemberRun Focus V3 implementation review

Status: candidate ready; explicit expected-image approval pending.

## What passes

- The center is one continuous MemberRun work surface, not an overview card grid.
- Assignment, live-only provider preview, evidence, handoff, and review pressure
  use distinct semantic nodes and remain tied to native fixture records.
- The default six-node projection keeps the latest review request in the first
  desktop viewport; `Full record` reveals all records without rewriting history.
- The right rail composes Wave, Team, Assignment, outputs, runtime, and observed
  delegation context without treating a MemberRun as a Standing Agent.
- Tablet Context opens as a right sheet; mobile Context opens as a bottom sheet.
- Thinking remains a sanitized transient preview and is absent from durable
  selectors, replay evidence, and acceptance truth.

## Intentional deviations from the generated candidate

- The implemented shell retains live connection controls and search because
  they are shared operator functions, while the generated concept simplified
  them.
- Fixture-native content replaces generated names, times, paths, artifacts, and
  status values.
- The actual activity rows are denser than the concept so the operator can see
  the complete causal path through review pressure without scrolling.

## Evidence

- Final captures: `.visual-evidence/execution-workbench-v3/member-run-v3-final/`
- Durable actuals: `../implemented/member-run-focus/`
- Expected/actual: `../comparisons/member-run-focus/`
- 50% overlays: `../overlays/member-run-focus/`
- Automated contract: `npx pnpm@9.15.4 check:dashboard`

No remaining product-truth or responsive-layout defect blocks candidate review.
Real-device software-keyboard behavior remains a follow-up, not a fixture claim.

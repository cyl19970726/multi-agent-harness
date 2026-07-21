# Team Activity visual correction

Date: 2026-07-21

Contract: `team-war-room--running-needs-you--desktop`

Result: pass with documented product-truth deviations

## Why the prior implementation was reopened

The prior Team Activity implementation reused a generic activity row. It mixed time into metadata, did not reserve independent columns for the semantic node and review action, and rendered every durable detail into the default stream. The records were truthful, but the first viewport no longer communicated the attempt, assignments, and current QA pressure with the hierarchy approved in the expected design.

## Corrected projection

Desktop now uses four independent columns: semantic node and continuous spine, timestamp, durable content, and optional pressure action. The default `All` projection contains attempt creation, assignment ownership, and the latest pressure record. A one-click filter control reveals the full durable record; category tabs also operate over that complete record.

Mobile preserves the semantic node and content while moving the timestamp into record metadata. It does not pretend the hidden detail records do not exist, and it does not persist or render provider thinking.

## Evidence

- Iteration 1: `.visual-evidence/execution-workbench-v3/team-activity-iteration-1/`
- Final: `.visual-evidence/execution-workbench-v3/team-activity-final/`
- Comparison: `../comparisons/team-war-room/running-needs-you--team-activity-correction.png`
- Overlay: `../overlays/team-war-room/running-needs-you--team-activity-correction.png`

## Gates

- Product truth: pass. Mission, Wave, attempt, MemberRuns, assignment correlations, actions, messages, evidence, and QA pressure remain derived from native fixture records.
- Visual fidelity: pass. The final fixed-viewport capture matches the approved Team Activity information architecture and responsive transform.

Remaining deviations are fixture truth (timestamps, provider/model labels, and exact record wording), not invented design data.

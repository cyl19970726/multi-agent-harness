# Implementation Loop

Use this reference after page specs, page-local layout contracts, and the
architecture decision exist. It is not a substitute for design.

## Slice Loop

Implement in narrow slices:

```text
select slice from page-local layout contract
  -> implement only the owned files for that slice
  -> run local build/type checks
  -> open browser at required viewport
  -> capture screenshot
  -> Implementation Questioner compares screenshot to spec
  -> fix local P1/P2 or stop on P0
  -> record pass/fail before next slice
```

Do not finish the whole frontend before the first screenshot review. The first
shell screenshot is a gate.

## Screenshot Review Card

Each implementation screenshot needs a written review:

```text
Screenshot Review
  route:
  viewport:
  screenshot_ref:
  first_impression:
  workbench_or_dashboard:
  matched_ascii_spec:
  visible_primary_surface:
  visible_secondary_surface:
  agent_member_as_workspace:
  team_as_collaboration_space:
  goal_task_docs_connected:
  debug_secondary:
  overflow_result:
  console_result:
  decision: pass | fix | reject
```

## P0 Stop Conditions

Stop implementation and record a rejected implementation when:

- the first major shell screenshot looks like a dashboard, card dump, report
  page, or raw object viewer;
- implementation does not match the accepted page-local layout ASCII/contract;
- old Dashboard components still define the primary viewport;
- AgentTeam is only a roster;
- AgentMember is only a side card;
- Goal is only a task list;
- Task is only a status card;
- Docs, Evidence, Decision, and Warnings are disconnected tabs instead of
  workflow context;
- mobile or tablet becomes a long stacked report;
- the developer must explain the product shape because the screenshot cannot.

## Rejected Implementation Record

Append a dated entry to the decision ledger in
`docs/dashboard/layout-history.md`:

```text
Rejected Implementation
  attempt:
  branch_or_pr:
  screenshot_refs:
  first_impression:
  violated_hard_gates:
  mismatch_with_selected_layout:
  old_dashboard_contamination:
  why_not_patchable:
  what_to_restart_from:
  code_disposition:
  reviewer_decision:
```

After rejection, do not keep patching the same UI direction unless the Reviewer
records a narrowed restart point and updated page-local layout contracts.

## PM/User Handoff

PM and User acceptance starts only after:

- all required screenshot review cards exist;
- desktop/tablet/mobile matrix exists;
- console and overflow proof exist;
- rejected attempts are recorded;
- no P0/P1 screenshot critic finding remains.

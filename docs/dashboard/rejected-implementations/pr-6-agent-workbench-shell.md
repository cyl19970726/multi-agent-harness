# Rejected Implementation: PR #6 Agent Workbench Shell

```text
attempt: PR #6 Agent Workbench shell
branch_or_pr: task/agent-workbench-shell-implementation / GitHub PR #6
status: rejected
reviewer_decision: do not merge; restart from page specs, architecture decision,
  and page-local layout contracts
```

## Screenshot Refs

Representative local screenshot from browser review:

```text
/Users/hhh0x/multi-agent-harness/current-agent-workbench-open.png
```

The screenshot is not committed evidence, but it triggered this rejection. PR
records should attach stable screenshots when this branch is updated or closed.

## First Impression

The first viewport reads as a dense dashboard/card arrangement, not a
multi-agent collaboration workbench. The UI shows many canonical objects, but
the product shape is still a tabbed card dump with a right detail panel.

## Violated Hard Gates

- Screenshot first impression is not a Feishu-like team workspace.
- AgentMember appears as an inspector card rather than a durable teammate
  workbench.
- Team is closer to a roster/activity panel than a collaboration workspace.
- Goal, Task, Docs, Evidence, Decision, and Warnings are present but feel like
  tabs and cards rather than connected workflow context.
- The prior `agent-workbench-shell-v1` spec was too vague: it allowed
  implementation to satisfy object visibility without satisfying product form.
- PM/User acceptance was too mechanical and overvalued data presence, console
  cleanliness, and lack of overflow.

## Mismatch With Selected Layout

Selected direction was:

```text
Team workspace shell
  + AgentMember as teammate/workbench
  + Goal/Task documents
  + controlled Graph/Kanban
  + mounted Docs/Evidence/Decision context
```

Actual result:

```text
app rail + roster/cards + tabbed workspace + right inspector
  -> many objects visible
  -> weak collaboration-space hierarchy
  -> AgentMember remains detail panel
  -> Goal/Task/Docs context not naturally connected
```

## Old Dashboard Contamination

- The UI still inherited dashboard thinking: cards, tabs, counts, and grouped
  panels as the main composition.
- Old component and read-model assumptions influenced the first viewport.
- Debug/raw state was secondary, but the broader page still felt like a report
  over objects rather than an operational workspace.

## Why Not Patchable

The failure is structural, not spacing/color polish:

- the page hierarchy is wrong;
- the member mental model is wrong;
- the Team workspace lacks a strong collaboration frame;
- page specs and layout contracts were not kept together;
- implementation began from too broad a shell spec and too much old code.

Patching this implementation would likely preserve the same failed information
architecture.

## Restart Point

Restart from:

- [../pages/README.md](../pages/README.md);
- the `## Layout Contract` section inside each changed page spec under
  [../pages/](../pages/);
- [../frontend-architecture.md](../frontend-architecture.md);
- [../acceptance.md](../acceptance.md).

## Code Disposition

The PR #6 implementation should be deleted or quarantined. It must not serve as
the base for the next frontend implementation. Stable pieces may be retained
only after explicit architecture review:

- API helper types;
- pure snapshot TypeScript types;
- pure read-model selectors that do not impose layout;
- Vite/React build setup if the architecture decision keeps it.

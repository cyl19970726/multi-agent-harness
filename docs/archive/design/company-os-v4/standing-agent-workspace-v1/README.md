# Company OS V4 · Standing Agent workspace

This folder is the complete visual contract for the Organization-native
Standing Agent detail page. It replaces the older profile-shaped Actual for
this route without turning a Standing Agent into an Agent Team MemberRun.

## Review order

1. [Baseline → Expected → Actual comparison](comparisons/standing-agent-focus--available--desktop-1536x1024.png)
2. [Approved Expected](expected/standing-agent-focus/document-architecture-agent--available--desktop-1536x1024.png)
3. [Desktop Actual](implemented/standing-agent-focus/document-architecture-agent--available--desktop-1536x1024.png)
4. [Tablet Actual](implemented/standing-agent-focus/document-architecture-agent--available--tablet-context-open-900x1180.png)
5. [Mobile Actual](implemented/standing-agent-focus/document-architecture-agent--available--mobile-context-open-390x844.png)
6. [Review](reviews/standing-agent-workspace-review.md)

The machine-readable [visual contract](visual-contract.json) links baseline,
Expected, Actual, prompt, design decomposition, interaction rules, asset
inventory, implementation iterations, product-truth gate and visual-fidelity
gate. `capture.mjs` reproduces the browser evidence from the canonical Company
OS fixture plus an explicit Standing Agent overlay.

## Product boundary

- Organization owns actor identity, membership, reporting and permission refs.
- Docs owns prompt content and maintained company knowledge.
- Work owns WorkItems and Assignments shown in the center.
- Finance appears only when a WorkItem requests a monetary effect.
- Execution records may be linked, but Mission, Wave, TeamRun, MemberRun and
  provider state do not define this page.
- Private thinking is neither persisted nor rendered as durable activity.

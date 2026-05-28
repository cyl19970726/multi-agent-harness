# Acceptance Gates

## Harness Workflow Acceptance

The frontend must show:

- Vision and Goal collection;
- completed and not-complete goals;
- selected GoalDesign;
- persistent AgentTeam;
- goal-level graph plus goal-level Kanban/lane view;
- dynamic TaskGraph with graph and Kanban/lane views;
- assignment messages;
- AgentMember realtime state;
- evidence, proposal, review, decision, and GoalEvaluation;
- distance-to-vision and next-round proposal.

## Browser Acceptance

Attach evidence:

- desktop screenshot;
- tablet screenshot;
- mobile screenshot;
- console output;
- proof of no page-level horizontal overflow;
- proof that raw/debug surfaces are not primary;
- proof that selecting a member shows realtime activity and send-message UI.

## Web Quality Acceptance

Use a web-quality audit when available, including the external skill source
`https://github.com/addyosmani/web-quality-skills`.

Check:

- accessibility;
- keyboard navigation and focus;
- Core Web Vitals;
- performance on representative snapshots;
- best practices;
- console cleanliness.

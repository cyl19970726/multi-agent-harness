# Company OS four-system relationship visuals

These diagrams explain the organization, responsibility, and collaboration boundaries between
Docs, Organization, Work, and Finance.

1. `organization-overview-ui--1536x1024.png` translates the hierarchy into the
   actual Organization product surface.
2. `hr-governance-agent-focus-ui--1536x1024.png` defines the Org/HR workspace
   for capability-gap decisions and governed Agent lifecycle.
3. `governance-led-organization--1536x1024.png` shows Lead's four Governance
   Agent reports, Org/HR ownership of every Business Agent, and the difference
   between reporting and collaboration.
4. `four-system-responsibility-map--1536x1024.png` shows which system owns each
   kind of company truth and which requests cross the boundaries.
5. `trademark-operating-loop--1536x1024.png` applies the model to the first
   governed acceptance scenario.

The diagrams are explanatory expected assets, not proof that every shown
object or Action has been implemented. The canonical written contract is
[`four-system-collaboration.md`](../../../company-os/four-system-collaboration.md).

`docs-governance-agent-focus-ui` and `work-governance-agent-focus-ui` are
reference concepts only. They explain the decision contracts but are not
near-term implementation baselines. The current product priority is canonical
documentation, Organization Overview, and the company-wide Work interface.
Agent configuration can initially live in a profile or Context Rail showing
responsibility, prompt, tools/Skills, permissions, maintained Docs, and linked
WorkItems.

## Reading rule

Boxes express ownership. Arrows express requests, governed Actions, or linked
projections. An arrow never transfers ownership of the underlying record.

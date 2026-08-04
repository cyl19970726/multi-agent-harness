# Nested Agent Team Organization Implementation Plan

```text
status: proposed; implementation begins only after this Spec PR is accepted
owner_role: product-architecture
```

- [ ] 1. Freeze the target vocabulary and retire conflicting active guidance
  - Make AgentMember the organization-agent identity and Host a Team relation.
  - Amend or supersede fixed Governance-Agent and StandingAgent scheduling text.
  - Mark current StandingAgent/WorkItem joins as compatibility implementation.
  - _Requirements: R1, R4_

- [ ] 2. Extend AgentTeam for persistent recursive topology
  - Add `parent_team_id` and durable `host_member_id`.
  - Enforce direct-host, acyclic graph, one-primary-child-Team V1 invariants.
  - Add recursive tree/store queries and schema fixtures.
  - _Requirements: R1, R6_

- [ ] 3. Promote Work from TeamRun scope to persistent Team scope
  - Add `team_id`, optional execution attempt ref, creator and assignee fields.
  - Keep one WorkEvent/WorkDelivery transition service.
  - Derive assigned/unassigned and ready/not-ready projections.
  - _Requirements: R2, R4_

- [ ] 4. Implement topology-derived Work authority
  - Member create-unassigned and create-self-owned operations.
  - Host assignment limited to self/direct Members.
  - Explicit claimable option; reject default peer claim and peer assignment.
  - _Requirements: R2, R6_

- [ ] 5. Implement recursive delegation
  - Create child Work only under Work owned by the delegating Member.
  - Preserve parent accountability and independent child acceptance.
  - Reject cross-subtree parent refs and sibling administration.
  - _Requirements: R3, R6_

- [ ] 6. Converge Company WorkItem onto the Work kernel
  - Model Document, Milestone, Module, Approval, Finance, Mission, and external
    refs as Work relations/extensions.
  - Produce an explicit migration/export validator for current Company WorkItem
    and Assignment rows.
  - Cut over without dual-read or dual-write owner/status authority.
  - _Requirements: R4_

- [ ] 7. Preserve the Message and delivery boundary
  - Keep authored Message optional and Work-linked.
  - Trigger provider delivery from versioned WorkDelivery, never assignee field
    alone and never Assignment Message.
  - Prove busy/idle/offline/closed/retired behavior.
  - _Requirements: R5_

- [ ] 8. Add the Supervising Operator application service
  - Global tree/Work reads.
  - Unassigned Work creation in an explicit Team scope.
  - Durable Lead messaging without Member impersonation.
  - Deny direct peer assignment, acceptance, and topology mutation.
  - _Requirements: R7_

- [ ] 9. Build the recursive Organization and global Works UX
  - Generate the visual contract before implementation.
  - Reuse Team War Room, Member Focus, mailbox, and Work components.
  - Add Team breadcrumbs, subtree drilldown, global filters, and lineage.
  - Verify desktop, tablet, 390px, and 320px states plus error/empty/loading.
  - _Requirements: R8_

- [ ] 10. Keep Mission/Wave optional
  - Link Work to Mission only when requested by the Host.
  - Prove ordinary nested Team work without Mission/Wave.
  - Prove a multi-Team Mission without changing Work ownership.
  - _Requirements: R9_

- [ ] 11. Run provider and recovery acceptance
  - Real persistent Codex, Claude, and Kimi Members across two Team depths.
  - Work question, blocker/resume, submit/review, and parent integration.
  - Same-contract Supervisor restart and compatible native-session resume.
  - Exactly one WorkDelivery per accepted version.
  - _Requirements: R1, R3, R5_

- [ ] 12. Dogfood the model on AgentOS development
  - Supervising Operator creates an unassigned root Work.
  - Lead assigns CTO; CTO creates a child Team and three child Works.
  - Child Agents implement/review in isolated worktrees.
  - CTO integrates; Lead accepts; source/result Docs update.
  - Organization and global Works reconstruct the complete tree and lineage.
  - _Requirements: all_

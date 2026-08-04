# Data provenance

Every value the five surfaces may render is listed here with its exact field
name, its source contract, and one of three classes:

```text
IMPLEMENTED  schema + store exist today; production UI may render it now
TARGET       PR #302 / ADR 0051 design contract; not shipped; UI may plan
             the slot but must not render it as live data before the
             store/API lands
RESEARCH     directional candidate (ai-first Docs infrastructure); informs
             layout slots only; never asserted in acceptance
```

Rendering rules:

- A UI element must never upgrade a field's class. If the store cannot
  reconstruct a claim, the element is absent or visibly unavailable.
- Derived values state their derivation (e.g. per-node Work counts are
  computed from Work rows, never stored counters).
- Where the target docs disagree with each other or with the shipped schema,
  the discrepancy is listed in §6 and frozen here; UI implementation must
  follow the schema freeze from the PR #302 migration, not paper over it.

## 1. Organization (recursive AgentTeam tree)

| Element | Field(s) | Source | Class |
| --- | --- | --- | --- |
| Team node identity | `AgentTeam.id`, `name`, `purpose` | `specs/nested-agent-team-organization/design.md:62-71` | TARGET (`purpose` absent from `schemas/agent-team.schema.json`) |
| Team node identity today | `AgentTeam.id`, `name`, `description` | `schemas/agent-team.schema.json` | IMPLEMENTED (compat; `owner_agent_id` is the Lead identity, reserved value `host`) |
| Parent/child edges | `AgentTeam.parent_team_id` (null = root), `member_ids[]` (direct children only) | `design.md:62-71` | TARGET (no `parent_team_id` in schema or `crates/`) |
| Host edge | `AgentTeam.host_member_id` | `design.md:62-71` | TARGET |
| Durable Team status | `status = active \| paused \| archived` | `design.md:62-71` | TARGET (implemented enum is `active \| closed \| archived`) |
| Member node identity | `AgentMember.id`, `name`, `role` | `schemas/agent-member.schema.json` | IMPLEMENTED |
| Durable Member status | `AgentMember.status = active \| paused \| retired` | `design.md:39-49` | TARGET (implemented is a 14-value runtime enum: `creating\|idle\|assigned\|running\|waiting_for_input\|waiting_for_approval\|reviewing\|blocked\|closing\|closed\|error\|paused\|stale\|retired`) |
| Runtime state per Member | `MemberRun.status`, `provider_capacity.state`, `native_session.availability`, `last_event_at` | `schemas/member-run.schema.json` | IMPLEMENTED — always labelled as runtime, never merged into durable status |
| Per-node Work counts | derived: count of Work by `status` (`assigned` = has owner, `in_progress`, `blocked`, `review`) | `design.md:276-281` + `schemas/work.schema.json` | IMPLEMENTED derivation today (TeamRun scope); TARGET at Team scope |
| Child-Team drilldown from Member | `AgentTeam.host_member_id` → Member → child Team | `design.md:282` | TARGET |
| Topology integrity | cycle / missing-parent / host-not-in-parent findings | `design.md:62-71` invariants | TARGET — rendered as integrity findings, never auto-repaired |

Invariants the UI must honor: non-root Host is a direct member of the parent
Team; at most one primary child Team per Member in V1; the tree is acyclic;
ancestry is never inferred from names, sessions, assignees, or first-row
fallback (`docs/company-os/nested-agent-team-organization.md:180-182`).

## 2. Global Works

| Element | Field(s) | Source | Class |
| --- | --- | --- | --- |
| Work row identity | `Work.id`, `title`, `status = open\|in_progress\|blocked\|review\|done\|cancelled`, `version` | `schemas/work.schema.json` | IMPLEMENTED |
| Owning scope | `Work.team_id` (persistent Team) | `design.md:89-110` | TARGET (implemented: `team_run_id`) |
| Responsible Member | `Work.assignee_member_id` | `design.md:89-110` | TARGET (implemented: `owner_member_id` + `active_member_run_id`) |
| Claim rules | `claimable = false\|true` | `design.md:89-110` | TARGET (implemented: `claim_mode = host_assign\|team_claim`, `eligible_member_ids[]`) |
| Priority / ordering | `priority = low\|normal\|high\|urgent`, `created_at`, `updated_at` | `schemas/work.schema.json` | IMPLEMENTED |
| Lineage | `parent_work_id`, `prerequisite_work_ids[]` | `schemas/work.schema.json` | IMPLEMENTED (same TeamRun today) |
| Provenance | `Work.source_refs[]` | `design.md:105` | TARGET (implemented: single `source_work_item_ref`; `WorkEvent.causation_ref`) |
| Demand class: discovered-unassigned | derived: `status == open && assignee_member_id == null` | `design.md:89-110`; `docs/company-os/nested-agent-team-organization.md:176-178` | TARGET derivation (today: `open && owner_member_id == null`) |
| Demand class: self-owned | derived: assignee is the viewing Member | same | IMPLEMENTED derivation per viewer |
| Demand class: delegated | `WorkDelegation{parent_work_ref, child_agent_team_id, child_team_run_id}` | `crates/harness-core/src/lib.rs:3081` | IMPLEMENTED in core |
| Demand class: follow-up | derived: `parent_work_id` or `source_refs` naming an originating Work/Document/result/review | `specs/.../requirements.md:232-233` | TARGET derivation |
| Filters | Team path, Host, Member, status, source, milestone | `design.md:286-290` | TARGET (milestone via `WorkRelation kind=milestone`; Company side has `WorkItem.milestone_ref` IMPLEMENTED) |
| Submission state | `result_summary`, `artifact_refs[]`, `check_refs[]`, `blocker_reason` | `schemas/work.schema.json` | IMPLEMENTED — "Awaiting Host acceptance" wording per `docs/product/agent-team-works.md:256` |

## 3. Member Focus (reused Agent Team Member Focus)

| Element | Field(s) | Source | Class |
| --- | --- | --- | --- |
| Current owned Work + completion criteria | `Work` rows where `owner_member_id`/`active_member_run_id` binds this Member; `completion_criteria_markdown` | `schemas/work.schema.json`; `design.md:291-300` | IMPLEMENTED |
| Created Work | `Work.created_by_actor{kind,id}` | `schemas/work.schema.json` | IMPLEMENTED |
| Child Work | `Work.parent_work_id` | `schemas/work.schema.json` | IMPLEMENTED |
| Child Team + direct Members | `AgentTeam` where `host_member_id` == this Member | `design.md:291-300` | TARGET |
| Inbox/Outbox + Work-linked conversation | `TeamMessage{from_member_id, recipients[], work_id?, kind, body, correlation_id, deliveries[]}` | `schemas/team-message.schema.json` | IMPLEMENTED (TARGET moves scope `team_run_id`→`team_id`, renames `body`→`body_markdown`) |
| Runtime/workspace/Provider/native-session facts | `MemberRun.status`, `provider_profile`, `provider_controls`, `provider_capacity`, `workspace_snapshot`, `worktree_ref`, `native_session{}` | `schemas/member-run.schema.json` | IMPLEMENTED — live-only where the provider store is the truth |
| Durable identity | `MemberRun.agent_member_id` → `AgentMember` | `schemas/member-run.schema.json`, `schemas/agent-member.schema.json` | IMPLEMENTED link; durable org fields (business-access ceiling, `created_by_member_id`) are TARGET |
| Pending interactions | `PendingInteraction{kind, route, status, title, prompt, options[]}` | `schemas/pending-interaction.schema.json` | IMPLEMENTED |
| Actions | create unassigned, take own Work, split Work, delegate to direct child | `design.md:300` | TARGET as Member-initiated actions (Host flows exist today); delegate hidden unless a direct child Team exists |

## 4. Team War Room (reused, plus Organization context)

| Element | Field(s) | Source | Class |
| --- | --- | --- | --- |
| Works tab (default) | `Work` rows + `WorkEvent{kind, expected_version, resulting_version, performed_by_actor, causation_ref}` + `WorkDelivery{status, recipient_member_run_id}` | `schemas/work.schema.json`, `schemas/work-event.schema.json`, `schemas/work-delivery.schema.json` | IMPLEMENTED |
| Activity tab | ordered `WorkEvent`, `TeamMessage` deliveries, control acknowledgements | `docs/product/agent-team-works.md` | IMPLEMENTED |
| Members tab | `MemberRun` rows with runtime status, capacity, native-session availability | `schemas/member-run.schema.json` | IMPLEMENTED |
| Mailboxes | `TeamMessage.deliveries[]{policy, status}` | `schemas/team-message.schema.json` | IMPLEMENTED |
| Pending interactions | `PendingInteraction` | `schemas/pending-interaction.schema.json` | IMPLEMENTED |
| Truthful capacity | `provider_capacity.state = available\|limited\|exhausted\|unauthorized\|unknown` | `schemas/member-run.schema.json` | IMPLEMENTED — never a "% utilization" invention |
| Organization breadcrumb | Team path from recursive `parent_team_id` chain | `design.md:302-306` | TARGET |
| Child-Team presence | child `AgentTeam` rows linked from this Team | `design.md:302-306` | TARGET |

## 5. Docs-to-Work handoff

| Element | Field(s) | Source | Class |
| --- | --- | --- | --- |
| Document identity | `document{id, space_id, parent_document_id?, title, kind, lifecycle_status, block_ids[], reference_refs[]}` | `schemas/company-os/knowledge.schema.json` | IMPLEMENTED |
| Block selection | `block{id, document_id, kind, position, content{}, referenced_entities[]}` (`kind` includes `work_item`, `relation_summary`, `decision`) | `schemas/company-os/knowledge.schema.json` | IMPLEMENTED |
| Document revision | `DocumentRevision{id, revision_number, content_digest, authored_by, ...}` | `docs/research/ai-first-multi-device-docs-infrastructure.md:370-387` | RESEARCH |
| Create Work from Document | `WorkItem.source_document_ref` (required), `context_refs[]` | `schemas/company-os/work.schema.json` | IMPLEMENTED relation; creation transport not connected in UI today |
| Create Team Work from Document | `Work.source_refs[]` incl. `DocumentInputRef{document_id, block_id?/anchor?, revision_selector}` | `design.md:105`; research `:530-542` | TARGET (`source_refs`) / RESEARCH (`DocumentInputRef`) |
| Team↔Company bridge | `Work.source_work_item_ref` | `schemas/work.schema.json`; `docs/product/agent-team-works.md:34-37` | IMPLEMENTED — Work status never silently mutates WorkItem |
| Related Works backlink | derived backlink owned by Docs, not a stored relation | research `:525-528` | RESEARCH (today: `RelatedWorkBlock` dedupes `source/context/result` refs) |
| Result return | `WorkItem.result_document_ref`, `result_record_refs[]`; `DocumentResultRef{work_id, document_id, revision_id, result_role}` | `schemas/company-os/work.schema.json`; research `:530-542` | IMPLEMENTED refs / RESEARCH exact-revision pinning |
| Execution chain projection | `WorkExecutionChain{assignment_id, work_item_id, work_id?, link_status = linked\|mismatch\|unavailable, ...}` | `docs/company-os/concept-model.md:334-371` | IMPLEMENTED read-only projection |
| Handoff boundaries | a Document revision does not complete Work; Work `done` does not approve the document; acceptance records the result Document/revision explicitly | research `:556-563` | RESEARCH codified here as contract rule |

Placement honesty: Work creation offers only the three legal placements —
self, unassigned in current Team, or direct-child Team when the viewer is its
Host (`design.md` invariants; research `:603-604`). Unavailable placements are
omitted, not disabled-with-tooltip invention.

## 6. Frozen discrepancies (resolve at schema freeze, not in UI)

1. `Work.team_id` (TARGET) vs `Work.team_run_id` (IMPLEMENTED).
2. `Work.assignee_member_id` (TARGET) vs `owner_member_id` (IMPLEMENTED).
3. `claimable` bool (TARGET) vs `claim_mode` + `eligible_member_ids[]`
   (IMPLEMENTED).
4. `Work.source_refs[]` (TARGET) vs single `source_work_item_ref`
   (IMPLEMENTED).
5. Target docs disagree: `created_by_actor_ref` (`design.md`) vs
   `creator_actor_ref` (`organization-and-actors.md:100-110`).
6. `AgentTeam`: `host_member_id`/`parent_team_id`/`purpose`/status `paused`
   (TARGET) vs `owner_agent_id`/`description`/status `closed` (IMPLEMENTED).
7. `AgentMember.status`: `active|paused|retired` (TARGET) vs 14-value runtime
   enum (IMPLEMENTED); the UI keeps durable and runtime status as separate
   labelled facts until the freeze lands.
8. `TeamMessage.body_markdown` + `team_id` (TARGET docs) vs `body` +
   `team_run_id` (IMPLEMENTED).
9. `DocumentRevision`, `DocumentChangeOperation`, `WorkRelation`,
   `CommentThread`/`Comment`/`Mention`, business-access ceiling,
   `created_by_member_id`: no schema anywhere; spec/research text only.

## 7. Never-render list

The following must not appear in any of the five surfaces:

- availability, authority, or Work relation inferred from runtime health,
  provider session state, matching names, or document authorship;
- a provider `completed` status presented as semantic success, answer, or
  approval;
- "% utilization" or any capacity number not present in
  `provider_capacity.state`;
- fixture-only Agents, Teams, or Work rows presented as live;
- an Assignment Message kind (Team Work has no such contract);
- a second department identity derived from an `AgentTeamRun`.

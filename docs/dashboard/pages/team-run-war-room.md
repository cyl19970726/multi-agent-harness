# Agent Team War Room Page Spec

```text
status: implemented
owner_role: product-design
canonical_for: one standalone or Mission-scoped AgentTeamRun
route_or_surface: Agent Teams -> TeamRun
architecture: ADR 0025 retained runtime contracts + ADR 0034 lifecycle +
              ADR 0037 collaboration
```

## User Problem

The Host and Human need one surface to understand and steer a living Agent
Team: who owns which assignment, what is active or blocked, what questions need
answers, which native sessions can be resumed, and what evidence has arrived.

The page must remain useful when the same TeamRun spans several Host-plan Waves.

## Canonical Semantics

Required data:

- independent `AgentTeam` definition, explicit Team Lead, and editable member
  identities;
- `AgentTeamRun`, optional `mission_id`, optional legacy `wave_id`, status,
  previous run, host/runtime facts, and outcome;
- `MemberRun` identity, role, provider/model, status, capability profile,
  worktree, and native-session binding;
- assignment/message correlation, delivery, ACK, optional `origin_wave_id`,
  pending interactions, controls, artifacts, and checks;
- provider-native activity read on demand, clearly labeled by source and
  availability.

Harness does not mirror provider transcripts, tool calls, commands, file
events, turns, or thinking. A provider `completed` lifecycle update is not an
answer, approval, or semantic result.

The TeamRun may be standalone or linked to a Mission. In the primary
Mission-scoped path it is not owned by one Wave. Wave context explains how the
Host is currently using the team.

The Host Agent that created and coordinates the team is the Team Lead. The page
must show that identity separately from the member roster. `host` means the
current Host Agent. Lead messages, assignments, composition changes, and
acceptance decisions are control-plane actions; they do not create an implicit
Lead `MemberRun`. If the Lead also executes a lane, that requires an explicit
member with its own native-session binding.

## Desktop Layout

Use the shared Workbench shell with the compact execution rail, participant
mailbox projections, one chronological group conversation, a persistent
composer, and flexible context modules.

```text
+----------------------+--------------------------------------+------------------+
| Compact exec rail    | Team header                          | Mission context  |
|                      | definition · Lead · run · actions    | Current Wave     |
|                      +--------------------------------------+ Selected member  |
|                      | Host + Member Inbox/Outbox           | Runtime          |
|                      | delivery and attention projections  | Artifacts        |
|                      +--------------------------------------+                  |
|                      | filtered Team group conversation     |                  |
|                      | Markdown/messages/actions/evidence   |                  |
|                      | sticky Team or @member composer      |                  |
+----------------------+--------------------------------------+------------------+
```

Every Host/Member mailbox is computed from TeamMessage recipients and delivery
records. It is a read-model projection, not a new stored mailbox object. The
Host mailbox is visible even though Host is not a fabricated MemberRun. Mailbox
selection filters sent/received conversation; Member portraits and names open
Member Focus. A blocking details drawer is not a replacement for the full page.

Activity is one source-aware timeline:

- Harness assignments, messages, pending interactions, controls, and outcomes;
- ephemeral provider-native tool/command/chat/turn activity when available;
- semantic Markdown handoffs, artifacts, and checks;
- explicit “native session unavailable” states instead of invented history.

The page is a joined read model, not a transcript database. Native activity is
read on demand and remains rebuildable.

Tool icons are meaningful and consistent; provider and member avatars never
replace status or source labels.

Participant, message-kind, and text-search filters combine locally without
mutating coordination truth. The default projection prioritizes assignments,
plan negotiation, questions, answers and handoffs; the complete durable record
remains one click away. Large message bodies use the safe shared Markdown
renderer rather than displaying raw Markdown syntax.

## Context Modules

1. **MissionCompact** — optional Mission relation and open-Mission action.
2. **CurrentHostPlan** — selected/latest Wave context excerpt for orientation;
   never claims runtime ownership.
3. **SelectedMember** — identity, assignment, capability, message, steer,
   interrupt, resume, and open-member actions supported by the real adapter.
4. **Runtime** — worktree, native session id, provider mode/version,
   permission/budget, and honest availability.
5. **Artifacts** — explicit files/checks/evidence with open/download actions.

The Host mailbox and conversation pressure rows together form the **Lead
Inbox** projection for member-authored
`question`, `blocker`, and `review_request` messages addressed to `host`.
Every item shows sender, Assignment correlation, delivery/ACK state, and the
responsible next action. Answering reuses the source correlation, records the
source message as causation, and acknowledges the source delivery.

Conversation rows also expose `causation_id` as “reply to” lineage. A later
Member Handoff therefore remains in the Assignment work chain while visibly
pointing to the exact Host or peer follow-up that caused that provider round.

## Actions

- Message the whole team or one explicit member. The composer distinguishes a
  new work chain from a reply to the selected Assignment correlation.
- Make it clear that Host-authored coordination comes from the Team Lead;
  Human/operator authorship remains separately attributable where supported.
- Create a correlated assignment with optional origin Wave metadata.
- Add, rename, deactivate, steer, interrupt, explicitly close, or resume a
  member where the selected provider mode honestly supports it. Interrupt ends
  one turn; Close ends the runtime.
- Inspect delivery/ACK/correlation lineage and answer PendingInteractions.
- Answer Lead Inbox items with inherited correlation and causation. The
  Dashboard may author Host/operator messages; it never impersonates a member.
- Open Mission, current Wave context, Member Focus, artifact, or native-session
  summary.
- Complete or stop the TeamRun only through a real acknowledged lifecycle
  transition.

Wave creation/advance occurs from Mission Canvas. It never implicitly stops or
restarts this TeamRun.

## States And Responsive Behavior

- No members: explain whether the stable team definition is empty or run
  materialization failed.
- Starting: show admission/runtime acquisition without calling it working.
- Blocked/question: attach pressure and action to the exact record.
- Provider/session unavailable: retain coordination and show the missing
  source.
- Completed/stopped: coordination history is read-only, but any still-live
  member runtime retains an explicit Host Close action. Resume/new-run choices
  follow the provider/session contract; do not imply a Mission or Wave
  completed.
- Tablet/mobile: collapse navigation, make the mailbox strip horizontally
  scrollable and keyboard accessible,
  preserve one stream and composer, and move context into sheet/bottom sheet.
- Navigation preserves filters, selected member, scroll, Mission id, selected
  Wave id, TeamRun id, and project id across Team → Member → Team deep links.
- A canonical MCP Dashboard URL for a Mission-scoped run includes the current
  Host-plan Wave as cold-link navigation context. This does not attach runtime
  ownership to that Wave and may change when the Host advances its plan.

## Screenshot And UX Acceptance

Desktop acceptance must show the shared compact execution shell, team identity,
Host/Member mailbox projections with portraits, a source-aware Markdown group
conversation, composer, Mission/Wave
orientation, runtime, and artifacts. Verify:

- member controls open the correct Member Focus and return without state loss;
- mailbox, participant, message-kind, and search filters preserve Team context;
- PendingInteraction answer, chat, steer, interrupt, Close, and resume states
  match real adapter acknowledgements;
- Markdown handoffs and tool activity render with suitable icons and density;
- the same TeamRun remains visible after Mission Wave advance;
- empty, loading, error, unavailable-native-session, and long-stream behavior;
- actual screenshot against the approved expected reference.

## Explicit Boundaries

- A TeamRun is not a Standing Agent or OrgUnit.
- Assignment correlation owns work; Wave prose explains Host intent.
- Provider-native subagents are observations unless a real orchestrated
  lifecycle exists.
- A member-to-member message is allowed inside the same TeamRun and remains
  visible to the Lead. It is queued for the peer's next eligible round rather
  than interrupting the current turn.
- TeamRun completion does not advance a Wave; Wave advance does not complete a
  TeamRun.

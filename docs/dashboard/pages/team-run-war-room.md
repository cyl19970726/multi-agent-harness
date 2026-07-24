# Agent Team War Room Page Spec

```text
status: implemented
owner_role: product-design
canonical_for: one standalone or Mission-scoped AgentTeamRun
route_or_surface: Agent Teams -> TeamRun
architecture: ADR 0025 retained runtime contracts + ADR 0034 lifecycle
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

Use the shared Workbench shell with compact member controls, one chronological
activity stream, a persistent composer, and flexible context modules.

```text
+----------------------+--------------------------------------+------------------+
| Product sidebar      | Team header                          | Mission context  |
|                      | definition · Lead · run · actions    | Current Wave     |
| Active context tree  +--------------------------------------+ Selected member  |
|                      | compact member controls              | Runtime          |
|                      | role/model/status/action/pressure    | Artifacts        |
|                      +--------------------------------------+                  |
|                      | unified Team activity stream         |                  |
|                      | messages/actions/decisions/evidence  |                  |
|                      | sticky Team or @member composer      |                  |
+----------------------+--------------------------------------+------------------+
```

Member controls use project-default portraits when no explicit avatar exists.
They navigate to Member Focus; a blocking details drawer is not a replacement
for the full page.

Activity is one source-aware timeline:

- Harness assignments, messages, pending interactions, controls, and outcomes;
- ephemeral provider-native tool/command/chat/turn activity when available;
- semantic Markdown handoffs, artifacts, and checks;
- explicit “native session unavailable” states instead of invented history.

The page is a joined read model, not a transcript database. Native activity is
read on demand and remains rebuildable.

Tool icons are meaningful and consistent; provider and member avatars never
replace status or source labels.

## Context Modules

1. **MissionCompact** — optional Mission relation and open-Mission action.
2. **CurrentHostPlan** — selected/latest Wave context excerpt for orientation;
   never claims runtime ownership.
3. **SelectedMember** — identity, assignment, capability, message, steer,
   interrupt, resume, and open-member actions supported by the real adapter.
4. **Runtime** — worktree, native session id, provider mode/version,
   permission/budget, and honest availability.
5. **Artifacts** — explicit files/checks/evidence with open/download actions.

## Actions

- Message the whole team or one explicit member.
- Make it clear that Host-authored coordination comes from the Team Lead;
  Human/operator authorship remains separately attributable where supported.
- Create a correlated assignment with optional origin Wave metadata.
- Add, rename, deactivate, steer, interrupt, or resume a member where the
  selected provider mode honestly supports it.
- Inspect delivery/ACK/correlation lineage and answer PendingInteractions.
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
- Completed/stopped: read-only history plus explicit resume/new-run choices;
  do not imply a Mission or Wave completed.
- Tablet/mobile: collapse sidebar, make member strip keyboard accessible,
  preserve one stream and composer, and move context into sheet/bottom sheet.
- Navigation preserves filters, selected member, scroll, Mission id, TeamRun id,
  and project id.

## Screenshot And UX Acceptance

Desktop acceptance must show the shared shell, team identity, compact member
controls with portraits, a source-aware activity stream, composer, Mission/Wave
orientation, runtime, and artifacts. Verify:

- member controls open the correct Member Focus and return without state loss;
- PendingInteraction answer, chat, steer, interrupt, and resume states match
  real adapter acknowledgements;
- Markdown handoffs and tool activity render with suitable icons and density;
- the same TeamRun remains visible after Mission Wave advance;
- empty, loading, error, unavailable-native-session, and long-stream behavior;
- actual screenshot against the approved expected reference.

## Explicit Boundaries

- A TeamRun is not a Standing Agent or OrgUnit.
- Assignment correlation owns work; Wave prose explains Host intent.
- Provider-native subagents are observations unless a real orchestrated
  lifecycle exists.
- TeamRun completion does not advance a Wave; Wave advance does not complete a
  TeamRun.

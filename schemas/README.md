# Schemas

## Legacy Mission coordination model

| Object | Schema |
| --- | --- |
| Mission | [mission.schema.json](mission.schema.json) |
| Mission log entry | Historical append-only store record (`MissionLogEntry`); no standalone JSON Schema is currently required by schema generation or fixture validation |

Mission and Mission Log are retired current authority (DOC-108): durable
AgentTeam, Team-run Work, and identity-first Message delivery replaced them.
The schema and fixtures remain validated so historical rows stay readable and
exportable; no writer path exists on any surface.

## Current schema registry

| Object | Schema |
| --- | --- |
| Agent team | [agent-team.schema.json](agent-team.schema.json) |
| Durable Agent member identity | [agent-member.schema.json](agent-member.schema.json) |
| Agent team run | [agent-team-run.schema.json](agent-team-run.schema.json) |
| Member run | [member-run.schema.json](member-run.schema.json) |
| Member run event view | [member-run-event.schema.json](member-run-event.schema.json) |
| Provider-native session locator | [native-session-ref.schema.json](native-session-ref.schema.json) |
| Canonical mutation event | [canonical-mutation-event.schema.json](canonical-mutation-event.schema.json) |
| Crash-atomic canonical operation | [canonical-operation.schema.json](canonical-operation.schema.json) |
| Team message | [team-message.schema.json](team-message.schema.json) |
| Member execution trust error | [trust-error.schema.json](trust-error.schema.json) |
| Message delivery | [message-delivery.schema.json](message-delivery.schema.json) |
| Member workspace binding | [member-workspace-binding.schema.json](member-workspace-binding.schema.json) |
| Agent Team Work | [work.schema.json](work.schema.json) |
| Agent Team Work event | [work-event.schema.json](work-event.schema.json) |
| Agent Team Work delivery | [work-delivery.schema.json](work-delivery.schema.json) |
| Agent Team Work condition record | [work-condition-record.schema.json](work-condition-record.schema.json) |
| Agent Team Work report | [work-report.schema.json](work-report.schema.json) |
| Work finding | [work-finding.schema.json](work-finding.schema.json) |
| Failure analysis | [failure-analysis.schema.json](failure-analysis.schema.json) |
| Work module definition | [work-module-definition.schema.json](work-module-definition.schema.json) |
| Work module binding | [work-module-binding.schema.json](work-module-binding.schema.json) |
| Gate requirement | [gate-requirement.schema.json](gate-requirement.schema.json) |
| Gate evaluation | [gate-evaluation.schema.json](gate-evaluation.schema.json) |
| Gate waiver | [gate-waiver.schema.json](gate-waiver.schema.json) |
| Agent Team Work evidence | [work-evidence.schema.json](work-evidence.schema.json) |
| Agent Team Work operational decision | [work-operational-decision.schema.json](work-operational-decision.schema.json) |
| Team Supervisor lease | [team-supervisor-lease.schema.json](team-supervisor-lease.schema.json) |
| Member action | [member-action.schema.json](member-action.schema.json) |
| Delegation run | [delegation-run.schema.json](delegation-run.schema.json) |
| Team run event | [team-run-event.schema.json](team-run-event.schema.json) |
| Provider child thread | [provider-child-thread.schema.json](provider-child-thread.schema.json) |
| Proposal | [proposal.schema.json](proposal.schema.json) |
| Evidence | [evidence.schema.json](evidence.schema.json) |
| Decision | [decision.schema.json](decision.schema.json) |
| Tool descriptor | [agent-harness-tool-descriptor.schema.json](agent-harness-tool-descriptor.schema.json) |
| Doc descriptor | [doc-descriptor.schema.json](doc-descriptor.schema.json) |
| Local AgentFirm RoleViews | [role-views/agentfirm.role_views.v1](role-views/agentfirm.role_views.v1) |
| Role action manifest | [role-views/role-action-manifest.v1.json](role-views/role-action-manifest.v1.json) |

## Legacy historical compatibility

| Object | Schema | Authority |
| --- | --- | --- |
| Mission | [mission.schema.json](mission.schema.json) | DOC-108 and earlier historical rows only; read/export compatibility, never a new write |
| Wave | [wave.schema.json](wave.schema.json) | ADR 0051 and earlier historical rows only; read/export compatibility, never a new write, lifecycle transition, or gate |

The Mission and Wave schemas and fixtures remain validated so old rows can be
read and exported without data loss. Their presence does not make Mission or
Wave part of the current AgentFirm model. `Mission.wave_ids` is deprecated and
read-only; historical rows may carry it, and no Mission writer exists.

Schemas in this directory are generic. Project-specific artifacts should live
in an adapter package or example directory.

ADR 0032 is implemented: `NativeSessionRef` references provider-owned
history/resume state, while Harness stores coordination and explicit outcomes.
Do not add transcript, stdout, JSONL, tool, command, or file-event mirrors.

Fixtures under `fixtures/<schema-name>/valid` and
`fixtures/<schema-name>/invalid` are checked by `pnpm check:schema-fixtures`.

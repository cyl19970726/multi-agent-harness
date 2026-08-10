# Schemas

| Object | Schema |
| --- | --- |
| Mission | [mission.schema.json](mission.schema.json) |
| Wave | [wave.schema.json](wave.schema.json) |
| Agent team | [agent-team.schema.json](agent-team.schema.json) |
| Durable Agent member identity | [agent-member.schema.json](agent-member.schema.json) |
| Agent team run | [agent-team-run.schema.json](agent-team-run.schema.json) |
| Member run | [member-run.schema.json](member-run.schema.json) |
| Member run event view | [member-run-event.schema.json](member-run-event.schema.json) |
| Provider-native session locator | [native-session-ref.schema.json](native-session-ref.schema.json) |
| Canonical mutation event | [canonical-mutation-event.schema.json](canonical-mutation-event.schema.json) |
| Crash-atomic canonical operation | [canonical-operation.schema.json](canonical-operation.schema.json) |
| Team message | [team-message.schema.json](team-message.schema.json) |
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
| Agent Team Work gate evaluation | [work-gate-evaluation.schema.json](work-gate-evaluation.schema.json) |
| Agent Team Work operational decision | [work-operational-decision.schema.json](work-operational-decision.schema.json) |
| Team Supervisor lease | [team-supervisor-lease.schema.json](team-supervisor-lease.schema.json) |
| Durable Member Close | [team-member-close-request.schema.json](team-member-close-request.schema.json) |
| Member action | [member-action.schema.json](member-action.schema.json) |
| Pending provider interaction | [pending-interaction.schema.json](pending-interaction.schema.json) |
| Delegation run | [delegation-run.schema.json](delegation-run.schema.json) |
| Team run event | [team-run-event.schema.json](team-run-event.schema.json) |
| Message | [message.schema.json](message.schema.json) |
| Provider child thread | [provider-child-thread.schema.json](provider-child-thread.schema.json) |
| Proposal | [proposal.schema.json](proposal.schema.json) |
| Evidence | [evidence.schema.json](evidence.schema.json) |
| Decision | [decision.schema.json](decision.schema.json) |
| Tool descriptor | [agent-harness-tool-descriptor.schema.json](agent-harness-tool-descriptor.schema.json) |
| Doc descriptor | [doc-descriptor.schema.json](doc-descriptor.schema.json) |

Schemas in this directory are generic. Project-specific artifacts should live
in an adapter package or example directory.

ADR 0032 is implemented: `NativeSessionRef` references provider-owned
history/resume state, while Harness stores coordination and explicit outcomes.
Do not add transcript, stdout, JSONL, tool, command, or file-event mirrors.

Fixtures under `fixtures/<schema-name>/valid` and
`fixtures/<schema-name>/invalid` are checked by `pnpm check:schema-fixtures`.

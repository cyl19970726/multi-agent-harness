# Schemas

| Object | Schema |
| --- | --- |
| Mission | [mission.schema.json](mission.schema.json) |
| Wave | [wave.schema.json](wave.schema.json) |
| Agent team | [agent-team.schema.json](agent-team.schema.json) |
| Agent member | [agent-member.schema.json](agent-member.schema.json) |
| Agent team run | [agent-team-run.schema.json](agent-team-run.schema.json) |
| Member run | [member-run.schema.json](member-run.schema.json) |
| Provider-native session locator | [native-session-ref.schema.json](native-session-ref.schema.json) |
| Team message | [team-message.schema.json](team-message.schema.json) |
| Member action | [member-action.schema.json](member-action.schema.json) |
| Pending provider interaction | [pending-interaction.schema.json](pending-interaction.schema.json) |
| Delegation run | [delegation-run.schema.json](delegation-run.schema.json) |
| Team run event | [team-run-event.schema.json](team-run-event.schema.json) |
| Message | [message.schema.json](message.schema.json) |
| Agent runtime | [agent-runtime.schema.json](agent-runtime.schema.json) |
| Agent event | [agent-event.schema.json](agent-event.schema.json) |
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

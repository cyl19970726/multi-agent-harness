# Host Agent CLI Control

```text
status: stable
owner: provider-integration
last reviewed: 2026-08-26
```

Managed Hosts are ordinary `AgentMember`s supervised by the machine-scoped
NodeDaemon. Their Work, Message, acceptance, interrupt, Close, and Reopen
actions use the exact `firm` binary and the Supervisor-issued `FIRM_*`
collaboration envelope. The CLI authenticates the live MemberRun and submits a
request to the current Supervisor; it never writes Store files directly.

The Harness coordination MCP server is retired. There is no `firm mcp`
subcommand, plugin MCP registration, MCP mutation/read fallback, or migration
path. MCP attachment in a provider launch profile is a separate provider
capability for unrelated reviewed tools and does not grant Harness authority.

`external_interactive` Hosts use the same CLI against their explicitly bound
identity, but remain pull-only: Harness does not create or claim a provider
session, receipt, or timely wake for them.

## Runtime envelope

The Supervisor injects the canonical `FIRM_BIN`, `FIRM_TEAM_RUN_ID`,
`FIRM_MEMBER_RUN_ID`, and, for assigned execution, `FIRM_WORK_ID` plus
`FIRM_WORK_VERSION`. Skills and provider prompts must use those exact values.
Permission and cwd are frozen when the AgentSession starts.

## Boundary

```text
coding agent
  -> exact firm CLI + FIRM_* envelope
  -> authenticated live Supervisor capability
  -> application service
  -> canonical Store
```

Provider-native transcripts remain in provider-native storage. CLI commands
are coordination requests, not evidence of provider completion or Host
acceptance.

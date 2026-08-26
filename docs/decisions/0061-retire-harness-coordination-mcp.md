# ADR 0061: Retire Harness Coordination MCP

```text
status: accepted; implemented by DEV-100
date: 2026-08-26
amends: ADR 0056, ADR 0057, ADR 0059
canonical_for: Agent-facing Harness coordination transport
```

## Context

The repository exposed the same Agent Team reads and mutations through a
large stdio MCP server and the authenticated `firm` CLI. Managed agents already
need an exact Supervisor-issued runtime capability, while maintaining two
command languages, plugin registrations, tests, and permission explanations
created drift without adding a second valid authority.

The MCP server owns no durable data. There is therefore nothing to migrate.
Provider launch profiles may still attach unrelated reviewed MCP servers; that
is a provider capability, not Harness coordination.

## Decision

The `firm` CLI is the sole Agent-facing Harness coordination interface. Work,
Message, acceptance, interrupt, Close, and Reopen requests use the exact
Supervisor-injected `FIRM_BIN` and identity envelope. The CLI invokes the same
application services and cannot write Store files directly.

Delete the Harness MCP subcommand, stdio server, tools, tests, plugin
registrations, environment authority, and current documentation. Do not retain
a compatibility server, alias, fallback, dual write, or transcript copy.
A structural gate prevents these surfaces from returning.

## Consequences

Skills and provider prompts have one executable command contract. Authentication,
CAS, idempotency, NodeDaemon/AgentSession generations, and Host/Member authority
remain unchanged. HTTP and Role Actions remain operator/product transports;
provider-native MCP attachment remains available but cannot carry Harness
coordination authority.

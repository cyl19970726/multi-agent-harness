# 0012: Dashboard Is Control Plane

## Decision

The Agent Dashboard is an operational control surface over harness state.

## Consequences

It must show legacy dependency graph, team state, message delivery, runtime health,
evidence, proposal, review, decision, and evaluation visibility. It should
link to project dashboards instead of replacing them.

Dashboard actions must update canonical harness objects through CLI/API/store
contracts.

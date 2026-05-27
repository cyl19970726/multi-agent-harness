# 0009: Task Graph As Derived View

## Decision

The task graph is a view over task nodes and their edges, not a separate source
of truth.

## Consequences

Edges include parent/child decomposition, dependencies, review, assignment
delivery, handoff, and follow-up creation. Dashboard graph views should be
read models over tasks and messages.

# 0002: Message-First Task System

## Decision

Task assignment and task reports flow through `Message`.

The stronger invariant is: assignment is message-delivered, not field-mutated.
`Task.assignee_agent_id` and `AgentMember.current_task_id` are projections of
message delivery and runtime state. They are not proof that an agent received
work.

## Consequences

CLI/API, Dashboard, and review gates should treat `Message(kind=task)` delivery
as the assignment event. Direct field mutation alone cannot prove task
assignment.

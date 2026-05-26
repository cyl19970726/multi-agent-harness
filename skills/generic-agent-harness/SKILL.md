---
name: generic-agent-harness
description: "Use when operating or extending a generic multi-agent harness: agent members, message-first task/report flow, claims, blockers, permissions, provider sessions, tool descriptors, and Agent Dashboard evidence."
---

# Generic Agent Harness

Use this skill when the work is about the multi-agent product itself, not a
domain project that the agents use as a tool.

## First Step

Read only what the task needs:

- Product boundary: `docs/product-boundary.zh.md`
- Lifecycle: `docs/multi-agent-lifecycle.zh.md`
- Architecture: `docs/architecture.md`
- Package plan: `docs/package-architecture.md`
- Agent Dashboard: `docs/agent-dashboard-design.zh.md`
- Schemas: `schemas/README.md`

## Rules

- Treat project systems as tools behind adapters.
- Do not put domain logic in the generic core.
- Use `AgentMessage` for task assignment, reports, follow-up questions, and
  handoff.
- Materialize messages into `Task`, `Report`, `Claim`, `Blocker`, or
  `Decision` before using them for gates.
- Keep provider chat below message/report artifacts in the trust order.
- Require explicit permission grants for live, money-moving, destructive, or
  secret-touching actions.

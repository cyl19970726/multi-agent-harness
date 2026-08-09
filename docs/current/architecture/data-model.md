# Data Model

This document owns the current object relationships. JSON Schemas own field
shape and validation.

## Canonical graph

```text
Mission 1 ── 1 AgentTeam ── * AgentTeamRun ── * MemberRun
                    │                 │
                    │                 └── * Work / Message / Evidence
                    └── 1 ExecutionNode

WorkDelegation: source Team/Work ──> target Team/Work
```

`Mission` carries durable intent. `AgentTeam` is the atomic agency unit: one
Mission, one Host Agent, one immutable Node placement, and a flat Member set.
Teams never nest. `AgentTeamRun` always names its Team, execution Node, and
project binding; Mission is derived through Team and is not duplicated on the
Run.

Cross-Team cooperation is explicit `WorkDelegation`, not parent/child topology.
The source remains accountable until the delegation reaches a terminal result.
Delegation events are append-only, versioned, idempotent, and reject cycles.
Creation idempotency fingerprints the complete delegation and target-Work
intent; omitted entity ids are derived from the idempotency key. Target Work
block/resume/terminal transitions embed their delegation roll-up in the same
`WorkOperation`, so every public mutation surface observes one atomic result.

## Runtime trust

One NodeDaemon generation owns each registered Execution Space on the local
Node. Each `TeamSupervisorLease` is a child of that exact NodeDaemon lease.
Mailbox delivery, provider resume, recovery, and lifecycle writes must prove
current parent and child generations.
`NodeProjectRegistration` writes must match the explicitly selected Execution
Space; a registration from one Store cannot name another Space.

## Sources of truth

| Question | Canonical record |
| --- | --- |
| Why does the Team exist? | `Mission` |
| Which agency owns it? | `AgentTeam.mission_id` |
| Who leads it? | `AgentTeam.host_agent_id` |
| Where may it execute? | `AgentTeam.node_id` |
| Which runtime attempt is active? | `AgentTeamRun` |
| Who executes a lane? | `MemberRun` plus current `Work` ownership |
| How does work cross Teams? | `WorkDelegation` and events |
| Who may drive a Run? | current parent-fenced `TeamSupervisorLease` |
| What proves execution? | Work result, checks, artifacts, provider-native refs |

The provider-native store owns transcript truth. Harness persists only the
native session locator and coordination evidence it needs; any activity shown
in the Dashboard is an ephemeral read projection from that provider-owned
source, never a second transcript ledger.

Wave rows are historical read-only compatibility data. They are not part of
new Team, Run, Work, scheduling, or acceptance contracts.

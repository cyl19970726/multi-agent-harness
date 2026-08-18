# Data Model

This document owns the current object relationships. JSON Schemas own field
shape and validation.

## Canonical graph

```text
AgentTeam 1 ── * Work / Evidence                # durable responsibility
     ├── * TeamMembership ──> AgentMember       # participation, not identity
     ├── 1 ExecutionNode
     └── * AgentTeamRun ── * MemberRun          # internal diagnostics/history
                        └── WorkExecutionBinding

AgentMember ── * AgentSession
      └── authors Message ── * CanonicalMessageDelivery ──> AgentSession

NodeDaemon ── * RuntimeCommand ──> provider effect

WorkDelegation: source Team/Work ──> target Team/Work
```

`AgentMember` is the sole durable agent identity root; `TeamMembership` records
only participation. The `AgentIdentity` name is a deprecated same-ID read-only
compatibility projection of `AgentMember` and is never a second identity root.

`AgentTeam` is the atomic agency unit: one Host membership, one immutable
Node placement, and a flat Member set. Teams never nest. Pre-cutover Teams
may carry read-only `legacy_mission_id` provenance (DOC-108); no Mission owns
or gates a Team. `AgentTeamRun` always names its Team, execution Node, and
project binding.

`Work` hangs off the durable `AgentTeam` through `accountable_team_id`.
`AgentTeamRun` and `MemberRun` are internal diagnostics and history
projections: `Work.team_run_id` only correlates the run that surfaced a Work,
and ending or discarding a run never moves or re-scopes responsibility.

Cross-Team cooperation is explicit `WorkDelegation`, not parent/child topology.
The collaboration fabric owns the relationship and decisions; source
and target Execution Spaces independently own their native Work. Cross-node
mutations route through the accepted Remote Node Fabric and fold relationship
truth only after an exact terminal application receipt. Target Work completion
never completes source Work. Local WorkDelegation writers and transport-time
fallbacks are retired. See
[Cross-machine Team collaboration](cross-machine-team-collaboration.md).

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
| Why does the Team exist? | its durable `AgentTeam` record and Work context |
| Which agency owns it? | the durable `AgentTeam` itself |
| Who is the durable agent? | `AgentMember` |
| Who leads it? | the Host-role `TeamMembership` |
| Where may it execute? | `AgentTeam.node_id` |
| Which runtime attempt is active? | `AgentTeamRun` |
| Who executes a lane? | `MemberRun` plus current `Work` ownership |
| How does work cross Teams? | `WorkDelegation` and events |
| Who may drive a Run? | current parent-fenced `TeamSupervisorLease` |
| Who authored conversation? | identity-first `Message`, attested by the source NodeDaemon generation |
| What proves one recipient's delivery state? | `CanonicalMessageDelivery` bound to that recipient identity and exact AgentSession generation |
| What authorizes a provider/process effect? | `RuntimeCommand`; never Message, CanonicalMessageDelivery, Work, TeamRun, or MemberRun |
| What proves execution? | Work result, checks, artifacts, provider-native refs |

The provider-native store owns transcript truth. Harness persists only the
native session locator and coordination evidence it needs; any activity shown
in the Dashboard is an ephemeral read projection from that provider-owned
source, never a second transcript ledger.

`Mission`, `MissionLogEntry`, `Wave`, `WaveStatus`, `WaveGateStatus`,
`Mission.wave_ids`, `missions.jsonl`, `mission_log.jsonl`, and `waves.jsonl`
are retired historical read/export compatibility data (DOC-108, and ADR 0051
before it). They are not part of new Team, Run, Work, scheduling, or
acceptance contracts, and no writer exists on any surface.

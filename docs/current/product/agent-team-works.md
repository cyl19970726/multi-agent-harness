# Agent Team Work

Status: current
Contract: AFM-2026.08.2

## Authority

Product doctrine for this topic — Work identity, ownership, the
submission/trust chain, and the Message boundary — is canonical in Notion;
see the single authority-boundary anchor in
`docs/current/documentation-governance.md` (Authority boundary: Notion vs
repository) for the current Notion location. This repository file survives
only as the implementation-bound remainder below.

## Implementation-bound invariants

```text
phase:      open -> active -> review -> closed
condition:  normal | blocked | on_hold
resolution: accepted | cancelled | failed   # closed only
```

`team_id` is a deprecated pre-cutover alias of `accountable_team_id`,
readable through the Rust serde alias and never written by current
binaries.

Mutation surface (all executable Work mutations):

```bash
firm team-run work list|show|create|assign|claim|start|block|resume|release
firm team-run work submit|review|request-changes|accept|cancel|retarget
firm team-run work reconcile-delivery|poll-github-ci
```

`firm work list` (DOC-106) replaces the retired `firm company work
list/query`; it reads native Work read-only and never falls back to the
former Company task ledger.

| Plane | Examples |
|---|---|
| Work | phase, condition, resolution, owner, report, gate, decision |
| runtime | MemberRun, provider session, process lifecycle |
| Work delivery | `WorkDelivery`: Work allocation/revision transport only |
| Message delivery | `CanonicalMessageDelivery`: per-recipient queued/routed/claimed/provider-received/acknowledged/failed/expired/invalidated state |
| runtime command | `RuntimeCommand`: fenced provider effects and live controls |
| identity | `AgentMember` identity and its `TeamMembership` participation |

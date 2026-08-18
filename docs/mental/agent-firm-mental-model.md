# Agent Firm Mental Model

```text
status: canonical — single source of truth
owner_role: product
canonical_for: product architecture, Agent Team operations
replaces: ADR 0027, ADR 0042 (scattered architecture descriptions)
supersedes: the Company-facet model (DOC-108 — Company/Organization/Docs are retired product layers)
```

This is the authoritative mental model. When any other document (ADR, skill, product spec, code comment) contradicts this document, this document wins. Update this document first, then cascade changes to downstream docs.

---

## Firm

A Firm is the top-level operating context: one operator's execution
foundation. It has two facets: Organization (who) and Execution (what's being
done). Company-memory/Knowledge management lives outside this repository's
current authority (DOC-108): the retired Company/Docs layers remain exportable
history, never a current facet.

---

## Organization

Organization answers "who exists and how are they organized."

**Agent Teams**: An independent unit of execution with a Host AgentMember and Members. The Organization contains flat AgentTeams — no nesting or parent/child Team authority. A Team is created without any Mission; pre-cutover Teams may carry read-only `legacy_mission_id` provenance. Every Team has immutable `node_id` placement on one machine. A Team's Members never cross machines. `labels` are optional filtering metadata; placement identity is not optional metadata.

**Agent Memberships**: Durable agent identities that persist across Team Runs. Not tied to any single execution. Examples: governance Agent auditing docs/works periodically, scheduled-task Agent running on a timer.

**Actors**: Four types — Human, Agent, External, Service. `ActorRef` (type + id) references a participant wherever needed.

---

## Execution

Execution answers "what work is being done right now."

**Missions (retired, DOC-108)**: Pre-cutover durable goals survive only as
read-only legacy provenance and export/verify history. No new Mission, Mission
Log, or Wave row may be written on any surface.

**Agent Team Runs**: One execution instance with MemberRuns (runtime bindings), shared Work board, message inbox. Existing execution model — Work state machine, Message delivery, Daemon supervision — unchanged.

**Work**: The responsibility kernel. Title, context, criteria, owner
(AgentMember), Team/TeamRun scope and three independent lifecycle axes:
`phase` (`open -> active -> review -> closed`), `condition`
(`normal | blocked | on_hold`), and closed-only `resolution`
(`accepted | cancelled | failed`). Optional `labels` (filtering). The Global
Work RoleView is a read-only aggregate over this same identity, not a second
task record.

Work creation answers WHAT and WHO; placement and verification are modular records.

- **WHERE** — `MemberWorkspaceBinding`: exact Execution Space, project, AgentMember, MemberRun, TeamRun, Work, absolute path, repository/base identity, generation and safety lifecycle.
- **HOW** — `WorkModuleBinding + GateRequirement + GateEvaluation/GateWaiver`: a frozen candidate-scoped requirement set. Result submission and Host acceptance use exact Work/report/Candidate fingerprints; stale state rejects with zero side effects under one Store writer lock.
- **WHO** — `owner_member_id`, `assignee`.

**Views**: All Execution visible on one page. Filters by Agent Team, status, date range. Tags on Work entries. Per-team views unchanged.

---

## Cross-Machine Communication

One logical Firm may place different AgentTeams on different ExecutionNodes. Each machine runs one machine-scoped NodeDaemon that supervises all local Teams across registered Execution Spaces. `NodeDaemonLease` is machine-scoped authority for all local Teams across registered Execution Spaces; it is never scoped to one Execution Space. A single Team never spans machines. Cross-Team responsibility uses explicit `WorkDelegation`; cross-machine transport must preserve the source and target Team identities instead of introducing nested Teams or optional placement.

---

## Host Agent Responsibilities

1. Receive work assignments, delegate to Members
2. Review submitted work, accept or request changes
3. Manage resources — worktrees are space resources, Host resolves conflicts
4. Create new work when supply is low
5. Recover from member failures (close + reopen, reassign work)

---

## Current Implementation State

| Component | Status | Notes |
|---|---|---|
| Agent Team execution | ✅ Live | Full lifecycle |
| Work — gates | ✅ Live | Open persistence/closed default registry, four built-ins, authority-bound Review, Store acceptance invariant |
| Work — workspace | ✅ Live | PR #406 — WorkWorkspace, ensure/cleanup, --worktree CLI |
| Organization — Agent Teams | ✅ Live | flat Teams; optional legacy Mission provenance; immutable node_id placement; labels |
| Organization — Agent Memberships | ✅ Live | durable AgentMember identity + TeamMembership generations (DEV-35) |
| Work — labels / tags | ❌ Not started | Filter + tag UI |
| Docs system | Retired (DOC-108) | Built-in Docs removed; historical data export/verify-only |
| Cross-machine communication | ✅ Live | peer-Team messaging over the canonical fabric (DEV-37) |
| Company selector | Retired (DOC-108) | Company Store registry removed |

---

## Document Governance

This document lives in `docs/mental/` — the canonical directory for mental model documents.

1. Update this document first when architecture changes
2. Cascade to skills (`collaborate-as-agent-team-member`)
3. Add "superseded by docs/mental/agent-firm-mental-model.md" to affected ADRs
4. Run `node scripts/check-cross-layer-consistency.mjs`
5. Run `bash scripts/manage-star-harness-install.sh --apply`

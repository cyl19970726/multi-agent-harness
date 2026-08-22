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
foundation. It has two facets: Teams and Members (who) and Execution (what's
being done). The retired Company Organization/Docs modules are not a facet:
company-memory/knowledge management lives outside this repository's current
authority (DOC-108) as exportable history only.

---

## Teams and Members

Teams and Members answer "who exists and how are they organized."

**Agent Teams**: An independent unit of execution with a Host AgentMember and Members. The Firm contains flat AgentTeams — no nesting or parent/child Team authority. A Team is created without any Mission; pre-cutover Teams may carry read-only `legacy_mission_id` provenance. Every Team has immutable `node_id` placement on one machine. A Team's Members never cross machines. `labels` are optional filtering metadata; placement identity is not optional metadata.

**Agent Members**: `AgentMember` is the sole durable agent identity. It persists across Team Runs and is not tied to any single execution. Examples: governance Agent auditing works periodically, scheduled-task Agent running on a timer.

**Team Memberships**: `TeamMembership` records only a member's participation in one Team, in generations. It never carries identity, and it is never a second identity root; the `AgentIdentity` name is a deprecated same-ID read-only compatibility projection of `AgentMember`.

**Actors**: Four types — Human, Agent, External, Service. `ActorRef` (type + id) references a participant wherever needed.

---

## Execution

Execution answers "what work is being done right now."

**Missions (retired, DOC-108)**: Pre-cutover durable goals survive only as
read-only legacy provenance and export/verify history. No new row of any of
these retired kinds — Mission, Mission Log, or Wave — may be written on any
surface.

**Agent Team Runs**: One execution instance with MemberRuns (runtime bindings),
shared Work board, and message inbox. A Run correlates execution history; it
does not contain or own durable Work responsibility.

**Work**: The responsibility kernel. Title, context, criteria, owner
(AgentMember), Team/TeamRun scope and three independent lifecycle axes:
`phase` (`open -> active -> review -> closed`), `condition`
(`normal | blocked | on_hold`), and closed-only `resolution`
(`accepted | cancelled | failed`). Optional `labels` (filtering). The Global
Work RoleView is a read-only aggregate over this same identity, not a second
task record.

**Work Graph**: Works are flat peer nodes. A hard directed edge means the
successor cannot be claimed or started until the prerequisite is accepted. A
Work may have several prerequisites (fan-in) and several derived successors
(fan-out), and the kernel rejects every direct or transitive cycle. Work never
contains another Work. Failed or cancelled prerequisites create Host replan
attention rather than silently resolving downstream Work. Messages may discuss
or propose edges, but only a versioned Work operation changes the graph.

```text
             ┌─> Work B ─┐
Work A ──────┤            ├─> Work D
             └─> Work C ─┘
```

Work creation answers WHAT and WHO; placement and verification are modular records.

- **WHERE** — `MemberWorkspaceBinding`: exact Execution Space, project, AgentMember, MemberRun, TeamRun, Work, absolute path, repository/base identity, generation and safety lifecycle.
- **HOW** — `WorkModuleBinding + GateRequirement + GateEvaluation/GateWaiver`:
  a frozen candidate-scoped requirement set. The enforced definition scope is
  closed: only built-in `integration-plan@1` is supported. Schemas do not imply
  an installable or dynamic Module registry. Result submission and Host
  acceptance use exact Work/report/Candidate fingerprints; stale state rejects
  with zero side effects under one Store writer lock.
- **WHO** — `owner_member_id`, `assignee`.

**Views**: All Execution visible on one page. Filters by Agent Team, status, date range. Tags on Work entries. Per-team views unchanged.

**Kernel boundary**: `firm-core` decides legal lifecycle and dependency
operations plus readiness. `firm-application` depends only on core, defines the
`WorkPersistence` port, and implements generic `WorkApplication<P>` use cases.
`firm-store` depends on application + core and implements that port with atomic
persistence and rebuildable projections. CLI composes both for HTTP, MCP, and
Role Actions. RoleViews explain graph/readiness and Dashboard renders them.
NodeDaemon, providers, transports, Dashboard, and Modules never own or recreate
Work policy.

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
| Work — dependency DAG | ✅ Live at DEV-60 cutover | Flat hard dependencies, cycle rejection, server-derived readiness and graph views |
| Work — gates | ✅ Live | Closed `integration-plan@1` built-in, frozen binding, authority-bound Review, Store acceptance invariant; no open Module registry |
| Work — workspace | ✅ Live | PR #406 — WorkWorkspace, ensure/cleanup, --worktree CLI |
| Teams and Members — Agent Teams | ✅ Live | flat Teams; optional legacy Mission provenance; immutable node_id placement; labels |
| Teams and Members — Agent Members | ✅ Live | durable AgentMember identity + TeamMembership participation generations (DEV-35) |
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

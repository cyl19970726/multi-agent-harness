# Agent Firm Mental Model

```text
status: canonical — single source of truth
owner_role: product
canonical_for: product architecture, Agent Team operations, Company OS
replaces: ADR 0027, ADR 0042 (scattered architecture descriptions)
```

This is the authoritative mental model. When any other document (ADR, skill, product spec, code comment) contradicts this document, this document wins. Update this document first, then cascade changes to downstream docs.

---

## Company

Company is the top-level entity. Everything belongs to a Company.

A user runs one Company at a time. Multi-company is a future concern; when it arrives, a Company selector appears in the dashboard. Until then, there is exactly one Company and it is implicit — no selection required.

Company has three facets: Organization (who), Execution (what's being done), Knowledge (what's known). These are three views into the same Company, connected through optional references on Work records.

---

## Organization

Organization answers "who exists and how are they organized."

**Agent Teams**: An independent unit of execution with a Host Agent and Members. The Organization contains flat AgentTeams — no nesting or parent/child Team authority. Every Team belongs to exactly one Mission and has immutable `node_id` placement on one machine. No two AgentTeams may reference the same Mission. A Team's Members never cross machines. `labels` are optional filtering metadata; placement identity is not optional metadata.

**Agent Memberships**: Durable agent identities that persist across Team Runs. Not tied to any single execution. Examples: governance Agent auditing docs/works periodically, scheduled-task Agent running on a timer.

**Actors**: Four types — Human, Agent, External, Service. `ActorRef` (type + id) references a participant wherever needed.

---

## Execution

Execution answers "what work is being done right now."

**Missions**: Durable goals owning exactly one flat AgentTeam. A Mission persists across multiple TeamRuns and Host-plan Waves without changing Team identity.

**Agent Team Runs**: One execution instance with MemberRuns (runtime bindings), shared Work board, message inbox. Existing execution model — Work state machine, Message delivery, Daemon supervision — unchanged.

**Work**: The responsibility kernel. Title, context, criteria, owner (AgentMember), team scope, status (open→in_progress→review→done). Optional `document_refs` (links to Docs) and `labels` (filtering).

Work creation answers WHAT and WHO; placement and verification are modular records.

- **WHERE** — `MemberWorkspaceBinding`: exact Execution Space, project, AgentMember, MemberRun, TeamRun, Work, absolute path, repository/base identity, generation and safety lifecycle.
- **HOW** — `WorkModuleBinding + GateRequirement + GateEvaluation/GateWaiver`: a frozen candidate-scoped requirement set. Result submission and Host acceptance use exact Work/report/Candidate fingerprints; stale state rejects with zero side effects under one Store writer lock.
- **WHO** — `owner_member_id`, `assignee`.

**Views**: All Execution visible on one page. Filters by Agent Team, status, date range. Tags on Work entries. Per-team views unchanged.

---

## Knowledge (Docs)

Knowledge answers "what does the Company know."

Documents are Company memory — structured content, typed records, views. Agent-operated (create, edit, govern), Human-reviewed (inspect, approve). Documents link to Work via `document_refs`, connecting "what we know" with "what we're doing."

---

## Cross-Machine Communication

One logical Firm may place different AgentTeams on different ExecutionNodes. Each machine runs one machine-scoped NodeDaemon that supervises all local Teams across registered Execution Spaces. `NodeDaemonLease` is machine-scoped authority for all local Teams across registered Execution Spaces; it is never scoped to one Execution Space. A single Team never spans machines. Cross-Team responsibility uses explicit `WorkDelegation`; future cross-machine transport must preserve the source and target Team identities instead of introducing nested Teams or optional placement.

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
| Organization — Agent Teams | ✅ Live | flat Mission-Team 1:1 + immutable node_id placement + labels |
| Organization — Agent Memberships | ❌ Not started | Design only |
| Work → Docs links | ❌ Not started | Optional document_refs |
| Work → labels / tags | ❌ Not started | Filter + tag UI |
| Docs system | ⚠️ Partial | PR #386 |
| Cross-machine communication | ❌ Not started | Design task |
| Company selector | ❌ Future | Not needed yet |

---

## Document Governance

This document lives in `docs/mental/` — the canonical directory for mental model documents.

1. Update this document first when architecture changes
2. Cascade to skills (`orchestrate-mission-waves`, `collaborate-as-agent-team-member`)
3. Add "superseded by docs/mental/agent-firm-mental-model.md" to affected ADRs
4. Run `node scripts/check-cross-layer-consistency.mjs`
5. Run `bash scripts/manage-star-harness-install.sh --apply`

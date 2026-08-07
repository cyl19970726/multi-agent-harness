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

**Agent Teams**: An independent unit of execution with a Host Agent and Members. Can be deployed on a specific machine. Flat topology — no nesting. Has optional `machine_id` (cross-machine awareness) and `labels` (filtering).

**Standing Agents**: Durable agent identities that persist across Team Runs. Not tied to any single execution. Examples: governance Agent auditing docs/works periodically, scheduled-task Agent running on a timer.

**Actors**: Four types — Human, Agent, External, Service. `ActorRef` (type + id) references a participant wherever needed.

---

## Execution

Execution answers "what work is being done right now."

**Missions**: Durable goals linking zero or more Agent Teams, persisting across multiple Team Runs.

**Agent Team Runs**: One execution instance with MemberRuns (runtime bindings), shared Work board, message inbox. Existing execution model — Work state machine, Message delivery, Daemon supervision — unchanged.

**Work**: The responsibility kernel. Title, context, criteria, owner (AgentMember), team scope, status (open→in_progress→review→done). Optional `document_refs` (links to Docs) and `labels` (filtering). Work is NOT being merged with Company WorkItem — the existing model stays.

**Views**: All Execution visible on one page. Filters by Agent Team, status, date range. Tags on Work entries. Per-team views unchanged.

---

## Knowledge (Docs)

Knowledge answers "what does the Company know."

Documents are Company memory — structured content, typed records, views. Agent-operated (create, edit, govern), Human-reviewed (inspect, approve). Documents link to Work via `document_refs`, connecting "what we know" with "what we're doing."

---

## Cross-Machine Communication

Agent Teams on different machines. Future requirement — design task, not implementation yet. Will provide message protocol, cross-machine work assignment, team discovery. The `machine_id` field on Agent Teams is the foundation.

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
| Organization — Agent Teams | ✅ PR #385 | machine_id + labels |
| Organization — Standing Agents | ❌ Not started | Design only |
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

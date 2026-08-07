# Agent Firm Mental Model

```text
status: canonical — single source of truth
owner_role: product
canonical_for: product architecture, Agent Team operations, Company OS
replaces: ADR 0027 section on "two primary systems", ADR 0042 section on "three independent identities"
```

This is the authoritative mental model. When any other document (ADR, skill, product spec, code comment) contradicts this document, this document wins. Update this document first, then cascade changes to downstream docs.

---

## Company

Company is the top-level entity. Everything belongs to a Company.

A user runs one Company at a time. Multi-company is a future concern; when it arrives, a Company selector appears in the dashboard. Until then, there is exactly one Company and it is implicit — no selection required.

Company has three facets:

- **Organization** — who the Company is made of
- **Execution** — what the Company is doing right now
- **Knowledge** — what the Company knows

These are not separate systems with separate databases. They are three views into the same Company, connected through optional references on Work records.

---

## Organization

Organization answers "who exists and how are they organized."

### Agent Teams

An Agent Team is an independent unit of execution. It has a Host Agent, Members, and can be deployed on a specific machine. Agent Teams do NOT nest — the topology is flat.

Each Agent Team can optionally declare a `machine_id` (for cross-machine awareness) and `labels` (for filtering).

### Standing Agents

A Standing Agent is a durable agent identity that persists across Team Runs. Unlike a MemberRun which is a runtime binding to a specific execution, a Standing Agent is a permanent role in the Organization. Examples: a governance Agent that periodically audits docs and works, a scheduled-task Agent that runs on a timer.

Standing Agents own their lifecycle. They are not tied to any single Team Run.

### Actors

Four actor types exist: Human, Agent, External, Service. An `ActorRef` (type + id) is used wherever a participant needs to be referenced — Work assignee, approval authority, document author.

---

## Execution

Execution answers "what work is being done right now."

### Missions

A Mission is a durable goal. It links zero or more Agent Teams and persists across multiple Team Runs.

### Agent Team Runs

An Agent Team Run is one execution instance. It contains MemberRuns (runtime bindings to provider sessions), a shared Work board, and a message inbox. Team Runs complete or cancel; Members idle, stop, or fail.

The existing Agent Team execution model (Work state machine, Message delivery, Daemon supervision) remains unchanged.

### Work

Work is the responsibility kernel. Each Work has:

- A title, context, completion criteria
- An owner (AgentMember)
- A team scope (which Agent Team it belongs to)
- A status: open → in_progress → review → done (or blocked, cancelled)
- Optional references: `document_refs` (links to Docs), `labels` (for filtering)

Work is NOT being merged with Company WorkItem. The existing Work model stays. The only addition is optional document and label references.

### Views

All Execution is visible on one page. Filters: by Agent Team, by status, by date range. Tags on Work entries. Each Agent Team also has its own per-team view (existing Team War Room, unchanged).

---

## Knowledge (Docs)

Knowledge answers "what does the Company know."

Documents are the Company's memory. They support structured content, typed records, and views. Documents are Agent-operated (agents create, edit, govern) and Human-reviewed (humans inspect and approve).

Documents can be linked from Work records via `document_refs`. This creates a natural connection between "what we know" and "what we're doing."

---

## Cross-Machine Communication

Agent Teams can be deployed on different machines. Cross-machine communication is a future requirement. When implemented, it will provide:

1. A message protocol for Agent Teams on different machines to exchange messages
2. Work assignment across machines
3. Agent Team discovery and health monitoring

This is a design task, not an implementation task yet. The `machine_id` field on Agent Teams is the foundation.

---

## Host Agent Responsibilities

The Host Agent (Lead) of each Agent Team:

1. Receives work assignments and delegates to Members
2. Reviews submitted work and accepts or requests changes
3. Manages resources — worktrees are space resources, Host resolves conflicts
4. Creates new work when supply is low (idle members should always have ready work)
5. Recovers from member failures (close + reopen, or reassign work)

---

## Current Implementation State

| Component | Status | Notes |
|---|---|---|
| Agent Team execution (work, messages, daemon) | ✅ Live | Full lifecycle working |
| Organization — Agent Teams flat list | ✅ PR #385 | Frontend views, machine_id + labels fields |
| Organization — Standing Agents | ❌ Not started | Design only |
| Work → Docs links | ❌ Not started | Optional document_refs field |
| Work → labels / tags | ❌ Not started | Filter + tag UI |
| Docs system (CRUD, views) | ⚠️ Partial | PR #386, needs audit |
| Cross-machine communication | ❌ Not started | Design task for future wave |
| Company selector (multi-company) | ❌ Future | Not needed yet |

---

## Document Governance

This document lives in `docs/mental/` — the canonical directory for mental model documents. When making changes:

1. Update this document first
2. Propagate changes to affected skills (`orchestrate-mission-waves`, `collaborate-as-agent-team-member`)
3. Update affected ADRs with a "superseded by docs/mental/agent-firm-mental-model.md" note
4. Run `node scripts/check-cross-layer-consistency.mjs` to verify skills match code
5. Run `bash scripts/manage-star-harness-install.sh --apply` to push to all platforms

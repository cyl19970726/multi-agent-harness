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

**Work**: The responsibility kernel. Title, context, criteria, owner (AgentMember), team scope, status (open→in_progress→review→done). Optional `document_refs` (links to Docs) and `labels` (filtering).

Work creation answers five questions: WHERE, WHAT, HOW, WHO.

- **WHERE** — `workspace`: Where the member works. Three kinds: `worktree` (isolated git checkout, for code), `dir` (plain directory, for exploration), `inherit` (project root, for read-only). Harness creates before member start, cleans up on completion. CLI: `--worktree <path>`.
- **HOW** — `gates`: Declarative verification gates that must pass before acceptance. Plugin names are non-empty and configs are JSON objects; old wire rows that omit config normalize to `{}`. Four typed built-ins are trusted by default: `github-pr`, `code-review`, `artifact-exists`, and `check-pass`. Custom declarations may persist, but default evaluation and Store acceptance fail unknown plugins closed; only an embedder with an explicit custom registry can evaluate one. Exact duplicate GateSpecs and more than one `code-review` Gate are rejected. `code-review.strategy` is required and limited to `peer | self | host`; omitting the whole Gate is the only no-review form. Candidate-bound Reviews persist performer attribution and authority separately; Host reviews always use fixed `Host/host` authority, so CLI `--actor` and HTTP `actor_id` cannot impersonate the reviewer. Artifact/check Gates match exact current-candidate refs only: they do not inspect files, prove truth, or rerun checks. Legacy unbound Reviews remain readable but cannot satisfy a Gate. `space migrate-from-project` fully preflights and validates source Reviews, strips untrusted binding fields, validates again, and fails before target writes on any invalid row. Migration uses same-parent staging, full source/stage verification, a target backup, and the shared registry/`ACTIVE_SPACE` lock; activation failure restores target and pointer snapshots. Post-commit backup cleanup failure is success with warning plus best-effort `cleanup_pending` manifest state and must not trigger a whole-migration retry. Empty Gates preserve manual Host accept compatibility. Store-managed `accept_work` enforces declared Gates and exposes no waiver flag. CLI: `--gate <plugin>[:key=val,...]`, `work check-gates`, `work accept`.
- **WHO** — `owner_member_id`, `assignee`.

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
| Work — gates | ✅ Live | Open persistence/closed default registry, four built-ins, authority-bound Review, Store acceptance invariant |
| Work — workspace | ✅ Live | PR #406 — WorkWorkspace, ensure/cleanup, --worktree CLI |
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

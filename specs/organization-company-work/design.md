# Organization + Company Work Model — Design

```text
status: superseded-by-issue-420
owner_role: product-architecture
target_spec: specs/organization-company-work/design.md
references:
  - docs/current/company-os/organization-and-actors.md
  - docs/current/company-os/nested-agent-team-organization.md
  - docs/decisions/0052-nested-agent-teams-are-the-agent-organization.md
  - specs/nested-agent-team-organization/requirements.md
  - specs/nested-agent-team-organization/design.md
```

> Historical design input only. The implemented #420 contract is documented in
> `docs/current/company-os/work-operating-system.md` and
> `docs/current/product/agent-team-works.md`. Existing Company task data is
> disposable: there is no migration, compatibility read, fallback, or dual write.

## 1. Purpose

This document designs the unified **Organization** and **Company Work** model that
collapses the current dual-system complexity (StandingAgent + AgentMember,
Company WorkItem + Team Work) into one minimal administrative kernel.  The model
is a concrete implementation bridge between ADR 0052's target contract and the
current repository truth.

## 2. Mental Model

```
Company
  └── Organization
        ├── AgentTeam "platform"      (host: alice-agent)
        │     ├── AgentMember bob-agent
        │     └── AgentMember carol-agent  ── (hosts child AgentTeam)
        │           └── AgentTeam "frontend"  (host: carol-agent, parent: platform)
        │                 ├── AgentMember dave-agent
        │                 └── AgentMember eve-agent
        ├── AgentTeam "infra"         (host: frank-agent, cross-machine)
        │     └── AgentMember grace-agent
        └── AgentTeam "docs"          (host: heidi-agent)
        ...
  └── Company Work (unified kernel, filterable by team)
```

- **Company** is the top-level administrative container.  It owns the Organization
  and the global Work pool.
- **Organization** is the durable projection of AgentTeams, their recursive
  topology, and their AgentMembers.  It is not a separate scheduler or actor
  store — it is AgentTeam topology rendered as a company-wide view.
- **AgentTeams** are independent administrative units.  A parent-child relation
  is a delegation edge, not a containment in the Company layer.  Teams may run on
  different machines (cross-machine).
- **AgentMember** is the single durable organization-agent identity.  Each
  Member belongs to exactly one parent Team and may optionally host one child
  Team.
- **Company Work** is the single Work kernel.  Every Work row is scoped to one
  AgentTeam.  Views aggregate across teams without dual-writing responsibility.

The canonical recursive topology is defined by ADR 0052 and
`docs/current/company-os/nested-agent-team-organization.md`.  This design adds the
Company-level projection, cross-machine awareness, and the unified Work kernel's
global views.

## 3. Organization Model

### 3.1 AgentTeam

`AgentTeam` is the unit of local administration.  The existing struct
(`crates/harness-core/src/lib.rs:454`) already carries the ADR 0052 fields:

```rust
pub struct AgentTeam {
    pub id: String,
    pub name: String,
    pub description: String,
    pub owner_agent_id: String,           // compatibility; converging to host_member_id
    pub status: AgentTeamStatus,           // Active | Closed | Archived
    pub member_ids: Vec<String>,           // DurableAgentMember ids
    pub parent_team_id: Option<String>,    // Recursive Organization (ADR 0052)
    pub host_member_id: Option<String>,    // DurableAgentMember that Hosts this team
    pub created_at: String,
    pub updated_at: String,
}
```

**Organization-layer additions:**

| Field | Type | Purpose |
|-------|------|---------|
| `company_id` | `Option<String>` | Owning Company.  `None` = unbound execution-only Team |
| `machine_id` | `Option<String>` | Machine where this Team's Supervisor runs.  `None` = default/unknown |
| `labels` | `Vec<String>` | User-defined tags for grouping and filtering |

**Topology invariants:**
1. The root Team has no `parent_team_id`.
2. A child Team's `host_member_id` must be a direct member of its `parent_team_id`.
3. The graph is acyclic (enforced by `validate_agent_team_topology()`).
4. V1: at most one child Team per hosting Member.
5. No Member may administer siblings, ancestors, or unrelated Teams.

**Cross-machine:**
- Teams on different machines communicate through the durable Store layer and
  the HTTP/MCP API surface.
- A Machine locator record (keyed by `machine_id`) maps to a loopback service
  address so that cross-machine Work delivery and Message routing work.

### 3.2 DurableAgentMember

`DurableAgentMember` (`crates/harness-core/src/lib.rs:488`) is the single
organization-agent identity.  The existing struct is already the target:

```rust
pub struct DurableAgentMember {
    pub id: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub provider_profile: Option<String>,
    pub model: Option<String>,
    pub workspace_policy: Option<String>,
    pub project_binding_id: Option<String>,
    pub business_access_ceiling_refs: Vec<String>,
    pub status: DurableAgentMemberStatus,   // Active | Paused | Retired
    pub created_by_member_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
```

**Organization-layer additions (non-invasive — extend the JSONL row, add
optional fields):**

| Field | Type | Purpose |
|-------|------|---------|
| `hosted_team_id` | `Option<String>` | Child Team this Member hosts, if any |
| `human_sponsor_ref` | `Option<ActorRef>` | Human accountable for this agent's business scope |

**Key invariant:** `DurableAgentMember` carries zero runtime state.
`MemberRun`, provider-native session, writable workspace, and process handle
are execution bindings.  Restart, crash, resume, Close, Reopen, or provider
replacement never creates a new `DurableAgentMember`.

### 3.3 ActorRef

All durable Company records reference actors through `ActorRef`:

```rust
pub struct ActorRef {
    pub actor_type: ActorType,  // Human | Agent | External | Service
    pub actor_id: String,
}
```

| Actor | Durable identity | Responsibility boundary |
|-------|-----------------|------------------------|
| Human | `HumanMember` (Company OS) | May own Work; mandatory for legal, financial, credential, irreversible effects |
| Agent | `DurableAgentMember` | May own Work, belong to a parent Team, host a child Team |
| External | `ExternalParticipant` (Company OS) | Time- and scope-bounded visibility/Work only |
| Service | `ServiceActor` (Company OS) | Declared automation; never impersonates Human or Agent |

### 3.4 Organization Projection

The Organization view is built read-only from the Store:

```
Organization =
  SELECT AgentTeam.*,
         host: DurableAgentMember WHERE host_member_id = AgentTeam.host_member_id,
         members: [DurableAgentMember WHERE id IN AgentTeam.member_ids],
         child_teams: [AgentTeam WHERE parent_team_id = AgentTeam.id]
  FROM AgentTeam
  ORDER BY parent_team_id ASC NULLS FIRST
```

No separate Organization table.  No dual-write.  UI constructs the tree from
explicit `parent_team_id` and `host_member_id` edges.

### 3.5 Migration from StandingAgent

The `StandingAgent` record and its `execution_agent_member_ref` join are
compatibility data marked for convergence.  Migration path:

1. `harness org member converge` creates a `DurableAgentMember` row for each
   `StandingAgent` that has an `execution_agent_member_ref`.
2. `harness org cutover-audit` validates: every Host is a `DurableAgentMember`,
   no dangling `owner_agent_id` references, no conflicting identities.
3. After audit passes, `StandingAgent` rows are exported and archived.  New code
   reads only `DurableAgentMember`.
4. `OrganizationMembership` and `OrgUnit` remain readable as business-grouping
   views but are not the scheduling authority.

## 4. Company Work Model

### 4.1 Unified Work Kernel

A single `Work` struct replaces both `WorkItem` (Company OS) and `Work`
(Agent Team).  The existing Team `Work` struct
(`crates/harness-core/src/lib.rs:3278`) becomes the canonical kernel.
Company OS concerns extend it through optional relations, not a second
lifecycle.

```rust
pub struct Work {
    // ── Core identity ──
    pub id: String,
    pub team_id: Option<String>,             // Durable AgentTeam scope (ADR 0052)
    pub team_run_id: String,                 // Current execution attempt
    pub parent_work_id: Option<String>,      // Delegation hierarchy

    // ── Content ──
    pub title: String,
    pub context_markdown: String,
    pub completion_criteria_markdown: String,

    // ── Status & ownership ──
    pub status: WorkStatus,                  // Open | InProgress | Blocked | Review | Done | Cancelled
    pub owner_member_id: Option<String>,     // → DurableAgentMember (null = unassigned)
    pub active_member_run_id: Option<String>,
    pub claim_mode: WorkClaimMode,           // HostAssign | TeamClaim
    pub eligible_member_ids: Vec<String>,
    pub priority: WorkPriority,              // Low | Normal | High | Urgent

    // ── Provenance ──
    pub created_by_actor: TeamActorRef,
    pub created_by_member_id: Option<String>,

    // ── Outcome ──
    pub result_summary: Option<String>,
    pub blocker_reason: Option<String>,
    pub artifact_refs: Vec<String>,
    pub check_refs: Vec<String>,

    // ── Company OS extensions (new fields) ──
    pub business_module_ref: Option<String>, // → BusinessModule (Company OS)
    pub milestone_ref: Option<String>,       // → Milestone (Company OS)
    pub document_refs: Vec<String>,          // → Documents (Company OS)
    pub approval_refs: Vec<String>,          // → Approvals (Company OS)
    pub finance_refs: Vec<String>,           // → Commitments/Payments (Company OS)
    pub source_observation_ref: Option<String>, // Provenance: which Work/Doc/runtime fact created this

    // ── Metadata ──
    pub version: u64,
    pub created_at: String,
    pub updated_at: String,
    pub due_at: Option<String>,
}
```

**Key difference from WorkItem:** The Company OS `WorkItem` carried
`accountable_owner`, `assignees`, `contributors`, `reviewer`, `approver` as
separate fields.  The unified `Work` has a single `owner_member_id` (the
accountable AgentMember).  Human and multi-actor assignment remains possible
through optional `document_refs` and relationship extensions but is not a
first-class Work kernel field.

### 4.2 Work Status Lifecycle

```
                  ┌──────────┐
          ┌──────►│ Cancelled │ (terminal)
          │       └──────────┘
          │
  ┌───────┴──┐     ┌────────────┐     ┌────────────┐     ┌────────┐     ┌──────┐
  │   Open   │────►│ InProgress │────►│   Review   │────►│  Done  │     │ Done │
  └──────────┘     └─────┬──────┘     └─────┬──────┘     └────────┘     └──────┘
                         │                  │                               ▲
                         │   ┌──────────┐   │                               │
                         └──►│ Blocked  │───┘                               │
                             └──────────┘                                   │
                                  │  (resume)                               │
                                  └─────────────────────────────────────────┘
```

Assigned vs unassigned is derived from `owner_member_id.is_some()`, not a
separate state.  This is already the Team Work semantics.

### 4.3 Creation and Assignment

| Actor | May create | May assign | May accept |
|-------|-----------|-----------|-----------|
| Supervising Operator | Unassigned Work in any Team | No | No |
| Ordinary Member | Unassigned or self-owned Work in its current Team; child Work under Work it owns | Self only | No peer Work |
| Team Host | Any Work in its direct Team | Direct members or unassigned pool | Work in its direct Team |
| Child Team Host | Any Work in its child Team | Direct child members | Child Team Work |

Every Member is a continuous Work-discovery node.  While executing, reviewing,
reading Docs, or observing runtime facts, it may create new Work with source
provenance.  The topology rules above constrain placement.

### 4.4 Delegation

Delegation creates child Work; it does not transfer the parent's promise.

```
Host assigns W0 to CTO in root Team
  → CTO creates child Team
  → CTO creates W1, W2, W3 with parent_work_id = W0
  → child Members execute and submit W1/W2/W3
  → CTO accepts or requests changes
  → CTO integrates results and submits W0 to Host
  → Host accepts or requests changes on W0
```

A child completion never auto-completes its parent.  The parent assignee remains
accountable for integration, conflicts, evidence, and the result returned upward.

### 4.5 WorkEvent, WorkDelivery, HostAttention

These remain unchanged from the current Team Work implementation:

- **WorkEvent** — append-only authority log.  Each event carries
  `expected_version` / `resulting_version` for optimistic concurrency.
  Kinds: `Created`, `Assigned`, `Claimed`, `Started`, `Blocked`, `Resumed`,
  `Submitted`, `ChangesRequested`, `Accepted`, `Cancelled`, `Updated`,
  `Rebound`, `TeamScopePromoted`, `ExecutionRetargeted`.

- **WorkDelivery** — outbox for member runtimes.  Statuses: `Queued`,
  `Claimed`, `ProviderReceived`, `Failed`, `Invalidated`.  The Runtime
  Supervisor claims and injects deliveries according to busy/idle/recovery state.

- **HostAttention** — Work-state → Host notification bridge.  Kinds:
  `WorkReviewRequested`, `WorkBlocked`, `MemberStoppedWithOwnedReadyWork`,
  `MemberFailedWithOwnedReadyWork`.

### 4.6 Cutover policy

The implemented cutover deletes the obsolete Company task ledger and bridge.
Historical rows are not migrated, interpreted, exported, or dual-written.

### 4.7 Cross-Machine Work

A Work row's `team_id` links it to an `AgentTeam` that may run on a different
machine.  Cross-machine delivery:

1. The Host creates a Work assigned to a Member on a remote Team.
2. The local Supervisor creates a `WorkDelivery` and posts it to the remote
   machine's HTTP API (`POST /v1/team-runs/:id/deliveries`).
3. The remote Supervisor claims, processes, and reports back.
4. The Host polls or receives SSE events for status changes.

Message routing follows the same pattern: `TeamMessage` with a remote recipient
is posted to the remote machine's inbox endpoint.

## 5. View Design

### 5.1 Global Works View ("Company Work")

The entry point for cross-Team Work visibility.  Reads all `Work` rows across
all Teams under the Company.

**Data source:**
```
SELECT Work.*,
       AgentTeam.name AS team_name,
       AgentTeam.parent_team_id,
       DurableAgentMember.name AS owner_name
FROM Work
LEFT JOIN AgentTeam ON Work.team_id = AgentTeam.id
LEFT JOIN DurableAgentMember ON Work.owner_member_id = DurableAgentMember.id
WHERE AgentTeam.company_id = <company_id>
```

**Filters:**
- By Team (team_id or team_name)
- By status (Open, InProgress, Blocked, Review, Done, Cancelled)
- By owner (AgentMember id)
- By priority (Low, Normal, High, Urgent)
- By source observation (discovered-unassigned, self-owned, delegated, follow-up)
- By business module, milestone, document

**Display columns:**
- Title, Team, Owner, Status, Priority, Created, Due

**Demand classes** (from `teamWorksSelectors.ts`):
- `unassigned` — no owner, Host must triage
- `owned` — has an owner
- `follow-up` — created from another Work's completion/review
- `discovered` — created from continuous observation

### 5.2 Per-Team Works View

Reuses the existing Team War Room Works board
(`apps/agent-dashboard/src/TeamWorks.tsx`).  This view is already implemented
and needs no redesign — it reads `snapshot.works` filtered by `team_id`.

### 5.3 Organization Tree View

Already partially implemented in `AgentTeamOrganization.tsx`
(`apps/agent-dashboard/src/AgentTeamOrganization.tsx`).  Renders the recursive
Team tree from `AgentTeam.parent_team_id` + `host_member_id` edges.

**Enhancements:**
1. **Cross-machine indicator** — badge showing which machine a Team runs on.
2. **Work counts per Team** — aggregate of `Work` rows by status (open, in_progress, blocked, review).
3. **Member roster** — list of `DurableAgentMember` per Team with runtime status.
4. **Host label** — clear visual distinction between Host and ordinary Members.

### 5.4 Member Focus

Renders a single `DurableAgentMember`:
- Current Team, role, status
- Hosted child Team (if any)
- Owned Work (from global Work pool, filtered by `owner_member_id`)
- Created Work (from global Work pool, filtered by `created_by_member_id`)
- Runtime state (from latest `MemberRun`)

### 5.5 Dashboard Snapshot API

The existing `GET /v1/snapshot` endpoint is extended:

```json
{
  "organization": {
    "teams": [...],                     // existing AgentTeam rows
    "durable_members": [...],           // existing DurableAgentMember rows
    "works": [...],                     // unified Work rows (was snapshot.works)
    "team_runs": [...],                 // existing TeamRun rows
    "member_runs": [...],               // existing MemberRun rows
    "org_tree": {                       // NEW: pre-built tree projection
      "team_id": "...",
      "team_name": "...",
      "host": { "member_id": "...", "name": "...", "status": "..." },
      "members": [...],
      "child_teams": [...],
      "work_counts": { "open": 3, "in_progress": 5, "blocked": 1, "review": 2 }
    }
  }
}
```

## 6. API Draft

### 6.1 CLI Commands

#### Organization

```bash
# Team management (existing, extended)
harness team create --name <name> --description <desc> \
  --lead <durable-member-id> \
  [--parent-team <id>] [--host-member <id>] \
  [--company <id>] [--machine <id>]

harness team list [--company <id>] [--machine <id>] [--all]
harness team show --id <id> [--tree]            # --tree shows recursive children
harness team rename --id <id> --name <name>
harness team add-member --id <id> --member <durable-member-id>
harness team remove-member --id <id> --member <durable-member-id>
harness team close --id <id>
harness team archive --id <id>

# Durable member management (existing)
harness org member create --id <id> --name <name> --role <role> \
  [--provider-profile <p>] [--model <m>] [--workspace-policy <p>]
harness org member converge --id <id> [--name|--role|...]
harness org member list
harness org member show --id <id>
harness org bootstrap-lead --team <id>
harness org host --team <id>
harness org cutover-audit

# Company binding (new)
harness company bind-team --company <id> --team <id>
harness company unbind-team --team <id>

# Machine registry (new)
harness machine register --id <id> --address <url>
harness machine list
harness machine show --id <id>
```

#### Company Work

```bash
# Work CRUD (existing team-run work, extended with company-scoped operations)
harness work list \
  [--company <id>] \              # scope to Company
  [--team <id>] \                 # scope to Team
  [--status <s>]* \
  [--owner <member-id>] \
  [--priority <p>] \
  [--milestone <id>] \
  [--module <id>] \
  [--due-before <iso>] \
  [--since <cursor>] \
  [--brief]

harness work show --id <id> [--with-events] [--with-deliveries]

harness work create \
  --team-run-id <id> \            # current execution attempt
  --title <t> \
  --completion-criteria <c> \
  [--context <md>] \
  [--team <id>] \                 # durable team scope (ADR 0052)
  [--owner-member-id <id>] \
  [--parent-work-id <id>] \
  [--claim-mode <host_assign|team_claim>] \
  [--priority <low|normal|high|urgent>] \
  [--milestone <id>] \
  [--module <id>] \
  [--due-at <iso>]

# Work lifecycle (existing, unchanged)
harness work assign --id <id> --version <n> --member-id <id>
harness work claim --id <id> --version <n> --member-run-id <id>
harness work start --id <id> --version <n> --member-run-id <id>
harness work block --id <id> --version <n> [--reason <s>]
harness work resume --id <id> --version <n>
harness work submit --id <id> --version <n> [--result-summary <s>] [--artifact-ref <r>]*
harness work request-changes --id <id> --version <n> [--reason <s>]
harness work accept --id <id> --version <n> [--summary <s>]
harness work cancel --id <id> --version <n>

```

### 6.2 HTTP API

#### Organization

```
GET  /v1/organization                       → full org tree + work counts
GET  /v1/organization/teams                 → list all teams
GET  /v1/organization/teams/:id             → single team + members + child teams
GET  /v1/organization/members               → list all durable members
GET  /v1/organization/members/:id           → single member + owned work + runtime
GET  /v1/organization/machines              → machine registry
POST /v1/organization/teams                 → create team
POST /v1/organization/teams/:id/members     → add member to team
POST /v1/organization/members               → create durable member
POST /v1/organization/machines              → register machine
```

#### Company Work

```
GET  /v1/works                               → global work list (filterable)
GET  /v1/works/:id                           → single work + events + deliveries
POST /v1/works                               → create work
POST /v1/works/:id/assign                    → assign work
POST /v1/works/:id/claim                     → claim work
POST /v1/works/:id/start                     → start work
POST /v1/works/:id/submit                    → submit work
POST /v1/works/:id/accept                    → accept work
POST /v1/works/:id/request-changes           → request changes
POST /v1/works/:id/cancel                    → cancel work
POST /v1/works/:id/block                     → block work
POST /v1/works/:id/resume                    → resume work

# Cross-machine delivery
POST /v1/team-runs/:id/deliveries            → accept remote WorkDelivery
POST /v1/team-runs/:id/messages              → accept remote TeamMessage
```

### 6.3 Query Parameters for Work List

| Parameter | Type | Description |
|-----------|------|-------------|
| `company_id` | string | Filter by Company |
| `team_id` | string | Filter by Team |
| `status` | string[] | Filter by status |
| `owner_member_id` | string | Filter by accountable owner |
| `priority` | string | Filter by priority |
| `milestone_id` | string | Filter by milestone |
| `module_id` | string | Filter by business module |
| `parent_work_id` | string | Filter children of a parent Work |
| `demand_class` | string | unassigned / owned / follow-up / discovered |
| `due_before` | ISO8601 | Filter by due date |
| `since` | cursor | Pagination cursor |
| `limit` | int | Page size (default 50) |
| `brief` | bool | Return summary fields only |

## 7. Store Layout

No new ledger files.  Extend existing ledgers:

| Ledger | Content | Change |
|--------|---------|--------|
| `teams.jsonl` | `AgentTeam` rows | Add `company_id`, `machine_id`, `labels` fields |
| `durable_agent_members.jsonl` | `DurableAgentMember` rows | Add `hosted_team_id`, `human_sponsor_ref` fields |
| `work_operations.jsonl` | `WorkOperation` rows | Add `business_module_ref`, `milestone_ref`, `document_refs`, `approval_refs`, `finance_refs`, `source_observation_ref`, `due_at` fields to `Work` |
| `company_os_standing_agents.jsonl` | Compatibility `StandingAgent` rows | Frozen after convergence; archived after audit |

New optional ledgers:

| Ledger | Content | Purpose |
|--------|---------|---------|
| `machines.jsonl` | Machine registry rows | Cross-machine routing |

## 8. Acceptance Criteria

1. `AgentTeam` rows carry `company_id` and `machine_id`; cross-machine routing
   works through the machine registry.
2. `DurableAgentMember` is the single agent identity; `StandingAgent` rows are
   converged and archived.
3. Unified `Work` rows are authoritative; obsolete Company task rows are discarded.
4. `GET /v1/organization` returns the full recursive Team tree with work counts.
5. `GET /v1/works` filters by team, status, owner, priority, milestone, module,
   and demand class.
6. Per-Team Works view reuses existing Team War Room without modification.
7. Global Works view shows all Work across all Teams under a Company.
8. A real dogfood run exercises Lead → child Team delegation, cross-machine
    Work delivery, and the global Works view.

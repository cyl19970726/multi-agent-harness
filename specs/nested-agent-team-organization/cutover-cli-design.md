# ADR 0052 Cutover CLI Design

```text
status: proposed technical design
owner_role: product-architecture
parent: specs/nested-agent-team-organization/design.md
dependencies:
  - docs/decisions/0052-nested-agent-teams-are-the-agent-organization.md
  - docs/current/company-os/nested-agent-team-organization.md
  - docs/current/company-os/work-operating-system.md (Lane A mapping: 10-state Company WorkItem ↔ 6-state Team Work kernel)
```

## Overview

This document specifies the complete CLI design for ADR 0052's AgentMember-based
Organization cutover. The first slice (`main.rs` ~L8980) implements CLI-only
identity operations against `durable_agent_members.jsonl` and `teams.jsonl`.
This design covers the full surface including command signatures, state
transitions, store impact, cutover-audit report shape, the Lane A
WorkItem→Work kernel mapping reference, and a Wave-2 implementation
breakdown. No Rust implementation is produced in this wave — only the design
document.

The CLI surface, as stated in the product document
`docs/current/company-os/nested-agent-team-organization.md` L46-48, spans four
top-level commands with four `member` subcommands:

```
harness org member create|converge|list|show
harness org bootstrap-lead
harness org host
harness org cutover-audit
```

For completeness, this document covers all seven subcommand signatures.

## Command Signatures

### 1. `harness org member create`

Create a new durable AgentMember identity. The member is written to
`durable_agent_members.jsonl` with latest-row-wins semantics.

```
harness org member create
  --name <name>                  (required)
  --description <description>    (required)
  --role <role>                  (required)
  [--id <id>]                    (auto-generated if omitted)
  [--provider-profile <profile>]
  [--model <model>]
  [--workspace-policy <policy>]
  [--project-binding <project-id>]
  [--business-access-ceiling <ref>]...   (repeatable)
  [--status active|paused|retired]       (default: active)
  [--created-by-member <member-id>]
```

**State transition:** (none) → Active | Paused | Retired

**Store impact:**

| Ledger | Operation |
|--------|-----------|
| `durable_agent_members.jsonl` | append |

**Errors:**
- Missing `--name`, `--description`, or `--role`
- Invalid `--status` value (expected `active|paused|retired`)

**Output:** JSON `DurableAgentMember` object.

**Implementation note (current slice):** Already implemented in
`build_durable_member_from_args()` → `store.insert_durable_member()`.

---

### 2. `harness org member converge`

Converge a compatibility `AgentMember` from `members.jsonl` into a
`DurableAgentMember`. The source row is read from the compatibility ledger;
the resulting durable row is written to `durable_agent_members.jsonl`.
Re-running convergence is deterministic — `created_at` and `updated_at`
come from the source row, not wall-clock time.

```
harness org member converge
  --id <id>                      (required; compatibility AgentMember id)
  [--project-binding <project-id>]
  [--business-access-ceiling <ref>]...   (repeatable)
  [--created-by-member <member-id>]
```

**Status mapping (compatibility → durable):**

| Source `AgentMemberStatus` | Target `DurableAgentMemberStatus` |
|----------------------------|-----------------------------------|
| `Retired`                  | `Retired`                         |
| `Paused`                   | `Paused`                          |
| `Stale`                    | `Paused`                          |
| `Closed`                   | `Paused`                          |
| `Closing`                  | `Paused`                          |
| `Active`, `Starting`, `Idle`, `Busy`, `Offline` | `Active`   |

**State transition:** compatibility `AgentMemberStatus` →
`DurableAgentMemberStatus` (see table)

**Store impact:**

| Ledger | Operation |
|--------|-----------|
| `members.jsonl`                | read  |
| `durable_agent_members.jsonl`  | write |

**Errors:**
- Compatibility `AgentMember` not found for `--id`

**Output:** JSON `DurableAgentMember` object.

**Implementation note (current slice):** Already implemented in
`converge_durable_member()` → `store.converge_registry_member()`.

---

### 3. `harness org member list`

List all durable AgentMembers (latest-row-wins projection).

```
harness org member list
```

**State transition:** read-only

**Store impact:**

| Ledger | Operation |
|--------|-----------|
| `durable_agent_members.jsonl` | read |

**Output:** JSON array of `DurableAgentMember` objects.

**Future extension candidates (not in this design):**
- `--status active|paused|retired` filter
- `--team <team-id>` filter (members reachable from a team subtree)
- `--page` / `--limit` pagination for large organizations

**Implementation note (current slice):** Already implemented in
`store.latest_durable_members()`.

---

### 4. `harness org member show`

Show a single durable AgentMember by id.

```
harness org member show
  --id <id>                      (required)
```

**State transition:** read-only

**Store impact:**

| Ledger | Operation |
|--------|-----------|
| `durable_agent_members.jsonl` | read |

**Errors:**
- Durable AgentMember not found for `--id`

**Output:** JSON `DurableAgentMember` object.

**Implementation note (current slice):** Already implemented.

---

### 5. `harness org bootstrap-lead`

Create a durable Host AgentMember and a root AgentTeam in one atomic
operation. The team gets `parent_team_id = null` and the member's id becomes
the team's `host_member_id`.

```
harness org bootstrap-lead
  --team <team-id>               (required)
  --name <name>                  (required)
  --description <description>    (required if member does not already exist)
  --role <role>                  (required if member does not already exist)
  [--provider-profile <profile>]
  [--model <model>]
  [--workspace-policy <policy>]
  [--project-binding <project-id>]
```

**State transition:** Creates one `DurableAgentMember` (Active) + one
`AgentTeam` (Active, root).

**Store impact:**

| Ledger | Operation |
|--------|-----------|
| `durable_agent_members.jsonl` | write |
| `teams.jsonl`                 | write |

**Errors:**
- Missing `--team` or `--name`
- Team id already exists
- Member missing required fields when not reusing an existing identity

**Output:** `{ "member": DurableAgentMember, "team": AgentTeam }`

**Implementation note (current slice):** Already implemented in
`store.bootstrap_root_lead_member()`.

---

### 6. `harness org host`

Resolve the Host authority for an AgentTeam. Returns the authoritative
Host member id and the resolution source.

```
harness org host
  --team <team-id>               (required)
  | --team-id <team-id>          (alias)
```

**State transition:** read-only

**Store impact:**

| Ledger | Operation |
|--------|-----------|
| `teams.jsonl` | read |

**Resolution logic (already implemented in `resolve_team_host_authority`):**

1. If `team.host_member_id` is set → return it with source `"explicit"`.
2. Otherwise, fall back to `team.owner_agent_id` if set → return it with
   source `"owner_agent_id_compatibility"`.
3. Otherwise → `HostAuthorityError::Missing`.

**Output:**
```json
{
  "team_id": "<team-id>",
  "host_member_id": "<member-id>",
  "source": "explicit" | "owner_agent_id_compatibility"
}
```

**Errors:**
- AgentTeam not found

---

### 7. `harness org cutover-audit`

Validate the current organization topology and Host-authority state. This is
the identity-side audit — it does NOT validate Work cutover (that lives in
`/v1/company-os/work-cutover` with its separate `WorkCutoverReport`).

```
harness org cutover-audit
```

**State transition:** read-only validation

**Store impact:**

| Ledger | Operation |
|--------|-----------|
| `teams.jsonl`                  | read |
| `durable_agent_members.jsonl`  | read |
| `members.jsonl`                | read (future: validate all StandingAgent have been converged) |

**Validation steps (already implemented as two functions):**

1. `validate_agent_team_topology(&teams)` — enforces:
   - No cycles in the `parent_team_id` graph
   - Host member exists in the parent team's `member_ids` (non-root teams)
   - A member does not host more than one primary child team (V1)
   - Every `host_member_id` references a known durable member

2. `validate_host_authority_cutover(&teams, &members)` — enforces:
   - Every team has a resolvable Host authority
   - No team has conflicting `owner_agent_id` and `host_member_id`
   - Every durable host member exists

**Output:**
```json
{
  "ready": true,
  "team_count": <n>,
  "durable_member_count": <n>,
  "authority": "host_member_id",
  "legacy_owner_is_alias_only": true
}
```

On failure, the command returns a `HostAuthorityError` message before
reaching JSON output.

**Implementation note (current slice):** Already implemented. The
function validates topology and authority but does not yet check that all
compatibility `AgentMember` rows have been converged. The `issues` expansion
is a Wave-2 follow-up.

**Wave-2 extension candidates:**
- Per-team validation status in output (not just pass/fail)
- List of unconverged compatibility members
- List of teams with compatibility-only `owner_agent_id`

---

## State Transitions Summary

### DurableAgentMemberStatus

```
(none)
  ├──→ Active   (create, converge)
  ├──→ Paused   (create)
  └──→ Retired  (create, converge)

Active
  ├──→ Paused   (future: `org member update --status paused`)
  └──→ Retired  (future: `org member update --status retired`)

Paused
  ├──→ Active   (future: `org member update --status active`)
  └──→ Retired  (future: `org member update --status retired`)

Retired → (terminal — no further transitions)
```

The `org member update` command is not in the current CLI slice; it is a
Wave-2 addition. The `create` and `converge` commands set the initial status;
no in-place mutation exists yet.

### AgentTeam host authority resolution

```
AgentTeam.host_member_id == Some(id)
  → source: "explicit"

AgentTeam.host_member_id == None && AgentTeam.owner_agent_id == Some(id)
  → source: "owner_agent_id_compatibility"

AgentTeam.host_member_id == None && AgentTeam.owner_agent_id == None
  → HostAuthorityError::Missing
```

---

## Store Impact Matrix

| Command | `durable_agent_members.jsonl` | `teams.jsonl` | `members.jsonl` |
|---------|-------------------------------|---------------|-----------------|
| `member create`     | write (append) | —       | —     |
| `member converge`   | write (append) | —       | read  |
| `member list`       | read           | —       | —     |
| `member show`       | read           | —       | —     |
| `bootstrap-lead`    | write (append) | write   | —     |
| `host`              | —              | read    | —     |
| `cutover-audit`     | read           | read    | read (future) |

`members.jsonl` is the compatibility `AgentMember` ledger. After full
cutover, convergence will be complete and `members.jsonl` becomes
read-only history.

---

## Lane A Mapping Reference

The 10-state Company WorkItem ↔ 6-state Team Work kernel mapping from
PR #326 (`docs/current/company-os/work-operating-system.md`) is the prerequisite
for this design. It defines:

- **Company WorkItem states** (10): `Draft`, `Open`, `InProgress`, `Blocked`,
  `InReview`, `Done`, `Cancelled`, `Deferred`, `Archived`, `OnHold`
- **Team Work kernel states** (6): `open`, `in_progress`, `blocked`,
  `review`, `done`, `cancelled`
- **Cutover authority rule:** A source-linked Team Work is accepted only
  after the corresponding Company WorkItem is no longer live (closed,
  cancelled, deferred, or archived). Active Company WorkItem rows must be
  converged or resolved before the unified Work kernel becomes the sole
  responsibility authority.

The mapping is stored in the Company Docs module and is referenced — not
duplicated — by this design. The `work_cutover_report` function in
`harness-store` and `validate_work_cutover_with_fences` in `harness-core`
already implement the fence-based concurrency-safe validation.

---

## Existing Implementation Surface

### CLI (`crates/harness-cli/src/main.rs`)

All seven subcommands are implemented as a first slice (~150 lines at L8984-L9131).
The implementation uses `value()`, `required()`, `many()` argument parsers and
direct `HarnessStore` calls. There is no separate application-service layer
yet — the CLI is the only consumer of the organization store operations.

### API (`crates/harness-cli/src/company_os_api.rs`)

- `GET /v1/company-os/snapshot` includes `durable_agent_members` in the
  execution-space projection (L285-288)
- `GET /v1/company-os/work-cutover` returns a `WorkCutoverReport` covering
  the Work-kernel side of the cutover (L132-143), distinct from the
  identity-side `org cutover-audit`

### Store (`crates/harness-store/src/lib.rs`)

- `insert_durable_member()` — append to `durable_agent_members.jsonl`
- `converge_registry_member()` — convergence write
- `latest_durable_members()` — latest-row-wins BTreeMap projection
- `bootstrap_root_lead_member()` — create lead member + root team
- `work_cutover_report()` — cross-store cutover validation with fences

### Core (`crates/harness-core/src/lib.rs`)

- `DurableAgentMember` struct (L488-508) — 11 fields
- `DurableAgentMemberStatus` enum — `Active | Paused | Retired`
- `HostAuthorityError` enum — `Missing | Conflicting`
- `WorkCutoverReport` struct (L3463-3471) — `valid`, counts, `issues[]`
- `validate_agent_team_topology()` — acyclic, direct-host, one-child invariants
- `validate_host_authority_cutover()` — team/host consistency

---

## Wave-2 Implementation Breakdown

### Phase 1: CLI hardening (~2 tasks)

**1a. Parameter validation and error messages**
- Audit all `required()` / `value()` chains for consistent error messages
- Add `--help` output for each subcommand
- Validate that `--business-access-ceiling` refs are well-formed
- Ensure `--project-binding` id references an existing project binding

**1b. `org member update` command**
- Add `--status active|paused|retired` mutation
- Enforce valid transitions (e.g., `Retired → *` is rejected)
- Add `--name`, `--description`, `--role` field updates
- Write new latest-row to `durable_agent_members.jsonl`

### Phase 2: Application service and HTTP/MCP API (~2 tasks)

**2a. Organization application service**
- Extract org operations from CLI into a shared application service
- Share the service between CLI, HTTP API, and future MCP plugin
- Ensure the service reads from the independently selected Execution Space

**2b. HTTP and MCP endpoints**
- `GET /v1/org/members` — list with optional status/team filters
- `GET /v1/org/members/{id}` — show single member
- `POST /v1/org/members` — create
- `POST /v1/org/members/{id}/converge` — converge from compatibility
- `PUT /v1/org/members/{id}` — update
- `POST /v1/org/bootstrap-lead` — bootstrap
- `GET /v1/org/host?team=<id>` — host resolution
- `GET /v1/org/cutover-audit` — audit (extended report)
- `GET /v1/org/tree` — recursive team tree

**2c. Cutover-audit response expansion**
- Extend `org cutover-audit` output to include per-team validation status
- Add `issues[]` with categorized problems: `missing_host`, `conflicting`,
  `unknown_member`, `compatibility_active`
- Add `unconverged_member_count` and per-member convergence status
- Keep the rich report shape compatible with Work-cutover `WorkCutoverReport`

### Phase 3: Dashboard integration (~2 tasks)

**3a. Organization overview page**
- Recursive team tree from `parent_team_id`, `host_member_id`, `member_ids`
- Per-node Work counts by status
- Runtime state shown separately from durable Agent status
- Drilldown from Member to child Team

**3b. Durable member management UI**
- Member list/search with status filters
- Member detail with provider/workspace policy, business access ceiling
- Create, converge, and update operations
- Bootstrap-lead wizard

### Phase 4: Cutover migration tooling (~2 tasks)

**4a. StandingAgent convergence audit and batch tool**
- Scan all compatibility `AgentMember` rows
- Report convergence status per member
- Batch `org member converge` for all unconverged members
- Dry-run mode that reports without writing

**4b. Full cutover verification suite**
- End-to-end test: init → converge all members → bootstrap-lead → audit pass
- Verify `host` resolution after bootstrap
- Verify `WorkCutoverReport.valid` after Work-kernel migration
- CI gate that prevents regression on cutover invariants

### Phase 5: Acceptance and documentation (~1 task)

**5a. End-to-end acceptance tests**
- `harness org member create` → `list` → `show` round-trip
- `harness org member converge` from a known compatibility member
- `harness org bootstrap-lead` → `host` → `cutover-audit` chain
- Rejection of duplicate team id, missing member, invalid status
- Full JSON schema validation against `DurableAgentMember` and output shapes

---

## Design Decisions

### Why seven subcommands, not six

The task description says "6 CLI commands" but lists seven tokens. The
product document groups them as four top-level commands (`org member`,
`org bootstrap-lead`, `org host`, `org cutover-audit`). This design covers
all seven subcommand signatures because each has a distinct implementation.

### Why org member update is deferred to Wave 2

The current slice intentionally provides create and converge only. Update
requires status-transition validation and field-level mutation that would
add scope without changing the identity foundation. It is a natural Phase 1b
addition.

### Why the cutover-audit response stays minimal in Wave 1

The current `cutover-audit` returns a boolean `ready` with counts. Expanding
it to a detailed per-team report with categorized issues is valuable but
requires schema design and backward-compatibility planning. Phase 2c adds
this without blocking the identity foundation.

### Why org tree is a dashboard concern, not a CLI command

Recursive tree projection requires joining teams, members, and Work counts.
It is a read projection best served by the HTTP API and rendered by the
Dashboard. A CLI `org tree` could be added later if needed, but it is not
in the current scope.

---

## References

- [ADR 0052](../decisions/0052-nested-agent-teams-are-the-agent-organization.md)
- [Nested Agent Team Organization (product doc)](../../docs/current/company-os/nested-agent-team-organization.md)
- [Nested Agent Team Organization (technical design)](./design.md)
- [Work Operating System (Lane A mapping)](../../docs/current/company-os/work-operating-system.md)
- [Implementation CLI reference](../../crates/harness-cli/src/main.rs) — `org_command()` at L9050, `durable_member_status()` at L8984
- [Store reference](../../crates/harness-store/src/lib.rs) — `work_cutover_report()` at L4163, durable member ops
- [Core types](../../crates/harness-core/src/lib.rs) — `DurableAgentMember` at L488, `WorkCutoverReport` at L3463
- [API reference](../../crates/harness-cli/src/company_os_api.rs) — snapshot at L285, work-cutover at L132

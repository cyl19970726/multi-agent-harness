# Wanchengwanling Company Store Migration

```text
status: copied, exact-row verified, and Docs relation backfill complete
date: 2026-07-29
company_store_id: agent-company
source_compat_project_id: new-day-wanchengwanling
canonical_for: moving Wanchengwanling Company OS dogfood rows from project-derived compatibility Store into an ADR 0042 Company Store
```

## Boundary

This migration copies Company OS truth only:

```text
copied: company_os_*.jsonl
not copied: Mission/Wave, Agent Team, Workflow, provider sessions, prompts, runtimes
dual write: false
destructive delete: false
```

The source compatibility Store remains available for audit:

```text
/Users/hhh0x/.harness/projects/new-day-wanchengwanling
```

The target Company Store is:

```text
/Users/hhh0x/.harness/companies/agent-company
```

## Command used

```bash
harness company migrate-from-project \
  --from-project new-day-wanchengwanling \
  --id agent-company \
  --name "Agent Company Workspace"
```

## Result

```text
copied files: 22 company_os_*.jsonl ledgers
copied records: 3942
skipped identical files: 0
```

## Post-migration verification

Commands:

```bash
harness company migrate-from-project \
  --from-project new-day-wanchengwanling \
  --id agent-company \
  --verify-only
harness --company agent-company company migrations
harness --company agent-company company docs health
harness --company agent-company company work list
harness --company agent-company company org list
```

The 2026-07-29 exact-row verification recorded:

```text
source ledgers: 22
source records: 3942
target records: 4396
missing exact source records: 0
execution ledgers in target: 0
```

The target append-only migration manifest is
`company_store_migrations.jsonl`. The source compatibility Store now contains
`COMPANY_OS_MIGRATED_TO_COMPANY.json`, which recommends read-only audit access
without claiming filesystem enforcement. The source is not deleted and dual
write remains disabled.

Observed latest Company Store counts after the relation backfill:

| Surface | Verified count |
| --- | --- |
| Docs | 13 Documents, 82 Blocks, 98 TypedRecords, 99 Relations, 11 BusinessModules; health `pass`, 0 findings |
| Work | 41 submitted WorkItems in board projection |
| Organization | 13 Actors, 3 OrgUnits, 13 Memberships |

No execution ledgers were present in the Company Store after migration:

```text
missions.jsonl: absent
waves.jsonl: absent
team_runs.jsonl: absent
provider_sessions.jsonl: absent
```

## Docs relation closeout

The pre-closeout `company docs health` report contained 34
`missing_document_record_relation` warnings. They were not lost TypedRecords:
the source Documents and records existed, but older seed/source-sync paths had
not appended their required `source_for` Relations.

The deterministic repair plan is:

```bash
harness --company agent-company company docs relation repair-missing \
  --definition page-wcw-software-product-sources \
  --actor agent-wcw-docs-governance \
  --dry-run
# 30 unique Relations

harness --company agent-company company docs relation repair-missing \
  --definition page-wcw-ip-product-design \
  --actor agent-wcw-docs-governance \
  --dry-run
# 3 unique Relations

harness --company agent-company company docs relation repair-missing \
  --definition page-wcw-launch-readiness \
  --actor agent-wcw-docs-governance \
  --dry-run
# 1 unique Relation
```

The confirmed governed dispatch appended 30 + 3 + 1 Relations. A second dry-run
for every definition planned zero writes, and `company docs health` returned
`pass` with zero findings. The repaired `source sync` and Wanchengwanling seed
paths now append the source Relation with each TypedRecord so this warning class
does not recur.

## Follow-up

The dashboard/API now understand Company Store selection:

- `GET /v1/companies`
- `GET /v1/companies/current`
- `POST /v1/companies/switch`
- `GET /v1/company-os/snapshot?company=agent-company`
- `GET /v1/snapshot?company=agent-company`

`/v1/snapshot` remains a blended read: execution keys come from the selected
Execution Space, provider execution context comes from the selected Project
Binding, and `company_os` comes from the selected Company Store. The migrated
`agent-company` store can be selected as the dogfood truth source for
Wanchengwanling and AgentOS operating areas.

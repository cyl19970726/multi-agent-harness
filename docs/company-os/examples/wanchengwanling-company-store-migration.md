# Wanchengwanling Company Store Migration

```text
status: verified local migration evidence
date: 2026-07-28
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
harness --company agent-company company docs health
harness --company agent-company company work list
harness --company agent-company company org list
```

Observed counts:

| Surface | Verified count |
| --- | --- |
| Docs | 12 Documents, 62 Blocks, 98 TypedRecords, 65 Relations, 11 BusinessModules |
| Work | 41 submitted WorkItems in board projection |
| Organization | 13 Actors, 3 OrgUnits, 13 Memberships |

No execution ledgers were present in the Company Store after migration:

```text
missions.jsonl: absent
waves.jsonl: absent
team_runs.jsonl: absent
provider_sessions.jsonl: absent
```

## Follow-up

The dashboard/API now understand Company Store selection:

- `GET /v1/companies`
- `GET /v1/companies/current`
- `POST /v1/companies/switch`
- `GET /v1/company-os/snapshot?company=agent-company`
- `GET /v1/snapshot?company=agent-company`

`/v1/snapshot` remains a blended read: execution keys come from the selected
Project Store, while `company_os` comes from the selected Company Store. The
next slice should use the migrated `agent-company` store as the default dogfood
truth source for Wanchengwanling and AgentOS operating areas.

# Harness CLI Map

status: stable  
owner: lead-operations  
last reviewed: 2026-07-27

This map records the current `target/debug/harness` command surface. It separates
implemented CLI from API/store-backed capability that does not yet have a
dedicated CLI.

## Status labels

- **Implemented**: routed by `crates/harness-cli/src/main.rs` and callable from
  the compiled `harness` binary.
- **Partial**: supported by store/API/UI/scripts, but the CLI is incomplete or
  only covers metadata/control slices.
- **Missing / next**: expected product surface with no dedicated CLI command yet.
- **Retired compatibility**: retained only to export or verify historical data.

## Top-level command map

| Area | Commands | Status | Notes |
| --- | --- | --- | --- |
| Execution Space / Project Binding routing | `init`, `space init/list/current/switch/show/migrate-from-project`, `project add/list/current/switch/remove/show/migrate` | Implemented | `--space` selects Mission/Wave/Agent Team/Workflow storage; `--project` independently selects provider cwd, instructions/Skills, Git/worktree and permission boundaries. Raw store overrides and project-derived execution stores are compatibility paths only. |
| Company Store routing | `company init/list/current/switch/show/migrate-from-project`, global `--company <id>` for `company ...`, `HARNESS_COMPANY` | Implemented | ADR 0042 Phase 2 first slice. `harness company ...` uses the selected Company Store when explicit/current Company exists; execution commands still use Project routing. |
| Mission | `mission create/list/show/update-context/create-team/link-team/unlink-team/close` | Implemented | Current durable intent surface. |
| Wave | `wave create/list/show/history/update/advance/gate` | Implemented | Lightweight host plan/judgment record. |
| Agent Team definition | `team create/list/show/rename/add-member/remove-member/close/archive` | Implemented | Defines reusable teams independent of Mission/Wave. |
| Agent Team run | `team-run create/list/status/inbox/host-inbox/bind-host/ack/reconcile-delivery/add-member/rename-member/deactivate-member/close-member/start/send/resolve-interaction/events/complete/cancel` | Implemented | Runtime control plane for persistent MemberRuns and typed mail. `--member-owned-path name:path` adds enforced write scope to reusable `--agent-team-id` members without dropping `agent_member_id`. |
| Agent Team member detail | `member-run show --id <member-run-id> [--json]`; `member-run open-native --id <member-run-id> [--print-only] [--json]` | Implemented | Joins one MemberRun, TeamRun, Assignment, Inbox/Outbox, actions, PendingInteractions, latest Handoff, Workspace and provider-native session locator without copying the provider transcript. `open-native` is provider-neutral fail-closed routing; the currently verified UI target is explicit Claude Agent SDK → Claude Desktop import on macOS. |
| Members/providers | `member register/list/providers` | Implemented | Provider compatibility review is exposed through `member providers`. |
| Standing Agent runtime | `agent create/list/show/start/health/send/route-inbox/deliver/retry-delivery/reconcile-delivery/gateway/close` | Implemented | Current standing-agent operational CLI. `route-inbox` idempotently joins stable Agent-addressed mail to one active AgentTeam MemberRun through `AgentMessageRoute`; it does not collapse Agent identity, Team participation, or Company OS authority. |
| Dynamic Workflow | `workflow list/run/run-script/get-output/patch/gc-worktrees/reap-workers/reap` | Implemented | WorkflowRun/WorkflowStep remain their own execution truth. |
| Dashboard | `dashboard snapshot` | Implemented | Produces operator projection. |
| Serve/API | `serve [--addr] [--once]`, `mcp`, `daemon start/status/stop`, `hook record` | Implemented | Local HTTP/API/MCP/daemon surfaces. |
| Historical migration | `legacy-goal-task export`, `legacy-goal-task verify` | Retired compatibility | Export/verify only. Current planning must use Mission/Wave. |
| Retired command families | old `goal`, `phase`, `task`, proposal/review/design surfaces | Retired compatibility | These fail explicitly and must not be used for new work. |

## Company OS CLI map

### Company Store

`harness company init/list/current/switch/show` manages the explicit Company
Store identity introduced by ADR 0042.

| Capability | Commands | Status | Notes |
| --- | --- | --- | --- |
| Registry | `company init --id <company-id> [--name <name>]`, `company list`, `company current`, `company show [company-id]`, `company switch <company-id>` | Implemented | Stores live under `<HARNESS_HOME>/companies/<id>/`; `ACTIVE_COMPANY` and `companies/registry.json` track the current Company. |
| Company OS routing | `harness --company <id> company ...`, `HARNESS_COMPANY=<id> harness company ...`, or active Company from `company switch/init` | Implemented | Applies only to `harness company ...`; Mission/Wave, Agent Team, Workflow, provider cwd, and Project selection remain separate. |
| Migration from project-derived stores | `company migrate-from-project --from-project <project-id\|path> --id <company-id> [--name <name>] [--force]`, `company migrate-from-project ... --verify-only`, `company migrations` | Implemented | Copies only `company_os_*.jsonl`, verifies every exact source row exists in the destination, appends a Company Store migration record, and writes an advisory source marker. `--verify-only` rechecks an existing destination without copying. The source remains audit evidence; no Mission/Wave, Agent Team, Workflow, provider session, prompt, or runtime ledger is copied. |

### Docs

`harness company docs ...` is the most complete Company OS CLI surface.

| Capability | Commands | Status | Notes |
| --- | --- | --- | --- |
| Read/projection | `query`, `search`, `traverse`, `refs`, `related`, `snapshot`, `diff`, `change-report`, `health` | Implemented | Agent-readable and human-readable projections over native Docs records. |
| Source sync | `source sync` | Implemented | Syncs external source state into Docs TypedRecords and idempotent `Document → source_for → TypedRecord` Relations with explicit boundaries. GitHub webhook transport is still a separate future adapter. |
| Module setup | `module create` | Implemented | Creates BusinessModule plus default/fallback View through governed API path. |
| Custom page metadata | `page-definition create`, `page scaffold`, `page verify`, `page publish` | Partial | Defines/scaffolds/verifies/publishes custom-page records and refs. It does not yet generate a complete production page from an arbitrary business brief. |
| Document lifecycle | `document create`, `document rename`, `document move`, `document archive` | Implemented | Structure maintenance exists. Archive requires confirmation; no physical delete. |
| Template lifecycle | `template create`, `template status` | Implemented | Template records exist; full template version approval workflow is still missing. |
| Blocks | `block append`, `block update`, `block archive`, `block remove`, `block reorder` | Implemented | Agent-first document editing primitives. Drag/drop editor is not the priority path. |
| Typed records | `typed-record append`, `typed-record update`, `typed-record validate` | Implemented | Core structured memory primitive. |
| Views | `view create`, `view update` | Partial | Basic view records exist. Advanced view editing, calendar/chart views and complex field configuration are still missing. |
| Relations | `relation link`, `relation unlink`, `relation relink`, `relation repair-missing` | Implemented | Native relation maintenance across Docs and adjacent systems. `repair-missing` is definition/module-scoped, dry-run-first, confirmation-gated, and idempotent. |

### Work

`harness company work ...` exists and is the second real Company OS CLI surface.

| Capability | Commands | Status | Notes |
| --- | --- | --- | --- |
| Work read | `work list`, `work query` | Implemented | Supports filtered WorkItem projections. |
| Intake | `work create` | Implemented | Creates WorkItem through governed Action dispatch. Requires source document, definition, owner, submitter and objective. |
| Metadata / responsibility correction | `work update` | Implemented | Governed `work_item.update` for description, acceptance, context, source, module/business line, WorkType, owner, assignees, reviewer/approver, priority, due date and risk. It cannot change lifecycle status or result/evidence/execution provenance. |
| Assignment delivery | `work assign` | Implemented | Appends Assignment delivery record. It proves routing/delivery but does not rewrite `WorkItem.assignees`; use `work update` when the Work projection itself must show a changed responsibility chain. |
| Lifecycle | `work transition`, `work close` | Implemented | Updates WorkItem status/provenance through governed Action dispatch. |
| Milestone management | `work milestone list`, `show`, `create`, `update`, `close` | Implemented | Uses native Milestone rows. Writes currently use Human-admin administrative governance because global Work milestone Actions are not yet modeled. |
| WorkType/business-line management | `work update --work-type ... --module ...` | Partial | WorkItems can now be reclassified against native WorkType/module fields. Dedicated catalogs for WorkType/business-line governance remain planned. |
| Approval request/decision | `harness company approval request`, `decide`, `list`, `show` | Implemented | Approval is a shared Company OS CLI group, not nested under Work. Requests/decisions dispatch governed Actions. |

### Organization

| Capability | Current surface | Status | Notes |
| --- | --- | --- | --- |
| Actors | `org actor list`, `show`, `create-human`, `create-agent`, `update-status` | Implemented | Native Human and Standing Agent authoring is available. Writes use Human-admin administrative governance. |
| Org units | `org unit list`, `show`, `create`, `update-status` | Implemented | Native OrgUnit authoring is available. |
| Membership/reporting | `org membership list`, `assign`, `update-status` | Implemented | Native Membership assignment is available. Move/retire are represented as status updates for now. |
| Execution relation | `org link-execution --authority <human> --actor <standing-agent> --agent-member <id> --execution-space <id> [--replace]`, `org unlink-execution --authority <human> --actor <standing-agent> [--expect-agent-member <id>]` (also as `org actor link-execution`/`unlink-execution`) | Implemented | Links an EXISTING StandingAgent to an EXISTING AgentMember. Both ids are explicit; equal ids never bind implicitly. `--execution-space` is required and has no fallback because `harness company ...` resolves the Company Store and never reaches the `--space` selector (ADR 0042); the named space is opened read-only to validate the AgentMember. Only `execution_agent_member_ref` and `updated_at` change; the write is a governed administrative re-append, never a raw JSONL edit. Re-running the same pair is a no-op (`changed:false`), and repointing requires `--replace`. `--authority` is validated as an active Human `company_os.admin` on every invocation, including no-ops. |
| Permissions/capabilities | actor create/update-status fields | Partial | Create commands can write permission/capability refs. Dedicated grant/revoke/proposal workflow is still missing. |
| HR / business-agent lifecycle | `create-agent`, `membership assign`, `update-status` plus skill/docs contract | Partial | Basic lifecycle exists. Proposal/approval/promotion workflow remains next. |

### Finance

| Capability | Current surface | Status | Notes |
| --- | --- | --- | --- |
| Commitments | `finance commitment list`, `show`, `propose`, `transition` | Implemented | `propose` is R2; `transition` uses `commitment.append` and enforces existing approval policy. |
| Payments | `finance payment list`, `show`, `record` | Implemented | `record` creates evidence-backed prepared Payments through governed `payment.append`; settlement transition remains a future command. |
| Approval-linked spend | `company approval ...` + `company finance ...` | Implemented | Approval request/decision can be linked to Commitment and Payment commands. |
| Budget/invoice/refund/reporting | docs/skill contract only | Missing / next | Need first-class finance CLI and acceptance tests. |

### External gateways

| Capability | Current surface | Status | Notes |
| --- | --- | --- | --- |
| Social platform readiness | `company gateway social readiness [--platform xiaohongshu|douyin|wechat_channels] [--adb adb] [--device <serial>]` | Implemented / read-only | Observes Android package/focus readiness for Xiaohongshu, Douyin, and WeChat Channels slots. It returns data suitable for `social_platform_account` records but does not write Store truth, log in, publish, delete, pay, or export messages. |
| GitHub source observation | `company docs source sync` | Implemented for local worktree observation | Writes external software source TypedRecords; GitHub webhook/API delivery remains next. |
| WeCom merchant gateway | docs/WorkItems only | Planned | Needs schema/API/CLI/Agent inbox implementation. |
| Social publication / metric evidence | Docs TypedRecords + WorkItems now; dedicated connector commands next | Partial | Current Store can model accounts, campaigns, post plans, publications, and metric snapshots. Dedicated publish/metrics commands must remain policy-gated. |

### Company OS API resources with no equivalent dedicated CLI

The local API can read/write more resources than the CLI exposes directly:

- `action-policies`
- `action-commands`
- `audit-events`

These should not all become raw append commands. The preferred next step is
ergonomic governed commands that preserve ownership boundaries:

- raw action policy / command / audit inspection helpers
- future budget / invoice / refund resources

## Skills and install status

Repository skills exist for the intended Company OS operator roles:

- `skills/company-docs-operator`
- `skills/company-work-operator`
- `skills/company-org-operator`
- `skills/company-finance-operator`
- `skills/company-module-designer`
- `skills/company-page-builder`
- `skills/company-business-project-bootstrap`

The skill suite is checked by `scripts/check-company-os-skill-suite.mjs` and
`acceptance:skill-install`. Skills are distribution/operator guidance. They are
not the authority for product architecture; canonical truth remains schemas,
store/API, CLI routing, UI, tests and ADRs.

## Current validation commands

| Check | What it proves |
| --- | --- |
| `pnpm check:company-os` | Docs CLI smoke, Work CLI smoke, skill suite, Company OS UI/runtime/navigation/trademark checks and docs governance. |
| `pnpm acceptance:company-os:docs-cli` | Store-live Docs CLI can create/update/query native Docs records. |
| `node scripts/check-company-os-work-cli-smoke.mjs` | Store-live Work CLI can create/assign/transition/close WorkItems. |
| `node scripts/check-company-os-operator-cli-smoke.mjs` | Store-live Org, Milestone, Approval, Commitment and Payment operator CLI commands work together. |
| `pnpm acceptance:mission-wave` | Deterministic Mission/Wave, TeamRun, MCP and dashboard contracts. |
| `harness governance check` | Documentation registry/link/retired-surface governance. |

## Recommended CLI roadmap

1. **Org governance workflow**: add proposal/approval/promotion commands for
   permission grant/revoke, agent creation, reporting changes and retirement.
2. **Finance lifecycle depth**: add explicit payment transition/settlement,
   budget, invoice, refund and finance report commands.
3. **Work taxonomy helpers**: add WorkType/business-line helpers when they
   become first-class records instead of fields.
4. **Custom page CLI**: promote page metadata into a full page-build loop:
   design contract -> component scaffold -> store-live fixture -> screenshot
   capture -> compare -> publish.
5. **External source CLI**: add GitHub/webhook adapter commands that update
   Docs source projections without letting GitHub become commercial truth.
6. **SQL read-model CLI**: when ADR 0035 is implemented, add rebuild/status/query
   commands for the derived read/index layer. JSONL ledgers remain canonical.
